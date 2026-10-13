//! What a forwarding destination is, as an operator writes it, and what it
//! resolves to once the vendor's preset is applied.
//!
//! A destination names a vendor and the few values that vendor needs: a key,
//! a region, a project. The preset turns those into the endpoint, the headers
//! and the attribute conventions the vendor documents, so an operator never
//! has to look up which header Honeycomb wants or which path Langfuse serves
//! OTLP on. Anything the preset decides can still be overridden with
//! `endpoint`, `protocol`, `headers` and `conventions`.
//!
//! Any string may name an environment variable as `${NAME}`. That is how a
//! credential stays out of a config file that is committed or mounted from a
//! ConfigMap: the file says where the key is, and the process environment
//! holds it.

use super::model::{AnyValue, Attrs, Signal};
use mcpd_core::SpanKind;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Where a destination sends, and in what shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Vendor {
    /// Any OTLP/HTTP receiver. JSON unless `protocol` says otherwise.
    #[default]
    #[serde(alias = "otlp_http")]
    Otlp,
    /// Any OTLP/gRPC receiver.
    OtlpGrpc,
    /// Langfuse, cloud or self-hosted.
    Langfuse,
    /// LangSmith.
    Langsmith,
    /// Braintrust.
    Braintrust,
    /// Arize AX.
    Arize,
    /// Arize Phoenix, cloud or self-hosted.
    Phoenix,
    /// Datadog LLM Observability.
    Datadog,
    /// Honeycomb.
    Honeycomb,
    /// New Relic.
    #[serde(alias = "new_relic")]
    Newrelic,
    /// Grafana Cloud, through its OTLP gateway.
    Grafana,
    /// Sentry.
    Sentry,
    /// Weights & Biases Weave.
    #[serde(alias = "wandb")]
    Weave,
    /// Laminar.
    Laminar,
    /// Helicone, through its logging API.
    Helicone,
    /// Comet Opik.
    #[serde(alias = "comet")]
    Opik,
    /// Elastic Observability.
    Elastic,
    /// Dynatrace.
    Dynatrace,
}

impl Vendor {
    /// Every vendor, for documentation and tests.
    pub const ALL: &'static [Self] = &[
        Self::Otlp,
        Self::OtlpGrpc,
        Self::Langfuse,
        Self::Langsmith,
        Self::Braintrust,
        Self::Arize,
        Self::Phoenix,
        Self::Datadog,
        Self::Honeycomb,
        Self::Newrelic,
        Self::Grafana,
        Self::Sentry,
        Self::Weave,
        Self::Laminar,
        Self::Helicone,
        Self::Opik,
        Self::Elastic,
        Self::Dynatrace,
    ];

    /// The name a config file uses.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Otlp => "otlp",
            Self::OtlpGrpc => "otlp_grpc",
            Self::Langfuse => "langfuse",
            Self::Langsmith => "langsmith",
            Self::Braintrust => "braintrust",
            Self::Arize => "arize",
            Self::Phoenix => "phoenix",
            Self::Datadog => "datadog",
            Self::Honeycomb => "honeycomb",
            Self::Newrelic => "newrelic",
            Self::Grafana => "grafana",
            Self::Sentry => "sentry",
            Self::Weave => "weave",
            Self::Laminar => "laminar",
            Self::Helicone => "helicone",
            Self::Opik => "opik",
            Self::Elastic => "elastic",
            Self::Dynatrace => "dynatrace",
        }
    }

    /// Parse the name a flag carries.
    ///
    /// # Errors
    /// Names every accepted vendor when the name is not one of them.
    pub fn parse(s: &str) -> Result<Self, String> {
        let wanted = s.trim().to_ascii_lowercase().replace('-', "_");
        serde_json::from_value(serde_json::Value::String(wanted)).map_err(|_| {
            format!(
                "{s:?} is not a forwarding vendor; use one of {}",
                Self::ALL.iter().map(|v| v.as_str()).collect::<Vec<_>>().join(", ")
            )
        })
    }

    /// The signals this vendor's intake accepts.
    #[must_use]
    pub const fn signals(self) -> &'static [Signal] {
        use Signal::{Logs, Metrics, Traces};
        match self {
            Self::Otlp
            | Self::OtlpGrpc
            | Self::Honeycomb
            | Self::Newrelic
            | Self::Grafana
            | Self::Elastic
            | Self::Dynatrace => &[Traces, Logs, Metrics],
            Self::Sentry => &[Traces, Logs],
            _ => &[Traces],
        }
    }

    /// The attribute conventions this vendor reads.
    #[must_use]
    pub const fn conventions(self) -> Conventions {
        match self {
            Self::Langfuse => Conventions::Langfuse,
            Self::Langsmith => Conventions::Langsmith,
            Self::Braintrust => Conventions::Braintrust,
            Self::Arize | Self::Phoenix | Self::Opik | Self::Weave => Conventions::Openinference,
            Self::Laminar => Conventions::Laminar,
            Self::Datadog => Conventions::GenAiMessages,
            _ => Conventions::GenAi,
        }
    }
}

/// How a request body is encoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    /// OTLP/HTTP with a JSON body.
    Json,
    /// OTLP/HTTP with a protobuf body.
    Protobuf,
    /// OTLP over gRPC.
    Grpc,
    /// The vendor's own HTTP API. Only Helicone, which has no OTLP intake.
    Api,
}

