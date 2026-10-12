//! Forwarding signals on to other systems.
//!
//! mcpd can copy what it stores to any number of destinations at once: another
//! OTLP collector, another mcpd, or a vendor such as Langfuse, Honeycomb or
//! Datadog. Each destination has its own filter, its own redaction and its own
//! spool, and is sent the endpoint, headers and attribute conventions its
//! vendor documents (see `config` for the presets and `mapping` for the
//! conventions).
//!
//! Forwarding **never blocks or fails ingest**. A signal is stored first and
//! copied second: the copy is filtered, mapped and encoded for each
//! destination, written to that destination's spool, and sent from there by
//! the destination's own workers. A destination that is slow or down fills
//! its own spool and nothing else; the local write, the other destinations and
//! the sender's request are all unaffected.
//!
//! **Retries are bounded by time and space, not by attempts.** An entry is
//! retried with backoff and jitter, honouring `Retry-After`, until it is
//! accepted, refused for good, or older than a day. A refused or expired entry
//! is moved to the spool's `dead/` directory and counted; a copy that finds
//! the spool full is dropped and counted. Both counts are per destination in
//! `/metrics`, beside each destination's spool depth and whether its last
//! attempt succeeded.

pub mod config;
pub mod deliver;
pub mod helicone;
pub mod mapping;
pub mod model;

pub use config::{
    Conventions, Destination, Filter, Protocol, Redact, Resolved, Settings, Signals, Vendor,
    DEFAULT_SPOOL_MAX_BYTES,
};
pub use model::Signal;

use crate::spool::{self, PushError, Spool};
use deliver::Outcome;
use mcpd_core::{LogRecord, MetricPoint, Span};
use serde_json::json;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The name earlier releases gave a destination.
pub type Forwarder = Destination;

/// The most ingest requests whose copies may be waiting to be written to the
/// spools at once.
///
/// One request's copy is every signal it stored, so this bounds the memory a
/// burst of ingest can hold before the copies reach disk. Past it a copy is
/// dropped at once and counted in `mcpd_forward_dropped_spans_total`, and the
/// ingest itself still succeeds.
pub const MAX_IN_FLIGHT: usize = 64;

/// How old an entry may get before it is given up on. A destination down for
/// longer than this is a destination nobody is reading, and holding a day of
/// its backlog forever would fill the disk for nothing.
pub const MAX_AGE: Duration = Duration::from_secs(24 * 3600);

/// Workers per destination. Two, so one slow request does not stall the
/// queue behind it, and few enough that a destination that has just recovered
/// is not met with every waiting batch at once.
const WORKERS: usize = 2;

/// What one destination has done since the process started.
#[derive(Default)]
pub struct Health {
    /// Items accepted.
    pub sent: AtomicU64,
    /// Requests accepted.
    pub requests: AtomicU64,
    /// Attempts that will be retried.
    pub failed_attempts: AtomicU64,
    /// Items refused for good or expired, and moved to `dead/`.
    pub dead_letter: AtomicU64,
    /// Items dropped because the spool was full.
    pub spool_full: AtomicU64,
    /// Items the filter kept from this destination.
    pub filtered: AtomicU64,
    /// Failures since the last success.
    pub consecutive_failures: AtomicU64,
    /// No worker claims before this, in milliseconds since the epoch. Set by
    /// a retry, so a `Retry-After` holds every worker of the destination and
    /// not only the one that read it.
    pub hold_until_ms: AtomicI64,
    /// When the last request was accepted, seconds since the epoch, or zero.
    pub last_success: AtomicI64,
    /// What the last failure was, without any URL or credential.
    pub last_error: Mutex<Option<String>>,
}

/// One destination at run time: what it resolved to, its spool, its health.
pub struct Sink {
    /// The destination as configured, for its filter, redaction and batch size.
    pub destination: Destination,
    /// The destination with its preset applied.
    pub resolved: Resolved,
    /// Work waiting to be sent.
    pub spool: Spool,
    /// What it has done.
    pub health: Health,
}

