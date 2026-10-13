//! The client: construction, the five signals, and the read API.

use crate::context::{self, SpanContext};
use crate::queue::{to_batch, Queue, Signal};
use crate::sink::Sink;
use crate::span::Span;
use crate::transport::Transport;
use crate::{Config, Error, Scale, Target, Throttle};
use mcpd_core::sampling::{Decision, Policy};
use mcpd_core::{
    ids, AnalyticsSummary, Correlation, CostBreakdown, Feedback, FeedbackKind,
    FeedbackListResponse, HealthStatus, LogLevel, LogListResponse, LogRecord, MetricKind,
    MetricPoint, MetricSeriesResponse, SamplingKind, Score, ScoreListResponse, ScoreSource,
    Span as SpanRecord, SpanKind, SpanListResponse, SpanStatus, TraceDetail, TraceListResponse,
};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;
use time::OffsetDateTime;
use tokio::sync::Notify;

/// How long [`Mcpd::flush`] and [`Mcpd::shutdown`] wait before they return
/// anyway, as in every SDK (contract section 2.4).
pub const DEFAULT_DEADLINE: Duration = Duration::from_secs(10);

/// A handle to mcpd. Cheap to clone, and every clone shares one queue and one
/// background worker.
///
/// ```no_run
/// # async fn f() {
/// let mcpd = mcpd_sdk::Mcpd::new();
/// let span = mcpd.trace("support agent");
/// span.set_output("refunded");
/// span.end();
/// let _ = mcpd.shutdown().await;
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct Mcpd {
    pub(crate) inner: Arc<ClientInner>,
}

/// The parts the worker needs to wait on. Kept apart from [`ClientInner`] so
/// the worker can hold them strongly while holding only a weak reference to the
/// client: a program that drops its client without calling `shutdown` leaves a
/// task that notices and exits, not one that runs forever.
#[derive(Debug)]
pub(crate) struct Shared {
    pub(crate) queue: Queue,
    stop: Notify,
}

/// Everything behind the handle.
pub(crate) struct ClientInner {
    pub(crate) config: Config,
    pub(crate) shared: Arc<Shared>,
    /// `None` when the client is disabled, which is what makes every call a
    /// no-op without a single branch in the calling code.
    transport: Option<Arc<Transport>>,
    /// Where drained batches go: the transport unless the configuration named
    /// another. `None` exactly when `transport` is.
    sink: Option<Arc<dyn Sink>>,
    sampler: Policy,
    /// Signals the sink accepted.
    exported: AtomicU64,
    /// Batches the sink refused.
    failed: AtomicU64,
    /// Spans [`Mcpd::record`] refused.
    rejected: AtomicU64,
    /// Values the configured redactor panicked on, dropped rather than sent.
    redaction_failures: AtomicU64,
    /// Fetched prompts, and the transport that fetches them even when the
    /// client is disabled.
    pub(crate) prompts: crate::prompts::PromptStore,
    /// Rate limiter for the "something is wrong with mcpd" messages. Without it
    /// an unreachable server turns into a second outage in the log pipeline.
    pub(crate) warn: Throttle,
    /// One batch at a time, so a manual `flush` and the worker cannot
    /// interleave two halves of the queue into two batches. Held by the task
    /// that sends the batch, so a caller that stops waiting at its deadline
    /// does not let a second batch start beside the one still in flight.
    send_lock: Arc<tokio::sync::Mutex<()>>,
    worker_started: AtomicBool,
    worker: Mutex<Option<tokio::task::JoinHandle<()>>>,
    stopped: AtomicBool,
    /// Set when a shutdown's deadline passed: no further batch starts, and
    /// what is still queued is given up (contract section 2.4).
    abandoned: AtomicBool,
}

