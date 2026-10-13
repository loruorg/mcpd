//! Sample data for `mcpd demo`.
//!
//! The first thing anyone does with an observability tool is open it, and an
//! empty screen teaches nothing. This writes one realistic support-agent run —
//! nested spans, a real prompt and completion, token usage, a tool call, a
//! retrieval, a failure, a score and a piece of human feedback — so the
//! interface can be understood in the first ten seconds.
//!
//! Everything here is clearly synthetic and labelled as such.

use anyhow::Result;
use mcpd_core::cost::PriceBook;
use mcpd_core::redact::Redactor;
use mcpd_core::sampling::Policy;
use mcpd_core::*;
use mcpd_server::pipeline::Intake;
use mcpd_server::AppState;
use mcpd_store::{Batch, Store};
use serde_json::json;
use std::sync::Arc;
use time::{Duration, OffsetDateTime};

fn corr(project: &str, session: &str) -> Correlation {
    Correlation {
        project: project.to_owned(),
        environment: Some("production".to_owned()),
        service_name: Some("support-agent".to_owned()),
        service_version: Some("1.4.2".to_owned()),
        session_id: Some(session.to_owned()),
        thread_id: Some(format!("{session}-t1")),
        user_id: Some("demo-user".to_owned()),
        resource_id: None,
        run_id: None,
        request_id: None,
        experiment_id: None,
        release: Some("1.4.2".to_owned()),
    }
}

#[allow(clippy::too_many_arguments)]
fn span(
    trace: &str,
    id: &str,
    parent: Option<&str>,
    name: &str,
    kind: SpanKind,
    start: OffsetDateTime,
    ms: i64,
    project: &str,
    session: &str,
) -> Span {
    let mut s = Span {
        trace_id: trace.to_owned(),
        span_id: id.to_owned(),
        parent_span_id: parent.map(str::to_owned),
        external_parent_span_id: None,
        name: name.to_owned(),
        kind,
        status: SpanStatus::Ok,
        start_time: start,
        end_time: Some(start + Duration::milliseconds(ms)),
        duration_ns: None,
        is_root: parent.is_none(),
        is_event: false,
        entity_kind: None,
        entity_id: None,
        entity_name: None,
        input: None,
        output: None,
        attributes: None,
        metadata: Some(json!({ "synthetic": true, "source": "mcpd demo" })),
        usage: None,
        cost: None,
        model_parameters: None,
        error: None,
        links: None,
        tags: parent.is_none().then(|| vec!["demo".to_owned(), "support".to_owned()]),
        correlation: corr(project, session),
        model: None,
        provider: None,
        received_at: OffsetDateTime::now_utc(),
        ingest_key: None,
    };
    s.normalise();
    s
}