impl Sink {
    fn record_error(&self, why: &str) {
        *self.health.last_error.lock().unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some(why.to_owned());
    }

    /// The last failure, if any.
    #[must_use]
    pub fn last_error(&self) -> Option<String> {
        self.health.last_error.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }

    /// Whether the last attempt succeeded, or none has been made.
    #[must_use]
    pub fn up(&self) -> bool {
        self.health.consecutive_failures.load(Ordering::Relaxed) == 0
    }
}

/// Every destination this process sends to.
pub struct Exporters {
    sinks: Vec<Arc<Sink>>,
    started: AtomicBool,
}

impl Exporters {
    /// Open every enabled destination's spool.
    ///
    /// A destination whose preset does not resolve is left out and logged.
    /// The daemon refuses to start on that before it gets here; this is the
    /// line for an embedder or a test that built a configuration some other
    /// way. A spool directory that cannot be opened falls back to memory, for
    /// the same reason, and says so.
    #[must_use]
    pub fn open(settings: &Settings) -> Self {
        let mut sinks = Vec::new();
        for d in settings.enabled() {
            let resolved = match d.resolve() {
                Ok(r) => r,
                Err(e) => {
                    tracing::error!("{e}; the destination is left out");
                    continue;
                }
            };
            let max = d.spool_max_bytes.unwrap_or(settings.spool_max_bytes).max(1);
            let dir =
                settings.spool_dir.as_ref().map(|p| p.join(format!("forward-{}", resolved.name)));
            let spool = match Spool::open(dir.as_deref(), max) {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!(
                        destination = %resolved.name,
                        "the spool directory could not be opened ({e}); this destination's \
                         backlog is held in memory and is lost on restart"
                    );
                    Spool::open(None, max).unwrap_or_else(|_| unreachable!("memory never fails"))
                }
            };
            sinks.push(Arc::new(Sink {
                destination: d.clone(),
                resolved,
                spool,
                health: Health::default(),
            }));
        }
        Self { sinks, started: AtomicBool::new(false) }
    }

    /// The destinations.
    #[must_use]
    pub fn sinks(&self) -> &[Arc<Sink>] {
        &self.sinks
    }

    /// Whether there is anywhere to send.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sinks.is_empty()
    }

    /// Per-destination metrics, in Prometheus text format.
    #[must_use]
    pub fn render(&self) -> String {
        if self.sinks.is_empty() {
            return String::new();
        }
        let mut s = String::new();
        let mut family = |name: &str, help: &str, kind: &str, read: &dyn Fn(&Sink) -> String| {
            s.push_str(&format!("# HELP {name} {help}\n# TYPE {name} {kind}\n"));
            for sink in &self.sinks {
                s.push_str(&format!(
                    "{name}{{destination=\"{}\",vendor=\"{}\"}} {}\n",
                    sink.resolved.name,
                    sink.resolved.vendor.as_str(),
                    read(sink)
                ));
            }
        };
        let n = |a: &AtomicU64| a.load(Ordering::Relaxed).to_string();
        family("mcpd_forward_sent_total", "Items a destination accepted.", "counter", &|k| {
            n(&k.health.sent)
        });
        family(
            "mcpd_forward_failed_attempts_total",
            "Attempts a destination did not accept and that will be retried.",
            "counter",
            &|k| n(&k.health.failed_attempts),
        );
        family(
            "mcpd_forward_dead_letter_total",
            "Items refused for good or older than a day, moved to the spool's dead/ directory.",
            "counter",
            &|k| n(&k.health.dead_letter),
        );
        family(
            "mcpd_forward_spool_full_total",
            "Items dropped because the destination's spool was full.",
            "counter",
            &|k| n(&k.health.spool_full),
        );
        family(
            "mcpd_forward_filtered_total",
            "Items the destination's filter kept from it.",
            "counter",
            &|k| n(&k.health.filtered),
        );
        family(
            "mcpd_forward_spool_entries",
            "Requests waiting in the destination's spool.",
            "gauge",
            &|k| k.spool.len().to_string(),
        );
        family("mcpd_forward_spool_bytes", "Bytes the destination's spool holds.", "gauge", &|k| {
            k.spool.bytes().to_string()
        });
        family(
            "mcpd_forward_up",
            "1 when the destination's last attempt succeeded or none has been made, 0 when it \
             failed.",
            "gauge",
            &|k| u8::from(k.up()).to_string(),
        );
        family(
            "mcpd_forward_last_success_timestamp_seconds",
            "When the destination last accepted a request, in seconds since the epoch, or 0.",
            "gauge",
            &|k| k.health.last_success.load(Ordering::Relaxed).to_string(),
        );
        s
    }
}

