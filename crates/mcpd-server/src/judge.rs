//! Judged evaluation: a model grades a stored run.
//!
//! The deterministic scorers in `mcpd_core::scoring` are ordinary code. They
//! cost nothing, never leave the process, and give the same answer twice. A
//! judge does none of those things, and that is the whole reason it is a
//! separate module with its own configuration: turning it on spends money on
//! every run it reads, so it is off until somebody says otherwise, and it never
//! runs unless a scorer is named explicitly.
//!
//! ## Why an OpenAI-shaped request
//!
//! `POST /v1/chat/completions` is not chosen out of preference for a vendor. It
//! is the shape that Ollama, vLLM, llama.cpp, LM Studio, OpenRouter, Together,
//! Groq and the major hosted providers all accept, so one implementation
//! reaches a laptop running a local model and a production gateway alike. The
//! same reasoning as forwarding: one protocol beats eleven integrations that go
//! stale.
//!
//! ## What it refuses to do
//!
//! A judge that returns a number it did not derive is worse than no judge,
//! because it looks like evidence. So a reply that is not the JSON that was
//! asked for is a decline with the reason attached, never a default score, and
//! a value outside the declared scale is a decline rather than a clamp.

use crate::state::AppState;
use mcpd_core::judged::{Judged, StepDecline};
use mcpd_core::scoring::{judge_request, ScorerConfig, Target};
use mcpd_core::{Correlation, Score, ScoreSource, Span, SpanKind, SpanLink, SpanStatus};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use time::OffsetDateTime;

/// A judged score and the model call that produced it.
///
/// The call is kept so it can be stored as a run of its own: a score that
/// looks wrong is debugged by reading the prompt the judge was given and the
/// reply it wrote, like any other model call.
#[derive(Debug, Clone)]
pub struct Graded {
    /// The score, with no `score_trace_id` yet.
    pub score: Score,
    /// The model call behind it.
    pub call: JudgeCall,
}

/// One judge call, as a scorer run records it.
#[derive(Debug, Clone)]
pub struct JudgeCall {
    /// The prompt the judge was sent.
    pub prompt: String,
    /// The reply it wrote.
    pub reply: String,
    /// Which model answered.
    pub model: String,
    /// What the endpoint reported it used.
    pub usage: Option<mcpd_core::TokenUsage>,
    /// When the first attempt started.
    pub started: OffsetDateTime,
    /// When the reply arrived.
    pub ended: OffsetDateTime,
}

impl Graded {
    /// The `scorer_run` span for this score, and the score pointing at it.
    ///
    /// A trace of its own, not a step inside the run it scored: adding the
    /// judge's tokens and cost to the scored run would change the very
    /// figures a reader compares runs by. A link names the scored run, and
    /// the span carries the scored run's correlation so it lands in the same
    /// project and environment.
    #[must_use]
    pub fn into_parts(mut self) -> (Score, Span) {
        let trace_id = mcpd_core::ids::new_trace_id();
        let started = self.call.started;
        let ended = self.call.ended.max(started);
        let span = Span {
            trace_id: trace_id.clone(),
            span_id: mcpd_core::ids::new_span_id(),
            parent_span_id: None,
            external_parent_span_id: None,
            name: format!("score {}", self.score.scorer_id),
            kind: SpanKind::ScorerRun,
            status: SpanStatus::Ok,
            start_time: started,
            end_time: Some(ended),
            duration_ns: u64::try_from((ended - started).whole_nanoseconds()).ok(),
            is_root: true,
            is_event: false,
            entity_kind: Some(mcpd_core::EntityKind::Scorer),
            entity_id: Some(self.score.scorer_id.clone()),
            entity_name: Some(self.score.scorer_id.clone()),
            input: Some(serde_json::json!({ "prompt": self.call.prompt })),
            output: Some(serde_json::json!({
                "reply": self.call.reply,
                "value": self.score.value,
                "reason": self.score.reason,
            })),
            attributes: Some(serde_json::json!({ "mcpd.score_id": self.score.score_id })),
            metadata: None,
            usage: self.call.usage.clone(),
            cost: self.score.cost.clone(),
            model_parameters: None,
            error: None,
            links: self.score.trace_id.clone().map(|t| {
                vec![SpanLink {
                    trace_id: t,
                    span_id: self.score.span_id.clone(),
                    kind: Some("scores".to_owned()),
                    attributes: None,
                }]
            }),
            tags: None,
            correlation: self.score.correlation.clone(),
            model: Some(self.call.model.clone()),
            provider: None,
            received_at: OffsetDateTime::now_utc(),
            ingest_key: None,
        };
        self.score.score_trace_id = Some(trace_id);
        (self.score, span)
    }
}

