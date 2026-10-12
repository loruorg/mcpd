//! Built-in scorers.
//!
//! A scorer answers one question about a run and returns a number on a declared
//! scale. Everything here is **deterministic**: the same span always produces
//! the same score, no model is called, and no network is touched. That matters
//! because a scorer that costs money and varies run to run cannot be used as a
//! regression gate, which is the main thing scorers are for.
//!
//! Most of these read the run's answer. Two of them read the run's record
//! instead: [`Builtin::SpanPresent`] and [`Builtin::ToolSucceeded`] ask what
//! the trace holds, never what the answer claims. An agent that ends with "I
//! ran all the tests and there were no regressions" satisfies every scorer that
//! reads the sentence, including a model judge, and satisfies neither of these
//! two if no test ever ran.
//!
//! Their limit is worth stating as plainly as their use. They prove an action
//! was **recorded**, not that it was correct: a `run_tests` span that ended `ok`
//! says the tool returned without error, not that the suite was the right one
//! or that its result was read. And a span exists only because the user's
//! process recorded one, so what these scorers measure is the trace, not the
//! world. Instrumentation that was never added looks exactly like work that
//! never happened, which is the one way they produce a false negative.
//!
//! Judged evaluation, where a model grades the output, is a different tool with
//! different economics. [`JudgeRequest`] describes what such a scorer sends. The
//! judge itself lives in `mcpd-server`, is off until a `judge` block names an
//! endpoint, a model and its criteria, and runs only when a request asks for a
//! configured criterion by name, because a daemon that silently spends your
//! budget is not a daemon anyone should run.

use crate::{Score, ScoreSource, Span, SpanKind, SpanStatus};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// What a scorer needs to look at.
///
/// A whole trace rather than one span, because most useful questions ("did it
/// answer?", "how many tools did it call?") are about the run.
#[derive(Debug, Clone)]
pub struct Target<'a> {
    /// Every span in the run, in start order.
    pub spans: &'a [Span],
}

impl<'a> Target<'a> {
    /// The root span, when the run has one.
    #[must_use]
    pub fn root(&self) -> Option<&Span> {
        self.spans.iter().find(|s| s.is_root).or_else(|| self.spans.first())
    }

    /// The final assistant output, as text, if there is one to read.
    #[must_use]
    pub fn output_text(&self) -> Option<String> {
        // Prefer the root's output: that is the run's answer. Fall back to the
        // last model generation, which is where the answer is when a caller
        // never set one on the root.
        let from_root = self.root().and_then(|s| s.output.as_ref()).and_then(text_of);
        from_root.or_else(|| {
            self.spans
                .iter()
                .rev()
                .find(|s| s.kind == SpanKind::ModelGeneration)
                .and_then(|s| s.output.as_ref())
                .and_then(text_of)
        })
    }

    /// The question the run was asked.
    #[must_use]
    pub fn input_text(&self) -> Option<String> {
        self.root().and_then(|s| s.input.as_ref()).and_then(text_of)
    }

    /// Wall-clock duration of the run in milliseconds.
    #[must_use]
    pub fn duration_ms(&self) -> Option<f64> {
        self.root().and_then(|s| s.duration_ms())
    }

    /// Total cost across the run.
    #[must_use]
    pub fn total_cost(&self) -> f64 {
        self.spans.iter().filter_map(|s| s.cost.as_ref()?.total_cost).sum()
    }

    /// How many spans of a kind the run contains.
    #[must_use]
    pub fn count(&self, kind: SpanKind) -> usize {
        self.spans.iter().filter(|s| s.kind == kind).count()
    }

    /// Whether any span failed.
    #[must_use]
    pub fn had_error(&self) -> bool {
        self.spans.iter().any(|s| s.status == SpanStatus::Error)
    }
}

/// Pull readable text out of a payload.
///
/// Handles the three shapes a completion actually arrives in: a bare string, a
/// `{ text | content | answer }` object, and a chat message list where the last
/// assistant turn is the answer.
fn text_of(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Object(map) => {
            // Both directions: the keys an answer arrives under, and the ones
            // a question does. A payload keyed `question` is as common as one
            // keyed `text`, and missing it made the input side silently empty.
            for key in [
                "text", "content", "answer", "output", "message", "question", "prompt", "query",
                "input",
            ] {
                if let Some(found) = map.get(key).and_then(text_of) {
                    return Some(found);
                }
            }
            if let Some(Value::Array(messages)) = map.get("messages") {
                // The last turn with content is the answer.
                return messages.iter().rev().find_map(|m| {
                    m.get("content").and_then(text_of).filter(|t| !t.trim().is_empty())
                });
            }
            None
        }
        Value::Array(items) => items.iter().rev().find_map(text_of),
        _ => None,
    }
}

/// A scorer's verdict.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Verdict {
    /// The value, always within `[min, max]`.
    pub value: f64,
    /// Lower bound of the scale.
    pub min: f64,
    /// Upper bound of the scale.
    pub max: f64,
    /// Why. A score with no explanation is not arguable, and a score nobody can
    /// argue with is one nobody trusts.
    pub reason: String,
    /// A categorical summary, where one is meaningful.
    pub label: Option<String>,
}

impl Verdict {
    /// A pass/fail verdict on the unit scale.
    #[must_use]
    pub fn binary(pass: bool, reason: impl Into<String>) -> Self {
        Self {
            value: if pass { 1.0 } else { 0.0 },
            min: 0.0,
            max: 1.0,
            reason: reason.into(),
            label: Some(if pass { "pass" } else { "fail" }.to_owned()),
        }
    }

