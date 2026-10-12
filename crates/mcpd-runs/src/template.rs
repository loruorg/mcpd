//! Templates and paths: section 3.3 of the run contract.
//!
//! A path names one value in the context a run offers a node: `input.order_id`,
//! `steps.order.output.lines[0].sku`, `iter.index`. A template is any JSON
//! value whose strings may carry `${path}` references. This module parses the
//! first and renders the second, and it is the only place either grammar lives:
//! the predicate evaluator, the walker and the write-time validator all call in
//! here, so a path cannot mean one thing to a guard and another to an input.
//!
//! Nothing here fails at run time. A path that names nothing, or that is not a
//! path at all, resolves to JSON null, because a run is moved inside the
//! transaction the tick holds and a guard that errors there is a run that never
//! advances and no attempt row to say why. What can be refused is refused
//! before the document is stored: [`check`] finds an unterminated `${` or a
//! reference that is not a path, and the validator turns that into a 400 that
//! names the field. A run therefore only ever meets the null case for a value
//! that is genuinely absent.
//!
//! There is no escape sequence. A literal `${` is not expressible in version 1,
//! and the contract says so.

use serde_json::Value;
use std::collections::HashSet;

/// One step of a parsed path.
enum Segment<'a> {
    /// A key into an object.
    Key(&'a str),
    /// An offset into an array.
    Index(usize),
}

/// Whether a character may appear in a name segment.
///
/// The set is open rather than an identifier alphabet because `input` holds
/// whatever the caller posted, and a key such as `content-type` has to be
/// addressable. The exclusions are the path's own punctuation and the
/// template's, so a string that has lost a brace cannot parse as a path with a
/// brace inside it, plus whitespace and control characters, which only ever
/// mean a typo.
fn is_name_char(c: char) -> bool {
    !matches!(c, '.' | '[' | ']' | '{' | '}' | '$') && !c.is_whitespace() && !c.is_control()
}

/// Split the leading name off `s`, or `None` when there is no name there.
fn take_name(s: &str) -> Option<(&str, &str)> {
    let end = s.find(|c: char| !is_name_char(c)).unwrap_or(s.len());
    if end == 0 {
        return None;
    }
    Some(s.split_at(end))
}

/// Parse a path, or `None` when the text is not one.
///
/// The grammar is a name, then any number of `.name` or `[n]` steps. A path
/// starts with a name and never with an index because every root the contract
/// offers is a name. An index is unsigned decimal digits; one that does not
/// fit `usize` cannot address anything and is refused rather than wrapped.
fn parse(path: &str) -> Option<Vec<Segment<'_>>> {
    let (name, mut rest) = take_name(path)?;
    let mut segments = vec![Segment::Key(name)];
    while !rest.is_empty() {
        if let Some(tail) = rest.strip_prefix('.') {
            let (name, tail) = take_name(tail)?;
            segments.push(Segment::Key(name));
            rest = tail;
        } else {
            let tail = rest.strip_prefix('[')?;
            let end = tail.find(']')?;
            let digits = &tail[..end];
            if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            segments.push(Segment::Index(digits.parse().ok()?));
            rest = &tail[end + 1..];
        }
    }
    Some(segments)
}

/// Whether `path` is a path the renderer can follow.
///
/// The write-time validator calls this on every path [`find_paths`] returns,
/// so a document with `${.foo}` in it is refused instead of rendering an empty
/// string on every run.
#[must_use]
pub fn is_valid_path(path: &str) -> bool {
    parse(path).is_some()
}

/// The root a path reads from: `input`, `steps`, `item`, `index`, `iter` or
/// `run` in a well-formed document, or whatever the author wrote in a bad one.
/// `None` when the text is not a path.
///
/// Which roots exist depends on where the path sits (`item` only inside a
/// `foreach` body, `iter` only inside a `loop`), and only the validator knows
/// where it is, so the check lives there and this only hands it the name.
#[must_use]
pub fn root(path: &str) -> Option<&str> {
    match parse(path)?.first() {
        Some(Segment::Key(name)) => Some(name),
        _ => None,
    }
}