impl Protocol {
    /// The name a config file uses.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Protobuf => "protobuf",
            Self::Grpc => "grpc",
            Self::Api => "api",
        }
    }

    /// Parse the name a flag carries.
    ///
    /// # Errors
    /// Names the accepted protocols.
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "json" | "http/json" => Ok(Self::Json),
            "protobuf" | "proto" | "http/protobuf" => Ok(Self::Protobuf),
            "grpc" => Ok(Self::Grpc),
            other => {
                Err(format!("{other:?} is not a forwarding protocol; use json, protobuf or grpc"))
            }
        }
    }
}

/// Which semantic-convention attributes a span is written with.
///
/// Every set carries the OpenTelemetry GenAI attributes and mcpd's own, so a
/// span forwarded to another mcpd loses nothing. The vendor sets add the
/// namespace that vendor reads its fields from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Conventions {
    /// `gen_ai.*`, with prompts in `input.value` and `output.value`.
    GenAi,
    /// `gen_ai.*`, with prompts as `gen_ai.input.messages` and
    /// `gen_ai.output.messages`.
    GenAiMessages,
    /// OpenInference: `openinference.span.kind`, `llm.*`, `input.value`.
    Openinference,
    /// `langfuse.*`.
    Langfuse,
    /// `langsmith.*`.
    Langsmith,
    /// `braintrust.*`.
    Braintrust,
    /// `lmnr.*`.
    Laminar,
}

/// Which spans a destination receives.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Filter {
    /// Only these projects. Empty means every project.
    #[serde(default)]
    pub projects: Vec<String>,
    /// Only these environments. Empty means every environment, including
    /// signals that name none.
    #[serde(default)]
    pub environments: Vec<String>,
    /// Only spans of these kinds. Empty means every kind. Logs and metrics
    /// have no kind and are not held to this.
    #[serde(default)]
    pub kinds: Vec<SpanKind>,
    /// The fraction of traces to send, 0.0 to 1.0. Decided from the trace id,
    /// so a trace is sent whole or not at all, and two destinations with the
    /// same fraction send the same traces. Logs inside a trace follow it.
    #[serde(default)]
    pub sample: Option<f64>,
}

/// What a destination is not sent, on top of what mcpd already redacted
/// before storing.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Redact {
    /// Send no prompts or completions: span input and output, and the fields
    /// of a log line, are left out.
    #[serde(default)]
    pub drop_payloads: bool,
    /// Mask any attribute, input or output value whose key contains one of
    /// these, matched on lowercase substrings.
    #[serde(default)]
    pub key_contains: Vec<String>,
}

/// Which signals a destination receives. Unset means every signal the vendor
/// accepts.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Signals {
    /// Spans.
    #[serde(default)]
    pub traces: Option<bool>,
    /// Log records.
    #[serde(default)]
    pub logs: Option<bool>,
    /// Metric points.
    #[serde(default)]
    pub metrics: Option<bool>,
}

/// One forwarding destination, as written in the config file.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Destination {
    /// A name for this destination, used in metrics, logs and `mcpd doctor`.
    /// Defaults to the vendor's name.
    #[serde(default)]
    pub name: Option<String>,
    /// Which vendor's preset to apply.
    #[serde(default)]
    pub vendor: Vendor,
    /// The base URL, without the `/v1/traces` path. Required for `otlp`,
    /// `otlp_grpc`, `elastic` and `dynatrace`; overrides the preset otherwise.
    #[serde(default)]
    pub endpoint: Option<String>,
    /// The body encoding. Defaults to the vendor's.
    #[serde(default)]
    pub protocol: Option<Protocol>,
    /// Extra headers. One with the same name as a preset header replaces it.
    #[serde(default, deserialize_with = "headers")]
    pub headers: Vec<(String, String)>,
    /// Stop sending without removing the configuration.
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Most items in one request.
    #[serde(default = "default_batch")]
    pub batch_size: usize,
    /// Which conventions spans are written with. Defaults to the vendor's.
    #[serde(default)]
    pub conventions: Option<Conventions>,
    /// The vendor's region or site, such as `eu` or `datadoghq.eu`.
    #[serde(default)]
    pub region: Option<String>,
    /// The vendor's API key.
    #[serde(default)]
    pub api_key: Option<String>,
    /// A bearer token, for Elastic's secret token or Dynatrace's API token.
    #[serde(default)]
    pub token: Option<String>,
    /// Langfuse's public key.
    #[serde(default)]
    pub public_key: Option<String>,
    /// Langfuse's secret key.
    #[serde(default)]
    pub secret_key: Option<String>,
    /// Grafana Cloud's instance id, the user half of its basic auth.
    #[serde(default)]
    pub instance_id: Option<String>,
    /// Arize's space id.
    #[serde(default)]
    pub space_id: Option<String>,
    /// The vendor's project: a LangSmith or Braintrust project, an Arize or
    /// Phoenix project, an Opik project, or `entity/project` on Weave.
    #[serde(default)]
    pub project: Option<String>,
    /// Opik's workspace.
    #[serde(default)]
    pub workspace: Option<String>,
    /// Honeycomb's dataset, for logs and metrics.
    #[serde(default)]
    pub dataset: Option<String>,
    /// Sentry's DSN.
    #[serde(default)]
    pub dsn: Option<String>,
    /// Which spans are sent.
    #[serde(default)]
    pub filter: Filter,
    /// What is held back from this destination.
    #[serde(default)]
    pub redact: Redact,
    /// Which signals are sent.
    #[serde(default)]
    pub signals: Signals,
    /// The most this destination's spool may hold on disk, in bytes. Defaults
    /// to `--spool-max-bytes`.
    #[serde(default)]
    pub spool_max_bytes: Option<u64>,
}