impl std::fmt::Debug for ClientInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientInner")
            .field("config", &self.config)
            .field("enabled", &self.transport.is_some())
            .field("stopped", &self.stopped.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl Mcpd {
    /// A client configured entirely from the environment.
    ///
    /// With nothing set at all this talks to a local mcpd on its default port,
    /// which is the whole of the five-minute path.
    #[must_use]
    pub fn new() -> Self {
        Self::with_config(Config::from_env())
    }

    /// A client from an explicit configuration.
    ///
    /// Never fails. A configuration mcpd cannot use disables the client and
    /// logs why, because an observability call is not a thing a program should
    /// have to handle an error from.
    #[must_use]
    pub fn with_config(config: Config) -> Self {
        let transport = if config.enabled {
            match Transport::new(&config) {
                Ok(t) => Some(Arc::new(t)),
                Err(e) => {
                    tracing::warn!(error = %e, "mcpd is disabled: the HTTP client could not be built");
                    None
                }
            }
        } else {
            None
        };
        let sink: Option<Arc<dyn Sink>> = match (&transport, &config.sink) {
            (None, _) => None,
            (Some(_), Some(custom)) => Some(Arc::clone(custom)),
            (Some(t), None) => Some(Arc::clone(t) as Arc<dyn Sink>),
        };

        let sampler = if config.sample >= 1.0 {
            Policy::always()
        } else if config.sample <= 0.0 {
            Policy { kind: SamplingKind::Never, ..Policy::default() }
        } else {
            // Deterministic rather than random: every process in a fleet then
            // makes the same decision about the same trace, so a trace that
            // crosses a service boundary is never half recorded.
            Policy::deterministic(config.sample)
        };

        let inner = Arc::new(ClientInner {
            shared: Arc::new(Shared {
                queue: Queue::new(config.max_queue, config.batch_size, config.flush_interval),
                stop: Notify::new(),
            }),
            transport,
            sink,
            sampler,
            exported: AtomicU64::new(0),
            failed: AtomicU64::new(0),
            rejected: AtomicU64::new(0),
            redaction_failures: AtomicU64::new(0),
            prompts: crate::prompts::PromptStore::default(),
            warn: Throttle::new(config.flush_interval),
            send_lock: Arc::new(tokio::sync::Mutex::new(())),
            worker_started: AtomicBool::new(false),
            worker: Mutex::new(None),
            stopped: AtomicBool::new(false),
            abandoned: AtomicBool::new(false),
            config,
        });
        inner.ensure_worker();
        Self { inner }
    }

    /// The configuration this client is using.
    #[must_use]
    pub fn config(&self) -> &Config {
        &self.inner.config
    }

    /// Whether this client will send anything at all.
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.inner.transport.is_some()
    }

    // -------------------------------------------------------------- tracing ---

    /// Start a run: a root span of kind [`SpanKind::AgentRun`].
    ///
    /// The sampling decision is made here, once, and every descendant inherits
    /// it.
    #[must_use]
    pub fn trace(&self, name: impl Into<String>) -> Span {
        self.root(name, SpanKind::AgentRun, None)
    }

    /// Start a root span of a given kind.
    #[must_use]
    pub fn trace_of(&self, name: impl Into<String>, kind: SpanKind) -> Span {
        self.root(name, kind, None)
    }

    /// Continue a trace that began in another service.
    ///
    /// Pass the inbound `traceparent` header. An absent or malformed header
    /// starts a new trace instead of failing, because a header nobody can read
    /// is not a reason to drop a request.
    #[must_use]
    pub fn trace_from(
        &self,
        name: impl Into<String>,
        traceparent: Option<&str>,
        kind: SpanKind,
    ) -> Span {
        let upstream = traceparent.and_then(SpanContext::from_traceparent);
        self.root(name, kind, upstream)
    }

    /// Start a span under whatever is current in this task, or a new root when
    /// nothing is.
    ///
    /// This is the call to reach for inside a helper function: it attaches to
    /// the run in progress without that function being handed the parent span.
    #[must_use]
    pub fn span(&self, name: impl Into<String>, kind: SpanKind) -> Span {
        match context::current_active() {
            Some(parent) => {
                let ctx = parent.ctx.child();
                if parent.ctx.sampled && self.inner.transport.is_some() {
                    // The run's correlation, not the client's: a span opened
                    // from a task-local is still a step of that run.
                    let correlation = parent
                        .span
                        .as_ref()
                        .map_or_else(|| self.inner.correlation(), |r| r.correlation());
                    let span = Span::start(
                        &self.inner,
                        ctx,
                        Some(parent.ctx.span_id),
                        name,
                        kind,
                        correlation,
                    );
                    // A span nested under a step's span, by scope rather than
                    // by handle, still adds its usage to that step.
                    span.adopt_usage_sink(parent.span.as_ref().and_then(|r| r.usage_sink()));
                    span
                } else {
                    Span::silent(ctx)
                }
            }
            None => self.root(name, kind, None),
        }
    }

    fn root(&self, name: impl Into<String>, kind: SpanKind, upstream: Option<SpanContext>) -> Span {
        let (trace_id, parent, upstream_sampled) = match upstream {
            Some(u) => (u.trace_id, Some(u.span_id), Some(u.sampled)),
            None => (mcpd_core::ids::new_trace_id(), None, None),
        };
        let sampled =
            matches!(self.inner.sampler.decide(&trace_id, upstream_sampled), Decision::Keep);
        let ctx = SpanContext { trace_id, span_id: mcpd_core::ids::new_span_id(), sampled };
        if sampled && self.inner.transport.is_some() {
            Span::start(&self.inner, ctx, parent, name, kind, self.inner.correlation())
        } else {
            Span::silent(ctx)
        }
    }

    /// Queue a span that already ended, ids, parent, start and end included.
    ///
    /// For an adapter from another tracing system, a batch import or a
    /// replay: what the caller holds is what goes on the wire, never
    /// re-parented, re-timed or re-sampled. Correlation fields the record
    /// leaves empty are filled from this client's; fields it sets are kept.
    /// A record whose ids are not OpenTelemetry-shaped, whose name is empty
    /// or whose start time is the epoch is dropped and counted in
    /// [`Mcpd::stats`] as `rejected`, never raised (B1).
    pub fn record(&self, mut span: SpanRecord) {
        if self.inner.transport.is_none() {
            return;
        }
        if let Err(why) = validate_record(&span) {
            self.inner.rejected.fetch_add(1, Ordering::Relaxed);
            if self.inner.warn.allow() {
                tracing::warn!(span = %span.name, "mcpd refused a recorded span: {why}");
            }
            return;
        }
        let base = self.inner.correlation();
        let c = &mut span.correlation;
        if c.project.trim().is_empty() {
            c.project = base.project;
        }
        c.environment = c.environment.take().or(base.environment);
        c.service_name = c.service_name.take().or(base.service_name);
        c.service_version = c.service_version.take().or(base.service_version);
        if !self.inner.config.capture_payloads {
            span.input = None;
            span.output = None;
        }
        span.is_root = span.parent_span_id.is_none();
        self.inner.enqueue(Signal::Span(Box::new(span)));
    }

    /// What this client has done since it was built.
    #[must_use]
    pub fn stats(&self) -> Stats {
        Stats {
            queued: self.inner.shared.queue.len(),
            dropped: self.inner.shared.queue.dropped(),
            capacity: self.inner.shared.queue.capacity(),
            exported: self.inner.exported.load(Ordering::Relaxed),
            failed: self.inner.failed.load(Ordering::Relaxed),
            rejected: self.inner.rejected.load(Ordering::Relaxed),
            redaction_failures: self.inner.redaction_failures.load(Ordering::Relaxed),
        }
    }

    // -------------------------------------------------- the other four signals ---

    /// Record a log line, correlated to the span the current task is inside.
    pub fn log(&self, level: LogLevel, message: impl Into<String>) {
        self.log_with(level, message, None);
    }

    /// A log line with structured fields.
    pub fn log_with(
        &self,
        level: LogLevel,
        message: impl Into<String>,
        fields: Option<serde_json::Value>,
    ) {
        // Below the floor costs one comparison: no record, no queue, no
        // correlation lookup.
        if !crate::logs::meets_level(level, self.inner.config.log_level) {
            return;
        }
        let ctx = context::current();
        let record = LogRecord {
            log_id: mcpd_core::ids::new_trace_id(),
            trace_id: ctx.as_ref().map(|c| c.trace_id.clone()),
            span_id: ctx.as_ref().map(|c| c.span_id.clone()),
            timestamp: OffsetDateTime::now_utc(),
            level,
            message: message.into(),
            fields,
            logger: self.inner.config.service_name.clone(),
            correlation: self.signal_correlation(),
            received_at: OffsetDateTime::UNIX_EPOCH,
        };
        self.inner.enqueue(Signal::Log(Box::new(record)));
    }

    /// A log line carrying an error under the `error` key.
    ///
    /// The error is serialised to its type, its message and its cause chain,
    /// which is the same shape the other five SDKs send (contract section 4.1).
    ///
    /// ```no_run
    /// # let mcpd = mcpd_sdk::Mcpd::with_config(mcpd_sdk::Config::default().with_enabled(false));
    /// # let err = std::io::Error::new(std::io::ErrorKind::NotFound, "no such file");
    /// mcpd.log_error(mcpd_sdk::LogLevel::Error, "reading the prompt", &err);
    /// ```
    pub fn log_error<E>(&self, level: LogLevel, message: impl Into<String>, error: &E)
    where
        E: std::error::Error + ?Sized,
    {
        self.log_with(
            level,
            message,
            Some(serde_json::json!({ "error": crate::logs::error_fields(error) })),
        );
    }

    /// Record a numeric observation.
    pub fn metric(
        &self,
        name: impl Into<String>,
        value: f64,
        kind: MetricKind,
        labels: Option<serde_json::Value>,
        unit: Option<&str>,
    ) {
        let name = name.into();
        // serde_json writes NaN and the infinities as `null`, where the daemon
        // needs a number, and the daemon refuses the whole batch over it. The
        // point is what cannot be sent, so it alone is dropped (contract
        // section 4.1).
        if !value.is_finite() {
            if self.inner.warn.allow() {
                tracing::warn!(
                    metric = %name,
                    %value,
                    "mcpd dropped a metric point whose value JSON cannot carry"
                );
            }
            return;
        }
        let ctx = context::current();
        let point = MetricPoint {
            metric_id: mcpd_core::ids::new_trace_id(),
            name,
            kind,
            value,
            unit: unit.map(str::to_owned),
            timestamp: OffsetDateTime::now_utc(),
            labels,
            buckets: None,
            // The exemplar is what turns a spike on a chart into a run you can
            // open, so it is attached whenever there is a span to attach it to.
            trace_id: ctx.as_ref().map(|c| c.trace_id.clone()),
            span_id: ctx.as_ref().map(|c| c.span_id.clone()),
            correlation: self.signal_correlation(),
            received_at: OffsetDateTime::UNIX_EPOCH,
        };
        self.inner.enqueue(Signal::Metric(Box::new(point)));
    }

    /// Increment a counter.
    pub fn counter(&self, name: impl Into<String>, value: f64, labels: Option<serde_json::Value>) {
        self.metric(name, value, MetricKind::Counter, labels, None);
    }

    /// Record a gauge reading.
    pub fn gauge(&self, name: impl Into<String>, value: f64, labels: Option<serde_json::Value>) {
        self.metric(name, value, MetricKind::Gauge, labels, None);
    }

    /// Attach a machine evaluation to a run or a step.
    ///
    /// The [`Scale`] is not optional. A bare number with no declared range
    /// cannot be compared to anything, so the type system asks for one.
    pub fn score(
        &self,
        target: impl Into<Target>,
        scorer_id: impl Into<String>,
        value: f64,
        scale: Scale,
        reason: Option<&str>,
    ) {
        let scorer_id = scorer_id.into();
        if !(value.is_finite() && scale.min.is_finite() && scale.max.is_finite()) {
            if self.inner.warn.allow() {
                tracing::warn!(
                    scorer = %scorer_id,
                    %value,
                    min = %scale.min,
                    max = %scale.max,
                    "mcpd dropped a score whose numbers JSON cannot carry"
                );
            }
            return;
        }
        let target = target.into();
        let score = Score {
            score_id: mcpd_core::ids::new_trace_id(),
            trace_id: Some(target.trace_id().to_owned()),
            span_id: target.span_id().map(str::to_owned),
            scorer_id,
            scorer_version: None,
            source: ScoreSource::Live,
            value,
            min: Some(scale.min),
            max: Some(scale.max),
            label: None,
            reason: reason.map(str::to_owned),
            score_trace_id: None,
            cost: None,
            metadata: None,
            timestamp: OffsetDateTime::now_utc(),
            correlation: self.signal_correlation(),
        };
        self.inner.enqueue(Signal::Score(Box::new(score)));
    }

    /// Attach a person's judgement to a run or a step.
    pub fn feedback(
        &self,
        target: impl Into<Target>,
        kind: FeedbackKind,
        value: impl Into<serde_json::Value>,
        comment: Option<&str>,
    ) {
        let target = target.into();
        let feedback = Feedback {
            feedback_id: mcpd_core::ids::new_trace_id(),
            trace_id: Some(target.trace_id().to_owned()),
            span_id: target.span_id().map(str::to_owned),
            kind,
            value: value.into(),
            comment: comment.map(str::to_owned),
            author_id: None,
            author_kind: None,
            timestamp: OffsetDateTime::now_utc(),
            metadata: None,
            correlation: self.signal_correlation(),
        };
        self.inner.enqueue(Signal::Feedback(Box::new(feedback)));
    }

    /// What a log, a metric, a score or a piece of feedback is tagged with.
    ///
    /// The run's correlation when there is a run in progress, the client's when
    /// there is not. Taking the client's while inside a run is what left a log
    /// line with no session on it, and a log you cannot filter by session is a
    /// log you cannot find.
    fn signal_correlation(&self) -> Correlation {
        context::current_correlation().unwrap_or_else(|| self.inner.correlation())
    }

    // -------------------------------------------------------------- lifecycle ---

    /// Send everything queued and wait for it to land, for at most
    /// [`DEFAULT_DEADLINE`].
    ///
    /// # Errors
    /// Returns the last export failure. Signals lost to it are not re-queued:
    /// the transport has already retried, and a queue that refills itself on
    /// failure is how an outage becomes an out-of-memory kill. The error is
    /// safe to ignore, and ignoring it is the right default. Also returns an
    /// error when the deadline passed first, as [`Mcpd::flush_within`].
    pub async fn flush(&self) -> Result<(), Error> {
        self.flush_within(DEFAULT_DEADLINE).await
    }

    /// [`Mcpd::flush`], waiting at most `limit`.
    ///
    /// A daemon that accepts the connection and never answers costs every
    /// batch three attempts of the full request timeout, so a flush with no
    /// deadline could hold its caller for over half an hour.
    ///
    /// # Errors
    /// As [`Mcpd::flush`], and [`Error::Export`] when `limit` passed first.
    /// What was not sent by then stays queued for the worker, and a batch
    /// already in flight finishes on its own.
    pub async fn flush_within(&self, limit: Duration) -> Result<(), Error> {
        match tokio::time::timeout(limit, self.inner.export_all()).await {
            Ok(result) => result,
            Err(_) => Err(Error::Export(format!(
                "flush did not finish within {limit:?}; {} signals are still queued",
                self.inner.shared.queue.len()
            ))),
        }
    }

    /// Flush, stop the worker, and refuse further signals, within
    /// [`DEFAULT_DEADLINE`]. Safe to call twice.
    ///
    /// Short-lived processes must call this. A serverless function that returns
    /// without it exits with its spans still in memory.
    ///
    /// # Errors
    /// As [`Mcpd::shutdown_within`].
    pub async fn shutdown(&self) -> Result<(), Error> {
        self.shutdown_within(DEFAULT_DEADLINE).await
    }

    /// [`Mcpd::shutdown`], returning within `limit` even when the daemon never
    /// answers. When `limit` passes, no further batch starts and what is still
    /// queued is given up; [`Mcpd::stats`] still counts it as queued.
    ///
    /// # Errors
    /// The last export failure, as [`Mcpd::flush`], or [`Error::Export`] when
    /// `limit` passed first.
    pub async fn shutdown_within(&self, limit: Duration) -> Result<(), Error> {
        // Already-stopped is not an error: shutdown is idempotent, and a second
        // call in a cleanup path is normal.
        if self.inner.stopped.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        // notify_one, not notify_waiters: it leaves a permit behind when the
        // worker happens to be mid-export rather than waiting, so a shutdown
        // that lands in that window is not lost until the next interval.
        self.inner.shared.stop.notify_one();
        let handle = self.inner.worker.lock().unwrap_or_else(|e| e.into_inner()).take();
        let work = async {
            if let Some(h) = handle {
                // The worker flushes on its way out; waiting for it means
                // shutdown returns only once the queue really is empty.
                let _ = h.await;
            }
            self.inner.export_all().await
        };
        match tokio::time::timeout(limit, work).await {
            Ok(result) => result,
            Err(_) => {
                // The worker may still be exporting. It finishes the batch in
                // flight and starts no other.
                self.inner.abandoned.store(true, Ordering::SeqCst);
                Err(Error::Export(format!(
                    "shutdown did not finish within {limit:?}; {} queued signals were given up",
                    self.inner.shared.queue.len()
                )))
            }
        }
    }

    /// How many signals are waiting to be exported.
    #[must_use]
    pub fn queued_signals(&self) -> usize {
        self.inner.shared.queue.len()
    }

    /// How many signals the queue's bound has cost this client.
    ///
    /// Nonzero means mcpd could not keep up, or could not be reached. It is the
    /// one number worth alerting on.
    #[must_use]
    pub fn dropped_signals(&self) -> u64 {
        self.inner.shared.queue.dropped()
    }

    // ------------------------------------------------------------------ reads ---

    /// List traces.
    ///
    /// # Errors
    /// Returns [`Error::Api`] when mcpd refuses the query and [`Error::Http`]
    /// when it cannot be reached.
    pub async fn list_traces(&self, filter: &TraceFilter) -> Result<TraceListResponse, Error> {
        self.get("/v1/api/traces", filter.params(&self.inner.config.project)).await
    }

    /// Fetch one trace and its spans, plus whatever `include` asks for.
    ///
    /// # Errors
    /// As [`Mcpd::list_traces`].
    pub async fn get_trace(&self, trace_id: &str, include: Include) -> Result<TraceDetail, Error> {
        let mut params = vec![("project", self.inner.config.project.clone())];
        if let Some(v) = include.as_param() {
            params.push(("include", v));
        }
        self.get(&format!("/v1/api/traces/{trace_id}"), params).await
    }

    /// Search spans across traces.
    ///
    /// # Errors
    /// As [`Mcpd::list_traces`].
    pub async fn list_spans(&self, filter: &SpanFilter) -> Result<SpanListResponse, Error> {
        self.get("/v1/api/spans", filter.params(&self.inner.config.project)).await
    }

    /// Search logs.
    ///
    /// # Errors
    /// As [`Mcpd::list_traces`].
    pub async fn list_logs(&self, filter: &LogFilter) -> Result<LogListResponse, Error> {
        self.get("/v1/api/logs", filter.params(&self.inner.config.project)).await
    }

    /// Query a metric as a time series.
    ///
    /// # Errors
    /// As [`Mcpd::list_traces`].
    pub async fn query_metrics(&self, query: &MetricQuery) -> Result<MetricSeriesResponse, Error> {
        self.get("/v1/api/metrics", query.params(&self.inner.config.project)?).await
    }

    /// List scores.
    ///
    /// # Errors
    /// As [`Mcpd::list_traces`].
    pub async fn list_scores(&self, filter: &ScoreFilter) -> Result<ScoreListResponse, Error> {
        self.get("/v1/api/scores", filter.params(&self.inner.config.project)).await
    }

    /// List human feedback for one run.
    ///
    /// # Errors
    /// As [`Mcpd::list_traces`].
    pub async fn list_feedback(&self, trace_id: &str) -> Result<FeedbackListResponse, Error> {
        self.get(
            "/v1/api/feedback",
            vec![("project", self.inner.config.project.clone()), ("trace_id", trace_id.to_owned())],
        )
        .await
    }

    /// Headline numbers for a window: runs, error rate, latency, tokens, cost.
    ///
    /// # Errors
    /// As [`Mcpd::list_traces`].
    pub async fn analytics_summary(
        &self,
        from: OffsetDateTime,
        to: OffsetDateTime,
        group_by: Option<&str>,
    ) -> Result<AnalyticsSummary, Error> {
        let mut params = vec![
            ("project", self.inner.config.project.clone()),
            ("from", rfc3339(from)),
            ("to", rfc3339(to)),
        ];
        if let Some(g) = group_by {
            params.push(("group_by", g.to_owned()));
        }
        self.get("/v1/api/analytics/summary", params).await
    }

    /// Where the money went.
    ///
    /// # Errors
    /// As [`Mcpd::list_traces`].
    pub async fn analytics_cost(
        &self,
        from: OffsetDateTime,
        to: OffsetDateTime,
        group_by: Option<&str>,
        bucket: Option<&str>,
    ) -> Result<CostBreakdown, Error> {
        let mut params = vec![
            ("project", self.inner.config.project.clone()),
            ("from", rfc3339(from)),
            ("to", rfc3339(to)),
        ];
        if let Some(g) = group_by {
            params.push(("group_by", g.to_owned()));
        }
        if let Some(b) = bucket {
            params.push(("bucket", b.to_owned()));
        }
        self.get("/v1/api/analytics/cost", params).await
    }

    /// Whether the server is alive, and which spec version it speaks.
    ///
    /// # Errors
    /// As [`Mcpd::list_traces`].
    pub async fn health(&self) -> Result<HealthStatus, Error> {
        self.get("/healthz", Vec::new()).await
    }

    pub(crate) async fn get<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        params: Vec<(&'static str, String)>,
    ) -> Result<T, Error> {
        self.transport()?.get(path, &params).await
    }

    pub(crate) async fn post<B: serde::Serialize + ?Sized, T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        params: Vec<(&'static str, String)>,
        body: &B,
    ) -> Result<T, Error> {
        self.transport()?.post(path, &params, body).await
    }

    pub(crate) fn transport(&self) -> Result<&Transport, Error> {
        self.inner.transport.as_deref().ok_or_else(|| {
            Error::Config("this client is disabled, so it cannot talk to mcpd".to_owned())
        })
    }

    /// The project this client was configured with.
    #[must_use]
    pub fn project(&self) -> &str {
        &self.inner.config.project
    }
}

