//! Model calls and in-process redaction (contract sections 3.1 and 2.1.1).

use mcpd_sdk::model::{InputTokenDetails, OutputTokenDetails};
use mcpd_sdk::{
    usage_from_anthropic, usage_from_openai, Config, LogLevel, Mcpd, RecordingSink, SpanKind,
    TokenUsage,
};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

/// A configuration whose export happens only when a test asks for it.
fn config() -> Config {
    Config {
        endpoint: "http://127.0.0.1:9".to_owned(),
        project: "test".to_owned(),
        flush_interval: Duration::from_secs(3600),
        ..Config::default()
    }
}

/// A client whose batches land in memory.
fn recorded(config: Config) -> (Arc<RecordingSink>, Mcpd) {
    let sink = Arc::new(RecordingSink::new());
    let mcpd = Mcpd::with_config(config.with_sink(sink.clone()));
    (sink, mcpd)
}

fn openai_chat() -> Value {
    json!({
        "id": "chatcmpl-1",
        "object": "chat.completion",
        "model": "gpt-4o-2024-08-06",
        "service_tier": "flex",
        "choices": [{ "index": 0, "finish_reason": "stop",
                      "message": { "role": "assistant", "content": "hi" } }],
        "usage": {
            "prompt_tokens": 1000,
            "completion_tokens": 200,
            "total_tokens": 1200,
            "prompt_tokens_details": { "cached_tokens": 600, "audio_tokens": 0 },
            "completion_tokens_details": { "reasoning_tokens": 50 }
        }
    })
}

fn anthropic_message() -> Value {
    json!({
        "id": "msg_1",
        "type": "message",
        "model": "claude-sonnet-4-5",
        "stop_reason": "end_turn",
        "usage": {
            "input_tokens": 100,
            "cache_read_input_tokens": 900,
            "cache_creation_input_tokens": 50,
            "cache_creation": { "ephemeral_5m_input_tokens": 20, "ephemeral_1h_input_tokens": 30 },
            "output_tokens": 40,
            "service_tier": "batch"
        }
    })
}

#[test]
fn openai_usage_splits_the_gross_prompt_into_text_and_cache() {
    assert_eq!(
        usage_from_openai(&openai_chat()),
        Some(TokenUsage {
            input_tokens: Some(1000),
            output_tokens: Some(200),
            total_tokens: Some(1200),
            input_details: Some(InputTokenDetails {
                text: Some(400),
                cache_read: Some(600),
                audio: Some(0),
                ..InputTokenDetails::default()
            }),
            output_details: Some(OutputTokenDetails {
                reasoning: Some(50),
                ..OutputTokenDetails::default()
            }),
        })
    );
    let responses = json!({
        "object": "response",
        "usage": { "input_tokens": 10, "output_tokens": 5,
                   "input_tokens_details": { "cached_tokens": 4 } }
    });
    assert_eq!(
        usage_from_openai(&responses),
        Some(TokenUsage {
            input_tokens: Some(10),
            output_tokens: Some(5),
            total_tokens: Some(15),
            input_details: Some(InputTokenDetails {
                text: Some(6),
                cache_read: Some(4),
                ..InputTokenDetails::default()
            }),
            output_details: None,
        })
    );
}

#[test]
fn anthropic_usage_reads_input_as_net_of_the_cache() {
    assert_eq!(
        usage_from_anthropic(&anthropic_message()),
        Some(TokenUsage {
            input_tokens: Some(1050),
            output_tokens: Some(40),
            total_tokens: Some(1090),
            input_details: Some(InputTokenDetails {
                text: Some(100),
                cache_read: Some(900),
                cache_write: Some(50),
                cache_write_5m: Some(20),
                cache_write_1h: Some(30),
                ..InputTokenDetails::default()
            }),
            output_details: None,
        })
    );
}

#[tokio::test]
async fn an_unknown_shape_reads_nothing_and_never_panics() {
    assert_eq!(usage_from_openai(&Value::Null), None);
    assert_eq!(usage_from_anthropic(&json!({ "usage": { "input_tokens": "many" } })), None);
    let (sink, mcpd) = recorded(config());
    let span = mcpd.trace("run");
    span.record_response(&json!(42), None).record_response(&json!({ "usage": "x" }), None);
    span.end();
    mcpd.flush().await.expect("flush");
    let rec = sink.spans().into_iter().last().expect("a span");
    assert_eq!(rec.usage, None);
    assert_eq!(rec.attributes, None);
}

