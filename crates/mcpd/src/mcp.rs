//! The Model Context Protocol server, so an agent can read its own traces.
//!
//! `mcpd mcp` speaks JSON-RPC 2.0 over stdio and answers three tools. It is a
//! client of a deployed daemon, never of the store:
//!
//! * SQLite takes one writer, and the daemon already holds it. A second process
//!   opening the same file would fight the one that is serving traffic.
//! * The deployment worth investigating is usually somewhere else. A store
//!   handle only works on the machine the data is on; an HTTP call works from a
//!   laptop.
//! * Going over HTTP means this obeys the same key scopes as everything else.
//!   `authorize_project` decides what a key reaches, once, in the server. An
//!   MCP server that read the file directly would be a hole straight through
//!   that decision.
//!
//! The protocol is hand-rolled for the same reason the OTLP protobuf decoder
//! is: the surface is three methods, and the product's claim is one static
//! binary with no runtime to install. A crate here would be a dependency in the
//! one place that promise is measured.
//!
//! Tool results are text, not JSON. An agent reading forty span objects spends
//! its context on punctuation; the same run as a waterfall is a few hundred
//! tokens and it can see the shape.

use anyhow::{Context, Result};
use mcpd_core::{AnalyticsSummary, Span, SpanKind, SpanStatus, TraceDetail, TraceListResponse};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use time::OffsetDateTime;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// Protocol revisions this server speaks, newest first.
///
/// A client that asks for one of these gets that one back. A client that asks
/// for anything else gets the newest, which is what the specification says to
/// do: the client then decides whether it can live with the answer.
const PROTOCOL_VERSIONS: [&str; 2] = ["2025-06-18", "2024-11-05"];

/// How wide a waterfall bar is, in characters.
const BAR_WIDTH: usize = 40;

/// Characters of payload text one step may contribute before it is cut.
const DEFAULT_PAYLOAD_CHARS: usize = 1200;

/// Characters of payload text a whole run may contribute.
///
/// A per-step cap alone does not bound anything: a run with sixty steps still
/// returns sixty payloads. This is the number that keeps one call from eating
/// an agent's context.
const RUN_PAYLOAD_BUDGET: usize = 20_000;

/// How many log lines one run renders before the rest are counted instead.
const MAX_LOG_LINES: usize = 50;

/// How many waterfall rows one run renders before the rest are counted instead.
const MAX_WATERFALL_ROWS: usize = 200;

/// Where `mcpd mcp` finds the daemon it reads.
#[derive(Debug, Clone, clap::Args)]
pub struct Args {
    /// The running daemon to query. This is a client: it opens no store.
    #[arg(long, env = "MCPD_ENDPOINT", default_value = "http://127.0.0.1:4317")]
    endpoint: String,

    /// A read key for that daemon. Omit when the daemon has no keys set.
    ///
    /// Whatever this key reaches is what the tools reach. A key scoped to one
    /// project cannot read another through here, because the server decides
    /// that and this is an ordinary client of it.
    #[arg(long, env = "MCPD_API_KEY")]
    api_key: Option<String>,
}

/// Serve MCP over stdio until stdin closes.
///
/// `default_project` is the daemon-wide `--project`, used when a tool call does
/// not name one.
///
/// # Errors
/// Returns an error if the HTTP client cannot be built, or if stdin or stdout
/// fails. A failure to answer one request is reported to the client instead,
/// because a debugging session should survive a bad tool call.
pub async fn run(args: &Args, default_project: &str) -> Result<()> {
    let daemon = Daemon::new(args, default_project)?;

    // Newline-delimited JSON is the stdio framing MCP defines. Nothing else may
    // ever reach stdout: one stray log line and the client's parser is done for
    // the rest of the session. That is why this subcommand is dispatched before
    // the tracing subscriber is installed.
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut out = tokio::io::stdout();

    while let Some(line) = lines.next_line().await.context("reading stdin")? {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(reply) = handle(&daemon, line).await {
            let mut text = serde_json::to_string(&reply).context("encoding a JSON-RPC reply")?;
            text.push('\n');
            out.write_all(text.as_bytes()).await.context("writing to stdout")?;
            out.flush().await.context("flushing stdout")?;
        }
    }
    Ok(())
}

// ----------------------------------------------------------------- protocol --

/// A JSON-RPC failure: the request could not be dispatched at all.
///
/// Distinct from a tool that ran and failed, which is a successful response
/// carrying `isError`. Collapsing the two would hide a daemon outage from the
/// model behind a transport error only the host ever sees.
struct RpcError {
    code: i64,
    message: String,
}

impl RpcError {
    fn new(code: i64, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }
}

fn ok_reply(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn err_reply(id: &Value, e: &RpcError) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": e.code, "message": e.message } })
}

/// Answer one line of input, or `None` when it was a notification.
///
/// A notification has no `id`, and JSON-RPC forbids replying to one. Clients
/// send `notifications/initialized` on every session, so getting this wrong
/// produces a spurious reply before the first real call.
async fn handle(daemon: &Daemon, line: &str) -> Option<Value> {
    let null = Value::Null;

    let request: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => {
            return Some(err_reply(
                &null,
                &RpcError::new(-32700, format!("that line is not JSON: {e}")),
            ))
        }
    };

    if request.is_array() {
        return Some(err_reply(
            &null,
            &RpcError::new(
                -32600,
                "this server does not accept batched requests; the 2025-06-18 protocol \
                 removed them. Send one request per line.",
            ),
        ));
    }

    let Some(object) = request.as_object() else {
        return Some(err_reply(
            &null,
            &RpcError::new(-32600, "a JSON-RPC request must be an object"),
        ));
    };

    // Absent id means a notification. A null id is a request with a null id,
    // which is legal, so the two are not the same thing.
    let id = object.get("id").cloned();
    let Some(method) = object.get("method").and_then(Value::as_str) else {
        return id.map(|id| {
            err_reply(&id, &RpcError::new(-32600, "the request has no `method` string"))
        });
    };

    let params = object.get("params").cloned().unwrap_or_else(|| json!({}));
    let id = id?;

    let result = match method {
        "initialize" => Ok(initialize(&params)),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tools() })),
        "tools/call" => call_tool(daemon, &params).await,
        other => Err(RpcError::new(
            -32601,
            format!(
                "{other:?} is not a method this server has; it serves initialize, ping, \
                 tools/list and tools/call"
            ),
        )),
    };

    Some(match result {
        Ok(value) => ok_reply(&id, value),
        Err(e) => err_reply(&id, &e),
    })
}

/// Negotiate a protocol version and describe what this server offers.
fn initialize(params: &Value) -> Value {
    let asked = params.get("protocolVersion").and_then(Value::as_str);
    let chosen = match asked {
        Some(v) if PROTOCOL_VERSIONS.contains(&v) => v,
        _ => PROTOCOL_VERSIONS[0],
    };

    json!({
        "protocolVersion": chosen,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": { "name": "mcpd", "version": mcpd_core::VERSION },
        "instructions":
            "Read agent traces recorded by mcpd. A run is one trace: an agent invocation \
             and every step under it. Start with find_runs to get a trace id, then \
             get_run for the waterfall, the failing step and the prompts. Use run_stats \
             to see whether one failure is a pattern. Every tool reads only what the \
             configured key is allowed to read.",
    })
}

// -------------------------------------------------------------------- tools --

