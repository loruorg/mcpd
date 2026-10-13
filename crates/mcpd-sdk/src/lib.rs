//! # mcpd-sdk
//!
//! The Rust SDK for [mcpd](https://mcpd.sh). Instrument an agent and ship its
//! spans, logs, metrics, scores and feedback.
//!
//! ```no_run
//! use mcpd_sdk::{Mcpd, Scale, SpanKind, Target};
//!
//! #[tokio::main]
//! async fn main() {
//!     let mcpd = Mcpd::new();
//!
//!     let run = mcpd.trace("support agent");
//!     run.set_input("where is my order?");
//!
//!     let call = run.child_of("answer", SpanKind::ModelGeneration);
//!     call.set_model("claude-sonnet-4", "anthropic");
//!     call.set_tokens(412, 96);
//!     call.set_output("It shipped on Tuesday.");
//!     call.end();
//!
//!     run.set_output("It shipped on Tuesday.");
//!     mcpd.score(Target::trace(run.trace_id()), "helpfulness", 0.9, Scale::UNIT, None);
//!     run.end();
//!
//!     // Nothing is lost on the way out.
//!     let _ = mcpd.shutdown().await;
//! }
//! ```
//!
//! ## What this SDK guarantees
//!
//! * **It never fails your program.** No instrumentation call returns an error
//!   or panics. An unreachable mcpd, a serialisation failure, a full queue: all
//!   are logged at most once per flush interval and swallowed.
//! * **It never blocks your program.** Recording appends to an in-memory queue
//!   and returns. Export happens on a background task.
//! * **It never grows without bound.** The queue is capped. When it is full the
//!   oldest signals are discarded and [`Mcpd::dropped_signals`] counts them.
//! * **A trace is never half recorded.** The sampling decision is made once, at
//!   the root, and inherited by every child and every downstream service.
//!
//! Read [`Span`] for the tracing surface and [`Config`] for what is settable.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod client;
pub mod context;
pub mod logs;
pub mod model_call;
pub mod prompts;
mod queue;
pub mod runs;
pub mod sink;
pub mod span;
mod transport;
pub mod worker;
pub mod workflow;

pub use client::{
    Include, LogFilter, Mcpd, MetricQuery, ScoreFilter, SpanFilter, Stats, TraceFilter,
    DEFAULT_DEADLINE,
};
pub use context::{current, current_traceparent, SpanContext};
pub use logs::{
    error_fields, log_level_from_severity, meets_level, parse_log_level, severity_number,
};
pub use model_call::{
    response_facts, response_shape, usage_from_anthropic, usage_from_openai, ResponseFacts,
};
pub use prompts::{
    render_prompt, GetPrompt, PromptFallback, PromptRenderError, Rendered, DEFAULT_PROMPT_LABEL,
    DEFAULT_PROMPT_TTL,
};
pub use runs::{ClaimSteps, PublishEvent, RunFilter, RunResult, StartRun};
pub use sink::{ConsoleSink, RecordingSink, Sink};
pub use span::Span;
pub use worker::{StepContext, StepFailure, StepOutcome, Worker, WorkerBuilder, WorkerStats};
pub use workflow::{
    all, any, exists, not, when, Branch, Limits, Predicate, RetryPolicy, StepOptions, Workflow,
};

/// The generated data model, shared by the server, the store and every SDK.
pub use mcpd_core as model;
pub use mcpd_core::{
    AttemptStatus, Backoff, ConflictReason, CreatePromptRequest, Definition, DefinitionDocument,
    DefinitionVersion, Effect, EntityKind, EventOutcome, FeedbackKind, LogLevel, MetricKind,
    OnError, Prompt, PromptKind, Run, RunDetail, RunEvent, RunEventKind, RunStatus, ScoreSource,
    SpanError, SpanKind, SpanStatus, StepAttempt, StepConflict, StepError, StepErrorClass,
    StepKind, StepLease, StepUsage, TokenUsage, WaitTimeout,
};

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Compiles every Rust block in the README as a doctest, so the first thing a
/// reader copies is the first thing that would break the build if it were
/// wrong. Present only when doctests run.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
pub struct Readme;