/// The value `path` names in `ctx`, or JSON null when it names nothing.
///
/// A key step walks an object and an index step walks an array; a step of the
/// wrong kind for the value in hand, a key that is absent, an index past the
/// end and a path that does not parse all resolve to null. Never panics.
#[must_use]
pub fn resolve(path: &str, ctx: &Value) -> Value {
    let Some(segments) = parse(path) else {
        return Value::Null;
    };
    let mut current = ctx;
    for segment in segments {
        let next = match (segment, current) {
            (Segment::Key(key), Value::Object(map)) => map.get(key),
            (Segment::Index(index), Value::Array(items)) => items.get(index),
            _ => None,
        };
        match next {
            Some(value) => current = value,
            None => return Value::Null,
        }
    }
    current.clone()
}

/// A slice of a template string.
enum Piece<'a> {
    /// Literal text, never empty.
    Text(&'a str),
    /// The inside of one `${...}`, braces removed.
    Path(&'a str),
    /// A `${` with no `}` after it, together with everything to the end of the
    /// string.
    Unterminated(&'a str),
}

/// The one scanner over template strings.
///
/// [`render_string`], [`find_paths`] and [`check`] all iterate this, so the
/// three cannot disagree about where a reference starts and ends. The first
/// `}` after a `${` closes it; there is no nesting and no escape.
struct Pieces<'a> {
    rest: &'a str,
}

impl<'a> Iterator for Pieces<'a> {
    type Item = Piece<'a>;

    fn next(&mut self) -> Option<Piece<'a>> {
        if self.rest.is_empty() {
            return None;
        }
        if let Some(after) = self.rest.strip_prefix("${") {
            let Some(end) = after.find('}') else {
                let whole = self.rest;
                self.rest = "";
                return Some(Piece::Unterminated(whole));
            };
            let path = &after[..end];
            self.rest = &after[end + 1..];
            return Some(Piece::Path(path));
        }
        let end = self.rest.find("${").unwrap_or(self.rest.len());
        let text = &self.rest[..end];
        self.rest = &self.rest[end..];
        Some(Piece::Text(text))
    }
}

fn pieces(s: &str) -> Pieces<'_> {
    Pieces { rest: s }
}

/// A value as it reads inside a string: a string verbatim, a number or bool as
/// JSON, an object or array as compact JSON, null as nothing at all.
fn as_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// `s` with every `${path}` replaced by the text of the value it names.
///
/// A reference to nothing renders as an empty string, which is the null rule
/// of section 3.3 applied to an absent value. An unterminated `${` is copied
/// through verbatim: the renderer cannot know what was meant, and dropping the
/// author's text would hide the mistake that [`check`] exists to report.
#[must_use]
pub fn render_string(s: &str, ctx: &Value) -> String {
    let mut out = String::with_capacity(s.len());
    for piece in pieces(s) {
        match piece {
            Piece::Text(text) | Piece::Unterminated(text) => out.push_str(text),
            Piece::Path(path) => out.push_str(&as_text(&resolve(path, ctx))),
        }
    }
    out
}

/// The path a string carries when the string is exactly one `${path}` and
/// nothing else.
///
/// `${a}${b}` starts and ends like one reference and is two. The first `}`
/// after `${` closes a reference, so an exact one has no brace inside.
fn exact_path(s: &str) -> Option<&str> {
    let inner = s.strip_prefix("${")?.strip_suffix('}')?;
    if inner.contains('}') {
        None
    } else {
        Some(inner)
    }
}