/// The three tools, and no more.
///
/// The question this exists to answer is "why did my run fail". That is a
/// search, a run, and the context to tell one bad run from a broken deployment.
/// Every further endpoint mcpd has would be another entry an agent has to read
/// past on every single call, and an agent with forty tools picks badly.
fn tools() -> Vec<Value> {
    let read_only =
        json!({ "readOnlyHint": true, "destructiveHint": false, "openWorldHint": true });

    vec![
        json!({
            "name": "find_runs",
            "description":
                "Search agent runs and return one line each: trace id, age, status, \
                 duration, cost, step count and name. Every filter is optional and they \
                 combine. Use status=\"error\" to list failures, sort=\"duration\" for the \
                 slowest, q for free text across names and payloads. The window defaults \
                 to the last 24 hours; widen it with since. Pass a returned trace id to \
                 get_run.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "project": { "type": "string", "description": "Project to search. Defaults to the daemon's own project." },
                    "status": { "type": "string", "enum": ["ok", "error", "unset", "cancelled"], "description": "Keep only runs that ended this way." },
                    "q": { "type": "string", "description": "Free text, matched across run names and payloads." },
                    "name": { "type": "string", "description": "Exact root span name, such as support_agent." },
                    "model": { "type": "string", "description": "Keep only runs that called this model." },
                    "environment": { "type": "string", "description": "Deployment environment, such as production." },
                    "session_id": { "type": "string", "description": "All the runs in one conversation." },
                    "user_id": { "type": "string", "description": "One end user. Pass the raw id; mcpd hashes it the same way it hashed the stored one." },
                    "tags": { "type": "array", "items": { "type": "string" }, "description": "Tags on the root span. A run must carry all of them." },
                    "since": { "type": "string", "description": "Start of the window: -15m, -1h, -7d, or an RFC 3339 timestamp. Default -24h." },
                    "until": { "type": "string", "description": "End of the window, same format. Default now." },
                    "min_duration_ms": { "type": "integer", "description": "Keep only runs at least this slow." },
                    "sort": { "type": "string", "enum": ["start_time", "duration", "cost", "span_count"], "description": "Largest or newest first. Default start_time." },
                    "limit": { "type": "integer", "description": "How many runs to return, 1 to 100. Default 20." },
                    "cursor": { "type": "string", "description": "The next_cursor printed by a previous call." }
                },
                "additionalProperties": false
            },
            "annotations": read_only,
        }),
        json!({
            "name": "get_run",
            "description":
                "Everything about one run: which step failed and why, a waterfall of every \
                 step with its timing, the prompts and completions of each model call, the \
                 logs, and any scores or feedback. This is the tool that answers why a run \
                 failed. Payloads are included by default and each one is capped; pass \
                 payloads=\"errors\" for a cheaper read, or \"none\" for the shape alone.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "trace_id": { "type": "string", "description": "The 32-character run id, from find_runs." },
                    "project": { "type": "string", "description": "Project the run is in. Defaults to the daemon's own project." },
                    "payloads": { "type": "string", "enum": ["all", "errors", "none"], "description": "Which steps show their input and output. Default all." },
                    "max_payload_chars": { "type": "integer", "description": "Characters per payload before it is cut. Default 1200." }
                },
                "required": ["trace_id"],
                "additionalProperties": false
            },
            "annotations": read_only,
        }),
        json!({
            "name": "run_stats",
            "description":
                "Cost, failure rate, latency and token use over a window, optionally broken \
                 down by model, run name, status, environment or user. Use it to tell one \
                 bad run from a broken deployment: if the failure rate is 40% the run you \
                 are reading is not the problem.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "project": { "type": "string", "description": "Project to measure. Defaults to the daemon's own project." },
                    "since": { "type": "string", "description": "Start of the window: -1h, -7d, or an RFC 3339 timestamp. Default -24h." },
                    "until": { "type": "string", "description": "End of the window, same format. Default now." },
                    "group_by": { "type": "string", "enum": ["model", "provider", "entity_name", "environment", "status", "session_id", "user_id", "service_name", "name"], "description": "Break the window down by this dimension." },
                    "limit": { "type": "integer", "description": "How many groups to show, 1 to 50. Default 10." }
                },
                "additionalProperties": false
            },
            "annotations": read_only,
        }),
    ]
}

/// Run one tool and wrap the result the way `tools/call` expects.
///
/// A tool that ran and failed is a successful response with `isError` set, so
/// the model reads the reason and can act on it. Only a call that could not be
/// dispatched at all is a JSON-RPC error.
async fn call_tool(daemon: &Daemon, params: &Value) -> Result<Value, RpcError> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| RpcError::new(-32602, "tools/call needs a `name`"))?;

    let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
    if !args.is_object() {
        return Err(RpcError::new(-32602, "`arguments` must be an object"));
    }

    let outcome = match name {
        "find_runs" => {
            known_args(&args, FIND_RUNS_ARGS)?;
            find_runs(daemon, &args).await
        }
        "get_run" => {
            known_args(&args, GET_RUN_ARGS)?;
            get_run(daemon, &args).await
        }
        "run_stats" => {
            known_args(&args, RUN_STATS_ARGS)?;
            run_stats(daemon, &args).await
        }
        other => {
            return Err(RpcError::new(
                -32602,
                format!("there is no tool called {other:?}; this server has find_runs, get_run and run_stats"),
            ))
        }
    };

    let (text, failed) = match outcome {
        Ok(text) => (text, false),
        Err(text) => (text, true),
    };
    Ok(json!({ "content": [{ "type": "text", "text": text }], "isError": failed }))
}

const FIND_RUNS_ARGS: &[&str] = &[
    "project",
    "status",
    "q",
    "name",
    "model",
    "environment",
    "session_id",
    "user_id",
    "tags",
    "since",
    "until",
    "min_duration_ms",
    "sort",
    "limit",
    "cursor",
];
const GET_RUN_ARGS: &[&str] = &["trace_id", "project", "payloads", "max_payload_chars"];
const RUN_STATS_ARGS: &[&str] = &["project", "since", "until", "group_by", "limit"];

/// Refuse an argument this tool does not have.
///
/// Silently ignoring `trace` for `trace_id`, or `from` for `since`, returns a
/// plausible wrong answer: the whole project instead of one run. Naming the
/// mistake costs one round trip and saves the agent from acting on it.
fn known_args(args: &Value, allowed: &[&str]) -> Result<(), RpcError> {
    let Some(map) = args.as_object() else { return Ok(()) };
    for key in map.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(RpcError::new(
                -32602,
                format!("{key:?} is not an argument of this tool; it takes {}", allowed.join(", ")),
            ));
        }
    }
    Ok(())
}

// ------------------------------------------------------------ daemon client --

/// An HTTP client for one deployed daemon.
struct Daemon {
    http: reqwest::Client,
    endpoint: String,
    api_key: Option<String>,
    default_project: String,
}

impl Daemon {
    fn new(args: &Args, default_project: &str) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(format!("mcpd-mcp/{}", mcpd_core::VERSION))
            // An MCP host blocks on a tool call, so a hung request is a hung
            // agent. Fail with something readable instead.
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .context("building the HTTP client")?;

        Ok(Self {
            http,
            endpoint: args.endpoint.trim_end_matches('/').to_owned(),
            api_key: args.api_key.clone(),
            default_project: default_project.to_owned(),
        })
    }

    /// GET one API path, or explain to the agent why it could not.
    async fn get<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        query: &[(String, String)],
    ) -> Result<T, String> {
        let url = format!("{}{path}", self.endpoint);
        let mut request = self.http.get(&url).query(query);
        if let Some(key) = &self.api_key {
            request = request.bearer_auth(key);
        }

        // The endpoint is named once, here; the error is quoted without its
        // URL, which repeats the endpoint and adds the query.
        let response = request.send().await.map_err(|e| {
            format!(
                "Cannot reach the mcpd daemon at {}: {}. Check that it is running, and \
                 that --endpoint (or MCPD_ENDPOINT) names it.",
                self.endpoint,
                mcpd_server::outbound_error(e)
            )
        })?;

        let status = response.status();
        let body = response.text().await.map_err(|e| {
            format!("The daemon at {} closed the response early: {e}", self.endpoint)
        })?;

        if !status.is_success() {
            return Err(self.explain(status, &body));
        }

        serde_json::from_str(&body).map_err(|e| {
            format!(
                "The daemon replied with something this build does not understand: {e}. The \
                 daemon at {} may be a newer version than this binary; upgrade mcpd so the \
                 two match.",
                self.endpoint
            )
        })
    }

    /// Turn an error response into a sentence naming the problem and the fix.
    fn explain(&self, status: reqwest::StatusCode, body: &str) -> String {
        let message = serde_json::from_str::<Value>(body)
            .ok()
            .and_then(|v| v.get("message").and_then(Value::as_str).map(str::to_owned))
            .unwrap_or_else(|| body.chars().take(300).collect());

        match status.as_u16() {
            401 => format!(
                "The daemon refused the credential: {message} Pass a read key with --api-key, \
                 or set MCPD_API_KEY. It has to be one of the daemon's own read keys."
            ),
            403 => format!(
                "That key is not allowed to read this: {message} A key scoped to a project \
                 reaches that project alone, which is deliberate. Ask for a project the key \
                 covers, or use a key with wider reach."
            ),
            404 => format!("Nothing found: {message}"),
            400 => format!("The daemon rejected the query: {message}"),
            _ => format!("The daemon at {} answered {status}: {message}", self.endpoint),
        }
    }

    fn project(&self, args: &Value) -> String {
        str_arg(args, "project").unwrap_or(&self.default_project).to_owned()
    }
}