impl Default for Mcpd {
    fn default() -> Self {
        Self::new()
    }
}

impl ClientInner {
    /// The correlation every signal from this client carries.
    pub(crate) fn correlation(&self) -> Correlation {
        Correlation {
            project: self.config.project.clone(),
            environment: self.config.environment.clone(),
            service_name: self.config.service_name.clone(),
            service_version: self.config.service_version.clone(),
            session_id: None,
            thread_id: None,
            user_id: None,
            resource_id: None,
            run_id: None,
            request_id: None,
            experiment_id: None,
            release: None,
        }
    }

    /// Queue one signal. Returns immediately, and never fails.
    pub(crate) fn enqueue(self: &Arc<Self>, signal: Signal) {
        if self.transport.is_none() {
            return;
        }
        if self.stopped.load(Ordering::Relaxed) {
            if self.warn.allow() {
                tracing::warn!("mcpd has been shut down; this signal was not recorded");
            }
            return;
        }
        let signal = match &self.config.redact {
            Some(redact) => self.redacted(redact, signal),
            None => signal,
        };
        self.shared.queue.push(signal);
        // A client built before the runtime started, or on a thread with none,
        // gets its worker at the first signal instead of never.
        self.ensure_worker();
    }

    /// The signal with the redactor applied to its payload fields. The queued
    /// record is already a copy, so the caller's span keeps what it was
    /// given. Fails closed, field by field (contract section 2.1.1).
    fn redacted(&self, redact: &crate::Redactor, signal: Signal) -> Signal {
        match signal {
            Signal::Span(mut span) => {
                let s = &mut *span;
                for (field, slot) in [
                    ("input", &mut s.input),
                    ("output", &mut s.output),
                    ("attributes", &mut s.attributes),
                    ("metadata", &mut s.metadata),
                ] {
                    self.redact_slot(redact, field, slot);
                }
                Signal::Span(span)
            }
            Signal::Log(mut log) => {
                self.redact_slot(redact, "fields", &mut log.fields);
                Signal::Log(log)
            }
            other => other,
        }
    }

