//! The worker: the user's own process, serving step bodies to the daemon.
//!
//! It registers the workflows it holds, long-polls for leases, and for each
//! lease posts `started`, heartbeats while the body runs, and posts `complete`
//! or `fail`. Every 409 the daemon can answer is handled by its reason, because
//! a worker that collapses them into one branch abandons steps it has already
//! finished or reports a failure for a success.
//!
//! Nothing here runs a model or reads a stored prompt. The daemon hands the
//! worker a lease with the rendered input and the prior steps; the body does
//! the rest with the crates, the pool and the credentials this process holds.

use std::collections::HashMap;
use std::fmt;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mcpd_core::{
    ConflictReason, DefinitionDocument, SpanKind, StepError, StepErrorClass, StepLease, StepResult,
    StepUsage, TokenUsage,
};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use tokio::sync::Notify;

use crate::client::Mcpd;
use crate::context::SpanContext;
use crate::runs::ClaimSteps;
use crate::span::{Span, UsageSink};
use crate::workflow::{StepHandler, Workflow};
use crate::Error;

/// A failure with a class, returned from a step body to tell the daemon how
/// to treat it.
#[derive(Debug, Clone)]
pub struct StepFailure {
    /// Whether it is retried, and how the run reads it.
    pub class: StepErrorClass,
    /// Sent to the daemon and shown in the journal. A stack trace is not.
    pub message: String,
    /// The least the daemon waits before the next attempt.
    pub retry_after: Option<Duration>,
    /// Anything structured worth keeping with the failure.
    pub details: Option<Value>,
}

impl StepFailure {
    /// A failure of the given class.
    #[must_use]
    pub fn new(class: StepErrorClass, message: impl Into<String>) -> Self {
        Self { class, message: message.into(), retry_after: None, details: None }
    }

    /// Another attempt would fail the same way. No retry.
    #[must_use]
    pub fn fatal(message: impl Into<String>) -> Self {
        Self::new(StepErrorClass::Fatal, message)
    }

    /// Worth another attempt: the daemon retries while attempts remain.
    #[must_use]
    pub fn retryable(message: impl Into<String>) -> Self {
        Self::new(StepErrorClass::Retryable, message)
    }

    /// The step's input cannot be acted on. No retry.
    #[must_use]
    pub fn invalid_input(message: impl Into<String>) -> Self {
        Self::new(StepErrorClass::InvalidInput, message)
    }

    /// The least the daemon waits before the next attempt.
    #[must_use]
    pub fn retry_after(mut self, wait: Duration) -> Self {
        self.retry_after = Some(wait);
        self
    }

    /// Structured detail kept with the failure.
    #[must_use]
    pub fn details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }

    fn wire(self) -> StepError {
        StepError {
            class: self.class,
            message: self.message,
            details: self.details,
            retry_after_ms: self
                .retry_after
                .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX)),
        }
    }
}

impl fmt::Display for StepFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for StepFailure {}

impl From<Error> for StepFailure {
    fn from(e: Error) -> Self {
        Self::fatal(e.to_string())
    }
}

impl From<serde_json::Error> for StepFailure {
    fn from(e: serde_json::Error) -> Self {
        Self::fatal(e.to_string())
    }
}

impl From<String> for StepFailure {
    fn from(message: String) -> Self {
        Self::fatal(message)
    }
}

impl From<&str> for StepFailure {
    fn from(message: &str) -> Self {
        Self::fatal(message)
    }
}

/// What the spans recorded under a step have spent so far.
#[derive(Debug, Default)]
struct Accumulated {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    total_tokens: Option<u64>,
    model: Option<String>,
    provider: Option<String>,
}

impl Accumulated {
    /// One span's usage, added once. Fields the span did not report leave
    /// the sum untouched; the last model and provider seen win.
    fn add(&mut self, usage: &TokenUsage, model: Option<&str>, provider: Option<&str>) {
        fn sum(into: &mut Option<u64>, value: Option<u64>) {
            if let Some(v) = value {
                *into = Some(into.unwrap_or(0).saturating_add(v));
            }
        }
        sum(&mut self.input_tokens, usage.input_tokens);
        sum(&mut self.output_tokens, usage.output_tokens);
        sum(&mut self.total_tokens, usage.total_tokens);
        if let Some(m) = model.filter(|m| !m.is_empty()) {
            self.model = Some(m.to_owned());
        }
        if let Some(p) = provider.filter(|p| !p.is_empty()) {
            self.provider = Some(p.to_owned());
        }
    }

