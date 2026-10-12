//! Relay mode: an edge collector that spools locally and ships upstream.
//!
//! `mcpd relay` accepts every HTTP ingest door a daemon serves, writes each
//! request to a spool on disk before answering, and ships the spooled requests
//! to one or more upstream daemons in order of preference. Run one in every
//! region your agents run in: a sender talks to a collector a few milliseconds
//! away, and the link between regions, which is the one that fails, is crossed
//! by the relay with retries rather than by every agent on its own.
//!
//! **Delivery is at least once, and a resend is never counted twice.** Every
//! request is shipped with an `Idempotency-Key`, the sender's own when it sent
//! one and otherwise a digest of the door and the body. The upstream records
//! the key with the batch, so a request the relay resends after a timeout, or
//! sends to the secondary after the primary took it and the answer was lost,
//! is recognised as a replay and reported as a duplicate.
//!
//! **Failover is ordered.** Each request goes to the first upstream that is
//! not cooling off after a failure; a retryable failure cools that upstream
//! and the request goes to the next one at once. When the primary's cool-off
//! ends, the next request tries it first again, so traffic returns to it on
//! its own.
//!
//! **Back-pressure is explicit.** The spool is bounded. At the bound a request
//! is refused with a 503 and `Retry-After`, which every SDK and OpenTelemetry
//! exporter retries, so the sender holds the data rather than the relay
//! dropping it. `/readyz` reports not ready once the spool is that full, or
//! nearly full with no upstream reachable, so a load balancer moves traffic
//! to a relay that can still deliver.

