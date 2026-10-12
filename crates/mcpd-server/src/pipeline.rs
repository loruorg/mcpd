//! The intake pipeline.
//!
//! Everything that must happen to a signal between arriving and being stored,
//! in one place: sampling, redaction, identifier hashing, cost, and metric
//! label hygiene.
//!
//! It lives here rather than inside an HTTP handler because there is more than
//! one way into the store. When this logic sat in the ingest handler, the demo
//! seeder wrote unredacted, unpriced rows straight past it, and the two paths
//! disagreed about what a stored span looks like. Anything that writes goes
//! through [`Intake::prepare`].

use mcpd_core::cost::PriceBook;
use mcpd_core::metrics::CardinalityFilter;
use mcpd_core::redact::Redactor;
use mcpd_core::sampling::{Decision, Policy};
use mcpd_core::{Feedback, LogRecord, MetricPoint, SamplingKind, Score};
use mcpd_store::Batch;
use std::sync::OnceLock;
use time::OffsetDateTime;

/// The empty map an intake without per-project policies points at, so the field
/// can be a reference without every caller owning a map.
static EMPTY_POLICIES: OnceLock<std::collections::BTreeMap<String, Policy>> = OnceLock::new();

/// What a pass over a batch did, for metrics and for the caller's response.
#[derive(Debug, Default, Clone, Copy)]
pub struct Prepared {
    /// Spans discarded by the sampling policy.
    pub sampled_out: u64,
    /// Metric labels removed for being unbounded.
    pub labels_dropped: u64,
    /// Logs, metrics, scores and feedback discarded because the trace they name
    /// was sampled out.
    ///
    /// Counted apart from `sampled_out` rather than folded into it: the ingest
    /// metric that reads that field is published as the number of traces the
    /// sampler discarded, and four other signal kinds arriving in the same
    /// counter would change what an operator's existing dashboard means.
    pub signals_sampled_out: u64,
}

/// The pipeline every write passes through.
pub struct Intake<'a> {
    /// Per-project sampling policies, consulted before the global one.
    ///
    /// The decision is per trace and a trace belongs to one project, so this is
    /// a lookup rather than a merge. One team's traffic should not decide how
    /// another team is sampled, which is the whole reason a project is a
    /// tenancy boundary.
    pub per_project: &'a std::collections::BTreeMap<String, Policy>,
    /// Redaction and identifier hashing.
    pub redactor: &'a Redactor,
    /// Rates used to price model calls at ingest.
    pub price_book: &'a PriceBook,
    /// Sampling policy, applied per trace at the root.
    pub sampling: &'a Policy,
    /// Metric label hygiene.
    pub cardinality: CardinalityFilter,
}

impl<'a> Intake<'a> {
    /// Build an intake from the pieces the server already holds.
    #[must_use]
    pub fn new(redactor: &'a Redactor, price_book: &'a PriceBook, sampling: &'a Policy) -> Self {
        Self {
            per_project: EMPTY_POLICIES.get_or_init(std::collections::BTreeMap::new),
            redactor,
            price_book,
            sampling,
            cardinality: CardinalityFilter::default(),
        }
    }

    /// The same intake, with per-project sampling policies applied.
    #[must_use]
    pub fn with_projects(
        mut self,
        per_project: &'a std::collections::BTreeMap<String, Policy>,
    ) -> Self {
        self.per_project = per_project;
        self
    }

    /// The policy that governs one project.
    fn policy_for(&self, project: &str) -> &Policy {
        self.per_project.get(project).unwrap_or(self.sampling)
    }

    /// Drop the signals in `v` whose trace the sampler discarded, and say how
    /// many went.
    fn retain_sampled_in<T: Signal>(
        &self,
        v: &mut Vec<T>,
        decided: &mut std::collections::HashMap<String, bool>,
    ) -> u64 {
        let mut dropped = 0;
        v.retain(|s| {
            let keep = self.keep_signal(decided, s.trace(), s.project());
            if !keep {
                dropped += 1;
            }
            keep
        });
        dropped
    }