// ------------------------------------------------------------ argument help --

fn str_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str).filter(|s| !s.trim().is_empty())
}

/// Read an integer argument, refusing a value that is not one.
///
/// A model that sends `"20"` where a number belongs is common enough that
/// accepting the string is worth more than being strict about it.
fn int_arg(args: &Value, key: &str) -> Result<Option<i64>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => {
            n.as_i64().map(Some).ok_or_else(|| format!("{key} must be a whole number, got {n}"))
        }
        Some(Value::String(s)) => s
            .trim()
            .parse::<i64>()
            .map(Some)
            .map_err(|_| format!("{key} must be a whole number, got {s:?}")),
        Some(other) => Err(format!("{key} must be a whole number, got {other}")),
    }
}

/// Read a list argument, accepting either an array or a comma-separated string.
fn list_arg(args: &Value, key: &str) -> Result<Vec<String>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::String(s)) => {
            Ok(s.split(',').map(|t| t.trim().to_owned()).filter(|t| !t.is_empty()).collect())
        }
        Some(Value::Array(items)) => items
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| format!("every entry of {key} must be a string, got {v}"))
            })
            .collect(),
        Some(other) => Err(format!("{key} must be an array of strings, got {other}")),
    }
}

/// Check a value against the tool's own enumeration before the round trip.
fn enum_arg<'a>(args: &'a Value, key: &str, allowed: &[&str]) -> Result<Option<&'a str>, String> {
    match str_arg(args, key) {
        None => Ok(None),
        Some(v) if allowed.contains(&v) => Ok(Some(v)),
        Some(v) => Err(format!("{key}={v:?} is not one of: {}", allowed.join(", "))),
    }
}

// -------------------------------------------------------------- find_runs --

async fn find_runs(daemon: &Daemon, args: &Value) -> Result<String, String> {
    let project = daemon.project(args);
    let since = str_arg(args, "since").unwrap_or("-24h");
    let limit = int_arg(args, "limit")?.unwrap_or(20).clamp(1, 100);

    let mut query: Vec<(String, String)> = vec![
        ("project".to_owned(), project.clone()),
        ("from".to_owned(), since.to_owned()),
        ("limit".to_owned(), limit.to_string()),
    ];

    if let Some(status) = enum_arg(args, "status", &["ok", "error", "unset", "cancelled"])? {
        query.push(("status".to_owned(), status.to_owned()));
    }
    if let Some(sort) = enum_arg(args, "sort", &["start_time", "duration", "cost", "span_count"])? {
        query.push(("sort".to_owned(), sort.to_owned()));
    }
    for key in ["q", "name", "model", "environment", "session_id", "user_id", "cursor"] {
        if let Some(v) = str_arg(args, key) {
            query.push((key.to_owned(), v.to_owned()));
        }
    }
    if let Some(until) = str_arg(args, "until") {
        query.push(("to".to_owned(), until.to_owned()));
    }
    if let Some(ms) = int_arg(args, "min_duration_ms")? {
        query.push(("min_duration_ms".to_owned(), ms.to_string()));
    }
    let tags = list_arg(args, "tags")?;
    if !tags.is_empty() {
        query.push(("tags".to_owned(), tags.join(",")));
    }

    let page: TraceListResponse = daemon.get("/v1/api/traces", &query).await?;
    Ok(render_run_list(&page, &project, since, args))
}

fn render_run_list(page: &TraceListResponse, project: &str, since: &str, args: &Value) -> String {
    let now = OffsetDateTime::now_utc();
    let mut out = Out::default();

    let mut filters = vec![format!("since={since}")];
    for key in
        ["status", "q", "name", "model", "environment", "session_id", "user_id", "until", "sort"]
    {
        if let Some(v) = str_arg(args, key) {
            filters.push(format!("{key}={v}"));
        }
    }

    if page.traces.is_empty() {
        out.line(format!("No runs in project {project:?} matched {}.", filters.join(", ")));
        out.blank();
        out.line("Widen the window with since (-7d), drop a filter, or name another project.");
        return out.finish();
    }

    let total = page.page.total.map_or_else(String::new, |t| format!(" of {}", fmt_count(t)));
    out.line(format!(
        "{} run{}{total} in project {project:?}, matching {}.",
        page.traces.len(),
        if page.traces.len() == 1 { "" } else { "s" },
        filters.join(", ")
    ));
    out.line(format!("Ages are relative to {}.", fmt_time(now)));
    out.blank();

    let mut table = Table::new(
        &["trace_id", "age", "status", "duration", "cost", "steps", "failed", "name"],
        &[false, true, false, true, true, true, true, false],
    );
    for trace in &page.traces {
        table.row(vec![
            trace.trace_id.clone(),
            fmt_age(trace.start_time, now),
            trace.status.as_str().to_owned(),
            trace.duration_ns.map_or_else(|| "running".to_owned(), fmt_ns),
            trace
                .cost
                .as_ref()
                .and_then(|c| c.total_cost)
                .map_or_else(|| "-".to_owned(), fmt_money),
            fmt_count(u64::from(trace.span_count)),
            fmt_count(u64::from(trace.error_count)),
            trace.name.clone(),
        ]);
    }
    for line in table.render("") {
        out.line(line);
    }

    out.blank();
    if let Some(cursor) = &page.page.next_cursor {
        out.line(format!("More runs match. Pass cursor={cursor:?} to read the next page."));
    }
    out.line("Pass a trace_id to get_run for the waterfall, the failing step and the prompts.");
    out.finish()
}

// ----------------------------------------------------------------- get_run --

async fn get_run(daemon: &Daemon, args: &Value) -> Result<String, String> {
    let trace_id = str_arg(args, "trace_id")
        .ok_or_else(|| "get_run needs a trace_id; find one with find_runs.".to_owned())?;
    let project = daemon.project(args);
    let payloads = enum_arg(args, "payloads", &["all", "errors", "none"])?.unwrap_or("all");
    let per_payload = usize::try_from(
        int_arg(args, "max_payload_chars")?
            .unwrap_or(DEFAULT_PAYLOAD_CHARS as i64)
            .clamp(1, 20_000),
    )
    .unwrap_or(DEFAULT_PAYLOAD_CHARS);

    let query = vec![
        ("project".to_owned(), project.clone()),
        ("include".to_owned(), "logs,scores,feedback".to_owned()),
    ];
    let detail: TraceDetail =
        daemon.get(&format!("/v1/api/traces/{trace_id}"), &query).await.map_err(|e| {
            if e.starts_with("Nothing found") {
                format!(
                    "{e} No run {trace_id:?} in project {project:?}. Check the project, or \
                     search for the run with find_runs."
                )
            } else {
                e
            }
        })?;

    Ok(render_run(&detail, &project, payloads, per_payload))
}

// --------------------------------------------------------------- run_stats --

