//! Redaction.
//!
//! Prompts and completions are the most valuable thing mcpd stores and the most
//! dangerous. Redaction runs **before** anything is written, so a value that
//! should never have been persisted never is — deleting it afterwards is not
//! the same guarantee, and an auditor knows it.
//!
//! Unlike the incumbent, redaction here is not a span-only concern: the same
//! pipeline is applied to logs, metric labels, scores and feedback, because a
//! secret leaks just as well through a log line as through a prompt.

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

pub use crate::RedactionMode;

/// One rule: which keys it matches, and what to do with them.
#[derive(Debug, Clone)]
pub struct Rule {
    /// Lowercase substrings; a key matches when it contains any of them.
    pub key_contains: Vec<String>,
    /// What to do with a matched value.
    pub mode: RedactionMode,
}

/// The redaction pipeline.
#[derive(Debug, Clone)]
pub struct Redactor {
    rules: Vec<Rule>,
    /// Key for the hash mode. Equal inputs hash equally under one key, so you
    /// can still group by a redacted user id; rotating the key breaks that link
    /// deliberately.
    hash_key: Vec<u8>,
    /// Replacement token for [`RedactionMode::Mask`].
    token: String,
    /// How deep to walk before giving up, so a cyclic or pathological document
    /// cannot pin a CPU.
    max_depth: usize,
    /// Longest string kept verbatim. Longer ones are truncated with a marker,
    /// which bounds the damage a runaway prompt does to storage.
    max_string: usize,
    /// Whether string values are searched for credential shapes, whatever key
    /// they sit under. See [`find_secrets`].
    detect_values: bool,
}

impl Default for Redactor {
    fn default() -> Self {
        Self::with_defaults()
    }
}

/// A value this short leaks most of itself through a partial mask, so `partial`
/// masks it entirely instead. It is also short enough that its digest can be
/// guessed, which is why `stronger` refuses to prefer Hash over Partial.
const SHORT_ENOUGH_TO_GUESS: usize = 6;

impl Redactor {
    /// A redactor that does nothing. Only appropriate when a deployment has
    /// deliberately opted out.
    #[must_use]
    pub fn disabled() -> Self {
        Self {
            rules: Vec::new(),
            hash_key: Vec::new(),
            token: String::new(),
            max_depth: 32,
            max_string: usize::MAX,
            detect_values: false,
        }
    }

    /// The default rules, which catch the field names credentials are actually
    /// stored under. On by default: a tool that only protects you once you have
    /// configured it protects nobody.
    #[must_use]
    pub fn with_defaults() -> Self {
        let mask = |names: &[&str]| Rule {
            key_contains: names.iter().map(|s| (*s).to_owned()).collect(),
            mode: RedactionMode::Mask,
        };
        Self {
            rules: vec![
                mask(&[
                    "api_key",
                    "apikey",
                    "api-key",
                    "secret",
                    "password",
                    "passwd",
                    "access_token",
                    "refresh_token",
                    "id_token",
                    "bearer",
                    "authorization",
                    "auth_token",
                    "private_key",
                    "client_secret",
                    "session_token",
                    "credential",
                    "signature",
                    "x-api-key",
                ]),
                Rule {
                    key_contains: vec![
                        "email".into(),
                        "phone".into(),
                        "ssn".into(),
                        "credit_card".into(),
                        "card_number".into(),
                        "iban".into(),
                    ],
                    mode: RedactionMode::Partial,
                },
                Rule {
                    key_contains: vec!["user_id".into(), "userid".into()],
                    mode: RedactionMode::Hash,
                },
            ],
            hash_key: b"mcpd-default-redaction-key".to_vec(),
            token: "[redacted]".to_owned(),
            max_depth: 32,
            max_string: 128 * 1024,
            detect_values: true,
        }
    }

    /// Replace the hash key. Do this in any deployment that hashes identifiers,
    /// or the digests are guessable by anyone who reads this source.
    #[must_use]
    pub fn with_hash_key(mut self, key: impl Into<Vec<u8>>) -> Self {
        self.hash_key = key.into();
        self
    }

    /// Add a rule on top of the defaults.
    #[must_use]
    pub fn with_rule(mut self, rule: Rule) -> Self {
        self.rules.push(rule);
        self
    }

    /// Cap on the length of any single stored string.
    #[must_use]
    pub fn with_max_string(mut self, n: usize) -> Self {
        self.max_string = n;
        self
    }

