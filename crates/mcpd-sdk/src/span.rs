//! Spans, and the guard that ends one when it goes out of scope.
//!
//! A [`Span`] is a handle, not a record. The record lives behind a mutex and is
//! sent twice: once when the span starts and once when it ends. That is
//! deliberate. A run that crashes half way through is exactly the run you need
//! to read, and a span that is only sent on completion is the one span you never
//! get to see.
//!
//! Every method here takes `&self` and returns nothing that can fail. Recording
//! a span must not be something a caller has to handle, so a span on a disabled
//! client, an unsampled trace or a dead server behaves identically to one on a
//! healthy client: it accepts everything and reports nothing.

use crate::client::ClientInner;
use crate::context::{self, Active, SpanContext};
use crate::queue::Signal;
use mcpd_core::{
    Correlation, EntityKind, ModelParameters, Span as SpanRecord, SpanError, SpanKind, SpanStatus,
    TokenUsage,
};
use serde::Serialize;
use std::sync::{Arc, Mutex};
use time::OffsetDateTime;

/// One step of an agent run.
///
/// Ends when it is dropped, so the common case needs no `end()` call:
///
/// ```no_run
/// # async fn f() {
/// let mcpd = mcpd_sdk::Mcpd::new();
/// {
///     let span = mcpd.trace("support agent");
///     span.set_input("where is my order?");
/// } // the span ends here
/// # }
/// ```
#[derive(Debug)]
pub struct Span {
    ctx: SpanContext,
    /// `None` when this span is not being recorded, which happens for a
    /// disabled client and for a trace sampling turned down. The handle stays
    /// fully usable either way.
    inner: Option<Arc<Recording>>,
}

/// Where a span hands its usage when it ends.
///
/// A step context installs one on the span it opens, and every span nested
/// under that span inherits it, so the step's usage is the sum of what its
/// spans spent without the body adding tokens by hand. Never on the wire.
pub(crate) type UsageSink = Arc<dyn Fn(&TokenUsage, Option<&str>, Option<&str>) + Send + Sync>;

/// A span that is being recorded, shared by the handle, its scope and its
/// children.
#[derive(Debug)]
pub(crate) struct Recording {
    client: Arc<ClientInner>,
    state: Mutex<State>,
}

impl Recording {
    /// What this span is tagged with, as it stands now.
    ///
    /// Read by a child on the way up and by a log line written inside the
    /// span's scope, so both see a field set on the run after it started.
    pub(crate) fn correlation(&self) -> Correlation {
        match self.state.lock() {
            Ok(s) => s.record.correlation.clone(),
            Err(e) => e.into_inner().record.correlation.clone(),
        }
    }

    /// The usage sink a child inherits, if this span carries one.
    pub(crate) fn usage_sink(&self) -> Option<UsageSink> {
        match self.state.lock() {
            Ok(s) => s.usage_sink.clone(),
            Err(e) => e.into_inner().usage_sink.clone(),
        }
    }
}

struct State {
    record: SpanRecord,
    ended: bool,
    /// Whether `input` and `output` may leave the process for this span.
    capture: bool,
    usage_sink: Option<UsageSink>,
    /// A span reports its usage upward once, at its end.
    usage_reported: bool,
}

impl std::fmt::Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("State")
            .field("record", &self.record)
            .field("ended", &self.ended)
            .field("capture", &self.capture)
            .field("usage_sink", &self.usage_sink.as_ref().map(|_| "<sink>"))
            .finish_non_exhaustive()
    }
}

