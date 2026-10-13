//! Streamed model calls (SDK contract 3.2).
//!
//! A streamed response spreads the facts [`Span::record_response`] reads
//! across its chunks: the id and model in the first, the text in the middle,
//! the finish reason and the usage at the end. [`Accumulator`] collects them
//! one chunk at a time and builds the response the provider would have sent
//! without streaming, so the span records exactly what it would have recorded
//! for the same call made the other way.
//!
//! Nothing here fails on a chunk it does not understand, and nothing grows
//! without bound: text and tool-call arguments share one budget, and tool
//! calls are capped by count.

use crate::span::Span;
use futures_core::Stream;
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::fmt::Display;
use std::pin::Pin;
use std::task::{Context, Poll};

/// Code points of text and tool-call arguments kept, together.
pub const STREAM_TEXT_BUDGET: usize = 32_768;

/// Tool calls kept. Later ones are ignored.
pub const STREAM_MAX_TOOL_CALLS: usize = 64;

/// The provider a stream's chunks were recognised as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    OpenAi,
    Anthropic,
}

#[derive(Debug, Default)]
struct ToolCall {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

/// What one stream has said so far.
#[derive(Debug)]
pub(crate) struct Accumulator {
    shape: Option<Shape>,
    pub(crate) truncated: bool,
    id: Option<String>,
    model: Option<String>,
    tier: Option<String>,
    text: String,
    budget: usize,
    tools: BTreeMap<u64, ToolCall>,
    finish: BTreeMap<u64, String>,
    stop: Option<String>,
    usage: Option<Map<String, Value>>,
}

impl Default for Accumulator {
    fn default() -> Self {
        Self {
            shape: None,
            truncated: false,
            id: None,
            model: None,
            tier: None,
            text: String::new(),
            budget: STREAM_TEXT_BUDGET,
            tools: BTreeMap::new(),
            finish: BTreeMap::new(),
            stop: None,
            usage: None,
        }
    }
}

/// A non-empty string, or nothing.
fn text_of(v: Option<&Value>) -> Option<&str> {
    v.and_then(Value::as_str).filter(|s| !s.is_empty())
}

/// A non-negative whole number, or nothing. `1.0` counts, as it does in the
/// languages whose JSON has one number type.
fn index_of(v: Option<&Value>) -> Option<u64> {
    let v = v?;
    v.as_u64().or_else(|| v.as_f64().filter(|f| *f >= 0.0 && f.fract() == 0.0).map(|f| f as u64))
}

/// Set `slot` from `v` unless it already holds something: the first non-empty
/// value wins.
fn first(slot: &mut Option<String>, v: Option<&str>) {
    if slot.is_none() {
        *slot = v.map(str::to_owned);
    }
}

impl Accumulator {
    /// Fold one chunk in. Answers whether it carried text or a tool-call delta.
    pub(crate) fn add(&mut self, chunk: &Value) -> bool {
        let Some(c) = chunk.as_object() else { return false };
        match c.get("choices") {
            Some(Value::Array(choices)) => return self.openai(c, choices),
            // A typed chunk with no choices can serialise them as null.
            Some(Value::Null) => return self.openai(c, &[]),
            _ => {}
        }
        match c.get("type").and_then(Value::as_str) {
            Some(
                kind @ ("message_start"
                | "content_block_start"
                | "content_block_delta"
                | "message_delta"),
            ) => self.anthropic(kind, c),
            _ => false,
        }
    }

