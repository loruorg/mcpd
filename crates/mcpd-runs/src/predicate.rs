//! Predicates: section 3.2 of the run contract.
//!
//! A predicate is the guard on a node's `when` and the exit test on a loop's
//! `until`. It is data and not code because the daemon evaluates it inside the
//! transaction that moves the run, and the daemon never executes user code.
//!
//! Evaluation cannot fail. A comparison between unlike JSON types is false, a
//! path that names nothing is null, and a pattern that does not compile is
//! false. An error at that point would be a run stuck at a guard with no
//! attempt row to explain why. What can be refused is refused before the
//! document is stored, by [`Predicate::validate`], so a guard that is spelled
//! wrong is a 400 at write time and not a run that skips its guard.

use crate::template;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::cmp::Ordering;

/// The operators a leaf predicate may apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Op {
    /// Equal, by JSON value.
    Eq,
    /// The same kind of JSON value and not equal. Unlike kinds compare false
    /// here as for every other operator; `{ "not": { "op": "eq" } }` is the
    /// test that is also true across kinds.
    Ne,
    /// Less than. Numbers by numeric order, strings by code point order.
    Lt,
    /// Less than or equal.
    Lte,
    /// Greater than.
    Gt,
    /// Greater than or equal.
    Gte,
    /// Equal to any element of an array value.
    In,
    /// The path names a value that is not null. Takes no value.
    Exists,
    /// A string matched by the regular expression the value holds.
    Matches,
}

impl Op {
    /// The wire spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Op::Eq => "eq",
            Op::Ne => "ne",
            Op::Lt => "lt",
            Op::Lte => "lte",
            Op::Gt => "gt",
            Op::Gte => "gte",
            Op::In => "in",
            Op::Exists => "exists",
            Op::Matches => "matches",
        }
    }
}

/// A guard, as section 3.2 writes it.
///
/// One of four shapes: a leaf `{ "path", "op", "value" }`, `{ "all": [...] }`,
/// `{ "any": [...] }` or `{ "not": ... }`. Deserialisation refuses anything
/// but an object, an unknown field, a mixture of shapes and a leaf missing
/// `path` or `op`, each with a message that names what to write instead. The
/// rules that depend on the operator (which ops take a value, of what kind,
/// whether a pattern compiles, whether a combinator is empty) are
/// [`Predicate::validate`]'s, and the write-time validator must call it: a
/// predicate that deserialised is well-shaped, not yet well-formed.
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(untagged, try_from = "Value")]
pub enum Predicate {
    /// One comparison.
    Leaf {
        /// What to read, in the path grammar of section 3.3.
        path: String,
        /// How to compare it.
        op: Op,
        /// What to compare it with. Absent for `exists`; a JSON null here is a
        /// value, distinct from absence, so `eq null` can be written.
        #[serde(skip_serializing_if = "Option::is_none")]
        value: Option<Value>,
    },
    /// True when every member is true. Of nothing, true: the validator
    /// refuses an empty list because a guard that always passes is a mistake.
    All {
        /// The members.
        all: Vec<Predicate>,
    },
    /// True when at least one member is true. Of nothing, false: the validator
    /// refuses an empty list because a guard that never passes is a mistake.
    Any {
        /// The members.
        any: Vec<Predicate>,
    },
    /// The negation of its member.
    Not {
        /// The member.
        not: Box<Predicate>,
    },
}

/// Every field any shape may carry, so that one `deny_unknown_fields` names
/// the whole vocabulary in its error and the shape rules below can produce
/// their own messages instead of serde's "did not match any variant".
///
/// It is read from a [`Value`] that [`TryFrom`] has already checked is an
/// object. A derived struct otherwise also accepts a JSON array as positional
/// fields, and `["input.amount", "eq", 240]` is not a predicate anyone wrote
/// on purpose.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    op: Option<Op>,
    #[serde(default, deserialize_with = "present")]
    value: Option<Value>,
    #[serde(default)]
    all: Option<Vec<Predicate>>,
    #[serde(default)]
    any: Option<Vec<Predicate>>,
    #[serde(default)]
    not: Option<Box<Predicate>>,
}

/// `Some` for any value that was written, including null. `Option<Value>` on
/// its own reads a written null as absence, which would turn `eq null` into a
/// leaf with no value.
fn present<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(d).map(Some)
}

impl TryFrom<Value> for Predicate {
    type Error = String;