    fn mode_for(&self, key: &str) -> Option<RedactionMode> {
        let k = key.to_ascii_lowercase();
        self.rules
            .iter()
            .find(|r| r.key_contains.iter().any(|n| k.contains(n.as_str())))
            .map(|r| r.mode)
    }

    fn hash(&self, value: &str) -> String {
        let mut h = Sha256::new();
        h.update(&self.hash_key);
        h.update(value.as_bytes());
        format!("sha256:{}", hex::encode(&h.finalize()[..12]))
    }

    fn partial(&self, value: &str) -> String {
        let chars: Vec<char> = value.chars().collect();
        // Too short to reveal anything safely: mask it entirely rather than
        // leaking most of a four-character value.
        if chars.len() <= SHORT_ENOUGH_TO_GUESS {
            return self.token.clone();
        }
        let head: String = chars.iter().take(2).collect();
        let tail: String =
            chars.iter().rev().take(2).collect::<Vec<_>>().into_iter().rev().collect();
        format!("{head}…{tail}")
    }

    fn apply_to_string(&self, mode: RedactionMode, s: &str) -> Value {
        match mode {
            RedactionMode::Drop => Value::Null,
            RedactionMode::Mask => Value::String(self.token.clone()),
            RedactionMode::Partial => Value::String(self.partial(s)),
            RedactionMode::Hash => Value::String(self.hash(s)),
        }
    }

    fn truncate(&self, s: &str) -> Value {
        if s.len() <= self.max_string {
            return Value::String(s.to_owned());
        }
        // Cut on a character boundary, never mid-codepoint.
        let mut end = self.max_string;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        Value::String(format!("{}… [{} bytes truncated]", &s[..end], s.len() - end))
    }

    /// Turn the search of string values for credential shapes on or off. On by
    /// default with the default rules.
    #[must_use]
    pub fn with_value_detection(mut self, on: bool) -> Self {
        self.detect_values = on;
        self
    }

    /// Redact a JSON document in place, returning the redacted copy.
    #[must_use]
    pub fn redact(&self, value: &Value) -> Value {
        if self.rules.is_empty() && self.max_string == usize::MAX && !self.detect_values {
            return value.clone();
        }
        self.walk(value, 0, None)
    }

    fn walk(&self, value: &Value, depth: usize, inherited: Option<RedactionMode>) -> Value {
        if depth > self.max_depth {
            return Value::String("[too deeply nested]".to_owned());
        }
        match value {
            Value::Object(map) => {
                let mut out = Map::with_capacity(map.len());
                for (k, v) in map {
                    // The STRONGER of the two, not the innermost. `.or(inherited)`
                    // let a weaker rule on an inner key beat the mask inherited
                    // from a secret parent, so `{"password": {"email": ...}}`
                    // came back with four characters of the value under
                    // `password` still readable, because `email` names a Partial
                    // rule and Partial won. The comment below has always said
                    // the opposite was intended.
                    let mode = stronger(self.mode_for(k), inherited);
                    match (mode, v) {
                        (Some(RedactionMode::Drop), _) => {}
                        (Some(m), Value::String(s)) => {
                            out.insert(k.clone(), self.apply_to_string(m, s));
                        }
                        // A matched key holding a structure: redact everything
                        // underneath it, so `{"credentials": {"token": "..."}}`
                        // does not slip through on the nested key alone.
                        (Some(m), other) => {
                            out.insert(k.clone(), self.walk(other, depth + 1, Some(m)));
                        }
                        (None, other) => {
                            out.insert(k.clone(), self.walk(other, depth + 1, None));
                        }
                    }
                }
                Value::Object(out)
            }
            Value::Array(items) => {
                Value::Array(items.iter().map(|v| self.walk(v, depth + 1, inherited)).collect())
            }
            Value::String(s) => match inherited {
                Some(m) => self.apply_to_string(m, s),
                None if self.detect_values => match find_secrets(s) {
                    Some(clean) => self.truncate(&clean),
                    None => self.truncate(s),
                },
                None => self.truncate(s),
            },
            // A scalar under a secret key is still a secret. This arm used to
            // clone Number and Bool unchanged, so `{"password": 123456}` and a
            // card number sent as a JSON number were persisted verbatim while
            // the same value in quotes was masked. A PIN does not become
            // public by being typed without quotes.
            other => match inherited {
                Some(m) => self.apply_to_string(m, &other.to_string()),
                None => other.clone(),
            },
        }
    }

