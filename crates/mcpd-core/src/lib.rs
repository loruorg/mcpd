//! # mcpd-core
//!
//! The data model every other part of mcpd is built on, plus the four pieces of
//! logic that must behave identically wherever they run: identifier handling,
//! cost accounting, redaction, and sampling.
//!
//! [`generated`] is produced from `spec/mcpd.spec.json` and must never be edited
//! by hand. Everything else in this crate is written against those types.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod cost;
pub mod generated;
pub mod histogram;
pub mod ids;
pub mod judged;
pub mod metrics;
pub mod otlp;
pub mod price_catalogue;
pub mod redact;
pub mod sampling;
pub mod scoring;
pub mod sketch;
pub mod stats;

pub use generated::*;

impl Correlation {
    /// A correlation naming only a project and an environment, which is all a
    /// metric point stores.
    #[must_use]
    pub fn for_project(project: String, environment: Option<String>) -> Self {
        Self {
            project,
            environment,
            service_name: None,
            service_version: None,
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
}

/// The crate version, which is also the released mcpd version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Anything that went wrong inside the core model.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// An identifier was not the shape the spec requires.
    #[error("invalid {kind} id {value:?}: {reason}")]
    InvalidId {
        /// Which kind of id: `trace` or `span`.
        kind: &'static str,
        /// The value that was rejected.
        value: String,
        /// Why it was rejected.
        reason: &'static str,
    },

    /// A span could not be accepted as written.
    #[error("invalid span: {0}")]
    InvalidSpan(String),

    /// An OTLP payload could not be understood.
    #[error("malformed OTLP payload: {0}")]
    Otlp(String),
}

/// Convenience alias.
pub type Result<T> = std::result::Result<T, Error>;

impl Span {
    /// Whether this span has finished.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.end_time.is_some()
    }

    /// Duration in milliseconds, when the span has ended.
    #[must_use]
    pub fn duration_ms(&self) -> Option<f64> {
        self.duration_ns.map(|ns| ns as f64 / 1_000_000.0)
    }

    /// Recompute `duration_ns` and `is_root` from the other fields.
    ///
    /// Called on every span at ingest so that these two columns are always
    /// consistent with the timestamps and parentage, whatever a client sent.
    pub fn normalise(&mut self) {
        self.is_root = self.parent_span_id.is_none();
        self.duration_ns = match (self.end_time, self.start_time) {
            (Some(end), start) => {
                let d = end - start;
                // A negative duration means clock skew on the producer. Clamp to
                // zero rather than storing a value that will corrupt every
                // aggregate that reads it.
                Some(d.whole_nanoseconds().max(0) as u64)
            }
            _ => None,
        };
        if self.is_event {
            self.end_time = None;
            self.duration_ns = None;
        }
    }

    /// Total tokens across input and output, when any were reported.
    #[must_use]
    pub fn total_tokens(&self) -> Option<u64> {
        let u = self.usage.as_ref()?;
        match (u.input_tokens, u.output_tokens, u.total_tokens) {
            (_, _, Some(t)) => Some(t),
            (a, b, None) if a.is_some() || b.is_some() => Some(a.unwrap_or(0) + b.unwrap_or(0)),
            _ => None,
        }
    }
}

impl SpanKind {
    /// Whether this kind of span consumes model tokens, and therefore whether
    /// cost accounting should look at it.
    #[must_use]
    pub const fn bills_tokens(&self) -> bool {
        matches!(
            self,
            Self::ModelGeneration | Self::ModelStep | Self::ModelInference | Self::RagEmbedding
        )
    }

    /// Whether this kind is normally the root of a trace. Used to pick a
    /// display name for a trace whose true root has not arrived yet.
    #[must_use]
    pub const fn is_usually_root(&self) -> bool {
        matches!(self, Self::AgentRun | Self::WorkflowRun)
    }