    fn try_from(value: Value) -> Result<Self, String> {
        if !value.is_object() {
            return Err(format!(
                "a predicate is an object, {{path, op, value}}, {{all: [...]}}, {{any: [...]}} \
                 or {{not: ...}}; got {}",
                kind(&value)
            ));
        }
        let Raw { path, op, value, all, any, not } =
            Raw::deserialize(value).map_err(|e| e.to_string())?;
        let leaf = path.is_some() || op.is_some() || value.is_some();
        let shapes: Vec<&str> = [
            (leaf, "a comparison"),
            (all.is_some(), "all"),
            (any.is_some(), "any"),
            (not.is_some(), "not"),
        ]
        .into_iter()
        .filter(|(present, _)| *present)
        .map(|(_, name)| name)
        .collect();
        if shapes.len() > 1 {
            return Err(format!(
                "a predicate is one shape, {{path, op, value}}, {{all}}, {{any}} or {{not}}; \
                 this one mixes {}: split it, or nest one inside the other",
                shapes.join(" and ")
            ));
        }
        if let Some(all) = all {
            return Ok(Predicate::All { all });
        }
        if let Some(any) = any {
            return Ok(Predicate::Any { any });
        }
        if let Some(not) = not {
            return Ok(Predicate::Not { not });
        }
        match (path, op) {
            (Some(path), Some(op)) => Ok(Predicate::Leaf { path, op, value }),
            (Some(_), None) => Err(
                "a comparison needs \"op\": one of eq, ne, lt, lte, gt, gte, in, exists, matches"
                    .to_string(),
            ),
            (None, Some(_)) => Err("a comparison needs \"path\": what to read, such as \
                                    \"steps.order.output.amount_usd\""
                .to_string()),
            (None, None) if value.is_some() => {
                Err("a value on its own is not a predicate: add \"path\" and \"op\"".to_string())
            }
            (None, None) => Err("an empty object is not a predicate: write {path, op, value}, \
                                 {all: [...]}, {any: [...]} or {not: ...}"
                .to_string()),
        }
    }
}

/// The ceiling on a compiled pattern's size.
///
/// A guard is compiled on every evaluation, on the daemon's tick, inside a
/// transaction. The regex crate's default admits a pattern that costs
/// megabytes to compile; refusing those at write time, and treating them as
/// false at run time, keeps a definition from taxing every run in the daemon.
const REGEX_SIZE_LIMIT: usize = 1 << 20;

/// Compile a pattern under the same limits at write time and run time, so
/// what validate accepts is what eval can match.
fn compile(pattern: &str) -> Result<regex::Regex, regex::Error> {
    regex::RegexBuilder::new(pattern).size_limit(REGEX_SIZE_LIMIT).build()
}

/// A JSON kind, for messages and for the like-kind rule.
fn kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a bool",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

/// Numeric order that keeps integer equality exact.
///
/// Two integers compare as integers, so values past 2^53 that differ in their
/// low bits are not made equal by a detour through f64. Anything else compares
/// as f64, which is what a JSON `240` against a JSON `240.0` needs.
fn compare_numbers(a: &serde_json::Number, b: &serde_json::Number) -> Option<Ordering> {
    if let (Some(x), Some(y)) = (a.as_i64(), b.as_i64()) {
        return Some(x.cmp(&y));
    }
    if let (Some(x), Some(y)) = (a.as_u64(), b.as_u64()) {
        return Some(x.cmp(&y));
    }
    a.as_f64()?.partial_cmp(&b.as_f64()?)
}

/// Equality by value, with the numeric rule above applied at every depth.
/// `serde_json`'s own `==` holds `1` and `1.0` apart, which a guard author
/// would not expect.
fn equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => compare_numbers(x, y) == Some(Ordering::Equal),
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| equal(p, q))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, p)| y.get(k).is_some_and(|q| equal(p, q)))
        }
        (Value::Null, Value::Null) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::String(x), Value::String(y)) => x == y,
        _ => false,
    }
}

/// Order for `lt` and its siblings: numbers and strings only.
fn order(a: &Value, b: &Value) -> Option<Ordering> {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => compare_numbers(x, y),
        (Value::String(x), Value::String(y)) => Some(x.cmp(y)),
        _ => None,
    }
}

impl Predicate {
    /// Evaluate against the context a node sees, per section 3.2.
    ///
    /// Never errors and never panics. A path that names nothing reads as null;
    /// a comparison across unlike JSON kinds is false; `in` against anything
    /// but an array is false; `matches` against a non-string, or with a
    /// pattern that does not compile, is false. The pattern is compiled on
    /// each evaluation: a guard runs once per visit and holding a compiled
    /// regex per definition version would be a cache with no eviction rule.
    #[must_use]
    pub fn eval(&self, ctx: &Value) -> bool {
        match self {
            Predicate::All { all } => all.iter().all(|p| p.eval(ctx)),
            Predicate::Any { any } => any.iter().any(|p| p.eval(ctx)),
            Predicate::Not { not } => !not.eval(ctx),
            Predicate::Leaf { path, op, value } => {
                let actual = template::resolve(path, ctx);
                if *op == Op::Exists {
                    return !actual.is_null();
                }
                let Some(expected) = value else {
                    return false;
                };
                match op {
                    Op::Eq => equal(&actual, expected),
                    Op::Ne => kind(&actual) == kind(expected) && !equal(&actual, expected),
                    Op::Lt => order(&actual, expected) == Some(Ordering::Less),
                    Op::Lte => {
                        matches!(order(&actual, expected), Some(Ordering::Less | Ordering::Equal))
                    }
                    Op::Gt => order(&actual, expected) == Some(Ordering::Greater),
                    Op::Gte => matches!(
                        order(&actual, expected),
                        Some(Ordering::Greater | Ordering::Equal)
                    ),
                    Op::In => match expected {
                        Value::Array(items) => items.iter().any(|item| equal(&actual, item)),
                        _ => false,
                    },
                    Op::Matches => match (&actual, expected) {
                        (Value::String(text), Value::String(pattern)) => {
                            compile(pattern).is_ok_and(|re| re.is_match(text))
                        }
                        _ => false,
                    },
                    Op::Exists => unreachable!("handled before the value is required"),
                }
            }
        }
    }