/// Write the demo run. Returns how many signals were stored.
pub async fn seed(
    store: &dyn Store,
    project: &str,
    redactor: &Redactor,
    price_book: &PriceBook,
    sampling: &Policy,
) -> Result<usize> {
    let now = OffsetDateTime::now_utc();
    let mut batch = Batch::default();

    // ---- Run one: a support question answered correctly. ----
    let t1 = "7a41f0c2e5b8493da1c0f6d27e34b915";
    let base = now - Duration::minutes(4);

    let mut root = span(
        t1,
        "9a1b2c3d4e5f6071",
        None,
        "support_agent",
        SpanKind::AgentRun,
        base,
        3180,
        project,
        "sess-demo-1",
    );
    root.entity_kind = Some(EntityKind::Agent);
    root.entity_name = Some("support_agent".to_owned());
    root.input = Some(
        json!({ "question": "My order #4417 hasn't arrived and the tracking hasn't moved in five days." }),
    );
    root.output = Some(
        json!({ "answer": "Order #4417 was scanned into the Leeds depot on the 3rd and has not moved since. I've opened a trace with the courier (ref TR-88213) and you'll have an update within 24 hours. If it doesn't move by Friday I'll send a replacement at no cost." }),
    );
    batch.spans.push(root);

    let mut recall = span(
        t1,
        "9a1b2c3d4e5f6072",
        Some("9a1b2c3d4e5f6071"),
        "recall_conversation",
        SpanKind::MemoryOperation,
        base + Duration::milliseconds(20),
        34,
        project,
        "sess-demo-1",
    );
    recall.output = Some(json!({ "messages_recalled": 6, "working_memory": true }));
    recall.attributes = Some(json!({ "operation": "recall", "last_messages": 10 }));
    batch.spans.push(recall);

    let mut retrieval = span(
        t1,
        "9a1b2c3d4e5f6073",
        Some("9a1b2c3d4e5f6071"),
        "search_order_policy",
        SpanKind::RagVectorOperation,
        base + Duration::milliseconds(70),
        128,
        project,
        "sess-demo-1",
    );
    retrieval.input = Some(json!({ "query": "late delivery replacement policy", "top_k": 4 }));
    retrieval.output = Some(json!({ "matches": 4, "top_score": 0.83, "index": "policies-v3" }));
    retrieval.attributes = Some(
        json!({ "operation": "query", "store": "embedded", "index_name": "policies-v3", "top_k": 4 }),
    );
    batch.spans.push(retrieval);

    let mut gen1 = span(
        t1,
        "9a1b2c3d4e5f6074",
        Some("9a1b2c3d4e5f6071"),
        "chat gpt-4o",
        SpanKind::ModelGeneration,
        base + Duration::milliseconds(210),
        1120,
        project,
        "sess-demo-1",
    );
    gen1.model = Some("gpt-4o".to_owned());
    gen1.provider = Some("openai".to_owned());
    gen1.entity_kind = Some(EntityKind::Model);
    gen1.entity_name = Some("gpt-4o".to_owned());
    gen1.input = Some(json!({ "messages": [
        { "role": "system", "content": "You are a support agent for a UK retailer. Be specific. Never promise a refund you cannot authorise." },
        { "role": "user", "content": "My order #4417 hasn't arrived and the tracking hasn't moved in five days." }
    ]}));
    gen1.output = Some(
        json!({ "role": "assistant", "tool_calls": [ { "name": "lookup_order", "arguments": { "order_id": "4417" } } ] }),
    );
    gen1.usage = Some(TokenUsage {
        input_tokens: Some(1284),
        output_tokens: Some(42),
        total_tokens: None,
        input_details: Some(InputTokenDetails {
            text: Some(284),
            cache_read: Some(1000),
            ..Default::default()
        }),
        output_details: None,
    });
    gen1.attributes = Some(
        json!({ "temperature": 0.2, "max_output_tokens": 1024, "finish_reason": "tool_calls", "streaming": true }),
    );
    batch.spans.push(gen1);

    let mut tool = span(
        t1,
        "9a1b2c3d4e5f6075",
        Some("9a1b2c3d4e5f6071"),
        "lookup_order",
        SpanKind::ToolCall,
        base + Duration::milliseconds(1350),
        412,
        project,
        "sess-demo-1",
    );
    tool.entity_kind = Some(EntityKind::Tool);
    tool.entity_name = Some("lookup_order".to_owned());
    tool.input = Some(json!({ "order_id": "4417" }));
    tool.output = Some(
        json!({ "status": "in_transit", "last_scan": "2026-08-03T09:14:00Z", "depot": "Leeds", "courier": "Northbound" }),
    );
    tool.attributes = Some(json!({ "tool_type": "function", "success": true }));
    batch.spans.push(tool);

    let mut courier = span(
        t1,
        "9a1b2c3d4e5f6076",
        Some("9a1b2c3d4e5f6075"),
        "GET /couriers/northbound/track",
        SpanKind::HttpRequest,
        base + Duration::milliseconds(1400),
        340,
        project,
        "sess-demo-1",
    );
    courier.attributes = Some(
        json!({ "http.request.method": "GET", "http.response.status_code": 200, "server.address": "api.northbound.example" }),
    );
    batch.spans.push(courier);

    let mut gen2 = span(
        t1,
        "9a1b2c3d4e5f6077",
        Some("9a1b2c3d4e5f6071"),
        "chat gpt-4o",
        SpanKind::ModelGeneration,
        base + Duration::milliseconds(1800),
        1340,
        project,
        "sess-demo-1",
    );
    gen2.model = Some("gpt-4o".to_owned());
    gen2.provider = Some("openai".to_owned());
    gen2.entity_kind = Some(EntityKind::Model);
    gen2.entity_name = Some("gpt-4o".to_owned());
    gen2.input = Some(
        json!({ "messages": [ { "role": "tool", "name": "lookup_order", "content": "{\"status\":\"in_transit\",\"depot\":\"Leeds\"}" } ]}),
    );
    gen2.output = Some(
        json!({ "role": "assistant", "content": "Order #4417 was scanned into the Leeds depot on the 3rd and has not moved since. I've opened a trace with the courier (ref TR-88213)." }),
    );
    gen2.usage = Some(TokenUsage {
        input_tokens: Some(1602),
        output_tokens: Some(188),
        total_tokens: None,
        input_details: Some(InputTokenDetails {
            text: Some(402),
            cache_read: Some(1200),
            ..Default::default()
        }),
        output_details: None,
    });
    gen2.attributes =
        Some(json!({ "temperature": 0.2, "finish_reason": "stop", "streaming": true }));
    batch.spans.push(gen2);

    let mut guard = span(
        t1,
        "9a1b2c3d4e5f6078",
        Some("9a1b2c3d4e5f6071"),
        "no_unauthorised_refunds",
        SpanKind::Guardrail,
        base + Duration::milliseconds(3140),
        12,
        project,
        "sess-demo-1",
    );
    guard.output = Some(json!({ "passed": true, "rule": "no_unauthorised_refunds" }));
    batch.spans.push(guard);

    // ---- Run two: the same agent, failing. This is the run people open. ----
    let t2 = "b3d9e71a4c0f428eb6a5d2317f90c684";
    let base2 = now - Duration::minutes(2);

    let mut root2 = span(
        t2,
        "1122334455667788",
        None,
        "support_agent",
        SpanKind::AgentRun,
        base2,
        8420,
        project,
        "sess-demo-2",
    );
    root2.entity_kind = Some(EntityKind::Agent);
    root2.entity_name = Some("support_agent".to_owned());
    root2.status = SpanStatus::Error;
    root2.input = Some(
        json!({ "question": "I want to return the jacket I bought last month, it doesn't fit." }),
    );
    root2.error = Some(SpanError {
        message: "the model called returns_policy three times and never produced an answer"
            .to_owned(),
        kind: Some("MaxStepsExceeded".to_owned()),
        stack: None,
        domain: Some("agent".to_owned()),
        retryable: Some(true),
        details: Some(json!({ "max_steps": 3, "steps_used": 3 })),
    });
    batch.spans.push(root2);

    for (i, id) in ["1122334455667789", "112233445566778a", "112233445566778b"].iter().enumerate() {
        let offset = 200 + (i as i64) * 2600;
        let mut g = span(
            t2,
            id,
            Some("1122334455667788"),
            "chat gpt-4o-mini",
            SpanKind::ModelGeneration,
            base2 + Duration::milliseconds(offset),
            2400,
            project,
            "sess-demo-2",
        );
        g.model = Some("gpt-4o-mini".to_owned());
        g.provider = Some("openai".to_owned());
        g.entity_kind = Some(EntityKind::Model);
        g.entity_name = Some("gpt-4o-mini".to_owned());
        g.input = Some(
            json!({ "messages": [ { "role": "user", "content": "I want to return the jacket I bought last month, it doesn't fit." } ], "step": i + 1 }),
        );
        g.output = Some(
            json!({ "role": "assistant", "tool_calls": [ { "name": "returns_policy", "arguments": {} } ] }),
        );
        g.usage = Some(TokenUsage {
            input_tokens: Some(890 + (i as u64) * 210),
            output_tokens: Some(28),
            total_tokens: None,
            input_details: None,
            output_details: None,
        });
        g.attributes = Some(json!({ "finish_reason": "tool_calls", "step_index": i }));
        batch.spans.push(g);

        let mut t = span(
            t2,
            &format!("2233445566778{i}a"),
            Some("1122334455667788"),
            "returns_policy",
            SpanKind::ToolCall,
            base2 + Duration::milliseconds(offset + 2410),
            90,
            project,
            "sess-demo-2",
        );
        t.entity_kind = Some(EntityKind::Tool);
        t.entity_name = Some("returns_policy".to_owned());
        t.input = Some(json!({}));
        t.output =
            Some(json!({ "policy": "Returns accepted within 30 days with proof of purchase." }));
        t.attributes = Some(json!({ "tool_type": "function", "success": true }));
        batch.spans.push(t);
    }

    // Evaluations, so the quality columns are not empty on first open.
    batch.scores.push(Score {
        score_id: "demo-score-1".to_owned(),
        trace_id: Some(t1.to_owned()),
        span_id: None,
        scorer_id: "answer_relevance".to_owned(),
        scorer_version: Some("1".to_owned()),
        source: ScoreSource::Live,
        value: 0.94,
        min: Some(0.0),
        max: Some(1.0),
        label: Some("relevant".to_owned()),
        reason: Some(
            "Names the order, the depot and the date, and states a concrete next step.".to_owned(),
        ),
        score_trace_id: None,
        cost: None,
        metadata: Some(json!({ "synthetic": true })),
        timestamp: base + Duration::seconds(4),
        correlation: corr(project, "sess-demo-1"),
    });
    batch.scores.push(Score {
        score_id: "demo-score-2".to_owned(),
        trace_id: Some(t2.to_owned()),
        span_id: None,
        scorer_id: "answer_relevance".to_owned(),
        scorer_version: Some("1".to_owned()),
        source: ScoreSource::Live,
        value: 0.08,
        min: Some(0.0),
        max: Some(1.0),
        label: Some("no answer".to_owned()),
        reason: Some("The run ended without answering; the model looped on one tool.".to_owned()),
        score_trace_id: None,
        cost: None,
        metadata: Some(json!({ "synthetic": true })),
        timestamp: base2 + Duration::seconds(9),
        correlation: corr(project, "sess-demo-2"),
    });

    batch.feedback.push(Feedback {
        feedback_id: "demo-feedback-1".to_owned(),
        trace_id: Some(t2.to_owned()),
        span_id: None,
        kind: FeedbackKind::Thumbs,
        value: json!("down"),
        comment: Some("It never told me how to return anything.".to_owned()),
        author_id: Some("demo-user".to_owned()),
        author_kind: Some("end_user".to_owned()),
        timestamp: base2 + Duration::seconds(30),
        metadata: Some(json!({ "synthetic": true })),
        correlation: corr(project, "sess-demo-2"),
    });

    // Logs and metrics, so those screens have content too.
    for (i, (level, msg)) in [
        (LogLevel::Info, "resolved 4 skills for support_agent"),
        (LogLevel::Warn, "returns_policy returned the same result three times"),
        (LogLevel::Error, "run aborted after 3 steps without a final answer"),
    ]
    .iter()
    .enumerate()
    {
        batch.logs.push(LogRecord {
            log_id: format!("demo-log-{i}"),
            trace_id: Some(t2.to_owned()),
            span_id: Some("1122334455667788".to_owned()),
            timestamp: base2 + Duration::seconds(i as i64 * 3),
            level: *level,
            message: (*msg).to_owned(),
            fields: Some(json!({ "agent": "support_agent", "synthetic": true })),
            logger: Some("agent".to_owned()),
            correlation: corr(project, "sess-demo-2"),
            received_at: now,
        });
    }

    for i in 0..40i64 {
        batch.metrics.push(MetricPoint {
            metric_id: format!("demo-metric-{i}"),
            name: "mcpd.model.duration".to_owned(),
            kind: MetricKind::Histogram,
            value: 900.0 + ((i * 37) % 700) as f64,
            unit: Some("ms".to_owned()),
            timestamp: now - Duration::minutes(40 - i),
            labels: Some(json!({ "model": if i % 3 == 0 { "gpt-4o" } else { "gpt-4o-mini" } })),
            buckets: None,
            trace_id: Some(if i % 3 == 0 { t1.to_owned() } else { t2.to_owned() }),
            span_id: None,
            correlation: corr(project, "sess-demo-1"),
            received_at: now,
        });
    }

    // Run the demo data through the same intake every other write uses, so what
    // it produces is exactly what a real run produces: redacted, priced, and
    // with identifiers hashed. Seeding past the pipeline is how the demo ended
    // up disagreeing with production about what a stored span looks like.
    let intake = Intake::new(redactor, price_book, sampling);
    intake.prepare(&mut batch);

    let n = batch.len();
    store.ingest(batch).await?;
    Ok(n)
}