    /// A short human label, used in the UI and in CLI output.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::AgentRun => "Agent",
            Self::ModelGeneration => "Generation",
            Self::ModelStep => "Step",
            Self::ModelInference => "Inference",
            Self::ModelChunk => "Chunk",
            Self::ToolCall => "Tool",
            Self::ClientToolCall => "Client tool",
            Self::ProviderToolCall => "Provider tool",
            Self::McpToolCall => "MCP tool",
            Self::Handoff => "Handoff",
            Self::WorkflowRun => "Workflow",
            Self::WorkflowStep => "Step",
            Self::WorkflowConditional => "Branch",
            Self::WorkflowConditionalEval => "Condition",
            Self::WorkflowParallel => "Parallel",
            Self::WorkflowLoop => "Loop",
            Self::WorkflowSleep => "Sleep",
            Self::WorkflowWaitEvent => "Wait",
            Self::MemoryOperation => "Memory",
            Self::RagIngestion => "Ingestion",
            Self::RagEmbedding => "Embedding",
            Self::RagVectorOperation => "Vector",
            Self::RagAction => "RAG",
            Self::GraphAction => "Graph",
            Self::ProcessorRun => "Processor",
            Self::ScorerRun => "Scorer",
            Self::ScorerStep => "Scorer step",
            Self::Guardrail => "Guardrail",
            Self::Retrieval => "Retrieval",
            Self::Mapping => "Mapping",
            Self::SkillResolution => "Skills",
            Self::WorkspaceAction => "Workspace",
            Self::HttpRequest => "HTTP",
            Self::Generic => "Span",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::OffsetDateTime;

    fn span() -> Span {
        Span {
            trace_id: "0".repeat(32),
            span_id: "0".repeat(16),
            parent_span_id: None,
            external_parent_span_id: None,
            name: "t".into(),
            kind: SpanKind::Generic,
            status: SpanStatus::Unset,
            start_time: OffsetDateTime::UNIX_EPOCH,
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
            model_parameters: None,
            error: None,
            links: None,
            tags: None,
            correlation: Correlation {
                project: "default".into(),
                environment: None,
                service_name: None,
                service_version: None,
                session_id: None,
                thread_id: None,
                user_id: None,
                resource_id: None,
                run_id: None,
                request_id: None,
                experiment_id: None,
                release: None,
            },
            model: None,
            provider: None,
            received_at: OffsetDateTime::UNIX_EPOCH,
            ingest_key: None,
        }
    }

    #[test]
    fn normalise_sets_root_and_duration() {
        let mut s = span();
        s.end_time = Some(OffsetDateTime::UNIX_EPOCH + time::Duration::milliseconds(250));
        s.normalise();
        assert!(s.is_root);
        assert_eq!(s.duration_ns, Some(250_000_000));
        assert_eq!(s.duration_ms(), Some(250.0));
    }

    #[test]
    fn negative_duration_is_clamped_not_stored() {
        // A producer whose clock ran backwards must not poison aggregates.
        let mut s = span();
        s.start_time = OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(10);
        s.end_time = Some(OffsetDateTime::UNIX_EPOCH);
        s.normalise();
        assert_eq!(s.duration_ns, Some(0));
    }

    #[test]
    fn event_spans_never_carry_an_end() {
        let mut s = span();
        s.is_event = true;
        s.end_time = Some(OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(1));
        s.normalise();
        assert!(s.end_time.is_none());
        assert!(s.duration_ns.is_none());
    }

    #[test]
    fn child_span_is_not_root() {
        let mut s = span();
        s.parent_span_id = Some("f".repeat(16));
        s.normalise();
        assert!(!s.is_root);
    }

    #[test]
    fn total_tokens_prefers_reported_total() {
        let mut s = span();
        s.usage = Some(TokenUsage {
            input_tokens: Some(10),
            output_tokens: Some(5),
            total_tokens: Some(99),
            input_details: None,
            output_details: None,
        });
        assert_eq!(s.total_tokens(), Some(99));
    }

    #[test]
    fn total_tokens_sums_when_no_total_given() {
        let mut s = span();
        s.usage = Some(TokenUsage {
            input_tokens: Some(10),
            output_tokens: Some(5),
            total_tokens: None,
            input_details: None,
            output_details: None,
        });
        assert_eq!(s.total_tokens(), Some(15));
    }

    #[test]
    fn every_span_kind_has_a_label() {
        for k in SpanKind::ALL {
            assert!(!k.label().is_empty(), "{k} has no label");
        }
    }
}