/// The signals one ingest stored, copied for forwarding.
#[derive(Default)]
pub struct Copy {
    /// Spans.
    pub spans: Vec<Span>,
    /// Log records.
    pub logs: Vec<LogRecord>,
    /// Metric points the sender sent. Points the daemon derived for itself are
    /// not in here: they repeat what the spans already say, and a destination
    /// would count them twice.
    pub metrics: Vec<MetricPoint>,
}

impl Copy {
    fn is_empty(&self) -> bool {
        self.spans.is_empty() && self.logs.is_empty() && self.metrics.is_empty()
    }
}

/// Copy what a batch will store, when any destination wants it.
#[must_use]
pub fn copy_of(state: &crate::AppState, batch: &mcpd_store::Batch) -> Copy {
    if !wanted(state) {
        return Copy::default();
    }
    let wants = |signal| state.exporters.sinks.iter().any(|s| s.resolved.wants(signal));
    Copy {
        spans: if wants(Signal::Traces) { batch.spans.clone() } else { Vec::new() },
        logs: if wants(Signal::Logs) { batch.logs.clone() } else { Vec::new() },
        metrics: if wants(Signal::Metrics) { batch.metrics.clone() } else { Vec::new() },
    }
}

/// Whether anything is forwarded, so the ingest path can skip copying when
/// nothing would be sent.
#[must_use]
pub fn wanted(state: &crate::AppState) -> bool {
    !state.exporters.is_empty()
}

/// Write a copy to every destination's spool, on a task of its own.
///
/// On its own task, and only after the local write succeeded, so a slow disk
/// adds nothing to the caller's request and a signal is never forwarded that
/// mcpd failed to keep. Tracked, so shutdown waits for it. At most
/// [`MAX_IN_FLIGHT`] copies wait at once; past that one is dropped and counted.
pub fn dispatch(state: &Arc<crate::AppState>, copy: Copy) {
    if copy.is_empty() || !wanted(state) {
        return;
    }
    spawn(state);
    let Ok(slot) = Arc::clone(&state.forward_slots).try_acquire_owned() else {
        state.metrics.forward_dropped_spans.fetch_add(copy.spans.len() as u64, Ordering::Relaxed);
        warn_full(copy.spans.len());
        return;
    };
    let owned = Arc::clone(state);
    state.tasks.spawn(async move {
        let _slot = slot;
        let spans = copy.spans.len() as u64;
        let inner = Arc::clone(&owned);
        // Mapping and the synced write are blocking work; off the runtime's
        // threads, so a burst of ingest cannot stall request handling.
        let done = tokio::task::spawn_blocking(move || enqueue(&inner, &copy)).await;
        if done.is_err() {
            tracing::error!(
                "writing a forwarded copy to the spools panicked; the copy is counted in \
                 mcpd_forward_dropped_spans_total"
            );
            owned.metrics.forward_dropped_spans.fetch_add(spans, Ordering::Relaxed);
        }
    });
}