fn serde_plain_status(status: &mcpd_core::RunStatus) -> String {
    serde_json::to_value(status)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// Anything the SDK can report to a caller who asked for an answer.
///
/// Nothing on the instrumentation path returns one of these. They come from the
/// read methods, from [`Mcpd::flush`] and from [`Mcpd::shutdown`], which are
/// calls a program makes deliberately.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The request could not be made or the response could not be read.
    #[error("mcpd could not be reached: {0}")]
    Http(#[from] reqwest::Error),

    /// mcpd answered, and refused.
    #[error("mcpd returned {status}: {message}")]
    Api {
        /// HTTP status code.
        status: u16,
        /// What mcpd said was wrong.
        message: String,
    },

    /// A worker call the daemon refused with one of its four closed reasons.
    /// The reason says what to do next; see the run contract's 409 table.
    #[error("mcpd refused the step call: {}", .0.message)]
    Conflict(Box<mcpd_core::StepConflict>),

    /// A run that ended `failed`, `timed_out` or `cancelled`, from
    /// [`Mcpd::wait_run`], carrying the run as it ended.
    #[error("run {} {}{}", .0.run_id, serde_plain_status(&.0.status), .0.error.as_deref().map(|e| format!(": {e}")).unwrap_or_default())]
    RunFailed(Box<mcpd_core::Run>),

    /// A payload could not be turned into JSON.
    #[error("could not encode the batch: {0}")]
    Encode(#[from] serde_json::Error),

    /// Every attempt to export a batch failed.
    #[error("export failed: {0}")]
    Export(String),

    /// A worker hook the user supplied panicked. Reported through the
    /// worker's `on_error`; the serve loop goes on.
    #[error("a worker hook panicked: {0}")]
    Hook(String),

    /// The client is misconfigured, or disabled and being asked to read.
    #[error("{0}")]
    Config(String),
}

/// How a client is configured.
///
/// Every field has a default that works against a local mcpd, and every field
/// can be set by environment variable. Code set after [`Config::from_env`] wins
/// over the environment, which is what makes a test able to override a
/// deployment's settings.
#[derive(Clone)]
pub struct Config {
    /// Where mcpd is listening. `MCPD_ENDPOINT`.
    pub endpoint: String,
    /// Sent as `Authorization: Bearer`. `MCPD_API_KEY`.
    pub api_key: Option<String>,
    /// The project every signal belongs to. `MCPD_PROJECT`.
    pub project: String,
    /// Deployment environment, such as `production`. `MCPD_ENVIRONMENT`.
    pub environment: Option<String>,
    /// The emitting service. `MCPD_SERVICE_NAME`.
    pub service_name: Option<String>,
    /// Its version, so a regression can be pinned to a release.
    /// `MCPD_SERVICE_VERSION`.
    pub service_version: Option<String>,
    /// Fraction of traces to record, 0.0 to 1.0. `MCPD_SAMPLE`.
    pub sample: f64,
    /// How many signals go in one batch. `MCPD_BATCH_SIZE`.
    pub batch_size: usize,
    /// How long the worker waits before exporting a partial batch.
    /// `MCPD_FLUSH_INTERVAL_MS`.
    pub flush_interval: Duration,
    /// The queue's bound. `MCPD_MAX_QUEUE`.
    pub max_queue: usize,
    /// Per-request HTTP timeout. `MCPD_TIMEOUT_MS`.
    pub timeout: Duration,
    /// When false every call is a no-op that still returns valid objects, so a
    /// test can turn mcpd off without touching the code under test.
    /// `MCPD_ENABLED`.
    pub enabled: bool,
    /// Whether a span's `input` and `output` may leave the process. When
    /// false, spans keep their timing, status, usage, model, correlation and
    /// errors, and the payloads are dropped before they reach the queue. A
    /// span can override it for itself with [`Span::capture_payloads`].
    /// `MCPD_CAPTURE_PAYLOADS`.
    pub capture_payloads: bool,
    /// The lowest level [`Mcpd::log`](crate::Mcpd::log) records. A line below
    /// it is dropped at the call, before the record is built. The default is
    /// `debug`, which keeps every level the spec defines except `trace`.
    /// `MCPD_LOG_LEVEL`.
    pub log_level: LogLevel,
    /// Where drained batches go. `None` is the HTTP transport, the only sink
    /// that talks to a daemon. A [`RecordingSink`] keeps batches in memory
    /// for a test; a [`ConsoleSink`] prints them. Chosen in code only.
    pub sink: Option<Arc<dyn Sink>>,
    /// Applied to every span's `input`, `output`, `attributes` and
    /// `metadata`, and to every log line's `fields`, before the record is
    /// queued. It returns the value to keep; `Value::Null` drops the field. A
    /// redactor that panics, or returns something other than an object for
    /// `attributes`, `metadata` or `fields`, drops that value and counts it in
    /// [`Stats::redaction_failures`]: a broken redactor never lets the
    /// original through (SDK contract 2.1.1). Set in code only, with
    /// [`Config::with_redact`].
    pub redact: Option<Redactor>,
}

/// A function that rewrites a payload before it leaves the process. See
/// [`Config::redact`].
pub type Redactor = Arc<dyn Fn(serde_json::Value) -> serde_json::Value + Send + Sync>;

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("endpoint", &self.endpoint)
            .field("api_key", &self.api_key.as_ref().map(|_| "<set>"))
            .field("project", &self.project)
            .field("environment", &self.environment)
            .field("service_name", &self.service_name)
            .field("service_version", &self.service_version)
            .field("sample", &self.sample)
            .field("batch_size", &self.batch_size)
            .field("flush_interval", &self.flush_interval)
            .field("max_queue", &self.max_queue)
            .field("timeout", &self.timeout)
            .field("enabled", &self.enabled)
            .field("capture_payloads", &self.capture_payloads)
            .field("log_level", &self.log_level)
            .field("sink", &self.sink.as_ref().map(|_| "<custom>"))
            .field("redact", &self.redact.as_ref().map(|_| "<set>"))
            .finish()
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:4317".to_owned(),
            api_key: None,
            project: "default".to_owned(),
            environment: None,
            service_name: None,
            service_version: None,
            sample: 1.0,
            batch_size: 128,
            flush_interval: Duration::from_secs(2),
            max_queue: 8192,
            timeout: Duration::from_secs(10),
            enabled: true,
            capture_payloads: true,
            log_level: LogLevel::Debug,
            sink: None,
            redact: None,
        }
    }
}