/// One theme's sentence, with its weight at the start of the noted window and
/// at the end of it. The pass groups notes by shared wording, and a writer
/// prompted the same way tends to repeat itself, so each theme here is one
/// sentence written about many runs.
type Mix = &'static [(&'static str, f64, f64)];

/// What the demo customers wanted. Refunds rise through the window, address
/// changes fall away, and cancellations only start part of the way through.
const GOALS: Mix = &[
    ("the customer wants a refund because their parcel arrived late", 0.15, 0.45),
    ("the customer wants to know where their parcel is and why tracking has not moved", 0.35, 0.25),
    ("the customer wants to change the delivery address on an open order", 0.4, 0.05),
    ("the customer wants to cancel their subscription before the next renewal", -0.25, 0.35),
];

/// The tone of the same messages. Frustration rises with the late parcels.
const TONES: Mix = &[
    ("the customer is frustrated and says they have waited too long for an answer", 0.2, 0.5),
    ("the customer is polite and patient and thanks the agent for the help", 0.5, 0.3),
    ("the customer is confused about a charge on their card they do not recognise", 0.3, 0.2),
];

/// What the agent did about it.
const BEHAVIOURS: Mix = &[
    ("the agent looked up the order and gave the customer a tracking update", 0.45, 0.3),
    ("the agent offered a refund to the original payment method and confirmed it", 0.2, 0.4),
    (
        "the agent handed the conversation to a human because it could not verify the account",
        0.35,
        0.3,
    ),
];