impl Span {
    /// Start a span that is being recorded.
    ///
    /// `correlation` is what this span starts tagged with: the client's own for
    /// a root, and the parent's for a child. A child that took the client's
    /// instead would drop every field the run was tagged with, which is the
    /// whole of why it is a parameter rather than read from the client here.
    pub(crate) fn start(
        client: &Arc<ClientInner>,
        ctx: SpanContext,
        parent_span_id: Option<String>,
        name: impl Into<String>,
        kind: SpanKind,
        correlation: Correlation,
    ) -> Self {
        let record = SpanRecord {
            trace_id: ctx.trace_id.clone(),
            span_id: ctx.span_id.clone(),
            parent_span_id,
            external_parent_span_id: None,
            name: name.into(),
            kind,
            status: SpanStatus::Unset,
            start_time: OffsetDateTime::now_utc(),
            end_time: None,
            duration_ns: None,
            is_root: false,
            is_event: false,
            entity_kind: None,
            entity_id: None,
            entity_name: None,
            input: None,
            output: None,
            attributes: None,
            metadata: None,
            usage: None,
            cost: None,
            // The SDK sets these from `record_model_parameters`, which the
            // model-parameter track adds. A started span has none yet.
            model_parameters: None,
            error: None,
            links: None,
            tags: None,
            correlation,
            model: None,
            provider: None,
            // The server stamps this on arrival. Sending the epoch says "I do
            // not know", which is honest; sending our own clock would make
            // retention depend on the producer's clock being right.
            received_at: OffsetDateTime::UNIX_EPOCH,
            ingest_key: None,
        };
        // is_root and duration are recomputed at ingest; setting is_root here
        // keeps a span that is read back before it ends consistent with one
        // that is read back after.
        let mut record = record;
        record.is_root = record.parent_span_id.is_none();

        let span = Self {
            ctx,
            inner: Some(Arc::new(Recording {
                client: Arc::clone(client),
                state: Mutex::new(State {
                    record,
                    ended: false,
                    capture: client.config.capture_payloads,
                    usage_sink: None,
                    usage_reported: false,
                }),
            })),
        };
        // Export on start (B8): an unfinished span is still worth seeing.
        span.emit();
        span
    }

    /// A handle that records nothing.
    pub(crate) const fn silent(ctx: SpanContext) -> Self {
        Self { ctx, inner: None }
    }

    /// Whether this span is being recorded. False for a disabled client and for
    /// a trace that sampling declined.
    #[must_use]
    pub const fn is_recording(&self) -> bool {
        self.inner.is_some()
    }

    /// The trace this span belongs to.
    #[must_use]
    pub fn trace_id(&self) -> &str {
        &self.ctx.trace_id
    }

    /// This span's id.
    #[must_use]
    pub fn span_id(&self) -> &str {
        &self.ctx.span_id
    }

    /// The context to hand to a child, or to a task you are about to spawn.
    #[must_use]
    pub fn context(&self) -> SpanContext {
        self.ctx.clone()
    }

    /// A W3C `traceparent` header value naming this span.
    ///
    /// Put it on an outbound request and the service you call continues this
    /// trace rather than starting its own.
    #[must_use]
    pub fn traceparent(&self) -> String {
        self.ctx.traceparent()
    }

    // ------------------------------------------------------------ children ---

    /// Start a child span of kind [`SpanKind::Generic`].
    #[must_use]
    pub fn child(&self, name: impl Into<String>) -> Self {
        self.child_of(name, SpanKind::Generic)
    }

    /// Start a child span of a given kind.
    #[must_use]
    pub fn child_of(&self, name: impl Into<String>, kind: SpanKind) -> Self {
        match &self.inner {
            Some(rec) => {
                let child = Self::start(
                    &rec.client,
                    self.ctx.child(),
                    Some(self.ctx.span_id.clone()),
                    name,
                    kind,
                    // Inherited from the parent as it stands now, so a session
                    // set on the run reaches every step started after it.
                    rec.correlation(),
                );
                // A child under a step's span adds its usage to that step too.
                child.adopt_usage_sink(rec.usage_sink());
                child
            }
            // A child of a span nobody is recording is not recorded either.
            // That is the inheritance rule, and it is what stops a trace being
            // half present.
            None => Self::silent(self.ctx.child()),
        }
    }

    /// Record a point in time inside this span: a retry, a cache miss, the first
    /// streamed token.
    pub fn event(&self, name: impl Into<String>) {
        self.event_with(name, serde_json::Value::Null);
    }