use crate::error::ApiError;
use crate::forward::deliver::{self, Outcome};
use crate::spool::{self, PushError, Spool};
use axum::body::{Body, Bytes};
use axum::extract::{OriginalUri, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// The header a resend is recognised by.
pub const IDEMPOTENCY_HEADER: &str = "idempotency-key";

/// The longest idempotency key read from a request. A longer one is ignored
/// rather than stored, since a key exists to be compared, not to carry data.
const MAX_KEY_LEN: usize = 200;

/// The idempotency key a request names in its `Idempotency-Key` header.
///
/// Every ingest door reads this, so a batch sent twice under one key is stored
/// once and the second answer reports it as a duplicate. It is the same
/// promise the native door makes for `ingest_key` in the body, for doors whose
/// body has no place for one. A key in the body wins over the header.
#[must_use]
pub fn idempotency_key(headers: &HeaderMap) -> Option<String> {
    let v = headers.get(IDEMPOTENCY_HEADER)?.to_str().ok()?.trim();
    (!v.is_empty() && v.len() <= MAX_KEY_LEN).then(|| format!("idem-{v}"))
}

/// How a relay is configured.
#[derive(Clone)]
pub struct RelayConfig {
    /// Upstream daemons, in order of preference.
    pub upstreams: Vec<String>,
    /// The key the relay presents upstream. With none, the key each sender
    /// presented travels with its request and is held in the spool until the
    /// request is delivered.
    pub upstream_key: Option<String>,
    /// Keys a sender must present to the relay. Empty accepts any request.
    pub ingest_keys: Vec<String>,
    /// Where the spool lives. `None` holds it in memory.
    pub spool_dir: Option<PathBuf>,
    /// The spool's bound, in bytes.
    pub spool_max_bytes: u64,
    /// Largest request body accepted.
    pub max_body_bytes: usize,
    /// Longest a request may take.
    pub request_timeout_seconds: u64,
    /// Requests shipped at once.
    pub workers: usize,
}

impl Default for RelayConfig {
    fn default() -> Self {
        Self {
            upstreams: Vec::new(),
            upstream_key: None,
            ingest_keys: Vec::new(),
            spool_dir: None,
            spool_max_bytes: 1024 * 1024 * 1024,
            max_body_bytes: 32 * 1024 * 1024,
            request_timeout_seconds: 30,
            workers: 4,
        }
    }
}

/// The spool fraction past which a relay with no reachable upstream reports
/// not ready. Below it the relay keeps accepting, which is what the spool is
/// for; above it the remaining room is kept for the senders already pointed
/// here while the load balancer moves new traffic elsewhere.
const READY_FILL_WITHOUT_UPSTREAM: f64 = 0.8;

/// How long after its last success an upstream still counts as reachable.
const REACHABLE_FOR: Duration = Duration::from_secs(30);

/// One upstream daemon and what the relay knows about it.
pub struct Upstream {
    /// Its base URL.
    pub url: String,
    /// Requests it accepted.
    pub delivered: AtomicU64,
    /// Attempts it did not accept.
    pub failed: AtomicU64,
    /// Failures since its last success.
    pub failures: AtomicU64,
    /// Until when, in milliseconds since the epoch, it is skipped.
    pub cool_until_ms: AtomicI64,
    /// When it last answered a probe or accepted a request, in seconds.
    pub last_ok: AtomicI64,
}

impl Upstream {
    fn new(url: String) -> Self {
        Self {
            url,
            delivered: AtomicU64::new(0),
            failed: AtomicU64::new(0),
            failures: AtomicU64::new(0),
            cool_until_ms: AtomicI64::new(0),
            last_ok: AtomicI64::new(0),
        }
    }

    /// Whether it answered recently.
    #[must_use]
    pub fn reachable(&self) -> bool {
        let last = self.last_ok.load(Ordering::Relaxed);
        last > 0 && now_s() - last <= REACHABLE_FOR.as_secs() as i64
    }

    fn ok(&self) {
        self.failures.store(0, Ordering::Relaxed);
        self.cool_until_ms.store(0, Ordering::Relaxed);
        self.last_ok.store(now_s(), Ordering::Relaxed);
    }

    fn cool(&self, asked: Option<Duration>) {
        self.failed.fetch_add(1, Ordering::Relaxed);
        let n = self.failures.fetch_add(1, Ordering::Relaxed);
        let wait = asked.unwrap_or_else(|| deliver::backoff(u32::try_from(n).unwrap_or(u32::MAX)));
        self.cool_until_ms.store(
            now_ms() + i64::try_from(wait.as_millis()).unwrap_or(i64::MAX),
            Ordering::Relaxed,
        );
    }
}

fn now_ms() -> i64 {
    i64::try_from(time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000).unwrap_or(0)
}

fn now_s() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

/// What a relay counts, for `/metrics`.
#[derive(Default)]
pub struct RelayMetrics {
    /// Requests written to the spool.
    pub accepted: AtomicU64,
    /// Requests refused because the spool was full.
    pub refused_full: AtomicU64,
    /// Requests that went to an upstream other than the first.
    pub failovers: AtomicU64,
    /// Requests an upstream refused for good, moved to `dead/`.
    pub dead_letter: AtomicU64,
}

/// A running relay.
pub struct RelayState {
    /// How it was configured.
    pub config: RelayConfig,
    /// Requests waiting to be shipped.
    pub spool: Spool,
    /// The upstreams, in order.
    pub upstreams: Vec<Upstream>,
    /// What it has counted.
    pub metrics: RelayMetrics,
    /// The client requests are shipped with.
    pub client: reqwest::Client,
    /// Background work: the shipping workers and the prober.
    pub tasks: tokio_util::task::TaskTracker,
    /// Cancelled when the stop signal arrives.
    pub shutdown: tokio_util::sync::CancellationToken,
    started: AtomicBool,
    born: std::time::Instant,
}

impl std::fmt::Debug for RelayState {
    /// The upstreams without credentials and the spool's size; never a key.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RelayState")
            .field(
                "upstreams",
                &self
                    .upstreams
                    .iter()
                    .map(|u| crate::forward::config::shown_url(&u.url))
                    .collect::<Vec<_>>(),
            )
            .field("spool_entries", &self.spool.len())
            .finish_non_exhaustive()
    }
}