/// One-off goals that match nothing else, so a stage has some noise in it, as
/// real traffic does.
const ODD_ONES: &[&str] = &[
    "the customer asked whether the warehouse is open on bank holidays",
    "the customer wanted a gift card balance quoted in euros",
    "a question about recycling the packaging",
    "the customer wanted the invoice reissued to a company name",
    "the customer asked for a poem about the delivery van",
    "the customer wanted to know if the jacket comes in a longer fit",
    "the customer asked how to add a second card to the account",
    "the customer wanted a parcel held at the depot over the weekend",
];

/// A small deterministic generator, so every `mcpd demo` seeds the same notes.
struct Dice(u64);

impl Dice {
    /// A number in `[0, 1)`.
    #[allow(clippy::cast_precision_loss)]
    fn roll(&mut self) -> f64 {
        // xorshift64*: enough to spread a few hundred picks, with no dependency.
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 11) as f64 / (1u64 << 53) as f64
    }

    /// One theme's sentence, weighted by how far through the window `f` is.
    fn pick(&mut self, mix: Mix, f: f64) -> &'static str {
        let weights: Vec<f64> = mix.iter().map(|(_, a, b)| (a + (b - a) * f).max(0.0)).collect();
        let mut left = self.roll() * weights.iter().sum::<f64>();
        let mut chosen = mix.len() - 1;
        for (i, w) in weights.iter().enumerate() {
            if left < *w {
                chosen = i;
                break;
            }
            left -= w;
        }
        mix[chosen].0
    }
}