async fn run_stats(daemon: &Daemon, args: &Value) -> Result<String, String> {
    const DIMENSIONS: &[&str] = &[
        "model",
        "provider",
        "entity_name",
        "environment",
        "status",
        "session_id",
        "user_id",
        "service_name",
        "name",
    ];

    let project = daemon.project(args);
    let since = str_arg(args, "since").unwrap_or("-24h");
    let group_by = enum_arg(args, "group_by", DIMENSIONS)?;
    let limit = usize::try_from(int_arg(args, "limit")?.unwrap_or(10).clamp(1, 50)).unwrap_or(10);

    let mut query =
        vec![("project".to_owned(), project.clone()), ("from".to_owned(), since.to_owned())];
    if let Some(until) = str_arg(args, "until") {
        query.push(("to".to_owned(), until.to_owned()));
    }
    if let Some(dimension) = group_by {
        query.push(("group_by".to_owned(), dimension.to_owned()));
    }

    let summary: AnalyticsSummary = daemon.get("/v1/api/analytics/summary", &query).await?;
    Ok(render_stats(&summary, &project, group_by, limit))
}

// ---------------------------------------------------------------- the run --

/// One span and how deep it sits in the tree.
struct Step<'a> {
    span: &'a Span,
    depth: usize,
}

/// Put the spans in the order a waterfall reads: parents before children,
/// siblings by start time.
///
/// The API already returns them start-then-depth ordered, but that ordering
/// cannot say which parent a row belongs under, and indentation is the only
/// thing that makes a forty-step run legible. Walked with an explicit stack and
/// a visited set: a trace is producer-supplied data, so a cycle or a
/// ten-thousand-deep chain has to be survivable, not a stack overflow.
fn order_spans(spans: &[Span]) -> Vec<Step<'_>> {
    let index: HashMap<&str, usize> =
        spans.iter().enumerate().map(|(i, s)| (s.span_id.as_str(), i)).collect();

    let mut children: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut roots: Vec<usize> = Vec::new();
    for (i, span) in spans.iter().enumerate() {
        match span.parent_span_id.as_deref().and_then(|p| index.get(p)) {
            // A span cannot be its own parent, and treating one as a root is
            // better than dropping it.
            Some(&parent) if parent != i => children.entry(parent).or_default().push(i),
            _ => roots.push(i),
        }
    }

    let by_start = |group: &mut Vec<usize>| {
        group.sort_by(|&a, &b| {
            spans[a]
                .start_time
                .cmp(&spans[b].start_time)
                .then_with(|| spans[a].name.cmp(&spans[b].name))
        });
    };
    by_start(&mut roots);
    for group in children.values_mut() {
        by_start(group);
    }

    let mut ordered = Vec::with_capacity(spans.len());
    let mut seen: HashSet<usize> = HashSet::new();
    let mut stack: Vec<(usize, usize)> = roots.iter().rev().map(|&i| (i, 0_usize)).collect();

    while let Some((i, depth)) = stack.pop() {
        if !seen.insert(i) {
            continue;
        }
        ordered.push(Step { span: &spans[i], depth });
        if let Some(kids) = children.get(&i) {
            for &kid in kids.iter().rev() {
                stack.push((kid, depth + 1));
            }
        }
    }

    // Only reachable when the parent links form a cycle. Showing those spans
    // flat is right: losing a step from a trace you are debugging is worse than
    // showing it without its place in the tree.
    for (i, span) in spans.iter().enumerate() {
        if !seen.contains(&i) {
            ordered.push(Step { span, depth: 0 });
        }
    }
    ordered
}

fn is_model(kind: SpanKind) -> bool {
    matches!(
        kind,
        SpanKind::ModelGeneration
            | SpanKind::ModelStep
            | SpanKind::ModelInference
            | SpanKind::ModelChunk
    )
}

