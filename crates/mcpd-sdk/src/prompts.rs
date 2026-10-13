//! Prompts: fetch a stored version, render it, and write new versions.
//!
//! A fetched prompt is the agent's input, not telemetry, so these calls work
//! on a disabled client and return their errors to the caller. The read side
//! still keeps the agent running through a daemon outage: a copy fetched once
//! is served however old it is when the next fetch fails (contract section
//! 5.1).

use crate::transport::Transport;
use crate::{Error, Mcpd};
use mcpd_core::{CreatePromptRequest, Prompt, PromptKind, PromptLabelRequest};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use time::OffsetDateTime;

/// How long a fetched prompt is served from memory before it is fetched again.
pub const DEFAULT_PROMPT_TTL: Duration = Duration::from_secs(60);

/// The label asked for when a call names neither a label nor a version.
pub const DEFAULT_PROMPT_LABEL: &str = "production";

/// The most prompts one client keeps in memory (B3).
const CACHE_CAPACITY: usize = 256;

/// What to return when a prompt cannot be fetched and no copy was fetched
/// before. It is never cached, so the next call tries the daemon again.
#[derive(Debug, Clone, PartialEq)]
pub enum PromptFallback {
    /// A template, returned as a `text` prompt.
    Template(String),
    /// A list of messages, each with a `role` and a `content`, returned as a
    /// `chat` prompt.
    Messages(Vec<Value>),
}

/// Which version [`Mcpd::get_prompt`] reads, and how it caches it.
///
/// ```
/// use mcpd_sdk::{GetPrompt, PromptFallback};
/// use std::time::Duration;
///
/// let options = GetPrompt::new()
///     .label("staging")
///     .cache_ttl(Duration::from_secs(10))
///     .fallback(PromptFallback::Template("Answer: {{question}}".into()));
/// assert_eq!(options.label.as_deref(), Some("staging"));
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct GetPrompt {
    /// Read the version this label points at. Not with `version`.
    pub label: Option<String>,
    /// Read this version. Not with `label`.
    pub version: Option<u32>,
    /// How long a fetched copy is served with no request. Zero fetches every
    /// time, and still serves the last copy when the fetch fails.
    pub cache_ttl: Duration,
    /// What to return when nothing can be fetched and nothing was before.
    pub fallback: Option<PromptFallback>,
}

impl Default for GetPrompt {
    fn default() -> Self {
        Self { label: None, version: None, cache_ttl: DEFAULT_PROMPT_TTL, fallback: None }
    }
}

impl GetPrompt {
    /// The default: the `production` label, or the latest version when no
    /// version carries it, cached for a minute, with no fallback.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Read the version this label points at.
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Read this version.
    #[must_use]
    pub fn version(mut self, version: u32) -> Self {
        self.version = Some(version);
        self
    }

    /// Serve a fetched copy for this long before fetching again.
    #[must_use]
    pub fn cache_ttl(mut self, ttl: Duration) -> Self {
        self.cache_ttl = ttl;
        self
    }

    /// Return this when nothing can be fetched and nothing was before.
    #[must_use]
    pub fn fallback(mut self, fallback: PromptFallback) -> Self {
        self.fallback = Some(fallback);
        self
    }
}

/// A rendered prompt: a string for a `text` prompt, messages for a `chat` one.
#[derive(Debug, Clone, PartialEq)]
pub enum Rendered {
    /// The template with every variable substituted.
    Text(String),
    /// A copy of the messages with every string `content` rendered.
    Messages(Vec<Value>),
}

impl Rendered {
    /// The text, for a `text` prompt.
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        match self {
            Self::Text(t) => Some(t),
            Self::Messages(_) => None,
        }
    }

    /// The messages, for a `chat` prompt.
    #[must_use]
    pub fn messages(&self) -> Option<&[Value]> {
        match self {
            Self::Text(_) => None,
            Self::Messages(m) => Some(m),
        }
    }
}

/// A prompt names variables the caller did not give. Sending it with a hole
/// in it would be worse than not sending it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "the prompt needs variables that were not given: {}; pass a value for each",
    .missing.join(", ")
)]
pub struct PromptRenderError {
    /// Every missing variable, in order of first use.
    pub missing: Vec<String>,
}