    /// Run the redactor on one field. A panic inside it, or a result of the
    /// wrong shape for an object field, drops the field. A panic is caught, because
    /// user code must not unwind through instrumentation (B1), and the field
    /// is dropped, because a broken redactor is the case where sending the
    /// original does the most harm.
    fn redact_slot(
        &self,
        redact: &crate::Redactor,
        field: &'static str,
        slot: &mut Option<serde_json::Value>,
    ) {
        let Some(value) = slot.take() else { return };
        // The closure owns the value and the redactor is shared, immutable
        // code; a panic half way through leaves nothing of ours inconsistent.
        let failed = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| redact(value)))
        {
            // Null is the redactor saying "drop it", which is a choice, not a
            // failure.
            Ok(serde_json::Value::Null) => false,
            // Attributes, metadata and log fields are objects on the wire; any
            // other shape is unusable, and dropped and counted like a panic
            // so every SDK agrees on what a broken redactor costs.
            Ok(kept) if field != "input" && field != "output" && !kept.is_object() => true,
            Ok(kept) => {
                *slot = Some(kept);
                false
            }
            Err(_) => true,
        };
        if failed {
            self.redaction_failures.fetch_add(1, Ordering::Relaxed);
            if self.warn.allow() {
                tracing::warn!(field, "the mcpd redactor failed on this field; it was dropped");
            }
        }
    }

    /// Start the background worker, once, if there is a runtime to start it on.
    fn ensure_worker(self: &Arc<Self>) {
        if self.transport.is_none() || self.worker_started.load(Ordering::Acquire) {
            return;
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            if self.warn.allow() {
                tracing::debug!(
                    "mcpd has no tokio runtime to export on; signals will be sent when you call flush()"
                );
            }
            return;
        };
        if self.worker_started.swap(true, Ordering::AcqRel) {
            return;
        }
        let handle = runtime.spawn(worker(
            Arc::downgrade(self),
            Arc::clone(&self.shared),
            self.config.flush_interval,
        ));
        *self.worker.lock().unwrap_or_else(|e| e.into_inner()) = Some(handle);
    }

    /// Drain the queue into batches and hand them to the sink, one at a time.
    ///
    /// Each batch is sent and counted on a task of its own that holds the send
    /// lock until the batch is done. A caller whose deadline passes drops this
    /// future and starts nothing more, while the batch already in flight
    /// finishes, is counted, and keeps the next one from starting beside it.
    async fn export_all(self: &Arc<Self>) -> Result<(), Error> {
        let Some(sink) = &self.sink else { return Ok(()) };
        let mut last: Option<Error> = None;
        loop {
            let one_at_a_time = Arc::clone(&self.send_lock).lock_owned().await;
            if self.abandoned.load(Ordering::SeqCst) {
                break;
            }
            let signals = self.shared.queue.drain();
            if signals.is_empty() {
                break;
            }
            let count = signals.len();
            let batch = to_batch(signals);
            let key = batch.ingest_key.clone().unwrap_or_default();
            let sink = Arc::clone(sink);
            let client = Arc::clone(self);
            let sent = tokio::spawn(async move {
                let _held = one_at_a_time;
                // On a task of its own, so a sink that panics is a failed
                // export rather than an unwind through whoever called `flush`
                // (B1).
                let outcome = tokio::spawn(async move { sink.export(batch, key).await }).await;
                let failure = match outcome {
                    Ok(Ok(())) => None,
                    Ok(Err(e)) => Some(e),
                    Err(join) => Some(Error::Export(format!("the sink panicked: {join}"))),
                };
                client.account(count, failure)
            });
            match sent.await {
                Ok(None) => {}
                Ok(Some(e)) => last = Some(e),
                Err(join) => last = Some(Error::Export(format!("the export task failed: {join}"))),
            }
        }
        match last {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// Count one batch's fate, and say so at most once per interval when it
    /// failed. Hands the failure back for the caller that is still waiting.
    fn account(&self, count: usize, failure: Option<Error>) -> Option<Error> {
        match failure {
            None => {
                self.exported.fetch_add(count as u64, Ordering::Relaxed);
                None
            }
            Some(e) => {
                self.failed.fetch_add(1, Ordering::Relaxed);
                if self.warn.allow() {
                    tracing::warn!(
                        error = %e,
                        signals = count,
                        endpoint = %self.config.endpoint,
                        "mcpd export failed; these signals are lost"
                    );
                }
                Some(e)
            }
        }
    }
}

/// What a client has done since it was built. The same seven names in every
/// SDK.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    /// Signals waiting to be exported.
    pub queued: usize,
    /// Signals the queue's bound has cost.
    pub dropped: u64,
    /// How many signals the queue can hold.
    pub capacity: usize,
    /// Signals the sink accepted.
    pub exported: u64,
    /// Batches the sink refused.
    pub failed: u64,
    /// Spans [`Mcpd::record`] refused.
    pub rejected: u64,
    /// Values the configured redactor panicked on, dropped rather than sent
    /// (contract section 2.1.1).
    pub redaction_failures: u64,
}

