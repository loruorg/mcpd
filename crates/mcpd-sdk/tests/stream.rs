//! Streamed model calls (contract section 3.2), read back through the test
//! client.

use futures::stream::{self, StreamExt};
use mcpd_sdk::testing::TestClient;
use mcpd_sdk::{Config, SpanKind, SpanStatus, STREAM_TEXT_BUDGET};
use serde_json::{json, Value};

/// The error a fake provider stream fails with.
#[derive(Debug, PartialEq, Eq)]
struct Reset;

impl std::fmt::Display for Reset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("connection reset")
    }
}

fn openai_chunks() -> Vec<Value> {
    let base = |choices: Value| {
        json!({ "id": "chatcmpl-1", "object": "chat.completion.chunk", "model": "gpt-4o",
                "choices": choices })
    };
    let mut last = base(json!([]));
    last["service_tier"] = json!("flex");
    last["usage"] = json!({ "prompt_tokens": 10, "completion_tokens": 2, "total_tokens": 12,
                            "prompt_tokens_details": { "cached_tokens": 4 } });
    vec![
        base(json!([{ "index": 0, "delta": { "role": "assistant" } }])),
        base(json!([{ "index": 0, "delta": { "content": "Hel" } }])),
        base(json!([{ "index": 0, "delta": { "content": "lo" } }])),
        base(json!([{ "index": 0, "delta": {}, "finish_reason": "stop" }])),
        last,
    ]
}

fn anthropic_events() -> Vec<Value> {
    vec![
        json!({ "type": "message_start", "message": { "id": "msg_1", "type": "message",
                "model": "claude-sonnet-4-5",
                "usage": { "input_tokens": 20, "cache_read_input_tokens": 80,
                           "output_tokens": 1 } } }),
        json!({ "type": "content_block_start", "index": 0,
                "content_block": { "type": "text", "text": "" } }),
        json!({ "type": "content_block_delta", "index": 0,
                "delta": { "type": "text_delta", "text": "Checking" } }),
        json!({ "type": "content_block_stop", "index": 0 }),
        json!({ "type": "content_block_start", "index": 1,
                "content_block": { "type": "tool_use", "id": "toolu_1", "name": "lookup",
                                   "input": {} } }),
        json!({ "type": "content_block_delta", "index": 1,
                "delta": { "type": "input_json_delta", "partial_json": "{\"id\":" } }),
        json!({ "type": "content_block_delta", "index": 1,
                "delta": { "type": "input_json_delta", "partial_json": "\"A-1\"}" } }),
        json!({ "type": "message_delta", "delta": { "stop_reason": "tool_use" },
                "usage": { "output_tokens": 15 } }),
        json!({ "type": "message_stop" }),
    ]
}

/// The chunks as a provider's stream that never fails.
fn ok(items: Vec<Value>) -> impl futures::Stream<Item = Result<Value, Reset>> {
    stream::iter(items.into_iter().map(Ok))
}

/// The chunks as a stream that fails in place of item `at`.
fn failing(items: Vec<Value>, at: usize) -> impl futures::Stream<Item = Result<Value, Reset>> {
    stream::iter(items.into_iter().take(at).map(Ok).chain(std::iter::once(Err(Reset))))
}

fn attribute(span: &mcpd_sdk::model::Span, key: &str) -> Option<Value> {
    span.attributes.as_ref().and_then(|a| a.get(key)).cloned()
}

#[tokio::test]
async fn an_openai_stream_records_what_record_response_would_and_passes_every_chunk() {
    let test = TestClient::new();
    let span = test.client().trace_of("call", SpanKind::ModelGeneration);
    let mut sent = openai_chunks();
    sent.push(json!("not a chunk"));
    let seen: Vec<Value> =
        span.record_stream(ok(sent.clone()), None).map(Result::unwrap).collect().await;
    assert_eq!(seen, sent, "every chunk passes through unchanged, the unknown one too");

    let s = test.span("call").await.unwrap();
    assert_eq!(s.model.as_deref(), Some("gpt-4o"));
    assert_eq!(s.provider.as_deref(), Some("openai"));
    assert_eq!(s.output, Some(json!("Hello")));
    let usage = s.usage.clone().expect("usage");
    assert_eq!(usage.input_tokens, Some(10));
    assert_eq!(usage.output_tokens, Some(2));
    assert_eq!(usage.total_tokens, Some(12));
    let details = usage.input_details.expect("input details");
    assert_eq!(details.text, Some(6));
    assert_eq!(details.cache_read, Some(4));
    assert_eq!(attribute(&s, "gen_ai.response.id"), Some(json!("chatcmpl-1")));
    assert_eq!(attribute(&s, "gen_ai.response.finish_reasons"), Some(json!(["stop"])));
    assert_eq!(attribute(&s, "gen_ai.response.service_tier"), Some(json!("flex")));
    assert!(attribute(&s, "mcpd.time_to_first_token_ms").is_some_and(|v| v.is_number()));
    assert_eq!(attribute(&s, "mcpd.stream.truncated"), None);
    assert_eq!(s.status, SpanStatus::Ok);
}

