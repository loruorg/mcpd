//! Short plain-text previews of payloads, for list columns.

use serde_json::Value;

/// How many characters a trace list preview holds.
pub const PREVIEW_CHARS: usize = 240;

/// A payload as one short line of plain text, for a list column.
///
/// `raw` is the stored JSON, possibly cut short by the store reading it.
/// What a person recognises a run by is chosen when the JSON parses: the text
/// of a string, the last user message of a chat, or the value under a key
/// such as `input`, `question` or `query`. A payload cut mid-way is read as
/// text with the JSON punctuation taken out. `None` for an empty payload.
#[must_use]
pub fn payload_preview(raw: &str) -> Option<String> {
    let text = match serde_json::from_str::<Value>(raw) {
        Ok(v) => preview_text(&v),
        Err(_) => raw
            .chars()
            .filter(|c| !matches!(c, '"' | '\\'))
            .map(|c| if matches!(c, '{' | '}' | '[' | ']') { ' ' } else { c })
            .collect(),
    };
    shorten(&text)
}

/// A payload already parsed, as one short line of plain text.
#[must_use]
pub fn value_preview(v: &Value) -> Option<String> {
    shorten(&preview_text(v))
}

/// Collapse whitespace and cut to [`PREVIEW_CHARS`], marking a cut.
fn shorten(text: &str) -> Option<String> {
    let mut out = String::with_capacity(PREVIEW_CHARS + 3);
    let mut count = 0usize;
    for word in text.split_whitespace() {
        if count > 0 {
            out.push(' ');
            count += 1;
        }
        for c in word.chars() {
            if count >= PREVIEW_CHARS {
                out.push('…');
                return Some(out);
            }
            out.push(c);
            count += 1;
        }
    }
    (!out.is_empty()).then_some(out)
}

fn preview_text(v: &Value) -> String {
    const KEYS: [&str; 9] =
        ["input", "question", "query", "prompt", "message", "text", "content", "output", "answer"];
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Array(items) => {
            // A chat: the last thing the user said is what the run was about.
            let is_chat = !items.is_empty() && items.iter().all(|m| m.get("role").is_some());
            if is_chat {
                let pick = items
                    .iter()
                    .rev()
                    .find(|m| m.get("role").and_then(Value::as_str) == Some("user"))
                    .or_else(|| items.last());
                return pick.and_then(|m| m.get("content")).map(preview_text).unwrap_or_default();
            }
            items.iter().map(preview_text).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ")
        }
        Value::Object(map) => {
            if let Some(msgs) = map.get("messages").filter(|m| m.is_array()) {
                return preview_text(msgs);
            }
            if let Some(found) = KEYS.iter().find_map(|k| map.get(*k)) {
                return preview_text(found);
            }
            // A content part, `{"type": "text", "text": ...}`, is caught above.
            map.values().map(preview_text).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ")
        }
    }
}
