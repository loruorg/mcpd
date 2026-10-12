//! Wire-level tests for OTLP ingest, driven from outside the crate.
//!
//! These exercise the two encodings the way a collector actually speaks them:
//! whole `ExportTraceServiceRequest` payloads with resources, scopes, parentage
//! and semantic-convention attributes, rather than the hand-built `OtlpSpan`
//! values the in-module unit tests use. Two properties are worth this file on
//! their own: `gen_ai.*` and OpenInference `llm.*` must land on identical
//! fields, because that equivalence is a headline claim of the project, and the
//! protobuf reader must survive hostile bytes, because it is hand-written and
//! reachable by anyone who can POST to `/v1/traces`.
//!
//! NOT COVERED HERE, and deliberately: the OTLP logs and metrics decoders.
//! `otlp_logs_decode` and `otlp_metrics_decode` are private functions in
//! `crates/mcpd-server/src/ingest.rs`, so nothing in `mcpd-core`'s test target
//! can reach them, and `mcpd-core` cannot depend on `mcpd-server`. They need a
//! test beside their own crate.

use mcpd_core::otlp::{decode_json, protobuf, to_span, to_spans, OtlpSpan};
use mcpd_core::{cost, ids, InputTokenDetails, Span, SpanKind, SpanStatus, TokenUsage};
use serde_json::{json, Value};
use time::OffsetDateTime;

const TRACE: &str = "4bf92f3577b34da6a3ce929d0e0e4736";
const SPAN: &str = "00f067aa0ba902b7";
const CHILD: &str = "7d3efb1a2c4b5e60";
const OTHER_TRACE: &str = "9c1b0e5a7f2d43886ae1c0d9b4f37521";

/// The span ids above as raw bytes, for the protobuf side.
const TRACE_BYTES: [u8; 16] = [
    0x4b, 0xf9, 0x2f, 0x35, 0x77, 0xb3, 0x4d, 0xa6, 0xa3, 0xce, 0x92, 0x9d, 0x0e, 0x0e, 0x47, 0x36,
];
const SPAN_BYTES: [u8; 8] = [0x00, 0xf0, 0x67, 0xaa, 0x0b, 0xa9, 0x02, 0xb7];
const CHILD_BYTES: [u8; 8] = [0x7d, 0x3e, 0xfb, 0x1a, 0x2c, 0x4b, 0x5e, 0x60];
const OTHER_TRACE_BYTES: [u8; 16] = [
    0x9c, 0x1b, 0x0e, 0x5a, 0x7f, 0x2d, 0x43, 0x88, 0x6a, 0xe1, 0xc0, 0xd9, 0xb4, 0xf3, 0x75, 0x21,
];

fn epoch() -> OffsetDateTime {
    OffsetDateTime::UNIX_EPOCH
}

fn attr(key: &str, value: Value) -> Value {
    json!({ "key": key, "value": value })
}

fn s(v: &str) -> Value {
    json!({ "stringValue": v })
}

/// OTLP/JSON sends int64 as a string so that JSON's 53-bit limit cannot round it.
fn i(v: i64) -> Value {
    json!({ "intValue": v.to_string() })
}

/// Decode a batch and map every span, which is the whole ingest path in one call.
fn spans_of(body: &Value) -> Vec<Span> {
    decode_json(body)
        .expect("body should decode")
        .iter()
        .map(|o| to_span(o, "fallback", epoch()))
        .collect()
}

// ---------------------------------------------------------------- json ---

/// A batch shaped like one an OpenTelemetry collector flushes: two resources,
/// a scope each, and a parent/child pair under the first.
fn collector_batch() -> Value {
    json!({
      "resourceSpans": [
        {
          "resource": { "attributes": [
            attr("service.name", s("checkout-agent")),
            attr("service.version", s("2.1.0")),
            attr("deployment.environment.name", s("production")),
            attr("telemetry.sdk.language", s("python"))
          ]},
          "scopeSpans": [{
            "scope": { "name": "opentelemetry.instrumentation.openai", "version": "0.40b0" },
            "spans": [
              {
                "traceId": TRACE, "spanId": SPAN, "name": "invoke_agent checkout",
                "kind": 1,
                "startTimeUnixNano": "1735689600000000000",
                "endTimeUnixNano": "1735689602250000000",
                "status": { "code": 1 },
                "attributes": [
                  attr("gen_ai.operation.name", s("invoke_agent")),
                  attr("gen_ai.agent.name", s("checkout")),
                  attr("gen_ai.conversation.id", s("conv-8812")),
                  attr("user.id", s("u-4471"))
                ]
              },
              {
                "traceId": TRACE, "spanId": CHILD, "parentSpanId": SPAN,
                "name": "chat gpt-4o",
                "startTimeUnixNano": "1735689600125000000",
                "endTimeUnixNano": "1735689601875000000",
                "status": { "code": 1 },
                "attributes": [
                  attr("gen_ai.system", s("openai")),
                  attr("gen_ai.request.model", s("gpt-4o")),
                  attr("gen_ai.operation.name", s("chat")),
                  attr("gen_ai.usage.input_tokens", i(1284)),
                  attr("gen_ai.usage.output_tokens", i(377))
                ]
              }
            ]
          }]
        },
        {
          "resource": { "attributes": [attr("service.name", s("search-indexer"))] },
          "scopeSpans": [{
            "scope": { "name": "opentelemetry.instrumentation.requests" },
            "spans": [{
              "traceId": OTHER_TRACE, "spanId": SPAN, "name": "GET /embeddings",
              "startTimeUnixNano": "1735689700000000000",
              "endTimeUnixNano": "1735689700090000000",
              "attributes": [attr("http.request.method", s("GET"))]
            }]
          }]
        }
      ]
    })
}

#[test]
fn a_collector_batch_keeps_every_span_with_its_own_resource_identity() {
    let spans = spans_of(&collector_batch());
    assert_eq!(spans.len(), 3, "every span in every scope of every resource must survive");

    let agent = &spans[0];
    assert_eq!(
        agent.kind,
        SpanKind::AgentRun,
        "gen_ai.operation.name=invoke_agent is an agent run"
    );
    assert!(agent.is_root, "a span with no parentSpanId is a root");
    assert_eq!(agent.parent_span_id, None);
    assert_eq!(agent.duration_ns, Some(2_250_000_000));
    assert_eq!(agent.entity_name.as_deref(), Some("checkout"));
    assert_eq!(agent.correlation.session_id.as_deref(), Some("conv-8812"));
    assert_eq!(agent.correlation.user_id.as_deref(), Some("u-4471"));
    assert_eq!(agent.correlation.environment.as_deref(), Some("production"));
    // release is not sent explicitly, so it falls back to the resource version.
    assert_eq!(agent.correlation.release.as_deref(), Some("2.1.0"));

    let generation = &spans[1];
    assert_eq!(generation.parent_span_id.as_deref(), Some(SPAN));
    assert!(!generation.is_root, "a span with a parent is never a root");
    assert_eq!(generation.kind, SpanKind::ModelGeneration);
    assert_eq!(generation.model.as_deref(), Some("gpt-4o"));
    assert_eq!(generation.usage.as_ref().unwrap().input_tokens, Some(1284));
    assert_eq!(
        generation.correlation.service_name.as_deref(),
        Some("checkout-agent"),
        "resource identity applies to every span under that resource"
    );

    let http = &spans[2];
    assert_eq!(http.trace_id, OTHER_TRACE);
    assert_eq!(http.kind, SpanKind::HttpRequest);
    assert_eq!(
        http.correlation.service_name.as_deref(),
        Some("search-indexer"),
        "the second resource must not inherit the first resource's service"
    );
    assert_eq!(http.correlation.environment, None, "the second resource declares no environment");
    // No status code and a finished span means Ok, not Unset.
    assert_eq!(http.status, SpanStatus::Ok);
}

#[test]
fn gen_ai_and_openinference_describe_the_same_generation_identically() {
    // The claim under test: point either convention at mcpd and the model,
    // provider and span kind land on the same three fields with the same
    // values. If one mapping table is edited without the other, this fails.
    let body = json!({ "resourceSpans": [{ "scopeSpans": [{ "spans": [
      {
        "traceId": TRACE, "spanId": SPAN, "name": "gen_ai",
        "startTimeUnixNano": "1735689600000000000",
        "endTimeUnixNano": "1735689601000000000",
        "attributes": [
          attr("gen_ai.operation.name", s("chat")),
          attr("gen_ai.system", s("anthropic")),
          attr("gen_ai.request.model", s("claude-sonnet-4-5")),
          attr("gen_ai.usage.input_tokens", i(9102)),
          attr("gen_ai.usage.output_tokens", i(614)),
          attr("gen_ai.usage.total_tokens", i(9716))
        ]
      },
      {
        "traceId": TRACE, "spanId": CHILD, "name": "openinference",
        "startTimeUnixNano": "1735689600000000000",
        "endTimeUnixNano": "1735689601000000000",
        "attributes": [
          attr("openinference.span.kind", s("LLM")),
          attr("llm.provider", s("anthropic")),
          attr("llm.model_name", s("claude-sonnet-4-5")),
          attr("llm.token_count.prompt", i(9102)),
          attr("llm.token_count.completion", i(614)),
          attr("llm.token_count.total", i(9716))
        ]
      }
    ]}]}]});

    let spans = spans_of(&body);
    let (genai, openinference) = (&spans[0], &spans[1]);

    // Asserted against literals as well as against each other, so that two
    // spans that both decoded to nothing could not pass this as "identical".
    assert_eq!(genai.kind, SpanKind::ModelGeneration);
    assert_eq!(genai.model.as_deref(), Some("claude-sonnet-4-5"));
    assert_eq!(genai.provider.as_deref(), Some("anthropic"));
    assert_eq!(genai.usage.as_ref().unwrap().input_tokens, Some(9102));
    assert_eq!(genai.usage.as_ref().unwrap().output_tokens, Some(614));
    assert_eq!(genai.usage.as_ref().unwrap().total_tokens, Some(9716));

    assert_eq!(openinference.kind, genai.kind, "span kind must not depend on the convention");
    assert_eq!(openinference.model, genai.model, "model must not depend on the convention");
    assert_eq!(
        openinference.provider, genai.provider,
        "provider must not depend on the convention"
    );
    assert_eq!(openinference.usage, genai.usage, "token usage must not depend on the convention");
}

#[test]
fn token_detail_lands_the_same_way_under_both_conventions() {
    // Cache and reasoning counts are what cost is computed from, so the two
    // conventions disagreeing here would be a billing bug, not a cosmetic one.
    let body = json!({ "resourceSpans": [{ "scopeSpans": [{ "spans": [
      {
        "traceId": TRACE, "spanId": SPAN, "name": "gen_ai",
        "startTimeUnixNano": "1", "endTimeUnixNano": "2",
        "attributes": [
          attr("gen_ai.system", s("anthropic")),
          attr("gen_ai.usage.input_tokens", i(9102)),
          attr("gen_ai.usage.output_tokens", i(614)),
          attr("gen_ai.usage.cache_read_input_tokens", i(8704)),
          attr("gen_ai.usage.cache_creation_input_tokens", i(256)),
          attr("gen_ai.usage.reasoning_tokens", i(431))
        ]
      },
      {
        "traceId": TRACE, "spanId": CHILD, "name": "openinference",
        "startTimeUnixNano": "1", "endTimeUnixNano": "2",
        "attributes": [
          attr("llm.provider", s("anthropic")),
          attr("llm.token_count.prompt", i(9102)),
          attr("llm.token_count.completion", i(614)),
          attr("llm.token_count.prompt_details.cache_read", i(8704)),
          attr("llm.token_count.prompt_details.cache_write", i(256)),
          attr("llm.token_count.completion_details.reasoning", i(431))
        ]
      }
    ]}]}]});

    let spans = spans_of(&body);
    let genai = spans[0].usage.clone().expect("gen_ai usage");
    let openinference = spans[1].usage.clone().expect("openinference usage");

    let input = genai.input_details.clone().expect("cache counts imply input detail");
    assert_eq!(input.cache_read, Some(8704));
    assert_eq!(input.cache_write, Some(256));
    let output = genai.output_details.clone().expect("reasoning implies output detail");
    assert_eq!(output.reasoning, Some(431));

    // Every count a producer reported lands identically. `text` is the one
    // field that does not, and it is not a count: it is what mcpd reads off the
    // attribute names about how the input total was counted. The gen_ai names
    // above are Anthropic's own field names, so the input count is the uncached
    // share; the OpenInference names are written by instrumentations for
    // providers that report a gross total, so nothing is inferred from them.
    let oi_input = openinference.input_details.clone().expect("cache counts imply input detail");
    assert_eq!(input.text, Some(9102), "Anthropic's field names say the total is net of cache");
    assert_eq!(oi_input.text, None, "the OpenInference names do not settle the convention");
    assert_eq!(
        InputTokenDetails { text: None, ..oi_input },
        InputTokenDetails { text: None, ..input },
        "the cache counts themselves must not depend on the convention",
    );
    assert_eq!(
        TokenUsage { input_details: None, ..openinference },
        TokenUsage { input_details: None, ..genai },
        "every reported count must be convention independent",
    );
}