    /// An event carrying structured detail.
    pub fn event_with(&self, name: impl Into<String>, attributes: serde_json::Value) {
        let Some(rec) = &self.inner else { return };
        let ctx = self.ctx.child();
        let (mut record, capture) = match rec.state.lock() {
            Ok(s) => (s.record.clone(), s.capture),
            Err(e) => {
                let s = e.into_inner();
                (s.record.clone(), s.capture)
            }
        };
        let mut attributes = attributes;
        if !capture {
            // An event's payload rides in its attributes; the capture switch
            // covers it the same way it covers a span's input and output.
            if let serde_json::Value::Object(map) = &mut attributes {
                map.remove("input");
                map.remove("output");
            }
        }
        record.span_id = ctx.span_id;
        record.parent_span_id = Some(self.ctx.span_id.clone());
        record.name = name.into();
        record.kind = SpanKind::Generic;
        record.status = SpanStatus::Ok;
        record.start_time = OffsetDateTime::now_utc();
        record.end_time = None;
        record.is_root = false;
        record.is_event = true;
        record.input = None;
        record.output = None;
        record.usage = None;
        record.error = None;
        record.attributes = (!attributes.is_null()).then_some(attributes);
        rec.client.enqueue(Signal::Span(Box::new(record)));
    }

    // ---------------------------------------------------------------- data ---

    /// What went into this step. For a model call, the prompt.
    ///
    /// Dropped, and nothing else, when payload capture is off for this span.
    pub fn set_input(&self, value: impl Serialize) -> &Self {
        if !self.capturing() {
            return self;
        }
        if let Some(v) = self.to_json(value, "input") {
            self.edit(|r| r.input = Some(v));
        }
        self
    }

    /// What came out. For a model call, the completion.
    ///
    /// Dropped, and nothing else, when payload capture is off for this span.
    pub fn set_output(&self, value: impl Serialize) -> &Self {
        if !self.capturing() {
            return self;
        }
        if let Some(v) = self.to_json(value, "output") {
            self.edit(|r| r.output = Some(v));
        }
        self
    }

    /// Whether this span's `input` and `output` may leave the process,
    /// overriding the client's `capture_payloads` for this span only, in
    /// either direction. Turning it off also drops a payload set before the
    /// call, so nothing set on this span ever leaves.
    pub fn capture_payloads(&self, capture: bool) -> &Self {
        self.edit_state(|state| {
            state.capture = capture;
            if !capture {
                state.record.input = None;
                state.record.output = None;
            }
        });
        self
    }

    /// Whether `set_input` and `set_output` keep what they are given.
    #[must_use]
    pub fn capturing(&self) -> bool {
        match &self.inner {
            Some(rec) => match rec.state.lock() {
                Ok(s) => s.capture,
                Err(e) => e.into_inner().capture,
            },
            None => false,
        }
    }

    /// A parent from a tracing system that is not mcpd, carried for
    /// correlation and never used for parenting inside mcpd's own tree.
    pub fn set_external_parent(&self, span_id: impl Into<String>) -> &Self {
        let id = span_id.into();
        self.edit(|r| r.external_parent_span_id = Some(id));
        self
    }

    /// Which model this step used, and who served it.
    pub fn set_model(&self, model: impl Into<String>, provider: impl Into<String>) -> &Self {
        let (model, provider) = (model.into(), provider.into());
        self.edit(|r| {
            r.model = Some(model);
            r.provider = Some(provider);
        });
        self
    }

    /// Token counts. This is what cost is computed from at ingest.
    pub fn set_usage(&self, usage: TokenUsage) -> &Self {
        self.edit(|r| r.usage = Some(usage));
        self
    }

    /// The common case of [`Span::set_usage`]: an input and an output total.
    pub fn set_tokens(&self, input_tokens: u64, output_tokens: u64) -> &Self {
        self.set_usage(TokenUsage {
            input_tokens: Some(input_tokens),
            output_tokens: Some(output_tokens),
            ..TokenUsage::default()
        })
    }

    /// How the model was called: temperature, nucleus and top-k sampling, the
    /// output ceiling, the seed, the stop sequences, and whether the output was
    /// constrained to a fixed shape.
    ///
    /// These are settings, not payload. Nothing here carries a prompt or a
    /// completion, so they are recorded whether or not payload capture is on.
    /// They are what a run has to be reproduced from, and dropping them with
    /// the prompt would leave a run nobody can repeat.
    ///
    /// `None` on a field means the caller set nothing; `Some(0)` is a real
    /// value and reaches the wire as one, which matters most for `seed`. One
    /// call describes one model call, so a second call replaces the first:
    /// settings half from one request and half from another would describe a
    /// call nobody made.
    ///
    /// ```no_run
    /// # fn f(span: &mcpd_sdk::Span) {
    /// use mcpd_sdk::model::ModelParameters;
    ///
    /// span.set_model_parameters(ModelParameters {
    ///     temperature: Some(0.0),
    ///     seed: Some(0),
    ///     max_tokens: Some(1024),
    ///     ..ModelParameters::default()
    /// });
    /// # }
    /// ```
    pub fn set_model_parameters(&self, parameters: ModelParameters) -> &Self {
        self.edit(|r| r.model_parameters = Some(parameters));
        self
    }