impl Config {
    /// Read the configuration from the process environment, falling back to the
    /// defaults for anything unset.
    #[must_use]
    pub fn from_env() -> Self {
        Self::from_source(|key| std::env::var(key).ok())
    }

    /// The body of [`Config::from_env`], with the environment injected.
    ///
    /// Split out so the parsing can be tested without mutating process-wide
    /// state that other tests are reading at the same time.
    fn from_source(get: impl Fn(&str) -> Option<String>) -> Self {
        let mut c = Self::default();
        if let Some(v) = get("MCPD_ENDPOINT") {
            c.endpoint = v;
        }
        c.api_key = get("MCPD_API_KEY").or(c.api_key);
        if let Some(v) = get("MCPD_PROJECT") {
            c.project = v;
        }
        c.environment = get("MCPD_ENVIRONMENT").or(c.environment);
        c.service_name = get("MCPD_SERVICE_NAME").or(c.service_name);
        c.service_version = get("MCPD_SERVICE_VERSION").or(c.service_version);
        if let Some(v) = parse_env("MCPD_SAMPLE", &get) {
            c.sample = f64::clamp(v, 0.0, 1.0);
        }
        // Zero is refused along with anything unparseable, as every other SDK
        // refuses it: a batch of none or a queue of none stops the exporter,
        // an interval of none spins it, and a typo in a deployment variable
        // must do neither.
        if let Some(v) = parse_env::<usize>("MCPD_BATCH_SIZE", &get).filter(|v| *v > 0) {
            c.batch_size = v;
        }
        if let Some(v) = parse_env::<u64>("MCPD_FLUSH_INTERVAL_MS", &get).filter(|v| *v > 0) {
            c.flush_interval = Duration::from_millis(v);
        }
        if let Some(v) = parse_env::<usize>("MCPD_MAX_QUEUE", &get).filter(|v| *v > 0) {
            c.max_queue = v;
        }
        if let Some(v) = parse_env::<u64>("MCPD_TIMEOUT_MS", &get).filter(|v| *v > 0) {
            c.timeout = Duration::from_millis(v);
        }
        if let Some(v) = parse_bool("MCPD_ENABLED", &get) {
            c.enabled = v;
        }
        if let Some(v) = parse_bool("MCPD_CAPTURE_PAYLOADS", &get) {
            c.capture_payloads = v;
        }
        // A name outside the enum is ignored rather than refused: a typo in a
        // deployment variable must not silence a service's logs.
        if let Some(v) = get("MCPD_LOG_LEVEL").as_deref().and_then(parse_log_level) {
            c.log_level = v;
        }
        c
    }