/// Where the judge sends its prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JudgeConfig {
    /// Base URL of an OpenAI-compatible API, without the path. `/v1/chat/completions`
    /// is appended, so `http://localhost:11434/v1` reaches Ollama.
    pub endpoint: String,
    /// Which model grades. A judge's answers are only comparable to answers
    /// from the same model, which is why this is recorded on every score.
    pub model: String,
    /// Sent as `Authorization: Bearer`. Empty for a local model that wants none.
    #[serde(default)]
    pub api_key: String,
    /// The criteria this deployment can grade against, by scorer id.
    ///
    /// A fixed set rather than free text on the request: a scorer id is stored
    /// on every score it produces and is what later comparisons group by, so
    /// letting each call invent both the id and the question would produce a
    /// column nobody can trust.
    #[serde(default)]
    pub criteria: std::collections::BTreeMap<String, String>,
    /// The longest one grading call may take, in seconds, from connecting to
    /// the last byte of the reply. A run that is not graded in time is
    /// declined as unreachable, with this key named.
    ///
    /// Its own setting because a model is slower than anything else the daemon
    /// calls. Grading shared the ten seconds the forwarding client allows a
    /// collector, which a local model reading a long run can need several
    /// times over, and the only way to change it was to rebuild.
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
}

/// How long a grading call may take when the config file does not say.
pub const DEFAULT_TIMEOUT_SECONDS: u64 = 30;

/// The longest a grading call may be allowed, in seconds: five minutes. Past
/// it a stalled endpoint holds a scoring request open for longer than any
/// client waits for an answer.
pub const TIMEOUT_SECONDS_MAX: u64 = 300;

/// The most of a judge's reply mcpd reads, in bytes.
///
/// A verdict is a JSON object of a number and a sentence, a few hundred bytes
/// inside a chat completion. The reply was read whole with no limit, so an
/// endpoint that streamed without end, or a proxy that answered with a large
/// page, was held in memory in full before anything looked at it.
pub const REPLY_BYTES_MAX: usize = 1024 * 1024;

const fn default_timeout() -> u64 {
    DEFAULT_TIMEOUT_SECONDS
}

impl JudgeConfig {
    /// Refuse a timeout that cannot work: zero gives no call time to connect,
    /// and one past the bound outlasts every client.
    ///
    /// # Errors
    /// Returns a message naming the key and the range it must be in.
    pub fn check(&self) -> Result<(), String> {
        if self.timeout_seconds == 0 || self.timeout_seconds > TIMEOUT_SECONDS_MAX {
            return Err(format!(
                "judge.timeout_seconds must be from 1 to {TIMEOUT_SECONDS_MAX}, got {}; omit it \
                 for the default of {DEFAULT_TIMEOUT_SECONDS}",
                self.timeout_seconds
            ));
        }
        Ok(())
    }
}

/// The subset of a chat completion mcpd reads.
#[derive(Deserialize)]
struct Completion {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: Message,
}

#[derive(Deserialize)]
struct Message {
    content: Option<String>,
}

/// What a judge is asked to reply with.
#[derive(Deserialize)]
struct Verdict {
    value: f64,
    #[serde(default)]
    reason: String,
}

/// Why a judged score was not produced.
///
/// Every arm names something a reader can act on. "the judge failed" would send
/// somebody to the logs to work out which of these it was.
#[derive(Debug)]
pub enum Declined {
    /// No judge is configured, so a judged scorer cannot run at all.
    NotConfigured,
    /// The scorer id is not one of the configured criteria.
    UnknownCriterion(String),
    /// The run holds no answer to grade.
    NothingToGrade,
    /// The model could not be reached, or refused the request.
    Unreachable(String),
    /// The model answered with something other than the JSON it was asked for.
    Unparsable(String),
    /// The model answered outside the scale it was given.
    OutOfScale(f64),
    /// The reply was larger than [`REPLY_BYTES_MAX`], so it was not read.
    Oversized,
    /// The request deadline had too little time left to make the call.
    OutOfTime,
    /// A step of a judged-library scorer could not produce what the next
    /// step needs.
    Step(StepDecline),
}

