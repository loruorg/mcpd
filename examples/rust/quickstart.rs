//! Instrumenting a support agent with mcpd.
//!
//! The agent is fake and runs in a few milliseconds, but everything around it
//! is real: a run, a model generation with token usage, a tool call, a tool
//! that fails, a score, a piece of human feedback, and a read of the finished
//! trace back out of mcpd.
//!
//! Start a server, then run this:
//!
//! ```text
//! mcpd demo --listen 127.0.0.1:4317
//! cargo run -p mcpd-sdk --example quickstart
//! ```

use futures::{Stream, StreamExt};
use mcpd_sdk::testing::TestClient;
use mcpd_sdk::{Config, FeedbackKind, Include, Mcpd, Scale, SpanKind, Target};
use serde_json::{json, Value};
use std::convert::Infallible;

#[tokio::main]
async fn main() {
    // #region init
    // Nothing here is required. `Mcpd::new()` reads MCPD_ENDPOINT, MCPD_API_KEY
    // and the rest from the environment and talks to a local mcpd by default.
    // Anything set in code wins over the environment.
    let mcpd = Mcpd::with_config(
        Config::from_env()
            .with_project("support")
            .with_environment("local")
            .with_service("support-agent", Some("1.0.0")),
    );
    // #endregion init

    let question = "Where is order 5512?";

    // #region trace
    // The root span is the run. The sampling decision is made here, once, and
    // every child inherits it.
    let run = mcpd.trace("support agent");
    run.set_input(question);
    run.set_session("session-4f21");
    run.set_user("customer-91");
    run.set_tags(["support", "orders"]);

    // A tool call. `run.child_of` nests it under the run.
    let lookup = run.child_of("lookup_order", SpanKind::ToolCall);
    lookup.set_input(json!({ "order_id": "5512" }));
    let order = json!({ "status": "shipped", "carrier": "DHL", "eta": "Tuesday" });
    lookup.set_output(&order);
    lookup.end();

    // A tool that fails. Recording the error sets the span's status to `error`,
    // and a message is required: "something failed" is not actionable.
    let notify = run.child_of("notify_customer", SpanKind::ToolCall);
    notify.set_input(json!({ "channel": "sms" }));
    notify.set_error("sms gateway timed out after 5s");
    notify.end();
    // The failure did not stop the run: the agent falls back to email, and the
    // trace shows both the attempt and the recovery.
    run.event_with("fell back to email", json!({ "after": "sms gateway timeout" }));

    // #endregion trace
    // The model call. Token usage is what mcpd prices at ingest, so this is the
    // one field worth never leaving out.
    let answer = {
        let generation = run.child_of("answer the customer", SpanKind::ModelGeneration);
        generation.set_input(json!([
            { "role": "system", "content": "You are a support agent." },
            { "role": "user", "content": question },
        ]));
        // Where a real provider call goes. This is the shape Anthropic answers in.
        generation.mark_first_token();
        let answer = format!(
            "Your order shipped with {} and arrives {}.",
            order["carrier"].as_str().unwrap_or("the carrier"),
            order["eta"].as_str().unwrap_or("soon"),
        );
        let response = json!({
            "id": "msg_01",
            "type": "message",
            "model": "claude-sonnet-4-5",
            "stop_reason": "end_turn",
            "content": [{ "type": "text", "text": answer }],
            "usage": { "input_tokens": 172, "cache_read_input_tokens": 640, "output_tokens": 37 },
        });
        generation.set_output(&answer);
        // The model, the token counts mcpd prices, the response id and the
        // finish reason, read from the response in one call.
        generation.record_response(&response, None);
        answer
        // `generation` is dropped here, which ends the span. Calling `end()`
        // yourself does the same thing; the guard is there so an early return
        // or a panic cannot leave a span open.
    };

    run.set_output(&answer);
    let trace_id = run.trace_id().to_owned();
    run.end();

    // #region score
    // A score is a machine judgement. The scale is not optional: a bare 0.9 with
    // no declared range cannot be compared to anything.
    mcpd.score(
        Target::trace(&trace_id),
        "answer_relevance",
        0.9,
        Scale::UNIT,
        Some("Names the carrier and the delivery day the customer asked about."),
    );

    // Feedback is a person's judgement, kept separate from a score because the
    // two answer different questions and must never be averaged together.
    mcpd.feedback(Target::trace(&trace_id), FeedbackKind::Thumbs, "up", Some("fast and correct"));
    // #endregion score

    // Short-lived processes must flush. Nothing is sent from memory by a program
    // that exits without this.
    // A streamed call, and the unit test that checks what it records.
    println!("streamed: {}", stream_answer(&mcpd, "Is order 5512 on its way?").await);
    check_stream_answer().await;

    if let Err(e) = mcpd.flush().await {
        eprintln!("mcpd could not export: {e}");
    }

    // #region query
    // The same client reads. This is the trace you just wrote.
    match mcpd.get_trace(&trace_id, Include::all()).await {
        Ok(detail) => {
            println!("{}: {} spans", detail.trace.name, detail.spans.len());
            for span in &detail.spans {
                println!(
                    "  {:<22} {:>8} {}",
                    span.name,
                    span.duration_ms().map_or("open".to_owned(), |ms| format!("{ms:.1}ms")),
                    span.status,
                );
            }
            if let Some(cost) = detail.trace.cost.as_ref().and_then(|c| c.total_cost) {
                println!("  cost: {cost:.6} USD");
            }
            for score in detail.scores.iter().flatten() {
                println!("  score {}: {}", score.scorer_id, score.value);
            }
        }
        Err(e) => eprintln!("could not read the trace back: {e}"),
    }
    // #endregion query

    if let Err(e) = mcpd.shutdown().await {
        eprintln!("mcpd could not shut down cleanly: {e}");
    }
}