    /// Set the endpoint.
    #[must_use]
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = endpoint.into();
        self
    }

    /// Set the API key.
    #[must_use]
    pub fn with_api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }

    /// Set the project.
    #[must_use]
    pub fn with_project(mut self, project: impl Into<String>) -> Self {
        self.project = project.into();
        self
    }

    /// Set the environment.
    #[must_use]
    pub fn with_environment(mut self, environment: impl Into<String>) -> Self {
        self.environment = Some(environment.into());
        self
    }

    /// Name the service, and optionally its version.
    #[must_use]
    pub fn with_service(mut self, name: impl Into<String>, version: Option<&str>) -> Self {
        self.service_name = Some(name.into());
        self.service_version = version.map(str::to_owned);
        self
    }

    /// Record this fraction of traces, 0.0 to 1.0.
    #[must_use]
    pub fn with_sample(mut self, sample: f64) -> Self {
        self.sample = sample.clamp(0.0, 1.0);
        self
    }

    /// Turn the client on or off.
    #[must_use]
    pub const fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Whether `input` and `output` may leave the process. Off, spans still
    /// carry their timing, status, usage, model, correlation and errors.
    #[must_use]
    pub const fn with_capture_payloads(mut self, capture: bool) -> Self {
        self.capture_payloads = capture;
        self
    }

    /// The lowest level `log` records. Anything below it never reaches the
    /// queue.
    #[must_use]
    pub const fn with_log_level(mut self, level: LogLevel) -> Self {
        self.log_level = level;
        self
    }

    /// Send drained batches somewhere other than the HTTP transport.
    #[must_use]
    pub fn with_sink(mut self, sink: Arc<dyn Sink>) -> Self {
        self.sink = Some(sink);
        self
    }

    /// Rewrite payloads before they are queued: mask an account number, drop
    /// a field. See [`Config::redact`] for what it covers and how it fails.
    #[must_use]
    pub fn with_redact(
        mut self,
        redact: impl Fn(serde_json::Value) -> serde_json::Value + Send + Sync + 'static,
    ) -> Self {
        self.redact = Some(Arc::new(redact));
        self
    }
}

/// Read one boolean environment variable, ignoring anything unreadable.
fn parse_bool(key: &str, get: &impl Fn(&str) -> Option<String>) -> Option<bool> {
    let raw = get(key)?;
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        other => {
            tracing::warn!(key, value = other, "this setting is not a boolean; using the default");
            None
        }
    }
}

/// Read and parse one environment variable, ignoring anything unreadable.
///
/// A typo in a deployment's environment must not stop the program: the default
/// is always a working value, and a warning says what was skipped.
fn parse_env<T: std::str::FromStr>(key: &str, get: &impl Fn(&str) -> Option<String>) -> Option<T> {
    let raw = get(key)?;
    match raw.trim().parse::<T>() {
        Ok(v) => Some(v),
        Err(_) => {
            tracing::warn!(key, value = %raw, "mcpd could not read this setting; using the default");
            None
        }
    }
}