impl std::fmt::Debug for Destination {
    /// Names and shape only. Every credential field, every header value and
    /// the endpoint's user, password and query are left out, because a
    /// `Debug` print ends up in a panic message or a log line sooner or later.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Destination")
            .field("name", &self.name)
            .field("vendor", &self.vendor)
            .field("endpoint", &self.endpoint.as_deref().map(shown_url))
            .field("protocol", &self.protocol)
            .field("headers", &self.headers.iter().map(|(k, _)| k).collect::<Vec<_>>())
            .field("enabled", &self.enabled)
            .field("batch_size", &self.batch_size)
            .field("region", &self.region)
            .field("project", &self.project)
            .finish_non_exhaustive()
    }
}

fn yes() -> bool {
    true
}
fn default_batch() -> usize {
    256
}

/// Headers as a list of pairs, which is how earlier configs wrote them, or as
/// an object of names to values.
fn headers<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<(String, String)>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Either {
        Pairs(Vec<(String, String)>),
        Map(std::collections::BTreeMap<String, String>),
    }
    Ok(match Either::deserialize(d)? {
        Either::Pairs(p) => p,
        Either::Map(m) => m.into_iter().collect(),
    })
}

impl Destination {
    /// A destination with every default, sending to `endpoint` as the
    /// vendor's preset says.
    #[must_use]
    pub fn new(vendor: Vendor, endpoint: Option<String>) -> Self {
        Self {
            name: None,
            vendor,
            endpoint,
            protocol: None,
            headers: Vec::new(),
            enabled: true,
            batch_size: default_batch(),
            conventions: None,
            region: None,
            api_key: None,
            token: None,
            public_key: None,
            secret_key: None,
            instance_id: None,
            space_id: None,
            project: None,
            workspace: None,
            dataset: None,
            dsn: None,
            filter: Filter::default(),
            redact: Redact::default(),
            signals: Signals::default(),
            spool_max_bytes: None,
        }
    }

    /// The name this destination goes by.
    #[must_use]
    pub fn name(&self) -> String {
        self.name.clone().unwrap_or_else(|| self.vendor.as_str().to_owned())
    }

    /// The endpoint as it is safe to log: no user, password, query or
    /// fragment. A collector's URL often carries its credential in one of
    /// those, and every line that names a destination goes to whoever reads
    /// the logs.
    #[must_use]
    pub fn shown_endpoint(&self) -> String {
        match self.resolve() {
            Ok(r) => shown_url(&r.base),
            Err(_) => self.endpoint.as_deref().map_or_else(|| "no endpoint".to_owned(), shown_url),
        }
    }

    /// The URL spans are sent to, as it is safe to log.
    #[must_use]
    pub fn traces_url(&self) -> String {
        match self.resolve() {
            Ok(r) => shown_url(&r.url(Signal::Traces)),
            Err(_) => self.shown_endpoint(),
        }
    }

    /// Apply the vendor preset and read every `${NAME}` from the environment.
    ///
    /// # Errors
    /// Says which value is missing or malformed, and how to supply it. The
    /// message never carries a credential.
    pub fn resolve(&self) -> Result<Resolved, String> {
        resolve(self, &|name| std::env::var(name).ok())
    }
}

/// A destination once its preset is applied: everything a sender needs.
#[derive(Clone)]
pub struct Resolved {
    /// The destination's name.
    pub name: String,
    /// The vendor.
    pub vendor: Vendor,
    /// The base URL.
    pub base: String,
    /// How bodies are encoded.
    pub protocol: Protocol,
    /// Every header to send, the credential among them.
    pub headers: Vec<(String, String)>,
    /// Resource attributes the vendor needs on every request.
    pub resource: Attrs,
    /// Which attributes spans are written with.
    pub conventions: Conventions,
    /// The signals sent.
    pub signals: Vec<Signal>,
    /// The longest string attribute sent, in bytes.
    pub max_value_bytes: usize,
}

impl std::fmt::Debug for Resolved {
    /// Header names only, and the base URL without credentials: a header
    /// value is how the destination authenticates.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Resolved")
            .field("name", &self.name)
            .field("vendor", &self.vendor)
            .field("base", &shown_url(&self.base))
            .field("protocol", &self.protocol)
            .field("headers", &self.headers.iter().map(|(k, _)| k).collect::<Vec<_>>())
            .field("signals", &self.signals)
            .finish_non_exhaustive()
    }
}

impl Resolved {
    /// The URL one signal is posted to.
    #[must_use]
    pub fn url(&self, signal: Signal) -> String {
        let base = self.base.trim_end_matches('/');
        match self.protocol {
            Protocol::Grpc => format!("{base}{}", signal.grpc_path()),
            Protocol::Api => format!("{base}/custom/v1/log"),
            Protocol::Json | Protocol::Protobuf => format!("{base}/v1/{}", signal.as_str()),
        }
    }

    /// Whether this destination receives `signal`.
    #[must_use]
    pub fn wants(&self, signal: Signal) -> bool {
        self.signals.contains(&signal)
    }
}

/// A URL with no user, password, query or fragment.
#[must_use]
pub fn shown_url(raw: &str) -> String {
    match reqwest::Url::parse(raw) {
        Ok(mut u) => {
            let _ = u.set_username("");
            let _ = u.set_password(None);
            u.set_query(None);
            u.set_fragment(None);
            u.to_string()
        }
        Err(_) => "an endpoint that is not a URL".to_owned(),
    }
}