/// The under-bill this file's mapper caused, priced end to end.
///
/// BUG, in `extract_usage` in `crates/mcpd-core/src/otlp/mod.rs`: both detail
/// structs were built with `text: None` on every path, so a span never said
/// what share of its input was uncached. `cost::compute` then read
/// `input_tokens` as a gross total and subtracted the cache from it, which is
/// exactly wrong for Anthropic, the one vendor whose attribute names the mapper
/// reads and whose `input_tokens` is already net of cache.
///
/// Input: one OTLP span with `gen_ai.request.model=claude-sonnet-4`,
/// `gen_ai.usage.input_tokens=100`, `gen_ai.usage.cache_read_input_tokens=1000`.
/// Expected: 100 uncached at 3.00 plus 1000 cached at 0.30 per million, 0.0006.
/// Observed: 0.0003, the cache alone, with the uncached share billed as nothing.
#[test]
fn an_anthropic_span_bills_its_uncached_input_instead_of_dropping_it() {
    let span = only_span(json!([
        attr("gen_ai.system", s("anthropic")),
        attr("gen_ai.request.model", s("claude-sonnet-4")),
        attr("gen_ai.usage.input_tokens", i(100)),
        attr("gen_ai.usage.output_tokens", i(0)),
        attr("gen_ai.usage.cache_read_input_tokens", i(1000))
    ]));

    let usage = span.usage.clone().expect("the span reports usage");
    let details = usage.input_details.clone().expect("a cache count implies input detail");
    assert_eq!(details.cache_read, Some(1000));
    assert_eq!(details.text, Some(100), "the uncached share has to reach the price book");

    let c = cost::compute(&cost::builtin_book(), span.model.as_deref().expect("a model"), &usage)
        .expect("a known model with usage is priced");
    assert!((c.input_cost.unwrap() - 0.000_6).abs() < 1e-12, "got {:?}", c.input_cost);
}

/// The same span shape with the cache smaller than the input count.
///
/// Worth its own test because the arithmetic cannot rescue this one. Where the
/// cache count is larger than the input count, `compute` can prove the total is
/// not gross and price it as the uncached share. Here both readings are
/// arithmetically consistent, so the only thing that separates them is what the
/// mapper recorded, and the gap between them is the whole cached share.
#[test]
fn the_convention_the_mapper_records_is_what_prices_an_ordinary_cached_call() {
    let span = only_span(json!([
        attr("gen_ai.system", s("anthropic")),
        attr("gen_ai.request.model", s("claude-sonnet-4")),
        attr("gen_ai.usage.input_tokens", i(5000)),
        attr("gen_ai.usage.output_tokens", i(0)),
        attr("gen_ai.usage.cache_read_input_tokens", i(3000))
    ]));
    let usage = span.usage.clone().expect("the span reports usage");
    let book = cost::builtin_book();

    let priced = cost::compute(&book, "claude-sonnet-4", &usage).expect("should price");
    // 5000 uncached at 3.00 plus 3000 cached at 0.30, per million.
    assert!((priced.input_cost.unwrap() - 0.015_9).abs() < 1e-12, "got {:?}", priced.input_cost);

    // The counterfactual, so the assertion above is not vacuous: the identical
    // counts with the convention thrown away price at 0.0069, because 3000 of
    // the 5000 uncached tokens are subtracted away and never billed.
    let stripped = TokenUsage {
        input_details: Some(InputTokenDetails { text: None, ..usage.input_details.unwrap() }),
        ..usage
    };
    let dropped = cost::compute(&book, "claude-sonnet-4", &stripped).expect("should price");
    assert!((dropped.input_cost.unwrap() - 0.006_9).abs() < 1e-12, "got {:?}", dropped.input_cost);
}

#[test]
fn an_exporter_that_writes_snake_case_produces_the_identical_span() {
    // The Python and Go exporters emit proto3 JSON with original field names on
    // some paths. Accepting only camelCase would silently drop those batches.
    let camel = json!({ "resourceSpans": [{
      "resource": { "attributes": [attr("service.name", s("checkout-agent"))] },
      "scopeSpans": [{ "spans": [{
        "traceId": TRACE, "spanId": CHILD, "parentSpanId": SPAN, "name": "chat gpt-4o",
        "startTimeUnixNano": "1735689600125000000",
        "endTimeUnixNano": "1735689601875000000",
        "status": { "code": 1 },
        "attributes": [
          { "key": "gen_ai.system", "value": { "stringValue": "openai" } },
          { "key": "gen_ai.usage.input_tokens", "value": { "intValue": "1284" } }
        ]
      }]}]
    }]});
    let snake = json!({ "resource_spans": [{
      "resource": { "attributes": [attr("service.name", s("checkout-agent"))] },
      "scope_spans": [{ "spans": [{
        "trace_id": TRACE, "span_id": CHILD, "parent_span_id": SPAN, "name": "chat gpt-4o",
        "start_time_unix_nano": "1735689600125000000",
        "end_time_unix_nano": "1735689601875000000",
        "status": { "code": 1 },
        "attributes": [
          { "key": "gen_ai.system", "value": { "string_value": "openai" } },
          { "key": "gen_ai.usage.input_tokens", "value": { "int_value": "1284" } }
        ]
      }]}]
    }]});

    let from_camel = spans_of(&camel);
    let from_snake = spans_of(&snake);
    assert_eq!(from_snake.len(), 1, "a snake_case batch must not be dropped");
    assert_eq!(from_snake, from_camel, "field naming must not change a single decoded field");
    assert_eq!(from_snake[0].usage.as_ref().unwrap().input_tokens, Some(1284));
}

#[test]
fn a_failed_span_falls_back_to_the_exception_attributes_for_its_error() {
    // Several SDKs set the status code from a recorded exception but leave the
    // status message empty, so the message has to come off the exception event.
    let body = json!({ "resourceSpans": [{ "scopeSpans": [{ "spans": [{
      "traceId": TRACE, "spanId": SPAN, "name": "chat gpt-4o",
      "startTimeUnixNano": "1735689600000000000",
      "endTimeUnixNano": "1735689600400000000",
      "status": { "code": 2 },
      "attributes": [
        attr("exception.type", s("RateLimitError")),
        attr("exception.message", s("429 Too Many Requests")),
        attr("exception.stacktrace", s("openai/_base_client.py:1043"))
      ]
    }]}]}]});

    let span = spans_of(&body).remove(0);
    assert_eq!(span.status, SpanStatus::Error);
    let error = span.error.expect("status code 2 must produce an error");
    assert_eq!(error.message, "429 Too Many Requests", "the exception message is the fallback");
    assert_eq!(error.kind.as_deref(), Some("RateLimitError"));
    assert_eq!(error.stack.as_deref(), Some("openai/_base_client.py:1043"));
}

#[test]
fn the_project_comes_from_the_span_then_the_resource_then_the_default() {
    let one = |resource: Value, attributes: Value| {
        json!({ "resourceSpans": [{
          "resource": { "attributes": resource },
          "scopeSpans": [{ "spans": [{
            "traceId": TRACE, "spanId": SPAN, "name": "x",
            "startTimeUnixNano": "1", "endTimeUnixNano": "2",
            "attributes": attributes
          }]}]
        }]})
    };

    let from_span = one(
        json!([attr("mcpd.project", s("from-resource"))]),
        json!([attr("mcpd.project", s("from-span"))]),
    );
    let from_resource = one(json!([attr("mcpd.project", s("from-resource"))]), json!([]));
    let from_default = one(json!([]), json!([]));

    assert_eq!(spans_of(&from_span)[0].correlation.project, "from-span", "the span wins");
    assert_eq!(
        spans_of(&from_resource)[0].correlation.project,
        "from-resource",
        "then the resource"
    );
    assert_eq!(
        spans_of(&from_default)[0].correlation.project,
        "fallback",
        "then the caller's default"
    );
}

// ------------------------------------------------------------ protobuf ---

/// A minimal protobuf writer, so these tests encode real wire bytes rather than
/// asserting against a fixture nobody can check by eye.
struct Pb(Vec<u8>);

impl Pb {
    fn new() -> Self {
        Self(Vec::new())
    }
    fn varint(&mut self, mut v: u64) -> &mut Self {
        loop {
            let b = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                self.0.push(b);
                return self;
            }
            self.0.push(b | 0x80);
        }
    }
    fn tag(&mut self, field: u32, wire: u32) -> &mut Self {
        self.varint(u64::from(field << 3 | wire))
    }
    fn uint(&mut self, field: u32, v: u64) -> &mut Self {
        self.tag(field, 0).varint(v)
    }
    fn fixed64(&mut self, field: u32, v: u64) -> &mut Self {
        self.tag(field, 1);
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn fixed32(&mut self, field: u32, v: u32) -> &mut Self {
        self.tag(field, 5);
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn bytes(&mut self, field: u32, v: &[u8]) -> &mut Self {
        self.tag(field, 2).varint(v.len() as u64);
        self.0.extend_from_slice(v);
        self
    }
    fn string(&mut self, field: u32, v: &str) -> &mut Self {
        self.bytes(field, v.as_bytes())
    }
    fn take(&self) -> Vec<u8> {
        self.0.clone()
    }
}

/// `KeyValue { string key = 1; AnyValue value = 2 }` around an already encoded value.
fn kv(key: &str, value: &[u8]) -> Vec<u8> {
    let mut out = Pb::new();
    out.string(1, key).bytes(2, value);
    out.take()
}

fn any_string(v: &str) -> Vec<u8> {
    let mut out = Pb::new();
    out.string(1, v);
    out.take()
}

fn any_int(v: i64) -> Vec<u8> {
    let mut out = Pb::new();
    out.uint(3, v as u64);
    out.take()
}

/// Wrap a string `AnyValue` in `levels` nested `ArrayValue` layers.
///
/// Each layer is about seven bytes on the wire, which is why an unbounded
/// decoder can be made to recurse thousands of times by a small request body.
fn nested_any_value(levels: usize) -> Vec<u8> {
    let mut buf = any_string("deep");
    for _ in 0..levels {
        let mut array = Pb::new();
        // ArrayValue { repeated AnyValue values = 1 }
        array.bytes(1, &buf);
        let mut wrapper = Pb::new();
        // AnyValue { ArrayValue array_value = 5 }
        wrapper.bytes(5, &array.take());
        buf = wrapper.take();
    }
    buf
}

fn nested_value(levels: usize) -> Value {
    let mut v = Value::from("deep");
    for _ in 0..levels {
        v = Value::Array(vec![v]);
    }
    v
}

/// `ExportTraceServiceRequest` carrying one span, from its pieces.
fn request(resource_attrs: &[Vec<u8>], spans: &[Vec<u8>]) -> Vec<u8> {
    let mut resource = Pb::new();
    for a in resource_attrs {
        resource.bytes(1, a);
    }
    let mut scope = Pb::new();
    for s in spans {
        scope.bytes(2, s);
    }
    let mut resource_spans = Pb::new();
    resource_spans.bytes(1, &resource.take()).bytes(2, &scope.take());
    let mut out = Pb::new();
    out.bytes(1, &resource_spans.take());
    out.take()
}

/// The same generation the JSON test above describes, on the protobuf wire.
fn generation_span() -> Vec<u8> {
    let mut status = Pb::new();
    status.string(2, "429 from provider").uint(3, 2);
    let mut span = Pb::new();
    span.bytes(1, &TRACE_BYTES)
        .bytes(2, &SPAN_BYTES)
        .bytes(4, &CHILD_BYTES)
        .string(5, "chat gpt-4o")
        .fixed64(7, 1_735_689_600_125_000_000)
        .fixed64(8, 1_735_689_601_875_000_000)
        .bytes(9, &kv("gen_ai.system", &any_string("openai")))
        .bytes(9, &kv("gen_ai.request.model", &any_string("gpt-4o")))
        .bytes(9, &kv("gen_ai.usage.input_tokens", &any_int(1284)))
        .bytes(15, &status.take());
    span.take()
}

fn generation_request() -> Vec<u8> {
    request(
        &[
            kv("service.name", &any_string("checkout-agent")),
            kv("service.version", &any_string("2.1.0")),
        ],
        &[generation_span()],
    )
}

#[test]
fn json_and_protobuf_lower_to_the_same_span() {
    // The module's stated invariant is that the two encodings share one mapper
    // and so cannot drift. That is only true if they also agree on the values
    // they hand it, which is what this compares.
    let from_protobuf = protobuf::decode_traces(&generation_request()).expect("protobuf decodes");

    let equivalent_json = json!({ "resourceSpans": [{
      "resource": { "attributes": [
        attr("service.name", s("checkout-agent")),
        attr("service.version", s("2.1.0"))
      ]},
      "scopeSpans": [{ "spans": [{
        "traceId": TRACE, "spanId": SPAN, "parentSpanId": CHILD, "name": "chat gpt-4o",
        "startTimeUnixNano": "1735689600125000000",
        "endTimeUnixNano": "1735689601875000000",
        "status": { "code": 2, "message": "429 from provider" },
        "attributes": [
          attr("gen_ai.system", s("openai")),
          attr("gen_ai.request.model", s("gpt-4o")),
          attr("gen_ai.usage.input_tokens", i(1284))
        ]
      }]}]
    }]});
    let from_json = decode_json(&equivalent_json).expect("json decodes");

    assert_eq!(from_protobuf.len(), 1);
    assert_eq!(from_json.len(), 1);
    let a = to_span(&from_protobuf[0], "fallback", epoch());
    let b = to_span(&from_json[0], "fallback", epoch());
    assert_eq!(a.status, SpanStatus::Error, "the payload really does describe a failure");
    assert_eq!(a.usage.as_ref().unwrap().input_tokens, Some(1284));
    assert_eq!(a, b, "the same logical span must decode identically from either encoding");
}

#[test]
fn every_truncation_of_a_real_payload_is_an_error_and_never_a_panic() {
    // A collector that dies mid-flush, or a proxy that cuts a body short, must
    // produce a rejected request rather than take the process with it.
    let full = generation_request();
    assert!(protobuf::decode_traces(&full).is_ok(), "the intact payload is the control");
    for cut in 1..full.len() {
        assert!(
            protobuf::decode_traces(&full[..cut]).is_err(),
            "a body cut at {cut} of {} bytes claims a length it does not have",
            full.len()
        );
    }
    // An empty body is a well formed request with nothing in it, not an error.
    assert!(protobuf::decode_traces(&[]).expect("empty is valid").is_empty());
}

#[test]
fn unknown_fields_are_skipped_and_the_known_ones_still_arrive() {
    // Forward compatibility is only worth anything if the fields this build
    // does understand survive the encounter, which the shape of the assertion
    // below is chosen to prove: it compares against the clean decode.
    let mut future_status = Pb::new();
    future_status.string(2, "429 from provider").uint(3, 2).fixed32(88, 7);

    let mut future_any = Pb::new();
    // An AnyValue with an unrecognised field before the string it carries.
    future_any.uint(41, 12).string(1, "openai");

    let mut future_kv = Pb::new();
    future_kv.string(1, "gen_ai.system").bytes(2, &future_any.take()).fixed64(9, 5);

    let mut span = Pb::new();
    span.bytes(1, &TRACE_BYTES)
        .bytes(2, &SPAN_BYTES)
        .bytes(4, &CHILD_BYTES)
        .string(5, "chat gpt-4o")
        .uint(6, 3)
        .fixed64(7, 1_735_689_600_125_000_000)
        .fixed64(8, 1_735_689_601_875_000_000)
        .bytes(9, &future_kv.take())
        .bytes(9, &kv("gen_ai.request.model", &any_string("gpt-4o")))
        .bytes(9, &kv("gen_ai.usage.input_tokens", &any_int(1284)))
        // Field 97 is not in the Span message at all. This used to be field
        // 11, which the decoder ignored until it learned to read events; a
        // forward-compatibility test has to name a field that is still unknown
        // or it stops testing anything.
        .string(97, "a field this decoder has never heard of")
        .bytes(15, &future_status.take())
        .fixed32(64, 1);

    let mut resource = Pb::new();
    resource
        .bytes(1, &kv("service.name", &any_string("checkout-agent")))
        .bytes(1, &kv("service.version", &any_string("2.1.0")))
        .uint(2, 4);

    let mut scope = Pb::new();
    scope.string(1, "scope metadata").bytes(2, &span.take()).string(3, "schema url");

    let mut resource_spans = Pb::new();
    resource_spans.bytes(1, &resource.take()).bytes(2, &scope.take()).string(3, "schema url");

    let mut body = Pb::new();
    body.bytes(1, &resource_spans.take()).fixed64(500, 1);

    let decoded = protobuf::decode_traces(&body.take()).expect("unknown fields are not fatal");
    assert_eq!(decoded.len(), 1, "the span must survive the unknown fields around it");

    let with_unknowns = to_span(&decoded[0], "fallback", epoch());
    let clean =
        to_span(&protobuf::decode_traces(&generation_request()).unwrap()[0], "fallback", epoch());
    assert_eq!(with_unknowns, clean, "an unknown field must change nothing that is known");
}

#[test]
fn an_attribute_nested_past_the_depth_limit_is_rejected() {
    // Forty layers is nothing to build and everything to recurse through. The
    // limit has to turn it into a rejected request, because the alternative was
    // a stack overflow, and a stack overflow is not a catchable failure.
    let body = request(&[kv("nest", &nested_any_value(40))], &[generation_span()]);
    let result = protobuf::decode_traces(&body);
    assert!(result.is_err(), "a 40 deep attribute must be refused, not walked");
    let message = result.unwrap_err().to_string();
    assert!(message.contains("nested too deeply"), "unexpected rejection reason: {message}");
}

#[test]
fn an_attribute_nested_inside_the_depth_limit_is_kept_whole() {
    // The other half of the limit: it must not have been bought by rejecting
    // legitimately structured attributes.
    let body = request(&[kv("nest", &nested_any_value(10))], &[generation_span()]);
    let decoded = protobuf::decode_traces(&body).expect("ten levels is well inside the limit");
    assert_eq!(decoded.len(), 1);
    assert_eq!(
        decoded[0].resource["nest"],
        nested_value(10),
        "a nested attribute must arrive with every layer intact"
    );
}

#[test]
fn a_span_whose_attribute_nests_too_deeply_is_dropped_alone() {
    // Inside a batch the rule is the softer one: one poisoned span is skipped
    // and its neighbours are still stored, matching the JSON path.
    let mut deep = Pb::new();
    deep.bytes(1, &TRACE_BYTES)
        .bytes(2, &CHILD_BYTES)
        .string(5, "too deep")
        .bytes(9, &kv("nest", &nested_any_value(40)));

    let body = request(
        &[kv("service.name", &any_string("checkout-agent"))],
        &[deep.take(), generation_span()],
    );
    let decoded = protobuf::decode_traces(&body).expect("one bad span must not fail the batch");
    let names: Vec<&str> = decoded.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["chat gpt-4o"], "only the span with the deep attribute is dropped");
}

