//! Model calls: what an OpenAI- or Anthropic-shaped response says about the
//! call that produced it, mapped onto mcpd's usage record (SDK contract 3.1).
//!
//! Nothing here depends on a provider SDK. The response is read as a
//! [`serde_json::Value`], so a parsed body, a provider type serialised with
//! `serde_json::to_value` and a hand-built test fixture all work, and a field
//! that is missing or of the wrong type is left absent rather than reported as
//! an error.

use mcpd_core::{InputTokenDetails, OutputTokenDetails, TokenUsage};
use serde_json::{Map, Value};

/// The value as an object, if it is one.
fn obj(v: Option<&Value>) -> Option<&Map<String, Value>> {
    v.and_then(Value::as_object)
}

/// A token count: a non-negative finite number, rounded down.
fn int(v: Option<&Value>) -> Option<u64> {
    let v = v?;
    if let Some(n) = v.as_u64() {
        return Some(n);
    }
    // A float count is not what the providers send, but a proxy that
    // re-encodes JSON may turn 12 into 12.0; reading it costs nothing.
    let f = v.as_f64()?;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    (f.is_finite() && f >= 0.0).then(|| f.floor() as u64)
}

/// A non-empty string.
fn text(v: Option<&Value>) -> Option<&str> {
    v.and_then(Value::as_str).filter(|s| !s.is_empty())
}

/// `None` for a details record with nothing set, so an empty object never
/// reaches the wire.
fn some_input(d: InputTokenDetails) -> Option<InputTokenDetails> {
    (d != InputTokenDetails::default()).then_some(d)
}

fn some_output(d: OutputTokenDetails) -> Option<OutputTokenDetails> {
    (d != OutputTokenDetails::default()).then_some(d)
}

fn some_usage(u: TokenUsage) -> Option<TokenUsage> {
    (u != TokenUsage::default()).then_some(u)
}

/// Token usage from an OpenAI chat-completions or responses-API response.
///
/// The prompt total is gross, so the plain text share is the total less the
/// cached and audio tokens.
#[must_use]
pub fn usage_from_openai(response: &Value) -> Option<TokenUsage> {
    let usage = obj(response.get("usage"))?;
    let input = int(usage.get("prompt_tokens")).or_else(|| int(usage.get("input_tokens")));
    let output = int(usage.get("completion_tokens")).or_else(|| int(usage.get("output_tokens")));
    let in_detail =
        obj(usage.get("prompt_tokens_details")).or_else(|| obj(usage.get("input_tokens_details")));
    let out_detail = obj(usage.get("completion_tokens_details"))
        .or_else(|| obj(usage.get("output_tokens_details")));
    let cached = in_detail.and_then(|d| int(d.get("cached_tokens")));
    let audio_in = in_detail.and_then(|d| int(d.get("audio_tokens")));
    let text_share = match (input, cached.is_some() || audio_in.is_some()) {
        (Some(total), true) => {
            Some(total.saturating_sub(cached.unwrap_or(0)).saturating_sub(audio_in.unwrap_or(0)))
        }
        _ => None,
    };
    some_usage(TokenUsage {
        input_tokens: input,
        output_tokens: output,
        total_tokens: int(usage.get("total_tokens"))
            .or_else(|| input.zip(output).map(|(i, o)| i.saturating_add(o))),
        input_details: some_input(InputTokenDetails {
            text: text_share,
            cache_read: cached,
            audio: audio_in,
            ..InputTokenDetails::default()
        }),
        output_details: some_output(OutputTokenDetails {
            reasoning: out_detail.and_then(|d| int(d.get("reasoning_tokens"))),
            audio: out_detail.and_then(|d| int(d.get("audio_tokens"))),
            ..OutputTokenDetails::default()
        }),
    })
}