/// Render a prompt with variables, by the daemon's own placeholder rule.
///
/// A placeholder is `{{name}}`, with whitespace allowed inside the braces. A
/// placeholder that is not a name, and a `{{` that is never closed, stay in
/// the output as written. Substitution is one pass, so a value containing
/// `{{x}}` is not rendered again. Extra variables are ignored.
///
/// ```
/// use mcpd_sdk::{render_prompt, Rendered};
/// # let prompt: mcpd_sdk::model::Prompt = serde_json::from_value(serde_json::json!({
/// #     "project": "p", "name": "greet", "version": 1, "kind": "text",
/// #     "template": "Hello {{ name }}", "variables": ["name"], "labels": [],
/// #     "created_at": "2026-01-01T00:00:00Z",
/// # })).unwrap();
/// let rendered = render_prompt(&prompt, [("name", "Ada")]).unwrap();
/// assert_eq!(rendered, Rendered::Text("Hello Ada".into()));
/// ```
///
/// # Errors
/// [`PromptRenderError`] naming every variable the prompt uses and
/// `variables` lacks.
pub fn render_prompt<I, K, V>(prompt: &Prompt, variables: I) -> Result<Rendered, PromptRenderError>
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<String>,
    V: Into<String>,
{
    let vars: HashMap<String, String> =
        variables.into_iter().map(|(k, v)| (k.into(), v.into())).collect();
    let mut missing = Vec::new();
    let rendered = match prompt.kind {
        PromptKind::Text => Rendered::Text(substitute(
            prompt.template.as_deref().unwrap_or(""),
            &vars,
            &mut missing,
        )),
        PromptKind::Chat => {
            let messages = match &prompt.messages {
                Some(Value::Array(m)) => m.as_slice(),
                _ => &[],
            };
            Rendered::Messages(
                messages
                    .iter()
                    .map(|m| {
                        let mut m = m.clone();
                        if let Some(Value::String(content)) = m.get_mut("content") {
                            *content = substitute(content, &vars, &mut missing);
                        }
                        m
                    })
                    .collect(),
            )
        }
    };
    if missing.is_empty() {
        Ok(rendered)
    } else {
        Err(PromptRenderError { missing })
    }
}

/// Whether the text between a pair of braces is a variable name. The same
/// rule as the daemon's, so the SDK renders exactly the variables the daemon
/// lists.
fn is_name(name: &str) -> bool {
    name.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
}

/// One pass over a template. Walks placeholders the way the daemon's
/// `variables_in` does, so a brace pair the daemon skips is skipped here too.
fn substitute(template: &str, vars: &HashMap<String, String>, missing: &mut Vec<String>) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find("{{") {
        let after = &rest[open + 2..];
        let Some(close) = after.find("}}") else { break };
        let name = after[..close].trim();
        out.push_str(&rest[..open]);
        if is_name(name) {
            match vars.get(name) {
                Some(value) => out.push_str(value),
                None => {
                    if !missing.iter().any(|m| m == name) {
                        missing.push(name.to_owned());
                    }
                }
            }
        } else {
            out.push_str(&rest[open..open + 2 + close + 2]);
        }
        rest = &after[close + 2..];
    }
    out.push_str(rest);
    out
}

/// Every variable a template names, in order of first use: the daemon's
/// `variables_in`, for a fallback the daemon never saw.
fn variables_in(template: &str, out: &mut Vec<String>) {
    let mut rest = template;
    while let Some(open) = rest.find("{{") {
        let after = &rest[open + 2..];
        let Some(close) = after.find("}}") else { break };
        let name = after[..close].trim();
        if is_name(name) && !out.iter().any(|v| v == name) {
            out.push(name.to_owned());
        }
        rest = &after[close + 2..];
    }
}

fn variables_in_value(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) => variables_in(s, out),
        Value::Array(a) => a.iter().for_each(|x| variables_in_value(x, out)),
        Value::Object(o) => o.values().for_each(|x| variables_in_value(x, out)),
        _ => {}
    }
}