#[test]
fn no_single_byte_corruption_makes_the_decoder_panic_or_invent_an_id() {
    // The reader is hand-written and reachable by anyone who can POST to
    // /v1/traces, so the contract is that arbitrary bytes yield Ok or Err and
    // that anything it does return still carries ids mcpd can store.
    let full = generation_request();
    for index in 0..full.len() {
        for mask in [0xff_u8, 0x01, 0x80] {
            let mut corrupt = full.clone();
            corrupt[index] ^= mask;
            let Ok(spans) = protobuf::decode_traces(&corrupt) else { continue };
            for span in &spans {
                assert!(
                    ids::validate_trace_id(&span.trace_id).is_ok(),
                    "byte {index} ^ {mask:#04x} produced trace id {:?}",
                    span.trace_id
                );
                assert!(
                    ids::validate_span_id(&span.span_id).is_ok(),
                    "byte {index} ^ {mask:#04x} produced span id {:?}",
                    span.span_id
                );
            }
        }
    }
}

#[test]
fn a_hand_built_otlp_span_still_needs_no_attributes_to_map() {
    // to_span must cope with the minimum a decoder can hand it, because both
    // decoders will hand it exactly this for an uninstrumented span.
    let bare = OtlpSpan {
        trace_id: TRACE.to_owned(),
        span_id: SPAN.to_owned(),
        name: "bare".to_owned(),
        ..OtlpSpan::default()
    };
    let span = to_span(&bare, "fallback", epoch());
    assert_eq!(span.kind, SpanKind::Generic, "nothing to infer from means generic, never a guess");
    assert_eq!(span.status, SpanStatus::Unset, "no end time and no code is unset, not ok");
    assert!(span.usage.is_none(), "absent counts must stay absent rather than become zero");
    assert!(span.error.is_none());
    assert_eq!(span.correlation.project, "fallback");
}

// ------------------------------------------------ convention conflicts ---

/// One finished span carrying `attributes`, inside the frames a collector sends.
fn one_span(attributes: Value) -> Value {
    resourced_span(json!([]), attributes)
}

fn resourced_span(resource: Value, attributes: Value) -> Value {
    json!({ "resourceSpans": [{
      "resource": { "attributes": resource },
      "scopeSpans": [{ "spans": [{
        "traceId": TRACE, "spanId": SPAN, "name": "chat",
        "startTimeUnixNano": "1735689600000000000",
        "endTimeUnixNano": "1735689601000000000",
        "attributes": attributes
      }]}]
    }]})
}

fn only_span(attributes: Value) -> Span {
    spans_of(&one_span(attributes)).remove(0)
}

#[test]
fn a_span_that_speaks_both_conventions_at_once_answers_from_a_fixed_order() {
    // Instrumenting one process twice is ordinary: an OpenInference library
    // running under an OpenTelemetry SDK writes both sets of attributes onto
    // the same span. The values then disagree, and which one mcpd keeps has to
    // be a decision rather than whatever the attribute map iterates first.
    let span = only_span(json!([
        attr("openinference.span.kind", s("TOOL")),
        attr("gen_ai.operation.name", s("chat")),
        attr("gen_ai.request.model", s("gpt-4o")),
        attr("gen_ai.response.model", s("gpt-4o-2024-08-06")),
        attr("llm.model_name", s("claude-sonnet-4-5")),
        attr("gen_ai.system", s("openai")),
        attr("llm.provider", s("anthropic")),
        attr("gen_ai.usage.input_tokens", i(100)),
        attr("gen_ai.usage.prompt_tokens", i(9001)),
        attr("llm.token_count.prompt", i(9002)),
        attr("gen_ai.usage.output_tokens", i(20)),
        attr("llm.token_count.completion", i(9003)),
        attr("gen_ai.usage.cache_read_input_tokens", i(64)),
        attr("llm.token_count.prompt_details.cache_read", i(9004))
    ]));

    assert_eq!(span.kind, SpanKind::ToolCall, "OpenInference decides the kind, ahead of gen_ai");
    assert_eq!(
        span.model.as_deref(),
        Some("gpt-4o"),
        "the requested model beats the responded one"
    );
    assert_eq!(span.provider.as_deref(), Some("openai"), "gen_ai decides the provider");

    let usage = span.usage.expect("both conventions report counts");
    assert_eq!(usage.input_tokens, Some(100), "the current gen_ai name beats the deprecated one");
    assert_eq!(usage.output_tokens, Some(20), "gen_ai decides the output count");
    assert_eq!(usage.input_details.unwrap().cache_read, Some(64), "gen_ai decides the cache read");
}

/// The rule the test above establishes, applied to the two counts that break it.
///
/// BUG, in `extract_usage` in `crates/mcpd-core/src/otlp/mod.rs`: `input` and
/// `output` are read gen_ai first, but `total` and `reasoning` are read
/// OpenInference first. A span carrying both conventions therefore reports a
/// total that belongs to one pair of counts and an input and output that belong
/// to the other, and `Span::total_tokens` prefers that total. Ingest stores it,
/// so every cost and token aggregate over such a span is wrong.
///
/// Input: one span with `gen_ai.usage.input_tokens=10`, `output_tokens=5`,
/// `total_tokens=15`, plus `llm.token_count.prompt=1000`, `completion=500`,
/// `total=1500`.
/// Expected: `total_tokens` reads gen_ai first, like input and output do, so 15.
/// Observed: 1500, the other convention's total, beside an input of 10.
#[test]
fn the_total_is_read_from_the_same_convention_as_the_counts_beside_it() {
    let span = only_span(json!([
        attr("gen_ai.usage.input_tokens", i(10)),
        attr("gen_ai.usage.output_tokens", i(5)),
        attr("gen_ai.usage.total_tokens", i(15)),
        attr("gen_ai.usage.reasoning_tokens", i(3)),
        attr("llm.token_count.prompt", i(1000)),
        attr("llm.token_count.completion", i(500)),
        attr("llm.token_count.total", i(1500)),
        attr("llm.token_count.completion_details.reasoning", i(300))
    ]));
    let usage = span.usage.expect("counts are present");
    assert_eq!((usage.input_tokens, usage.output_tokens), (Some(10), Some(5)));
    assert_eq!(usage.total_tokens, Some(15), "the total must come from the winning convention");
    assert_eq!(usage.output_details.unwrap().reasoning, Some(3));
}
#[test]
fn mcpds_own_attributes_win_the_kind_and_lose_the_model_and_the_counts() {
    // Worth pinning because it is asymmetric and nothing else says so: the
    // mcpd.* namespace is the first thing consulted for the span kind and the
    // project, and the last thing consulted for the model, the provider and the
    // token counts. Someone adding an override attribute needs to know which
    // end of the list they are adding to.
    let span = only_span(json!([
        attr("mcpd.span.kind", s("workflow_run")),
        attr("openinference.span.kind", s("LLM")),
        attr("mcpd.model", s("mine")),
        attr("gen_ai.request.model", s("gpt-4o")),
        attr("mcpd.provider", s("mine")),
        attr("gen_ai.system", s("openai")),
        attr("mcpd.usage.input_tokens", i(7)),
        attr("gen_ai.usage.input_tokens", i(100))
    ]));
    assert_eq!(span.kind, SpanKind::WorkflowRun, "mcpd.span.kind is consulted before anything");
    assert_eq!(
        span.model.as_deref(),
        Some("gpt-4o"),
        "mcpd.model is the last resort, not the first"
    );
    assert_eq!(span.provider.as_deref(), Some("openai"));
    assert_eq!(span.usage.unwrap().input_tokens, Some(100));
}

#[test]
fn an_unusable_mcpd_span_kind_falls_through_to_the_next_convention() {
    // The kind is read with FromStr and the failure is ignored, which is what
    // lets a typo degrade to the convention underneath instead of to Generic.
    let span = only_span(json!([
        attr("mcpd.span.kind", s("workflow-run")),
        attr("openinference.span.kind", s("LLM"))
    ]));
    assert_eq!(span.kind, SpanKind::ModelGeneration, "an unparsable override must not win");
}

#[test]
fn the_openinference_kinds_the_unit_tests_leave_out_all_map() {
    // CHAIN, RERANKER and EVALUATOR are the three the in-module table omits,
    // and the case folding is what lets a producer send them lower case.
    for (value, expect) in [
        ("CHAIN", SpanKind::WorkflowRun),
        ("RERANKER", SpanKind::RagAction),
        ("EVALUATOR", SpanKind::ScorerRun),
        ("llm", SpanKind::ModelGeneration),
        ("Retriever", SpanKind::Retrieval),
    ] {
        let span = only_span(json!([attr("openinference.span.kind", s(value))]));
        assert_eq!(span.kind, expect, "OpenInference {value} mapped wrong");
    }
}