impl RelayState {
    /// Open the spool and get ready to serve.
    ///
    /// # Errors
    /// No upstream is named, an upstream is not a URL, or the spool cannot be
    /// opened.
    pub fn new(config: RelayConfig) -> Result<Self, String> {
        if config.upstreams.is_empty() {
            return Err(
                "mcpd relay needs at least one --upstream: the daemon it ships to, such as \
                 https://mcpd.eu-west.example:4317"
                    .to_owned(),
            );
        }
        for u in &config.upstreams {
            reqwest::Url::parse(u).map_err(|e| {
                format!(
                    "--upstream {} is not a URL ({e}); write it with its scheme, such as \
                     https://mcpd.example:4317",
                    crate::forward::config::shown_url(u)
                )
            })?;
        }
        let spool =
            Spool::open(config.spool_dir.as_deref(), config.spool_max_bytes).map_err(|e| {
                format!(
                "the relay spool at {} could not be opened: {e}; set --spool-dir to a writable \
                 directory",
                config.spool_dir.as_deref().map_or("memory".into(), |p| p.display().to_string())
            )
            })?;
        Ok(Self {
            upstreams: config.upstreams.iter().map(|u| Upstream::new(u.clone())).collect(),
            spool,
            metrics: RelayMetrics::default(),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
            tasks: tokio_util::task::TaskTracker::new(),
            shutdown: tokio_util::sync::CancellationToken::new(),
            started: AtomicBool::new(false),
            born: std::time::Instant::now(),
            config,
        })
    }

    /// Stop the background work and wait for it, for at most `grace`.
    /// Returns how many tasks were still running when it gave up.
    pub async fn drain(&self, grace: Duration) -> usize {
        self.shutdown.cancel();
        self.tasks.close();
        match tokio::time::timeout(grace, self.tasks.wait()).await {
            Ok(()) => 0,
            Err(_) => self.tasks.len(),
        }
    }

    fn fill(&self) -> f64 {
        self.spool.bytes() as f64 / self.spool.max_bytes().max(1) as f64
    }

    /// Whether the relay should take traffic, and if not, why.
    pub fn readiness(&self) -> Result<Option<String>, String> {
        let fill = self.fill();
        let reachable = self.upstreams.iter().any(Upstream::reachable);
        let pct = (fill * 100.0).round();
        if fill >= 1.0 {
            return Err(format!(
                "the spool is full ({} entries); requests are refused until an upstream accepts \
                 the backlog",
                self.spool.len()
            ));
        }
        if !reachable && fill >= READY_FILL_WITHOUT_UPSTREAM {
            return Err(format!(
                "no upstream has answered for {}s and the spool is {pct}% full",
                REACHABLE_FOR.as_secs()
            ));
        }
        if !reachable {
            return Ok(Some(format!(
                "no upstream has answered for {}s; requests are spooled ({pct}% full) and \
                 shipped once one does",
                REACHABLE_FOR.as_secs()
            )));
        }
        Ok(None)
    }