    fn is_empty(&self) -> bool {
        self.input_tokens.is_none()
            && self.output_tokens.is_none()
            && self.total_tokens.is_none()
            && self.model.is_none()
            && self.provider.is_none()
    }
}

struct ContextInner {
    mcpd: Mcpd,
    lease: StepLease,
    cancelled: AtomicBool,
    cancel_reason: Mutex<Option<String>>,
    cancel: Notify,
    state: Mutex<Option<Map<String, Value>>>,
    /// What `set_usage` was given: the explicit override.
    explicit: Mutex<Option<StepUsage>>,
    /// What the spans under this step reported. Shared with the usage sink
    /// each span holds, and nothing else, so a span never keeps the context
    /// alive.
    accumulated: Arc<Mutex<Accumulated>>,
}

/// What a step body is handed: the lease, the run's state, and the means to
/// report usage, record spans and notice a cancellation.
///
/// Cheap to clone; every clone is the same step.
#[derive(Clone)]
pub struct StepContext {
    inner: Arc<ContextInner>,
}

impl fmt::Debug for StepContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StepContext")
            .field("run_id", &self.inner.lease.run_id)
            .field("step_key", &self.inner.lease.step_key)
            .field("attempt", &self.inner.lease.attempt)
            .finish_non_exhaustive()
    }
}

impl StepContext {
    fn new(mcpd: Mcpd, lease: StepLease) -> Self {
        Self {
            inner: Arc::new(ContextInner {
                mcpd,
                lease,
                cancelled: AtomicBool::new(false),
                cancel_reason: Mutex::new(None),
                cancel: Notify::new(),
                state: Mutex::new(None),
                explicit: Mutex::new(None),
                accumulated: Arc::new(Mutex::new(Accumulated::default())),
            }),
        }
    }

    /// The sink every span under this step reports its usage to.
    fn usage_sink(&self) -> UsageSink {
        let accumulated = Arc::clone(&self.inner.accumulated);
        Arc::new(move |usage, model, provider| {
            accumulated.lock().unwrap_or_else(|e| e.into_inner()).add(usage, model, provider);
        })
    }

    /// The lease, in full.
    #[must_use]
    pub fn lease(&self) -> &StepLease {
        &self.inner.lease
    }

    /// Which run.
    #[must_use]
    pub fn run_id(&self) -> &str {
        &self.inner.lease.run_id
    }

    /// Which step, with any loop or item index folded in.
    #[must_use]
    pub fn step_key(&self) -> &str {
        &self.inner.lease.step_key
    }

    /// Which attempt, from zero.
    #[must_use]
    pub fn attempt(&self) -> u32 {
        self.inner.lease.attempt
    }

    /// A key for a third party that accepts one. Stable across retries of
    /// this step.
    #[must_use]
    pub fn idempotency_key(&self) -> &str {
        &self.inner.lease.idempotency_key
    }

    /// The step's span, which the daemon opened; record your own spans
    /// under it.
    #[must_use]
    pub fn parent(&self) -> SpanContext {
        SpanContext {
            trace_id: self.inner.lease.trace_id.clone(),
            span_id: self.inner.lease.span_id.clone(),
            sampled: true,
        }
    }

    /// The step's rendered input, decoded.
    ///
    /// # Errors
    /// A `StepFailure` classed `invalid_input` when the input is not a `T`.
    pub fn input<T: DeserializeOwned>(&self) -> Result<T, StepFailure> {
        decode(self.inner.lease.input.clone().unwrap_or(Value::Null), "input")
    }

    /// The steps that already ended, keyed by step id, decoded.
    ///
    /// # Errors
    /// A `StepFailure` classed `invalid_input` when the prior is not a `T`.
    pub fn prior<T: DeserializeOwned>(&self) -> Result<T, StepFailure> {
        decode(self.inner.lease.prior.clone(), "prior")
    }

    /// The run's accumulated state as of this lease, decoded.
    ///
    /// # Errors
    /// A `StepFailure` classed `invalid_input` when the state is not a `T`.
    pub fn state<T: DeserializeOwned>(&self) -> Result<T, StepFailure> {
        decode(self.inner.lease.state.clone().unwrap_or(Value::Null), "state")
    }