/// How many runs of the support agent carry notes.
const NOTED_RUNS: usize = 180;
/// How far back the noted runs reach, in hours.
const NOTED_HOURS: i64 = 16;
/// When each clustering pass ran, in hours before now. Four passes give each
/// theme a short history, and the mix of goals moves between them.
const PASSES: [i64; 4] = [9, 6, 3, 0];

/// Runs of the support agent with aspect notes about each, then four
/// clustering passes over them, so the themes screen has snapshots, a history
/// and a flow to show. Written through the ingest path and the same pass the
/// daemon's timer runs, so the screen shows what a deployment with notes would
/// see. Returns how many signals were stored, or zero when the storage backend
/// holds no theme tables.
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
pub async fn seed_aspects(state: &Arc<AppState>, project: &str) -> Result<usize> {
    if !state.store.capabilities().theme_analysis {
        return Ok(0);
    }
    let now = OffsetDateTime::now_utc();
    let first = now - Duration::hours(NOTED_HOURS);
    let step = Duration::hours(NOTED_HOURS) / NOTED_RUNS as f64;
    let mut dice = Dice(0x6d63_7064_6465_6d6f);
    let mut odd = 0;
    let mut batch = Batch::default();

    for i in 0..NOTED_RUNS {
        let f = i as f64 / NOTED_RUNS as f64;
        let start = first + step * i as f64;
        let trace = format!("{:032x}", 0x5eed_0000_u128 + i as u128);
        let session = format!("sess-noted-{i}");
        // Each odd one is used once, so it stays noise rather than recurring.
        let goal = match ODD_ONES.get(odd) {
            Some(text) if dice.roll() < 0.05 => {
                odd += 1;
                *text
            }
            _ => dice.pick(GOALS, f),
        };
        let tone = dice.pick(TONES, f);
        let behaviour = dice.pick(BEHAVIOURS, f);
        let failed = dice.roll() < 0.12;

        let mut root = span(
            &trace,
            &format!("{:016x}", 0x5eed_0000_u64 + i as u64),
            None,
            "support_agent",
            SpanKind::AgentRun,
            start,
            1400 + (dice.roll() * 3000.0) as i64,
            project,
            &session,
        );
        root.received_at = start;
        root.entity_kind = Some(EntityKind::Agent);
        root.entity_name = Some("support_agent".to_owned());
        root.input = Some(json!({ "summary": goal }));
        if failed {
            root.status = SpanStatus::Error;
            root.error = Some(SpanError {
                message: "the courier API timed out after 10 seconds".to_owned(),
                kind: Some("TimeoutError".to_owned()),
                stack: None,
                domain: Some("tool".to_owned()),
                retryable: Some(true),
                details: None,
            });
        }
        batch.spans.push(root);

        let written = start + Duration::minutes(2);
        for (aspect, text) in [("goal", goal), ("sentiment", tone), ("behaviour", behaviour)] {
            batch.aspect_notes.push(AspectNote {
                note_id: format!("demo-{aspect}-{i}"),
                trace_id: trace.clone(),
                aspect: aspect.to_owned(),
                text: text.to_owned(),
                definition_version: 1,
                model: Some("gpt-4o-mini".to_owned()),
                created_at: written,
                correlation: corr(project, &session),
            });
        }
    }

    // The runs first and their notes after, in two writes: a note is filed
    // under the entity of its run, so the run has to be stored before it.
    let intake = Intake::new(&state.redactor, &state.price_book, &state.config.sampling);
    let mut notes =
        Batch { aspect_notes: std::mem::take(&mut batch.aspect_notes), ..Batch::default() };
    intake.prepare(&mut batch);
    intake.prepare(&mut notes);
    let n = batch.len() + notes.len();
    state.store.ingest(batch).await?;
    state.store.ingest(notes).await?;

    for hours in PASSES {
        mcpd_server::aspects::run_once(state, now - Duration::hours(hours)).await?;
    }
    Ok(n)
}