/// The forward-compatibility half of the kind mapping.
///
/// BUG, in `infer_kind` in `crates/mcpd-core/src/otlp/mod.rs`: a value this
/// build does not recognise returns `Generic` from inside the match arm instead
/// of falling through to the rules below it. A span from a newer OpenInference
/// release that also carries `gen_ai.operation.name=chat` and
/// `gen_ai.system=openai` is filed as Generic, so it leaves the model views and
/// stops billing tokens, even though two other attributes say plainly what it
/// is. `gen_ai.operation.name` has the same shape and the same consequence.
///
/// Input: `openinference.span.kind=PLANNER`, `gen_ai.operation.name=chat`,
/// `gen_ai.system=openai`.
/// Expected: an unrecognised value is no answer, so the next rule applies and
/// the span is a model generation.
/// Observed: `SpanKind::Generic`.
#[test]
fn an_unknown_kind_value_falls_through_to_the_conventions_underneath_it() {
    let openinference = only_span(json!([
        attr("openinference.span.kind", s("PLANNER")),
        attr("gen_ai.operation.name", s("chat")),
        attr("gen_ai.system", s("openai"))
    ]));
    assert_eq!(openinference.kind, SpanKind::ModelGeneration);

    let gen_ai = only_span(json!([
        attr("gen_ai.operation.name", s("chat_stream")),
        attr("gen_ai.system", s("openai")),
        attr("gen_ai.request.model", s("gpt-4o"))
    ]));
    assert_eq!(gen_ai.kind, SpanKind::ModelGeneration);
}
#[test]
fn every_gen_ai_operation_name_the_mapper_knows_reaches_its_kind() {
    for (op, expect) in [
        ("chat", SpanKind::ModelGeneration),
        ("text_completion", SpanKind::ModelGeneration),
        ("generate_content", SpanKind::ModelGeneration),
        ("embeddings", SpanKind::RagEmbedding),
        ("execute_tool", SpanKind::ToolCall),
        ("create_agent", SpanKind::AgentRun),
        ("invoke_agent", SpanKind::AgentRun),
    ] {
        let span = only_span(json!([attr("gen_ai.operation.name", s(op))]));
        assert_eq!(span.kind, expect, "gen_ai.operation.name={op} mapped wrong");
    }
}

#[test]
fn naming_a_provider_or_a_model_is_enough_to_be_a_generation() {
    // The rule under the two operation-name rules: a span that says nothing
    // about what it did, but names who it called, is still a model call. Some
    // exporters emit only this.
    for key in ["gen_ai.system", "llm.provider", "llm.model_name"] {
        let span = only_span(json!([attr(key, s("anthropic"))]));
        assert_eq!(span.kind, SpanKind::ModelGeneration, "{key} alone must imply a generation");
    }
}

#[test]
fn the_last_two_kind_rules_read_the_mcp_and_http_conventions() {
    let mcp = only_span(json!([attr("mcp.tool.name", s("search_docs"))]));
    assert_eq!(mcp.kind, SpanKind::McpToolCall);

    // http.method is the pre-1.0 name and is still what several instrumentation
    // libraries emit.
    let legacy = only_span(json!([attr("http.method", s("POST"))]));
    assert_eq!(legacy.kind, SpanKind::HttpRequest);

    let neither = only_span(json!([attr("db.system", s("postgresql"))]));
    assert_eq!(neither.kind, SpanKind::Generic, "an unmapped convention is never guessed at");
}

#[test]
fn a_token_count_that_is_not_a_whole_positive_number_is_left_absent() {
    // A negative or fractional count must not become a token total, because
    // cost is computed from these and a wrapped u64 would be unbounded money.
    for value in [
        json!({ "intValue": "-5" }),
        json!({ "doubleValue": 12.5 }),
        json!({ "boolValue": true }),
        json!({ "arrayValue": { "values": [{ "intValue": "5" }] } }),
    ] {
        let span =
            only_span(json!([json!({ "key": "gen_ai.usage.input_tokens", "value": value })]));
        assert!(span.usage.is_none(), "{value} must not be counted as tokens");
    }

    // The control, so the test above cannot pass by decoding nothing at all.
    let good = only_span(json!([attr("gen_ai.usage.input_tokens", i(5))]));
    assert_eq!(good.usage.unwrap().input_tokens, Some(5));
}

#[test]
fn a_count_sent_as_a_string_is_still_counted() {
    // OTLP/JSON int64 is a string by specification, and `intValue` is unwrapped
    // for that. An SDK that reports the count as a plain `stringValue` instead
    // is out of specification but common, and parsing it costs nothing.
    let span = only_span(json!([
        attr("gen_ai.usage.input_tokens", s("1284")),
        // Larger than i64, so it arrives as a string and has to survive as one.
        json!({ "key": "gen_ai.usage.output_tokens",
                "value": { "intValue": "18446744073709551615" } })
    ]));
    let usage = span.usage.expect("both counts parse");
    assert_eq!(usage.input_tokens, Some(1284));
    assert_eq!(usage.output_tokens, Some(u64::MAX));
}

#[test]
fn a_span_that_reports_only_a_total_still_carries_usage() {
    // Several providers return a total and nothing else on a cached response.
    let span = only_span(json!([attr("gen_ai.usage.total_tokens", i(4096))]));
    let usage = span.usage.expect("a total alone is still usage");
    assert_eq!(usage.total_tokens, Some(4096));
    assert_eq!(usage.input_tokens, None, "an absent count stays absent, never zero");
    assert!(usage.input_details.is_none(), "no cache counts means no detail record");
}

#[test]
fn cache_and_audio_detail_appear_only_when_something_reports_them() {
    let span = only_span(json!([
        attr("gen_ai.usage.input_tokens", i(100)),
        attr("gen_ai.usage.output_tokens", i(10)),
        attr("llm.token_count.prompt_details.audio", i(12)),
        attr("llm.token_count.completion_details.audio", i(3))
    ]));
    let usage = span.usage.expect("counts are present");
    let input = usage.input_details.expect("an audio count implies input detail");
    assert_eq!(input.audio, Some(12));
    assert_eq!(input.cache_read, None);
    assert_eq!(usage.output_details.unwrap().audio, Some(3));
}

#[test]
fn the_prompt_and_the_completion_are_read_from_whichever_convention_carries_them() {
    // Nothing else covers input and output at all, and they are what the trace
    // view shows. OpenInference's input.value is consulted first, gen_ai's
    // prompt second, mcpd's own last.
    let contested = only_span(json!([
        attr("input.value", s("from openinference")),
        attr("gen_ai.prompt", s("from gen_ai")),
        attr("mcpd.input", s("from mcpd")),
        attr("gen_ai.completion", s("the answer")),
        attr("mcpd.output", s("not this one"))
    ]));
    assert_eq!(contested.input, Some(json!("from openinference")));
    assert_eq!(contested.output, Some(json!("the answer")));

    // A structured value has to arrive as structure, not as a rendered string.
    let structured = only_span(json!([json!({
        "key": "input.value",
        "value": { "kvlistValue": { "values": [
            attr("role", s("user")),
            attr("content", s("hello"))
        ]}}
    })]));
    assert_eq!(structured.input, Some(json!({ "role": "user", "content": "hello" })));

    let neither = only_span(json!([attr("gen_ai.system", s("openai"))]));
    assert_eq!(neither.input, None, "an absent prompt is absent, not an empty string");
}

#[test]
fn the_correlation_fields_resolve_by_precedence_when_several_sources_disagree() {
    let span = spans_of(&resourced_span(
        json!([
            attr("service.name", s("checkout-agent")),
            attr("service.version", s("2.1.0")),
            attr("deployment.environment.name", s("production")),
            attr("deployment.environment", s("legacy-production"))
        ]),
        json!([
            attr("gen_ai.conversation.id", s("conv-1")),
            attr("session.id", s("sess-2")),
            attr("mcpd.session_id", s("sess-3")),
            attr("user.id", s("u-1")),
            attr("enduser.id", s("u-2")),
            attr("mcpd.user_id", s("u-3")),
            attr("mcpd.thread_id", s("th-1")),
            attr("thread.id", s("th-2")),
            attr("mcpd.request_id", s("req-1")),
            attr("http.request.id", s("req-2")),
            attr("mcpd.release", s("release-1"))
        ]),
    ))
    .remove(0);

    let c = span.correlation;
    assert_eq!(c.session_id.as_deref(), Some("conv-1"), "the gen_ai conversation id comes first");
    assert_eq!(c.user_id.as_deref(), Some("u-1"));
    assert_eq!(c.thread_id.as_deref(), Some("th-1"));
    assert_eq!(c.request_id.as_deref(), Some("req-1"));
    assert_eq!(c.release.as_deref(), Some("release-1"), "an explicit release beats the version");
    assert_eq!(c.environment.as_deref(), Some("production"), "the current name beats the old one");
    assert_eq!(c.service_version.as_deref(), Some("2.1.0"));
}

#[test]
fn the_deployment_environment_is_read_from_the_resource_and_never_from_the_span() {
    // Both spellings are resource attributes in the semantic conventions, so
    // the same key on a span is not a second place to look. mcpd.environment is
    // the span-level way to say it, and it is the only one.
    let on_the_span = only_span(json!([
        attr("deployment.environment.name", s("production")),
        attr("deployment.environment", s("production"))
    ]));
    assert_eq!(on_the_span.correlation.environment, None);

    let overridden = spans_of(&resourced_span(
        json!([attr("deployment.environment.name", s("production"))]),
        json!([attr("mcpd.environment", s("staging"))]),
    ))
    .remove(0);
    assert_eq!(overridden.correlation.environment.as_deref(), Some("staging"));
}

#[test]
fn tags_are_taken_only_from_an_array_and_only_the_strings_in_it() {
    let mixed = only_span(json!([json!({
        "key": "mcpd.tags",
        "value": { "arrayValue": { "values": [
            s("canary"), json!({ "intValue": "7" }), s("eu-west-1")
        ]}}
    })]));
    assert_eq!(mixed.tags, Some(vec!["canary".to_owned(), "eu-west-1".to_owned()]));

    let not_a_list = only_span(json!([attr("mcpd.tags", s("canary"))]));
    assert_eq!(not_a_list.tags, None, "a bare string is not a tag list");
}

#[test]
fn an_id_is_accepted_in_every_encoding_an_sdk_sends_and_in_no_other() {
    let with_ids = |trace: &str, span: &str| {
        json!({ "resourceSpans": [{ "scopeSpans": [{ "spans": [{
          "traceId": trace, "spanId": span, "name": "x", "startTimeUnixNano": "1"
        }]}]}]})
    };

    // Upper case hex is legal and has to normalise, or the same trace arrives
    // under two ids and the waterfall splits in half.
    let upper = spans_of(&with_ids(&TRACE.to_ascii_uppercase(), &SPAN.to_ascii_uppercase()));
    assert_eq!(upper.len(), 1);
    assert_eq!(upper[0].trace_id, TRACE);
    assert_eq!(upper[0].span_id, SPAN);

    // The URL-safe base64 alphabet, which is what several exporters emit.
    let url_safe = spans_of(&with_ids("S_kvNXezTaajzpKdDg5HNg==", "APBnqgupArc="));
    assert_eq!(url_safe.len(), 1, "url-safe base64 ids must be accepted");
    assert_eq!(url_safe[0].trace_id.len(), 32);

    for (trace, span) in [
        (TRACE, "00f067aa0ba902"),
        ("4bf92f3577b34da6a3ce929d0e0e47", SPAN),
        ("not hex and not base64!!", SPAN),
        (TRACE, "0000000000000000"),
        ("", SPAN),
    ] {
        assert!(
            spans_of(&with_ids(trace, span)).is_empty(),
            "trace {trace:?} span {span:?} must not produce a storable span"
        );
    }
}

// ----------------------------------------------------- the wire itself ---

/// A span message carrying the given fields, wrapped so it can be decoded.
fn batch_of(spans: &[Vec<u8>]) -> Vec<u8> {
    request(&[kv("service.name", &any_string("checkout-agent"))], spans)
}

fn names_in(body: &[u8]) -> Vec<String> {
    protobuf::decode_traces(body)
        .expect("the batch itself is well formed")
        .iter()
        .map(|s| s.name.clone())
        .collect()
}

#[test]
fn a_known_field_that_arrives_with_the_wrong_wire_type_is_skipped_not_misread() {
    // No conforming encoder does this, but a corrupted body or a hostile one
    // can, and the decoder matches on the field and the wire type together. The
    // pair that does not match falls to the skip path, so the field keeps its
    // default rather than being read as bytes that mean something else.
    let mut wrong = Pb::new();
    wrong
        .bytes(1, &TRACE_BYTES)
        .bytes(2, &SPAN_BYTES)
        .uint(5, 42)
        .uint(7, 1_735_689_600_000_000_000)
        .uint(8, 1_735_689_601_000_000_000)
        .fixed64(15, 2);

    let decoded = protobuf::decode_traces(&batch_of(&[wrong.take()])).expect("still a valid body");
    assert_eq!(decoded.len(), 1, "a wrong wire type must not cost the whole span");
    assert_eq!(decoded[0].name, "span", "a varint name is skipped, so the default stands");
    assert_eq!(decoded[0].start_unix_nano, 0, "a varint start time is not read as a timestamp");
    assert_eq!(decoded[0].end_unix_nano, 0);
    assert_eq!(decoded[0].status_code, 0, "a fixed64 status is not read as a status message");

    // The control: the same field numbers with the wire types OTLP declares.
    let mut right = Pb::new();
    let mut status = Pb::new();
    status.uint(3, 2);
    right
        .bytes(1, &TRACE_BYTES)
        .bytes(2, &SPAN_BYTES)
        .string(5, "chat gpt-4o")
        .fixed64(7, 1_735_689_600_000_000_000)
        .fixed64(8, 1_735_689_601_000_000_000)
        .bytes(15, &status.take());
    let decoded = protobuf::decode_traces(&batch_of(&[right.take()])).expect("the control decodes");
    assert_eq!(decoded[0].name, "chat gpt-4o");
    assert_eq!(decoded[0].start_unix_nano, 1_735_689_600_000_000_000);
    assert_eq!(decoded[0].status_code, 2);
}