    /// Merge keys into the run's state when the step completes. Top-level
    /// keys replace. A patch that is not an object is ignored.
    pub fn set_state(&self, patch: Value) {
        if let Value::Object(fields) = patch {
            let mut state = self.inner.state.lock().unwrap_or_else(|e| e.into_inner());
            state.get_or_insert_with(Map::new).extend(fields);
        }
    }

    /// Report the tokens this step spent, so the daemon prices and charges
    /// them.
    ///
    /// Usually unnecessary: every span recorded through [`StepContext::span`],
    /// and every span nested under one, adds its usage to the step when it
    /// ends, with the last model and provider seen. This call is the explicit
    /// override for a body that records no spans: the fields it sets replace
    /// the accumulated ones, and fields set here replace fields set earlier.
    pub fn set_usage(&self, usage: StepUsage) {
        let mut current = self.inner.explicit.lock().unwrap_or_else(|e| e.into_inner());
        let merged = current.take().unwrap_or_default();
        *current = Some(StepUsage {
            input_tokens: usage.input_tokens.or(merged.input_tokens),
            output_tokens: usage.output_tokens.or(merged.output_tokens),
            total_tokens: usage.total_tokens.or(merged.total_tokens),
            input_details: usage.input_details.or(merged.input_details),
            output_details: usage.output_details.or(merged.output_details),
            model: usage.model.or(merged.model),
            provider: usage.provider.or(merged.provider),
        });
    }

    /// [`StepContext::set_usage`] for the two counts most bodies have.
    pub fn set_tokens(&self, input_tokens: u64, output_tokens: u64) {
        self.set_usage(StepUsage {
            input_tokens: Some(input_tokens),
            output_tokens: Some(output_tokens),
            ..StepUsage::default()
        });
    }

    /// A span nested under this step in the run's trace, carrying the run's
    /// correlation. End it, or scope a future under it, as with any span.
    /// Its usage, and the usage of every span nested under it, is added to
    /// the step's when it ends.
    #[must_use]
    pub fn span(&self, name: impl Into<String>, kind: SpanKind) -> Span {
        let span = self.inner.mcpd.trace_from(name, Some(&self.parent().traceparent()), kind);
        span.adopt_usage_sink(Some(self.usage_sink()));
        let c = &self.inner.lease.correlation;
        if let Some(v) = &c.session_id {
            span.set_session(v.clone());
        }
        if let Some(v) = &c.thread_id {
            span.set_thread(v.clone());
        }
        if let Some(v) = &c.user_id {
            span.set_user(v.clone());
        }
        if let Some(v) = &c.request_id {
            span.set_request(v.clone());
        }
        if let Some(v) = &c.release {
            span.set_release(v.clone());
        }
        span
    }

    /// True once the daemon has fenced this lease or ended the run.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::Acquire)
    }

    /// Why, once [`StepContext::is_cancelled`] is true.
    #[must_use]
    pub fn cancel_reason(&self) -> Option<String> {
        self.inner.cancel_reason.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Resolves when the lease is lost or the run ends. Select on it next to
    /// the body's own I/O to stop when the daemon has moved on.
    pub async fn cancelled(&self) {
        loop {
            // Registered before the flag is read: `notify_waiters` wakes only
            // the waiters already registered, so a cancel landing between a
            // read and a registration would otherwise be missed for good.
            let notified = self.inner.cancel.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.is_cancelled() {
                return;
            }
            notified.await;
        }
    }

    fn cancel(&self, reason: String) {
        *self.inner.cancel_reason.lock().unwrap_or_else(|e| e.into_inner()) = Some(reason);
        self.inner.cancelled.store(true, Ordering::Release);
        self.inner.cancel.notify_waiters();
    }

    fn collected(&self) -> (Option<Value>, Option<StepUsage>) {
        let state = self.inner.state.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let explicit = self.inner.explicit.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let usage = {
            let acc = self.inner.accumulated.lock().unwrap_or_else(|e| e.into_inner());
            if acc.is_empty() && explicit.is_none() {
                None
            } else {
                // The explicit override wins field by field over the sum.
                let e = explicit.unwrap_or_default();
                Some(StepUsage {
                    input_tokens: e.input_tokens.or(acc.input_tokens),
                    output_tokens: e.output_tokens.or(acc.output_tokens),
                    total_tokens: e.total_tokens.or(acc.total_tokens),
                    input_details: e.input_details,
                    output_details: e.output_details,
                    model: e.model.or_else(|| acc.model.clone()),
                    provider: e.provider.or_else(|| acc.provider.clone()),
                })
            }
        };
        (state.map(Value::Object), usage)
    }
}