#[tokio::test]
async fn a_response_records_model_usage_id_reasons_and_tier_but_never_the_text() {
    let (sink, mcpd) = recorded(config());
    let a = mcpd.trace_of("a", SpanKind::ModelGeneration);
    a.record_response(&openai_chat(), None).end();
    let b = mcpd.trace_of("b", SpanKind::ModelGeneration);
    b.record_response(&anthropic_message(), Some("aws.bedrock")).end();
    mcpd.flush().await.expect("flush");
    let last = |name: &str| sink.spans().into_iter().rfind(|s| s.name == name).unwrap();
    let (ra, rb) = (last("a"), last("b"));
    assert_eq!(ra.model.as_deref(), Some("gpt-4o-2024-08-06"));
    assert_eq!(ra.provider.as_deref(), Some("openai"));
    assert_eq!(ra.output, None);
    assert_eq!(
        ra.attributes,
        Some(json!({
            "gen_ai.response.id": "chatcmpl-1",
            "gen_ai.response.finish_reasons": ["stop"],
            "gen_ai.response.service_tier": "flex"
        }))
    );
    assert_eq!(ra.usage.and_then(|u| u.input_tokens), Some(1000));
    assert_eq!(rb.provider.as_deref(), Some("aws.bedrock"));
    assert_eq!(rb.attributes.unwrap()["gen_ai.response.service_tier"], json!("batch"));
}

#[tokio::test]
async fn the_first_token_is_recorded_once_beside_a_guardrail_outcome() {
    let (sink, mcpd) = recorded(config());
    let s = mcpd.trace("stream");
    s.mark_first_token();
    tokio::time::sleep(Duration::from_millis(5)).await;
    s.mark_first_token();
    s.set_guardrail(true, Some("pii in output")).end();
    mcpd.flush().await.expect("flush");
    let rec = sink.spans().into_iter().rfind(|x| x.name == "stream").unwrap();
    let attrs = rec.attributes.expect("attributes");
    let ttft = attrs["mcpd.time_to_first_token_ms"].as_f64().expect("a number");
    assert!(ttft < 5.0, "the second call must not overwrite the first: {ttft}");
    assert_eq!(attrs["mcpd.guardrail.tripped"], json!(true));
    assert_eq!(attrs["mcpd.guardrail.reason"], json!("pii in output"));
}

#[tokio::test]
async fn redact_rewrites_payloads_before_queueing_and_drops_what_it_panics_on() {
    let (sink, mcpd) = recorded(config().with_redact(|v| {
        assert!(v.as_str() != Some("boom"), "redactor broke");
        let text = v.to_string().replace("1234-5678", "****");
        serde_json::from_str(&text).unwrap_or(Value::Null)
    }));
    let span = mcpd.trace("run");
    span.set_input(json!({ "card": "1234-5678" }));
    span.set_output("boom").end();
    mcpd.log_with(LogLevel::Info, "paid", Some(json!({ "card": "1234-5678" })));
    mcpd.flush().await.expect("flush");
    let rec = sink.spans().into_iter().rfind(|x| x.name == "run").unwrap();
    assert_eq!(rec.input, Some(json!({ "card": "****" })));
    assert_eq!(rec.output, None, "a panicking redactor fails closed");
    assert_eq!(sink.logs()[0].fields, Some(json!({ "card": "****" })));
    // One panic per emission of the span that carries "boom": only the end
    // carried it, the start had no output yet.
    assert_eq!(mcpd.stats().redaction_failures, 1);
    assert!(span.capturing());
}

#[tokio::test]
async fn redact_counts_a_non_object_for_an_object_field_and_null_drops_quietly() {
    let (sink, mcpd) = recorded(config().with_redact(|v| {
        if v.get("drop").is_some() {
            Value::Null
        } else if v.get("k").is_some() {
            json!("flattened")
        } else {
            v
        }
    }));
    let span = mcpd.trace("run");
    span.set_input("kept as any value");
    span.set_attributes(json!({ "k": 1 }));
    span.set_metadata(json!({ "drop": true })).end();
    mcpd.flush().await.expect("flush");
    let rec = sink.spans().into_iter().rfind(|x| x.name == "run").unwrap();
    assert_eq!(rec.input, Some(json!("kept as any value")));
    assert_eq!(rec.attributes, None, "a string is no use as attributes");
    assert_eq!(rec.metadata, None);
    assert_eq!(mcpd.stats().redaction_failures, 1, "null is a choice, not a failure");
}