    /// Kind-specific structured detail.
    pub fn set_attributes(&self, attributes: serde_json::Value) -> &Self {
        self.edit(|r| r.attributes = Some(attributes));
        self
    }

    /// Anything you want to carry along. mcpd never interprets it.
    pub fn set_metadata(&self, metadata: serde_json::Value) -> &Self {
        self.edit(|r| r.metadata = Some(metadata));
        self
    }

    /// Label the whole run.
    ///
    /// Tags belong to a trace, so they are only kept on a root span. On a child
    /// they are ignored, because a tag that only labels step four of a run is a
    /// filter that returns something nobody asked for.
    pub fn set_tags<I, S>(&self, tags: I) -> &Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let tags: Vec<String> = tags.into_iter().map(Into::into).collect();
        self.edit(|r| {
            if r.is_root {
                r.tags = Some(tags);
            } else {
                tracing::debug!(span = %r.name, "tags are only recorded on a root span; ignoring");
            }
        });
        self
    }

    /// Name the thing that produced this span: an agent, a tool, a scorer.
    pub fn set_entity(&self, kind: EntityKind, name: impl Into<String>) -> &Self {
        let name = name.into();
        self.edit(|r| {
            r.entity_kind = Some(kind);
            r.entity_name = Some(name);
        });
        self
    }

    /// The conversation this run belongs to.
    pub fn set_session(&self, session_id: impl Into<String>) -> &Self {
        let id = session_id.into();
        self.edit(|r| r.correlation.session_id = Some(id));
        self
    }

    /// A thread inside the session.
    pub fn set_thread(&self, thread_id: impl Into<String>) -> &Self {
        let id = thread_id.into();
        self.edit(|r| r.correlation.thread_id = Some(id));
        self
    }

    /// The end user this run is for.
    pub fn set_user(&self, user_id: impl Into<String>) -> &Self {
        let id = user_id.into();
        self.edit(|r| r.correlation.user_id = Some(id));
        self
    }

    /// The inbound request that caused this work.
    pub fn set_request(&self, request_id: impl Into<String>) -> &Self {
        let id = request_id.into();
        self.edit(|r| r.correlation.request_id = Some(id));
        self
    }

    /// The release or git sha this ran on, so a regression can be pinned to one.
    pub fn set_release(&self, release: impl Into<String>) -> &Self {
        let release = release.into();
        self.edit(|r| r.correlation.release = Some(release));
        self
    }

    // -------------------------------------------------------------- status ---

    /// Record a failure. Sets the status to `error`.
    ///
    /// The message is required and not optional, because "something failed" is
    /// not a thing anyone can act on.
    pub fn set_error(&self, message: impl Into<String>) -> &Self {
        let message = message.into();
        self.set_error_detail(SpanError {
            // B10: an error must carry a message. An empty one reaches the
            // trace list as a failure nobody can act on, so record that the
            // producer gave nothing rather than storing "".
            message: if message.trim().is_empty() {
                "no message given".to_owned()
            } else {
                message
            },
            kind: None,
            stack: None,
            domain: None,
            retryable: None,
            details: None,
        })
    }

    /// Record a failure with its class, stack and retryability.
    pub fn set_error_detail(&self, error: SpanError) -> &Self {
        self.edit(|r| {
            r.status = SpanStatus::Error;
            r.error = Some(error);
        });
        self
    }

    /// Record a failure from any [`std::error::Error`], taking its `Display`
    /// text as the message and its type name as the kind.
    pub fn record_error<E: std::error::Error + ?Sized>(&self, error: &E) -> &Self {
        self.set_error_detail(SpanError {
            message: error.to_string(),
            kind: Some(
                std::any::type_name::<E>().rsplit("::").next().unwrap_or("Error").to_owned(),
            ),
            stack: None,
            domain: None,
            retryable: None,
            details: error.source().map(|s| serde_json::json!({ "source": s.to_string() })),
        })
    }

    /// Set the outcome without ending the span.
    pub fn set_status(&self, status: SpanStatus) -> &Self {
        self.edit(|r| r.status = status);
        self
    }

    // ----------------------------------------------------------------- end ---

    /// End the span. Idempotent, and implied by dropping it.
    pub fn end(&self) {
        self.finish(None);
    }

    /// End the span with an explicit outcome, overriding whatever status it
    /// carries.
    pub fn end_with(&self, status: SpanStatus) {
        self.finish(Some(status));
    }

    // --------------------------------------------------------------- scope ---

    /// Run a future with this span as the task-local current span, so anything
    /// it calls can attach children without being handed the span.
    pub async fn scope<F: std::future::Future>(&self, future: F) -> F::Output {
        context::scope(self.active(), future).await
    }

    /// The blocking form of [`Span::scope`].
    pub fn sync_scope<T>(&self, f: impl FnOnce() -> T) -> T {
        context::sync_scope(self.active(), f)
    }

    /// This span, in the shape a scope publishes.
    fn active(&self) -> Active {
        Active { ctx: self.ctx.clone(), span: self.inner.clone() }
    }

    // ------------------------------------------------------------ internals ---

    /// Hand this span's usage upward when it ends. Installed by a step
    /// context on the span it opens and inherited by every span under it.
    pub(crate) fn adopt_usage_sink(&self, sink: Option<UsageSink>) {
        if sink.is_none() {
            return;
        }
        self.edit_state(|state| state.usage_sink = sink);
    }

    /// Apply a change to the record, unless the span has already ended.
    fn edit(&self, f: impl FnOnce(&mut SpanRecord)) {
        self.edit_state(|state| f(&mut state.record));
    }

    /// Apply a change to the whole state, unless the span has already ended.
    fn edit_state(&self, f: impl FnOnce(&mut State)) {
        let Some(rec) = &self.inner else { return };
        let mut state = match rec.state.lock() {
            Ok(s) => s,
            // A panic elsewhere must not spread into the caller's code path.
            Err(e) => e.into_inner(),
        };
        if state.ended {
            tracing::debug!(span = %state.record.name, "ignoring a change to a span that has ended");
            return;
        }
        f(&mut state);
    }

    /// Serialise a payload, reporting rather than propagating a failure.
    fn to_json(&self, value: impl Serialize, field: &str) -> Option<serde_json::Value> {
        let rec = self.inner.as_ref()?;
        match serde_json::to_value(value) {
            Ok(v) => Some(v),
            Err(e) => {
                if rec.client.warn.allow() {
                    tracing::warn!(field, error = %e, "mcpd could not serialise a span payload");
                }
                None
            }
        }
    }

    /// Queue the current state of the record.
    fn emit(&self) {
        let Some(rec) = &self.inner else { return };
        let record = match rec.state.lock() {
            Ok(s) => s.record.clone(),
            Err(e) => e.into_inner().record.clone(),
        };
        rec.client.enqueue(Signal::Span(Box::new(record)));
    }

    fn finish(&self, status: Option<SpanStatus>) {
        let Some(rec) = &self.inner else { return };
        let report = {
            let mut state = match rec.state.lock() {
                Ok(s) => s,
                Err(e) => e.into_inner(),
            };
            if state.ended {
                return;
            }
            state.ended = true;
            state.record.end_time = Some(OffsetDateTime::now_utc());
            match status {
                Some(s) => state.record.status = s,
                // An error recorded during the span outranks the default
                // success an ordinary end implies.
                None if state.record.status == SpanStatus::Unset => {
                    state.record.status = SpanStatus::Ok;
                }
                None => {}
            }
            let due =
                !state.usage_reported && state.usage_sink.is_some() && state.record.usage.is_some();
            if due {
                state.usage_reported = true;
            }
            due.then(|| {
                (
                    state.usage_sink.clone(),
                    state.record.usage.clone(),
                    state.record.model.clone(),
                    state.record.provider.clone(),
                )
            })
        };
        self.emit();
        // Outside the lock: the sink is the step context's, not this span's,
        // and it must be free to read any span it likes.
        if let Some((Some(sink), Some(usage), model, provider)) = report {
            sink(&usage, model.as_deref(), provider.as_deref());
        }
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        self.finish(None);
    }
}

impl From<&Span> for SpanContext {
    fn from(span: &Span) -> Self {
        span.context()
    }
}