/// How a step body ended, as handed to `on_step_end`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepOutcome {
    /// The body returned an output and `complete` is being posted.
    Completed,
    /// The body failed, or the worker failed it, and `fail` is being posted.
    Failed {
        /// How the daemon reads it.
        class: StepErrorClass,
        /// What is sent.
        message: String,
    },
}

/// What a worker has done since it was built.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkerStats {
    /// Leases taken from the daemon.
    pub claimed: u64,
    /// Bodies running right now.
    pub inflight: usize,
    /// Bodies that returned an output.
    pub completed: u64,
    /// Bodies that failed, by error class as it went on the wire.
    pub failed: HashMap<String, u64>,
    /// 409s applied, by the daemon's reason.
    pub conflicts: HashMap<String, u64>,
    /// Leases handed back before the body, for a definition, version or step
    /// this worker does not hold.
    pub unservable: u64,
}

fn decode<T: DeserializeOwned>(value: Value, what: &str) -> Result<T, StepFailure> {
    serde_json::from_value(value).map_err(|e| {
        StepFailure::invalid_input(format!("the step's {what} could not be read: {e}"))
    })
}

/// Where a worker reports its own failures.
pub type OnError = Arc<dyn Fn(&str, &Error) + Send + Sync>;
/// Called with each lease as it is claimed, before anything is posted.
pub type OnLease = Arc<dyn Fn(&StepLease) + Send + Sync>;
/// Called once `started` was accepted and the body is about to run.
pub type OnStepStart = Arc<dyn Fn(&StepContext) + Send + Sync>;
/// Called when the body finished, with what is about to be posted.
pub type OnStepEnd = Arc<dyn Fn(&StepContext, &StepOutcome) + Send + Sync>;
/// Called when a 409 was applied, with the daemon's reason.
pub type OnConflict = Arc<dyn Fn(&StepLease, &str) + Send + Sync>;

#[derive(Default)]
struct Hooks {
    on_lease: Option<OnLease>,
    on_step_start: Option<OnStepStart>,
    on_step_end: Option<OnStepEnd>,
    on_conflict: Option<OnConflict>,
}

#[derive(Default)]
struct Counters {
    claimed: AtomicU64,
    completed: AtomicU64,
    unservable: AtomicU64,
    failed: Mutex<HashMap<String, u64>>,
    conflicts: Mutex<HashMap<String, u64>>,
}

fn bump(map: &Mutex<HashMap<String, u64>>, key: &str) {
    *map.lock().unwrap_or_else(|e| e.into_inner()).entry(key.to_owned()).or_insert(0) += 1;
}

struct Inner {
    mcpd: Mcpd,
    id: String,
    concurrency: usize,
    project: Option<String>,
    wait: Duration,
    /// The caller's own reporter, which sees every failure.
    on_error: Option<OnError>,
    /// When each kind of failure was last written by the default reporter.
    logged: Mutex<HashMap<&'static str, std::time::Instant>>,
    hooks: Hooks,
    counters: Counters,
    workflows: Vec<Workflow>,
    heartbeat_ms: AtomicU64,
    running: AtomicUsize,
    freed: Notify,
    stopping: AtomicBool,
    stop: Notify,
}

impl Inner {
    /// Report one of the worker's own failures.
    ///
    /// A caller's `on_error` sees every one. Without one, each kind is written
    /// at most once per flush interval. The kind is the key, never the
    /// message: a message names its step, and a key that did would let every
    /// step of a failing run through the limit.
    fn report(&self, kind: &'static str, message: &str, error: &Error) {
        if let Some(on_error) = &self.on_error {
            on_error(message, error);
            return;
        }
        let now = std::time::Instant::now();
        let interval = self.mcpd.config().flush_interval;
        let mut logged = self.logged.lock().unwrap_or_else(|e| e.into_inner());
        if logged.get(kind).is_some_and(|last| now.duration_since(*last) < interval) {
            return;
        }
        logged.insert(kind, now);
        drop(logged);
        tracing::warn!(error = %error, "{message}");
    }

    /// Run one hook. A hook is the user's code, so a panic in it is reported
    /// through `on_error` and the loop goes on (B1 for workers).
    fn hook(&self, name: &str, f: impl FnOnce()) {
        if let Err(panic) = catch_unwind(AssertUnwindSafe(f)) {
            let what = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                .unwrap_or_else(|| "no message".to_owned());
            self.report(
                "hook",
                &format!("mcpd worker {}: the {name} hook panicked", self.id),
                &Error::Hook(format!("{name}: {what}")),
            );
        }
    }
}