#[tokio::test]
async fn anthropic_tool_call_deltas_are_joined_and_message_delta_usage_merged() {
    let test = TestClient::new();
    let span = test.client().trace_of("call", SpanKind::ModelGeneration);
    let _: Vec<_> = span.record_stream(ok(anthropic_events()), None).collect().await;

    let s = test.span("call").await.unwrap();
    assert_eq!(s.model.as_deref(), Some("claude-sonnet-4-5"));
    assert_eq!(s.provider.as_deref(), Some("anthropic"));
    assert_eq!(
        s.output,
        Some(json!({
            "content": "Checking",
            "tool_calls": [{ "id": "toolu_1", "name": "lookup", "arguments": "{\"id\":\"A-1\"}" }],
        }))
    );
    let usage = s.usage.clone().expect("usage");
    assert_eq!(usage.input_tokens, Some(100));
    assert_eq!(usage.output_tokens, Some(15));
    assert_eq!(attribute(&s, "gen_ai.response.id"), Some(json!("msg_1")));
    assert_eq!(attribute(&s, "gen_ai.response.finish_reasons"), Some(json!(["tool_use"])));
}

#[tokio::test]
async fn the_usage_is_read_from_a_chunk_whose_choices_are_null() {
    let test = TestClient::new();
    let span = test.client().trace("call");
    let usage = json!({ "prompt_tokens": 3, "completion_tokens": 1, "total_tokens": 4 });
    let chunks = vec![json!({ "model": "gpt-4o", "choices": null, "usage": usage })];
    let _: Vec<_> = span.record_stream(ok(chunks), None).collect().await;
    let s = test.span("call").await.unwrap();
    assert_eq!(s.usage.and_then(|u| u.output_tokens), Some(1));
    assert_eq!(s.provider.as_deref(), Some("openai"));
}

#[tokio::test]
async fn openai_tool_call_deltas_are_joined_by_index() {
    let test = TestClient::new();
    let span = test.client().trace("call");
    let call = |d: Value| json!({ "choices": [{ "index": 0, "delta": { "tool_calls": [d] } }] });
    let chunks = vec![
        call(json!({ "index": 0, "id": "call_1",
                     "function": { "name": "lookup", "arguments": "{\"a\"" } })),
        call(json!({ "index": 0, "function": { "arguments": ":1}" } })),
        json!({ "choices": [{ "index": 0, "delta": {}, "finish_reason": "tool_calls" }] }),
    ];
    let _: Vec<_> = span.record_stream(ok(chunks), None).collect().await;
    let s = test.span("call").await.unwrap();
    assert_eq!(
        s.output,
        Some(json!({
            "content": "",
            "tool_calls": [{ "id": "call_1", "name": "lookup", "arguments": "{\"a\":1}" }],
        }))
    );
}

#[tokio::test]
async fn the_first_token_is_marked_once_at_the_first_content_chunk() {
    let pause = |ms| tokio::time::sleep(std::time::Duration::from_millis(ms));
    let test = TestClient::new();
    let span = test.client().trace("call");
    let mut stream = span.record_stream(ok(openai_chunks()), None);
    pause(20).await;
    stream.next().await; // the role only: no content yet
    pause(40).await;
    stream.next().await; // "Hel", the first token, about 60ms in
    pause(80).await;
    while stream.next().await.is_some() {}
    let s = test.span("call").await.unwrap();
    let ms = attribute(&s, "mcpd.time_to_first_token_ms").and_then(|v| v.as_f64()).unwrap();
    assert!(ms >= 60.0, "marked before the first content chunk: {ms}ms");
    assert!(ms < 140.0, "marked again after the first content chunk: {ms}ms");
}