#[allow(clippy::too_many_lines)]
fn render_run(detail: &TraceDetail, project: &str, payloads: &str, per_payload: usize) -> String {
    let now = OffsetDateTime::now_utc();
    let trace = &detail.trace;
    let steps = order_spans(&detail.spans);
    let mut out = Out::default();

    out.line(format!("run {}", trace.trace_id));

    let mut head: Vec<(&str, String)> = vec![
        ("project", project.to_owned()),
        ("name", format!("{} ({})", trace.name, trace.kind)),
        (
            "status",
            if trace.is_complete {
                trace.status.as_str().to_owned()
            } else {
                format!("{}, still running", trace.status)
            },
        ),
        ("started", fmt_when(trace.start_time, now)),
        ("duration", trace.duration_ns.map_or_else(|| "still running".to_owned(), fmt_ns)),
    ];

    let mut counts = vec![format!("{} steps", fmt_count(u64::from(trace.span_count)))];
    if trace.error_count > 0 {
        counts.push(format!("{} failed", fmt_count(u64::from(trace.error_count))));
    }
    counts.push(plural(u64::from(trace.model_count), "model call"));
    counts.push(plural(u64::from(trace.tool_count), "tool call"));
    head.push(("steps", counts.join(", ")));

    if let Some(usage) = &trace.usage {
        head.push((
            "tokens",
            format!(
                "{} in / {} out",
                fmt_count(usage.input_tokens.unwrap_or(0)),
                fmt_count(usage.output_tokens.unwrap_or(0))
            ),
        ));
    }
    if let Some(cost) = &trace.cost {
        if let Some(total) = cost.total_cost {
            head.push((
                "cost",
                format!(
                    "{} {}{}",
                    fmt_money(total),
                    cost.currency,
                    if cost.is_estimated { ", estimated from a price book" } else { "" }
                ),
            ));
        }
    }

    let c = &trace.correlation;
    let mut context = Vec::new();
    for (label, value) in [
        ("environment", &c.environment),
        ("service", &c.service_name),
        ("release", &c.release),
        ("session", &c.session_id),
        ("thread", &c.thread_id),
        ("user", &c.user_id),
        ("experiment", &c.experiment_id),
    ] {
        if let Some(v) = value {
            context.push(format!("{label}={v}"));
        }
    }
    if !context.is_empty() {
        head.push(("context", context.join(" ")));
    }
    if let Some(tags) = &trace.tags {
        if !tags.is_empty() {
            head.push(("tags", tags.join(", ")));
        }
    }

    let width = head.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
    for (label, value) in &head {
        out.line(format!("{label:width$}  {value}"));
    }

    // The answer first. Everything below is the evidence for it.
    let failures: Vec<(usize, &Step<'_>)> = steps
        .iter()
        .enumerate()
        .filter(|(_, s)| s.span.status == SpanStatus::Error)
        .map(|(i, s)| (i + 1, s))
        .collect();
    if !failures.is_empty() {
        out.blank();
        out.line(format!("{} failed:", plural(failures.len() as u64, "step")));
        for (number, step) in failures.iter().take(20) {
            let message =
                step.span.error.as_ref().map_or("no message recorded", |e| e.message.as_str());
            out.line(format!(
                "  step {number}  {} ({}): {}",
                step.span.name,
                step.span.kind,
                one_line(message, 200)
            ));
        }
        if failures.len() > 20 {
            out.line(format!("  ... and {} more", failures.len() - 20));
        }
    }

    if steps.is_empty() {
        out.blank();
        out.line("No steps have arrived for this run yet.");
        return out.finish();
    }

    out.blank();
    for line in waterfall(&steps) {
        out.line(line);
    }

    let mut spent = 0_usize;
    let mut budget_noted = false;
    let mut detail_lines = Out::default();
    for (i, step) in steps.iter().enumerate() {
        render_step(
            &mut detail_lines,
            i + 1,
            step.span,
            payloads,
            per_payload,
            &mut spent,
            &mut budget_noted,
        );
    }
    let detail_lines = detail_lines.finish();
    if !detail_lines.is_empty() {
        out.blank();
        out.line("step detail");
        out.blank();
        out.line(detail_lines);
    }

    let numbers: HashMap<&str, usize> =
        steps.iter().enumerate().map(|(i, s)| (s.span.span_id.as_str(), i + 1)).collect();

    if let Some(logs) = &detail.logs {
        if !logs.is_empty() {
            out.blank();
            out.line(format!(
                "logs ({}{})",
                fmt_count(logs.len() as u64),
                if logs.len() > MAX_LOG_LINES {
                    format!(", showing the first {MAX_LOG_LINES}")
                } else {
                    String::new()
                }
            ));
            let mut table = Table::new(&["time", "level", "step", "message"], &[false; 4]);
            for log in logs.iter().take(MAX_LOG_LINES) {
                let step = log
                    .span_id
                    .as_deref()
                    .and_then(|id| numbers.get(id))
                    .map_or_else(|| "-".to_owned(), |n| n.to_string());
                table.row(vec![
                    fmt_clock(log.timestamp),
                    log.level.to_string(),
                    step,
                    one_line(&log.message, 220),
                ]);
            }
            for line in table.render("  ") {
                out.line(line);
            }
        }
    }

    if let Some(scores) = &detail.scores {
        if !scores.is_empty() {
            out.blank();
            out.line(format!("scores ({})", scores.len()));
            for score in scores {
                let scale = match (score.min, score.max) {
                    (Some(lo), Some(hi)) => format!(" of {lo} to {hi}"),
                    _ => String::new(),
                };
                let label = score.label.as_ref().map_or_else(String::new, |l| format!(" [{l}]"));
                out.line(format!(
                    "  {} = {}{scale}{label}, {}",
                    score.scorer_id, score.value, score.source
                ));
                if let Some(reason) = &score.reason {
                    out.line(format!("    reason: {}", one_line(reason, 400)));
                }
            }
        }
    }

    if let Some(feedback) = &detail.feedback {
        if !feedback.is_empty() {
            out.blank();
            out.line(format!("feedback ({})", feedback.len()));
            for item in feedback {
                let value = match &item.value {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                out.line(format!("  {}: {value}", item.kind));
                if let Some(comment) = &item.comment {
                    out.line(format!("    {}", one_line(comment, 400)));
                }
            }
        }
    }

    out.finish()
}

/// The waterfall: one row per step, indented by depth, with a bar drawn to
/// scale against the whole run.
fn waterfall(steps: &[Step<'_>]) -> Vec<String> {
    let base =
        steps.iter().map(|s| s.span.start_time).min().unwrap_or_else(OffsetDateTime::now_utc);
    let last =
        steps.iter().map(|s| s.span.end_time.unwrap_or(s.span.start_time)).max().unwrap_or(base);
    let total = (last - base).whole_nanoseconds().max(0);

    let mut lines =
        vec![format!("waterfall, {} across the bar", fmt_ns(u64::try_from(total).unwrap_or(0)))];

    let mut table = Table::new(
        &["#", "step", "kind", "status", "start", "duration", "timeline"],
        &[true, false, false, false, true, true, false],
    );
    for (i, step) in steps.iter().take(MAX_WATERFALL_ROWS).enumerate() {
        let span = step.span;
        let offset = (span.start_time - base).whole_nanoseconds().max(0);
        let duration = i128::from(span.duration_ns.unwrap_or(0));
        table.row(vec![
            (i + 1).to_string(),
            format!("{}{}", "  ".repeat(step.depth), span.name),
            span.kind.to_string(),
            span.status.as_str().to_owned(),
            fmt_offset(offset),
            if span.is_event {
                "event".to_owned()
            } else {
                span.duration_ns.map_or_else(|| "running".to_owned(), fmt_ns)
            },
            bar(offset, duration, total),
        ]);
    }
    lines.extend(table.render(""));
    if steps.len() > MAX_WATERFALL_ROWS {
        lines.push(format!("... and {} more steps", steps.len() - MAX_WATERFALL_ROWS));
    }
    lines
}

/// Draw one bar, positioned by offset and sized by duration.
///
/// A step always gets at least one character. A zero-width bar would say the
/// step never happened, and the fast steps are often the interesting ones.
fn bar(offset_ns: i128, duration_ns: i128, total_ns: i128) -> String {
    if total_ns <= 0 {
        return "#".to_owned();
    }
    let scale = |v: i128| (v as f64) / (total_ns as f64) * (BAR_WIDTH as f64);
    let lead = (scale(offset_ns).floor().max(0.0) as usize).min(BAR_WIDTH - 1);
    let len = (scale(duration_ns).round().max(1.0) as usize).min(BAR_WIDTH - lead);
    format!("{}{}", " ".repeat(lead), "#".repeat(len))
}

/// One step's detail block, or nothing when it has none worth printing.
fn render_step(
    out: &mut Out,
    number: usize,
    span: &Span,
    payloads: &str,
    per_payload: usize,
    spent: &mut usize,
    budget_noted: &mut bool,
) {
    let failed = span.status == SpanStatus::Error;
    let show_payloads = match payloads {
        "all" => true,
        "errors" => failed,
        _ => false,
    };
    let has_payload = span.input.is_some() || span.output.is_some();

    let mut body = Out::default();

    if let Some(error) = &span.error {
        body.line(format!("  error: {}", one_line(&error.message, 600)));
        let mut about = Vec::new();
        if let Some(kind) = &error.kind {
            about.push(format!("kind={kind}"));
        }
        if let Some(domain) = &error.domain {
            about.push(format!("domain={domain}"));
        }
        if let Some(retryable) = error.retryable {
            about.push(format!("retryable={retryable}"));
        }
        if !about.is_empty() {
            body.line(format!("  {}", about.join(" ")));
        }
        if let Some(stack) = &error.stack {
            body.line("  stack:");
            for line in clip(stack, 1200).lines() {
                body.line(format!("    {line}"));
            }
        }
    }

    if is_model(span.kind) {
        let mut about = Vec::new();
        if let Some(model) = &span.model {
            about.push(match &span.provider {
                Some(provider) => format!("model={model} ({provider})"),
                None => format!("model={model}"),
            });
        }
        if let Some(usage) = &span.usage {
            about.push(format!(
                "tokens={} in / {} out",
                fmt_count(usage.input_tokens.unwrap_or(0)),
                fmt_count(usage.output_tokens.unwrap_or(0))
            ));
        }
        if let Some(total) = span.cost.as_ref().and_then(|c| c.total_cost) {
            about.push(format!("cost={}", fmt_money(total)));
        }
        if !about.is_empty() {
            body.line(format!("  {}", about.join("  ")));
        }
    }

    if show_payloads && has_payload {
        let model_call = is_model(span.kind);
        for (label, value) in [
            (if model_call { "prompt" } else { "input" }, &span.input),
            (if model_call { "completion" } else { "output" }, &span.output),
        ] {
            let Some(value) = value else { continue };
            if *spent >= RUN_PAYLOAD_BUDGET {
                if !*budget_noted {
                    *budget_noted = true;
                    body.line(
                        "  payloads below this point are omitted: this run's payload budget \
                         is spent. Call get_run again with payloads=\"errors\", or read one \
                         step at a time.",
                    );
                }
                break;
            }
            let allowance = per_payload.min(RUN_PAYLOAD_BUDGET - *spent);
            let text = clip(&payload_text(value), allowance);
            *spent += text.len();
            body.line(format!("  {label}:"));
            for line in text.lines() {
                body.line(format!("    {line}"));
            }
        }
    }

    let body = body.finish();
    if body.is_empty() {
        return;
    }
    out.line(format!("step {number}  {}  {}  {}", span.name, span.kind, span.status.as_str()));
    out.line(body);
    out.blank();
}

/// Render a stored payload as prose rather than as JSON.
///
/// A prompt arrives as `{"messages": [{"role": ..., "content": ...}]}` more
/// often than as anything else, and printing that verbatim spends several
/// hundred tokens of the agent's context on braces to say what four lines of
/// `role: text` say. Anything unrecognised falls back to pretty JSON, which is
/// still better than one long line.
fn payload_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Array(items) if items.iter().all(is_message) => {
            items.iter().map(message_line).collect::<Vec<_>>().join("\n")
        }
        Value::Object(map) => match map.get("messages") {
            Some(messages @ Value::Array(items)) if items.iter().all(is_message) => {
                payload_text(messages)
            }
            // A completion is usually one message on its own, and when the model
            // called a tool it is the tool call that says what the run did next.
            _ if is_message(value) => message_line(value),
            _ => serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string()),
        },
        other => serde_json::to_string_pretty(other).unwrap_or_else(|_| other.to_string()),
    }
}