/// A fallback as a prompt: version 0, which no stored version has, so a span
/// linked to it is not mistaken for one that used a stored version.
fn fallback_prompt(project: &str, name: &str, fallback: &PromptFallback) -> Prompt {
    let mut variables = Vec::new();
    let (kind, template, messages) = match fallback {
        PromptFallback::Template(t) => {
            variables_in(t, &mut variables);
            (PromptKind::Text, Some(t.clone()), None)
        }
        PromptFallback::Messages(m) => {
            let m = Value::Array(m.clone());
            variables_in_value(&m, &mut variables);
            (PromptKind::Chat, None, Some(m))
        }
    };
    Prompt {
        project: project.to_owned(),
        name: name.to_owned(),
        version: 0,
        kind,
        template,
        messages,
        config: None,
        variables,
        labels: Vec::new(),
        note: None,
        created_by: None,
        created_at: OffsetDateTime::now_utc(),
    }
}

/// Which version a cached copy answers for. The default is its own key: it
/// may be the `production` version or the latest, and caching it under either
/// would answer a later explicit request with the wrong one.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Selector {
    Default,
    Label(String),
    Version(u32),
}

type Key = (String, String, Selector);

struct Cached {
    prompt: Prompt,
    fetched: Instant,
    /// Insertion order. Two fetches can share an `Instant`, so eviction
    /// orders by this instead.
    seq: u64,
}

#[derive(Default)]
struct Cache {
    entries: HashMap<Key, Cached>,
    next: u64,
}

/// Each client's prompt cache, and the transport its prompt calls use.
#[derive(Default)]
pub(crate) struct PromptStore {
    cache: Mutex<Cache>,
    /// Built on first use by a disabled client, which has no transport of its
    /// own: a disabled client still needs its prompts.
    transport: OnceLock<Result<Arc<Transport>, String>>,
}

impl PromptStore {
    fn lock(&self) -> std::sync::MutexGuard<'_, Cache> {
        // The map is only ever replaced whole entry by whole entry, so a panic
        // elsewhere cannot leave it half written.
        self.cache.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn fresh(&self, key: &Key, ttl: Duration) -> Option<Prompt> {
        let cache = self.lock();
        let c = cache.entries.get(key)?;
        (c.fetched.elapsed() < ttl).then(|| c.prompt.clone())
    }

    fn any(&self, key: &Key) -> Option<Prompt> {
        self.lock().entries.get(key).map(|c| c.prompt.clone())
    }

    fn store(&self, key: Key, prompt: Prompt) {
        let mut cache = self.lock();
        if cache.entries.len() >= CACHE_CAPACITY && !cache.entries.contains_key(&key) {
            let oldest = cache.entries.iter().min_by_key(|(_, c)| c.seq).map(|(k, _)| k.clone());
            if let Some(oldest) = oldest {
                cache.entries.remove(&oldest);
            }
        }
        let seq = cache.next;
        cache.next += 1;
        cache.entries.insert(key, Cached { prompt, fetched: Instant::now(), seq });
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.lock().entries.len()
    }
}

/// A path segment with everything but the unreserved characters escaped. A
/// prompt name may contain `/`, and sent bare it would address another route.
pub(crate) fn segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

impl Mcpd {
    /// The transport prompt calls use, which a disabled client also has.
    fn prompt_transport(&self) -> Result<&Transport, Error> {
        if let Ok(t) = self.transport() {
            return Ok(t);
        }
        self.inner
            .prompts
            .transport
            .get_or_init(|| {
                Transport::new(&self.inner.config).map(Arc::new).map_err(|e| e.to_string())
            })
            .as_deref()
            .map_err(|e| Error::Config(e.clone()))
    }