    fn openai(&mut self, c: &Map<String, Value>, choices: &[Value]) -> bool {
        self.shape.get_or_insert(Shape::OpenAi);
        first(&mut self.id, text_of(c.get("id")));
        first(&mut self.model, text_of(c.get("model")));
        first(&mut self.tier, text_of(c.get("service_tier")));
        if let Some(usage) = c.get("usage").and_then(Value::as_object) {
            self.usage = Some(usage.clone());
        }
        let mut content = false;
        for (position, raw) in choices.iter().enumerate() {
            let Some(choice) = raw.as_object() else { continue };
            let at = index_of(choice.get("index")).unwrap_or(position as u64);
            if let Some(reason) = text_of(choice.get("finish_reason")) {
                self.finish.insert(at, reason.to_owned());
            }
            // Only the first choice is the answer; the others count for their
            // finish reasons alone.
            if at != 0 {
                continue;
            }
            let Some(delta) = choice.get("delta").and_then(Value::as_object) else { continue };
            if let Some(text) = text_of(delta.get("content")) {
                content = true;
                let kept = self.take(text);
                self.text.push_str(kept);
            }
            let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) else { continue };
            for (i, raw_call) in calls.iter().enumerate() {
                let Some(call) = raw_call.as_object() else { continue };
                content = true;
                let at = index_of(call.get("index")).unwrap_or(i as u64);
                let function = call.get("function").and_then(Value::as_object);
                let id = text_of(call.get("id"));
                let name = function.and_then(|f| text_of(f.get("name")));
                let args = function.and_then(|f| text_of(f.get("arguments")));
                self.tool_delta(at, id, name, args);
            }
        }
        content
    }

    fn anthropic(&mut self, kind: &str, c: &Map<String, Value>) -> bool {
        self.shape.get_or_insert(Shape::Anthropic);
        match kind {
            "message_start" => {
                let message = c.get("message").and_then(Value::as_object);
                first(&mut self.id, message.and_then(|m| text_of(m.get("id"))));
                first(&mut self.model, message.and_then(|m| text_of(m.get("model"))));
                let usage = message.and_then(|m| m.get("usage")).and_then(Value::as_object);
                if let Some(usage) = usage {
                    self.usage = Some(usage.clone());
                }
                false
            }
            "content_block_start" => {
                let block = c.get("content_block").and_then(Value::as_object);
                if block.and_then(|b| b.get("type")).and_then(Value::as_str) == Some("tool_use") {
                    let at = index_of(c.get("index")).unwrap_or(self.tools.len() as u64);
                    let id = block.and_then(|b| text_of(b.get("id")));
                    let name = block.and_then(|b| text_of(b.get("name")));
                    self.tool_delta(at, id, name, None);
                    return true;
                }
                let Some(text) = block.and_then(|b| text_of(b.get("text"))) else { return false };
                let kept = self.take(text);
                self.text.push_str(kept);
                true
            }
            "content_block_delta" => {
                let delta = c.get("delta").and_then(Value::as_object);
                match delta.and_then(|d| d.get("type")).and_then(Value::as_str) {
                    Some("text_delta") => {
                        let Some(text) = delta.and_then(|d| text_of(d.get("text"))) else {
                            return false;
                        };
                        let kept = self.take(text);
                        self.text.push_str(kept);
                        true
                    }
                    Some("input_json_delta") => {
                        let Some(args) = delta.and_then(|d| text_of(d.get("partial_json"))) else {
                            return false;
                        };
                        // A delta for a block that never started, or one past
                        // the cap, has nowhere to go.
                        if let Some(at) = index_of(c.get("index")) {
                            if self.tools.contains_key(&at) {
                                let kept = self.take(args).to_owned();
                                if let Some(tool) = self.tools.get_mut(&at) {
                                    tool.arguments.push_str(&kept);
                                }
                            }
                        }
                        true
                    }
                    _ => false,
                }
            }
            "message_delta" => {
                let stop = c.get("delta").and_then(|d| text_of(d.get("stop_reason")));
                if let Some(stop) = stop {
                    self.stop = Some(stop.to_owned());
                }
                // Each field the final event reports replaces the one from
                // the start; the rest of the start's usage stands.
                if let Some(usage) = c.get("usage").and_then(Value::as_object) {
                    let merged = self.usage.get_or_insert_with(Map::new);
                    for (k, v) in usage {
                        if !v.is_null() {
                            merged.insert(k.clone(), v.clone());
                        }
                    }
                }
                false
            }
            _ => false,
        }
    }