/// Replace every `${NAME}` in `raw` with that variable's value.
fn expand(
    raw: &str,
    field: &str,
    dest: &str,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<String, String> {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(at) = rest.find("${") {
        out.push_str(&rest[..at]);
        let after = &rest[at + 2..];
        let Some(end) = after.find('}') else {
            return Err(format!(
                "forward destination {dest:?}: {field} has a `${{` with no closing `}}`"
            ));
        };
        let name = &after[..end];
        match env(name) {
            Some(v) if !v.is_empty() => out.push_str(&v),
            _ => {
                return Err(format!(
                    "forward destination {dest:?}: {field} names the environment variable \
                     {name}, which is not set; export it, or write the value in place"
                ))
            }
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Base64, standard alphabet with padding, for the two vendors that take
/// basic auth. Thirty lines here are cheaper than a crate for one call.
pub(crate) fn base64(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { ALPHABET[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { ALPHABET[n as usize & 63] as char } else { '=' });
    }
    out
}

/// The preset logic, with the environment passed in so tests need not set
/// process-wide variables.
#[allow(clippy::too_many_lines)]
pub(crate) fn resolve(
    d: &Destination,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<Resolved, String> {
    let name = d.name();
    if name.trim().is_empty()
        || !name.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
    {
        return Err(format!(
            "forward destination name {name:?} must be letters, digits, `-`, `_` or `.`; it \
             becomes a metric label"
        ));
    }
    let get = |field: &str, v: &Option<String>| -> Result<Option<String>, String> {
        v.as_deref().map(|s| expand(s, field, &name, env)).transpose()
    };
    let need = |field: &str, v: Option<String>, what: &str| -> Result<String, String> {
        v.ok_or_else(|| {
            format!("forward destination {name:?} ({}) needs `{field}`: {what}", d.vendor.as_str())
        })
    };

    let endpoint = get("endpoint", &d.endpoint)?;
    let region = get("region", &d.region)?.map(|r| r.trim().to_ascii_lowercase());
    let api_key = get("api_key", &d.api_key)?;
    let token = get("token", &d.token)?;
    let project = get("project", &d.project)?;
    let mut headers: Vec<(String, String)> = Vec::new();
    let mut resource: Attrs = Vec::new();
    let region_is = |accepted: &[&str]| -> Result<String, String> {
        let r = region.clone().unwrap_or_else(|| accepted[0].to_owned());
        if accepted.contains(&r.as_str()) {
            Ok(r)
        } else {
            Err(format!(
                "forward destination {name:?} ({}) has region {r:?}; use one of {}",
                d.vendor.as_str(),
                accepted.join(", ")
            ))
        }
    };
    // The user's header beats the preset's, so a vendor that changes how it
    // authenticates can be followed from the config file without a release.
    let user_sets = |h: &str| d.headers.iter().any(|(k, _)| k.eq_ignore_ascii_case(h));

    let (base, protocol) = match d.vendor {
        Vendor::Otlp => (
            need("endpoint", endpoint, "the receiver's base URL, without /v1/traces")?,
            Protocol::Json,
        ),
        Vendor::OtlpGrpc => (
            need("endpoint", endpoint, "the receiver's address, such as http://collector:4317")?,
            Protocol::Grpc,
        ),
        Vendor::Langfuse => {
            let base = match endpoint {
                Some(e) if e.trim_end_matches('/').ends_with("/api/public/otel") => e,
                Some(e) => format!("{}/api/public/otel", e.trim_end_matches('/')),
                None => match region_is(&["eu", "us", "hipaa"])?.as_str() {
                    "us" => "https://us.cloud.langfuse.com/api/public/otel".to_owned(),
                    "hipaa" => "https://hipaa.cloud.langfuse.com/api/public/otel".to_owned(),
                    _ => "https://cloud.langfuse.com/api/public/otel".to_owned(),
                },
            };
            if !user_sets("authorization") {
                let pk = need(
                    "public_key",
                    get("public_key", &d.public_key)?,
                    "the project's public key, pk-lf-...",
                )?;
                let sk = need(
                    "secret_key",
                    get("secret_key", &d.secret_key)?,
                    "the project's secret key, sk-lf-...",
                )?;
                headers.push((
                    "authorization".into(),
                    format!("Basic {}", base64(format!("{pk}:{sk}").as_bytes())),
                ));
            }
            (base, Protocol::Protobuf)
        }
        Vendor::Langsmith => {
            let base = endpoint.unwrap_or_else(|| {
                if region.as_deref() == Some("eu") {
                    "https://eu.api.smith.langchain.com/otel".to_owned()
                } else {
                    "https://api.smith.langchain.com/otel".to_owned()
                }
            });
            region_is(&["us", "eu"])?;
            if !user_sets("x-api-key") {
                headers.push((
                    "x-api-key".into(),
                    need("api_key", api_key, "a LangSmith API key, lsv2_...")?,
                ));
            }
            if let Some(p) = project {
                headers.push(("langsmith-project".into(), p));
            }
            (base, Protocol::Protobuf)
        }
        Vendor::Braintrust => {
            let base = endpoint.unwrap_or_else(|| "https://api.braintrust.dev/otel".to_owned());
            if !user_sets("authorization") {
                let key = need("api_key", api_key, "a Braintrust API key")?;
                headers.push(("authorization".into(), format!("Bearer {key}")));
            }
            if !user_sets("x-bt-parent") {
                let p = need("project", project, "the Braintrust project the spans are logged to")?;
                headers.push(("x-bt-parent".into(), format!("project_name:{p}")));
            }
            (base, Protocol::Protobuf)
        }
        Vendor::Arize => {
            let base = match endpoint {
                Some(e) => e,
                None => match region_is(&["us", "eu"])?.as_str() {
                    "eu" => "https://otlp.eu-west-1a.arize.com".to_owned(),
                    _ => "https://otlp.arize.com".to_owned(),
                },
            };
            if !user_sets("space_id") {
                headers.push((
                    "space_id".into(),
                    need("space_id", get("space_id", &d.space_id)?, "the Arize space id")?,
                ));
            }
            if !user_sets("api_key") {
                headers.push(("api_key".into(), need("api_key", api_key, "an Arize API key")?));
            }
            let p = need("project", project, "the Arize project the traces belong to")?;
            resource.push(("openinference.project.name".into(), AnyValue::Str(p.clone())));
            resource.push(("model_id".into(), AnyValue::Str(p)));
            (base, Protocol::Protobuf)
        }
        Vendor::Phoenix => {
            let base = endpoint.unwrap_or_else(|| "http://localhost:6006".to_owned());
            if let (Some(k), false) = (api_key, user_sets("authorization")) {
                headers.push(("authorization".into(), format!("Bearer {k}")));
            }
            if let Some(p) = project {
                resource.push(("openinference.project.name".into(), AnyValue::Str(p)));
            }
            (base, Protocol::Protobuf)
        }
        Vendor::Datadog => {
            let site = match region.as_deref() {
                None | Some("us1" | "us" | "datadoghq.com") => "datadoghq.com",
                Some("us3" | "us3.datadoghq.com") => "us3.datadoghq.com",
                Some("us5" | "us5.datadoghq.com") => "us5.datadoghq.com",
                Some("eu" | "eu1" | "datadoghq.eu") => "datadoghq.eu",
                Some("ap1" | "ap1.datadoghq.com") => "ap1.datadoghq.com",
                Some("ap2" | "ap2.datadoghq.com") => "ap2.datadoghq.com",
                Some("gov" | "ddog-gov.com") => "ddog-gov.com",
                Some(other) => {
                    return Err(format!(
                        "forward destination {name:?} (datadog) has region {other:?}; use your \
                         Datadog site: datadoghq.com, us3.datadoghq.com, us5.datadoghq.com, \
                         datadoghq.eu, ap1.datadoghq.com, ap2.datadoghq.com or ddog-gov.com"
                    ))
                }
            };
            let base = endpoint.unwrap_or_else(|| format!("https://otlp.{site}"));
            if !user_sets("dd-api-key") {
                headers.push(("dd-api-key".into(), need("api_key", api_key, "a Datadog API key")?));
            }
            if !user_sets("dd-otlp-source") {
                headers.push(("dd-otlp-source".into(), "llmobs".into()));
            }
            (base, Protocol::Protobuf)
        }
        Vendor::Honeycomb => {
            let base = match endpoint {
                Some(e) => e,
                None => match region_is(&["us", "eu"])?.as_str() {
                    "eu" => "https://api.eu1.honeycomb.io".to_owned(),
                    _ => "https://api.honeycomb.io".to_owned(),
                },
            };
            if !user_sets("x-honeycomb-team") {
                headers.push((
                    "x-honeycomb-team".into(),
                    need("api_key", api_key, "a Honeycomb ingest key")?,
                ));
            }
            if let Some(ds) = get("dataset", &d.dataset)? {
                headers.push(("x-honeycomb-dataset".into(), ds));
            }
            (base, Protocol::Protobuf)
        }
        Vendor::Newrelic => {
            let base = match endpoint {
                Some(e) => e,
                None => match region_is(&["us", "eu", "fedramp"])?.as_str() {
                    "eu" => "https://otlp.eu01.nr-data.net".to_owned(),
                    "fedramp" => "https://gov-otlp.nr-data.net".to_owned(),
                    _ => "https://otlp.nr-data.net".to_owned(),
                },
            };
            if !user_sets("api-key") {
                headers
                    .push(("api-key".into(), need("api_key", api_key, "a New Relic license key")?));
            }
            (base, Protocol::Protobuf)
        }
        Vendor::Grafana => {
            let base = match (endpoint, region) {
                (Some(e), _) => e,
                (None, Some(zone)) => format!("https://otlp-gateway-{zone}.grafana.net/otlp"),
                (None, None) => {
                    return Err(format!(
                        "forward destination {name:?} (grafana) needs `endpoint`, the OTLP \
                         gateway URL from the Grafana Cloud portal, or `region`, the stack's \
                         zone such as prod-eu-west-2"
                    ))
                }
            };
            if !user_sets("authorization") {
                let user = need(
                    "instance_id",
                    get("instance_id", &d.instance_id)?,
                    "the stack's instance id, from the OTLP page of the Grafana Cloud portal",
                )?;
                let key = need(
                    "api_key",
                    api_key.or(token),
                    "a Grafana Cloud access policy token with write scopes",
                )?;
                headers.push((
                    "authorization".into(),
                    format!("Basic {}", base64(format!("{user}:{key}").as_bytes())),
                ));
            }
            (base, Protocol::Protobuf)
        }
        Vendor::Sentry => {
            let (base, auth) = match get("dsn", &d.dsn)? {
                Some(dsn) => sentry_from_dsn(&dsn, &name)?,
                None => (
                    need(
                        "dsn",
                        endpoint,
                        "the project's DSN, from Settings, Client Keys in Sentry",
                    )?,
                    None,
                ),
            };
            if let (Some(a), false) = (auth, user_sets("x-sentry-auth")) {
                headers.push(("x-sentry-auth".into(), a));
            }
            (base, Protocol::Protobuf)
        }
        Vendor::Weave => {
            let base = endpoint.unwrap_or_else(|| "https://trace.wandb.ai/otel".to_owned());
            if !user_sets("authorization") {
                let key = need("api_key", api_key, "a W&B API key")?;
                headers.push((
                    "authorization".into(),
                    format!("Basic {}", base64(format!("api:{key}").as_bytes())),
                ));
            }
            if !user_sets("project_id") {
                let p = need("project", project, "the W&B project, as entity/project")?;
                headers.push(("project_id".into(), p));
            }
            (base, Protocol::Protobuf)
        }
        Vendor::Laminar => {
            let base = endpoint.unwrap_or_else(|| "https://api.lmnr.ai".to_owned());
            if !user_sets("authorization") {
                let key = need("api_key", api_key, "a Laminar project API key")?;
                headers.push(("authorization".into(), format!("Bearer {key}")));
            }
            (base, Protocol::Protobuf)
        }
        Vendor::Helicone => {
            let base = endpoint.unwrap_or_else(|| "https://api.worker.helicone.ai".to_owned());
            if !user_sets("authorization") {
                let key = need("api_key", api_key, "a Helicone API key, sk-helicone-...")?;
                headers.push(("authorization".into(), format!("Bearer {key}")));
            }
            (base, Protocol::Api)
        }
        Vendor::Opik => {
            let base = endpoint
                .unwrap_or_else(|| "https://www.comet.com/opik/api/v1/private/otel".to_owned());
            if let (Some(k), false) = (api_key, user_sets("authorization")) {
                headers.push(("authorization".into(), k));
            }
            if let Some(w) = get("workspace", &d.workspace)? {
                headers.push(("comet-workspace".into(), w));
            }
            if let Some(p) = project {
                headers.push(("projectname".into(), p));
            }
            (base, Protocol::Protobuf)
        }
        Vendor::Elastic => {
            let base =
                need("endpoint", endpoint, "the deployment's managed OTLP or APM Server endpoint")?;
            if !user_sets("authorization") {
                match (api_key, token) {
                    (Some(k), _) => headers.push(("authorization".into(), format!("ApiKey {k}"))),
                    (None, Some(t)) => {
                        headers.push(("authorization".into(), format!("Bearer {t}")));
                    }
                    (None, None) => {
                        return Err(format!(
                            "forward destination {name:?} (elastic) needs `api_key` or `token`, \
                             the APM secret token"
                        ))
                    }
                }
            }
            (base, Protocol::Protobuf)
        }
        Vendor::Dynatrace => {
            let e = need(
                "endpoint",
                endpoint,
                "the environment URL, such as https://abc12345.live.dynatrace.com",
            )?;
            let base = if e.trim_end_matches('/').ends_with("/api/v2/otlp") {
                e
            } else {
                format!("{}/api/v2/otlp", e.trim_end_matches('/'))
            };
            if !user_sets("authorization") {
                let t = need(
                    "token",
                    token.or(api_key),
                    "an access token with the openTelemetryTrace.ingest scope",
                )?;
                headers.push(("authorization".into(), format!("Api-Token {t}")));
            }
            (base, Protocol::Protobuf)
        }
    };

    let protocol = match (d.protocol, protocol) {
        (None, p) => p,
        (Some(_), Protocol::Api) => {
            return Err(format!(
                "forward destination {name:?} (helicone) takes no `protocol`: Helicone has no \
                 OTLP intake, so it is sent through its logging API"
            ))
        }
        (Some(Protocol::Api), _) => {
            return Err(format!(
                "forward destination {name:?}: protocol \"api\" is only for helicone; use \
                 json, protobuf or grpc"
            ))
        }
        // Dynatrace refuses JSON, so the choice would only fail later, at the
        // first batch, with a 415 nobody connects to this line.
        (Some(Protocol::Json), _) if d.vendor == Vendor::Dynatrace => {
            return Err(format!(
                "forward destination {name:?} (dynatrace) cannot use protocol json: Dynatrace \
                 accepts OTLP as protobuf only. Remove `protocol`"
            ))
        }
        (Some(p), _) => p,
    };

    reqwest::Url::parse(&base).map_err(|e| {
        format!(
            "forward destination {name:?} has an endpoint that is not a URL ({e}); write it \
             with its scheme, such as https://collector.example:4318"
        )
    })?;

    for (k, v) in &d.headers {
        let v = expand(v, &format!("header {k}"), &name, env)?;
        if reqwest::header::HeaderName::from_bytes(k.as_bytes()).is_err()
            || reqwest::header::HeaderValue::from_str(&v).is_err()
        {
            return Err(format!(
                "forward destination {name:?} has a header {k:?} that cannot be sent; check the \
                 name and the value for spaces or line breaks"
            ));
        }
        headers.retain(|(h, _)| !h.eq_ignore_ascii_case(k));
        headers.push((k.clone(), v));
    }

    let supported = d.vendor.signals();
    let mut signals = Vec::new();
    for (signal, wanted) in [
        (Signal::Traces, d.signals.traces),
        (Signal::Logs, d.signals.logs),
        (Signal::Metrics, d.signals.metrics),
    ] {
        match wanted {
            Some(true) if !supported.contains(&signal) => {
                return Err(format!(
                    "forward destination {name:?} asks for {} but {} accepts only {}; remove \
                     signals.{}",
                    signal.as_str(),
                    d.vendor.as_str(),
                    supported.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" and "),
                    signal.as_str()
                ))
            }
            Some(false) => {}
            _ if supported.contains(&signal) => signals.push(signal),
            _ => {}
        }
    }

    if let Some(s) = d.filter.sample {
        if !(0.0..=1.0).contains(&s) {
            return Err(format!(
                "forward destination {name:?}: filter.sample must be between 0.0 and 1.0, got {s}"
            ));
        }
    }
    if d.batch_size == 0 {
        return Err(format!("forward destination {name:?}: batch_size must be at least 1, got 0"));
    }

    Ok(Resolved {
        name,
        vendor: d.vendor,
        base,
        protocol,
        headers,
        resource,
        conventions: d.conventions.unwrap_or_else(|| d.vendor.conventions()),
        signals,
        // New Relic truncates an attribute past 4095 characters on its side,
        // and an attribute it cut is one a reader cannot tell was cut. mcpd
        // cuts first and says so in the value.
        max_value_bytes: if d.vendor == Vendor::Newrelic { 4095 } else { 32 * 1024 },
    })
}

/// Sentry's OTLP endpoint and auth header, from a DSN of the form
/// `https://PUBLIC_KEY@HOST/PROJECT_ID`.
fn sentry_from_dsn(dsn: &str, name: &str) -> Result<(String, Option<String>), String> {
    let bad = || {
        format!(
            "forward destination {name:?} (sentry) has a dsn that is not one; copy it from \
             Settings, Client Keys in Sentry: https://KEY@oNNN.ingest.sentry.io/PROJECT"
        )
    };
    let u = reqwest::Url::parse(dsn).map_err(|_| bad())?;
    let key = u.username();
    if key.is_empty() {
        return Err(bad());
    }
    let host = u.host_str().ok_or_else(bad)?;
    let path = u.path().trim_matches('/');
    let (prefix, project) = match path.rsplit_once('/') {
        Some((p, id)) => (format!("/{p}"), id),
        None => (String::new(), path),
    };
    if project.is_empty() || !project.chars().all(|c| c.is_ascii_digit()) {
        return Err(bad());
    }
    let port = u.port().map(|p| format!(":{p}")).unwrap_or_default();
    Ok((
        format!("{}://{host}{port}{prefix}/api/{project}/integration/otlp", u.scheme()),
        Some(format!("sentry sentry_key={key}")),
    ))
}

/// Everything about forwarding the daemon was configured with.
#[derive(Clone)]
pub struct Settings {
    /// The destinations, in the order they were written.
    pub destinations: Vec<Destination>,
    /// Where each destination keeps its spool, one directory each. `None`
    /// holds the spool in memory, which is what a store of `:memory:` gets.
    pub spool_dir: Option<PathBuf>,
    /// The default ceiling on one destination's spool, in bytes.
    pub spool_max_bytes: u64,
}

/// The spool ceiling when nothing sets one: 256 MiB per destination.
pub const DEFAULT_SPOOL_MAX_BYTES: u64 = 256 * 1024 * 1024;

impl Default for Settings {
    fn default() -> Self {
        Self { destinations: Vec::new(), spool_dir: None, spool_max_bytes: DEFAULT_SPOOL_MAX_BYTES }
    }
}

impl Settings {
    /// The destinations that are switched on.
    pub fn enabled(&self) -> impl Iterator<Item = &Destination> {
        self.destinations.iter().filter(|d| d.enabled)
    }

    /// Resolve every destination, and refuse two with the same name.
    ///
    /// # Errors
    /// The first problem found, worded for the operator.
    pub fn check(&self) -> Result<Vec<Resolved>, String> {
        let mut seen = std::collections::BTreeSet::new();
        let mut out = Vec::new();
        for d in &self.destinations {
            let r = d.resolve()?;
            if !seen.insert(r.name.clone()) {
                return Err(format!(
                    "two forward destinations are named {:?}; give each a distinct `name`, \
                     because it is the label their metrics and spool are kept under",
                    r.name
                ));
            }
            if d.enabled {
                out.push(r);
            }
        }
        Ok(out)
    }
}

/// Read the `forward` block: one destination as an object, which is how
/// earlier configs wrote it, or several as an array.
///
/// # Errors
/// Names the entry and the field that is wrong.
pub fn parse_block(v: &serde_json::Value) -> Result<Vec<Destination>, String> {
    let one = |v: &serde_json::Value, at: &str| {
        serde_json::from_value::<Destination>(v.clone()).map_err(|e| format!("forward{at}: {e}"))
    };
    match v {
        serde_json::Value::Null => Ok(Vec::new()),
        serde_json::Value::Object(_) => Ok(vec![one(v, "")?]),
        serde_json::Value::Array(items) => {
            items.iter().enumerate().map(|(i, v)| one(v, &format!("[{i}]"))).collect()
        }
        _ => Err("forward must be an object, or an array of them".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn env(name: &str) -> Option<String> {
        match name {
            "LF_PK" => Some("pk-lf-1".into()),
            "LF_SK" => Some("sk-lf-2".into()),
            "KEY" => Some("k3y".into()),
            _ => None,
        }
    }

    fn dest(v: serde_json::Value) -> Destination {
        serde_json::from_value(v).expect("a destination")
    }

    fn header<'a>(r: &'a Resolved, name: &str) -> Option<&'a str> {
        r.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    #[test]
    fn base64_matches_the_rfc_vectors() {
        for (i, o) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(i.as_bytes()), o);
        }
    }

    #[test]
    fn langfuse_reads_its_keys_from_the_environment_into_basic_auth() {
        let d = dest(json!({
            "vendor": "langfuse", "region": "us",
            "public_key": "${LF_PK}", "secret_key": "${LF_SK}"
        }));
        let r = resolve(&d, &env).expect("resolves");
        assert_eq!(
            r.url(Signal::Traces),
            "https://us.cloud.langfuse.com/api/public/otel/v1/traces"
        );
        assert_eq!(
            header(&r, "authorization"),
            Some(format!("Basic {}", base64(b"pk-lf-1:sk-lf-2")).as_str())
        );
        assert_eq!(r.protocol, Protocol::Protobuf);
        assert_eq!(r.signals, vec![Signal::Traces]);
    }

    #[test]
    fn a_missing_variable_is_named_and_its_value_never_appears() {
        let d = dest(json!({ "vendor": "honeycomb", "api_key": "${NOPE}" }));
        let err = resolve(&d, &env).expect_err("refused");
        assert!(err.contains("NOPE") && err.contains("not set"), "{err}");
    }

    #[test]
    fn a_vendor_without_its_key_says_which_field_and_what_it_is() {
        let err = resolve(&dest(json!({ "vendor": "datadog" })), &env).expect_err("refused");
        assert!(err.contains("api_key") && err.contains("Datadog API key"), "{err}");
    }

    #[test]
    fn a_user_header_replaces_the_preset_one() {
        let d = dest(json!({
            "vendor": "honeycomb", "api_key": "a",
            "headers": { "X-Honeycomb-Team": "b" }
        }));
        let r = resolve(&d, &env).expect("resolves");
        let teams: Vec<_> =
            r.headers.iter().filter(|(k, _)| k.eq_ignore_ascii_case("x-honeycomb-team")).collect();
        assert_eq!(teams.len(), 1);
        assert_eq!(teams[0].1, "b");
    }

    #[test]
    fn sentry_derives_its_endpoint_and_auth_from_the_dsn() {
        let d = dest(json!({
            "vendor": "sentry", "dsn": "https://abc123@o42.ingest.us.sentry.io/4507"
        }));
        let r = resolve(&d, &env).expect("resolves");
        assert_eq!(
            r.url(Signal::Traces),
            "https://o42.ingest.us.sentry.io/api/4507/integration/otlp/v1/traces"
        );
        assert_eq!(header(&r, "x-sentry-auth"), Some("sentry sentry_key=abc123"));
        assert_eq!(r.signals, vec![Signal::Traces, Signal::Logs]);
    }

    #[test]
    fn dynatrace_appends_its_otlp_path_and_refuses_json() {
        let d = dest(json!({
            "vendor": "dynatrace", "endpoint": "https://abc.live.dynatrace.com", "token": "${KEY}"
        }));
        let r = resolve(&d, &env).expect("resolves");
        assert_eq!(r.url(Signal::Logs), "https://abc.live.dynatrace.com/api/v2/otlp/v1/logs");
        assert_eq!(header(&r, "authorization"), Some("Api-Token k3y"));
        let mut json_one = d.clone();
        json_one.protocol = Some(Protocol::Json);
        assert!(resolve(&json_one, &env).is_err());
    }

    #[test]
    fn grpc_posts_to_the_export_service_path() {
        let d = dest(json!({ "vendor": "otlp_grpc", "endpoint": "http://collector:4317" }));
        let r = resolve(&d, &env).expect("resolves");
        assert_eq!(
            r.url(Signal::Metrics),
            "http://collector:4317/opentelemetry.proto.collector.metrics.v1.MetricsService/Export"
        );
    }

    #[test]
    fn a_signal_the_vendor_cannot_take_is_refused_at_start() {
        let d = dest(json!({
            "vendor": "langsmith", "api_key": "k", "signals": { "metrics": true }
        }));
        let err = resolve(&d, &env).expect_err("refused");
        assert!(err.contains("metrics") && err.contains("traces"), "{err}");
    }

    #[test]
    fn the_old_single_object_form_still_reads() {
        let v =
            json!({ "endpoint": "https://c.example", "headers": [["a", "b"]], "batch_size": 9 });
        let list = parse_block(&v).expect("reads");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].vendor, Vendor::Otlp);
        assert_eq!(list[0].batch_size, 9);
        let r = resolve(&list[0], &env).expect("resolves");
        assert_eq!(r.protocol, Protocol::Json, "an existing config keeps sending JSON");
    }

    #[test]
    fn an_unknown_field_in_an_entry_is_named_with_its_index() {
        let v = json!([{ "vendor": "otlp", "endpoint": "http://a" }, { "vendr": "x" }]);
        let err = parse_block(&v).expect_err("refused");
        assert!(err.starts_with("forward[1]") && err.contains("vendr"), "{err}");
    }

    #[test]
    fn debug_never_prints_a_credential() {
        let d = dest(json!({
            "vendor": "langfuse", "public_key": "pk-secret-1", "secret_key": "sk-secret-2",
            "endpoint": "https://u:p4ss@lf.example?token=t0k",
            "headers": [["x-extra", "v4lue"]]
        }));
        let shown = format!("{d:?}");
        for secret in ["pk-secret-1", "sk-secret-2", "p4ss", "t0k", "v4lue"] {
            assert!(!shown.contains(secret), "{shown}");
        }
    }

    #[test]
    fn every_vendor_resolves_with_its_documented_minimum() {
        let minimum = |v: Vendor| -> serde_json::Value {
            let mut d = json!({ "vendor": v.as_str(), "api_key": "k", "project": "p" });
            match v {
                Vendor::Otlp | Vendor::OtlpGrpc | Vendor::Elastic | Vendor::Dynatrace => {
                    d["endpoint"] = json!("https://collector.example");
                }
                Vendor::Langfuse => {
                    d["public_key"] = json!("pk");
                    d["secret_key"] = json!("sk");
                }
                Vendor::Arize => d["space_id"] = json!("s"),
                Vendor::Grafana => {
                    d["region"] = json!("prod-eu-west-2");
                    d["instance_id"] = json!("123");
                }
                Vendor::Sentry => d["dsn"] = json!("https://k@o1.ingest.sentry.io/2"),
                _ => {}
            }
            d
        };
        for v in Vendor::ALL {
            let r = resolve(&dest(minimum(*v)), &env)
                .unwrap_or_else(|e| panic!("{} did not resolve: {e}", v.as_str()));
            assert!(r.url(Signal::Traces).starts_with("http"), "{}", v.as_str());
            assert_eq!(Vendor::parse(v.as_str()), Ok(*v));
        }
    }
}