fn is_message(value: &Value) -> bool {
    value.get("role").and_then(Value::as_str).is_some()
        && (value.get("content").is_some() || value.get("tool_calls").is_some())
}

fn message_line(value: &Value) -> String {
    let role = value.get("role").and_then(Value::as_str).unwrap_or("?");
    let mut parts = Vec::new();

    let text = content_text(value.get("content"));
    if !text.is_empty() {
        parts.push(text);
    }
    // The step an agent took next. A looping agent shows up as the same call
    // three times, which is exactly the thing worth seeing at a glance.
    for call in value.get("tool_calls").and_then(Value::as_array).into_iter().flatten() {
        parts.push(format!("-> {}", tool_call_text(call)));
    }

    if parts.is_empty() {
        format!("{role}:")
    } else {
        format!("{role}: {}", parts.join("\n"))
    }
}

fn content_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(s)) => s.clone(),
        // The block form every current provider uses for multimodal turns.
        Some(Value::Array(blocks)) => blocks
            .iter()
            .map(|b| match b.get("text").and_then(Value::as_str) {
                Some(t) => t.to_owned(),
                None => format!("[{}]", b.get("type").and_then(Value::as_str).unwrap_or("block")),
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

/// One tool call, in both shapes the providers use: flat, and nested under
/// `function` with the arguments as a JSON string.
fn tool_call_text(call: &Value) -> String {
    let function = call.get("function");
    let name = call
        .get("name")
        .and_then(Value::as_str)
        .or_else(|| function.and_then(|f| f.get("name")).and_then(Value::as_str))
        .unwrap_or("tool");
    let arguments = call.get("arguments").or_else(|| function.and_then(|f| f.get("arguments")));
    let arguments = match arguments {
        Some(Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
        None => String::new(),
    };
    format!("{name}({arguments})")
}

// --------------------------------------------------------------- the window --

/// What one row of a grouped breakdown counts.
///
/// The store groups the model, provider and entity dimensions over spans and
/// every other dimension over traces. A single "runs" header would therefore be
/// wrong half the time, and a wrong column header is worse than none: an agent
/// reads "3 runs" and reports a number that was never runs.
const fn group_unit(dimension: &str) -> &'static str {
    match dimension.as_bytes() {
        b"model" | b"provider" | b"entity_name" => "steps",
        _ => "runs",
    }
}

fn render_stats(
    summary: &AnalyticsSummary,
    project: &str,
    group_by: Option<&str>,
    limit: usize,
) -> String {
    let mut out = Out::default();
    out.line(format!(
        "project {project:?}, {} to {}",
        fmt_time(summary.from),
        fmt_time(summary.to)
    ));
    out.blank();

    if summary.trace_count == 0 {
        out.line("No runs in that window.");
        out.blank();
        out.line("Widen it with since (-7d), or name another project.");
        return out.finish();
    }

    let mut rows: Vec<(&str, String)> = vec![
        ("runs", fmt_count(summary.trace_count)),
        (
            "failed",
            format!(
                "{} ({:.1}% of runs)",
                fmt_count(summary.error_count),
                summary.error_rate * 100.0
            ),
        ),
        ("steps", fmt_count(summary.span_count)),
    ];

    let l = &summary.latency;
    let mut latency = Vec::new();
    for (label, value) in
        [("p50", l.p50), ("p90", l.p90), ("p95", l.p95), ("p99", l.p99), ("max", l.max)]
    {
        if let Some(v) = value {
            latency.push(format!("{label} {}", fmt_millis(v)));
        }
    }
    if !latency.is_empty() {
        rows.push(("latency", latency.join("  ")));
    }
    rows.push((
        "tokens",
        format!(
            "{} in / {} out",
            fmt_count(summary.input_tokens),
            fmt_count(summary.output_tokens)
        ),
    ));
    rows.push(("cost", format!("{} {}", fmt_money(summary.total_cost), summary.currency)));

    let width = rows.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
    for (label, value) in &rows {
        out.line(format!("{label:width$}  {value}"));
    }

    match (group_by, &summary.groups) {
        (Some(dimension), Some(groups)) if !groups.is_empty() => {
            out.blank();
            let unit = group_unit(dimension);
            out.line(format!(
                "by {dimension}, {} of {}, most {unit} first",
                groups.len().min(limit),
                groups.len()
            ));
            let mut table = Table::new(
                &[dimension, unit, "failed", "cost", "tokens in", "tokens out", "slowest"],
                &[false, true, true, true, true, true, true],
            );
            for group in groups.iter().take(limit) {
                table.row(vec![
                    if group.key.is_empty() { "(none)".to_owned() } else { group.key.clone() },
                    fmt_count(group.count),
                    fmt_count(group.error_count),
                    group.total_cost.map_or_else(|| "-".to_owned(), fmt_money),
                    group.input_tokens.map_or_else(|| "-".to_owned(), fmt_count),
                    group.output_tokens.map_or_else(|| "-".to_owned(), fmt_count),
                    // The store fills `mean` and `max` for a group and leaves the
                    // quantiles empty, so a p95 column here would be a row of
                    // dashes pretending to be a measurement.
                    group
                        .latency
                        .as_ref()
                        .and_then(|l| l.max)
                        .map_or_else(|| "-".to_owned(), fmt_millis),
                ]);
            }
            for line in table.render("") {
                out.line(line);
            }
        }
        (Some(dimension), _) => {
            out.blank();
            out.line(format!("No {dimension} breakdown: nothing in the window carries one."));
        }
        (None, _) => {
            out.blank();
            out.line(
                "Pass group_by=\"model\" or \"name\" to see where the cost and the failures \
                 are concentrated.",
            );
        }
    }

    out.finish()
}

// ------------------------------------------------------------------ text --

/// A growing block of output.
///
/// `writeln!` into a `String` cannot fail, so using it here would scatter a
/// hundred discarded results through the rendering code for no gain.
#[derive(Default)]
struct Out(String);

impl Out {
    fn line(&mut self, text: impl AsRef<str>) {
        self.0.push_str(text.as_ref());
        self.0.push('\n');
    }

    fn blank(&mut self) {
        self.0.push('\n');
    }

    fn finish(self) -> String {
        self.0.trim_end().to_owned()
    }
}

/// A fixed-width text table.
///
/// Columns are sized to their contents so a model reads down a column instead
/// of counting commas. The last column is never padded, so nothing carries
/// trailing whitespace that a transport might trim.
struct Table {
    head: Vec<String>,
    right: Vec<bool>,
    rows: Vec<Vec<String>>,
}

impl Table {
    fn new(head: &[&str], right: &[bool]) -> Self {
        Self {
            head: head.iter().map(|s| (*s).to_owned()).collect(),
            right: right.to_vec(),
            rows: Vec::new(),
        }
    }

    fn row(&mut self, cells: Vec<String>) {
        self.rows.push(cells);
    }

    fn render(&self, indent: &str) -> Vec<String> {
        let count = self.head.len();
        let mut widths: Vec<usize> = self.head.iter().map(|h| h.chars().count()).collect();
        for row in &self.rows {
            for (i, cell) in row.iter().enumerate().take(count) {
                widths[i] = widths[i].max(cell.chars().count());
            }
        }

        let render_row = |cells: &[String]| {
            let mut line = String::from(indent);
            for (i, cell) in cells.iter().enumerate().take(count) {
                let pad = widths[i].saturating_sub(cell.chars().count());
                let last = i + 1 == count;
                if self.right.get(i).copied().unwrap_or(false) {
                    line.push_str(&" ".repeat(pad));
                    line.push_str(cell);
                } else {
                    line.push_str(cell);
                    if !last {
                        line.push_str(&" ".repeat(pad));
                    }
                }
                if !last {
                    line.push_str("  ");
                }
            }
            line.trim_end().to_owned()
        };

        let mut lines = vec![render_row(&self.head)];
        lines.extend(self.rows.iter().map(|r| render_row(r)));
        lines
    }
}

/// Cut text to a character budget and say how much was cut.
///
/// Cutting silently is the failure that matters: an agent that cannot see the
/// cut treats a truncated prompt as the whole prompt.
fn clip(text: &str, budget: usize) -> String {
    let total = text.chars().count();
    if total <= budget {
        return text.to_owned();
    }
    let kept: String = text.chars().take(budget).collect();
    format!("{kept}\n... cut here, {} more characters", fmt_count((total - budget) as u64))
}

/// Flatten to one line, for a table cell or a summary.
fn one_line(text: &str, budget: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= budget {
        return flat;
    }
    format!("{}...", flat.chars().take(budget).collect::<String>())
}

fn plural(n: u64, noun: &str) -> String {
    format!("{} {noun}{}", fmt_count(n), if n == 1 { "" } else { "s" })
}

/// Group digits, because `1204` and `12040` are hard to tell apart in a column.
fn fmt_count(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// A duration in the largest unit that keeps it readable.
fn fmt_ns(ns: u64) -> String {
    let f = ns as f64;
    if ns < 10_000 {
        format!("{ns}ns")
    } else if ns < 1_000_000 {
        format!("{:.2}ms", f / 1e6)
    } else if ns < 1_000_000_000 {
        format!("{:.0}ms", f / 1e6)
    } else if ns < 60_000_000_000 {
        format!("{:.2}s", f / 1e9)
    } else {
        format!("{}m{:02}s", ns / 60_000_000_000, (ns / 1_000_000_000) % 60)
    }
}

/// A latency in milliseconds, as the analytics endpoint reports it.
fn fmt_millis(ms: f64) -> String {
    if ms.is_sign_negative() || !ms.is_finite() {
        return "-".to_owned();
    }
    fmt_ns((ms * 1e6) as u64)
}

fn fmt_offset(ns: i128) -> String {
    format!("+{:.3}s", (ns as f64) / 1e9)
}

fn fmt_money(v: f64) -> String {
    if v == 0.0 {
        "$0".to_owned()
    } else if v.abs() < 0.01 {
        format!("${v:.6}")
    } else if v.abs() < 1.0 {
        format!("${v:.4}")
    } else {
        format!("${v:.2}")
    }
}

fn fmt_time(t: OffsetDateTime) -> String {
    let t = t.to_offset(time::UtcOffset::UTC);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}Z",
        t.year(),
        u8::from(t.month()),
        t.day(),
        t.hour(),
        t.minute(),
        t.second()
    )
}

fn fmt_clock(t: OffsetDateTime) -> String {
    let t = t.to_offset(time::UtcOffset::UTC);
    format!("{:02}:{:02}:{:02}.{:03}Z", t.hour(), t.minute(), t.second(), t.millisecond())
}

/// How long ago, in one unit. An agent should not have to subtract timestamps.
fn fmt_age(t: OffsetDateTime, now: OffsetDateTime) -> String {
    let seconds = (now - t).whole_seconds();
    if seconds < 0 {
        return "ahead".to_owned();
    }
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m", seconds / 60)
    } else if seconds < 86_400 {
        format!("{}h", seconds / 3600)
    } else {
        format!("{}d", seconds / 86_400)
    }
}