    /// Whether one signal survives the decision that governs the trace it names.
    ///
    /// `decided` holds what the span pass already resolved for this batch, so a
    /// signal that arrived beside its spans is answered from that and never
    /// asks the policy a second time.
    fn keep_signal(
        &self,
        decided: &mut std::collections::HashMap<String, bool>,
        trace_id: Option<&str>,
        project: &str,
    ) -> bool {
        // A signal that names no trace cannot be orphaned, because there is no
        // trace for it to point at. An aggregate metric with no exemplar is the
        // ordinary case, not a broken one, and dropping it would delete a
        // reading the sampler was never asked about.
        let Some(trace_id) = trace_id else { return true };
        if let Some(keep) = decided.get(trace_id) {
            return *keep;
        }
        // Nothing in this batch decided this trace. That is not evidence the
        // trace is absent: its spans may have arrived in an earlier batch and
        // been kept, and dropping the signal for arriving alone would lose real
        // data about a run that is on disk. So ask the policy again. A sampler
        // that answers a trace id the same way every time is not taking a new
        // decision here, it is recalling the one the trace already got.
        let policy = self.policy_for(project);
        if !recallable(policy) {
            return true;
        }
        let keep = !matches!(policy.decide(trace_id, None), Decision::Drop);
        decided.insert(trace_id.to_owned(), keep);
        keep
    }

    /// Apply the whole pipeline to a batch, in place.
    ///
    /// Sampling takes signals out of the batch, and it is the only step here
    /// that does. A caller that reads a record back out afterwards must take it
    /// with `first()` rather than by index: a batch holding one score whose
    /// trace was sampled out comes back empty, and an empty batch is the
    /// correct answer there, not a missing element.
    ///
    /// The per-project ceiling is deliberately **not** applied here, even though
    /// that would enforce it on every door in one edit. Ingest applies the
    /// ceiling after this call, where the caller reads what was cut; a door that
    /// writes one record charges it with `ingest::charge_one` and refuses, so it
    /// can answer the sender rather than silently storing less than it claimed.
    pub fn prepare(&self, batch: &mut Batch) -> Prepared {
        let now = OffsetDateTime::now_utc();
        let mut out = Prepared::default();

        // Sampling first: there is no point redacting or pricing a span that is
        // about to be discarded. The decision is per trace, so a trace is never
        // half-recorded.
        // Skipped entirely when nothing samples anything, which is the default
        // and the common case.
        let samples_anything = !matches!(self.sampling.kind, SamplingKind::Always)
            || self.per_project.values().any(|p| !matches!(p.kind, SamplingKind::Always));
        if samples_anything {
            // Keyed by trace alone, not by trace and project: a trace belongs to
            // one project, which is the same assumption the per-project lookup
            // above is built on.
            let mut decided: std::collections::HashMap<String, bool> =
                std::collections::HashMap::new();
            batch.spans.retain(|s| {
                let keep = *decided.entry(s.trace_id.clone()).or_insert_with(|| {
                    let policy = self.policy_for(&s.correlation.project);
                    !matches!(policy.decide(&s.trace_id, None), Decision::Drop)
                });
                if !keep {
                    out.sampled_out += 1;
                }
                keep
            });

            // The other four signals go with the trace they name. A log line
            // that outlives its own trace is worse than a missing one: it is a
            // stored row whose `trace_id` and `span_id` resolve to nothing, and
            // a reader meets it as a broken link rather than as a sampling
            // policy. What is stored has to be internally consistent, so the
            // decision that discarded the spans discards these too.
            //
            // Spans are decided first, so a signal that travelled in the same
            // batch as its spans obeys the identical decision rather than a
            // freshly taken one.
            out.signals_sampled_out += self.retain_sampled_in(&mut batch.logs, &mut decided);
            out.signals_sampled_out += self.retain_sampled_in(&mut batch.metrics, &mut decided);
            out.signals_sampled_out += self.retain_sampled_in(&mut batch.scores, &mut decided);
            out.signals_sampled_out += self.retain_sampled_in(&mut batch.feedback, &mut decided);
            out.signals_sampled_out +=
                self.retain_sampled_in(&mut batch.aspect_notes, &mut decided);
        }

        for span in &mut batch.spans {
            span.correlation.user_id =
                self.redactor.hash_identifier(span.correlation.user_id.as_deref());
            span.input = self.redactor.redact_opt(&span.input);
            span.output = self.redactor.redact_opt(&span.output);
            span.attributes = self.redactor.redact_opt(&span.attributes);
            span.metadata = self.redactor.redact_opt(&span.metadata);

            // Price it now, from the book this server is configured with, so a
            // later rate change cannot silently rewrite what last month cost.
            if span.cost.is_none() {
                if let (Some(model), Some(usage)) = (span.model.as_deref(), span.usage.as_ref()) {
                    span.cost = mcpd_core::cost::compute(self.price_book, model, usage);
                }
            }
            if span.received_at == OffsetDateTime::UNIX_EPOCH {
                span.received_at = now;
            }
            span.normalise();
        }

        for log in &mut batch.logs {
            log.correlation.user_id =
                self.redactor.hash_identifier(log.correlation.user_id.as_deref());
            log.fields = self.redactor.redact_opt(&log.fields);
            if log.received_at == OffsetDateTime::UNIX_EPOCH {
                log.received_at = now;
            }
        }

        for m in &mut batch.metrics {
            m.labels = self.redactor.redact_opt(&m.labels);
            let dropped = self.cardinality.filter(&mut m.labels);
            out.labels_dropped += dropped.keys.len() as u64;
            if m.received_at == OffsetDateTime::UNIX_EPOCH {
                m.received_at = now;
            }
        }

        for s in &mut batch.scores {
            s.correlation.user_id = self.redactor.hash_identifier(s.correlation.user_id.as_deref());
            s.metadata = self.redactor.redact_opt(&s.metadata);
        }

        for f in &mut batch.feedback {
            f.correlation.user_id = self.redactor.hash_identifier(f.correlation.user_id.as_deref());
            f.metadata = self.redactor.redact_opt(&f.metadata);
            f.author_id = self.redactor.hash_identifier(f.author_id.as_deref());
            f.comment = f.comment.as_ref().map(|c| self.redactor.redact_text(c));
        }

        // An aspect note is a sentence about what a user asked for, so its text
        // is payload and goes through the redactor like every other stored
        // string. What the redactor can do to a bare sentence is the length
        // bound: its rules match on keys, and a sentence has none. That is
        // stated on `Redactor::redact_text` rather than left to be discovered,
        // because it is the difference between "redacted" and "bounded".
        //
        // The subject is hashed here as it is on every other signal, which is
        // what makes an erasure able to find a note by the person it is about
        // rather than only by the run it describes.
        for n in &mut batch.aspect_notes {
            n.correlation.user_id = self.redactor.hash_identifier(n.correlation.user_id.as_deref());
            n.text = self.redactor.redact_text(&n.text);
        }

        out
    }
}