/// Why a recorded span is refused, if it is.
fn validate_record(span: &SpanRecord) -> Result<(), String> {
    ids::validate_trace_id(&span.trace_id).map_err(|e| format!("trace_id: {e}"))?;
    ids::validate_span_id(&span.span_id).map_err(|e| format!("span_id: {e}"))?;
    if let Some(parent) = &span.parent_span_id {
        ids::validate_span_id(parent).map_err(|e| format!("parent_span_id: {e}"))?;
    }
    if span.name.trim().is_empty() {
        return Err("name is empty".to_owned());
    }
    if span.start_time == OffsetDateTime::UNIX_EPOCH {
        return Err("start_time is missing".to_owned());
    }
    Ok(())
}

/// The background exporter.
///
/// It holds the queue strongly and the client weakly, so it keeps working while
/// the client lives and stops on its own once the client is gone.
async fn worker(client: Weak<ClientInner>, shared: Arc<Shared>, interval: std::time::Duration) {
    // A zero interval set in code makes every sleep return at once, and the
    // loop would spin a core exporting an empty queue. The default stands in.
    let interval = if interval.is_zero() { Config::default().flush_interval } else { interval };
    loop {
        let stop = tokio::select! {
            () = tokio::time::sleep(interval) => false,
            () = shared.queue.ready() => false,
            () = shared.stop.notified() => true,
        };
        let Some(client) = client.upgrade() else { return };
        // Errors are logged inside, at most once per interval. Nothing here can
        // fail in a way the traced program should ever hear about.
        let _ = client.export_all().await;
        if stop || client.stopped.load(Ordering::Relaxed) {
            return;
        }
    }
}