/// The declared range a score is measured on.
///
/// Required, and not optional, wherever a score is recorded: a bare number with
/// no scale cannot be compared to anything, and by the time someone is reading
/// the chart it is too late to ask what 7 meant.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scale {
    /// Lower bound.
    pub min: f64,
    /// Upper bound.
    pub max: f64,
}

impl Scale {
    /// The usual one: 0.0 to 1.0.
    pub const UNIT: Self = Self { min: 0.0, max: 1.0 };

    /// A scale with explicit bounds, such as 1 to 5 stars.
    #[must_use]
    pub const fn new(min: f64, max: f64) -> Self {
        Self { min, max }
    }
}

/// What a score or a piece of feedback is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A whole run.
    Trace(String),
    /// One step inside a run.
    Span {
        /// The run the step belongs to.
        trace_id: String,
        /// The step.
        span_id: String,
    },
}

impl Target {
    /// A whole run.
    #[must_use]
    pub fn trace(trace_id: impl Into<String>) -> Self {
        Self::Trace(trace_id.into())
    }

    /// One step inside a run.
    #[must_use]
    pub fn span(trace_id: impl Into<String>, span_id: impl Into<String>) -> Self {
        Self::Span { trace_id: trace_id.into(), span_id: span_id.into() }
    }

    /// The run this target belongs to.
    #[must_use]
    pub fn trace_id(&self) -> &str {
        match self {
            Self::Trace(t) | Self::Span { trace_id: t, .. } => t,
        }
    }

    /// The step, when the target is one.
    #[must_use]
    pub fn span_id(&self) -> Option<&str> {
        match self {
            Self::Trace(_) => None,
            Self::Span { span_id, .. } => Some(span_id),
        }
    }
}

impl From<&Span> for Target {
    fn from(span: &Span) -> Self {
        Self::span(span.trace_id(), span.span_id())
    }
}

impl From<&SpanContext> for Target {
    fn from(ctx: &SpanContext) -> Self {
        Self::span(&ctx.trace_id, &ctx.span_id)
    }
}

/// A rate limiter for log messages.
///
/// Everything mcpd complains about is something that will be true for every
/// signal until it is fixed. Saying it once per interval keeps a failed export
/// from becoming a second outage in the log pipeline.
#[derive(Debug)]
pub(crate) struct Throttle {
    interval: Duration,
    last: Mutex<Option<Instant>>,
}

impl Throttle {
    pub(crate) const fn new(interval: Duration) -> Self {
        Self { interval, last: Mutex::new(None) }
    }