    /// A graded verdict on the unit scale, clamped so a scorer cannot report
    /// outside the scale it declared.
    #[must_use]
    pub fn graded(value: f64, reason: impl Into<String>) -> Self {
        Self {
            value: value.clamp(0.0, 1.0),
            min: 0.0,
            max: 1.0,
            reason: reason.into(),
            label: None,
        }
    }
}

// `ScorerConfig` is generated from the spec, so the SDKs and the API reference
// describe exactly the settings the scorers below actually read.
pub use crate::generated::ScorerConfig;

/// Every scorer mcpd ships.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Builtin {
    /// The run produced a non-empty answer.
    Answered,
    /// No span in the run failed.
    NoErrors,
    /// The answer exactly equals `expected`.
    ExactMatch,
    /// The answer contains `needle`.
    Contains,
    /// The answer is valid JSON.
    JsonValid,
    /// The run finished within `max_ms`.
    LatencyBudget,
    /// The run cost no more than `max_cost`.
    CostBudget,
    /// The run did not loop: no tool was called with the same input twice.
    NoToolLoop,
    /// The run made no more than `max_tool_calls` tool calls.
    ToolEfficiency,
    /// The answer is not a refusal or a non-answer.
    NotARefusal,
    /// The answer does not contain an obvious unredacted secret.
    NoLeakedSecret,
    /// A span of the configured kind, name, or both was recorded in the run.
    SpanPresent,
    /// The tool named by `tool_name` ran, and at least one call of it ended `ok`.
    ToolSucceeded,
}