    /// The relay's metrics, in Prometheus text format.
    #[must_use]
    pub fn render(&self) -> String {
        fn one(s: &mut String, name: &str, help: &str, kind: &str, v: &str) {
            s.push_str(&format!("# HELP {name} {help}\n# TYPE {name} {kind}\n{name} {v}\n"));
        }
        let mut s = String::new();
        let n = |a: &AtomicU64| a.load(Ordering::Relaxed).to_string();
        let m = &self.metrics;
        one(
            &mut s,
            "mcpd_relay_accepted_total",
            "Requests written to the spool.",
            "counter",
            &n(&m.accepted),
        );
        one(
            &mut s,
            "mcpd_relay_refused_full_total",
            "Requests refused with a 503 because the spool was full.",
            "counter",
            &n(&m.refused_full),
        );
        one(
            &mut s,
            "mcpd_relay_failovers_total",
            "Requests delivered to an upstream other than the first.",
            "counter",
            &n(&m.failovers),
        );
        one(
            &mut s,
            "mcpd_relay_dead_letter_total",
            "Requests an upstream refused for good, moved to the spool's dead/ directory.",
            "counter",
            &n(&m.dead_letter),
        );
        one(
            &mut s,
            "mcpd_relay_spool_entries",
            "Requests waiting in the spool.",
            "gauge",
            &self.spool.len().to_string(),
        );
        one(
            &mut s,
            "mcpd_relay_spool_bytes",
            "Bytes the spool holds.",
            "gauge",
            &self.spool.bytes().to_string(),
        );
        one(
            &mut s,
            "mcpd_relay_spool_oldest_seconds",
            "Age of the oldest request waiting in the spool, or 0.",
            "gauge",
            &self.spool.oldest_age_seconds().unwrap_or(0).to_string(),
        );
        type Family<'a> = (&'a str, &'a str, &'a str, &'a dyn Fn(&Upstream) -> String);
        let families: [Family<'_>; 3] = [
            ("mcpd_relay_delivered_total", "Requests an upstream accepted.", "counter", &|u| {
                n(&u.delivered)
            }),
            (
                "mcpd_relay_failed_attempts_total",
                "Attempts an upstream did not accept.",
                "counter",
                &|u| n(&u.failed),
            ),
            (
                "mcpd_relay_upstream_up",
                "1 when the upstream answered in the last 30 seconds, 0 otherwise.",
                "gauge",
                &|u| u8::from(u.reachable()).to_string(),
            ),
        ];
        for (name, help, kind, read) in families {
            s.push_str(&format!("# HELP {name} {help}\n# TYPE {name} {kind}\n"));
            for (i, u) in self.upstreams.iter().enumerate() {
                s.push_str(&format!(
                    "{name}{{upstream=\"{}\",order=\"{i}\"}} {}\n",
                    crate::forward::config::shown_url(&u.url).replace('"', ""),
                    read(u)
                ));
            }
        }
        one(
            &mut s,
            "mcpd_uptime_seconds",
            "Seconds since this process started.",
            "gauge",
            &self.born.elapsed().as_secs().to_string(),
        );
        s
    }
}

/// The relay's routes: the probes, and every other POST as an ingest door.
///
/// Any POST is accepted rather than a fixed list, so a door a newer daemon
/// adds is relayed without a relay release. The upstream decides what the
/// body means; the relay only promises to get it there.
pub fn router(state: Arc<RelayState>) -> Router {
    use tower_http::limit::RequestBodyLimitLayer;
    use tower_http::timeout::TimeoutLayer;
    let max = state.config.max_body_bytes;
    let deadline = Duration::from_secs(state.config.request_timeout_seconds.max(1));
    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/metrics", get(metrics))
        .fallback(accept)
        .layer(axum::extract::DefaultBodyLimit::disable())
        .layer(RequestBodyLimitLayer::new(max))
        .layer(TimeoutLayer::with_status_code(StatusCode::SERVICE_UNAVAILABLE, deadline))
        .layer(tower_http::catch_panic::CatchPanicLayer::new())
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .with_state(state)
}

fn health(state: &RelayState, status: &str, detail: Option<String>) -> serde_json::Value {
    let mut v = json!({
        "status": status,
        "version": mcpd_core::VERSION,
        "spec_version": mcpd_core::SPEC_VERSION,
        "uptime_seconds": state.born.elapsed().as_secs(),
        "storage": "relay",
    });
    if let Some(d) = detail {
        v["detail"] = json!(d);
    }
    v
}

async fn healthz(State(state): State<Arc<RelayState>>) -> Json<serde_json::Value> {
    Json(health(&state, "ok", None))
}

async fn readyz(State(state): State<Arc<RelayState>>) -> (StatusCode, Json<serde_json::Value>) {
    match state.readiness() {
        Ok(None) => (StatusCode::OK, Json(health(&state, "ok", None))),
        Ok(Some(why)) => (StatusCode::OK, Json(health(&state, "degraded", Some(why)))),
        Err(why) => (StatusCode::SERVICE_UNAVAILABLE, Json(health(&state, "degraded", Some(why)))),
    }
}

async fn metrics(State(state): State<Arc<RelayState>>) -> impl IntoResponse {
    ([("content-type", "text/plain; version=0.0.4; charset=utf-8")], state.render())
}