    /// Fetch a prompt: the version a label points at, a numbered version, or
    /// by default the `production` label, falling back to the latest version
    /// when no version carries it.
    ///
    /// A copy younger than `cache_ttl` is returned with no request. When a
    /// fetch fails, the last copy is returned however old it is, and the
    /// failure is logged; with no copy, the fallback is returned if one was
    /// given. Works on a disabled client.
    ///
    /// # Errors
    /// [`Error::Config`] when both a label and a version are given, or the
    /// name is empty. Otherwise the fetch error, when there is neither a copy
    /// nor a fallback to return.
    pub async fn get_prompt(&self, name: &str, options: &GetPrompt) -> Result<Prompt, Error> {
        let selector = match (&options.label, options.version) {
            (Some(_), Some(_)) => {
                return Err(Error::Config(format!(
                    "get_prompt({name:?}) was given both a label and a version; give one, or \
                     neither for the `{DEFAULT_PROMPT_LABEL}` label"
                )));
            }
            (Some(label), None) => Selector::Label(label.clone()),
            (None, Some(version)) => Selector::Version(version),
            (None, None) => Selector::Default,
        };
        if name.is_empty() {
            return Err(Error::Config("get_prompt needs a prompt name".to_owned()));
        }
        let project = self.project().to_owned();
        let key = (project.clone(), name.to_owned(), selector);
        let store = &self.inner.prompts;
        if let Some(fresh) = store.fresh(&key, options.cache_ttl) {
            return Ok(fresh);
        }
        let failure = match self.fetch_prompt(name, &key.2).await {
            Ok(prompt) => {
                store.store(key, prompt.clone());
                return Ok(prompt);
            }
            Err(e) => e,
        };
        if let Some(stale) = store.any(&key) {
            if self.inner.warn.allow() {
                tracing::warn!(
                    prompt = name,
                    error = %failure,
                    "mcpd could not fetch the prompt; serving the copy fetched before"
                );
            }
            return Ok(stale);
        }
        if let Some(fallback) = &options.fallback {
            if self.inner.warn.allow() {
                tracing::warn!(
                    prompt = name,
                    error = %failure,
                    "mcpd could not fetch the prompt; serving the fallback"
                );
            }
            return Ok(fallback_prompt(&project, name, fallback));
        }
        Err(failure)
    }

    async fn fetch_prompt(&self, name: &str, selector: &Selector) -> Result<Prompt, Error> {
        let transport = self.prompt_transport()?;
        let path = format!("/v1/api/prompts/{}", segment(name));
        let mut params = vec![("project", self.project().to_owned())];
        match selector {
            Selector::Label(label) => params.push(("label", label.clone())),
            Selector::Version(version) => params.push(("version", version.to_string())),
            Selector::Default => {
                params.push(("label", DEFAULT_PROMPT_LABEL.to_owned()));
                match transport.get(&path, &params).await {
                    // No version carries the label yet: the latest is the
                    // answer, so a prompt works before anyone releases it.
                    Err(Error::Api { status: 404, .. }) => params.truncate(1),
                    other => return other,
                }
            }
        }
        transport.get(&path, &params).await
    }

    /// Store a new version of a prompt in this client's project and return it.
    /// A body identical to the latest version stores nothing and returns that
    /// version, so a program can post its prompts on every start.
    ///
    /// # Errors
    /// [`Error::Api`] when mcpd refuses the body, [`Error::Http`] when it
    /// cannot be reached.
    pub async fn create_prompt(&self, request: &CreatePromptRequest) -> Result<Prompt, Error> {
        self.prompt_transport()?
            .post("/v1/api/prompts", &[("project", self.project().to_owned())], request)
            .await
    }