impl Builtin {
    /// The scorer's stable identifier, as stored on a [`Score`].
    #[must_use]
    pub const fn id(&self) -> &'static str {
        match self {
            Self::Answered => "answered",
            Self::NoErrors => "no_errors",
            Self::ExactMatch => "exact_match",
            Self::Contains => "contains",
            Self::JsonValid => "json_valid",
            Self::LatencyBudget => "latency_budget",
            Self::CostBudget => "cost_budget",
            Self::NoToolLoop => "no_tool_loop",
            Self::ToolEfficiency => "tool_efficiency",
            Self::NotARefusal => "not_a_refusal",
            Self::NoLeakedSecret => "no_leaked_secret",
            Self::SpanPresent => "span_present",
            Self::ToolSucceeded => "tool_succeeded",
        }
    }

    /// One line on what this scorer measures, shown in the interface.
    #[must_use]
    pub const fn description(&self) -> &'static str {
        match self {
            Self::Answered => "The run produced a non-empty answer.",
            Self::NoErrors => "No span in the run failed.",
            Self::ExactMatch => "The answer exactly equals the expected text.",
            Self::Contains => "The answer contains the required text.",
            Self::JsonValid => "The answer parses as JSON.",
            Self::LatencyBudget => "The run finished inside its latency budget.",
            Self::CostBudget => "The run cost no more than its budget.",
            Self::NoToolLoop => "No tool was called twice with the same input.",
            Self::ToolEfficiency => "The run stayed within its tool-call budget.",
            Self::NotARefusal => "The answer is not a refusal or a non-answer.",
            Self::NoLeakedSecret => "The answer carries no obvious unredacted secret.",
            Self::SpanPresent => "The run recorded a span of the required kind or name.",
            Self::ToolSucceeded => "The required tool ran and ended ok.",
        }
    }

    /// Every scorer, for the API and the interface.
    pub const ALL: &'static [Self] = &[
        Self::Answered,
        Self::NoErrors,
        Self::ExactMatch,
        Self::Contains,
        Self::JsonValid,
        Self::LatencyBudget,
        Self::CostBudget,
        Self::NoToolLoop,
        Self::ToolEfficiency,
        Self::NotARefusal,
        Self::NoLeakedSecret,
        Self::SpanPresent,
        Self::ToolSucceeded,
    ];

    /// Parse an identifier.
    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|s| s.id() == id)
    }

    /// Whether this scorer needs configuration to mean anything. A scorer that
    /// silently passes because nothing was configured is worse than absent.
    #[must_use]
    pub const fn needs_config(&self) -> bool {
        matches!(
            self,
            Self::ExactMatch
                | Self::Contains
                | Self::LatencyBudget
                | Self::CostBudget
                | Self::ToolEfficiency
                | Self::SpanPresent
                | Self::ToolSucceeded
        )
    }

    /// The settings this scorer reads from [`ScorerConfig`], and none it does
    /// not. Empty when it needs no configuration.
    ///
    /// This exists so a scorer that declines can say what it was waiting for.
    /// "Needs configuration" leaves the caller to guess which of nine fields
    /// that was. Any one of the listed settings is enough on its own; no scorer
    /// here requires two at once, and one that did would need a richer shape
    /// than a list.
    #[must_use]
    pub const fn config_keys(&self) -> &'static [&'static str] {
        match self {
            Self::ExactMatch => &["expected"],
            Self::Contains => &["needle"],
            Self::LatencyBudget => &["max_ms"],
            Self::CostBudget => &["max_cost"],
            Self::ToolEfficiency => &["max_tool_calls"],
            Self::SpanPresent => &["span_kind", "span_name"],
            Self::ToolSucceeded => &["tool_name"],
            Self::Answered
            | Self::NoErrors
            | Self::JsonValid
            | Self::NoToolLoop
            | Self::NotARefusal
            | Self::NoLeakedSecret => &[],
        }
    }

    /// Run the scorer.
    ///
    /// Returns `None` when the scorer cannot answer: no output to read, or a
    /// required setting missing. Not scoring is the honest outcome there; a
    /// default of zero would read as a failure the run did not have.
    #[must_use]
    pub fn run(&self, target: &Target<'_>, config: &ScorerConfig) -> Option<Verdict> {
        let normalise = |s: &str| {
            if config.case_sensitive {
                s.trim().to_owned()
            } else {
                s.trim().to_lowercase()
            }
        };

        match self {
            Self::Answered => {
                let text = target.output_text().unwrap_or_default();
                Some(Verdict::binary(
                    !text.trim().is_empty(),
                    if text.trim().is_empty() {
                        "the run ended without producing any output".to_owned()
                    } else {
                        format!("answered with {} characters", text.trim().len())
                    },
                ))
            }

            Self::NoErrors => {
                let failed: Vec<&str> = target
                    .spans
                    .iter()
                    .filter(|s| s.status == SpanStatus::Error)
                    .map(|s| s.name.as_str())
                    .collect();
                Some(Verdict::binary(
                    failed.is_empty(),
                    if failed.is_empty() {
                        "every span succeeded".to_owned()
                    } else {
                        format!("{} span(s) failed: {}", failed.len(), failed.join(", "))
                    },
                ))
            }

            Self::ExactMatch => {
                let expected = config.expected.as_ref()?;
                let got = target.output_text()?;
                let hit = normalise(&got) == normalise(expected);
                Some(Verdict::binary(
                    hit,
                    if hit {
                        "the answer matches exactly".to_owned()
                    } else {
                        format!("expected {expected:?}, got {:?}", truncate(&got, 120))
                    },
                ))
            }

            Self::Contains => {
                let needle = config.needle.as_ref()?;
                let got = target.output_text()?;
                let hit = normalise(&got).contains(&normalise(needle));
                Some(Verdict::binary(
                    hit,
                    if hit {
                        format!("the answer contains {needle:?}")
                    } else {
                        format!("the answer never mentions {needle:?}")
                    },
                ))
            }

            Self::JsonValid => {
                let got = target.output_text()?;
                let ok = serde_json::from_str::<Value>(got.trim()).is_ok();
                Some(Verdict::binary(
                    ok,
                    if ok { "the answer is valid JSON" } else { "the answer is not valid JSON" },
                ))
            }

            Self::LatencyBudget => {
                let budget = config.max_ms?;
                let took = target.duration_ms()?;
                Some(Verdict::binary(
                    took <= budget,
                    format!("took {took:.0} ms against a {budget:.0} ms budget"),
                ))
            }

            Self::CostBudget => {
                let budget = config.max_cost?;
                let spent = target.total_cost();
                Some(Verdict::binary(
                    spent <= budget,
                    format!("cost {spent:.5} against a {budget:.5} budget"),
                ))
            }

            Self::NoToolLoop => {
                // A tool called repeatedly with identical input is the classic
                // agent failure: the model is stuck and will not converge.
                let mut seen: BTreeMap<(String, String), usize> = BTreeMap::new();
                for s in target.spans.iter().filter(|s| {
                    matches!(
                        s.kind,
                        SpanKind::ToolCall | SpanKind::McpToolCall | SpanKind::ClientToolCall
                    )
                }) {
                    let input = s
                        .input
                        .as_ref()
                        .map(|v| serde_json::to_string(v).unwrap_or_default())
                        .unwrap_or_default();
                    *seen.entry((s.name.clone(), input)).or_insert(0) += 1;
                }
                let worst = seen.iter().max_by_key(|(_, n)| **n);
                match worst {
                    Some(((name, _), n)) if *n > 1 => Some(Verdict::binary(
                        false,
                        format!("{name} was called {n} times with identical input"),
                    )),
                    _ => Some(Verdict::binary(true, "no tool was repeated with the same input")),
                }
            }

            Self::ToolEfficiency => {
                let budget = config.max_tool_calls? as usize;
                let used = target.count(SpanKind::ToolCall)
                    + target.count(SpanKind::McpToolCall)
                    + target.count(SpanKind::ClientToolCall);
                Some(Verdict::binary(
                    used <= budget,
                    format!("made {used} tool call(s) against a budget of {budget}"),
                ))
            }

            Self::NotARefusal => {
                let got = target.output_text()?;
                let lower = got.to_lowercase();
                // Deliberately narrow. A broad list would flag legitimate
                // answers that happen to contain a hedge, and a scorer with
                // false positives gets switched off.
                const MARKERS: &[&str] = &[
                    "i cannot help",
                    "i can't help",
                    "i'm unable to",
                    "i am unable to",
                    "as an ai language model",
                    "i cannot provide",
                    "i can't provide",
                    "i don't have access",
                ];
                let hit = MARKERS.iter().find(|m| lower.contains(**m));
                Some(Verdict::binary(
                    hit.is_none(),
                    match hit {
                        Some(m) => format!("the answer refuses: {m:?}"),
                        None => "the answer is not a refusal".to_owned(),
                    },
                ))
            }

            Self::NoLeakedSecret => {
                let got = target.output_text()?;
                // Shapes, not a dictionary: a long high-entropy token with a
                // known prefix is a credential whatever service issued it.
                let leaked = got
                    .split(|c: char| c.is_whitespace() || c == '"' || c == '\'')
                    .find(|w| looks_like_a_secret(w));
                Some(Verdict::binary(
                    leaked.is_none(),
                    match leaked {
                        // Eight characters, not eight bytes. A byte cut
                        // inside a multi-byte character panicked, and the
                        // panic failed every scoring pass the run was in.
                        Some(w) => format!(
                            "the answer contains what looks like a credential: {}…",
                            w.chars().take(8).collect::<String>()
                        ),
                        None => "no credential-shaped token in the answer".to_owned(),
                    },
                ))
            }

            Self::SpanPresent => {
                // Neither setting means nothing was asked for. Answering here
                // would mark every run green against a check nobody wrote.
                if config.span_kind.is_none() && config.span_name.is_none() {
                    return None;
                }
                let wanted = span_criterion(config.span_kind, config.span_name.as_deref());
                let found = target
                    .spans
                    .iter()
                    .filter(|s| span_answers(s, config.span_kind, config.span_name.as_deref()))
                    .count();
                Some(Verdict::binary(
                    found > 0,
                    if found > 0 {
                        format!("the run records {found} span(s) of {wanted}")
                    } else if config.span_name.is_some() {
                        // A wrong name and absent instrumentation look identical
                        // from a bare "not found", and they are opposite fixes.
                        format!(
                            "no span of {wanted}; the run records {} span(s), named: {}",
                            target.spans.len(),
                            sample(target.spans.iter().map(|s| s.name.clone())),
                        )
                    } else {
                        format!(
                            "no span of {wanted}; the run records {} span(s), of kinds: {}",
                            target.spans.len(),
                            sample(target.spans.iter().map(|s| s.kind.to_string())),
                        )
                    },
                ))
            }

            Self::ToolSucceeded => {
                let wanted = config.tool_name.as_ref()?;
                let calls: Vec<&Span> = target
                    .spans
                    .iter()
                    .filter(|s| is_tool_call(s.kind) && s.name == *wanted)
                    .collect();
                let Some(last) = calls.last() else {
                    // Never ran and ran-then-failed are both a zero, and they
                    // are different bugs: one is a step the agent skipped, the
                    // other a step that broke. The reason has to say which.
                    let others: Vec<&Span> =
                        target.spans.iter().filter(|s| is_tool_call(s.kind)).collect();
                    let context = if others.is_empty() {
                        "the run records no tool call at all".to_owned()
                    } else {
                        format!(
                            "the run records {} tool call(s), named: {}",
                            others.len(),
                            sample(others.iter().map(|s| s.name.clone())),
                        )
                    };
                    return Some(Verdict::binary(
                        false,
                        format!("{wanted:?} never ran; {context}"),
                    ));
                };
                let n = calls.len();
                let ok = calls.iter().filter(|s| s.status == SpanStatus::Ok).count();
                Some(Verdict::binary(
                    // One success is enough: a tool that failed and was retried
                    // did in the end run and succeed, which is the claim being
                    // checked. The count of failures stays in the reason so a
                    // run that barely passed is still visible.
                    ok > 0,
                    if ok == n {
                        format!("{wanted:?} ran {n} time(s) and ended ok")
                    } else if ok > 0 {
                        format!("{wanted:?} ran {n} time(s), {ok} ended ok and {} did not", n - ok)
                    } else {
                        let detail = last.error.as_ref().map_or_else(
                            || "no error was recorded".to_owned(),
                            |e| truncate(&e.message, 160),
                        );
                        format!(
                            "{wanted:?} ran {n} time(s) and none ended ok; the last ended {}: \
                             {detail}",
                            last.status,
                        )
                    },
                ))
            }
        }
    }

    /// Build a [`Score`] from a verdict, ready to store.
    #[must_use]
    pub fn score(
        &self,
        verdict: &Verdict,
        trace_id: Option<String>,
        span_id: Option<String>,
        source: ScoreSource,
        correlation: crate::Correlation,
        now: time::OffsetDateTime,
    ) -> Score {
        Score {
            score_id: crate::ids::new_trace_id(),
            trace_id,
            span_id,
            scorer_id: self.id().to_owned(),
            scorer_version: Some("1".to_owned()),
            source,
            value: verdict.value,
            min: Some(verdict.min),
            max: Some(verdict.max),
            label: verdict.label.clone(),
            reason: Some(verdict.reason.clone()),
            score_trace_id: None,
            // A deterministic scorer costs nothing, and saying so is worth more
            // than leaving the field blank next to a judged score that does.
            cost: None,
            metadata: None,
            timestamp: now,
            correlation,
        }
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_owned();
    }
    s.chars().take(n).collect::<String>() + "…"
}