/// Builds a [`Worker`].
pub struct WorkerBuilder {
    mcpd: Mcpd,
    id: Option<String>,
    concurrency: usize,
    project: Option<String>,
    wait: Duration,
    on_error: Option<OnError>,
    hooks: Hooks,
    workflows: Vec<Workflow>,
}

impl WorkerBuilder {
    /// A name for this process, recorded as the owner of every lease it takes.
    #[must_use]
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// How many steps to run at once. Default 4.
    #[must_use]
    pub fn concurrency(mut self, concurrency: usize) -> Self {
        self.concurrency = concurrency.max(1);
        self
    }

    /// Which project to serve. Needed only when the key reaches several.
    #[must_use]
    pub fn project(mut self, project: impl Into<String>) -> Self {
        self.project = Some(project.into());
        self
    }

    /// How long each claim waits for work. Default 20 seconds, at most 60.
    #[must_use]
    pub fn wait(mut self, wait: Duration) -> Self {
        self.wait = wait.min(Duration::from_secs(60));
        self
    }

    /// Where the worker reports its own failures. Default: `tracing::warn!`.
    #[must_use]
    pub fn on_error(mut self, f: impl Fn(&str, &Error) + Send + Sync + 'static) -> Self {
        self.on_error = Some(Arc::new(f));
        self
    }

    /// Called with each lease as it is claimed, before anything is posted.
    #[must_use]
    pub fn on_lease(mut self, f: impl Fn(&StepLease) + Send + Sync + 'static) -> Self {
        self.hooks.on_lease = Some(Arc::new(f));
        self
    }

    /// Called once `started` was accepted and the body is about to run.
    #[must_use]
    pub fn on_step_start(mut self, f: impl Fn(&StepContext) + Send + Sync + 'static) -> Self {
        self.hooks.on_step_start = Some(Arc::new(f));
        self
    }

    /// Called when the body finished, with what is about to be posted:
    /// completed, or failed with its class.
    #[must_use]
    pub fn on_step_end(
        mut self,
        f: impl Fn(&StepContext, &StepOutcome) + Send + Sync + 'static,
    ) -> Self {
        self.hooks.on_step_end = Some(Arc::new(f));
        self
    }

    /// Called when a 409 was applied, with the daemon's reason. A `fenced`
    /// lease abandoned in silence looks exactly like no work from outside;
    /// this is how a worker tells the two apart.
    #[must_use]
    pub fn on_conflict(mut self, f: impl Fn(&StepLease, &str) + Send + Sync + 'static) -> Self {
        self.hooks.on_conflict = Some(Arc::new(f));
        self
    }

    /// A workflow this worker serves. Its definition is posted by
    /// [`Worker::serve`] or [`Worker::register_definitions`].
    #[must_use]
    pub fn register(mut self, workflow: Workflow) -> Self {
        self.workflows.push(workflow);
        self
    }

    /// The worker.
    #[must_use]
    pub fn build(self) -> Worker {
        Worker {
            inner: Arc::new(Inner {
                mcpd: self.mcpd,
                id: self.id.unwrap_or_else(default_worker_id),
                concurrency: self.concurrency,
                project: self.project,
                wait: self.wait,
                on_error: self.on_error,
                logged: Mutex::new(HashMap::new()),
                hooks: self.hooks,
                counters: Counters::default(),
                workflows: self.workflows,
                heartbeat_ms: AtomicU64::new(10_000),
                running: AtomicUsize::new(0),
                freed: Notify::new(),
                stopping: AtomicBool::new(false),
                stop: Notify::new(),
            }),
        }
    }
}

fn default_worker_id() -> String {
    let host = std::env::var("HOSTNAME").unwrap_or_else(|_| "worker".to_owned());
    format!("{host}-{}", &mcpd_core::ids::new_span_id()[..8])
}

/// Serves step bodies from this process. See the module comment for the
/// protocol it drives. Cheap to clone; every clone is the same worker.
#[derive(Clone)]
pub struct Worker {
    inner: Arc<Inner>,
}

impl fmt::Debug for Worker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Worker")
            .field("id", &self.inner.id)
            .field("concurrency", &self.inner.concurrency)
            .field("workflows", &self.inner.workflows.len())
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Landed,
    Done,
    Abandon,
}