/// A provider's streamed answer, shaped as OpenAI's chat-completion chunks.
fn fake_chat_stream(text: &str) -> impl Stream<Item = Result<Value, Infallible>> {
    let base = json!({ "id": "chatcmpl-01", "model": "gpt-4o-mini" });
    let chunk = |choices: Value| {
        let mut c = base.clone();
        c["choices"] = choices;
        c
    };
    let mut chunks: Vec<Value> = text
        .split_inclusive(' ')
        .map(|word| chunk(json!([{ "index": 0, "delta": { "content": word } }])))
        .collect();
    chunks.push(chunk(json!([{ "index": 0, "delta": {}, "finish_reason": "stop" }])));
    let mut last = chunk(json!([]));
    last["usage"] = json!({ "prompt_tokens": 24, "completion_tokens": 6, "total_tokens": 30 });
    chunks.push(last);
    futures::stream::iter(chunks.into_iter().map(Ok))
}

// #region stream
/// A streamed model call. Read the wrapper instead of the provider's stream:
/// every chunk comes through unchanged, and when the stream ends the span gets
/// the time to first token, the text, the usage and the finish reason.
async fn stream_answer(client: &Mcpd, question: &str) -> String {
    let generation = client.span("stream_answer", SpanKind::ModelGeneration);
    generation.set_input(json!([{ "role": "user", "content": question }]));
    // Where your provider's streaming call goes.
    let stream = fake_chat_stream("Your order is in transit.");
    // The wrapper owns the span, and ends it when the stream ends.
    let mut chunks = generation.record_stream(stream, None);
    let mut answer = String::new();
    while let Some(Ok(chunk)) = chunks.next().await {
        answer.push_str(chunk["choices"][0]["delta"]["content"].as_str().unwrap_or(""));
    }
    answer
}
// #endregion stream

// #region testing
/// A unit test. Pass a `TestClient` where your code takes an `Mcpd`, then read
/// back what it recorded: no daemon, no port, no flush.
async fn check_stream_answer() {
    let test = TestClient::new();
    let answer = stream_answer(test.client(), "Where is order 5512?").await;

    let call = test.span("stream_answer").await.expect("one stream_answer span");
    assert_eq!(call.output, Some(json!(answer)));
    assert_eq!(call.model.as_deref(), Some("gpt-4o-mini"));
    assert_eq!(call.usage.and_then(|u| u.output_tokens), Some(6));

    let trees = test.trees().await;
    assert_eq!(trees[0].span.name, "stream_answer");
}
// #endregion testing