/// `value` rendered against `ctx`.
///
/// A string that is exactly one `${path}` becomes the JSON value the path
/// names, so `"amount": "${steps.order.output.amount_usd}"` renders to a
/// number and not to a string holding digits. Any other string renders through
/// [`render_string`]. Objects and arrays render their values recursively, with
/// keys left as written: a key is a name, not a template. Numbers, bools and
/// null pass through unchanged.
///
/// Recursion follows the shape of `value`. Every template the daemon renders
/// arrived through `serde_json`, which refuses to parse deeper than its own
/// recursion limit, so the depth here is bounded by the parser's and not by
/// the author.
#[must_use]
pub fn render(value: &Value, ctx: &Value) -> Value {
    match value {
        Value::String(s) => match exact_path(s) {
            Some(path) => resolve(path, ctx),
            None => Value::String(render_string(s, ctx)),
        },
        Value::Array(items) => Value::Array(items.iter().map(|item| render(item, ctx)).collect()),
        Value::Object(map) => {
            Value::Object(map.iter().map(|(key, item)| (key.clone(), render(item, ctx))).collect())
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => value.clone(),
    }
}

/// Every path a template references, once each, in document order.
///
/// This is for the validator, so it returns what the author wrote and not
/// what parses: `${.foo}` yields `.foo`, and [`is_valid_path`] is how the
/// caller refuses it. An unterminated `${` references nothing and is reported
/// by [`check`] instead.
#[must_use]
pub fn find_paths(value: &Value) -> Vec<String> {
    let mut paths = Vec::new();
    let mut seen = HashSet::new();
    collect_paths(value, &mut paths, &mut seen);
    paths
}

fn collect_paths(value: &Value, paths: &mut Vec<String>, seen: &mut HashSet<String>) {
    match value {
        Value::String(s) => {
            for piece in pieces(s) {
                if let Piece::Path(path) = piece {
                    if seen.insert(path.to_string()) {
                        paths.push(path.to_string());
                    }
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_paths(item, paths, seen);
            }
        }
        Value::Object(map) => {
            for item in map.values() {
                collect_paths(item, paths, seen);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

/// One step of a path, for a caller that has to read the path itself rather
/// than follow it.
///
/// [`resolve`] walks a path against a value. The cause walk in
/// [`crate::cause`] has to read the path instead, because
/// `steps.classify.output.kind` names the step whose output it reads and the
/// walk needs that name. Both go through the same parser, so a path cannot
/// mean one thing to the renderer and another to the walk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathStep {
    /// A key into an object.
    Key(String),
    /// An offset into an array.
    Index(usize),
}

/// Every step of `path`, in order, or `None` when the text is not a path.
#[must_use]
pub fn path_steps(path: &str) -> Option<Vec<PathStep>> {
    Some(
        parse(path)?
            .into_iter()
            .map(|segment| match segment {
                Segment::Key(name) => PathStep::Key(name.to_string()),
                Segment::Index(index) => PathStep::Index(index),
            })
            .collect(),
    )
}

/// A string cut down to fit in an error message.
fn excerpt(s: &str) -> String {
    const LIMIT: usize = 60;
    if s.chars().count() <= LIMIT {
        return s.to_string();
    }
    let head: String = s.chars().take(LIMIT).collect();
    format!("{head}...")
}

/// Refuse a template the renderer could not honour.
///
/// A template that passes here may still reference a root that does not exist
/// at its position; that check needs the document around it and belongs to
/// the validator.
///
/// # Errors
///
/// The first problem found: an unterminated `${`, or a reference whose inside
/// is not a path. The message quotes the offending string so the caller can
/// prefix it with the field it sits in.
pub fn check(value: &Value) -> Result<(), String> {
    match value {
        Value::String(s) => {
            for piece in pieces(s) {
                match piece {
                    Piece::Text(_) => {}
                    Piece::Path(path) if is_valid_path(path) => {}
                    Piece::Path(path) => {
                        return Err(format!(
                            "\"${{{}}}\" is not a path: write a name followed by .name or [n] \
                             steps, such as input.order.lines[0].sku",
                            excerpt(path)
                        ));
                    }
                    Piece::Unterminated(tail) => {
                        return Err(format!(
                            "\"{}\" opens a ${{ reference and never closes it: add the \
                             closing brace, or remove the ${{; there is no escape for a \
                             literal ${{",
                            excerpt(tail)
                        ));
                    }
                }
            }
            Ok(())
        }
        Value::Array(items) => items.iter().try_for_each(check),
        Value::Object(map) => map.values().try_for_each(check),
        Value::Null | Value::Bool(_) | Value::Number(_) => Ok(()),
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
                "negative": -3,
                "approved": true,
                "nothing": null,
                "order": { "lines": [ { "qty": 2, "sku": "A1" }, { "qty": 1, "sku": "B2" } ] },
                "matrix": [[1, 2], [3, [4, 5]]],
                "content-type": "text/plain"
            },
            "steps": {
                "order": { "status": "succeeded", "output": { "amount_usd": 240, "tags": ["a", "b"] } },
                "approval": { "status": "skipped" }
            },
            "iter": { "index": 3 },
            "run": { "run_id": "run_1", "definition_version": 3 }
        })
    }

    // Section 3.3: dot-separated with [n] for arrays.

    #[test]
    fn resolve_walks_objects_and_arrays() {
        let c = ctx();
        assert_eq!(resolve("input.order_id", &c), json!("ord_913"));
        assert_eq!(resolve("input.order.lines[0].sku", &c), json!("A1"));
        assert_eq!(resolve("input.order.lines[1]", &c), json!({ "qty": 1, "sku": "B2" }));
        assert_eq!(resolve("steps.order.output.amount_usd", &c), json!(240));
        assert_eq!(resolve("steps.order.output.tags[1]", &c), json!("b"));
        assert_eq!(resolve("iter.index", &c), json!(3));
        assert_eq!(resolve("input", &c), c["input"]);
    }

    #[test]
    fn resolve_nested_arrays() {
        let c = ctx();
        assert_eq!(resolve("input.matrix[1][1][0]", &c), json!(4));
        assert_eq!(resolve("input.matrix[0][1]", &c), json!(2));
        assert_eq!(resolve("input.matrix[1][1]", &c), json!([4, 5]));
        assert_eq!(resolve("input.matrix[1][1][2]", &c), Value::Null);
    }

    #[test]
    fn resolve_accepts_keys_outside_the_identifier_alphabet() {
        assert_eq!(resolve("input.content-type", &ctx()), json!("text/plain"));
    }

    #[test]
    fn missing_path_is_null() {
        let c = ctx();
        assert_eq!(resolve("input.missing", &c), Value::Null);
        assert_eq!(resolve("steps.missing.output", &c), Value::Null);
        assert_eq!(resolve("input.order.lines[9].sku", &c), Value::Null);
        assert_eq!(resolve("nothing", &c), Value::Null);
        // A null that is present resolves to null too: the two are the same
        // to a reader, which is the contract's rule.
        assert_eq!(resolve("input.nothing", &c), Value::Null);
    }

    #[test]
    fn wrong_step_kind_for_the_value_is_null() {
        let c = ctx();
        // An index into an object.
        assert_eq!(resolve("input[0]", &c), Value::Null);
        // A key into an array, even one that spells a number: `[n]` is the
        // only index syntax.
        assert_eq!(resolve("input.order.lines.sku", &c), Value::Null);
        assert_eq!(resolve("input.order.lines.0", &c), Value::Null);
        assert_eq!(resolve("input.matrix.1.0", &c), Value::Null);
        // A step into a scalar.
        assert_eq!(resolve("input.order_id.len", &c), Value::Null);
        assert_eq!(resolve("input.amount[0]", &c), Value::Null);
        // A root that is not an object.
        assert_eq!(resolve("a.b", &json!([1])), Value::Null);
        assert_eq!(resolve("a", &json!("text")), Value::Null);
    }

    #[test]
    fn invalid_syntax_is_null_and_not_a_path() {
        let c = ctx();
        for bad in [
            "",
            ".",
            ".input",
            "input.",
            "input..order_id",
            "input[",
            "input[]",
            "input[a]",
            "input[0",
            "input]",
            "input[0]order",
            "[0]",
            "[0].input",
            "input order_id",
            "input.order id",
            " input",
            "input\t",
            "input.${order_id}",
            "input.order_id}",
            "input.{order_id",
            "input[99999999999999999999999]",
            "input[-1]",
            "input[1.5]",
            "input[ 1]",
        ] {
            assert!(!is_valid_path(bad), "{bad:?} should not parse");
            assert_eq!(resolve(bad, &c), Value::Null, "{bad:?} should resolve to null");
            assert_eq!(root(bad), None, "{bad:?} has no root");
        }
    }

    #[test]
    fn valid_syntax_parses() {
        for good in [
            "input",
            "input.order_id",
            "input.order.lines[0].sku",
            "input.matrix[1][1][0]",
            "steps.order.output",
            "steps.fanout.branch_a.output",
            "item",
            "index",
            "iter.index",
            "run.run_id",
            "input.content-type",
            "input.Ünïcödé",
            "input[0]",
            "input[007]",
        ] {
            assert!(is_valid_path(good), "{good:?} should parse");
        }
    }

    #[test]
    fn path_steps_reads_the_path_the_renderer_would_walk() {
        assert_eq!(
            path_steps("steps.classify.output.kinds[2]"),
            Some(vec![
                PathStep::Key("steps".into()),
                PathStep::Key("classify".into()),
                PathStep::Key("output".into()),
                PathStep::Key("kinds".into()),
                PathStep::Index(2),
            ])
        );
        assert_eq!(path_steps("input"), Some(vec![PathStep::Key("input".into())]));
        assert_eq!(path_steps(".bad"), None);
        assert_eq!(path_steps(""), None);
    }

    #[test]
    fn root_is_the_first_name() {
        assert_eq!(root("input.order.lines[0].sku"), Some("input"));
        assert_eq!(root("steps"), Some("steps"));
        assert_eq!(root("item[2]"), Some("item"));
        assert_eq!(root(""), None);
    }

    // Section 3.3: how a value reads inside a string.

    #[test]
    fn render_string_substitutes_each_kind_of_value() {
        let c = ctx();
        assert_eq!(render_string("id=${input.order_id}", &c), "id=ord_913");
        assert_eq!(render_string("n=${input.amount}", &c), "n=240");
        assert_eq!(render_string("r=${input.ratio}", &c), "r=2.5");
        assert_eq!(render_string("m=${input.negative}", &c), "m=-3");
        assert_eq!(render_string("ok=${input.approved}", &c), "ok=true");
        // Compact means no whitespace; key order is the map's own.
        assert_eq!(render_string("o=${input.order.lines[0]}", &c), "o={\"qty\":2,\"sku\":\"A1\"}");
        assert_eq!(render_string("a=${steps.order.output.tags}", &c), "a=[\"a\",\"b\"]");
        assert_eq!(render_string("nested=${input.matrix}", &c), "nested=[[1,2],[3,[4,5]]]");
    }

    #[test]
    fn null_renders_as_an_empty_string() {
        let c = ctx();
        assert_eq!(render_string("<${input.nothing}>", &c), "<>");
        assert_eq!(render_string("<${input.missing}>", &c), "<>");
        assert_eq!(render_string("<${steps.missing.output.text}>", &c), "<>");
    }

    #[test]
    fn render_string_handles_several_references() {
        let c = ctx();
        assert_eq!(
            render_string("${input.order_id}/${input.amount}/${iter.index}", &c),
            "ord_913/240/3"
        );
        assert_eq!(render_string("${input.order_id}${input.amount}", &c), "ord_913240");
        assert_eq!(render_string("plain text", &c), "plain text");
        assert_eq!(render_string("", &c), "");
        assert_eq!(render_string("$ {input.order_id}", &c), "$ {input.order_id}");
        assert_eq!(render_string("{input.order_id}", &c), "{input.order_id}");
    }

    #[test]
    fn a_string_that_is_exactly_one_reference_yields_the_json_value() {
        let c = ctx();
        assert_eq!(render(&json!("${input.amount}"), &c), json!(240));
        assert_eq!(render(&json!("${input.ratio}"), &c), json!(2.5));
        assert_eq!(render(&json!("${input.approved}"), &c), json!(true));
        assert_eq!(render(&json!("${input.order_id}"), &c), json!("ord_913"));
        assert_eq!(render(&json!("${input.order.lines[0]}"), &c), json!({ "qty": 2, "sku": "A1" }));
        assert_eq!(render(&json!("${steps.order.output.tags}"), &c), json!(["a", "b"]));
        assert_eq!(render(&json!("${input.nothing}"), &c), Value::Null);
        assert_eq!(render(&json!("${input.missing}"), &c), Value::Null);
        assert_eq!(render(&json!("${}"), &c), Value::Null);
    }

    #[test]
    fn any_other_string_renders_to_a_string() {
        let c = ctx();
        assert_eq!(render(&json!(" ${input.amount}"), &c), json!(" 240"));
        assert_eq!(render(&json!("${input.amount} "), &c), json!("240 "));
        assert_eq!(render(&json!("${input.amount}${input.amount}"), &c), json!("240240"));
        assert_eq!(render(&json!("${input.amount}}"), &c), json!("240}"));
        assert_eq!(render(&json!("${input.nothing}x"), &c), json!("x"));
        assert_eq!(render(&json!("x${input.nothing}"), &c), json!("x"));
        assert_eq!(
            render(&json!("${input.order.lines[0]}!"), &c),
            json!("{\"qty\":2,\"sku\":\"A1\"}!")
        );
    }

    #[test]
    fn render_recurses_and_passes_scalars_through() {
        let c = ctx();
        let template = json!({
            "prompt": "Refund ${input.order_id} for ${input.amount} USD",
            "amount": "${input.amount}",
            "lines": ["${input.order.lines[0].sku}", "${input.order.lines[1]}", 7, false, null],
            "nested": { "deep": { "id": "${run.run_id}", "keep": 1.5 } },
            "${input.order_id}": "keys are not templates",
            "n": 42,
            "b": true,
            "z": null
        });
        assert_eq!(
            render(&template, &c),
            json!({
                "prompt": "Refund ord_913 for 240 USD",
                "amount": 240,
                "lines": ["A1", { "qty": 1, "sku": "B2" }, 7, false, null],
                "nested": { "deep": { "id": "run_1", "keep": 1.5 } },
                "${input.order_id}": "keys are not templates",
                "n": 42,
                "b": true,
                "z": null
            })
        );
    }

    #[test]
    fn find_paths_lists_every_reference_once_in_document_order() {
        let template = json!({
            "a": "${input.x} and ${input.y}",
            "b": ["${input.x}", { "c": "${steps.s.output}" }],
            "d": 1,
            "e": "${.bad}",
            "f": "${unterminated"
        });
        assert_eq!(find_paths(&template), vec!["input.x", "input.y", "steps.s.output", ".bad"]);
        assert_eq!(find_paths(&json!(null)), Vec::<String>::new());
        assert_eq!(find_paths(&json!("no references")), Vec::<String>::new());
    }

    #[test]
    fn check_accepts_a_well_formed_template() {
        assert_eq!(
            check(&json!({ "a": "${input.x}", "b": ["${steps.s.output[0]}", 1, null, "text"] })),
            Ok(())
        );
        assert_eq!(check(&json!("$ not a reference")), Ok(()));
        assert_eq!(check(&json!(3)), Ok(()));
    }

    #[test]
    fn check_refuses_an_unterminated_reference() {
        let err = check(&json!({ "a": "fine", "b": "hello ${input.x" })).unwrap_err();
        assert!(err.contains("${input.x"), "{err}");
        assert!(err.contains("never closes"), "{err}");
    }

    #[test]
    fn check_refuses_a_reference_that_is_not_a_path() {
        for bad in ["${}", "${.input}", "${input.}", "${input[a]}", "${input x}"] {
            let err = check(&json!([1, bad])).unwrap_err();
            assert!(err.contains("is not a path"), "{bad}: {err}");
        }
        // A name may hold anything but the path's own punctuation, so this
        // is a path, and the validator's root check is what would refuse it.
        assert_eq!(check(&json!("${input.\u{1F600}}")), Ok(()));
    }

    #[test]
    fn check_shortens_a_long_offender() {
        let long = format!("${{{}", "a".repeat(1000));
        let err = check(&json!(long)).unwrap_err();
        assert!(err.len() < 300, "{}", err.len());
        assert!(err.contains("..."), "{err}");
    }

    // Adversarial inputs.

    #[test]
    fn unterminated_reference_renders_verbatim() {
        let c = ctx();
        assert_eq!(render_string("a ${input.order_id", &c), "a ${input.order_id");
        assert_eq!(render_string("${", &c), "${");
        assert_eq!(render_string("$", &c), "$");
        assert_eq!(render_string("${input.order_id} ${", &c), "ord_913 ${");
        assert_eq!(render(&json!("${input.order_id"), &c), json!("${input.order_id"));
    }

    #[test]
    fn empty_and_malformed_references_render_as_nothing() {
        let c = ctx();
        assert_eq!(render_string("a${}b", &c), "ab");
        assert_eq!(render_string("a${.input}b", &c), "ab");
        assert_eq!(render_string("a${input.}b", &c), "ab");
        assert_eq!(render_string("a${input[x]}b", &c), "ab");
        // The first brace closes the reference, so the inside is not a path
        // and the second brace is text.
        assert_eq!(render_string("a${input.${order_id}}b", &c), "a}b");
        assert_eq!(render_string("}${input.amount}{", &c), "}240{");
    }

    #[test]
    fn deeply_nested_arrays_resolve_without_recursion() {
        // resolve walks iteratively, so a path with thousands of index steps
        // costs a loop and not a stack frame per step.
        let depth = 2_000;
        let mut value = json!(42);
        for _ in 0..depth {
            value = Value::Array(vec![value]);
        }
        // json! would serialise the interpolated value recursively; insert
        // it instead so nothing but the code under test walks the depth.
        let mut map = serde_json::Map::new();
        map.insert("input".to_string(), value);
        let c = Value::Object(map);
        let path = format!("input{}", "[0]".repeat(depth));
        assert!(is_valid_path(&path));
        assert_eq!(resolve(&path, &c), json!(42));
        let short = format!("input{}", "[0]".repeat(depth - 1));
        assert_eq!(resolve(&short, &c), json!([42]));
        let long = format!("input{}", "[0]".repeat(depth + 1));
        assert_eq!(resolve(&long, &c), Value::Null);
        // A long path against a shallow value walks off the value at the
        // first extra step, in a loop.
        let shallow = json!({ "input": [[42]] });
        assert_eq!(resolve(&path, &shallow), Value::Null);
        assert_eq!(render_string(&format!("<${{{path}}}>"), &shallow), "<>");
        // Dropping a value this deep recurses once per level; keep that out
        // of the test thread's stack by unwinding it by hand.
        let mut value = match c {
            Value::Object(mut map) => map.remove("input").unwrap(),
            _ => unreachable!(),
        };
        while let Value::Array(mut items) = value {
            value = items.pop().unwrap_or(Value::Null);
        }
    }

    #[test]
    fn render_handles_the_deepest_document_the_parser_admits() {
        // serde_json refuses to parse deeper than 128 levels, so this is the
        // deepest template a definition can carry.
        let depth = 127;
        let text = format!("{}\"${{input.x}}\"{}", "[".repeat(depth), "]".repeat(depth));
        let template: Value = serde_json::from_str(&text).unwrap();
        let rendered = render(&template, &json!({ "input": { "x": 7 } }));
        let mut current = &rendered;
        for _ in 0..depth {
            current = &current[0];
        }
        assert_eq!(current, &json!(7));
        assert_eq!(find_paths(&template), vec!["input.x"]);
        assert_eq!(check(&template), Ok(()));
    }

    #[test]
    fn a_one_megabyte_string_renders() {
        let c = ctx();
        let size = 1 << 20;

        let plain = "x".repeat(size);
        assert_eq!(render_string(&plain, &c), plain);

        let references = "${input.amount}".repeat(size / 15);
        let out = render_string(&references, &c);
        assert_eq!(out.len(), 3 * (size / 15));
        assert!(out.starts_with("240240"));

        let mut open = String::from("${");
        open.push_str(&"y".repeat(size));
        assert_eq!(render_string(&open, &c), open);

        let long_path = format!("${{{}}}", "z".repeat(size));
        assert_eq!(render_string(&long_path, &c), "");
        assert_eq!(render(&json!(long_path), &c), Value::Null);

        let literal_json = Value::String(plain.clone());
        assert_eq!(render(&literal_json, &c), literal_json);
    }
}