/// The two things the sampler needs from a signal that is not a span: the trace
/// it points at, and the project whose policy governs it.
trait Signal {
    /// The trace this signal names, if it names one.
    fn trace(&self) -> Option<&str>;
    /// The project whose sampling policy applies.
    fn project(&self) -> &str;
}

impl Signal for LogRecord {
    fn trace(&self) -> Option<&str> {
        self.trace_id.as_deref()
    }
    fn project(&self) -> &str {
        &self.correlation.project
    }
}

impl Signal for MetricPoint {
    fn trace(&self) -> Option<&str> {
        self.trace_id.as_deref()
    }
    fn project(&self) -> &str {
        &self.correlation.project
    }
}

impl Signal for Score {
    fn trace(&self) -> Option<&str> {
        self.trace_id.as_deref()
    }
    fn project(&self) -> &str {
        &self.correlation.project
    }
}

impl Signal for Feedback {
    fn trace(&self) -> Option<&str> {
        self.trace_id.as_deref()
    }
    fn project(&self) -> &str {
        &self.correlation.project
    }
}

impl Signal for mcpd_core::AspectNote {
    /// Always a trace, unlike the four above. A note is a sentence about one
    /// run and cannot exist without it, so a note whose trace the sampler
    /// discarded is prose about a run that is not stored, which is exactly what
    /// this trait exists to prevent.
    fn trace(&self) -> Option<&str> {
        Some(&self.trace_id)
    }
    fn project(&self) -> &str {
        &self.correlation.project
    }
}