/// Filter, redact, map and spool one copy for every destination.
fn enqueue(state: &crate::AppState, copy: &Copy) {
    for sink in &state.exporters.sinks {
        let d = &sink.destination;
        let r = &sink.resolved;
        let batch = d.batch_size.max(1);
        if r.wants(Signal::Traces) && !copy.spans.is_empty() {
            let mut kept = mapping::filter_spans(&copy.spans, &d.filter);
            note_filtered(sink, copy.spans.len(), kept.len());
            kept.iter_mut().for_each(|s| mapping::redact_span(s, &d.redact));
            if r.protocol == Protocol::Api {
                let logs = helicone::logs(&kept, r);
                note_filtered(sink, kept.len(), logs.len());
                for body in logs {
                    push(
                        state,
                        sink,
                        Signal::Traces,
                        "application/json",
                        1,
                        body.to_string().as_bytes(),
                    );
                }
            } else {
                for chunk in kept.chunks(batch) {
                    let export = mapping::spans(chunk, r);
                    push_export(state, sink, &export);
                }
            }
        }
        if r.wants(Signal::Logs) && !copy.logs.is_empty() {
            let mut kept = mapping::filter_logs(&copy.logs, &d.filter);
            note_filtered(sink, copy.logs.len(), kept.len());
            kept.iter_mut().for_each(|l| mapping::redact_log(l, &d.redact));
            for chunk in kept.chunks(batch) {
                push_export(state, sink, &mapping::logs(chunk, r));
            }
        }
        if r.wants(Signal::Metrics) && !copy.metrics.is_empty() {
            let mut kept = mapping::filter_metrics(&copy.metrics, &d.filter);
            note_filtered(sink, copy.metrics.len(), kept.len());
            kept.iter_mut().for_each(|m| mapping::redact_metric(m, &d.redact));
            for chunk in kept.chunks(batch) {
                push_export(state, sink, &mapping::metrics(chunk, r));
            }
        }
    }
}

fn note_filtered(sink: &Sink, before: usize, after: usize) {
    if before > after {
        sink.health.filtered.fetch_add((before - after) as u64, Ordering::Relaxed);
    }
}

/// The content type and body one export is sent with.
#[must_use]
pub fn encode(export: &model::Export, protocol: Protocol) -> (&'static str, Vec<u8>) {
    match protocol {
        Protocol::Json | Protocol::Api => {
            ("application/json", export.to_json().to_string().into_bytes())
        }
        Protocol::Protobuf => ("application/x-protobuf", export.to_protobuf()),
        Protocol::Grpc => ("application/grpc", export.to_protobuf()),
    }
}

fn push_export(state: &crate::AppState, sink: &Sink, export: &model::Export) {
    if export.is_empty() {
        return;
    }
    let (ct, body) = encode(export, sink.resolved.protocol);
    let items = export.len();
    push(state, sink, export.signal(), ct, items, &body);
}

fn push(
    state: &crate::AppState,
    sink: &Sink,
    signal: Signal,
    content_type: &str,
    items: usize,
    body: &[u8],
) {
    let meta = json!({ "signal": signal.as_str(), "ct": content_type, "n": items });
    match sink.spool.push(&spool::frame(&meta, body)) {
        Ok(_) => {}
        Err(e) => {
            sink.health.spool_full.fetch_add(items as u64, Ordering::Relaxed);
            if signal == Signal::Traces {
                state.metrics.forward_dropped_spans.fetch_add(items as u64, Ordering::Relaxed);
            }
            let why = match e {
                PushError::Full => format!(
                    "the spool is full at {} bytes, so new copies are dropped until the \
                     destination catches up",
                    sink.spool.max_bytes()
                ),
                PushError::Io(e) => format!("the spool could not be written: {e}"),
            };
            sink.record_error(&why);
            tracing::warn!(destination = %sink.resolved.name, items, "{why}");
        }
    }
}