/// Whether a token has the shape of a credential.
fn looks_like_a_secret(word: &str) -> bool {
    const PREFIXES: &[&str] = &["sk-", "pk-", "ghp_", "gho_", "xoxb-", "xoxp-", "AKIA", "AIza"];
    if word.len() < 16 {
        return false;
    }
    if PREFIXES.iter().any(|p| word.starts_with(p)) {
        return true;
    }
    // A long run of base64-ish characters with both cases and digits is very
    // unlikely to be prose.
    let alnum = word.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    let has_upper = word.chars().any(|c| c.is_ascii_uppercase());
    let has_lower = word.chars().any(|c| c.is_ascii_lowercase());
    let has_digit = word.chars().any(|c| c.is_ascii_digit());
    alnum && has_upper && has_lower && has_digit && word.len() >= 32
}

/// Whether a span kind is a tool call, in the sense `tool_succeeded` means.
///
/// All four kinds, because which one a run records is a property of where the
/// tool lives rather than of whether it ran. An MCP server, the calling client
/// and the model provider all execute tools, and an assertion about
/// `run_tests` must not start failing because the tool moved behind MCP.
const fn is_tool_call(kind: SpanKind) -> bool {
    matches!(
        kind,
        SpanKind::ToolCall
            | SpanKind::ClientToolCall
            | SpanKind::ProviderToolCall
            | SpanKind::McpToolCall
    )
}