#[test]
fn a_span_whose_ids_arrive_with_the_wrong_wire_type_is_dropped_rather_than_stored() {
    // Skipping the field is right for a name and wrong for an id, so the
    // emptiness check after the loop is what has to catch this.
    let mut no_trace = Pb::new();
    no_trace.uint(1, 0x4bf9_2f35).bytes(2, &SPAN_BYTES).string(5, "no trace id");
    let mut no_span = Pb::new();
    no_span.bytes(1, &TRACE_BYTES).fixed64(2, 0x00f0_67aa).string(5, "no span id");

    assert_eq!(
        names_in(&batch_of(&[no_trace.take(), no_span.take(), generation_span()])),
        ["chat gpt-4o"],
        "only the span that still has both ids may be stored"
    );
}

#[test]
fn the_wire_types_proto3_removed_are_refused_wherever_they_appear() {
    // 3 and 4 are the group markers, which proto3 deleted; 6 and 7 have never
    // been assigned. Reading a length or a value for any of them would be a
    // guess, so the batch is rejected instead.
    for wire in [3u32, 4, 6, 7] {
        let mut body = Pb::new();
        body.tag(1, wire).varint(0);
        let outcome = protobuf::decode_traces(&body.take());
        let Err(error) = outcome else {
            panic!("wire type {wire} was decoded rather than refused: {outcome:?}")
        };
        assert!(error.to_string().contains("unsupported wire type"), "wire {wire} gave: {error}");
    }

    // Inside one span the softer rule applies: that span is lost, the rest of
    // the batch is not.
    let mut grouped = Pb::new();
    grouped.bytes(1, &TRACE_BYTES).bytes(2, &CHILD_BYTES).string(5, "grouped").tag(5, 3);
    grouped.varint(0);
    assert_eq!(
        names_in(&batch_of(&[grouped.take(), generation_span()])),
        ["chat gpt-4o"],
        "a group marker inside a span drops that span alone"
    );
}

#[test]
fn a_varint_is_read_to_ten_bytes_and_refused_past_them() {
    // The status code is the varint that reaches a stored field, so it is the
    // one to send the awkward values through. u64::MAX is ten bytes, the
    // longest a 64 bit varint can be.
    let mut status = Pb::new();
    status.uint(3, u64::MAX);
    let mut span = Pb::new();
    span.bytes(1, &TRACE_BYTES).bytes(2, &SPAN_BYTES).string(5, "max").bytes(15, &status.take());
    let decoded = protobuf::decode_traces(&batch_of(&[span.take()])).expect("ten bytes is legal");
    assert_eq!(decoded[0].status_code, -1, "the code is narrowed to i32, not rejected");
    assert_eq!(
        to_span(&decoded[0], "fallback", epoch()).status,
        SpanStatus::Unset,
        "a code outside the enum is neither ok nor error, and an unfinished span is unset"
    );

    // An eleventh continuation byte cannot be part of a 64 bit value.
    let mut eleven = Pb::new();
    eleven.tag(1, 0);
    for _ in 0..10 {
        eleven.0.push(0xff);
    }
    eleven.0.push(0x7f);
    let error = protobuf::decode_traces(&eleven.take()).expect_err("must not be read").to_string();
    assert!(error.contains("longer than 64 bits"), "unexpected reason: {error}");

    // A varint whose last byte still asks for another one, with none left.
    let mut cut = Pb::new();
    cut.tag(1, 0);
    cut.0.push(0x80);
    let error = protobuf::decode_traces(&cut.take()).expect_err("must not be read").to_string();
    assert!(error.contains("truncated varint"), "unexpected reason: {error}");

    // Multi-byte varints in the ordinary range still carry their value.
    let mut tokens = Pb::new();
    tokens.uint(3, 300_000);
    let mut kv_big = Pb::new();
    kv_big.string(1, "gen_ai.usage.input_tokens").bytes(2, &tokens.take());
    let mut span = Pb::new();
    span.bytes(1, &TRACE_BYTES).bytes(2, &SPAN_BYTES).bytes(9, &kv_big.take());
    let decoded = protobuf::decode_traces(&batch_of(&[span.take()])).expect("decodes");
    assert_eq!(decoded[0].attributes["gen_ai.usage.input_tokens"], Value::from(300_000));
}

#[test]
fn a_length_that_claims_more_than_its_frame_holds_is_refused_at_that_frame() {
    // The outer message is the only level where this fails the request, which
    // is the same division the truncation test relies on.
    let mut lying_top = Pb::new();
    lying_top.tag(1, 2).varint(200);
    lying_top.0.extend_from_slice(&[0u8; 4]);
    let error =
        protobuf::decode_traces(&lying_top.take()).expect_err("a lie must not be read").to_string();
    assert!(error.contains("runs past the end"), "unexpected reason: {error}");

    // A length larger than the address space must be caught by the same check
    // rather than by an overflowing addition.
    let mut enormous = Pb::new();
    enormous.tag(1, 2).varint(u64::MAX);
    enormous.0.extend_from_slice(&[0u8; 4]);
    assert!(protobuf::decode_traces(&enormous.take()).is_err(), "u64::MAX bytes are not present");

    // Inside a span the lie costs that span and nothing else.
    let mut liar = Pb::new();
    liar.bytes(1, &TRACE_BYTES).bytes(2, &CHILD_BYTES).string(5, "liar").tag(9, 2).varint(4096);
    assert_eq!(
        names_in(&batch_of(&[liar.take(), generation_span()])),
        ["chat gpt-4o"],
        "an attribute that claims four kilobytes it does not have drops its own span"
    );

    // One level further in: the KeyValue frame is intact, the AnyValue inside
    // it is not.
    let mut inner_liar = Pb::new();
    inner_liar.tag(1, 2).varint(64);
    let mut attribute = Pb::new();
    attribute.string(1, "gen_ai.system").bytes(2, &inner_liar.take());
    let mut span = Pb::new();
    span.bytes(1, &TRACE_BYTES)
        .bytes(2, &CHILD_BYTES)
        .string(5, "inner")
        .bytes(9, &attribute.take());
    assert_eq!(names_in(&batch_of(&[span.take(), generation_span()])), ["chat gpt-4o"]);
}

#[test]
fn a_field_that_should_appear_once_and_appears_twice_keeps_the_last_value() {
    // proto3 says the last value wins for a scalar, and an exporter that
    // retries a partial write can produce exactly this.
    let mut status_one = Pb::new();
    status_one.uint(3, 1);
    let mut status_two = Pb::new();
    status_two.string(2, "second").uint(3, 2);
    let mut span = Pb::new();
    span.bytes(1, &TRACE_BYTES)
        .bytes(2, &SPAN_BYTES)
        .string(5, "first name")
        .string(5, "second name")
        .fixed64(7, 111)
        .fixed64(7, 222)
        .bytes(15, &status_one.take())
        .bytes(15, &status_two.take());

    let decoded = protobuf::decode_traces(&batch_of(&[span.take()])).expect("decodes");
    assert_eq!(decoded[0].name, "second name");
    assert_eq!(decoded[0].start_unix_nano, 222);
    assert_eq!(decoded[0].status_code, 2, "the last status block wins");
    assert_eq!(decoded[0].status_message.as_deref(), Some("second"));
}

#[test]
fn an_id_that_appears_twice_is_judged_on_the_one_that_cannot_be_stored() {
    // Whichever way round they arrive, a reserved id anywhere in the field
    // drops the span. Storing the other one would mean storing an id the
    // producer did not mean.
    let mut good_then_bad = Pb::new();
    good_then_bad.bytes(1, &TRACE_BYTES).bytes(1, &[0u8; 16]).bytes(2, &SPAN_BYTES);
    let mut bad_then_good = Pb::new();
    bad_then_good.bytes(1, &[0u8; 16]).bytes(1, &TRACE_BYTES).bytes(2, &SPAN_BYTES);

    for span in [good_then_bad.take(), bad_then_good.take()] {
        assert!(
            protobuf::decode_traces(&batch_of(&[span])).expect("the batch survives").is_empty(),
            "a span carrying an all-zero trace id must not be stored"
        );
    }

    // The wrong length is the other way a repeated id field goes bad.
    let mut short = Pb::new();
    short.bytes(1, &TRACE_BYTES).bytes(1, &TRACE_BYTES[..8]).bytes(2, &SPAN_BYTES);
    assert!(protobuf::decode_traces(&batch_of(&[short.take()])).unwrap().is_empty());
}

#[test]
fn an_attribute_key_that_repeats_keeps_the_last_value_on_both_paths() {
    // A map is a repeated KeyValue on the wire, so nothing stops a producer
    // sending the same key twice. Both decoders have to answer the same way,
    // because a model name is one of these keys.
    let mut span = Pb::new();
    span.bytes(1, &TRACE_BYTES)
        .bytes(2, &SPAN_BYTES)
        .string(5, "chat")
        .bytes(9, &kv("gen_ai.request.model", &any_string("gpt-4o-mini")))
        .bytes(9, &kv("gen_ai.request.model", &any_string("gpt-4o")));
    let decoded = protobuf::decode_traces(&batch_of(&[span.take()])).expect("decodes");
    assert_eq!(decoded[0].attributes["gen_ai.request.model"], Value::from("gpt-4o"));

    let from_json = only_span(json!([
        attr("gen_ai.request.model", s("gpt-4o-mini")),
        attr("gen_ai.request.model", s("gpt-4o"))
    ]));
    assert_eq!(from_json.model.as_deref(), Some("gpt-4o"), "the two paths must not disagree");
}

/// Wrap a string `AnyValue` in `levels` nested `KeyValueList` layers.
///
/// The other recursive path through the decoder. Each layer costs two levels of
/// depth, because an `AnyValue` holding a list and a `KeyValue` holding a value
/// each count.
fn nested_kvlist_value(levels: usize) -> Vec<u8> {
    let mut buf = any_string("deep");
    for _ in 0..levels {
        let mut list = Pb::new();
        list.bytes(1, &kv("k", &buf));
        let mut wrapper = Pb::new();
        wrapper.bytes(6, &list.take());
        buf = wrapper.take();
    }
    buf
}

fn nested_object(levels: usize) -> Value {
    let mut v = Value::from("deep");
    for _ in 0..levels {
        v = json!({ "k": v });
    }
    v
}

#[test]
fn a_key_value_list_is_bounded_by_the_same_depth_limit_as_an_array() {
    // The array path is already covered. This is the other way to recurse, and
    // it reaches the limit twice as fast, so a payload half the size of the
    // array one has to be refused all the same.
    let deep = request(&[kv("nest", &nested_kvlist_value(20))], &[generation_span()]);
    let error = protobuf::decode_traces(&deep).expect_err("twenty layers is past the limit");
    assert!(error.to_string().contains("nested too deeply"), "{error}");

    let shallow = request(&[kv("nest", &nested_kvlist_value(8))], &[generation_span()]);
    let decoded = protobuf::decode_traces(&shallow).expect("eight layers is inside the limit");
    assert_eq!(decoded[0].resource["nest"], nested_object(8), "every layer must arrive intact");
}

#[test]
fn the_attribute_kinds_the_unit_tests_leave_out_arrive_whole() {
    // bytes_value and kvlist_value are the two AnyValue arms nothing else
    // exercises through a whole request, and bytes is the one that changes
    // shape on the way in: OTLP sends raw bytes, mcpd stores hex.
    let mut blob = Pb::new();
    blob.bytes(7, &[0xde, 0xad, 0xbe, 0xef]);
    let mut list = Pb::new();
    list.bytes(1, &kv("role", &any_string("user")));
    let mut kvlist = Pb::new();
    kvlist.bytes(6, &list.take());
    let mut span = Pb::new();
    span.bytes(1, &TRACE_BYTES)
        .bytes(2, &SPAN_BYTES)
        .string(5, "kinds")
        .bytes(9, &kv("blob", &blob.take()))
        .bytes(9, &kv("message", &kvlist.take()))
        .bytes(9, &kv("nothing", &[]));

    let decoded = protobuf::decode_traces(&batch_of(&[span.take()])).expect("decodes");
    let attributes = &decoded[0].attributes;
    assert_eq!(attributes["blob"], Value::from("deadbeef"), "bytes arrive hex encoded");
    assert_eq!(attributes["message"], json!({ "role": "user" }));
    assert_eq!(attributes["nothing"], Value::Null, "an AnyValue with no arm set is null");
}

#[test]
fn protobuf_that_is_valid_and_describes_nothing_is_an_empty_batch_not_an_error() {
    // Every one of these is a legal encoding, and a batching exporter that
    // flushes on a timer produces them when there was nothing to send.
    let mut empty_resource_spans = Pb::new();
    empty_resource_spans.bytes(1, &[]);

    let mut scope_only = Pb::new();
    scope_only.string(1, "scope with no spans");
    let mut no_spans = Pb::new();
    no_spans.bytes(2, &scope_only.take());
    let mut no_spans_request = Pb::new();
    no_spans_request.bytes(1, &no_spans.take());

    let mut resource_only = Pb::new();
    resource_only.bytes(1, &kv("service.name", &any_string("checkout-agent")));
    let mut no_scopes = Pb::new();
    no_scopes.bytes(1, &resource_only.take());
    let mut no_scopes_request = Pb::new();
    no_scopes_request.bytes(1, &no_scopes.take());

    for body in [
        empty_resource_spans.take(),
        no_spans_request.take(),
        no_scopes_request.take(),
        // A span message with no fields at all: no ids, so nothing to store.
        batch_of(&[Vec::new()]),
    ] {
        let decoded = protobuf::decode_traces(&body).expect("a legal encoding is not an error");
        assert!(decoded.is_empty(), "nothing was described, so nothing may be stored");
    }
}