fn fmt_when(t: OffsetDateTime, now: OffsetDateTime) -> String {
    let age = fmt_age(t, now);
    if age == "ahead" {
        format!("{} (ahead of this clock)", fmt_time(t))
    } else {
        format!("{} ({age} ago)", fmt_time(t))
    }
}

// ------------------------------------------------------------------ tests --

#[cfg(test)]
mod tests {
    use super::*;
    use mcpd_core::{Correlation, SpanStatus};
    use time::Duration;

    fn span(id: &str, parent: Option<&str>, name: &str, offset_s: i64) -> Span {
        let base = OffsetDateTime::UNIX_EPOCH + Duration::days(20_000);
        Span {
            trace_id: "t".repeat(32),
            span_id: id.to_owned(),
            parent_span_id: parent.map(str::to_owned),
            external_parent_span_id: None,
            name: name.to_owned(),
            kind: SpanKind::ToolCall,
            status: SpanStatus::Ok,
            start_time: base + Duration::seconds(offset_s),
            end_time: Some(base + Duration::seconds(offset_s + 1)),
            duration_ns: Some(1_000_000_000),
            is_root: parent.is_none(),
            is_event: false,
            entity_kind: None,
            entity_id: None,
            entity_name: None,
            input: None,
            output: None,
            attributes: None,
            metadata: None,
            usage: None,
            cost: None,
            model_parameters: None,
            error: None,
            links: None,
            tags: None,
            correlation: Correlation {
                project: "p".to_owned(),
                environment: None,
                service_name: None,
                service_version: None,
                session_id: None,
                thread_id: None,
                user_id: None,
                resource_id: None,
                run_id: None,
                request_id: None,
                experiment_id: None,
                release: None,
            },
            model: None,
            provider: None,
            received_at: base,
            ingest_key: None,
        }
    }

    #[test]
    fn the_tree_is_parents_then_children_then_siblings_by_time() {
        let spans = vec![
            span("bbbbbbbbbbbbbbbb", Some("aaaaaaaaaaaaaaaa"), "second", 2),
            span("cccccccccccccccc", Some("aaaaaaaaaaaaaaaa"), "first", 1),
            span("aaaaaaaaaaaaaaaa", None, "root", 0),
        ];
        let ordered = order_spans(&spans);
        let names: Vec<&str> = ordered.iter().map(|s| s.span.name.as_str()).collect();
        assert_eq!(names, ["root", "first", "second"]);
        assert_eq!(ordered.iter().map(|s| s.depth).collect::<Vec<_>>(), [0, 1, 1]);
    }

    #[test]
    fn a_parent_cycle_does_not_hang_and_loses_no_step() {
        // Producer-supplied ids. A cycle must not cost the reader a step, and
        // must not cost the process its stack.
        let spans = vec![
            span("aaaaaaaaaaaaaaaa", Some("bbbbbbbbbbbbbbbb"), "one", 0),
            span("bbbbbbbbbbbbbbbb", Some("aaaaaaaaaaaaaaaa"), "two", 1),
        ];
        assert_eq!(order_spans(&spans).len(), 2);
    }

    #[test]
    fn a_span_that_parents_itself_is_a_root() {
        let spans = vec![span("aaaaaaaaaaaaaaaa", Some("aaaaaaaaaaaaaaaa"), "self", 0)];
        let ordered = order_spans(&spans);
        assert_eq!(ordered.len(), 1);
        assert_eq!(ordered[0].depth, 0);
    }

    #[test]
    fn an_orphan_still_appears() {
        let spans = vec![span("aaaaaaaaaaaaaaaa", Some("dddddddddddddddd"), "orphan", 0)];
        assert_eq!(order_spans(&spans).len(), 1);
    }

    #[test]
    fn a_bar_is_positioned_and_never_empty() {
        // A step at the very end of a run, one nanosecond long: it still has to
        // be visible, and it has to sit at the right-hand end.
        let b = bar(99, 1, 100);
        assert!(b.starts_with(&" ".repeat(BAR_WIDTH - 1)));
        assert_eq!(b.trim_start(), "#");
        assert_eq!(b.chars().count(), BAR_WIDTH);

        assert_eq!(bar(0, 100, 100).chars().count(), BAR_WIDTH);
        assert_eq!(bar(0, 0, 0), "#");
    }

    #[test]
    fn a_bar_never_runs_past_its_track() {
        // A child that outlives its parent, which storage permits, used to
        // produce a bar wider than the column and broke every row's alignment.
        assert_eq!(bar(90, 1000, 100).chars().count(), BAR_WIDTH);
    }

    #[test]
    fn the_negotiated_version_is_the_one_asked_for_when_supported() {
        let old = initialize(&json!({ "protocolVersion": "2024-11-05" }));
        assert_eq!(old["protocolVersion"], "2024-11-05");

        let new = initialize(&json!({ "protocolVersion": "2025-06-18" }));
        assert_eq!(new["protocolVersion"], "2025-06-18");

        // Anything else, including a version we have never heard of, is answered
        // with the newest one we speak. The client then decides.
        let unknown = initialize(&json!({ "protocolVersion": "1999-01-01" }));
        assert_eq!(unknown["protocolVersion"], PROTOCOL_VERSIONS[0]);
        assert_eq!(initialize(&json!({}))["protocolVersion"], PROTOCOL_VERSIONS[0]);
    }