impl std::fmt::Display for Declined {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotConfigured => {
                write!(f, "no judge is configured; set `judge` in the config file")
            }
            Self::UnknownCriterion(id) => {
                write!(f, "{id} is not a configured criterion; add it under `judge.criteria`")
            }
            Self::NothingToGrade => write!(f, "the run recorded no answer to grade"),
            Self::Unreachable(e) => write!(f, "the judge model could not be reached: {e}"),
            Self::Unparsable(s) => {
                write!(f, "the judge did not answer with the JSON it was asked for: {s}")
            }
            Self::OutOfScale(v) => {
                write!(f, "the judge answered {v}, which is outside the 0 to 1 scale it was given")
            }
            Self::Oversized => write!(
                f,
                "the judge's reply was larger than the {REPLY_BYTES_MAX} bytes mcpd reads; a \
                 verdict is a few hundred bytes, so check that judge.endpoint is an \
                 OpenAI-compatible API"
            ),
            Self::OutOfTime => write!(
                f,
                "the request deadline left no time to call the judge for this run; send fewer \
                 runs per request, or raise --request-timeout-seconds"
            ),
            Self::Step(d) => write!(f, "{d}"),
        }
    }
}

/// Pull the JSON object out of a model's reply.
///
/// Models wrap JSON in prose and in fenced code blocks however firmly they are
/// asked not to, so the first balanced `{...}` is taken rather than requiring
/// the whole reply to parse. Anything with no object in it at all is a decline.
fn extract_json(reply: &str) -> Option<&str> {
    let start = reply.find('{')?;
    let mut depth = 0_i32;
    let mut in_string = false;
    let mut escaped = false;
    for (i, c) in reply[start..].char_indices() {
        if in_string {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&reply[start..=start + i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// The least time worth starting a call with. Less than this cannot carry a
/// connection and a reply, so the call would only be paid for and cut off.
const MIN_CALL: std::time::Duration = std::time::Duration::from_millis(100);

/// How many times one grading call is tried when the endpoint answers that it
/// is busy (429, 502, 503 or 504) or the connection fails before a reply.
/// A call that times out is not retried: it may already have been paid for.
pub const ATTEMPTS: u32 = 3;

/// What one chat call returned.
struct Reply {
    /// The assistant message's text.
    content: String,
    /// The token counts the endpoint reported, when it reported any.
    usage: Option<mcpd_core::TokenUsage>,
    /// How many attempts the call took.
    attempts: u32,
}

/// The token counts an OpenAI-shaped completion reports.
fn usage_of(body: &serde_json::Value) -> Option<mcpd_core::TokenUsage> {
    let u = body.get("usage")?;
    let n = |k: &str| u.get(k).and_then(serde_json::Value::as_u64);
    let cached = u
        .get("prompt_tokens_details")
        .and_then(|d| d.get("cached_tokens"))
        .and_then(serde_json::Value::as_u64);
    let input = n("prompt_tokens");
    Some(mcpd_core::TokenUsage {
        input_tokens: input,
        output_tokens: n("completion_tokens"),
        total_tokens: n("total_tokens"),
        input_details: cached.map(|c| mcpd_core::InputTokenDetails {
            cache_read: Some(c),
            text: input.map(|i| i.saturating_sub(c)),
            ..Default::default()
        }),
        output_details: None,
    })
}

/// Send one prompt to the judge and read the reply, retrying a busy endpoint.
///
/// `latest` is when the call must have finished, so the request it serves can
/// still answer inside its deadline. Each attempt is held to whichever comes
/// first, that or `judge.timeout_seconds`, and the decline names the one that
/// cut it short.
async fn chat(
    state: &Arc<AppState>,
    cfg: &JudgeConfig,
    prompt: &str,
    latest: std::time::Instant,
) -> Result<Reply, Declined> {
    let mut attempt = 0;
    loop {
        attempt += 1;
        match chat_once(state, cfg, prompt, latest).await {
            Ok((content, usage)) => return Ok(Reply { content, usage, attempts: attempt }),
            Err((declined, retryable)) => {
                let backoff = std::time::Duration::from_millis(250 * (1 << (attempt - 1)));
                let room = latest.saturating_duration_since(std::time::Instant::now());
                if !retryable || attempt >= ATTEMPTS || room < backoff + MIN_CALL {
                    return Err(declined);
                }
                tokio::time::sleep(backoff).await;
            }
        }
    }
}

/// One attempt. The flag says whether a failure is worth another attempt.
async fn chat_once(
    state: &Arc<AppState>,
    cfg: &JudgeConfig,
    prompt: &str,
    latest: std::time::Instant,
) -> Result<(String, Option<mcpd_core::TokenUsage>), (Declined, bool)> {
    let mut http = state
        .outbound
        .post(format!("{}/chat/completions", cfg.endpoint.trim_end_matches('/')))
        .json(&serde_json::json!({
            "model": cfg.model,
            // Zero, because a judge that gives a different number for the same
            // run each time cannot be compared with itself, which is the only
            // thing scores are for.
            "temperature": 0,
            "messages": [{ "role": "user", "content": prompt }],
        }));
    if !cfg.api_key.is_empty() {
        http = http.header("Authorization", format!("Bearer {}", cfg.api_key));
    }

    // Per request, so it beats the client's own ten seconds, which are the
    // forwarding client's and were chosen for a collector, not a model.
    //
    // And never past `latest`. The run cap divides the deadline by this
    // timeout and leaves nothing over, so a judge that never answered used the
    // whole deadline and the client got a 503 instead of the declines.
    let configured = std::time::Duration::from_secs(cfg.timeout_seconds.max(1));
    let left = latest.saturating_duration_since(std::time::Instant::now());
    if left < MIN_CALL {
        return Err((Declined::OutOfTime, false));
    }
    let cut_by_deadline = left < configured;
    let wait = configured.min(left);
    let unreachable = |e: reqwest::Error| {
        Declined::Unreachable(if e.is_timeout() && cut_by_deadline {
            format!(
                "no answer within {:.1}s, which is all the request deadline left; send fewer \
                 runs per request, or raise --request-timeout-seconds",
                wait.as_secs_f64()
            )
        } else if e.is_timeout() {
            format!(
                "no answer within {}s; raise judge.timeout_seconds if the model needs longer",
                wait.as_secs()
            )
        } else {
            // Without the URL: a judge endpoint is where a key in a query
            // string is most likely to be, and this reason is handed back to
            // the caller.
            crate::outbound_error(e)
        })
    };
    let res = match http.timeout(wait).send().await {
        Ok(r) => r,
        Err(e) => {
            let retry = e.is_connect();
            return Err((unreachable(e), retry));
        }
    };
    let status = res.status();
    if !status.is_success() {
        // The body is only there to be quoted, so a part of it is enough, and
        // a body too large to read is a refusal with nothing to quote.
        let body = read_capped(res, &unreachable).await.unwrap_or_default();
        let body = String::from_utf8_lossy(&body);
        let busy = matches!(status.as_u16(), 429 | 502 | 503 | 504);
        return Err((Declined::Unreachable(format!("{status}: {}", truncate(&body))), busy));
    }
    let body = read_capped(res, &unreachable).await.map_err(|d| (d, false))?;

    let raw: serde_json::Value =
        serde_json::from_slice(&body).map_err(|e| (Declined::Unparsable(e.to_string()), false))?;
    let completion: Completion = serde_json::from_value(raw.clone())
        .map_err(|e| (Declined::Unparsable(e.to_string()), false))?;
    let reply =
        completion.choices.first().and_then(|c| c.message.content.clone()).unwrap_or_default();
    Ok((reply, usage_of(&raw)))
}

/// What a judge call cost, priced with the daemon's own book, and the record
/// of the call kept on the score.
fn call_record(
    state: &AppState,
    cfg: &JudgeConfig,
    reply: &Reply,
) -> (Option<mcpd_core::Cost>, serde_json::Value) {
    let cost = reply
        .usage
        .as_ref()
        .and_then(|u| mcpd_core::cost::compute(&state.price_book, &cfg.model, u));
    let record = serde_json::json!({
        "model": cfg.model,
        "attempts": reply.attempts,
        "usage": reply.usage,
    });
    (cost, record)
}

/// Grade one run against one configured criterion.
///
/// # Errors
/// Returns [`Declined`] rather than an error type, because none of these is a
/// server fault: each is a reason the caller should see next to the run it
/// applies to.
pub async fn grade(
    state: &Arc<AppState>,
    scorer_id: &str,
    target: &Target<'_>,
    trace_id: &str,
    correlation: Correlation,
    now: OffsetDateTime,
    latest: std::time::Instant,
) -> Result<Graded, Declined> {
    let cfg = state.config.judge.as_ref().ok_or(Declined::NotConfigured)?;
    let criterion = cfg
        .criteria
        .get(scorer_id)
        .ok_or_else(|| Declined::UnknownCriterion(scorer_id.to_owned()))?;
    let version = cfg.model.clone();
    grade_criterion(
        state,
        scorer_id,
        criterion,
        &version,
        target,
        trace_id,
        correlation,
        now,
        latest,
    )
    .await
}

/// Grade one run against a criterion's text, under a scorer id and version.
///
/// What [`grade`] does for a criterion in the config file, and what a stored
/// judge definition does with its own prompt.
///
/// # Errors
/// Returns [`Declined`] for the same reasons as [`grade`].
#[allow(clippy::too_many_arguments)]
pub async fn grade_criterion(
    state: &Arc<AppState>,
    scorer_id: &str,
    criterion: &str,
    version: &str,
    target: &Target<'_>,
    trace_id: &str,
    correlation: Correlation,
    now: OffsetDateTime,
    latest: std::time::Instant,
) -> Result<Graded, Declined> {
    let cfg = state.config.judge.as_ref().ok_or(Declined::NotConfigured)?;

    // Nothing to grade is a decline, not a zero. A run with no answer has not
    // scored badly on relevance; it has not been measured.
    if target.output_text().is_none_or(|t| t.trim().is_empty()) {
        return Err(Declined::NothingToGrade);
    }

    let req = judge_request(scorer_id, criterion, target);
    let started = OffsetDateTime::now_utc();
    let reply = chat(state, cfg, &req.prompt, latest).await?;
    let ended = OffsetDateTime::now_utc();

    let json = extract_json(&reply.content)
        .ok_or_else(|| Declined::Unparsable(truncate(&reply.content)))?;
    let verdict: Verdict =
        serde_json::from_str(json).map_err(|_| Declined::Unparsable(truncate(&reply.content)))?;

    if !verdict.value.is_finite() || verdict.value < req.min || verdict.value > req.max {
        return Err(Declined::OutOfScale(verdict.value));
    }
    let (cost, call) = call_record(state, cfg, &reply);
    let judge_call = JudgeCall {
        prompt: req.prompt.clone(),
        reply: reply.content.clone(),
        model: cfg.model.clone(),
        usage: reply.usage.clone(),
        started,
        ended,
    };

    let score = Score {
        score_id: mcpd_core::ids::new_trace_id(),
        trace_id: Some(trace_id.to_owned()),
        span_id: None,
        scorer_id: scorer_id.to_owned(),
        // For a configured criterion the model is the version: two scores
        // from different judges are not comparable, and this is what says so
        // on the record. A stored definition adds its own number to it.
        scorer_version: Some(version.to_owned()),
        source: ScoreSource::Offline,
        value: verdict.value,
        min: Some(req.min),
        max: Some(req.max),
        label: None,
        reason: (!verdict.reason.trim().is_empty()).then_some(verdict.reason),
        score_trace_id: None,
        cost,
        metadata: Some(serde_json::json!({ "judge": call })),
        timestamp: now,
        correlation,
    };
    Ok(Graded { score, call: judge_call })
}

/// Grade one run with a scorer from the judged library, in its four steps.
///
/// The model is called once, at the analyze step. A decline names the step
/// it happened at, and the score keeps every itemised verdict, so the number
/// can be checked against the workings.
///
/// # Errors
/// [`Declined`] as for [`grade`], and [`Declined::Step`] for a step that could
/// not produce what the next one needs.
#[allow(clippy::too_many_arguments)]
pub async fn grade_judged(
    state: &Arc<AppState>,
    scorer: Judged,
    config: &ScorerConfig,
    target: &Target<'_>,
    trace_id: &str,
    correlation: Correlation,
    now: OffsetDateTime,
    latest: std::time::Instant,
) -> Result<Graded, Declined> {
    let cfg = state.config.judge.as_ref().ok_or(Declined::NotConfigured)?;
    let inputs = scorer.preprocess(target, config).map_err(Declined::Step)?;
    let prompt = scorer.analyze_prompt(&inputs);
    let started = OffsetDateTime::now_utc();
    let reply = chat(state, cfg, &prompt, latest).await?;
    let ended = OffsetDateTime::now_utc();
    let json = extract_json(&reply.content).ok_or_else(|| {
        Declined::Step(StepDecline {
            step: "analyze",
            reason: format!("the judge did not answer with JSON: {}", truncate(&reply.content)),
        })
    })?;
    let items = scorer.parse_analysis(json).map_err(Declined::Step)?;
    let (value, reason) = scorer.score(&items).map_err(Declined::Step)?;
    let (cost, call) = call_record(state, cfg, &reply);
    let judge_call = JudgeCall {
        prompt,
        reply: reply.content.clone(),
        model: cfg.model.clone(),
        usage: reply.usage.clone(),
        started,
        ended,
    };
    let score = Score {
        score_id: mcpd_core::ids::new_trace_id(),
        trace_id: Some(trace_id.to_owned()),
        span_id: None,
        scorer_id: scorer.id().to_owned(),
        // The rubric and the model together are the version: a change to
        // either makes the scores incomparable.
        scorer_version: Some(format!("{}:{}", scorer.version(), cfg.model)),
        source: ScoreSource::Offline,
        value,
        min: Some(0.0),
        max: Some(1.0),
        label: None,
        reason: Some(reason),
        score_trace_id: None,
        cost,
        metadata: Some(serde_json::json!({
            "judge": call,
            "steps": {
                "preprocess": {
                    "question": inputs.question.is_some(),
                    "answer": inputs.answer.is_some(),
                    "expected": inputs.expected.is_some(),
                    "context_passages": inputs.context.len(),
                },
                "analyze": { "items": items },
                "score": { "value": value },
                "reason": { "text": reason_text(&items) },
            }
        })),
        timestamp: now,
        correlation,
    };
    Ok(Graded { score, call: judge_call })
}

/// How many items took each verdict, for the reason step's record.
fn reason_text(items: &[mcpd_core::judged::Item]) -> String {
    let count = |v: &str| items.iter().filter(|i| i.verdict == v).count();
    format!("{} yes, {} no, {} unsure", count("yes"), count("no"), count("unsure"))
}

/// Read a reply, refusing it once it passes [`REPLY_BYTES_MAX`].
///
/// A declared length past the cap is refused before a byte is read, and an
/// undeclared one as the reading crosses it, so what is held is never more
/// than the cap and one chunk.
async fn read_capped(
    mut res: reqwest::Response,
    unreachable: &impl Fn(reqwest::Error) -> Declined,
) -> Result<Vec<u8>, Declined> {
    if res.content_length().is_some_and(|n| n > REPLY_BYTES_MAX as u64) {
        return Err(Declined::Oversized);
    }
    let mut out = Vec::new();
    while let Some(chunk) = res.chunk().await.map_err(unreachable)? {
        if out.len() + chunk.len() > REPLY_BYTES_MAX {
            return Err(Declined::Oversized);
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

/// Keep a bad reply short enough to read in a response body.
fn truncate(s: &str) -> String {
    let s = s.trim();
    if s.chars().count() <= 200 {
        return s.to_owned();
    }
    s.chars().take(200).collect::<String>() + "..."
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_is_found_inside_prose_and_fences() {
        assert_eq!(
            extract_json("Sure! ```json\n{\"value\": 0.8}\n``` hope that helps"),
            Some("{\"value\": 0.8}")
        );
        assert_eq!(extract_json("{\"a\":{\"b\":1}}"), Some("{\"a\":{\"b\":1}}"));
    }

    #[test]
    fn a_brace_inside_a_string_does_not_end_the_object() {
        // The reason field is written by a model and routinely contains braces
        // and escaped quotes. Counting them would truncate the object early and
        // turn a good answer into a decline.
        let r = r#"{"value": 1, "reason": "it said \"{done}\" at the end"}"#;
        assert_eq!(extract_json(r), Some(r));
    }

    #[test]
    fn a_reply_with_no_object_is_not_guessed_at() {
        assert_eq!(extract_json("I cannot grade this."), None);
        assert_eq!(extract_json("{\"unclosed\": 1"), None);
    }
}