/// Whether this policy's head decision can be asked for again later and give
/// back the answer it gave the first time.
///
/// This is what lets a signal that arrives after its spans be decided at all.
/// `ratio` is the exception: it flips a coin per call, so asking it again is a
/// new decision rather than the old one recalled, and a signal answered that way
/// would be discarded at the sampling rate whatever happened to its trace. At
/// its two extremes it does not flip, so there it can be recalled. `tail` defers
/// at the head and a deferred trace is kept, which is the same answer every
/// time.
fn recallable(policy: &Policy) -> bool {
    match policy.kind {
        SamplingKind::Always | SamplingKind::Never | SamplingKind::Deterministic => true,
        // Its own arm, because it is true for a different reason: `tail` never
        // answers at the head, and the caller keeps what it defers.
        SamplingKind::Tail => true,
        SamplingKind::Ratio => policy.probability >= 1.0 || policy.probability <= 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcpd_core::*;
    use serde_json::json;

    fn span_with(model: &str, user: &str) -> Span {
        let mut s = Span {
            trace_id: "4bf92f3577b34da6a3ce929d0e0e4736".into(),
            span_id: "00f067aa0ba902b7".into(),
            parent_span_id: None,
            external_parent_span_id: None,
            name: "gen".into(),
            kind: SpanKind::ModelGeneration,
            status: SpanStatus::Ok,
            start_time: OffsetDateTime::UNIX_EPOCH,
            end_time: Some(OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(1)),
            duration_ns: None,
            is_root: true,
            is_event: false,
            entity_kind: None,
            entity_id: None,
            entity_name: None,
            input: Some(json!({ "api_key": "sk-live-leak", "prompt": "hello" })),
            output: None,
            attributes: None,
            metadata: None,
            usage: Some(TokenUsage {
                input_tokens: Some(1_000_000),
                output_tokens: Some(0),
                total_tokens: None,
                input_details: None,
                output_details: None,
            }),
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
                user_id: Some(user.into()),
                resource_id: None,
                run_id: None,
                request_id: None,
                experiment_id: None,
                release: None,
            },
            model: Some(model.into()),
            provider: Some("openai".into()),
            received_at: OffsetDateTime::UNIX_EPOCH,
            ingest_key: None,
        };
        s.normalise();
        s
    }

    fn intake_over(batch: &mut Batch) -> Prepared {
        let r = Redactor::with_defaults();
        let b = mcpd_core::cost::builtin_book();
        let p = Policy::always();
        Intake::new(&r, &b, &p).prepare(batch)
    }

    #[test]
    fn redacts_prices_and_hashes_in_one_pass() {
        let mut batch =
            Batch { spans: vec![span_with("gpt-4o", "customer-9")], ..Batch::default() };
        intake_over(&mut batch);
        let s = &batch.spans[0];
        assert_eq!(s.input.as_ref().unwrap()["api_key"], json!("[redacted]"));
        assert_eq!(s.input.as_ref().unwrap()["prompt"], json!("hello"));
        assert!(s.cost.is_some(), "a known model must be priced at ingest");
        let uid = s.correlation.user_id.as_deref().unwrap();
        assert!(uid.starts_with("sha256:"), "user id must be hashed, got {uid}");
    }

    #[test]
    fn the_same_user_hashes_the_same_so_grouping_survives() {
        let mut a = Batch { spans: vec![span_with("gpt-4o", "customer-9")], ..Batch::default() };
        let mut b = Batch { spans: vec![span_with("gpt-4o", "customer-9")], ..Batch::default() };
        intake_over(&mut a);
        intake_over(&mut b);
        assert_eq!(a.spans[0].correlation.user_id, b.spans[0].correlation.user_id);
    }

    #[test]
    fn drops_unbounded_metric_labels() {
        let mut batch = Batch {
            metrics: vec![MetricPoint {
                metric_id: "m1".into(),
                name: "x".into(),
                kind: MetricKind::Counter,
                value: 1.0,
                unit: None,
                timestamp: OffsetDateTime::UNIX_EPOCH,
                labels: Some(json!({ "model": "gpt-4o", "trace_id": "abc" })),
                buckets: None,
                trace_id: None,
                span_id: None,
                correlation: span_with("gpt-4o", "u").correlation,
                received_at: OffsetDateTime::UNIX_EPOCH,
            }],
            ..Batch::default()
        };
        let out = intake_over(&mut batch);
        assert_eq!(out.labels_dropped, 1);
        let labels = batch.metrics[0].labels.as_ref().unwrap();
        assert!(labels.get("trace_id").is_none());
        assert_eq!(labels["model"], json!("gpt-4o"));
    }

    #[test]
    fn sampling_drops_whole_traces_not_halves() {
        let r = Redactor::with_defaults();
        let b = mcpd_core::cost::builtin_book();
        let never = Policy { kind: SamplingKind::Never, ..Policy::default() };
        let mut batch = Batch {
            spans: vec![span_with("gpt-4o", "u"), span_with("gpt-4o", "u")],
            ..Batch::default()
        };
        let out = Intake::new(&r, &b, &never).prepare(&mut batch);
        assert_eq!(out.sampled_out, 2);
        assert!(batch.spans.is_empty());
    }
}