    /// Hash a bare identifier, for correlation fields rather than payloads.
    ///
    /// `user_id` is personal data in almost every deployment, but it is also
    /// the field you group by to find one customer's runs. Hashing it with a
    /// stable key keeps the grouping and loses the identity, which is the only
    /// version of this that is useful and defensible at once.
    ///
    /// Returns `None` unchanged so an absent field stays absent.
    #[must_use]
    pub fn hash_identifier(&self, value: Option<&str>) -> Option<String> {
        let v = value?;
        if self.rules.is_empty() {
            // A disabled redactor is a passthrough here too.
            return Some(v.to_owned());
        }
        Some(self.hash(v))
    }

    /// Redact one bare string of prose, the shape a comment or an aspect note
    /// arrives in.
    ///
    /// Every rule here matches on a **key**, and a sentence has none: nothing
    /// in `{"password": "hunter2"}` is visible when the same secret is written
    /// as `the user gave the password hunter2`. So what this applies is the
    /// length bound, which is the one protection that does not need a key, and
    /// it applies it through the same walk every other stored string takes
    /// rather than through a second copy of the rule.
    ///
    /// Two callers rely on that being said out loud. A piece of feedback holds
    /// a person's own words, and an aspect note holds a sentence about what a
    /// user asked for. Both are payload, both are swept by retention and erased
    /// with their subject, and neither can be protected by a key rule. A
    /// deployment that needs more than the bound has to keep the secret out of
    /// the sentence.
    #[must_use]
    pub fn redact_text(&self, s: &str) -> String {
        match self.redact(&Value::String(s.to_owned())) {
            Value::String(out) => out,
            // Unreachable: `walk` returns a string for a string. Returning the
            // input rather than panicking keeps a redaction slip out of the
            // ingest path, which must never fail on the shape of a payload.
            _ => s.to_owned(),
        }
    }

    /// Redact an optional field, the shape most model payloads arrive in.
    #[must_use]
    pub fn redact_opt(&self, value: &Option<Value>) -> Option<Value> {
        value.as_ref().map(|v| self.redact(v))
    }
}