/// One workflow definition and one completed run of it, so the runs screen
/// and the trace it leaves behind have something to show. Written through the
/// same store methods a worker drives, in the order a worker would drive them,
/// so what the interface shows is a run that happened rather than rows typed
/// in by hand.
pub async fn seed_runs(runs: &dyn mcpd_runs::RunStore, project: &str) -> Result<usize> {
    use mcpd_runs::{DefinitionDocument, DefinitionKind, StartRun, StepCompletion};

    let body = json!({
        "limits": { "max_steps": 20, "run_timeout_ms": 900_000 },
        "steps": [
            { "kind": "task", "id": "lookup_order",
              "input": { "order_id": "${input.order_id}" } },
            { "kind": "agent", "id": "draft_reply",
              "input": { "question": "${input.question}",
                         "order": "${steps.lookup_order.output}" } },
            { "kind": "wait_event", "id": "approval", "event": "approved",
              "key": "${input.order_id}", "timeout_ms": 259_200_000,
              "when": { "path": "steps.lookup_order.output.refund_usd", "op": "gt", "value": 100 } },
            { "kind": "task", "id": "send_reply",
              "input": { "text": "${steps.draft_reply.output.text}" } }
        ],
        "output": { "reply": "${steps.draft_reply.output.text}" }
    });
    runs.put_definition(DefinitionDocument {
        project: project.to_owned(),
        id: "support_ticket".to_owned(),
        kind: DefinitionKind::Workflow,
        version: Some(1),
        body,
    })
    .await?;

    let now = OffsetDateTime::now_utc();
    let mut clock = now - Duration::minutes(2);
    let run = runs
        .start_run(
            StartRun {
                project: project.to_owned(),
                definition_id: "support_ticket".to_owned(),
                definition_version: None,
                input: Some(json!({
                    "order_id": "A-1042",
                    "question": "My order has not arrived and the tracking has not moved."
                })),
                correlation: Some(json!({
                    "environment": "production", "service_name": "support-agent",
                    "session_id": "sess-demo-3", "user_id": "demo-user"
                })),
                idempotency_key: Some("demo-ticket-1042".to_owned()),
            },
            clock,
        )
        .await?;

    let outputs = [
        json!({ "status": "in transit", "eta": "2026-09-02", "refund_usd": 0 }),
        json!({ "text": "Your order A-1042 is in transit and should arrive on 2 September." }),
        json!({ "sent": true }),
    ];
    let usage = [
        None,
        Some(
            json!({ "input_tokens": 412, "output_tokens": 58, "model": "gpt-4o", "provider": "openai" }),
        ),
        None,
    ];
    let mut seeded = 1;
    for (output, usage) in outputs.into_iter().zip(usage) {
        clock += Duration::seconds(3);
        let leases = runs.claim(project, "demo-worker", 1, Some(&run.run_id), clock).await?;
        let Some(lease) = leases.into_iter().next() else { break };
        runs.started(&lease.lease_id, clock).await?;
        clock += Duration::seconds(2);
        let cost_usd = usage.as_ref().map(|_| 0.0016);
        runs.complete_step(
            &lease.lease_id,
            StepCompletion { output: Some(output), state: None, usage, cost_usd },
            clock,
        )
        .await?;
        seeded += 1;
    }
    Ok(seeded)
}