#[test]
fn timestamps_that_could_not_have_happened_never_wrap_a_duration_or_panic() {
    // Clock skew between two machines routinely puts an end before a start, and
    // a producer with a broken clock can send anything a u64 of nanoseconds
    // holds. Neither may produce a negative duration, which is stored unsigned.
    let case = |name: &str, start: u64, end: u64| {
        let mut span = Pb::new();
        span.bytes(1, &TRACE_BYTES).bytes(2, &SPAN_BYTES).string(5, name);
        if start > 0 {
            span.fixed64(7, start);
        }
        if end > 0 {
            span.fixed64(8, end);
        }
        let decoded = protobuf::decode_traces(&batch_of(&[span.take()])).expect("decodes");
        to_span(&decoded[0], "fallback", epoch())
    };

    let skewed = case("end before start", 2_000_000_000_000_000_000, 1_000_000_000_000_000_000);
    assert_eq!(skewed.duration_ns, Some(0), "a negative duration is clamped, never wrapped");
    assert!(skewed.end_time.is_some(), "the end time itself is kept as it was sent");

    let running = case("still running", 1_735_689_600_000_000_000, 0);
    assert_eq!(running.end_time, None);
    assert_eq!(running.duration_ns, None, "an unfinished span has no duration to report");
    assert_eq!(running.status, SpanStatus::Unset);

    let far_future = case("far future", 0, u64::MAX);
    assert_eq!(
        far_future.duration_ns,
        Some(u64::MAX),
        "the largest nanosecond value a fixed64 holds must survive the subtraction"
    );
    assert_eq!(far_future.start_time, epoch(), "a zero start time is the epoch, not an error");
    assert_eq!(far_future.status, SpanStatus::Ok, "a finished span with no code reads as ok");
}

// -------------------------------------------- where the two paths differ ---

/// BUG, in `flatten_attributes` in `crates/mcpd-core/src/otlp/mod.rs` against
/// `key_values_at` and `span` in `otlp/protobuf.rs`: the two decoders keep the
/// opposite halves of a degenerate `KeyValue`. The JSON reader stores an
/// attribute whose key is the empty string and drops one that has a key but no
/// value; the protobuf reader drops the empty key and stores the valueless key
/// as null. So the same batch, re-exported over the other protocol, produces a
/// different attribute map, which is what the module header says cannot happen.
///
/// Input: one span with `KeyValue{key:"", value:"orphan"}` and
/// `KeyValue{key:"keyed"}` with no value, sent both ways.
/// Expected: one set of attribute keys, whichever set is chosen.
/// Observed: `[""]` from JSON and `["keyed"]` from protobuf.
#[test]
fn the_two_encodings_agree_about_a_degenerate_attribute() {
    // A KeyValue with an empty key, and one with a key and no value at all.
    // Both are legal on the wire and neither carries anything usable, so the
    // two decoders have to reach the same answer about them.
    let mut empty_key = Pb::new();
    empty_key.string(1, "").bytes(2, &any_string("orphan"));
    let mut no_value = Pb::new();
    no_value.string(1, "keyed");
    let mut span = Pb::new();
    span.bytes(1, &TRACE_BYTES)
        .bytes(2, &SPAN_BYTES)
        .string(5, "degenerate")
        .bytes(9, &empty_key.take())
        .bytes(9, &no_value.take());
    let from_protobuf = protobuf::decode_traces(&batch_of(&[span.take()])).expect("decodes");
    let protobuf_keys: Vec<&String> = from_protobuf[0].attributes.keys().collect();

    let from_json = decode_json(&one_span(json!([
        json!({ "key": "", "value": { "stringValue": "orphan" } }),
        json!({ "key": "keyed" })
    ])))
    .expect("decodes");
    let json_keys: Vec<&String> = from_json[0].attributes.keys().collect();

    assert_eq!(json_keys, protobuf_keys, "one payload, two encodings, one set of attributes");
}

#[test]
fn an_attribute_nested_forty_deep_is_refused_over_protobuf_and_kept_over_json() {
    // Not a duplicate of the depth-limit tests: the point here is that the
    // limit exists on one path only. The JSON reader recurses through the same
    // shape with no bound of its own and is held up only by serde_json's parse
    // depth limit, which the server relies on without saying so.
    let over_protobuf = protobuf::decode_traces(&request(
        &[kv("nest", &nested_any_value(40))],
        &[generation_span()],
    ));
    assert!(over_protobuf.is_err(), "the protobuf path refuses forty layers");

    let over_json = decode_json(&one_span(json!([
        json!({ "key": "nest", "value": nested_json_any_value(40) })
    ])))
    .expect("the JSON path accepts what the protobuf path refuses");
    assert_eq!(over_json[0].attributes["nest"], nested_value(40));
}

/// The JSON spelling of [`nested_any_value`]: `levels` nested `arrayValue`s.
fn nested_json_any_value(levels: usize) -> Value {
    let mut v = json!({ "stringValue": "deep" });
    for _ in 0..levels {
        v = json!({ "arrayValue": { "values": [v] } });
    }
    v
}

#[test]
fn arbitrary_bytes_are_an_error_or_a_batch_and_never_a_panic() {
    // The corruption test next door mutates one real payload, so every input it
    // tries still has the shape of a trace request. This one has no shape at
    // all, which is what an unauthenticated POST of anything looks like.
    // Deterministic, so a failure can be reproduced from the seed alone.
    let mut state = 0x2545_f491_4f6c_dd1d_u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };

    let mut decoded_something = 0;
    for _ in 0..4000 {
        let len = (next() % 64) as usize;
        let body: Vec<u8> = (0..len).map(|_| (next() & 0xff) as u8).collect();
        let Ok(spans) = protobuf::decode_traces(&body) else { continue };
        decoded_something += usize::from(!spans.is_empty());
        for span in &spans {
            // Anything that survives is about to be written to storage, so its
            // ids have to be ids.
            assert!(ids::validate_trace_id(&span.trace_id).is_ok(), "body {body:?}");
            assert!(ids::validate_span_id(&span.span_id).is_ok(), "body {body:?}");
            assert!(span.name.len() <= body.len(), "a name longer than the body it came from");
        }
    }
    // Random bytes almost never contain a whole span, and that is the point of
    // reporting it: the loop above must be reading the decoder, not skipping it.
    assert_eq!(decoded_something, 0, "random bytes are not expected to describe a span");
}

/// The over-bill the attribute-name heuristic caused on its own.
///
/// BUG, in `extract_usage` in `crates/mcpd-core/src/otlp/mod.rs`: `net_input`
/// was `attr(cache_read_input_tokens | cache_creation_input_tokens).is_some()`
/// and nothing else. `gen_ai.*` is a vendor-neutral namespace, so an
/// instrumentation mapping OpenAI's `prompt_tokens_details.cached_tokens` onto
/// `gen_ai.usage.cache_read_input_tokens` is ordinary, and OpenAI's
/// `prompt_tokens` is GROSS of it. Its whole prompt was then recorded as the
/// uncached share and the cached tokens billed a second time at the plain rate.
///
/// Input: one OTLP span, `gen_ai.system=openai`,
/// `gen_ai.request.model=gpt-4o`, `gen_ai.usage.input_tokens=1000`,
/// `gen_ai.usage.cache_read_input_tokens=900`.
/// Expected: 100 uncached at 2.50 plus 900 cached at 1.25 per million,
/// 0.001375. That is what the same daemon charges for the same counts when
/// they arrive from an SDK, which is the point: one call must not cost two
/// different figures because of which door it came in through.
/// Observed: 0.003625, a 164% over-bill.
#[test]
fn an_openai_span_using_anthropics_cache_attribute_is_still_billed_gross() {
    let span = only_span(json!([
        attr("gen_ai.system", s("openai")),
        attr("gen_ai.request.model", s("gpt-4o")),
        attr("gen_ai.usage.input_tokens", i(1000)),
        attr("gen_ai.usage.output_tokens", i(0)),
        attr("gen_ai.usage.cache_read_input_tokens", i(900))
    ]));

    let usage = span.usage.clone().expect("the span reports usage");
    let details = usage.input_details.clone().expect("a cache count implies input detail");
    assert_eq!(details.cache_read, Some(900));
    assert_eq!(
        details.text, None,
        "the span says OpenAI served a gpt-4o call, so its input total is gross",
    );

    let c = cost::compute(&cost::builtin_book(), "gpt-4o", &usage).expect("a known model prices");
    assert!((c.input_cost.unwrap() - 0.001_375).abs() < 1e-12, "got {:?}", c.input_cost);
}

/// The convention belongs to the model vendor, not to whoever served the call.
///
/// Bedrock and Vertex hand back Anthropic's own usage block, net of cache, and
/// they say `aws.bedrock` and `gcp.vertex_ai` in `gen_ai.system`. Gating the
/// net reading on the provider name alone would have lost every Claude call
/// that did not go to Anthropic directly, which is most of the enterprise ones.
#[test]
fn a_claude_call_served_by_bedrock_still_reports_its_uncached_share() {
    for (system, model) in [
        ("aws.bedrock", "us.anthropic.claude-sonnet-4-20250514-v1:0"),
        ("gcp.vertex_ai", "claude-sonnet-4@20250514"),
        ("anthropic", "claude-sonnet-4"),
    ] {
        let span = only_span(json!([
            attr("gen_ai.system", s(system)),
            attr("gen_ai.request.model", s(model)),
            attr("gen_ai.usage.input_tokens", i(100)),
            attr("gen_ai.usage.output_tokens", i(0)),
            attr("gen_ai.usage.cache_read_input_tokens", i(1000))
        ]));
        let details =
            span.usage.expect("usage").input_details.expect("a cache count implies input detail");
        assert_eq!(details.text, Some(100), "{system} lost the uncached share for {model}");
    }
}

/// A span that names nobody keeps the reading the attribute name gives.
///
/// The gate narrows the guess where the span contradicts it. It must not turn
/// into a requirement that a span identify itself, or the under-bill comes
/// straight back for every producer that reports counts and nothing else.
#[test]
fn a_span_that_names_no_provider_or_model_still_reads_the_attribute_name() {
    let span = only_span(json!([
        attr("gen_ai.usage.input_tokens", i(100)),
        attr("gen_ai.usage.output_tokens", i(0)),
        attr("gen_ai.usage.cache_read_input_tokens", i(1000))
    ]));
    let details =
        span.usage.expect("usage").input_details.expect("a cache count implies input detail");
    assert_eq!(details.text, Some(100));
}

// ------------------------------------------------ links, events and flags ---
//
// Everything below covers the four fields the decoders used to drop on the
// floor: `links`, `events`, `flags`, and the `external_parent_span_id` that
// only a whole request can decide. Each property is asserted on BOTH encodings,
// because the module's headline claim is that they cannot drift, and a mapping
// added on one path only is exactly how they would.

/// A span event on the JSON wire.
fn json_event(time: Option<&str>, name: &str, attributes: Value) -> Value {
    let mut v = json!({ "name": name, "attributes": attributes });
    if let Some(t) = time {
        v["timeUnixNano"] = json!(t);
    }
    v
}

/// A span link on the JSON wire.
fn json_link(trace: &str, span: Option<&str>, attributes: Value) -> Value {
    let mut v = json!({ "traceId": trace, "attributes": attributes });
    if let Some(s) = span {
        v["spanId"] = json!(s);
    }
    v
}

/// One JSON span, with everything this section varies.
fn json_span(extra: Value) -> Value {
    let mut span = json!({
        "traceId": TRACE, "spanId": SPAN, "name": "checkout",
        "startTimeUnixNano": "1735689600000000000",
        "endTimeUnixNano": "1735689601000000000"
    });
    for (k, v) in extra.as_object().expect("an object of span fields") {
        span[k.clone()] = v.clone();
    }
    json!({ "resourceSpans": [{ "scopeSpans": [{ "spans": [span] }] }] })
}

/// `Span.Event { fixed64 time_unix_nano = 1; string name = 2; KeyValue attributes = 3 }`
fn pb_event(time: u64, name: &str, attributes: &[Vec<u8>]) -> Vec<u8> {
    let mut e = Pb::new();
    if time > 0 {
        e.fixed64(1, time);
    }
    e.string(2, name);
    for a in attributes {
        e.bytes(3, a);
    }
    e.take()
}

/// `Span.Link { bytes trace_id = 1; bytes span_id = 2; KeyValue attributes = 4 }`
fn pb_link(trace: &[u8], span: Option<&[u8]>, attributes: &[Vec<u8>]) -> Vec<u8> {
    let mut l = Pb::new();
    l.bytes(1, trace);
    if let Some(s) = span {
        l.bytes(2, s);
    }
    for a in attributes {
        l.bytes(4, a);
    }
    l.take()
}