/// Which extras to fetch alongside a trace.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Include {
    /// Log records emitted inside the run.
    pub logs: bool,
    /// Machine evaluations of the run.
    pub scores: bool,
    /// Human judgements of the run.
    pub feedback: bool,
    /// Metric observations linked to the run.
    pub metrics: bool,
}

impl Include {
    /// Everything attached to the trace.
    #[must_use]
    pub const fn all() -> Self {
        Self { logs: true, scores: true, feedback: true, metrics: true }
    }

    fn as_param(self) -> Option<String> {
        let mut parts = Vec::new();
        if self.logs {
            parts.push("logs");
        }
        if self.scores {
            parts.push("scores");
        }
        if self.feedback {
            parts.push("feedback");
        }
        if self.metrics {
            parts.push("metrics");
        }
        (!parts.is_empty()).then(|| parts.join(","))
    }
}

fn rfc3339(t: OffsetDateTime) -> String {
    t.format(&time::format_description::well_known::Rfc3339).unwrap_or_default()
}

/// Push a parameter when it is set.
fn put<T: ToString>(
    params: &mut Vec<(&'static str, String)>,
    key: &'static str,
    value: &Option<T>,
) {
    if let Some(v) = value {
        params.push((key, v.to_string()));
    }
}

/// Which traces to list. Every field mirrors a query parameter of
/// `GET /v1/api/traces`, and every one of them defaults to absent.
#[derive(Debug, Clone, Default)]
pub struct TraceFilter {
    /// Full-text search across span names, prompts and completions.
    pub q: Option<String>,
    /// Only traces with this outcome.
    pub status: Option<SpanStatus>,
    /// Only traces whose root is this kind.
    pub kind: Option<SpanKind>,
    /// Exact root span name.
    pub name: Option<String>,
    /// Traces carrying all of these tags.
    pub tags: Vec<String>,
    /// Every trace in one conversation.
    pub session_id: Option<String>,
    /// Every trace in one thread.
    pub thread_id: Option<String>,
    /// Every trace for one end user.
    pub user_id: Option<String>,
    /// Restrict to one environment.
    pub environment: Option<String>,
    /// Traces that used this model.
    pub model: Option<String>,
    /// Start of the window, inclusive.
    pub from: Option<OffsetDateTime>,
    /// End of the window, exclusive.
    pub to: Option<OffsetDateTime>,
    /// Only traces at least this slow.
    pub min_duration_ms: Option<u64>,
    /// Only traces at most this slow.
    pub max_duration_ms: Option<u64>,
    /// Only traces at least this expensive.
    pub min_cost: Option<f64>,
    /// Only traces at most this expensive.
    pub max_cost: Option<f64>,
    /// Only traces that used at least this many tokens, input and output
    /// together.
    pub min_tokens: Option<u64>,
    /// Only traces carrying a score from this scorer.
    pub has_score: Option<String>,
    /// With `has_score`: only traces that scorer marked below this value.
    pub score_below: Option<f64>,
    /// With `has_score`: only traces that scorer marked above this value.
    pub score_above: Option<f64>,
    /// `true` for only traces a person gave feedback on, `false` for only
    /// traces with none.
    pub has_feedback: Option<bool>,
    /// `start_time`, `duration`, `cost`, `span_count`, `tokens` or `score`.
    pub sort: Option<String>,
    /// `asc` or `desc`.
    pub order: Option<String>,
    /// Page size, 1 to 500.
    pub limit: Option<u32>,
    /// Opaque cursor from a previous page.
    pub cursor: Option<String>,
}

impl TraceFilter {
    fn params(&self, project: &str) -> Vec<(&'static str, String)> {
        let mut p = vec![("project", project.to_owned())];
        put(&mut p, "q", &self.q);
        put(&mut p, "status", &self.status);
        put(&mut p, "kind", &self.kind);
        put(&mut p, "name", &self.name);
        if !self.tags.is_empty() {
            p.push(("tags", self.tags.join(",")));
        }
        put(&mut p, "session_id", &self.session_id);
        put(&mut p, "thread_id", &self.thread_id);
        put(&mut p, "user_id", &self.user_id);
        put(&mut p, "environment", &self.environment);
        put(&mut p, "model", &self.model);
        put(&mut p, "from", &self.from.map(rfc3339));
        put(&mut p, "to", &self.to.map(rfc3339));
        put(&mut p, "min_duration_ms", &self.min_duration_ms);
        put(&mut p, "max_duration_ms", &self.max_duration_ms);
        put(&mut p, "min_cost", &self.min_cost);
        put(&mut p, "max_cost", &self.max_cost);
        put(&mut p, "min_tokens", &self.min_tokens);
        put(&mut p, "has_score", &self.has_score);
        put(&mut p, "score_below", &self.score_below);
        put(&mut p, "score_above", &self.score_above);
        put(&mut p, "has_feedback", &self.has_feedback);
        put(&mut p, "sort", &self.sort);
        put(&mut p, "order", &self.order);
        put(&mut p, "limit", &self.limit);
        put(&mut p, "cursor", &self.cursor);
        p
    }
}

/// Which spans to search. Mirrors `GET /v1/api/spans`.
#[derive(Debug, Clone, Default)]
pub struct SpanFilter {
    /// Full-text search across name, input and output.
    pub q: Option<String>,
    /// Only this kind of step.
    pub kind: Option<SpanKind>,
    /// Only this outcome.
    pub status: Option<SpanStatus>,
    /// Exact span name.
    pub name: Option<String>,
    /// Only spans against this model.
    pub model: Option<String>,
    /// Restrict to one trace.
    pub trace_id: Option<String>,
    /// Only spans linked to this prompt by [`Span::set_prompt`].
    pub prompt: Option<String>,
    /// With `prompt`: only spans that used this version of it.
    pub prompt_version: Option<u32>,
    /// Start of the window.
    pub from: Option<OffsetDateTime>,
    /// End of the window.
    pub to: Option<OffsetDateTime>,
    /// Only spans at least this slow.
    pub min_duration_ms: Option<u64>,
    /// `start_time`, `duration` or `cost`.
    pub sort: Option<String>,
    /// `asc` or `desc`.
    pub order: Option<String>,
    /// Page size, 1 to 500.
    pub limit: Option<u32>,
    /// Opaque cursor.
    pub cursor: Option<String>,
}

impl SpanFilter {
    fn params(&self, project: &str) -> Vec<(&'static str, String)> {
        let mut p = vec![("project", project.to_owned())];
        put(&mut p, "q", &self.q);
        put(&mut p, "kind", &self.kind);
        put(&mut p, "status", &self.status);
        put(&mut p, "name", &self.name);
        put(&mut p, "model", &self.model);
        put(&mut p, "trace_id", &self.trace_id);
        put(&mut p, "prompt", &self.prompt);
        put(&mut p, "prompt_version", &self.prompt_version);
        put(&mut p, "from", &self.from.map(rfc3339));
        put(&mut p, "to", &self.to.map(rfc3339));
        put(&mut p, "min_duration_ms", &self.min_duration_ms);
        put(&mut p, "sort", &self.sort);
        put(&mut p, "order", &self.order);
        put(&mut p, "limit", &self.limit);
        put(&mut p, "cursor", &self.cursor);
        p
    }
}

/// Which logs to search. Mirrors `GET /v1/api/logs`.
#[derive(Debug, Clone, Default)]
pub struct LogFilter {
    /// Full-text search across the message and structured fields.
    pub q: Option<String>,
    /// Minimum severity, inclusive.
    pub level: Option<LogLevel>,
    /// Only logs from this run.
    pub trace_id: Option<String>,
    /// Only logs from this step.
    pub span_id: Option<String>,
    /// Start of the window.
    pub from: Option<OffsetDateTime>,
    /// End of the window.
    pub to: Option<OffsetDateTime>,
    /// Page size, 1 to 1000.
    pub limit: Option<u32>,
    /// Opaque cursor.
    pub cursor: Option<String>,
}

impl LogFilter {
    fn params(&self, project: &str) -> Vec<(&'static str, String)> {
        let mut p = vec![("project", project.to_owned())];
        put(&mut p, "q", &self.q);
        put(&mut p, "level", &self.level);
        put(&mut p, "trace_id", &self.trace_id);
        put(&mut p, "span_id", &self.span_id);
        put(&mut p, "from", &self.from.map(rfc3339));
        put(&mut p, "to", &self.to.map(rfc3339));
        put(&mut p, "limit", &self.limit);
        put(&mut p, "cursor", &self.cursor);
        p
    }
}

/// A metric query. Mirrors `GET /v1/api/metrics`.
#[derive(Debug, Clone)]
pub struct MetricQuery {
    /// Metric name.
    pub name: String,
    /// Start of the window.
    pub from: OffsetDateTime,
    /// End of the window.
    pub to: OffsetDateTime,
    /// Bucket width, such as `1m`, `5m` or `1h`. Chosen for you when absent.
    pub bucket: Option<String>,
    /// `sum`, `count`, `mean`, `min`, `max`, `p50`, `p90`, `p95` or `p99`.
    pub agg: Option<String>,
    /// Label keys to split the series on.
    pub group_by: Vec<String>,
    /// Keep points whose labels have these values. Sent as `key:value` pairs
    /// joined by commas, so a key cannot contain `:` or `,` and a value cannot
    /// contain `,`; [`Mcpd::query_metrics`] refuses either before a request is
    /// made.
    pub filter: Vec<(String, String)>,
}

impl MetricQuery {
    /// A query for one metric over a window, with the server's defaults for
    /// bucket width and aggregation.
    #[must_use]
    pub fn new(name: impl Into<String>, from: OffsetDateTime, to: OffsetDateTime) -> Self {
        Self {
            name: name.into(),
            from,
            to,
            bucket: None,
            agg: None,
            group_by: Vec::new(),
            filter: Vec::new(),
        }
    }

    fn params(&self, project: &str) -> Result<Vec<(&'static str, String)>, Error> {
        let mut p = vec![
            ("project", project.to_owned()),
            ("name", self.name.clone()),
            ("from", rfc3339(self.from)),
            ("to", rfc3339(self.to)),
        ];
        put(&mut p, "bucket", &self.bucket);
        put(&mut p, "agg", &self.agg);
        if !self.group_by.is_empty() {
            p.push(("group_by", self.group_by.join(",")));
        }
        if !self.filter.is_empty() {
            p.push(("filter", metric_filter(&self.filter)?));
        }
        Ok(p)
    }
}

/// Label filters the way the daemon reads them: `key:value` joined by commas.
///
/// A key or value the format cannot carry is refused rather than sent, because
/// the daemon would split it into a different filter and answer a different
/// question.
fn metric_filter(filter: &[(String, String)]) -> Result<String, Error> {
    let mut pairs = Vec::with_capacity(filter.len());
    for (key, value) in filter {
        if key.is_empty() || key.contains([':', ',']) {
            return Err(Error::Config(format!(
                "metric filter key {key:?} cannot be empty or contain ':' or ','"
            )));
        }
        if value.contains(',') {
            return Err(Error::Config(format!(
                "metric filter value {value:?} for {key:?} cannot contain ','"
            )));
        }
        pairs.push(format!("{key}:{value}"));
    }
    Ok(pairs.join(","))
}

/// Which scores to list. Mirrors `GET /v1/api/scores`.
#[derive(Debug, Clone, Default)]
pub struct ScoreFilter {
    /// Only this scorer.
    pub scorer_id: Option<String>,
    /// Only scores from this source.
    pub source: Option<ScoreSource>,
    /// Only scores for this run.
    pub trace_id: Option<String>,
    /// Lower bound, inclusive.
    pub min_value: Option<f64>,
    /// Upper bound, inclusive.
    pub max_value: Option<f64>,
    /// Start of the window.
    pub from: Option<OffsetDateTime>,
    /// End of the window.
    pub to: Option<OffsetDateTime>,
    /// Page size.
    pub limit: Option<u32>,
    /// Opaque cursor.
    pub cursor: Option<String>,
}

impl ScoreFilter {
    fn params(&self, project: &str) -> Vec<(&'static str, String)> {
        let mut p = vec![("project", project.to_owned())];
        put(&mut p, "scorer_id", &self.scorer_id);
        put(&mut p, "source", &self.source);
        put(&mut p, "trace_id", &self.trace_id);
        put(&mut p, "min_value", &self.min_value);
        put(&mut p, "max_value", &self.max_value);
        put(&mut p, "from", &self.from.map(rfc3339));
        put(&mut p, "to", &self.to.map(rfc3339));
        put(&mut p, "limit", &self.limit);
        put(&mut p, "cursor", &self.cursor);
        p
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn include_renders_the_comma_list_the_api_expects() {
        assert_eq!(Include::default().as_param(), None);
        assert_eq!(Include::all().as_param().as_deref(), Some("logs,scores,feedback,metrics"));
        assert_eq!(
            Include { scores: true, ..Include::default() }.as_param().as_deref(),
            Some("scores")
        );
    }

    #[test]
    fn a_default_filter_asks_only_for_a_project() {
        let p = TraceFilter::default().params("shop");
        assert_eq!(p, vec![("project", "shop".to_owned())]);
    }

    #[test]
    fn filters_render_enums_and_lists_the_way_the_api_reads_them() {
        let f = TraceFilter {
            status: Some(SpanStatus::Error),
            kind: Some(SpanKind::AgentRun),
            tags: vec!["billing".into(), "beta".into()],
            limit: Some(10),
            ..TraceFilter::default()
        };
        let p = f.params("shop");
        assert!(p.contains(&("status", "error".to_owned())));
        assert!(p.contains(&("kind", "agent_run".to_owned())));
        assert!(p.contains(&("tags", "billing,beta".to_owned())));
        assert!(p.contains(&("limit", "10".to_owned())));
    }

    #[test]
    fn a_metric_filter_goes_out_as_the_key_value_pairs_the_daemon_reads() {
        let mut q =
            MetricQuery::new("calls", OffsetDateTime::UNIX_EPOCH, OffsetDateTime::UNIX_EPOCH);
        q.filter = vec![("model".into(), "gpt-4o".into()), ("status".into(), "ok".into())];
        let p = q.params("shop").expect("a well-formed filter is sent");
        assert!(p.contains(&("filter", "model:gpt-4o,status:ok".to_owned())), "{p:?}");

        for (key, value) in [("a:b", "x"), ("a,b", "x"), ("", "x"), ("model", "a,b")] {
            q.filter = vec![(key.into(), value.into())];
            assert!(
                matches!(q.params("shop"), Err(Error::Config(_))),
                "{key:?}={value:?} cannot be written as key:value pairs and should be refused"
            );
        }
    }

    #[test]
    fn timestamps_go_out_as_rfc3339_in_utc() {
        let t = OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(1);
        assert_eq!(rfc3339(t), "1970-01-01T00:00:01Z");
    }
}