/// Whether one span answers a `span_present` configuration.
///
/// Set together, the kind and the name both have to hold. The name is compared
/// exactly, and `case_sensitive` does not loosen it: a span name is an
/// identifier the producer chose, not prose, and a loose comparison would let
/// `run_tests_dry` satisfy an assertion about `run_tests`, which is the one run
/// the scorer exists to catch.
fn span_answers(span: &Span, kind: Option<SpanKind>, name: Option<&str>) -> bool {
    kind.is_none_or(|k| span.kind == k) && name.is_none_or(|n| span.name == n)
}

/// What a `span_present` configuration was looking for, in words.
fn span_criterion(kind: Option<SpanKind>, name: Option<&str>) -> String {
    match (kind, name) {
        (Some(k), Some(n)) => format!("kind {k} named {n:?}"),
        (Some(k), None) => format!("kind {k}"),
        (None, Some(n)) => format!("any kind named {n:?}"),
        // Refused before this is reached: a criterion with neither half is a
        // check on nothing.
        (None, None) => "no kind and no name".to_owned(),
    }
}

/// A bounded, sorted, de-duplicated list of what a run actually holds.
///
/// The bound is not cosmetic. A reason is stored on every score, so a run with
/// four hundred spans would otherwise write four hundred names into every row
/// the scorer produced.
fn sample(values: impl Iterator<Item = String>) -> String {
    const SHOWN: usize = 6;
    let all: BTreeSet<String> = values.collect();
    if all.is_empty() {
        return "none".to_owned();
    }
    let head = all.iter().take(SHOWN).cloned().collect::<Vec<_>>().join(", ");
    if all.len() > SHOWN {
        format!("{head} and {} more", all.len() - SHOWN)
    } else {
        head
    }
}

/// What a judged scorer would send to a model.
///
/// mcpd builds this and hands it back; it never calls a model itself. A daemon
/// that spends your budget without being asked is not one anyone should run,
/// and an evaluation whose cost is invisible is one nobody can govern.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JudgeRequest {
    /// The scorer this request is for.
    pub scorer_id: String,
    /// The prompt to send.
    pub prompt: String,
    /// The scale the judge must answer on.
    pub min: f64,
    /// Upper bound of that scale.
    pub max: f64,
}