    /// Apply one tool-call delta to the call at `at`, made on first sight.
    /// Past the cap the delta is ignored and the stream marked truncated.
    fn tool_delta(&mut self, at: u64, id: Option<&str>, name: Option<&str>, args: Option<&str>) {
        if !self.tools.contains_key(&at) {
            if self.tools.len() >= STREAM_MAX_TOOL_CALLS {
                self.truncated = true;
                return;
            }
            self.tools.insert(at, ToolCall::default());
        }
        let kept = args.map(|a| self.take(a).to_owned());
        if let Some(tool) = self.tools.get_mut(&at) {
            first(&mut tool.id, id);
            first(&mut tool.name, name);
            if let Some(kept) = kept {
                tool.arguments.push_str(&kept);
            }
        }
    }

    /// As much of `s` as the budget still allows, counted in code points.
    fn take<'s>(&mut self, s: &'s str) -> &'s str {
        if self.budget == 0 {
            self.truncated = true;
            return "";
        }
        if let Some((cut, _)) = s.char_indices().nth(self.budget) {
            self.truncated = true;
            self.budget = 0;
            return &s[..cut];
        }
        self.budget -= s.chars().count();
        s
    }

    /// The response the provider would have sent, or `None` when no chunk was
    /// recognised.
    pub(crate) fn response(&self) -> Option<Value> {
        let mut out = Map::new();
        let mut put = |k: &str, v: Option<Value>| {
            if let Some(v) = v {
                out.insert(k.to_owned(), v);
            }
        };
        match self.shape? {
            Shape::OpenAi => {
                put("object", Some(Value::from("chat.completion")));
                put("id", self.id.clone().map(Value::from));
                put("model", self.model.clone().map(Value::from));
                put("service_tier", self.tier.clone().map(Value::from));
                let choices = self
                    .finish
                    .iter()
                    .map(|(i, reason)| serde_json::json!({ "index": i, "finish_reason": reason }))
                    .collect();
                put("choices", Some(Value::Array(choices)));
                put("usage", self.usage.clone().map(Value::Object));
            }
            Shape::Anthropic => {
                put("type", Some(Value::from("message")));
                put("id", self.id.clone().map(Value::from));
                put("model", self.model.clone().map(Value::from));
                put("stop_reason", self.stop.clone().map(Value::from));
                put("usage", self.usage.clone().map(Value::Object));
            }
        }
        Some(Value::Object(out))
    }

    /// The text, or the text and the tool calls, or `None` when neither
    /// arrived.
    pub(crate) fn output(&self) -> Option<Value> {
        if self.tools.is_empty() {
            return (!self.text.is_empty()).then(|| Value::from(self.text.as_str()));
        }
        let calls = self
            .tools
            .values()
            .map(|t| {
                let mut call = Map::new();
                if let Some(id) = &t.id {
                    call.insert("id".to_owned(), Value::from(id.as_str()));
                }
                if let Some(name) = &t.name {
                    call.insert("name".to_owned(), Value::from(name.as_str()));
                }
                call.insert("arguments".to_owned(), Value::from(t.arguments.as_str()));
                Value::Object(call)
            })
            .collect();
        Some(serde_json::json!({ "content": self.text, "tool_calls": Value::Array(calls) }))
    }
}

/// A provider's stream, read through a span. Made by [`Span::record_stream`].
///
/// It yields the same items as the stream it wraps, in the same order. It
/// owns the span and ends it once: when the stream ends, at the first `Err`
/// item, or when you drop it before the end.
#[must_use = "a stream does nothing unless you read it"]
pub struct RecordedStream<S> {
    inner: Pin<Box<S>>,
    span: Span,
    provider: Option<String>,
    acc: Accumulator,
    done: bool,
}

impl<S> std::fmt::Debug for RecordedStream<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecordedStream")
            .field("span", &self.span)
            .field("provider", &self.provider)
            .field("done", &self.done)
            .finish_non_exhaustive()
    }
}

impl<S> RecordedStream<S> {
    pub(crate) fn new(span: Span, stream: S, provider: Option<&str>) -> Self {
        Self {
            // Boxed so the wrapper is `Unpin` whatever the provider's stream
            // is, and reads with `StreamExt::next` without pinning by hand.
            inner: Box::pin(stream),
            span,
            provider: provider.map(str::to_owned),
            acc: Accumulator::default(),
            done: false,
        }
    }