/// The credential shapes searched for in every string value, with the label
/// each is replaced by.
///
/// A key rule only works when the secret sits under a key that names it. A
/// prompt that pastes a token into a sentence, a URL with a password in it and
/// a log field called `header` all carry a secret under a key that names
/// nothing, so these are matched by their shape instead. Each pattern needs a
/// long, high-entropy run with a fixed prefix, so ordinary prose does not
/// match; an email address is not on the list, because a prompt about a
/// customer's email is the common case and masking it would destroy the run.
fn secret_patterns() -> &'static [(regex::Regex, &'static str)] {
    static P: std::sync::OnceLock<Vec<(regex::Regex, &'static str)>> = std::sync::OnceLock::new();
    P.get_or_init(|| {
        [
            (
                r"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----",
                "private key",
            ),
            (r"\beyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}", "jwt"),
            (r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b", "aws key"),
            (r"\bsk-(?:ant-|proj-)?[A-Za-z0-9_-]{20,}", "api key"),
            (r"\bgh[pousr]_[A-Za-z0-9]{36,}\b", "github token"),
            (r"\bxox[abposr]-[A-Za-z0-9-]{10,}", "slack token"),
            (r"\bAIza[0-9A-Za-z_-]{35}\b", "google key"),
            (r"(?i)\bbearer\s+[A-Za-z0-9._~+/=-]{16,}", "bearer token"),
            (r"(?i)\b([a-z][a-z0-9+.-]*://)[^/\s:@]+:[^/\s@]+@", "url credentials"),
        ]
        .into_iter()
        .filter_map(|(p, label)| regex::Regex::new(p).ok().map(|r| (r, label)))
        .collect()
    })
}

/// The string with every credential shape replaced, or `None` when it holds
/// none, so the common case allocates nothing.
fn find_secrets(s: &str) -> Option<String> {
    // A cheap screen first: every pattern needs one of these somewhere in the
    // string, and nearly every string has none of them.
    const HINTS: [&str; 10] =
        ["PRIVATE KEY", "eyJ", "AKIA", "ASIA", "sk-", "gh", "xox", "AIza", "earer", "://"];
    if !HINTS.iter().any(|h| s.contains(h)) {
        return None;
    }
    let mut out = std::borrow::Cow::Borrowed(s);
    for (re, label) in secret_patterns() {
        if !re.is_match(&out) {
            continue;
        }
        let replaced = if *label == "url credentials" {
            re.replace_all(&out, "${1}[redacted]@").into_owned()
        } else {
            re.replace_all(&out, format!("[redacted {label}]").as_str()).into_owned()
        };
        out = std::borrow::Cow::Owned(replaced);
    }
    match out {
        std::borrow::Cow::Owned(o) => Some(o),
        std::borrow::Cow::Borrowed(_) => None,
    }
}

/// The stronger of two redaction modes, for a key that sits under another that
/// already matched.
///
/// Strength is how little of the value survives: `Drop` leaves nothing, `Mask`
/// leaves a fixed token, `Hash` leaves something unrecoverable but joinable, and
/// `Partial` deliberately leaves a readable prefix and suffix. Nesting must only
/// ever tighten, because the alternative is that adding a rule to an inner field
/// quietly weakens the protection on an outer one.
fn stronger(a: Option<RedactionMode>, b: Option<RedactionMode>) -> Option<RedactionMode> {
    // Hash and Partial share a rank deliberately. Neither dominates the other,
    // so ordering them would make one of the two cases wrong, and the tie is
    // handled below rather than decided arbitrarily here.
    const fn rank(m: RedactionMode) -> u8 {
        match m {
            RedactionMode::Drop => 2,
            RedactionMode::Mask => 1,
            RedactionMode::Hash | RedactionMode::Partial => 0,
        }
    }
    match (a, b) {
        (Some(x), Some(y)) if rank(x) == rank(y) && x != y => {
            // Hash and Partial are the only pair that ties, and neither
            // dominates: `partial` masks a value of six characters or fewer
            // entirely while `hash` never does, so Partial protects a short
            // value better, and Hash protects a long one better by revealing no
            // characters at all. This function cannot see the value, so it
            // cannot pick correctly.
            //
            // Both rules agreed the field is sensitive and disagreed only about
            // treatment, so take the treatment that reveals least either way.
            // Ranking Hash above Partial unconditionally was measured storing a
            // five-digit value as a digest that an audit recovered in under a
            // hundred thousand guesses, where the inner Partial rule would have
            // masked it. This costs a groupable digest in the rare case where an
            // outer and an inner rule both match with different modes, which is
            // the right thing to lose.
            Some(RedactionMode::Mask)
        }
        (Some(x), Some(y)) => Some(if rank(x) >= rank(y) { x } else { y }),
        (some, None) | (None, some) => some,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn masks_secrets_by_key_name() {
        let r = Redactor::with_defaults();
        let out = r.redact(&json!({ "api_key": "sk-live-abcdef", "model": "gpt-4o" }));
        assert_eq!(out["api_key"], json!("[redacted]"));
        assert_eq!(out["model"], json!("gpt-4o"), "non-secrets must survive untouched");
    }

    #[test]
    fn matching_is_case_insensitive_and_substring() {
        let r = Redactor::with_defaults();
        let out = r.redact(&json!({ "Authorization": "Bearer x", "X-API-Key": "y" }));
        assert_eq!(out["Authorization"], json!("[redacted]"));
        assert_eq!(out["X-API-Key"], json!("[redacted]"));
    }

    #[test]
    fn redacts_everything_under_a_matched_key() {
        // The nested value has an innocent key; it must still be redacted
        // because its parent matched.
        let r = Redactor::with_defaults();
        let out = r.redact(&json!({ "credential": { "value": "hunter2", "note": "prod" } }));
        assert_eq!(out["credential"]["value"], json!("[redacted]"));
        assert_eq!(out["credential"]["note"], json!("[redacted]"));
    }

    #[test]
    fn partial_keeps_enough_to_recognise_but_not_to_use() {
        let r = Redactor::with_defaults();
        let out = r.redact(&json!({ "email": "alice@example.com" }));
        assert_eq!(out["email"], json!("al…om"));
    }

    #[test]
    fn partial_masks_short_values_entirely() {
        let r = Redactor::with_defaults();
        let out = r.redact(&json!({ "phone": "12345" }));
        assert_eq!(out["phone"], json!("[redacted]"));
    }

    #[test]
    fn hashing_is_stable_and_groupable() {
        let r = Redactor::with_defaults();
        let a = r.redact(&json!({ "user_id": "u-1" }));
        let b = r.redact(&json!({ "user_id": "u-1" }));
        let c = r.redact(&json!({ "user_id": "u-2" }));
        assert_eq!(a["user_id"], b["user_id"], "same input must hash the same");
        assert_ne!(a["user_id"], c["user_id"]);
        assert!(a["user_id"].as_str().unwrap().starts_with("sha256:"));
    }

    #[test]
    fn rotating_the_key_breaks_the_link() {
        let a = Redactor::with_defaults().with_hash_key("key-a");
        let b = Redactor::with_defaults().with_hash_key("key-b");
        assert_ne!(a.redact(&json!({ "user_id": "u" })), b.redact(&json!({ "user_id": "u" })));
    }

    #[test]
    fn walks_arrays_and_nesting() {
        let r = Redactor::with_defaults();
        let out = r.redact(&json!({
            "messages": [
                { "role": "user", "content": "hi", "api_key": "leak" },
                { "role": "assistant", "content": "hello" }
            ]
        }));
        assert_eq!(out["messages"][0]["api_key"], json!("[redacted]"));
        assert_eq!(out["messages"][0]["content"], json!("hi"));
        assert_eq!(out["messages"][1]["content"], json!("hello"));
    }

    #[test]
    fn truncates_runaway_strings_on_a_char_boundary() {
        let r = Redactor::with_defaults().with_max_string(8);
        // Multi-byte characters: a naive slice here would panic.
        let out = r.redact(&json!({ "content": "ααααααααααα" }));
        let s = out["content"].as_str().unwrap();
        assert!(s.contains("truncated"), "got {s}");
    }

    #[test]
    fn bounds_pathological_nesting() {
        let mut v = json!("leaf");
        for _ in 0..200 {
            v = json!({ "n": v });
        }
        let r = Redactor::with_defaults();
        let out = r.redact(&v); // must return, not blow the stack
        assert!(out.is_object());
    }

    #[test]
    fn identifiers_hash_stably_and_pass_through_when_disabled() {
        let r = Redactor::with_defaults();
        let a = r.hash_identifier(Some("customer-9")).unwrap();
        let b = r.hash_identifier(Some("customer-9")).unwrap();
        assert_eq!(a, b, "grouping by a hashed id must still work");
        assert_ne!(a, "customer-9");
        assert!(a.starts_with("sha256:"));
        assert!(r.hash_identifier(None).is_none());
        assert_eq!(
            Redactor::disabled().hash_identifier(Some("customer-9")).as_deref(),
            Some("customer-9"),
        );
    }

    #[test]
    fn disabled_redactor_is_a_passthrough() {
        let r = Redactor::disabled();
        let v = json!({ "api_key": "sk-live" });
        assert_eq!(r.redact(&v), v);
    }
}

#[cfg(test)]
mod scalar_tests {
    use super::*;
    use serde_json::json;

    /// A PIN does not become public by being typed without quotes. This arm
    /// cloned Number and Bool unchanged, so a card number sent as a JSON number
    /// was persisted verbatim while the same value in quotes was masked.
    #[test]
    fn a_secret_under_a_secret_key_is_redacted_whatever_its_json_type() {
        let r = Redactor::with_defaults();
        let out = r.redact(&json!({
            "password": 123_456,
            "card_number": 4_111_111_111_111_111_i64,
            "api_key": true,
            "note": "kept"
        }));
        let s = out.to_string();
        assert!(!s.contains("123456"), "numeric password survived: {s}");
        assert!(!s.contains("4111111111111111"), "numeric card survived: {s}");
        assert!(s.contains("kept"), "an ordinary field must be untouched: {s}");
    }

    #[test]
    fn a_credential_in_prose_is_found_by_its_shape() {
        let r = Redactor::with_defaults();
        let text = "use key sk-ant-REDACTEDREDACTEDREDACTED1234 and token \
                    eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N \
                    against https://admin:hunter2@db.example.com/x with AKIAABCDEFGHIJKLMNOP";
        let out = r.redact_text(text);
        for gone in ["sk-ant-REDACTED", "eyJhbGci", "hunter2", "AKIAABCDEFGHIJKLMNOP"] {
            assert!(!out.contains(gone), "{gone} survived: {out}");
        }
        assert!(
            out.contains("[redacted api key]") && out.contains("https://[redacted]@db"),
            "{out}"
        );
        assert!(out.contains("[redacted jwt]") && out.contains("[redacted aws key]"), "{out}");
    }

    #[test]
    fn ordinary_prose_and_an_email_are_left_alone() {
        let r = Redactor::with_defaults();
        let text = "Ship order A-7741 to jane@example.com via https://example.com/track";
        assert_eq!(r.redact_text(text), text);
        let off = Redactor::with_defaults().with_value_detection(false);
        assert_eq!(
            off.redact_text("sk-ant-REDACTEDREDACTEDREDACTED1234"),
            "sk-ant-REDACTEDREDACTEDREDACTED1234"
        );
        assert_eq!(
            Redactor::disabled().redact_text("Bearer abcdefghijklmnopqrstuvwxyz"),
            "Bearer abcdefghijklmnopqrstuvwxyz"
        );
    }
}