/// Build the prompt for a judged scorer.
///
/// Public so a caller can run it against whatever model they already pay for,
/// then post the answer through the ordinary scores API.
#[must_use]
pub fn judge_request(scorer_id: &str, criterion: &str, target: &Target<'_>) -> JudgeRequest {
    let question = target.input_text().unwrap_or_else(|| "(no input recorded)".to_owned());
    let answer = target.output_text().unwrap_or_else(|| "(no output recorded)".to_owned());
    JudgeRequest {
        scorer_id: scorer_id.to_owned(),
        prompt: format!(
            "Score the answer against this criterion, from 0 to 1.\n\n\
             Criterion: {criterion}\n\n\
             Question:\n{}\n\nAnswer:\n{}\n\n\
             Reply with JSON only: {{\"value\": <number 0-1>, \"reason\": \"<one sentence>\"}}",
            truncate(&question, 4000),
            truncate(&answer, 4000),
        ),
        min: 0.0,
        max: 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use time::OffsetDateTime;

    fn span(kind: SpanKind, name: &str) -> Span {
        let mut s = Span {
            trace_id: "4bf92f3577b34da6a3ce929d0e0e4736".into(),
            span_id: crate::ids::new_span_id(),
            parent_span_id: None,
            external_parent_span_id: None,
            name: name.into(),
            kind,
            status: SpanStatus::Ok,
            start_time: OffsetDateTime::UNIX_EPOCH,
            end_time: Some(OffsetDateTime::UNIX_EPOCH + time::Duration::milliseconds(500)),
            duration_ns: None,
            is_root: kind == SpanKind::AgentRun,
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
            correlation: crate::Correlation {
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
        };
        s.normalise();
        s
    }

    fn run_with_answer(answer: &str) -> Vec<Span> {
        let mut root = span(SpanKind::AgentRun, "agent");
        root.input = Some(json!({ "question": "where is my order" }));
        root.output = Some(json!({ "text": answer }));
        vec![root]
    }

    fn score(b: Builtin, spans: &[Span], cfg: ScorerConfig) -> Option<Verdict> {
        b.run(&Target { spans }, &cfg)
    }

    #[test]
    fn every_scorer_has_an_id_and_a_description() {
        for b in Builtin::ALL {
            assert!(!b.id().is_empty());
            assert!(b.description().ends_with('.'), "{} needs a full sentence", b.id());
            assert_eq!(Builtin::from_id(b.id()), Some(*b));
        }
    }

    #[test]
    fn answered_distinguishes_an_answer_from_silence() {
        let yes = score(
            Builtin::Answered,
            &run_with_answer("It shipped Tuesday."),
            ScorerConfig::default(),
        )
        .unwrap();
        assert_eq!(yes.value, 1.0);
        let no =
            score(Builtin::Answered, &run_with_answer("   "), ScorerConfig::default()).unwrap();
        assert_eq!(no.value, 0.0);
        assert!(no.reason.contains("without producing"));
    }

    #[test]
    fn no_errors_names_the_spans_that_failed() {
        let mut spans = run_with_answer("ok");
        let mut bad = span(SpanKind::ToolCall, "lookup_order");
        bad.status = SpanStatus::Error;
        spans.push(bad);
        let v = score(Builtin::NoErrors, &spans, ScorerConfig::default()).unwrap();
        assert_eq!(v.value, 0.0);
        assert!(v.reason.contains("lookup_order"), "{}", v.reason);
    }

    #[test]
    fn exact_match_is_case_insensitive_unless_asked() {
        let spans = run_with_answer("Tuesday");
        let loose = score(
            Builtin::ExactMatch,
            &spans,
            ScorerConfig { expected: Some("tuesday".into()), ..Default::default() },
        )
        .unwrap();
        assert_eq!(loose.value, 1.0);
        let strict = score(
            Builtin::ExactMatch,
            &spans,
            ScorerConfig {
                expected: Some("tuesday".into()),
                case_sensitive: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(strict.value, 0.0);
    }

    #[test]
    fn a_scorer_that_needs_config_says_which_setting_it_wants() {
        // The two must not drift. A scorer marked as needing configuration but
        // naming no setting produces a decline nobody can act on, and one that
        // names a setting while claiming to need none never gets given it.
        for b in Builtin::ALL {
            assert_eq!(
                b.needs_config(),
                !b.config_keys().is_empty(),
                "{} disagrees with itself about needing configuration",
                b.id()
            );
        }
    }

    #[test]
    fn a_scorer_missing_its_config_declines_rather_than_failing_the_run() {
        // Returning 0 here would look like the run failed the check, when in
        // fact nobody said what the check was.
        for b in Builtin::ALL.iter().filter(|b| b.needs_config()) {
            assert!(
                score(*b, &run_with_answer("hello"), ScorerConfig::default()).is_none(),
                "{} should decline without config",
                b.id()
            );
        }
    }

    #[test]
    fn no_tool_loop_catches_the_classic_agent_failure() {
        let mut spans = run_with_answer("");
        for _ in 0..3 {
            let mut t = span(SpanKind::ToolCall, "returns_policy");
            t.input = Some(json!({}));
            spans.push(t);
        }
        let v = score(Builtin::NoToolLoop, &spans, ScorerConfig::default()).unwrap();
        assert_eq!(v.value, 0.0);
        assert!(v.reason.contains("3 times"), "{}", v.reason);
    }

    #[test]
    fn no_tool_loop_allows_the_same_tool_with_different_input() {
        let mut spans = run_with_answer("ok");
        for i in 0..3 {
            let mut t = span(SpanKind::ToolCall, "lookup");
            t.input = Some(json!({ "id": i }));
            spans.push(t);
        }
        assert_eq!(score(Builtin::NoToolLoop, &spans, ScorerConfig::default()).unwrap().value, 1.0);
    }

    #[test]
    fn budgets_compare_against_the_real_figures() {
        let spans = run_with_answer("ok");
        let fast = score(
            Builtin::LatencyBudget,
            &spans,
            ScorerConfig { max_ms: Some(1000.0), ..Default::default() },
        )
        .unwrap();
        assert_eq!(fast.value, 1.0);
        let slow = score(
            Builtin::LatencyBudget,
            &spans,
            ScorerConfig { max_ms: Some(100.0), ..Default::default() },
        )
        .unwrap();
        assert_eq!(slow.value, 0.0);
        assert!(slow.reason.contains("500"), "{}", slow.reason);
    }

    #[test]
    fn refusal_detection_does_not_fire_on_an_ordinary_hedge() {
        assert_eq!(
            score(
                Builtin::NotARefusal,
                &run_with_answer("I cannot help with that request."),
                ScorerConfig::default()
            )
            .unwrap()
            .value,
            0.0
        );
        // A legitimate answer that happens to hedge must still pass.
        assert_eq!(
            score(
                Builtin::NotARefusal,
                &run_with_answer("I am not certain, but the depot scan suggests Tuesday."),
                ScorerConfig::default()
            )
            .unwrap()
            .value,
            1.0
        );
    }

    #[test]
    fn leaked_secret_detection_is_shape_based() {
        assert_eq!(
            score(
                Builtin::NoLeakedSecret,
                &run_with_answer("your key is sk-live-abcdefghijklmnop"),
                ScorerConfig::default()
            )
            .unwrap()
            .value,
            0.0
        );
        assert_eq!(
            score(
                Builtin::NoLeakedSecret,
                &run_with_answer("The order shipped on Tuesday from the Leeds depot."),
                ScorerConfig::default()
            )
            .unwrap()
            .value,
            1.0
        );
    }

    #[test]
    fn a_leaked_secret_with_a_multi_byte_character_near_its_start_is_reported_not_a_panic() {
        // Byte 8 of `sk-abc€€€€€€€` is inside the first `€`, where a byte cut
        // panicked and turned `POST /v1/api/score` into a 500.
        let v = score(
            Builtin::NoLeakedSecret,
            &run_with_answer("your key is sk-abc€€€€€€€ ok"),
            ScorerConfig::default(),
        )
        .unwrap();
        assert_eq!(v.value, 0.0);
        assert!(v.reason.ends_with("credential: sk-abc€€…"), "{}", v.reason);
    }

    #[test]
    fn output_text_reads_a_chat_message_list() {
        let mut root = span(SpanKind::AgentRun, "agent");
        root.output = Some(json!({ "messages": [
            { "role": "user", "content": "hi" },
            { "role": "assistant", "content": "It shipped Tuesday." }
        ]}));
        let spans = vec![root];
        let t = Target { spans: &spans };
        assert_eq!(t.output_text().as_deref(), Some("It shipped Tuesday."));
    }

    #[test]
    fn a_verdict_cannot_report_outside_its_own_scale() {
        assert_eq!(Verdict::graded(4.2, "x").value, 1.0);
        assert_eq!(Verdict::graded(-1.0, "x").value, 0.0);
    }

    /// A run whose answer claims work that its trace does not record.
    ///
    /// This is the failure the trace-grounded scorers exist for: the sentence is
    /// confident, and nothing behind it ran.
    fn run_that_claims_it_tested() -> Vec<Span> {
        run_with_answer("I ran all the tests and there were no regressions.")
    }

    fn with_tool(mut spans: Vec<Span>, name: &str, status: SpanStatus) -> Vec<Span> {
        let mut t = span(SpanKind::ToolCall, name);
        t.status = status;
        if status == SpanStatus::Error {
            t.error = Some(crate::SpanError {
                message: "pytest exited 1: 3 failed".into(),
                kind: Some("CalledProcessError".into()),
                stack: None,
                domain: None,
                retryable: Some(false),
                details: None,
            });
        }
        spans.push(t);
        spans
    }

    #[test]
    fn a_claim_in_the_answer_cannot_satisfy_a_trace_grounded_scorer() {
        let spans = run_that_claims_it_tested();
        // A scorer that reads the sentence believes it.
        assert_eq!(
            score(
                Builtin::Contains,
                &spans,
                ScorerConfig { needle: Some("no regressions".into()), ..Default::default() }
            )
            .unwrap()
            .value,
            1.0
        );
        // A scorer that reads the trace does not.
        let grounded = score(
            Builtin::ToolSucceeded,
            &spans,
            ScorerConfig { tool_name: Some("run_tests".into()), ..Default::default() },
        )
        .unwrap();
        assert_eq!(grounded.value, 0.0);
        assert!(grounded.reason.contains("never ran"), "{}", grounded.reason);
    }

    #[test]
    fn span_present_declines_when_it_was_told_nothing_to_look_for() {
        // Neither a kind nor a name is not "look for anything", it is a
        // question nobody asked. Passing here would mark every run green.
        assert!(score(Builtin::SpanPresent, &run_that_claims_it_tested(), ScorerConfig::default())
            .is_none());
    }

    #[test]
    fn span_present_finds_a_span_by_kind_by_name_and_by_both() {
        let spans = with_tool(run_that_claims_it_tested(), "run_tests", SpanStatus::Ok);
        for cfg in [
            ScorerConfig { span_kind: Some(SpanKind::ToolCall), ..Default::default() },
            ScorerConfig { span_name: Some("run_tests".into()), ..Default::default() },
            ScorerConfig {
                span_kind: Some(SpanKind::ToolCall),
                span_name: Some("run_tests".into()),
                ..Default::default()
            },
        ] {
            let v = score(Builtin::SpanPresent, &spans, cfg).unwrap();
            assert_eq!(v.value, 1.0, "{}", v.reason);
        }
    }

    #[test]
    fn span_present_narrows_on_both_settings_at_once() {
        // The name is on a memory operation, not a tool call. Asking for both
        // must not be satisfied by either half.
        let mut spans = run_that_claims_it_tested();
        spans.push(span(SpanKind::MemoryOperation, "run_tests"));
        spans.push(span(SpanKind::ToolCall, "lookup_order"));
        let v = score(
            Builtin::SpanPresent,
            &spans,
            ScorerConfig {
                span_kind: Some(SpanKind::ToolCall),
                span_name: Some("run_tests".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(v.value, 0.0, "{}", v.reason);
    }

    #[test]
    fn span_present_matches_a_name_exactly() {
        // A span name is an identifier the producer chose, not prose. If
        // `run_tests_dry` satisfied an assertion about `run_tests`, the scorer
        // would pass on the one run it exists to catch.
        let spans = with_tool(run_that_claims_it_tested(), "run_tests_dry", SpanStatus::Ok);
        let v = score(
            Builtin::SpanPresent,
            &spans,
            ScorerConfig { span_name: Some("run_tests".into()), ..Default::default() },
        )
        .unwrap();
        assert_eq!(v.value, 0.0, "{}", v.reason);
    }

    #[test]
    fn span_present_says_what_the_run_does_hold_when_it_finds_nothing() {
        // A bare "not found" leaves you unable to tell a wrong name from
        // missing instrumentation.
        let spans = with_tool(run_that_claims_it_tested(), "lookup_order", SpanStatus::Ok);
        let v = score(
            Builtin::SpanPresent,
            &spans,
            ScorerConfig {
                span_kind: Some(SpanKind::ToolCall),
                span_name: Some("run_tests".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(v.value, 0.0);
        assert!(v.reason.contains("run_tests"), "{}", v.reason);
        assert!(v.reason.contains("tool_call"), "{}", v.reason);
        assert!(v.reason.contains("lookup_order"), "{}", v.reason);
    }

    #[test]
    fn tool_succeeded_tells_a_tool_that_failed_apart_from_one_that_never_ran() {
        let cfg = || ScorerConfig { tool_name: Some("run_tests".into()), ..Default::default() };

        let never = score(Builtin::ToolSucceeded, &run_that_claims_it_tested(), cfg()).unwrap();
        let failed = score(
            Builtin::ToolSucceeded,
            &with_tool(run_that_claims_it_tested(), "run_tests", SpanStatus::Error),
            cfg(),
        )
        .unwrap();

        // Both are failures of the claim, and they are different bugs: one is
        // an agent that skipped the step, the other is a step that broke.
        assert_eq!(never.value, 0.0);
        assert_eq!(failed.value, 0.0);
        assert_ne!(never.reason, failed.reason);
        assert!(never.reason.contains("never ran"), "{}", never.reason);
        assert!(!failed.reason.contains("never ran"), "{}", failed.reason);
        assert!(failed.reason.contains("none ended ok"), "{}", failed.reason);
        // The reason carries the recorded error, so the score is actionable
        // without opening the run.
        assert!(failed.reason.contains("pytest exited 1"), "{}", failed.reason);
    }

    #[test]
    fn tool_succeeded_passes_when_a_retry_finally_worked() {
        let spans = with_tool(
            with_tool(run_that_claims_it_tested(), "run_tests", SpanStatus::Error),
            "run_tests",
            SpanStatus::Ok,
        );
        let v = score(
            Builtin::ToolSucceeded,
            &spans,
            ScorerConfig { tool_name: Some("run_tests".into()), ..Default::default() },
        )
        .unwrap();
        assert_eq!(v.value, 1.0, "{}", v.reason);
        // The failed attempt is still reported: it passed, but not cleanly.
        assert!(v.reason.contains('2'), "{}", v.reason);
    }

    #[test]
    fn the_reasons_the_documentation_quotes_are_the_reasons_the_scorer_produces() {
        // web/src/pages/docs/evaluation.astro prints these three strings as the
        // three outcomes of `tool_succeeded`. A documented example that has
        // drifted from what the code produces teaches something untrue and no
        // other test would catch it, so the exact text is pinned here.
        let cfg = || ScorerConfig { tool_name: Some("run_tests".into()), ..Default::default() };

        let mut never = run_that_claims_it_tested();
        never = with_tool(never, "lookup_order", SpanStatus::Ok);
        never = with_tool(never, "send_email", SpanStatus::Ok);
        assert_eq!(
            score(Builtin::ToolSucceeded, &never, cfg()).unwrap().reason,
            "\"run_tests\" never ran; the run records 2 tool call(s), \
             named: lookup_order, send_email"
        );

        let failed = with_tool(run_that_claims_it_tested(), "run_tests", SpanStatus::Error);
        assert_eq!(
            score(Builtin::ToolSucceeded, &failed, cfg()).unwrap().reason,
            "\"run_tests\" ran 1 time(s) and none ended ok; \
             the last ended error: pytest exited 1: 3 failed"
        );

        let retried = with_tool(failed, "run_tests", SpanStatus::Ok);
        assert_eq!(
            score(Builtin::ToolSucceeded, &retried, cfg()).unwrap().reason,
            "\"run_tests\" ran 2 time(s), 1 ended ok and 1 did not"
        );
    }

    #[test]
    fn tool_succeeded_ignores_a_span_that_is_not_a_tool_call() {
        // A workflow step named after the tool is not evidence the tool ran.
        let mut spans = run_that_claims_it_tested();
        spans.push(span(SpanKind::WorkflowStep, "run_tests"));
        let v = score(
            Builtin::ToolSucceeded,
            &spans,
            ScorerConfig { tool_name: Some("run_tests".into()), ..Default::default() },
        )
        .unwrap();
        assert_eq!(v.value, 0.0, "{}", v.reason);
        assert!(v.reason.contains("never ran"), "{}", v.reason);
    }

    #[test]
    fn tool_succeeded_counts_every_tool_kind_a_provider_can_record() {
        for kind in [
            SpanKind::ToolCall,
            SpanKind::ClientToolCall,
            SpanKind::ProviderToolCall,
            SpanKind::McpToolCall,
        ] {
            let mut spans = run_that_claims_it_tested();
            spans.push(span(kind, "run_tests"));
            let v = score(
                Builtin::ToolSucceeded,
                &spans,
                ScorerConfig { tool_name: Some("run_tests".into()), ..Default::default() },
            )
            .unwrap();
            assert_eq!(v.value, 1.0, "{kind} should count as a tool call: {}", v.reason);
        }
    }

    #[test]
    fn a_judge_request_carries_the_question_and_the_answer() {
        let spans = run_with_answer("It shipped Tuesday.");
        let r = judge_request(
            "answer_relevance",
            "Does the answer address the question?",
            &Target { spans: &spans },
        );
        assert!(r.prompt.contains("where is my order"));
        assert!(r.prompt.contains("It shipped Tuesday."));
        assert!(r.prompt.contains("Does the answer address"));
        assert_eq!((r.min, r.max), (0.0, 1.0));
    }
}