/// Start every destination's workers. Safe to call more than once: only the
/// first call starts anything. The daemon calls it at start, so a backlog a
/// previous process left on disk is sent without waiting for new traffic, and
/// [`dispatch`] calls it too, for an embedder that never did.
pub fn spawn(state: &Arc<crate::AppState>) {
    if state.exporters.sinks.is_empty() || state.exporters.started.swap(true, Ordering::AcqRel) {
        return;
    }
    for sink in &state.exporters.sinks {
        if !sink.spool.is_empty() {
            tracing::info!(
                destination = %sink.resolved.name,
                entries = sink.spool.len(),
                "sending the backlog the spool holds from before this start"
            );
        }
        for _ in 0..WORKERS {
            let (state, sink) = (Arc::clone(state), Arc::clone(sink));
            state.clone().tasks.spawn(async move { work(&state, &sink).await });
        }
    }
}

/// One worker: claim, send, settle, until the process stops.
async fn work(state: &Arc<crate::AppState>, sink: &Arc<Sink>) {
    let client = &state.outbound;
    loop {
        if state.shutdown.is_cancelled() {
            final_pass(state, sink).await;
            return;
        }
        let held = sink.health.hold_until_ms.load(Ordering::Relaxed) - now_ms();
        if held > 0 {
            tokio::select! {
                () = state.shutdown.cancelled() => {}
                () = tokio::time::sleep(Duration::from_millis(held.unsigned_abs())) => {}
            }
            continue;
        }
        let Some(entry) = sink.spool.claim() else {
            tokio::select! {
                () = state.shutdown.cancelled() => {}
                () = sink.spool.ready() => {}
                () = tokio::time::sleep(Duration::from_secs(1)) => {}
            }
            continue;
        };
        match settle(state, sink, client, &entry).await {
            None => {}
            Some(wait) => {
                tokio::select! {
                    () = state.shutdown.cancelled() => {}
                    () = tokio::time::sleep(wait) => {}
                }
            }
        }
    }
}