    /// The span this stream records into, for setting anything else on it
    /// while you read.
    #[must_use]
    pub const fn span(&self) -> &Span {
        &self.span
    }

    fn finish(&mut self, error: Option<String>) {
        if self.done {
            return;
        }
        self.done = true;
        self.span.finish_stream(&self.acc, self.provider.as_deref(), error);
    }
}

impl<S, T, E> Stream for RecordedStream<S>
where
    S: Stream<Item = Result<T, E>>,
    T: Serialize,
    E: Display,
{
    type Item = Result<T, E>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        let item = match this.inner.as_mut().poll_next(cx) {
            Poll::Ready(item) => item,
            Poll::Pending => return Poll::Pending,
        };
        match &item {
            None => this.finish(None),
            Some(Err(e)) => this.finish(Some(e.to_string())),
            Some(Ok(chunk)) => {
                // Nothing to collect for a span nobody records, and nothing
                // after the span has ended.
                if !this.done && this.span.is_recording() {
                    if let Ok(value) = serde_json::to_value(chunk) {
                        if this.acc.add(&value) {
                            this.span.mark_first_token();
                        }
                    }
                }
            }
        }
        Poll::Ready(item)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<S> Drop for RecordedStream<S> {
    fn drop(&mut self) {
        // A caller that stops reading early still gets the span it opened.
        self.finish(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_budget_counts_code_points_and_cuts_at_one() {
        let mut acc = Accumulator::default();
        let big = format!("{}ab", "😀".repeat(STREAM_TEXT_BUDGET - 1));
        assert!(acc.add(&json!({ "choices": [{ "index": 0, "delta": { "content": big } }] })));
        let out = acc.output().unwrap();
        let text = out.as_str().unwrap();
        assert_eq!(text.chars().count(), STREAM_TEXT_BUDGET);
        assert!(text.ends_with("😀a"));
        assert!(acc.truncated);
    }

    #[test]
    fn text_that_fills_the_budget_exactly_is_not_truncated() {
        let mut acc = Accumulator::default();
        let exact = "x".repeat(STREAM_TEXT_BUDGET);
        acc.add(&json!({ "choices": [{ "index": 0, "delta": { "content": exact } }] }));
        assert!(!acc.truncated);
        acc.add(&json!({ "choices": [{ "index": 0, "delta": { "content": "y" } }] }));
        assert!(acc.truncated);
    }

    #[test]
    fn tool_calls_past_the_cap_are_ignored_and_marked() {
        let mut acc = Accumulator::default();
        let calls: Vec<Value> = (0..=STREAM_MAX_TOOL_CALLS)
            .map(|i| json!({ "index": i, "function": { "arguments": "1" } }))
            .collect();
        acc.add(&json!({ "choices": [{ "index": 0, "delta": { "tool_calls": calls } }] }));
        let out = acc.output().unwrap();
        assert_eq!(out["tool_calls"].as_array().unwrap().len(), STREAM_MAX_TOOL_CALLS);
        assert!(acc.truncated);
    }

    #[test]
    fn an_unknown_chunk_is_not_recognised_and_leaves_no_response() {
        let mut acc = Accumulator::default();
        assert!(!acc.add(&json!("not a chunk")));
        assert!(!acc.add(&json!({ "type": "ping" })));
        assert!(!acc.add(&json!({ "choices": "wrong type" })));
        assert!(acc.response().is_none());
        assert!(acc.output().is_none());
    }

    #[test]
    fn finish_reasons_are_kept_for_every_choice_by_index() {
        let mut acc = Accumulator::default();
        acc.add(&json!({ "choices": [
            { "index": 1, "delta": { "content": "ignored" }, "finish_reason": "length" },
            { "index": 0, "delta": { "content": "kept" }, "finish_reason": "stop" },
        ] }));
        let response = acc.response().unwrap();
        let expected = json!([
            { "index": 0, "finish_reason": "stop" },
            { "index": 1, "finish_reason": "length" },
        ]);
        assert_eq!(response["choices"], expected);
        assert_eq!(acc.output(), Some(json!("kept")));
    }
}