/// The protobuf twin of [`json_span`]: the same span, with events, links and
/// flags supplied as already encoded messages.
fn pb_span_request(
    parent: Option<&[u8]>,
    flags: u32,
    events: &[Vec<u8>],
    links: &[Vec<u8>],
    attributes: &[Vec<u8>],
) -> Vec<u8> {
    let mut span = Pb::new();
    span.bytes(1, &TRACE_BYTES).bytes(2, &SPAN_BYTES);
    if let Some(p) = parent {
        span.bytes(4, p);
    }
    span.string(5, "checkout")
        .fixed64(7, 1_735_689_600_000_000_000)
        .fixed64(8, 1_735_689_601_000_000_000);
    for a in attributes {
        span.bytes(9, a);
    }
    for e in events {
        span.bytes(11, e);
    }
    for l in links {
        span.bytes(13, l);
    }
    if flags != 0 {
        span.fixed32(16, flags);
    }
    request(&[], &[span.take()])
}

/// Map a protobuf request the way the ingest door does.
fn pb_spans(body: &[u8]) -> Vec<Span> {
    let decoded = protobuf::decode_traces(body).expect("protobuf decodes");
    to_spans(&decoded, "fallback", epoch())
}

/// Map a JSON request the way the ingest door does.
fn json_spans(body: &Value) -> Vec<Span> {
    to_spans(&decode_json(body).expect("json decodes"), "fallback", epoch())
}

/// The one span in a mapped batch that is not an event.
fn carrier(spans: &[Span]) -> &Span {
    spans.iter().find(|s| !s.is_event).expect("the batch should carry one real span")
}

fn events_of(spans: &[Span]) -> Vec<&Span> {
    spans.iter().filter(|s| s.is_event).collect()
}

#[test]
fn a_link_reaches_the_stored_span_on_both_encodings() {
    // P019. Both decoders dropped `links` unconditionally, so a batch job
    // caused by twenty requests arrived with no record of any of them, even
    // though the model, all three stores and four SDKs already carry links.
    let from_json = json_spans(&json_span(json!({
        "links": [
            json_link(OTHER_TRACE, Some(CHILD), json!([attr("mcpd.link.kind", s("retry_of"))])),
            json_link(OTHER_TRACE, None, json!([]))
        ]
    })));
    let from_pb = pb_spans(&pb_span_request(
        None,
        0,
        &[],
        &[
            pb_link(
                &OTHER_TRACE_BYTES,
                Some(&CHILD_BYTES),
                &[kv("mcpd.link.kind", &any_string("retry_of"))],
            ),
            pb_link(&OTHER_TRACE_BYTES, None, &[]),
        ],
        &[],
    ));

    for (label, spans) in [("json", from_json), ("protobuf", from_pb)] {
        let links = carrier(&spans).links.clone().unwrap_or_else(|| panic!("{label}: no links"));
        assert_eq!(links.len(), 2, "{label} lost a link");
        assert_eq!(links[0].trace_id, OTHER_TRACE, "{label}");
        assert_eq!(links[0].span_id.as_deref(), Some(CHILD), "{label}");
        assert_eq!(links[0].kind.as_deref(), Some("retry_of"), "{label}");
        assert_eq!(links[0].attributes.as_ref().expect("attributes")["mcpd.link.kind"], "retry_of");
        // A link may name a trace and no span. mcpd's model allows exactly
        // that, so it is a link to the whole trace, not a malformed one.
        assert_eq!(links[1].trace_id, OTHER_TRACE, "{label}");
        assert_eq!(links[1].span_id, None, "{label}");
        assert_eq!(links[1].kind, None, "{label} invented a reason for a link that gave none");
        assert_eq!(links[1].attributes, None, "{label}");
    }
}

#[test]
fn the_opentracing_shim_reference_type_is_read_as_the_link_reason() {
    // The one other convention that says why a link exists: OpenTelemetry's
    // OpenTracing shim writes this when it converts a FOLLOWS_FROM reference.
    let spans = json_spans(&json_span(json!({
        "links": [json_link(
            OTHER_TRACE,
            Some(CHILD),
            json!([attr("opentracing.ref_type", s("follows_from"))])
        )]
    })));
    let links = carrier(&spans).links.clone().expect("links");
    assert_eq!(links[0].kind.as_deref(), Some("follows_from"));
}

#[test]
fn a_link_whose_trace_id_is_reserved_is_dropped_on_both_encodings() {
    // P011 over the new surface. An all-zero id is reserved, and a link
    // carrying one points at nothing that can ever be looked up.
    let zeros = "0".repeat(32);
    let from_json = json_spans(&json_span(json!({
        "links": [
            json_link(&zeros, Some(CHILD), json!([])),
            json_link(OTHER_TRACE, Some(CHILD), json!([]))
        ]
    })));
    let from_pb = pb_spans(&pb_span_request(
        None,
        0,
        &[],
        &[
            pb_link(&[0u8; 16], Some(&CHILD_BYTES), &[]),
            pb_link(&OTHER_TRACE_BYTES, Some(&CHILD_BYTES), &[]),
        ],
        &[],
    ));

    for (label, spans) in [("json", from_json), ("protobuf", from_pb)] {
        let links = carrier(&spans).links.clone().unwrap_or_else(|| panic!("{label}: no links"));
        assert_eq!(links.len(), 1, "{label} stored a link to the reserved trace id");
        assert_eq!(links[0].trace_id, OTHER_TRACE, "{label}");
    }
}

#[test]
fn a_span_event_becomes_an_event_span_under_the_span_that_carried_it() {
    // P019. mcpd already models a point in time as a zero-duration span with
    // `is_event` set, so an OTel span event maps onto one parented to the span
    // it was recorded on. Nothing else in the model can hold it, and a blob
    // hanging off the parent would be invisible to every query mcpd has.
    let from_json = json_spans(&json_span(json!({
        "attributes": [attr("mcpd.project", s("checkout"))],
        "events": [json_event(
            Some("1735689600500000000"),
            "cache.miss",
            json!([attr("cache.key", s("orders/42"))])
        )]
    })));
    let from_pb = pb_spans(&pb_span_request(
        None,
        0,
        &[pb_event(
            1_735_689_600_500_000_000,
            "cache.miss",
            &[kv("cache.key", &any_string("orders/42"))],
        )],
        &[],
        &[kv("mcpd.project", &any_string("checkout"))],
    ));

    for (label, spans) in [("json", from_json), ("protobuf", from_pb)] {
        assert_eq!(spans.len(), 2, "{label}: the event should be a span of its own");
        let events = events_of(&spans);
        assert_eq!(events.len(), 1, "{label}");
        let e = events[0];
        assert_eq!(e.name, "cache.miss", "{label}");
        assert_eq!(e.trace_id, TRACE, "{label}: an event stays in its own trace");
        assert_eq!(e.parent_span_id.as_deref(), Some(SPAN), "{label}");
        assert!(!e.is_root, "{label}: an event is never a root");
        assert!(e.is_event, "{label}");
        assert_eq!(e.end_time, None, "{label}: an event is a point in time");
        assert_eq!(e.duration_ns, None, "{label}");
        assert_eq!(
            e.start_time.unix_timestamp_nanos(),
            1_735_689_600_500_000_000,
            "{label}: the event keeps its own instant"
        );
        assert_eq!(e.attributes.as_ref().expect("attributes")["cache.key"], "orders/42", "{label}");
        // Correlation is inherited from the carrier. Re-deriving it from the
        // event's own attributes would file this event under `fallback` while
        // the span that carried it went to `checkout`.
        assert_eq!(e.correlation.project, "checkout", "{label}");
        assert_eq!(carrier(&spans).correlation.project, "checkout", "{label}");
        // The id has to be storable, and every store refuses anything else.
        ids::validate_span_id(&e.span_id).unwrap_or_else(|why| panic!("{label}: {why}"));
        assert_ne!(e.span_id, SPAN, "{label}: an event must not overwrite its own parent");
    }
}

#[test]
fn an_exception_event_gives_the_failing_span_its_stack() {
    // P019, the half that was silently lost: `span.recordException(e)` puts the
    // type, the message and the stack on an event, and the mapper only ever
    // read span attributes, so every stack from a stock OpenTelemetry SDK was
    // dropped while the span still said it had failed.
    let exception = json!([
        attr("exception.type", s("RateLimitError")),
        attr("exception.message", s("429 too many requests")),
        attr("exception.stacktrace", s("Traceback:\n  provider.py:88 in chat"))
    ]);
    let from_json = json_spans(&json_span(json!({
        "status": { "code": 2 },
        "events": [json_event(Some("1735689600900000000"), "exception", exception)]
    })));

    let mut status = Pb::new();
    status.uint(3, 2);
    let mut span = Pb::new();
    span.bytes(1, &TRACE_BYTES)
        .bytes(2, &SPAN_BYTES)
        .string(5, "checkout")
        .fixed64(7, 1_735_689_600_000_000_000)
        .fixed64(8, 1_735_689_601_000_000_000)
        .bytes(
            11,
            &pb_event(
                1_735_689_600_900_000_000,
                "exception",
                &[
                    kv("exception.type", &any_string("RateLimitError")),
                    kv("exception.message", &any_string("429 too many requests")),
                    kv("exception.stacktrace", &any_string("Traceback:\n  provider.py:88 in chat")),
                ],
            ),
        )
        .bytes(15, &status.take());
    let from_pb = pb_spans(&request(&[], &[span.take()]));

    for (label, spans) in [("json", from_json), ("protobuf", from_pb)] {
        let failed = carrier(&spans);
        assert_eq!(failed.status, SpanStatus::Error, "{label}");
        let error = failed.error.as_ref().unwrap_or_else(|| panic!("{label}: no error"));
        assert_eq!(error.kind.as_deref(), Some("RateLimitError"), "{label}");
        assert_eq!(error.message, "429 too many requests", "{label}");
        assert!(
            error.stack.as_deref().unwrap_or_default().contains("provider.py:88"),
            "{label}: the stack from the exception event never reached the span"
        );
        // And it is still an event in its own right, at the instant it was
        // thrown, so the waterfall shows when the failure happened.
        let events = events_of(&spans);
        assert_eq!(events.len(), 1, "{label}");
        assert_eq!(events[0].name, "exception", "{label}");
        assert_eq!(
            events[0].start_time.unix_timestamp_nanos(),
            1_735_689_600_900_000_000,
            "{label}"
        );
    }
}

#[test]
fn a_span_attribute_still_beats_the_exception_event_it_disagrees_with() {
    // The span's own attributes are a deliberate statement about the span; the
    // event is a record of one throw among possibly several. Order matters
    // only when they disagree, which is what this pins.
    let spans = json_spans(&json_span(json!({
        "status": { "code": 2 },
        "attributes": [attr("exception.type", s("FromTheSpan"))],
        "events": [json_event(
            Some("1735689600900000000"),
            "exception",
            json!([
                attr("exception.type", s("FromTheEvent")),
                attr("exception.stacktrace", s("only the event has this"))
            ])
        )]
    })));
    let error = carrier(&spans).error.as_ref().expect("error");
    assert_eq!(error.kind.as_deref(), Some("FromTheSpan"));
    assert_eq!(error.stack.as_deref(), Some("only the event has this"));
}

#[test]
fn a_handled_exception_does_not_invent_a_failure() {
    // OpenTelemetry is explicit that recording an exception does not fail a
    // span: an instrumentation that catches and recovers records one on a span
    // that succeeded. Deriving an error from the event would turn every handled
    // exception into a failed run, and the error rate with it.
    let spans = json_spans(&json_span(json!({
        "status": { "code": 1 },
        "events": [json_event(
            Some("1735689600900000000"),
            "exception",
            json!([attr("exception.type", s("TimeoutError"))])
        )]
    })));
    let ok = carrier(&spans);
    assert_eq!(ok.status, SpanStatus::Ok);
    assert!(ok.error.is_none(), "a recorded exception is not a failed span");
    // The record itself is still kept, where it happened.
    let events = events_of(&spans);
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].attributes.as_ref().expect("attributes")["exception.type"],
        "TimeoutError"
    );
    assert_eq!(events[0].status, SpanStatus::Unset, "an event span reports no outcome of its own");
}

#[test]
fn an_event_span_id_is_the_same_every_time_the_same_export_arrives() {
    // An exporter retries, and all three stores key a span on its id. A random
    // id here would store another copy of every event on every retry, which is
    // the defect the logs and metrics doors were fixed for.
    let body = json_span(json!({
        "events": [
            json_event(Some("1735689600500000000"), "retry", json!([])),
            json_event(Some("1735689600500000000"), "retry", json!([]))
        ]
    }));
    let first = json_spans(&body);
    let second = json_spans(&body);

    let ids_of = |spans: &[Span]| -> Vec<String> {
        events_of(spans).iter().map(|s| s.span_id.clone()).collect()
    };
    assert_eq!(ids_of(&first), ids_of(&second), "the same export produced different ids");
    // Two identical events at the same instant are two records, not one. The
    // index is in the digest for exactly this.
    assert_eq!(first.len(), 3);
    assert_ne!(ids_of(&first)[0], ids_of(&first)[1], "two events collapsed into one row");

    // And the protobuf encoding of the same export agrees, or a collector that
    // switches protocol would double every event it has already sent.
    let pb = pb_spans(&pb_span_request(
        None,
        0,
        &[
            pb_event(1_735_689_600_500_000_000, "retry", &[]),
            pb_event(1_735_689_600_500_000_000, "retry", &[]),
        ],
        &[],
        &[],
    ));
    assert_eq!(ids_of(&pb), ids_of(&first), "the two encodings gave one event two ids");
}

