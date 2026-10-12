//! Trace context, and how it travels.
//!
//! Two mechanisms, deliberately separate:
//!
//! * A **task-local** carries the active span inside one process, so a function
//!   three layers down can attach a child span without every caller threading a
//!   parameter through.
//! * A **`traceparent` header** carries it between processes. Parsing one on the
//!   way in and emitting one on the way out is the whole of what makes a trace
//!   survive a service boundary.
//!
//! The task-local is set by [`Span::scope`](crate::Span::scope) and
//! [`Span::sync_scope`](crate::Span::sync_scope); nothing else writes it.

use crate::span::Recording;
use mcpd_core::{ids, Correlation};
use std::sync::Arc;

tokio::task_local! {
    /// The span the current task is running inside.
    static CURRENT: Active;
}

/// What a scope publishes about the span that opened it.
///
/// The ids alone are not enough. A log line written inside a run has to carry
/// the run's correlation, and the run is tagged after it starts, so the scope
/// keeps a handle on the span rather than a copy of what it held at the time.
/// Reading through the handle is what makes a session set on the root reach a
/// log written a moment later.
#[derive(Debug, Clone)]
pub(crate) struct Active {
    pub(crate) ctx: SpanContext,
    /// `None` for a span nobody is recording, which has no correlation to read.
    pub(crate) span: Option<Arc<Recording>>,
}

/// The three facts a child span needs from its parent, and the three a
/// `traceparent` header carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpanContext {
    /// The trace this span belongs to: 32 lowercase hex characters.
    pub trace_id: String,
    /// This span: 16 lowercase hex characters.
    pub span_id: String,
    /// Whether this trace is being recorded. Decided once at the root and
    /// inherited by every descendant, in this process and across the wire.
    pub sampled: bool,
}

impl SpanContext {
    /// Start a fresh trace.
    #[must_use]
    pub fn root(sampled: bool) -> Self {
        Self { trace_id: ids::new_trace_id(), span_id: ids::new_span_id(), sampled }
    }

    /// A child of this context: same trace, new span, same sampling decision.
    #[must_use]
    pub fn child(&self) -> Self {
        Self { trace_id: self.trace_id.clone(), span_id: ids::new_span_id(), sampled: self.sampled }
    }

    /// Render as a W3C `traceparent` header value, ready to put on an outbound
    /// request.
    #[must_use]
    pub fn traceparent(&self) -> String {
        ids::TraceParent {
            trace_id: self.trace_id.clone(),
            parent_id: self.span_id.clone(),
            flags: u8::from(self.sampled),
        }
        .to_header()
    }

    /// Read an inbound `traceparent`, returning a context whose `span_id` is the
    /// **caller's** span. Pass it to [`Mcpd::trace_from`](crate::Mcpd::trace_from),
    /// which makes the new span a child of it.
    ///
    /// A malformed header returns `None` rather than an error: an unreadable
    /// header is a reason to start a new trace, never a reason to fail the
    /// request that carried it.
    #[must_use]
    pub fn from_traceparent(header: &str) -> Option<Self> {
        let p = ids::parse_traceparent(header)?;
        let sampled = p.sampled();
        Some(Self { trace_id: p.trace_id, span_id: p.parent_id, sampled })
    }
}

/// The span the calling task is inside, if any.
///
/// Returns `None` outside any [`Span::scope`](crate::Span::scope), which is the
/// normal state at the top of a program.
#[must_use]
pub fn current() -> Option<SpanContext> {
    CURRENT.try_with(|a| a.ctx.clone()).ok()
}

/// The active span, ids and all, for code that also needs its correlation.
pub(crate) fn current_active() -> Option<Active> {
    CURRENT.try_with(Clone::clone).ok()
}

/// The correlation of the span the calling task is inside.
///
/// `None` outside a scope, and `None` inside a scope opened by a span nobody is
/// recording. Both mean the same thing to a caller: there is no run to inherit
/// from, so fall back to the client's own correlation.
pub(crate) fn current_correlation() -> Option<Correlation> {
    CURRENT.try_with(|a| a.span.as_ref().map(|r| r.correlation())).ok().flatten()
}

/// The active span's `traceparent`, for putting on an outbound request.
#[must_use]
pub fn current_traceparent() -> Option<String> {
    current().map(|c| c.traceparent())
}

/// Run a future with `active` as the current span.
pub(crate) async fn scope<F: std::future::Future>(active: Active, f: F) -> F::Output {
    CURRENT.scope(active, f).await
}

/// Run a closure with `active` as the current span.
pub(crate) fn sync_scope<T>(active: Active, f: impl FnOnce() -> T) -> T {
    CURRENT.sync_scope(active, f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_root_context_is_well_formed() {
        let c = SpanContext::root(true);
        ids::validate_trace_id(&c.trace_id).unwrap();
        ids::validate_span_id(&c.span_id).unwrap();
    }

    #[test]
    fn a_child_keeps_the_trace_and_the_decision() {
        let root = SpanContext::root(false);
        let child = root.child();
        assert_eq!(child.trace_id, root.trace_id);
        assert_ne!(child.span_id, root.span_id);
        assert!(!child.sampled);
    }

    #[test]
    fn traceparent_round_trips() {
        let c = SpanContext::root(true);
        let header = c.traceparent();
        let back = SpanContext::from_traceparent(&header).expect("should parse");
        assert_eq!(back, c);
    }

    #[test]
    fn an_unsampled_context_round_trips_its_flag() {
        let c = SpanContext::root(false);
        assert!(c.traceparent().ends_with("-00"));
        assert!(!SpanContext::from_traceparent(&c.traceparent()).unwrap().sampled);
    }

    #[test]
    fn a_malformed_traceparent_is_none_not_an_error() {
        for bad in [
            "",
            "nonsense",
            "00-abc-def-01",
            "ff-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
        ] {
            assert!(SpanContext::from_traceparent(bad).is_none(), "accepted {bad:?}");
        }
    }

    #[test]
    fn there_is_no_current_context_outside_a_scope() {
        assert!(current().is_none());
    }

    #[tokio::test]
    async fn a_scope_sets_and_restores_the_current_context() {
        let ctx = SpanContext::root(true);
        let active = Active { ctx: ctx.clone(), span: None };
        let seen = scope(active, async { current() }).await;
        assert_eq!(seen, Some(ctx));
        assert!(current().is_none());
    }

    #[tokio::test]
    async fn a_scope_with_no_recording_span_has_no_correlation_to_inherit() {
        let active = Active { ctx: SpanContext::root(true), span: None };
        // A signal recorded here falls back to the client's correlation rather
        // than inventing one from a span that is not being recorded.
        assert!(scope(active, async { current_correlation() }).await.is_none());
    }
}