impl Worker {
    /// Start building a worker on `mcpd`.
    #[must_use]
    pub fn builder(mcpd: &Mcpd) -> WorkerBuilder {
        WorkerBuilder {
            mcpd: mcpd.clone(),
            id: None,
            concurrency: 4,
            project: None,
            wait: Duration::from_secs(20),
            on_error: None,
            hooks: Hooks::default(),
            workflows: Vec::new(),
        }
    }

    /// What this worker has done since it was built.
    #[must_use]
    pub fn stats(&self) -> WorkerStats {
        let c = &self.inner.counters;
        WorkerStats {
            claimed: c.claimed.load(Ordering::Relaxed),
            inflight: self.inflight(),
            completed: c.completed.load(Ordering::Relaxed),
            failed: c.failed.lock().unwrap_or_else(|e| e.into_inner()).clone(),
            conflicts: c.conflicts.lock().unwrap_or_else(|e| e.into_inner()).clone(),
            unservable: c.unservable.load(Ordering::Relaxed),
        }
    }

    /// The name every lease records as its owner.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.inner.id
    }

    /// Steps running right now.
    #[must_use]
    pub fn inflight(&self) -> usize {
        self.inner.running.load(Ordering::Acquire)
    }

    /// Register, then claim and serve until [`Worker::stop`] is called.
    /// Resolves once every step in flight has reported.
    ///
    /// # Errors
    /// When a definition cannot be built or the daemon refuses the
    /// registration. A failed claim is reported and retried, not returned.
    pub async fn serve(&self) -> Result<(), Error> {
        let inner = &self.inner;
        inner.stopping.store(false, Ordering::Release);
        self.register_definitions().await?;
        while !inner.stopping.load(Ordering::Acquire) {
            let free = inner.concurrency.saturating_sub(self.inflight());
            if free == 0 {
                tokio::select! {
                    () = inner.freed.notified() => {}
                    () = inner.stop.notified() => {}
                    () = tokio::time::sleep(Duration::from_secs(1)) => {}
                }
                continue;
            }
            match self.claim(free, None).await {
                Ok(0) => self.pause(Duration::from_millis(10)).await,
                Ok(_) => {}
                Err(e) => {
                    inner.report(
                        "claim",
                        &format!("mcpd worker {}: claim failed; retrying", inner.id),
                        &e,
                    );
                    self.pause(Duration::from_secs(1)).await;
                }
            }
        }
        self.drain().await;
        Ok(())
    }

    async fn pause(&self, d: Duration) {
        tokio::select! {
            () = tokio::time::sleep(d) => {}
            () = self.inner.stop.notified() => {}
        }
    }

    /// Stop claiming. [`Worker::serve`] resolves once the steps in flight
    /// have reported; [`Worker::drain`] waits for the same from anywhere.
    pub fn stop(&self) {
        self.inner.stopping.store(true, Ordering::Release);
        self.inner.stop.notify_waiters();
        self.inner.stop.notify_one();
    }

    /// Post every registered definition and read back the lease timings.
    ///
    /// # Errors
    /// When a definition cannot be built or the daemon refuses it.
    pub async fn register_definitions(&self) -> Result<(), Error> {
        let docs = self
            .inner
            .workflows
            .iter()
            .map(Workflow::document)
            .collect::<Result<Vec<DefinitionDocument>, Error>>()?;
        let registered = self
            .inner
            .mcpd
            .register_worker(&self.inner.id, docs, self.inner.project.as_deref())
            .await?;
        self.inner.heartbeat_ms.store(registered.heartbeat_ms.max(1_000), Ordering::Release);
        Ok(())
    }

    /// One claim, serving what it granted on tasks of its own. Returns how
    /// many leases were taken. [`Worker::serve`] calls this in a loop; a
    /// script that drives its own run calls it with that run's id until the
    /// run ends, then [`Worker::drain`].
    ///
    /// # Errors
    /// When the daemon cannot be reached or refuses the claim.
    pub async fn claim(&self, max: usize, run_id: Option<&str>) -> Result<usize, Error> {
        let inner = &self.inner;
        let response = inner
            .mcpd
            .claim_steps(ClaimSteps {
                worker_id: inner.id.clone(),
                max: u32::try_from(max.clamp(1, 100)).unwrap_or(1),
                wait: inner.wait,
                run_id: run_id.map(str::to_owned),
                project: inner.project.clone(),
            })
            .await?;
        let taken = response.leases.len();
        inner.counters.claimed.fetch_add(taken as u64, Ordering::Relaxed);
        for lease in response.leases {
            inner.running.fetch_add(1, Ordering::AcqRel);
            let worker = self.clone();
            tokio::spawn(async move {
                let _guard = RunningGuard(worker.inner.clone());
                worker.serve_lease(lease).await;
            });
        }
        Ok(taken)
    }

    /// Wait for every step in flight.
    pub async fn drain(&self) {
        while self.inflight() > 0 {
            self.inner.freed.notified().await;
        }
    }

    fn handler_for(&self, lease: &StepLease) -> Option<StepHandler> {
        self.inner.workflows.iter().find_map(|w| {
            if w.id() != lease.definition_id {
                return None;
            }
            if let Some(v) = w.version() {
                if v != lease.definition_version {
                    return None;
                }
            }
            w.handler_for(&lease.step_key)
        })
    }

    async fn serve_lease(&self, lease: StepLease) {
        let inner = &self.inner;
        let mcpd = &inner.mcpd;
        if let Some(hook) = &inner.hooks.on_lease {
            inner.hook("on_lease", || hook(&lease));
        }
        let Some(handler) = self.handler_for(&lease) else {
            inner.counters.unservable.fetch_add(1, Ordering::Relaxed);
            let message = format!(
                "worker {} does not serve {}@{} step {}",
                inner.id, lease.definition_id, lease.definition_version, lease.step_key
            );
            self.report(
                &lease,
                mcpd.fail_step(
                    &lease.lease_id,
                    StepError {
                        class: StepErrorClass::Unservable,
                        message,
                        details: None,
                        retry_after_ms: None,
                    },
                ),
            )
            .await;
            return;
        };

        // Before the body. Always. This is what tells the daemon the
        // difference between "offered" and "running", and on an at_most_once
        // step it refuses a complete without it.
        if self.report(&lease, mcpd.step_started(&lease.lease_id)).await == Outcome::Abandon {
            return;
        }

        let ctx = StepContext::new(mcpd.clone(), lease.clone());
        if let Some(hook) = &inner.hooks.on_step_start {
            inner.hook("on_step_start", || hook(&ctx));
        }
        let stop_beat = Arc::new(Notify::new());
        let beat = {
            let worker = self.clone();
            let ctx = ctx.clone();
            let stop = Arc::clone(&stop_beat);
            let lease_id = lease.lease_id.clone();
            let step_key = lease.step_key.clone();
            tokio::spawn(async move {
                let every =
                    Duration::from_millis(worker.inner.heartbeat_ms.load(Ordering::Acquire));
                loop {
                    tokio::select! {
                        () = tokio::time::sleep(every) => {}
                        () = stop.notified() => return,
                    }
                    match worker.inner.mcpd.step_heartbeat(&lease_id).await {
                        Ok(_) => {}
                        Err(Error::Conflict(conflict)) => {
                            // The daemon has moved on without us; the body learns through `cancelled`.
                            ctx.cancel(format!(
                                "{}: {}",
                                serde_plain(&conflict.reason),
                                conflict.message
                            ));
                            return;
                        }
                        Err(e) => {
                            // A transport failure is not evidence of anything; the next beat may land.
                            worker.inner.report(
                                "heartbeat",
                                &format!(
                                    "mcpd worker {}: heartbeat failed for {step_key}",
                                    worker.inner.id
                                ),
                                &e,
                            );
                        }
                    }
                }
            })
        };

        // The step's own ceiling, enforced here as well as by the daemon: a
        // body that runs past `deadline_ms` is cancelled through `cancelled()`
        // and the attempt is failed retryable, so the task's slot is freed and
        // the journal says why, instead of the daemon reclaiming a lease still
        // heartbeating from inside a body that will never return.
        let deadline = lease.deadline_ms.filter(|ms| *ms > 0).map(|ms| {
            let worker = self.clone();
            let ctx = ctx.clone();
            let lease = lease.clone();
            let stop = Arc::clone(&stop_beat);
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(ms)).await;
                if ctx.is_cancelled() {
                    return;
                }
                let reason = format!("step deadline of {ms}ms passed");
                ctx.cancel(reason.clone());
                stop.notify_one();
                worker.ended(
                    &ctx,
                    &StepOutcome::Failed {
                        class: StepErrorClass::Retryable,
                        message: reason.clone(),
                    },
                );
                worker
                    .report(
                        &lease,
                        worker.inner.mcpd.fail_step(
                            &lease.lease_id,
                            StepError {
                                class: StepErrorClass::Retryable,
                                message: reason,
                                details: None,
                                retry_after_ms: None,
                            },
                        ),
                    )
                    .await;
            })
        });

        // The body is raced against its own cancellation. When the lease is
        // given up, at its deadline or by a fence, the body's future is
        // dropped and the slot is freed then, whether or not the body was
        // watching `cancelled()`, as contract section 7.2 says.
        let result = tokio::select! {
            // The body first, so one that is watching `cancelled()` gets to
            // finish what it does on the way out.
            biased;
            result = handler(ctx.clone()) => Some(result),
            () = ctx.cancelled() => None,
        };
        stop_beat.notify_one();
        let _ = beat.await;
        if let Some(deadline) = deadline {
            // A deadline that already fired is posting the failure; this
            // lease is not finished until that post has landed. One that has
            // not fired is not needed any more.
            if deadline.is_finished() || ctx.is_cancelled() {
                let _ = deadline.await;
            } else {
                deadline.abort();
            }
        }
        // Fenced, the deadline, or the run ended: post nothing more for this
        // lease. `None` means the body was given up before it finished.
        let Some(result) = result.filter(|_| !ctx.is_cancelled()) else {
            return;
        };
        match result {
            Err(failure) => {
                self.ended(
                    &ctx,
                    &StepOutcome::Failed { class: failure.class, message: failure.message.clone() },
                );
                self.report(&lease, mcpd.fail_step(&lease.lease_id, failure.wire())).await;
            }
            Ok(output) => {
                self.ended(&ctx, &StepOutcome::Completed);
                let (state, usage) = ctx.collected();
                let result = StepResult { output: Some(output), state, usage };
                self.report(&lease, mcpd.complete_step(&lease.lease_id, &result)).await;
            }
        }
    }

    /// Count a body's outcome and tell the `on_step_end` hook.
    fn ended(&self, ctx: &StepContext, outcome: &StepOutcome) {
        let inner = &self.inner;
        match outcome {
            StepOutcome::Completed => {
                inner.counters.completed.fetch_add(1, Ordering::Relaxed);
            }
            StepOutcome::Failed { class, .. } => bump(&inner.counters.failed, &serde_plain(class)),
        }
        if let Some(hook) = &inner.hooks.on_step_end {
            inner.hook("on_step_end", || hook(ctx, outcome));
        }
    }

    /// Post one worker call and apply the 409 table: `already_applied` is a
    /// success, and every other reason means abandon this lease.
    async fn report<T>(
        &self,
        lease: &StepLease,
        call: impl std::future::Future<Output = Result<T, Error>>,
    ) -> Outcome {
        match call.await {
            Ok(_) => Outcome::Landed,
            Err(Error::Conflict(conflict)) => {
                let reason = serde_plain(&conflict.reason);
                bump(&self.inner.counters.conflicts, &reason);
                if let Some(hook) = &self.inner.hooks.on_conflict {
                    self.inner.hook("on_conflict", || hook(lease, &reason));
                }
                self.conflicted(lease, conflict)
            }
            Err(e) => {
                self.inner.report(
                    "call",
                    &format!("mcpd worker {}: call failed for {}", self.inner.id, lease.step_key),
                    &e,
                );
                Outcome::Abandon
            }
        }
    }

    /// The 409 table, by reason.
    fn conflicted(&self, lease: &StepLease, conflict: Box<mcpd_core::StepConflict>) -> Outcome {
        match conflict.reason {
            ConflictReason::AlreadyApplied => Outcome::Done,
            ConflictReason::NotStarted => {
                self.inner.report(
                    "not_started",
                    &format!(
                        "mcpd worker {}: a bug in this worker: {} was reported before started",
                        self.inner.id, lease.step_key
                    ),
                    &Error::Conflict(conflict),
                );
                Outcome::Abandon
            }
            ConflictReason::Fenced | ConflictReason::RunTerminal => Outcome::Abandon,
        }
    }
}

/// Decrements the running count when a lease's task ends, however it ends.
struct RunningGuard(Arc<Inner>);

impl Drop for RunningGuard {
    fn drop(&mut self) {
        self.0.running.fetch_sub(1, Ordering::AcqRel);
        self.0.freed.notify_waiters();
        self.0.freed.notify_one();
    }
}

fn serde_plain<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()
}