#[test]
fn an_event_with_no_timestamp_sits_at_the_start_of_the_span_that_carried_it() {
    // `timeUnixNano` is optional. The Unix epoch is the wrong answer: it sorts
    // the run to 1970 and stretches every waterfall drawn over it.
    let from_json = json_spans(&json_span(json!({
        "events": [json_event(None, "started", json!([]))]
    })));
    let from_pb = pb_spans(&pb_span_request(None, 0, &[pb_event(0, "started", &[])], &[], &[]));

    for (label, spans) in [("json", from_json), ("protobuf", from_pb)] {
        let events = events_of(&spans);
        assert_eq!(
            events[0].start_time.unix_timestamp_nanos(),
            1_735_689_600_000_000_000,
            "{label}: an untimed event landed at the epoch"
        );
    }
}

#[test]
fn an_event_may_name_its_own_kind_and_defaults_to_generic() {
    let spans = json_spans(&json_span(json!({
        "events": [
            json_event(Some("1"), "chunk", json!([attr("mcpd.span.kind", s("model_chunk"))])),
            json_event(Some("2"), "plain", json!([]))
        ]
    })));
    let events = events_of(&spans);
    assert_eq!(events[0].kind, SpanKind::ModelChunk);
    assert_eq!(events[1].kind, SpanKind::Generic, "an event mcpd cannot read is never guessed at");
}

#[test]
fn the_number_of_events_one_span_can_turn_into_is_bounded() {
    // An event becomes a stored span, so an unbounded list is a fan-out rather
    // than a large field. The cap matches what the OpenTelemetry SDKs limit
    // themselves to, so a conforming producer is never truncated.
    let many: Vec<Value> = (0..400)
        .map(|n| json_event(Some("1735689600500000000"), &format!("e{n}"), json!([])))
        .collect();
    let from_json = json_spans(&json_span(json!({ "events": many })));

    let encoded: Vec<Vec<u8>> =
        (0..400).map(|n| pb_event(1_735_689_600_500_000_000, &format!("e{n}"), &[])).collect();
    let from_pb = pb_spans(&pb_span_request(None, 0, &encoded, &[], &[]));

    for (label, spans) in [("json", from_json), ("protobuf", from_pb)] {
        assert_eq!(events_of(&spans).len(), 128, "{label}: the fan-out is unbounded");
        // Truncating the list must not cost the span that carried them.
        assert_eq!(carrier(&spans).name, "checkout", "{label}");
    }
}

#[test]
fn the_upstream_sampled_bit_survives_the_door_on_both_encodings() {
    // P006. The flags byte was discarded at the door, so an upstream decision
    // to sample could not beat this server's own ratio sampler, and a
    // distributed trace could be kept by one service and cut by the next.
    let from_json = json_spans(&json_span(json!({ "flags": 1 })));
    let from_pb = pb_spans(&pb_span_request(None, 1, &[], &[], &[]));

    for (label, spans) in [("json", from_json), ("protobuf", from_pb)] {
        assert_eq!(
            mcpd_core::otlp::upstream_sampled(carrier(&spans)),
            Some(true),
            "{label}: the sampled bit never reached the sampler"
        );
    }
}

#[test]
fn a_flags_field_sent_as_a_string_is_read_the_same_way() {
    // Some exporters render every integer as a string, the way OTLP/JSON
    // requires for the 64-bit ones.
    let spans = json_spans(&json_span(json!({ "flags": "769" })));
    assert_eq!(mcpd_core::otlp::upstream_sampled(carrier(&spans)), Some(true));
}

#[test]
fn a_clear_sampled_bit_is_not_a_decision_to_drop() {
    // OTLP leaves `flags` off the wire when it is zero, so "not sampled" and
    // "the producer never said" are the same bytes. Reading a clear bit as a
    // decision would throw away every span from every SDK that does not
    // populate the field, and the span is here, which means somebody exported
    // it on purpose.
    for body in [
        json_span(json!({})),
        json_span(json!({ "flags": 0 })),
        json_span(json!({
            // Only the "parent is remote" bits, with sampled clear.
            "flags": 768
        })),
    ] {
        let spans = json_spans(&body);
        assert_eq!(
            mcpd_core::otlp::upstream_sampled(carrier(&spans)),
            None,
            "a span with no sampled bit must fall through to this server's own policy"
        );
    }
    let pb = pb_spans(&pb_span_request(None, 768, &[], &[], &[]));
    assert_eq!(mcpd_core::otlp::upstream_sampled(carrier(&pb)), None);
}

#[test]
fn a_remote_parent_that_did_not_arrive_is_recorded_as_external() {
    // P012. `external_parent_span_id` records a parent from a tracing system
    // mcpd is not receiving. Both conditions are needed: the flags say another
    // process created the parent, and it is not among the spans in this
    // request either.
    let remote = 0x0000_0300; // has_is_remote | is_remote
    let from_json = json_spans(&json_span(json!({ "parentSpanId": CHILD, "flags": remote })));
    let from_pb = pb_spans(&pb_span_request(Some(&CHILD_BYTES), remote, &[], &[], &[]));

    for (label, spans) in [("json", from_json), ("protobuf", from_pb)] {
        let span = carrier(&spans);
        assert_eq!(span.external_parent_span_id.as_deref(), Some(CHILD), "{label}");
        // The tree is left exactly as it was. The field is carried for
        // correlation and never used to build mcpd's own parentage.
        assert_eq!(span.parent_span_id.as_deref(), Some(CHILD), "{label}");
        assert!(!span.is_root, "{label}");
    }
}

#[test]
fn a_parent_that_arrived_in_the_same_request_is_never_external() {
    // Two services behind one collector land in one request. mcpd holds the
    // parent, so nothing about it is foreign.
    let remote = 0x0000_0300;
    let body = json!({ "resourceSpans": [{ "scopeSpans": [{ "spans": [
        { "traceId": TRACE, "spanId": CHILD, "name": "upstream",
          "startTimeUnixNano": "1735689600000000000" },
        { "traceId": TRACE, "spanId": SPAN, "parentSpanId": CHILD, "name": "downstream",
          "startTimeUnixNano": "1735689600100000000", "flags": remote }
    ]}]}]});
    let spans = json_spans(&body);
    let child = spans.iter().find(|s| s.span_id == SPAN).expect("the child");
    assert_eq!(child.external_parent_span_id, None);
    assert_eq!(child.parent_span_id.as_deref(), Some(CHILD));
}

#[test]
fn a_parent_that_is_merely_missing_is_not_called_external() {
    // The failure this guards. A span ends after its children, so a batch
    // processor exports the children first and the parent minutes later. On
    // absence alone, almost every child in an ordinary deployment would be
    // labelled as having a foreign parent, moments before that parent arrived.
    for flags in [
        json!({}),                 // no flags at all: any SDK older than OTLP 1.1
        json!({ "flags": 1 }),     // sampled, and nothing said about the parent
        json!({ "flags": 0x100 }), // the producer states the parent is local
    ] {
        let mut extra = json!({ "parentSpanId": CHILD });
        for (k, v) in flags.as_object().expect("object") {
            extra[k.clone()] = v.clone();
        }
        let spans = json_spans(&json_span(extra));
        assert_eq!(
            carrier(&spans).external_parent_span_id,
            None,
            "a parent that has not arrived yet is not a foreign parent ({flags})"
        );
    }
}

#[test]
fn an_all_zero_span_id_is_refused_on_both_encodings() {
    // P011, from the outside. The reserved id means "no span", and a row keyed
    // on it can never be looked up or joined to anything.
    let zeros = "0".repeat(16);
    let from_json = decode_json(&json!({ "resourceSpans": [{ "scopeSpans": [{ "spans": [
        { "traceId": TRACE, "spanId": zeros, "name": "reserved",
          "startTimeUnixNano": "1735689600000000000" },
        { "traceId": TRACE, "spanId": SPAN, "name": "fine",
          "startTimeUnixNano": "1735689600000000000" }
    ]}]}]}))
    .expect("the batch still decodes");

    let mut bad = Pb::new();
    bad.bytes(1, &TRACE_BYTES).bytes(2, &[0u8; 8]).string(5, "reserved");
    let mut good = Pb::new();
    good.bytes(1, &TRACE_BYTES).bytes(2, &SPAN_BYTES).string(5, "fine");
    let from_pb = protobuf::decode_traces(&request(&[], &[bad.take(), good.take()]))
        .expect("the batch still decodes");

    for (label, decoded) in [("json", from_json), ("protobuf", from_pb)] {
        assert_eq!(decoded.len(), 1, "{label} stored a span keyed on the reserved id");
        assert_eq!(decoded[0].name, "fine", "{label} dropped the wrong span");
    }
}

#[test]
fn an_all_zero_parent_is_a_root_and_not_a_dropped_span() {
    // The same reserved id in the parent field means something different: no
    // parent. Dropping the span there would delete every root a producer that
    // pads the field sends.
    let from_json = json_spans(&json_span(json!({ "parentSpanId": "0".repeat(16) })));
    let from_pb = pb_spans(&pb_span_request(Some(&[0u8; 8]), 0, &[], &[], &[]));

    for (label, spans) in [("json", from_json), ("protobuf", from_pb)] {
        assert_eq!(spans.len(), 1, "{label} dropped a root span");
        assert!(spans[0].is_root, "{label}");
        assert_eq!(spans[0].parent_span_id, None, "{label}");
        assert_eq!(spans[0].external_parent_span_id, None, "{label}");
    }
}

#[test]
fn the_two_encodings_agree_about_a_span_carrying_everything_at_once() {
    // The module's claim, applied to the four fields added here together: one
    // span with a link, an event, flags and a remote parent must map to the
    // same mcpd spans whichever protocol carried it.
    let remote_and_sampled = 0x0000_0301;
    let from_json = json_spans(&json_span(json!({
        "parentSpanId": CHILD,
        "flags": remote_and_sampled,
        "links": [json_link(OTHER_TRACE, Some(CHILD), json!([attr("why", s("batch"))]))],
        "events": [json_event(Some("1735689600500000000"), "flush", json!([attr("n", i(7))]))]
    })));
    let from_pb = pb_spans(&pb_span_request(
        Some(&CHILD_BYTES),
        remote_and_sampled,
        &[pb_event(1_735_689_600_500_000_000, "flush", &[kv("n", &any_int(7))])],
        &[pb_link(&OTHER_TRACE_BYTES, Some(&CHILD_BYTES), &[kv("why", &any_string("batch"))])],
        &[],
    ));

    assert_eq!(from_json.len(), 2);
    assert_eq!(from_json.len(), from_pb.len(), "the two encodings produced different batches");
    for (a, b) in from_json.iter().zip(from_pb.iter()) {
        assert_eq!(a.span_id, b.span_id);
        assert_eq!(a.parent_span_id, b.parent_span_id);
        assert_eq!(a.external_parent_span_id, b.external_parent_span_id);
        assert_eq!(a.is_event, b.is_event);
        assert_eq!(a.start_time, b.start_time);
        assert_eq!(a.end_time, b.end_time);
        assert_eq!(a.links, b.links);
        assert_eq!(a.attributes, b.attributes);
    }
}

#[test]
fn an_event_or_link_that_cannot_be_read_costs_only_itself() {
    // One unreadable record inside a span must not throw away the span. The
    // whole decoder is built on that rule at the batch level, and it has to
    // hold one level down or the rule buys nothing: a span carrying a hundred
    // good events and one bad frame would be lost entirely.
    let from_json = json_spans(&json_span(json!({
        "events": ["not an event at all", json_event(Some("1735689600500000000"), "real", json!([]))],
        "links": ["not a link either", json_link(OTHER_TRACE, Some(CHILD), json!([]))]
    })));

    let mut span = Pb::new();
    span.bytes(1, &TRACE_BYTES)
        .bytes(2, &SPAN_BYTES)
        .string(5, "checkout")
        .fixed64(7, 1_735_689_600_000_000_000)
        .fixed64(8, 1_735_689_601_000_000_000)
        // A frame whose bytes are not a valid Event or Link message.
        .bytes(11, &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff])
        .bytes(11, &pb_event(1_735_689_600_500_000_000, "real", &[]))
        .bytes(13, &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff])
        .bytes(13, &pb_link(&OTHER_TRACE_BYTES, Some(&CHILD_BYTES), &[]));
    let from_pb = pb_spans(&request(&[], &[span.take()]));

    for (label, spans) in [("json", from_json), ("protobuf", from_pb)] {
        assert_eq!(carrier(&spans).name, "checkout", "{label}: the span itself was lost");
        let events = events_of(&spans);
        assert_eq!(events.len(), 1, "{label}");
        assert_eq!(events[0].name, "real", "{label}");
        let links = carrier(&spans).links.clone().unwrap_or_else(|| panic!("{label}: no links"));
        assert_eq!(links.len(), 1, "{label}");
        assert_eq!(links[0].trace_id, OTHER_TRACE, "{label}");
    }
}

#[test]
fn an_upstream_sampled_span_beats_a_policy_that_would_have_dropped_it() {
    // The expression the ingest pipeline needs, proven here because the
    // pipeline itself belongs to another file: an upstream decision to sample
    // wins, and a span that carries no decision falls through to the policy.
    // A distributed trace that one collector keeps and the next one cuts is
    // not a trace, which is why `Policy::decide` takes this argument at all.
    use mcpd_core::sampling::{Decision, Policy};
    use mcpd_core::SamplingKind;

    let policy = Policy { kind: SamplingKind::Never, ..Policy::default() };
    let sampled = json_spans(&json_span(json!({ "flags": 1 })));
    assert_eq!(
        policy.decide(TRACE, mcpd_core::otlp::upstream_sampled(carrier(&sampled))),
        Decision::Keep,
        "an upstream sampled bit must survive the door and win"
    );

    let silent = json_spans(&json_span(json!({})));
    assert_eq!(
        policy.decide(TRACE, mcpd_core::otlp::upstream_sampled(carrier(&silent))),
        Decision::Drop,
        "a span that states nothing must obey this server's own policy"
    );
}