/// The key a request presents, the way the daemon reads it.
fn presented(headers: &HeaderMap) -> Option<String> {
    let clean = |s: &str| Some(s.trim()).filter(|s| !s.is_empty()).map(str::to_owned);
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer ").or_else(|| v.strip_prefix("bearer ")))
        .and_then(clean)
        .or_else(|| headers.get("x-mcpd-key").and_then(|v| v.to_str().ok()).and_then(clean))
}

/// Equal without saying where they differ: both sides are hashed first.
fn same(a: &str, b: &str) -> bool {
    let (x, y) = (Sha256::digest(a.as_bytes()), Sha256::digest(b.as_bytes()));
    x.iter().zip(y.iter()).fold(0u8, |d, (p, q)| d | (p ^ q)) == 0
}

/// Every POST: check the key, spool the request, answer.
async fn accept(
    State(state): State<Arc<RelayState>>,
    OriginalUri(uri): OriginalUri,
    method: axum::http::Method,
    headers: HeaderMap,
    body: Body,
) -> Response {
    if method != axum::http::Method::POST {
        return ApiError::NotFound(format!(
            "{} is not served by a relay; it accepts ingest POSTs and serves /healthz, /readyz \
             and /metrics",
            uri.path()
        ))
        .into_response();
    }
    let key = presented(&headers);
    if !state.config.ingest_keys.is_empty()
        && !key.as_deref().is_some_and(|k| state.config.ingest_keys.iter().any(|x| same(x, k)))
    {
        return ApiError::Unauthorized(
            "this relay requires an ingest key; send it as `Authorization: Bearer <key>`"
                .to_owned(),
        )
        .into_response();
    }
    let body: Bytes = match axum::body::to_bytes(body, state.config.max_body_bytes).await {
        Ok(b) => b,
        Err(_) => {
            return ApiError::TooLarge(format!(
                "request body is larger than the {} bytes this relay accepts per request; send \
                 fewer signals per request",
                state.config.max_body_bytes
            ))
            .into_response()
        }
    };
    let path =
        uri.path_and_query().map_or_else(|| uri.path().to_owned(), |p| p.as_str().to_owned());
    let header = |name: &str| headers.get(name).and_then(|v| v.to_str().ok()).map(str::to_owned);
    // The sender's key when it sent one, so the sender's own retry and the
    // relay's resend share it. Otherwise a digest of where the request was
    // going and what it carried, so the relay's resends share it.
    let idempotency = header(IDEMPOTENCY_HEADER)
        .filter(|k| !k.trim().is_empty() && k.len() <= MAX_KEY_LEN)
        .unwrap_or_else(|| {
            let mut h = Sha256::new();
            h.update(path.as_bytes());
            h.update([0]);
            h.update(header("content-encoding").unwrap_or_default().as_bytes());
            h.update([0]);
            h.update(&body);
            format!("relay-{}", hex::encode(&h.finalize()[..16]))
        });
    let mut meta = json!({
        "path": path,
        "ct": header("content-type"),
        "ce": header("content-encoding"),
        "key": idempotency,
    });
    if state.config.upstream_key.is_none() {
        // Kept so the upstream authorises the request as the sender's, which
        // is what decides the projects it reaches. The entry file is readable
        // by this user only.
        meta["auth"] = json!(header("authorization"));
        meta["mcpd_key"] = json!(header("x-mcpd-key"));
    }
    let framed = spool::frame(&meta, &body);
    let owned = Arc::clone(&state);
    let pushed = tokio::task::spawn_blocking(move || owned.spool.push(&framed)).await;
    match pushed {
        Ok(Ok(_)) => {
            state.metrics.accepted.fetch_add(1, Ordering::Relaxed);
            spawn(&state);
            accepted_answer(uri.path())
        }
        Ok(Err(PushError::Full)) => {
            state.metrics.refused_full.fetch_add(1, Ordering::Relaxed);
            let mut r = ApiError::Unavailable(
                "the relay's spool is full while it waits for an upstream; retry shortly"
                    .to_owned(),
            )
            .into_response();
            r.headers_mut()
                .insert(axum::http::header::RETRY_AFTER, axum::http::HeaderValue::from(5_u32));
            r
        }
        Ok(Err(PushError::Io(e))) => {
            ApiError::Unavailable(format!("the relay could not write its spool: {e}"))
                .into_response()
        }
        Err(_) => ApiError::Internal("writing the spool panicked".to_owned()).into_response(),
    }
}

