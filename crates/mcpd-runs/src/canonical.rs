//! Canonical JSON, and the hash section 4 of the contract is defined over.
//!
//! `serde_json` sorts object keys only when its `preserve_order` feature is
//! off, and a feature is unified across the whole workspace: one crate turning
//! it on would silently change every hash this crate has ever written. So the
//! sort is done here, by hand, and does not depend on a feature nobody in this
//! crate controls.

use serde_json::Value;
use sha2::Digest;
use std::collections::BTreeMap;

/// Serialise with sorted keys and no insignificant whitespace.
#[must_use]
pub fn to_string(value: &Value) -> String {
    sorted(value).to_string()
}

fn sorted(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let ordered: BTreeMap<&String, Value> =
                map.iter().map(|(k, v)| (k, sorted(v))).collect();
            let mut out = serde_json::Map::new();
            for (k, v) in ordered {
                out.insert(k.clone(), v);
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(sorted).collect()),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => value.clone(),
    }
}

/// The canonical body of a definition document: sorted keys, no whitespace,
/// minus a top-level `version`, so naming a version does not change the
/// identity of the body it names.
#[must_use]
pub fn canonical_body(body: &Value) -> String {
    match body {
        Value::Object(map) => {
            let mut trimmed = map.clone();
            trimmed.remove("version");
            to_string(&Value::Object(trimmed))
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_) => {
            to_string(body)
        }
    }
}

/// `sha256` of the canonical body, as lowercase hex.
#[must_use]
pub fn body_hash(body: &Value) -> String {
    hex::encode(sha2::Sha256::digest(canonical_body(body).as_bytes()))
}

/// `sha256` of arbitrary text, as lowercase hex. The lease's idempotency key
/// is this over `run_id:step_key`.
#[must_use]
pub fn sha256_hex(text: &str) -> String {
    hex::encode(sha2::Sha256::digest(text.as_bytes()))
}