/// One attempt at one entry, and its bookkeeping. Returns how long to wait
/// before the next claim when the attempt is to be retried.
async fn settle(
    state: &crate::AppState,
    sink: &Sink,
    client: &reqwest::Client,
    entry: &spool::Claimed,
) -> Option<Duration> {
    let name = &sink.resolved.name;
    let Some((meta, body)) = spool::unframe(&entry.bytes) else {
        sink.spool.bury(entry.id);
        return None;
    };
    let items = meta["n"].as_u64().unwrap_or(0);
    let is_traces = meta["signal"] == "traces";
    let age = time::OffsetDateTime::now_utc().unix_timestamp() - entry.created;
    if u64::try_from(age).unwrap_or(0) > MAX_AGE.as_secs() {
        sink.spool.bury(entry.id);
        sink.health.dead_letter.fetch_add(items, Ordering::Relaxed);
        if is_traces {
            state.metrics.forward_dropped_spans.fetch_add(items, Ordering::Relaxed);
        }
        tracing::warn!(
            destination = %name,
            items,
            "a forwarded batch waited longer than a day without being accepted, so it is \
             moved to dead/ and counted in mcpd_forward_dead_letter_total"
        );
        return None;
    }
    let signal = match meta["signal"].as_str() {
        Some("logs") => Signal::Logs,
        Some("metrics") => Signal::Metrics,
        _ => Signal::Traces,
    };
    let url = sink.resolved.url(signal);
    let outcome = match sink.resolved.protocol {
        Protocol::Grpc => {
            deliver::post_grpc(&grpc_client(), &url, &sink.resolved.headers, body).await
        }
        Protocol::Api => {
            deliver::post(
                client,
                &url,
                &sink.resolved.headers,
                "application/json",
                body.to_vec(),
                false,
            )
            .await
        }
        _ => {
            let ct = meta["ct"].as_str().unwrap_or("application/json");
            deliver::post(client, &url, &sink.resolved.headers, ct, body.to_vec(), true).await
        }
    };
    match outcome {
        Outcome::Delivered => {
            sink.spool.ack(entry.id);
            sink.health.sent.fetch_add(items, Ordering::Relaxed);
            sink.health.requests.fetch_add(1, Ordering::Relaxed);
            sink.health.consecutive_failures.store(0, Ordering::Relaxed);
            sink.health
                .last_success
                .store(time::OffsetDateTime::now_utc().unix_timestamp(), Ordering::Relaxed);
            None
        }
        Outcome::Refused { why } => {
            sink.spool.bury(entry.id);
            sink.health.dead_letter.fetch_add(items, Ordering::Relaxed);
            if is_traces {
                state.metrics.forward_dropped_spans.fetch_add(items, Ordering::Relaxed);
            }
            sink.record_error(&why);
            tracing::warn!(
                destination = %name,
                endpoint = %config::shown_url(&sink.resolved.base),
                items,
                "the destination {why}; the batch is refused for good, so it is moved to dead/ \
                 and not retried. Check this destination's settings, and lower batch_size if \
                 the answer was 413"
            );
            None
        }
        Outcome::Retry { why, after } => {
            sink.spool.release(entry.id);
            sink.health.failed_attempts.fetch_add(1, Ordering::Relaxed);
            let failures = sink.health.consecutive_failures.fetch_add(1, Ordering::Relaxed);
            sink.record_error(&why);
            let wait = after
                .unwrap_or_else(|| deliver::backoff(u32::try_from(failures).unwrap_or(u32::MAX)));
            // Once per streak, then every tenth, so an outage that lasts an
            // hour is a handful of lines rather than thousands.
            if failures == 0 || failures % 10 == 9 {
                tracing::warn!(
                    destination = %name,
                    endpoint = %config::shown_url(&sink.resolved.base),
                    failures = failures + 1,
                    backlog = sink.spool.len(),
                    retry_in_ms = wait.as_millis(),
                    "the destination {why}; the batch stays in the spool and is retried"
                );
            }
            sink.health.hold_until_ms.store(
                now_ms() + i64::try_from(wait.as_millis()).unwrap_or(i64::MAX),
                Ordering::Relaxed,
            );
            Some(wait)
        }
    }
}

/// On stop, one attempt each at whatever a memory spool holds. A spool on
/// disk is left alone: it is sent by the next start, and spending the grace
/// period on a destination that is down would hold the stop for nothing.
async fn final_pass(state: &crate::AppState, sink: &Sink) {
    if sink.spool.dir().is_some() {
        return;
    }
    let mut tried = std::collections::HashSet::new();
    while let Some(entry) = sink.spool.claim() {
        if !tried.insert(entry.id) {
            sink.spool.release(entry.id);
            break;
        }
        if settle(state, sink, &state.outbound, &entry).await.is_some() {
            break;
        }
    }
}

/// The client gRPC is sent with: HTTP/2 from the first byte, which is what a
/// gRPC server expects on a plain-text port, and the same over TLS, where the
/// handshake would agree on it anyway.
fn grpc_client() -> reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .http2_prior_knowledge()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_default()
        })
        .clone()
}

fn now_ms() -> i64 {
    i64::try_from(time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000).unwrap_or(0)
}

/// Say that copies are being dropped for want of room, at most once a minute.
fn warn_full(spans: usize) {
    static LAST: AtomicU64 = AtomicU64::new(0);
    let now = time::OffsetDateTime::now_utc().unix_timestamp().unsigned_abs();
    let last = LAST.load(Ordering::Relaxed);
    if now.saturating_sub(last) >= 60
        && LAST.compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed).is_ok()
    {
        tracing::warn!(
            spans,
            in_flight = MAX_IN_FLIGHT,
            "{MAX_IN_FLIGHT} forwarded copies are already waiting to be spooled, so new copies \
             are dropped and counted in mcpd_forward_dropped_spans_total; the local writes are \
             unaffected"
        );
    }
}