/// What a sender is told once its request is on disk.
///
/// The OTLP doors answer OTLP's success shape with a 200, which is what an
/// OpenTelemetry exporter requires. Every other door answers 202: the request
/// is held and will be delivered, and the counts a daemon would give belong to
/// the upstream, which has not seen it yet. Every SDK reads any 2xx as
/// success.
fn accepted_answer(path: &str) -> Response {
    let otlp = match path {
        "/v1/traces" => Some("rejectedSpans"),
        "/v1/logs" => Some("rejectedLogRecords"),
        "/v1/metrics" => Some("rejectedDataPoints"),
        _ => None,
    };
    match otlp {
        Some(field) => {
            (StatusCode::OK, Json(json!({ "partialSuccess": { field: 0, "errorMessage": "" } })))
                .into_response()
        }
        None => (
            StatusCode::ACCEPTED,
            [("mcpd-relay", "spooled")],
            Json(json!({ "accepted": 0, "rejected": 0, "duplicate": 0 })),
        )
            .into_response(),
    }
}

/// Start the shipping workers and the upstream prober. Only the first call
/// starts anything.
pub fn spawn(state: &Arc<RelayState>) {
    if state.started.swap(true, Ordering::AcqRel) {
        return;
    }
    if !state.spool.is_empty() {
        tracing::info!(
            entries = state.spool.len(),
            "shipping the backlog the spool holds from before this start"
        );
    }
    for _ in 0..state.config.workers.max(1) {
        let s = Arc::clone(state);
        state.tasks.spawn(async move { ship(&s).await });
    }
    let s = Arc::clone(state);
    state.tasks.spawn(async move { probe(&s).await });
}

/// Ask every upstream's `/readyz` every few seconds, so readiness reflects
/// whether one can be reached before a request has to find out.
async fn probe(state: &RelayState) {
    let client =
        reqwest::Client::builder().timeout(Duration::from_secs(3)).build().unwrap_or_default();
    loop {
        for u in &state.upstreams {
            let url = format!("{}/readyz", u.url.trim_end_matches('/'));
            if let Ok(res) = client.get(url).send().await {
                if res.status().is_success() {
                    u.last_ok.store(now_s(), Ordering::Relaxed);
                }
            }
        }
        tokio::select! {
            () = state.shutdown.cancelled() => return,
            () = tokio::time::sleep(Duration::from_secs(5)) => {}
        }
    }
}

/// One shipping worker.
async fn ship(state: &RelayState) {
    loop {
        if state.shutdown.is_cancelled() {
            return;
        }
        let Some(entry) = state.spool.claim() else {
            tokio::select! {
                () = state.shutdown.cancelled() => return,
                () = state.spool.ready() => {}
                () = tokio::time::sleep(Duration::from_secs(1)) => {}
            }
            continue;
        };
        if let Some(wait) = deliver_one(state, &entry).await {
            tokio::select! {
                () = state.shutdown.cancelled() => return,
                () = tokio::time::sleep(wait) => {}
            }
        }
    }
}