    /// Point a label at a version. Moving `production` releases a version and
    /// moving it back rolls one back. The read cache is not cleared, so a
    /// running agent sees the move within its `cache_ttl`.
    ///
    /// # Errors
    /// [`Error::Api`] with status 404 when the prompt or the version does not
    /// exist, [`Error::Http`] when mcpd cannot be reached.
    pub async fn set_prompt_label(
        &self,
        name: &str,
        label: &str,
        version: u32,
    ) -> Result<Prompt, Error> {
        let path = format!("/v1/api/prompts/{}/labels/{}", segment(name), segment(label));
        self.prompt_transport()?
            .put(&path, &[("project", self.project().to_owned())], &PromptLabelRequest { version })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn prompt(kind: PromptKind, template: Option<&str>, messages: Option<Value>) -> Prompt {
        Prompt {
            project: "p".into(),
            name: "n".into(),
            version: 1,
            kind,
            template: template.map(str::to_owned),
            messages,
            config: None,
            variables: Vec::new(),
            labels: Vec::new(),
            note: None,
            created_by: None,
            created_at: OffsetDateTime::UNIX_EPOCH,
        }
    }

    fn text(template: &str, vars: &[(&str, &str)]) -> Result<String, PromptRenderError> {
        render_prompt(&prompt(PromptKind::Text, Some(template), None), vars.iter().copied())
            .map(|r| r.text().unwrap_or_default().to_owned())
    }

    #[test]
    fn whitespace_inside_the_braces_names_the_same_variable() {
        let out = text("{{city}}, {{ city }}, {{\tcity\n}}", &[("city", "Oslo")]);
        assert_eq!(out.unwrap(), "Oslo, Oslo, Oslo");
    }

    #[test]
    fn a_placeholder_that_is_not_a_name_is_left_as_written() {
        let t = r#"{{ "a": 1 }} {{9lives}} {{}} {{a-b}} {{ok}} {{ never closed"#;
        assert_eq!(
            text(t, &[("ok", "yes")]).unwrap(),
            r#"{{ "a": 1 }} {{9lives}} {{}} {{a-b}} yes {{ never closed"#
        );
    }

    #[test]
    fn dots_and_underscores_are_part_of_a_name() {
        assert_eq!(
            text("{{user.name}} {{_x}}", &[("user.name", "Ada"), ("_x", "1")]).unwrap(),
            "Ada 1"
        );
    }

    #[test]
    fn substitution_is_one_pass() {
        let out = text("{{a}} {{b}}", &[("a", "{{b}}"), ("b", "B")]);
        assert_eq!(out.unwrap(), "{{b}} B");
    }

    #[test]
    fn every_missing_variable_is_named_in_order_of_first_use() {
        let err = text("{{z}} {{a}} {{z}} {{given}} {{m}}", &[("given", "x"), ("extra", "y")])
            .unwrap_err();
        assert_eq!(err.missing, ["z", "a", "m"]);
        let message = err.to_string();
        assert!(message.contains("z, a, m"), "{message}");
    }

    #[test]
    fn chat_content_is_rendered_and_every_other_field_kept() {
        let messages = json!([
            { "role": "system", "content": "You help {{ who }}.", "name": "{{who}}" },
            { "role": "user", "content": [{ "type": "text", "text": "{{who}}" }] },
            "not an object",
        ]);
        let p = prompt(PromptKind::Chat, None, Some(messages));
        let out = render_prompt(&p, [("who", "Ada")]).unwrap();
        assert_eq!(
            out.messages().unwrap(),
            [
                json!({ "role": "system", "content": "You help Ada.", "name": "{{who}}" }),
                json!({ "role": "user", "content": [{ "type": "text", "text": "{{who}}" }] }),
                json!("not an object"),
            ]
        );
        let missing = render_prompt(&p, Vec::<(String, String)>::new()).unwrap_err();
        assert_eq!(missing.missing, ["who"]);
    }

    #[test]
    fn a_fallback_lists_its_variables_by_the_daemons_rule() {
        let p = fallback_prompt(
            "shop",
            "greet",
            &PromptFallback::Template("{{a}} {{ b }} {{a}} {{1}}".into()),
        );
        assert_eq!(
            (p.version, p.kind, p.variables.as_slice()),
            (0, PromptKind::Text, &["a".to_owned(), "b".to_owned()][..])
        );
        assert_eq!(p.project, "shop");
        assert!(p.labels.is_empty());
        let p = fallback_prompt(
            "shop",
            "greet",
            &PromptFallback::Messages(vec![json!({ "role": "user", "content": "{{q}}" })]),
        );
        assert_eq!((p.kind, p.variables.as_slice()), (PromptKind::Chat, &["q".to_owned()][..]));
    }

    #[test]
    fn the_cache_holds_its_bound_and_evicts_the_oldest() {
        let store = PromptStore::default();
        let key = |i: usize| ("p".to_owned(), format!("n{i}"), Selector::Default);
        for i in 0..=CACHE_CAPACITY {
            store.store(key(i), prompt(PromptKind::Text, Some("x"), None));
        }
        assert_eq!(store.len(), CACHE_CAPACITY);
        assert!(store.any(&key(0)).is_none(), "the oldest entry should have gone");
        assert!(store.any(&key(CACHE_CAPACITY)).is_some());
    }

    #[test]
    fn a_path_segment_escapes_everything_that_could_change_the_route() {
        assert_eq!(segment("support/triage v2"), "support%2Ftriage%20v2");
        assert_eq!(segment("a-b_c.d~e"), "a-b_c.d~e");
    }
}