    #[test]
    fn every_tool_declares_a_schema_and_reads_nothing_else() {
        let tools = tools();
        assert_eq!(tools.len(), 3, "the tool set is deliberately three");
        for tool in &tools {
            assert!(tool["name"].is_string());
            assert!(tool["description"].as_str().unwrap().len() > 40);
            assert_eq!(tool["inputSchema"]["type"], "object");
            assert_eq!(tool["inputSchema"]["additionalProperties"], false);
            assert_eq!(tool["annotations"]["readOnlyHint"], true);
        }
    }

    #[test]
    fn the_declared_schema_and_the_accepted_arguments_agree() {
        // Two lists that must not drift: the schema an agent reads, and the
        // allowlist the call checks against. A property in one and not the
        // other is either an argument nobody can pass or one nobody validates.
        for (tool, allowed) in tools().iter().zip([FIND_RUNS_ARGS, GET_RUN_ARGS, RUN_STATS_ARGS]) {
            let declared: Vec<&str> = tool["inputSchema"]["properties"]
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            let mut sorted = declared.clone();
            sorted.sort_unstable();
            let mut expected = allowed.to_vec();
            expected.sort_unstable();
            assert_eq!(sorted, expected, "{} drifted", tool["name"]);
        }
    }

    #[test]
    fn an_unknown_argument_is_refused_by_name() {
        let e = known_args(&json!({ "trace": "abc" }), GET_RUN_ARGS).unwrap_err();
        assert!(e.message.contains("\"trace\""), "{}", e.message);
        assert!(e.message.contains("trace_id"), "{}", e.message);
        assert!(known_args(&json!({ "trace_id": "abc" }), GET_RUN_ARGS).is_ok());
    }

    #[test]
    fn chat_payloads_read_as_prose() {
        let value = json!({
            "messages": [
                { "role": "system", "content": "You are terse." },
                { "role": "user", "content": [{ "type": "text", "text": "where is order 41?" }] },
            ]
        });
        assert_eq!(payload_text(&value), "system: You are terse.\nuser: where is order 41?");
    }

    #[test]
    fn a_completion_shows_the_tool_the_model_called() {
        // The flat shape, and the nested one with the arguments as a string.
        let flat = json!({
            "role": "assistant",
            "tool_calls": [{ "name": "returns_policy", "arguments": { "sku": "AB" } }]
        });
        assert_eq!(payload_text(&flat), "assistant: -> returns_policy({\"sku\":\"AB\"})");

        let nested = json!({
            "role": "assistant",
            "content": "checking",
            "tool_calls": [{ "function": { "name": "lookup", "arguments": "{}" } }]
        });
        assert_eq!(payload_text(&nested), "assistant: checking\n-> lookup({})");
    }

    #[test]
    fn a_payload_that_is_not_a_conversation_stays_json() {
        let value = json!({ "order_id": 41 });
        assert!(payload_text(&value).contains("\"order_id\""));
        assert_eq!(payload_text(&json!("plain text")), "plain text");
    }

    #[test]
    fn a_cut_payload_says_it_was_cut() {
        let long = "x".repeat(50);
        let cut = clip(&long, 10);
        assert!(cut.starts_with(&"x".repeat(10)));
        assert!(cut.contains("40 more characters"));
        assert_eq!(clip("short", 10), "short");
    }

    #[test]
    fn table_columns_line_up() {
        let mut t = Table::new(&["a", "count"], &[false, true]);
        t.row(vec!["one".to_owned(), "1".to_owned()]);
        t.row(vec!["twenty".to_owned(), "20".to_owned()]);
        let lines = t.render("");
        assert_eq!(lines[0], "a       count");
        assert_eq!(lines[1], "one         1");
        assert_eq!(lines[2], "twenty     20");
        // Nothing may carry trailing whitespace: a transport that trims it would
        // silently change the alignment of every row.
        assert!(lines.iter().all(|l| l.trim_end() == *l));
    }

    #[test]
    fn durations_and_counts_read_at_a_glance() {
        assert_eq!(fmt_ns(500), "500ns");
        assert_eq!(fmt_ns(500_000), "0.50ms");
        assert_eq!(fmt_ns(120_000_000), "120ms");
        assert_eq!(fmt_ns(4_210_000_000), "4.21s");
        assert_eq!(fmt_ns(64_000_000_000), "1m04s");
        assert_eq!(fmt_count(1_204), "1,204");
        assert_eq!(fmt_count(0), "0");
        assert_eq!(fmt_money(0.0121), "$0.0121");
        assert_eq!(fmt_money(18.4), "$18.40");
        assert_eq!(fmt_money(0.0), "$0");
    }

    #[test]
    fn a_grouped_row_is_labelled_with_what_it_actually_counts() {
        assert_eq!(group_unit("model"), "steps");
        assert_eq!(group_unit("provider"), "steps");
        assert_eq!(group_unit("name"), "runs");
        assert_eq!(group_unit("environment"), "runs");
    }

    #[test]
    fn ages_are_reported_in_one_unit() {
        let now = OffsetDateTime::UNIX_EPOCH + Duration::days(20_000);
        assert_eq!(fmt_age(now - Duration::seconds(30), now), "30s");
        assert_eq!(fmt_age(now - Duration::minutes(5), now), "5m");
        assert_eq!(fmt_age(now - Duration::hours(4), now), "4h");
        assert_eq!(fmt_age(now - Duration::days(3), now), "3d");
        // A producer clock ahead of this one must not print a negative age.
        assert_eq!(fmt_age(now + Duration::hours(1), now), "ahead");
    }

    #[test]
    fn a_notification_is_never_answered() {
        let daemon = Daemon::new(
            &Args { endpoint: "http://127.0.0.1:1".to_owned(), api_key: None },
            "default",
        )
        .unwrap();
        let notification = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
        let reply = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(handle(&daemon, notification));
        assert!(reply.is_none());
    }

    #[test]
    fn protocol_mistakes_come_back_as_json_rpc_errors() {
        let daemon = Daemon::new(
            &Args { endpoint: "http://127.0.0.1:1".to_owned(), api_key: None },
            "default",
        )
        .unwrap();
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let call = |line: &str| rt.block_on(handle(&daemon, line)).unwrap();

        assert_eq!(call("not json")["error"]["code"], -32700);
        assert_eq!(call(r#"{"jsonrpc":"2.0","id":1}"#)["error"]["code"], -32600);
        assert_eq!(call(r#"[{"jsonrpc":"2.0","id":1,"method":"ping"}]"#)["error"]["code"], -32600);
        assert_eq!(
            call(r#"{"jsonrpc":"2.0","id":1,"method":"resources/list"}"#)["error"]["code"],
            -32601
        );
        assert_eq!(
            call(r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"nope"}}"#)
                ["error"]["code"],
            -32602
        );
        assert_eq!(call(r#"{"jsonrpc":"2.0","id":7,"method":"ping"}"#)["id"], 7);
        assert_eq!(
            call(r#"{"jsonrpc":"2.0","id":"a","method":"tools/list"}"#)["result"]["tools"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
    }

    #[test]
    fn a_daemon_that_is_not_there_is_a_readable_tool_failure() {
        // Not a JSON-RPC error: the model has to see this and act on it, and a
        // transport error only ever reaches the host.
        let daemon = Daemon::new(
            &Args { endpoint: "http://127.0.0.1:1".to_owned(), api_key: None },
            "default",
        )
        .unwrap();
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let reply = rt
            .block_on(handle(
                &daemon,
                r#"{"jsonrpc":"2.0","id":1,"method":"tools/call",
                    "params":{"name":"run_stats","arguments":{}}}"#,
            ))
            .unwrap();
        assert_eq!(reply["result"]["isError"], true);
        let text = reply["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("Cannot reach the mcpd daemon"), "{text}");
        assert!(text.contains("MCPD_ENDPOINT"), "{text}");
    }
}