/// Try one spooled request against the upstreams in order. Returns how long
/// to wait when none took it.
pub(crate) async fn deliver_one(state: &RelayState, entry: &spool::Claimed) -> Option<Duration> {
    let Some((meta, body)) = spool::unframe(&entry.bytes) else {
        state.spool.bury(entry.id);
        return None;
    };
    let path = meta["path"].as_str().unwrap_or("/v1/ingest");
    let mut headers: Vec<(String, String)> = Vec::new();
    if let Some(ce) = meta["ce"].as_str() {
        headers.push(("content-encoding".into(), ce.to_owned()));
    }
    if let Some(k) = meta["key"].as_str() {
        headers.push((IDEMPOTENCY_HEADER.into(), k.to_owned()));
    }
    match &state.config.upstream_key {
        Some(k) => headers.push(("authorization".into(), format!("Bearer {k}"))),
        None => {
            if let Some(a) = meta["auth"].as_str() {
                headers.push(("authorization".into(), a.to_owned()));
            }
            if let Some(a) = meta["mcpd_key"].as_str() {
                headers.push(("x-mcpd-key".into(), a.to_owned()));
            }
        }
    }
    let ct = meta["ct"].as_str().unwrap_or("application/json");

    let now = now_ms();
    let mut tried = false;
    for (i, u) in state.upstreams.iter().enumerate() {
        if u.cool_until_ms.load(Ordering::Relaxed) > now {
            continue;
        }
        tried = true;
        let url = format!("{}{path}", u.url.trim_end_matches('/'));
        match deliver::post(&state.client, &url, &headers, ct, body.to_vec(), false).await {
            Outcome::Delivered => {
                state.spool.ack(entry.id);
                u.delivered.fetch_add(1, Ordering::Relaxed);
                u.ok();
                if i > 0 {
                    state.metrics.failovers.fetch_add(1, Ordering::Relaxed);
                }
                return None;
            }
            Outcome::Refused { why } => {
                // Final: the request is wrong for every upstream alike, since
                // they run the same daemon. Sending it to the next would be
                // the same answer at more cost.
                state.spool.bury(entry.id);
                state.metrics.dead_letter.fetch_add(1, Ordering::Relaxed);
                u.ok();
                tracing::warn!(
                    upstream = %crate::forward::config::shown_url(&u.url),
                    path,
                    "the upstream {why}; the request is refused for good, so it is moved to \
                     the spool's dead/ directory"
                );
                return None;
            }
            Outcome::Retry { why, after } => {
                let first = u.failures.load(Ordering::Relaxed) == 0;
                u.cool(after);
                if first {
                    tracing::warn!(
                        upstream = %crate::forward::config::shown_url(&u.url),
                        order = i,
                        "the upstream {why}; trying the next one, and this one again after a \
                         pause"
                    );
                }
            }
        }
    }
    state.spool.release(entry.id);
    // Every upstream is cooling. Wait for the first to come back, and at
    // least a moment, so a relay with no upstream does not spin.
    let soonest = state
        .upstreams
        .iter()
        .map(|u| u.cool_until_ms.load(Ordering::Relaxed) - now_ms())
        .min()
        .unwrap_or(0)
        .max(if tried { 0 } else { 100 });
    Some(Duration::from_millis(u64::try_from(soonest).unwrap_or(0).max(50)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_header_key_is_read_trimmed_and_bounded() {
        let mut h = HeaderMap::new();
        assert_eq!(idempotency_key(&h), None);
        h.insert(IDEMPOTENCY_HEADER, " abc ".parse().unwrap());
        assert_eq!(idempotency_key(&h).as_deref(), Some("idem-abc"));
        h.insert(IDEMPOTENCY_HEADER, "x".repeat(MAX_KEY_LEN + 1).parse().unwrap());
        assert_eq!(idempotency_key(&h), None);
    }

    #[test]
    fn a_relay_with_no_upstream_or_a_bad_one_is_refused() {
        assert!(RelayState::new(RelayConfig::default()).is_err());
        let bad = RelayConfig { upstreams: vec!["not a url".into()], ..RelayConfig::default() };
        let err = RelayState::new(bad).expect_err("refused");
        assert!(err.contains("--upstream"), "{err}");
    }

    #[test]
    fn readiness_follows_the_spool_and_the_upstreams() {
        let state = RelayState::new(RelayConfig {
            upstreams: vec!["http://127.0.0.1:9".into()],
            spool_max_bytes: 1000,
            ..RelayConfig::default()
        })
        .expect("a relay");
        // No upstream has answered yet, but the spool is empty: degraded and
        // still ready, because spooling is what the relay is for.
        assert!(matches!(state.readiness(), Ok(Some(_))));
        state.spool.push(&[0u8; 850]).expect("fits");
        assert!(state.readiness().is_err(), "nearly full with nothing reachable");
        state.upstreams[0].ok();
        assert!(matches!(state.readiness(), Ok(None)));
    }
}