    /// Whether the caller should log this time.
    pub(crate) fn allow(&self) -> bool {
        let now = Instant::now();
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        match *last {
            Some(t) if now.duration_since(t) < self.interval => false,
            _ => {
                *last = Some(now);
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_config_talks_to_a_local_mcpd() {
        let c = Config::default();
        assert_eq!(c.endpoint, "http://localhost:4317");
        assert_eq!(c.project, "default");
        assert_eq!(c.batch_size, 128);
        assert_eq!(c.max_queue, 8192);
        assert_eq!(c.flush_interval, Duration::from_secs(2));
        assert_eq!(c.timeout, Duration::from_secs(10));
        assert!((c.sample - 1.0).abs() < f64::EPSILON);
        assert!(c.enabled);
        assert!(c.capture_payloads);
        assert!(c.sink.is_none());
    }

    #[test]
    fn the_environment_sets_every_field() {
        let env = |key: &str| {
            Some(
                match key {
                    "MCPD_ENDPOINT" => "http://mcpd.internal:4317",
                    "MCPD_API_KEY" => "k-1",
                    "MCPD_PROJECT" => "shop",
                    "MCPD_ENVIRONMENT" => "production",
                    "MCPD_SERVICE_NAME" => "checkout",
                    "MCPD_SERVICE_VERSION" => "1.4.0",
                    "MCPD_SAMPLE" => "0.25",
                    "MCPD_BATCH_SIZE" => "16",
                    "MCPD_FLUSH_INTERVAL_MS" => "500",
                    "MCPD_MAX_QUEUE" => "32",
                    "MCPD_TIMEOUT_MS" => "1500",
                    "MCPD_ENABLED" => "false",
                    "MCPD_CAPTURE_PAYLOADS" => "off",
                    _ => return None,
                }
                .to_owned(),
            )
        };
        let c = Config::from_source(env);
        assert_eq!(c.endpoint, "http://mcpd.internal:4317");
        assert_eq!(c.api_key.as_deref(), Some("k-1"));
        assert_eq!(c.project, "shop");
        assert_eq!(c.environment.as_deref(), Some("production"));
        assert_eq!(c.service_name.as_deref(), Some("checkout"));
        assert_eq!(c.service_version.as_deref(), Some("1.4.0"));
        assert!((c.sample - 0.25).abs() < f64::EPSILON);
        assert_eq!(c.batch_size, 16);
        assert_eq!(c.flush_interval, Duration::from_millis(500));
        assert_eq!(c.max_queue, 32);
        assert_eq!(c.timeout, Duration::from_millis(1500));
        assert!(!c.enabled);
        assert!(!c.capture_payloads);
    }

    #[test]
    fn a_zero_tuning_variable_is_refused_and_the_default_kept() {
        // An interval of zero spun the export loop; a batch or a queue of zero
        // stopped it. Every other SDK refuses zero the same way.
        let env = |key: &str| {
            matches!(
                key,
                "MCPD_BATCH_SIZE" | "MCPD_FLUSH_INTERVAL_MS" | "MCPD_MAX_QUEUE" | "MCPD_TIMEOUT_MS"
            )
            .then(|| "0".to_owned())
        };
        let c = Config::from_source(env);
        let d = Config::default();
        assert_eq!(c.batch_size, d.batch_size);
        assert_eq!(c.flush_interval, d.flush_interval);
        assert_eq!(c.max_queue, d.max_queue);
        assert_eq!(c.timeout, d.timeout);
    }

    #[test]
    fn an_unreadable_setting_falls_back_rather_than_failing() {
        let c = Config::from_source(|key| match key {
            "MCPD_SAMPLE" => Some("very often".to_owned()),
            "MCPD_MAX_QUEUE" => Some("".to_owned()),
            "MCPD_ENABLED" => Some("maybe".to_owned()),
            "MCPD_CAPTURE_PAYLOADS" => Some("sometimes".to_owned()),
            _ => None,
        });
        assert!((c.sample - 1.0).abs() < f64::EPSILON);
        assert_eq!(c.max_queue, 8192);
        assert!(c.enabled);
        assert!(c.capture_payloads);
    }

    #[test]
    fn code_wins_over_the_environment() {
        let c = Config::from_source(|k| (k == "MCPD_PROJECT").then(|| "from-env".to_owned()))
            .with_project("from-code");
        assert_eq!(c.project, "from-code");
    }

    #[test]
    fn a_sample_outside_the_unit_range_is_clamped() {
        assert!((Config::default().with_sample(4.0).sample - 1.0).abs() < f64::EPSILON);
        assert!(Config::default().with_sample(-1.0).sample.abs() < f64::EPSILON);
    }

    #[test]
    fn a_target_knows_its_trace_and_its_step() {
        let t = Target::trace("a");
        assert_eq!(t.trace_id(), "a");
        assert_eq!(t.span_id(), None);
        let s = Target::span("a", "b");
        assert_eq!(s.trace_id(), "a");
        assert_eq!(s.span_id(), Some("b"));
    }

    #[test]
    fn the_throttle_lets_one_message_through_per_interval() {
        let t = Throttle::new(Duration::from_secs(60));
        assert!(t.allow(), "the first message should always be logged");
        assert!(!t.allow());
        assert!(!t.allow());

        let open = Throttle::new(Duration::ZERO);
        assert!(open.allow());
        assert!(open.allow(), "a zero interval should never suppress");
    }
}