/// Token usage from an Anthropic messages response.
///
/// `input_tokens` there is net of the cache, so it is the text share, and the
/// prompt's size is the sum of it and both cache meters.
#[must_use]
pub fn usage_from_anthropic(response: &Value) -> Option<TokenUsage> {
    let usage = obj(response.get("usage"))?;
    let text_share = int(usage.get("input_tokens"));
    let read = int(usage.get("cache_read_input_tokens"));
    let write = int(usage.get("cache_creation_input_tokens"));
    let split = obj(usage.get("cache_creation"));
    let output = int(usage.get("output_tokens"));
    let input = (text_share.is_some() || read.is_some() || write.is_some()).then(|| {
        text_share.unwrap_or(0).saturating_add(read.unwrap_or(0)).saturating_add(write.unwrap_or(0))
    });
    some_usage(TokenUsage {
        input_tokens: input,
        output_tokens: output,
        total_tokens: input.zip(output).map(|(i, o)| i.saturating_add(o)),
        input_details: some_input(InputTokenDetails {
            text: text_share,
            cache_read: read,
            cache_write: write,
            cache_write_5m: split.and_then(|s| int(s.get("ephemeral_5m_input_tokens"))),
            cache_write_1h: split.and_then(|s| int(s.get("ephemeral_1h_input_tokens"))),
            ..InputTokenDetails::default()
        }),
        output_details: None,
    })
}

/// Which provider shaped this response: `"openai"`, `"anthropic"`, or `None`
/// when neither did.
#[must_use]
pub fn response_shape(response: &Value) -> Option<&'static str> {
    let r = response.as_object()?;
    let usage = obj(r.get("usage"));
    let has = |key: &str| usage.is_some_and(|u| u.contains_key(key));
    if r.get("type").and_then(Value::as_str) == Some("message") || has("cache_read_input_tokens") {
        return Some("anthropic");
    }
    if has("prompt_tokens") || has("input_tokens_details") {
        return Some("openai");
    }
    if r.get("choices").is_some_and(Value::is_array)
        || r.get("object").and_then(Value::as_str) == Some("response")
    {
        return Some("openai");
    }
    if has("input_tokens") {
        return Some("anthropic");
    }
    None
}

/// What [`Span::record_response`](crate::Span::record_response) sets on a
/// span.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResponseFacts {
    /// The model the response names.
    pub model: Option<String>,
    /// The provider given, or else the one the shape implies.
    pub provider: Option<String>,
    /// The token usage, with its cache and reasoning meters.
    pub usage: Option<TokenUsage>,
    /// `gen_ai.response.id`, `gen_ai.response.finish_reasons` and
    /// `gen_ai.response.service_tier`, each only when present.
    pub attributes: Map<String, Value>,
}

/// Everything a response says about its call: the model, the provider, the
/// usage, and the id, finish reasons and service tier as `gen_ai.response.*`
/// attributes. Never the text: that stays the caller's to record or not.
#[must_use]
pub fn response_facts(response: &Value, provider: Option<&str>) -> ResponseFacts {
    let empty = Map::new();
    let r = response.as_object().unwrap_or(&empty);
    let shape = response_shape(response);
    let mut attributes = Map::new();
    if let Some(id) = text(r.get("id")) {
        attributes.insert("gen_ai.response.id".to_owned(), Value::from(id));
    }
    let mut reasons: Vec<Value> = Vec::new();
    if let Some(choices) = r.get("choices").and_then(Value::as_array) {
        reasons
            .extend(choices.iter().filter_map(|c| text(c.get("finish_reason"))).map(Value::from));
    }
    if let Some(stop) = text(r.get("stop_reason")) {
        reasons.push(Value::from(stop));
    }
    if reasons.is_empty() && r.get("object").and_then(Value::as_str) == Some("response") {
        if let Some(status) = text(r.get("status")) {
            reasons.push(Value::from(status));
        }
    }
    if !reasons.is_empty() {
        attributes.insert("gen_ai.response.finish_reasons".to_owned(), Value::Array(reasons));
    }
    let tier = text(r.get("service_tier"))
        .or_else(|| obj(r.get("usage")).and_then(|u| text(u.get("service_tier"))));
    if let Some(tier) = tier {
        attributes.insert("gen_ai.response.service_tier".to_owned(), Value::from(tier));
    }
    let usage = match shape {
        Some("anthropic") => usage_from_anthropic(response),
        Some("openai") => usage_from_openai(response),
        _ => None,
    };
    ResponseFacts {
        model: text(r.get("model")).map(str::to_owned),
        provider: provider.map(str::to_owned).or_else(|| shape.map(str::to_owned)),
        usage,
        attributes,
    }
}