    /// Refuse a predicate that could never be meant.
    ///
    /// # Errors
    ///
    /// The first problem found, worded for the author: a path that is not a
    /// path; an op that needs a value and has none; `exists` with a value;
    /// `in` with a value that is not an array; `matches` with a value that is
    /// not a string, or a pattern the engine refuses; an ordering op against a
    /// value that is not a number or a string, since nothing could ever order
    /// against it; and an empty `all` or `any`, since a guard that is always
    /// true or never true is not a guard.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Predicate::All { all } if all.is_empty() => Err("\"all\" needs at least one \
                 predicate: an empty all is always true, so remove the guard or fill it"
                .to_string()),
            Predicate::All { all } => all.iter().try_for_each(Predicate::validate),
            Predicate::Any { any } if any.is_empty() => Err("\"any\" needs at least one \
                 predicate: an empty any is never true, so the node it guards would never run"
                .to_string()),
            Predicate::Any { any } => any.iter().try_for_each(Predicate::validate),
            Predicate::Not { not } => not.validate(),
            Predicate::Leaf { path, op, value } => {
                if !template::is_valid_path(path) {
                    return Err(format!(
                        "\"path\" {path:?} is not a path: write a name followed by .name or \
                         [n] steps, such as steps.order.output.amount_usd"
                    ));
                }
                match (op, value) {
                    (Op::Exists, Some(_)) => Err("op exists takes no value: remove \
                         \"value\", or use eq to compare against one"
                        .to_string()),
                    (Op::Exists, None) => Ok(()),
                    (op, None) => Err(format!("op {} needs a value: add \"value\"", op.as_str())),
                    (Op::In, Some(v)) if !v.is_array() => Err(format!(
                        "op in needs an array value, got {}: write \"value\": [...] listing \
                         what the path may equal",
                        kind(v)
                    )),
                    (Op::Matches, Some(Value::String(pattern))) => compile(pattern)
                        .map(|_| ())
                        .map_err(|e| format!("op matches has a pattern the engine refuses: {e}")),
                    (Op::Matches, Some(v)) => Err(format!(
                        "op matches needs a string value holding the regular expression, got {}",
                        kind(v)
                    )),
                    (Op::Lt | Op::Lte | Op::Gt | Op::Gte, Some(v))
                        if !(v.is_number() || v.is_string()) =>
                    {
                        Err(format!(
                            "op {} orders numbers and strings, and {} can never match: compare \
                             against a number or a string",
                            op.as_str(),
                            kind(v)
                        ))
                    }
                    (Op::Eq | Op::Ne | Op::Lt | Op::Lte | Op::Gt | Op::Gte | Op::In, Some(_)) => {
                        Ok(())
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx() -> Value {
        json!({
            "input": {
                "order_id": "ord_913",
                "amount": 240,
                "ratio": 2.5,
                "big": 9_007_199_254_740_993_u64,
                "approved": true,
                "nothing": null,
                "tags": ["a", "b"],
                "order": { "sku": "A1", "qty": 2 }
            },
            "steps": {
                "order": { "status": "succeeded", "output": { "amount_usd": 240 } },
                "small_refund": { "status": "failed", "error": { "class": "retryable", "message": "429" } },
                "approval": { "status": "skipped" }
            }
        })
    }

    fn parse(text: &str) -> Predicate {
        serde_json::from_str(text).unwrap()
    }

    fn holds(text: &str) -> bool {
        parse(text).eval(&ctx())
    }

    fn refused(text: &str) -> String {
        serde_json::from_str::<Predicate>(text).unwrap_err().to_string()
    }

    // Every op.

    #[test]
    fn eq() {
        assert!(holds(r#"{ "path": "input.amount", "op": "eq", "value": 240 }"#));
        assert!(holds(r#"{ "path": "input.amount", "op": "eq", "value": 240.0 }"#));
        assert!(!holds(r#"{ "path": "input.amount", "op": "eq", "value": 241 }"#));
        assert!(holds(r#"{ "path": "input.order_id", "op": "eq", "value": "ord_913" }"#));
        assert!(holds(r#"{ "path": "input.approved", "op": "eq", "value": true }"#));
        assert!(holds(r#"{ "path": "input.tags", "op": "eq", "value": ["a", "b"] }"#));
        assert!(!holds(r#"{ "path": "input.tags", "op": "eq", "value": ["b", "a"] }"#));
        assert!(holds(
            r#"{ "path": "input.order", "op": "eq", "value": { "qty": 2.0, "sku": "A1" } }"#
        ));
        assert!(!holds(r#"{ "path": "input.order", "op": "eq", "value": { "sku": "A1" } }"#));
        assert!(holds(r#"{ "path": "steps.approval.status", "op": "eq", "value": "skipped" }"#));
    }

    #[test]
    fn eq_null_tests_absence_and_null_alike() {
        assert!(holds(r#"{ "path": "input.nothing", "op": "eq", "value": null }"#));
        assert!(holds(r#"{ "path": "input.missing", "op": "eq", "value": null }"#));
        assert!(!holds(r#"{ "path": "input.amount", "op": "eq", "value": null }"#));
    }

    #[test]
    fn ne() {
        assert!(holds(r#"{ "path": "input.amount", "op": "ne", "value": 241 }"#));
        assert!(!holds(r#"{ "path": "input.amount", "op": "ne", "value": 240 }"#));
        assert!(!holds(r#"{ "path": "input.amount", "op": "ne", "value": 240.0 }"#));
        assert!(holds(
            r#"{ "path": "steps.small_refund.status", "op": "ne", "value": "succeeded" }"#
        ));
        assert!(holds(r#"{ "path": "input.tags", "op": "ne", "value": ["b", "a"] }"#));
        assert!(!holds(r#"{ "path": "input.nothing", "op": "ne", "value": null }"#));
    }

    #[test]
    fn ordering_on_numbers() {
        assert!(holds(r#"{ "path": "input.amount", "op": "lt", "value": 241 }"#));
        assert!(!holds(r#"{ "path": "input.amount", "op": "lt", "value": 240 }"#));
        assert!(holds(r#"{ "path": "input.amount", "op": "lte", "value": 240 }"#));
        assert!(!holds(r#"{ "path": "input.amount", "op": "lte", "value": 239.9 }"#));
        assert!(holds(r#"{ "path": "input.amount", "op": "gt", "value": 100 }"#));
        assert!(!holds(r#"{ "path": "input.amount", "op": "gt", "value": 240 }"#));
        assert!(holds(r#"{ "path": "input.amount", "op": "gte", "value": 240.0 }"#));
        assert!(!holds(r#"{ "path": "input.amount", "op": "gte", "value": 240.5 }"#));
        assert!(holds(r#"{ "path": "input.ratio", "op": "gt", "value": 2 }"#));
        assert!(holds(r#"{ "path": "input.ratio", "op": "lt", "value": 3 }"#));
        assert!(holds(r#"{ "path": "steps.order.output.amount_usd", "op": "gt", "value": 100 }"#));
        assert!(holds(r#"{ "path": "input.amount", "op": "gt", "value": -1 }"#));
    }

    #[test]
    fn ordering_on_strings_is_by_code_point() {
        assert!(holds(r#"{ "path": "input.order_id", "op": "gt", "value": "ord_000" }"#));
        assert!(holds(r#"{ "path": "input.order_id", "op": "lt", "value": "ord_999" }"#));
        assert!(holds(r#"{ "path": "input.order_id", "op": "gte", "value": "ord_913" }"#));
        assert!(holds(r#"{ "path": "input.order_id", "op": "lte", "value": "ord_913" }"#));
        assert!(!holds(r#"{ "path": "input.order_id", "op": "lt", "value": "ord_913" }"#));
        // A timestamp in the contract's own format orders correctly as text.
        let c = json!({ "input": { "at": "2026-09-01T12:00:00Z" } });
        let p = parse(r#"{ "path": "input.at", "op": "lt", "value": "2026-09-02T00:00:00Z" }"#);
        assert!(p.eval(&c));
    }

    #[test]
    fn integer_equality_survives_past_the_f64_mantissa() {
        // 2^53 + 1 and 2^53 + 2 are one f64 apart from each other only as
        // integers; as f64 they round to the same value.
        assert!(holds(r#"{ "path": "input.big", "op": "eq", "value": 9007199254740993 }"#));
        assert!(!holds(r#"{ "path": "input.big", "op": "eq", "value": 9007199254740994 }"#));
        assert!(holds(r#"{ "path": "input.big", "op": "ne", "value": 9007199254740994 }"#));
        assert!(holds(r#"{ "path": "input.big", "op": "lt", "value": 9007199254740994 }"#));
        assert!(holds(r#"{ "path": "input.big", "op": "gt", "value": 9007199254740992 }"#));
        // Negative integers take the i64 path; the same pair, negated.
        let c = json!({ "n": -9_007_199_254_740_993_i64 });
        assert!(parse(r#"{ "path": "n", "op": "eq", "value": -9007199254740993 }"#).eval(&c));
        assert!(!parse(r#"{ "path": "n", "op": "eq", "value": -9007199254740992 }"#).eval(&c));
        assert!(parse(r#"{ "path": "n", "op": "lt", "value": -9007199254740992 }"#).eval(&c));
        // A u64 past i64::MAX against an i64 still orders as numbers.
        let c = json!({ "n": u64::MAX });
        assert!(parse(r#"{ "path": "n", "op": "gt", "value": 1 }"#).eval(&c));
        assert!(parse(r#"{ "path": "n", "op": "gt", "value": -1 }"#).eval(&c));
        assert!(parse(r#"{ "path": "n", "op": "ne", "value": 1 }"#).eval(&c));
    }

    #[test]
    fn r#in() {
        assert!(holds(
            r#"{ "path": "steps.small_refund.status", "op": "in", "value": ["failed", "timed_out"] }"#
        ));
        assert!(!holds(
            r#"{ "path": "steps.small_refund.status", "op": "in", "value": ["succeeded"] }"#
        ));
        assert!(holds(r#"{ "path": "input.amount", "op": "in", "value": [1, 240.0, 3] }"#));
        assert!(holds(r#"{ "path": "input.tags", "op": "in", "value": [["a", "b"]] }"#));
        assert!(holds(r#"{ "path": "input.missing", "op": "in", "value": [null] }"#));
        assert!(!holds(r#"{ "path": "input.amount", "op": "in", "value": [] }"#));
        // Not an array: false, never an error.
        assert!(!holds(r#"{ "path": "input.amount", "op": "in", "value": 240 }"#));
        assert!(!holds(r#"{ "path": "input.order_id", "op": "in", "value": "ord_913" }"#));
    }

    #[test]
    fn exists() {
        assert!(holds(r#"{ "path": "input.amount", "op": "exists" }"#));
        assert!(holds(r#"{ "path": "input.approved", "op": "exists" }"#));
        assert!(holds(r#"{ "path": "steps.approval", "op": "exists" }"#));
        assert!(!holds(r#"{ "path": "input.nothing", "op": "exists" }"#));
        assert!(!holds(r#"{ "path": "input.missing", "op": "exists" }"#));
        assert!(!holds(r#"{ "path": "steps.reply.output", "op": "exists" }"#));
        // false is a value that exists.
        let c = json!({ "input": { "flag": false, "zero": 0, "empty": "" } });
        assert!(parse(r#"{ "path": "input.flag", "op": "exists" }"#).eval(&c));
        assert!(parse(r#"{ "path": "input.zero", "op": "exists" }"#).eval(&c));
        assert!(parse(r#"{ "path": "input.empty", "op": "exists" }"#).eval(&c));
        // A value on an exists leaf is ignored at eval; validate refuses it.
        assert!(holds(r#"{ "path": "input.amount", "op": "exists", "value": 1 }"#));
    }

    #[test]
    fn matches() {
        assert!(holds(r#"{ "path": "input.order_id", "op": "matches", "value": "^ord_[0-9]+$" }"#));
        assert!(holds(r#"{ "path": "input.order_id", "op": "matches", "value": "913" }"#));
        assert!(!holds(r#"{ "path": "input.order_id", "op": "matches", "value": "^913" }"#));
        assert!(holds(
            r#"{ "path": "steps.small_refund.error.message", "op": "matches", "value": "^4[0-9]{2}$" }"#
        ));
        // A number is not matched, even when its digits would be.
        assert!(!holds(r#"{ "path": "input.amount", "op": "matches", "value": "240" }"#));
        assert!(!holds(r#"{ "path": "input.missing", "op": "matches", "value": ".*" }"#));
        // A bad pattern is false, never an error.
        assert!(!holds(r#"{ "path": "input.order_id", "op": "matches", "value": "(" }"#));
        assert!(!holds(r#"{ "path": "input.order_id", "op": "matches", "value": "[" }"#));
        // A non-string pattern is false.
        assert!(!holds(r#"{ "path": "input.order_id", "op": "matches", "value": 913 }"#));
        assert!(!holds(r#"{ "path": "input.order_id", "op": "matches", "value": ["ord"] }"#));
    }

    #[test]
    fn a_pattern_past_the_size_limit_is_false_and_refused() {
        let text = format!(
            r#"{{ "path": "input.order_id", "op": "matches", "value": "{}" }}"#,
            "(a{1000}){1000}"
        );
        let p = parse(&text);
        assert!(!p.eval(&ctx()));
        assert!(p.validate().unwrap_err().contains("engine refuses"));
    }

    // Every combinator.

    #[test]
    fn all() {
        assert!(holds(
            r#"{ "all": [
                { "path": "input.amount", "op": "gt", "value": 100 },
                { "path": "steps.order.status", "op": "eq", "value": "succeeded" } ] }"#
        ));
        assert!(!holds(
            r#"{ "all": [
                { "path": "input.amount", "op": "gt", "value": 100 },
                { "path": "steps.order.status", "op": "eq", "value": "failed" } ] }"#
        ));
        assert!(holds(r#"{ "all": [ { "path": "input.amount", "op": "exists" } ] }"#));
        // Of nothing, true.
        assert!(Predicate::All { all: vec![] }.eval(&ctx()));
        assert!(holds(r#"{ "all": [] }"#));
    }

    #[test]
    fn any() {
        assert!(holds(
            r#"{ "any": [
                { "path": "input.amount", "op": "lt", "value": 100 },
                { "path": "steps.small_refund.status", "op": "eq", "value": "failed" } ] }"#
        ));
        assert!(!holds(
            r#"{ "any": [
                { "path": "input.amount", "op": "lt", "value": 100 },
                { "path": "steps.small_refund.status", "op": "eq", "value": "succeeded" } ] }"#
        ));
        // Of nothing, false.
        assert!(!Predicate::Any { any: vec![] }.eval(&ctx()));
        assert!(!holds(r#"{ "any": [] }"#));
    }

    #[test]
    fn not() {
        assert!(holds(r#"{ "not": { "path": "input.amount", "op": "lt", "value": 100 } }"#));
        assert!(!holds(r#"{ "not": { "path": "input.amount", "op": "gt", "value": 100 } }"#));
        assert!(holds(r#"{ "not": { "path": "steps.reply.output", "op": "exists" } }"#));
        // not eq is the cross-kind inequality that ne is not.
        assert!(holds(r#"{ "not": { "path": "input.amount", "op": "eq", "value": "240" } }"#));
        assert!(!holds(r#"{ "path": "input.amount", "op": "ne", "value": "240" }"#));
    }

    #[test]
    fn combinators_nest() {
        assert!(holds(
            r#"{ "all": [
                { "any": [
                    { "path": "input.amount", "op": "lte", "value": 100 },
                    { "path": "steps.approval.status", "op": "in", "value": ["skipped", "succeeded"] } ] },
                { "not": { "all": [
                    { "path": "input.approved", "op": "eq", "value": false },
                    { "path": "input.nothing", "op": "exists" } ] } } ] }"#
        ));
    }

    // Every unlike-kind case.

    #[test]
    fn unlike_kinds_compare_false_for_every_op_that_takes_a_value() {
        let c = json!({
            "s": "240", "n": 240, "b": true, "z": null, "a": [240], "o": { "v": 240 }
        });
        let values =
            [json!("240"), json!(240), json!(true), json!(null), json!([240]), json!({ "v": 240 })];
        for (path, own) in [("s", 0), ("n", 1), ("b", 2), ("z", 3), ("a", 4), ("o", 5)] {
            for (i, value) in values.iter().enumerate() {
                if i == own {
                    continue;
                }
                for op in ["eq", "ne", "lt", "lte", "gt", "gte", "matches"] {
                    let p = Predicate::Leaf {
                        path: path.to_string(),
                        op: serde_json::from_value(json!(op)).unwrap(),
                        value: Some(value.clone()),
                    };
                    assert!(!p.eval(&c), "{path} {op} {value} must be false across kinds");
                }
            }
        }
        // A missing path is null, so it is unlike everything but null.
        for op in ["eq", "ne", "lt", "lte", "gt", "gte", "matches"] {
            let text = format!(r#"{{ "path": "missing", "op": "{op}", "value": 1 }}"#);
            assert!(!parse(&text).eval(&c), "{op} against a missing path must be false");
        }
    }

    #[test]
    fn like_kinds_that_do_not_order_compare_false_on_ordering_ops() {
        let c = json!({ "b": true, "a": [1], "o": { "v": 1 }, "z": null });
        for (path, value) in
            [("b", json!(false)), ("a", json!([0])), ("o", json!({ "v": 0 })), ("z", json!(null))]
        {
            for op in ["lt", "lte", "gt", "gte"] {
                let p = Predicate::Leaf {
                    path: path.to_string(),
                    op: serde_json::from_value(json!(op)).unwrap(),
                    value: Some(value.clone()),
                };
                assert!(!p.eval(&c), "{path} {op} must be false: {value} does not order");
            }
        }
    }

    #[test]
    fn a_leaf_without_a_value_is_false_except_for_exists() {
        // Against a value, and against null, where a missing value that
        // read as null would make `eq` true.
        for path in ["input.amount", "input.nothing", "input.missing"] {
            for op in ["eq", "ne", "lt", "lte", "gt", "gte", "in", "matches"] {
                let p = Predicate::Leaf {
                    path: path.to_string(),
                    op: serde_json::from_value(json!(op)).unwrap(),
                    value: None,
                };
                assert!(!p.eval(&ctx()), "{op} on {path} with no value must be false");
            }
        }
    }

    // Deserialisation: shape.

    #[test]
    fn unknown_field_is_refused() {
        let err = refused(r#"{ "path": "input.amount", "op": "eq", "value": 1, "extra": 1 }"#);
        assert!(err.contains("unknown field `extra`"), "{err}");
        assert!(err.contains("`path`"), "{err}");
        let err = refused(r#"{ "all": [], "when": 1 }"#);
        assert!(err.contains("unknown field `when`"), "{err}");
        let err = refused(r#"{ "path": "x", "op": "eq", "value": 1, "Path": "y" }"#);
        assert!(err.contains("unknown field `Path`"), "{err}");
    }

    #[test]
    fn unknown_field_deep_inside_is_refused() {
        let err = refused(
            r#"{ "all": [ { "not": { "any": [ { "path": "x", "op": "eq", "value": 1, "op2": 1 } ] } } ] }"#,
        );
        assert!(err.contains("unknown field `op2`"), "{err}");
    }

    #[test]
    fn mixed_shapes_are_refused() {
        let err = refused(r#"{ "all": [], "any": [] }"#);
        assert!(err.contains("mixes all and any"), "{err}");
        let err = refused(r#"{ "path": "x", "op": "eq", "value": 1, "not": { "all": [] } }"#);
        assert!(err.contains("mixes a comparison and not"), "{err}");
        let err = refused(r#"{ "value": 1, "any": [] }"#);
        assert!(err.contains("mixes a comparison and any"), "{err}");
    }

    #[test]
    fn a_leaf_missing_path_or_op_is_refused() {
        let err = refused(r#"{ "path": "x", "value": 1 }"#);
        assert!(err.contains("needs \"op\""), "{err}");
        let err = refused(r#"{ "op": "eq", "value": 1 }"#);
        assert!(err.contains("needs \"path\""), "{err}");
        let err = refused(r#"{ "value": 1 }"#);
        assert!(err.contains("value on its own"), "{err}");
        let err = refused("{}");
        assert!(err.contains("empty object"), "{err}");
    }

    #[test]
    fn a_wrong_op_is_refused_by_name() {
        let err = refused(r#"{ "path": "x", "op": "equals", "value": 1 }"#);
        assert!(err.contains("unknown variant `equals`"), "{err}");
        assert!(err.contains("`matches`"), "{err}");
        let err = refused(r#"{ "path": "x", "op": "EQ", "value": 1 }"#);
        assert!(err.contains("unknown variant `EQ`"), "{err}");
    }

    #[test]
    fn wrong_types_for_the_shape_fields_are_refused() {
        let err = refused(r#"{ "path": 1, "op": "eq", "value": 1 }"#);
        assert!(err.contains("expected a string"), "{err}");
        let err = refused(r#"{ "all": { "path": "x" } }"#);
        assert!(err.contains("expected a sequence"), "{err}");
        let err = refused(r#"{ "not": [ { "path": "x", "op": "exists" } ] }"#);
        assert!(err.contains("a predicate is an object") && err.contains("got an array"), "{err}");
        let err = refused(r#""a string""#);
        assert!(err.contains("got a string"), "{err}");
        let err = refused("null");
        assert!(err.contains("got null"), "{err}");
    }

    #[test]
    fn an_array_is_not_a_positional_leaf() {
        // A derived struct would read this as path, op, value in order.
        let err = refused(r#"["input.amount", "eq", 240]"#);
        assert!(err.contains("a predicate is an object"), "{err}");
        let err = refused(r#"{ "all": [ ["input.amount", "eq", 240] ] }"#);
        assert!(err.contains("got an array"), "{err}");
    }

    #[test]
    fn serialisation_round_trips_and_keeps_null_apart_from_absence() {
        for text in [
            r#"{"path":"input.amount","op":"exists"}"#,
            r#"{"path":"input.amount","op":"eq","value":null}"#,
            r#"{"path":"input.amount","op":"in","value":[1,2]}"#,
            r#"{"all":[{"path":"a","op":"gt","value":1},{"not":{"any":[{"path":"b","op":"matches","value":"^x"}]}}]}"#,
        ] {
            let p: Predicate = serde_json::from_str(text).unwrap();
            assert_eq!(serde_json::to_string(&p).unwrap(), text);
            assert_eq!(
                serde_json::from_str::<Predicate>(&serde_json::to_string(&p).unwrap()).unwrap(),
                p
            );
        }
        let with_null = parse(r#"{ "path": "x", "op": "eq", "value": null }"#);
        assert_eq!(
            with_null,
            Predicate::Leaf { path: "x".into(), op: Op::Eq, value: Some(Value::Null) }
        );
        let without = parse(r#"{ "path": "x", "op": "exists" }"#);
        assert_eq!(without, Predicate::Leaf { path: "x".into(), op: Op::Exists, value: None });
    }

    // Every validate refusal.

    fn invalid(text: &str) -> String {
        parse(text).validate().unwrap_err()
    }

    #[test]
    fn validate_accepts_well_formed_predicates() {
        for text in [
            r#"{ "path": "input.amount", "op": "eq", "value": 1 }"#,
            r#"{ "path": "input.amount", "op": "eq", "value": null }"#,
            r#"{ "path": "input.amount", "op": "ne", "value": [1] }"#,
            r#"{ "path": "input.amount", "op": "lt", "value": 1 }"#,
            r#"{ "path": "input.amount", "op": "lte", "value": "a" }"#,
            r#"{ "path": "input.amount", "op": "gt", "value": 1.5 }"#,
            r#"{ "path": "input.amount", "op": "gte", "value": -1 }"#,
            r#"{ "path": "input.amount", "op": "in", "value": [] }"#,
            r#"{ "path": "input.amount", "op": "in", "value": [1, "a", null] }"#,
            r#"{ "path": "input.amount", "op": "exists" }"#,
            r#"{ "path": "input.amount", "op": "matches", "value": "^ord_[0-9]+$" }"#,
            r#"{ "path": "steps.order.output.lines[0].sku", "op": "exists" }"#,
            r#"{ "all": [ { "path": "a", "op": "exists" } ] }"#,
            r#"{ "any": [ { "path": "a", "op": "exists" }, { "not": { "path": "b", "op": "exists" } } ] }"#,
            r#"{ "not": { "all": [ { "path": "a", "op": "exists" } ] } }"#,
        ] {
            assert_eq!(parse(text).validate(), Ok(()), "{text}");
        }
    }

    #[test]
    fn validate_refuses_an_op_without_its_value() {
        for op in ["eq", "ne", "lt", "lte", "gt", "gte", "in", "matches"] {
            let err = invalid(&format!(r#"{{ "path": "x", "op": "{op}" }}"#));
            assert!(err.contains(&format!("op {op} needs a value")), "{err}");
        }
    }

    #[test]
    fn validate_refuses_a_value_where_none_is_allowed() {
        let err = invalid(r#"{ "path": "x", "op": "exists", "value": 1 }"#);
        assert!(err.contains("exists takes no value"), "{err}");
        let err = invalid(r#"{ "path": "x", "op": "exists", "value": null }"#);
        assert!(err.contains("exists takes no value"), "{err}");
    }

    #[test]
    fn validate_refuses_in_without_an_array() {
        let err = invalid(r#"{ "path": "x", "op": "in", "value": 1 }"#);
        assert!(err.contains("in needs an array value, got a number"), "{err}");
        let err = invalid(r#"{ "path": "x", "op": "in", "value": "a" }"#);
        assert!(err.contains("got a string"), "{err}");
        let err = invalid(r#"{ "path": "x", "op": "in", "value": { "a": 1 } }"#);
        assert!(err.contains("got an object"), "{err}");
    }

    #[test]
    fn validate_refuses_a_bad_regex() {
        let err = invalid(r#"{ "path": "x", "op": "matches", "value": "(" }"#);
        assert!(err.contains("op matches has a pattern the engine refuses"), "{err}");
        let err = invalid(r#"{ "path": "x", "op": "matches", "value": "a{2,1}" }"#);
        assert!(err.contains("engine refuses"), "{err}");
        let err = invalid(r#"{ "path": "x", "op": "matches", "value": 1 }"#);
        assert!(err.contains("needs a string value"), "{err}");
        let err = invalid(r#"{ "path": "x", "op": "matches", "value": ["^x"] }"#);
        assert!(err.contains("got an array"), "{err}");
    }

    #[test]
    fn validate_refuses_ordering_against_a_kind_that_does_not_order() {
        for op in ["lt", "lte", "gt", "gte"] {
            for value in ["true", "null", "[1]", "{}"] {
                let err = invalid(&format!(r#"{{ "path": "x", "op": "{op}", "value": {value} }}"#));
                assert!(err.contains(&format!("op {op} orders numbers and strings")), "{err}");
            }
        }
    }

    #[test]
    fn validate_refuses_an_empty_all_or_any() {
        let err = invalid(r#"{ "all": [] }"#);
        assert!(err.contains("\"all\" needs at least one"), "{err}");
        let err = invalid(r#"{ "any": [] }"#);
        assert!(err.contains("\"any\" needs at least one"), "{err}");
        // Deep inside too.
        let err =
            invalid(r#"{ "not": { "all": [ { "path": "x", "op": "exists" }, { "any": [] } ] } }"#);
        assert!(err.contains("\"any\" needs at least one"), "{err}");
    }

    #[test]
    fn validate_refuses_a_path_that_is_not_a_path() {
        for path in ["", ".x", "x.", "x[a]", "x y"] {
            let err = invalid(&format!(r#"{{ "path": "{path}", "op": "exists" }}"#));
            assert!(err.contains("is not a path"), "{path:?}: {err}");
        }
    }

    #[test]
    fn validate_reports_the_first_problem_inside_a_combinator() {
        let err = invalid(
            r#"{ "all": [ { "path": "a", "op": "exists" }, { "path": "b", "op": "in", "value": 1 }, { "any": [] } ] }"#,
        );
        assert!(err.contains("in needs an array"), "{err}");
    }
}