#[tokio::test]
async fn a_stream_that_fails_ends_the_span_with_the_error_and_hands_it_on() {
    let test = TestClient::new();
    let span = test.client().trace("call");
    let seen: Vec<Result<Value, Reset>> =
        span.record_stream(failing(openai_chunks(), 3), None).collect().await;
    assert_eq!(seen.len(), 4);
    assert_eq!(seen[3], Err(Reset), "the provider's error reaches the caller unchanged");

    let s = test.span("call").await.unwrap();
    assert_eq!(s.status, SpanStatus::Error);
    assert_eq!(s.error.as_ref().map(|e| e.message.as_str()), Some("connection reset"));
    assert_eq!(s.output, Some(json!("Hello")));
}

#[tokio::test]
async fn a_caller_that_stops_early_still_ends_the_span() {
    let test = TestClient::new();
    let span = test.client().trace("call");
    let mut stream = span.record_stream(ok(openai_chunks()), None);
    while let Some(Ok(chunk)) = stream.next().await {
        if chunk["choices"][0]["delta"]["content"] == "Hel" {
            break;
        }
    }
    drop(stream);
    let s = test.span("call").await.unwrap();
    assert_eq!(s.output, Some(json!("Hel")));
    assert_eq!(s.status, SpanStatus::Ok);
    let ends = test.recorder().spans().iter().filter(|r| r.end_time.is_some()).count();
    assert_eq!(ends, 1, "the span is finished exactly once");
}

#[tokio::test]
async fn text_past_the_budget_is_cut_and_marked() {
    let test = TestClient::new();
    let span = test.client().trace("call");
    let big = format!("{}ab", "😀".repeat(STREAM_TEXT_BUDGET - 1));
    let chunk = json!({ "choices": [{ "index": 0, "delta": { "content": big } }] });
    let _: Vec<_> = span.record_stream(ok(vec![chunk]), None).collect().await;
    let s = test.span("call").await.unwrap();
    let text = s.output.as_ref().and_then(Value::as_str).expect("text output");
    assert_eq!(text.chars().count(), STREAM_TEXT_BUDGET);
    assert!(text.ends_with("😀a"));
    assert_eq!(attribute(&s, "mcpd.stream.truncated"), Some(json!(true)));
}

#[tokio::test]
async fn payload_capture_off_keeps_the_output_in_the_process() {
    let test = TestClient::with_config(Config::default().with_capture_payloads(false));
    let span = test.client().trace("call");
    let _: Vec<_> = span.record_stream(ok(openai_chunks()), None).collect().await;
    let s = test.span("call").await.unwrap();
    assert_eq!(s.output, None);
    assert_eq!(s.model.as_deref(), Some("gpt-4o"));
}

#[tokio::test]
async fn the_provider_argument_wins_over_the_shape() {
    let test = TestClient::new();
    let span = test.client().trace("call");
    let _: Vec<_> = span.record_stream(ok(openai_chunks()), Some("azure")).collect().await;
    assert_eq!(test.span("call").await.unwrap().provider.as_deref(), Some("azure"));
}

#[tokio::test]
async fn a_typed_chunk_is_read_through_serde() {
    #[derive(serde::Serialize)]
    struct Delta {
        content: String,
    }
    #[derive(serde::Serialize)]
    struct Choice {
        index: u32,
        delta: Delta,
    }
    #[derive(serde::Serialize)]
    struct Chunk {
        model: String,
        choices: Vec<Choice>,
    }
    let test = TestClient::new();
    let span = test.client().trace("call");
    let chunk = Chunk {
        model: "gpt-4o".into(),
        choices: vec![Choice { index: 0, delta: Delta { content: "typed".into() } }],
    };
    let items = stream::iter(vec![Ok::<_, Reset>(chunk)]);
    let _: Vec<_> = span.record_stream(items, None).collect().await;
    let s = test.span("call").await.unwrap();
    assert_eq!(s.output, Some(json!("typed")));
    assert_eq!(s.model.as_deref(), Some("gpt-4o"));
}

#[tokio::test]
async fn a_disabled_client_passes_the_chunks_and_records_nothing() {
    let test = TestClient::with_config(Config::default().with_enabled(false));
    let span = test.client().trace("call");
    let seen: Vec<_> = span.record_stream(ok(openai_chunks()), None).collect().await;
    assert_eq!(seen.len(), openai_chunks().len());
    assert!(test.spans(None, None).await.is_empty());
}
