//! The ClickHouse backend.
//!
//! For deployments where retention is measured in billions of spans rather than
//! millions. The embedded SQLite store is a single writer on a single file,
//! which is exactly right on a laptop and a hard ceiling in a cluster.
//!
//! Everything the daemon does above the [`Store`](crate::Store) trait is
//! unchanged: the trait is deliberately narrow so a second backend is one
//! implementation and nothing else. Talking to ClickHouse over its HTTP
//! interface rather than the native protocol keeps that true, since it means no
//! driver, no connection pool to tune, and a wire format anyone can reproduce
//! with `curl` while debugging.

use crate::{
    decode_delta_cursor, encode_delta_cursor, hashed_id_regex, is_hashed_identifier,
    unmatchable_identifiers, Aggregation, AspectNoteQuery, AspectNoteRollup, AspectRollupQuery,
    AuditQuery, Batch, Delta, DeltaQuery, DeltaSignal, Discovered, DiscoveryQuery, ErasureOutcome,
    Error, IdentifierProbe, Include, LogQuery, MetricQuery, Order, Paged, Partitioning,
    PolledSignal, Result, RetentionOutcome, RetentionPolicy, ScoreQuery, SnapshotQuery,
    SnapshotWrite, SpanQuery, SpanSort, Store, StoreCapabilities, StoredNote, StoredTheme,
    TextSearch, ThemeLineage, ThemeMember, ThemeMemberQuery, ThemeQuery, TraceQuery, TraceSort,
    ERASED_ITEM,
};
use mcpd_core::{
    AnalyticsSummary, AspectNote, AuditEvent, AuditResult, Correlation, Cost, CostBreakdown,
    CostBucket, Dataset, DatasetItem, EntityKind, ErasureResult, Experiment, ExperimentStatus,
    Feedback, FeedbackKind, GroupedStat, IngestResult, LatencyStats, LogLevel, LogRecord,
    MetricSeries, MetricSeriesPoint, MetricSeriesResponse, Score, ScoreSource, Span, SpanError,
    SpanKind, SpanLink, SpanStatus, Theme, ThemeSnapshot, ThemeState, TokenUsage, Trace,
    TraceDetail,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::str::FromStr;
use std::time::Duration;
use time::OffsetDateTime;

const SCHEMA: &str = include_str!("clickhouse_schema.sql");

/// How to reach a ClickHouse.
#[derive(Debug, Clone)]
pub struct ClickHouseConfig {
    /// Base URL of the HTTP interface, such as `http://clickhouse:8123`.
    pub url: String,
    /// Database name. Created on migrate if absent.
    pub database: String,
    /// Username, when the server requires one.
    pub user: Option<String>,
    /// Password.
    pub password: Option<String>,
    /// Per-request timeout.
    pub timeout: Duration,
    /// How long to keep data, enforced by the table's own TTL.
    /// Days to keep, as a table TTL. `None` emits no TTL at all.
    ///
    /// Optional because `--retention-days` documents "omit to keep everything",
    /// and a backend that quietly applied ninety days anyway would delete a
    /// quarter of somebody's history against an explicit promise. There is no
    /// safe default here: the only honest reading of "unset" is "keep it".
    pub retention_days: Option<u32>,
}

impl Default for ClickHouseConfig {
    fn default() -> Self {
        Self {
            url: "http://localhost:8123".to_owned(),
            database: "mcpd".to_owned(),
            user: None,
            password: None,
            timeout: Duration::from_secs(30),
            retention_days: None,
        }
    }
}

/// The schema version this build writes.
///
/// Recorded in `mcpd.schema_version` beside a fingerprint of the statements
/// that produced it, so `migrate` can tell "already applied" from "changed"
/// without reading every table's definition.
const SCHEMA_VERSION: u32 = 4;

// 4 adds `aspect_notes`, `theme_snapshots`, `themes` and `theme_members`. The
// fingerprint above covers the schema file, so appending the four tables is
// enough to make an existing deployment apply them on its next start.

/// A ClickHouse-backed store.
pub struct ClickHouseStore {
    config: ClickHouseConfig,
    http: reqwest::Client,
    /// Whether the signal tables are cut one partition per day.
    ///
    /// Read from the server in [`Store::migrate`] rather than assumed from the
    /// schema file. ClickHouse cannot change a partition key in place, so a
    /// database created before this build keeps whatever it had, and a
    /// retention pass that dropped a partition believing it held one day would
    /// drop a month. It starts true, which is what a fresh database gets;
    /// `migrate` runs before anything reads and corrects it.
    day_partitioned: std::sync::atomic::AtomicBool,
}

impl ClickHouseStore {
    /// Build a store. Does not connect; [`Store::migrate`] and
    /// [`Store::ping`] are where a bad configuration surfaces.
    ///
    /// # Errors
    /// Fails only if the HTTP client cannot be constructed.
    pub fn new(config: ClickHouseConfig) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(|e| Error::Backend(format!("building the HTTP client: {e}")))?;
        Ok(Self { config, http, day_partitioned: std::sync::atomic::AtomicBool::new(true) })
    }

    /// Execute a statement that returns nothing.
    async fn execute(&self, sql: &str) -> Result<()> {
        self.post(sql, None).await.map(|_| ())
    }

    /// Execute a query and return the raw response body.
    async fn post(&self, sql: &str, body: Option<String>) -> Result<String> {
        let mut req = self
            .http
            .post(&self.config.url)
            .query(&[("query", sql)])
            // Never let a runaway query take the server down with it.
            .query(&[("max_execution_time", "30")])
            // ClickHouse renders 64-bit integers as JSON strings by default, to
            // survive a reader whose numbers are doubles. Every 64-bit column
            // read here lands in a `u64`, so a quoted one is a deserialisation
            // failure rather than a rounding risk: `duration_ns` came back as
            // "8420000000" and no trace could be read at all. Rust has real
            // 64-bit integers, so ask for numbers.
            .query(&[("output_format_json_quote_64bit_integers", "0")]);

        if let (Some(u), Some(p)) = (&self.config.user, &self.config.password) {
            req = req.basic_auth(u, Some(p));
        } else if let Some(u) = &self.config.user {
            req = req.basic_auth(u, None::<&str>);
        }
        // The length is set by hand, and an absent body is sent as an empty one.
        // ClickHouse answers a POST carrying neither `Content-Length` nor
        // chunked encoding with `411 Length Required`, and reqwest puts neither
        // on the wire for a request whose body is absent or empty. Every
        // statement with no payload comes through here, `migrate` first of all,
        // so without this the backend never gets past its own schema.
        let body = body.unwrap_or_default();
        req = req.header(reqwest::header::CONTENT_LENGTH, body.len()).body(body);

        let res = req
            .send()
            .await
            .map_err(|e| Error::Backend(format!("clickhouse is unreachable: {e}")))?;

        let status = res.status();
        let text = res.text().await.unwrap_or_default();
        if !status.is_success() {
            // ClickHouse puts a useful message in the body; passing the status
            // alone would throw away the only part worth reading.
            return Err(Error::Backend(format!("clickhouse returned {status}: {}", text.trim())));
        }
        Ok(text)
    }

    /// Run a `SELECT` and deserialise each row from `JSONEachRow`.
    async fn select<T: for<'de> Deserialize<'de>>(&self, sql: &str) -> Result<Vec<T>> {
        let body = self.post(&format!("{sql} FORMAT JSONEachRow"), None).await?;
        let mut out = Vec::new();
        for line in body.lines().filter(|l| !l.trim().is_empty()) {
            out.push(
                serde_json::from_str(line)
                    .map_err(|e| Error::Backend(format!("unreadable row: {e}: {line}")))?,
            );
        }
        Ok(out)
    }

    fn table(&self, name: &str) -> String {
        format!("{}.{name}", self.config.database)
    }

    /// Count the rows a predicate selects, then delete them.
    ///
    /// The count is taken first because a ClickHouse delete reports no row
    /// count over HTTP. Counting afterwards would race the mutation, and
    /// returning a fixed number would be a lie about what an erasure removed.
    ///
    /// The predicate is interpolated, so every value inside it must already
    /// have been through [`quote`].
    async fn count_then_delete(&self, table: &str, predicate: &str) -> Result<u32> {
        // `FINAL` on the count, never on the delete.
        //
        // `spans`, `traces`, `scores` and `feedback` are ReplacingMergeTree, so
        // the same key can sit in the table more than once until a merge
        // collapses it. A bare `count()` counts the copies, and the number this
        // endpoint returns is the number an operator repeats to a person who
        // asked what was held about them. It was wrong by however many
        // re-ingested rows had not merged yet. `is_replacing` decides per
        // table, so `logs` and `metrics`, which are plain MergeTree, do not pay
        // for a merge they do not need.
        let f = if is_replacing(table) { " FINAL" } else { "" };
        let counted: Vec<CountRow> = self
            .select(&format!(
                "SELECT toUInt32(count()) AS n FROM {}{f} WHERE {predicate}",
                self.table(table)
            ))
            .await?;
        let n = counted.first().map_or(0, |c| c.n);
        self.execute(&format!("DELETE FROM {} WHERE {predicate}", self.table(table))).await?;
        Ok(n)
    }

    /// The same, waiting for the mutation to finish before returning.
    ///
    /// Retention runs on a timer rather than on a request, so it can afford to
    /// wait, and waiting is what makes its count mean something: a pass that
    /// returned before the mutation applied would report the same rows again on
    /// its next tick and never settle. `erase_subject` and `delete_trace`
    /// deliberately do not wait, which is what
    /// [`StoreCapabilities::immediate_deletes`] reports as false.
    async fn count_then_delete_now(&self, table: &str, predicate: &str) -> Result<u32> {
        let f = if is_replacing(table) { " FINAL" } else { "" };
        let counted: Vec<CountRow> = self
            .select(&format!(
                "SELECT toUInt32(count()) AS n FROM {}{f} WHERE {predicate}",
                self.table(table)
            ))
            .await?;
        let n = counted.first().map_or(0, |c| c.n);
        self.execute(&format!(
            "DELETE FROM {} WHERE {predicate} SETTINGS mutations_sync = 2",
            self.table(table)
        ))
        .await?;
        Ok(n)
    }

    /// What the project still holds that names a person, read after an erasure.
    ///
    /// The same three questions the embedded store asks, in the same order and
    /// by the same rule. Two are existence checks that stop at the first row of
    /// each table, and only the third reads anything in bulk: the single union
    /// that answered all three at once read every identifier in the project
    /// whatever the answer turned out to be.
    ///
    /// The deletes above are asynchronous, so this can still see rows this call
    /// has already removed. That only ever makes the answer more cautious,
    /// never less. The subject's own identifier is excluded from the second
    /// question for the same reason: it is the one this request has addressed,
    /// and counting it would report every erasure as incomplete on this backend
    /// alone.
    async fn probe_identifiers(&self, project: &str, subject: &str) -> Result<IdentifierProbe> {
        #[derive(Deserialize)]
        struct IdRow {
            id: String,
        }

        let hashed = is_hashed_identifier(subject);
        let [attributed, opaque, candidates] =
            probe_statements(&|t| self.table(t), project, subject, hashed);

        let counted: Vec<CountRow> = self.select(&attributed).await?;
        let nothing_attributed = counted.first().map_or(0, |r| r.n) == 0;

        let opaque_remaining = if hashed {
            let counted: Vec<CountRow> = self.select(&opaque).await?;
            counted.first().map_or(0, |r| r.n) > 0
        } else {
            false
        };

        let candidates: Vec<IdRow> = self.select(&candidates).await?;
        let candidates: Vec<String> = candidates.into_iter().map(|r| r.id).collect();

        Ok(IdentifierProbe {
            nothing_attributed,
            unmatchable: unmatchable_identifiers(subject, &candidates),
            opaque_remaining,
        })
    }
}

/// The four columns that name a person. A column missing from this list is a
/// person the identifier probe cannot see.
const ATTRIBUTING_COLUMNS: [(&str, &str); 4] =
    [("traces", "user_id"), ("spans", "user_id"), ("logs", "user_id"), ("feedback", "author_id")];

/// The three statements the identifier probe runs, built in one place.
///
/// Extracted so a test can read them without a server. This interface takes SQL
/// as text and has no bind parameters, so the subject reaches the statement
/// through [`quote`] like every other value here, and being able to look at the
/// result is worth more than the indirection costs.
///
/// `hashed` is whether the subject this erasure carried is a digest, by
/// [`is_hashed_identifier`]. It decides which form is the other one.
fn probe_statements(
    table: &dyn Fn(&str) -> String,
    project: &str,
    subject: &str,
    hashed: bool,
) -> [String; 3] {
    let p = quote(project);
    let f = |t: &str| if is_replacing(t) { " FINAL" } else { "" };

    // Each arm stops at its first row, so a yes costs one row per table and a
    // no costs one scan, rather than a union that reads every identifier.
    let exists = |predicate: &dyn Fn(&str) -> String| -> String {
        let arms = ATTRIBUTING_COLUMNS
            .iter()
            .map(|(t, c)| {
                format!(
                    "SELECT 1 AS hit FROM {}{} WHERE project = {p} AND {} LIMIT 1",
                    table(t),
                    f(t),
                    predicate(c)
                )
            })
            .collect::<Vec<_>>()
            .join(" UNION ALL ");
        format!("SELECT toUInt32(count()) AS n FROM ({arms})")
    };

    let attributed = exists(&|c| format!("{c} <> ''"));
    // A digest other than the subject's own. Excluding the subject is what
    // stops this reporting the rows this erasure has just addressed, and it is
    // why the answer says nothing about whether the erasure worked.
    //
    // The whole shape, not the prefix. `LIKE 'sha256:%'` decides the question
    // rather than narrowing it, and decides it wrongly: an identifier a caller
    // hashed themselves carries the prefix, [`unmatchable_identifiers`] counts
    // it as the other form, and `NOT LIKE` dropped it before the rule could see
    // it. `match` against [`hashed_id_regex`] is anchored by the pattern and
    // case sensitive, so it selects exactly the rows the rule puts on the other
    // side.
    let shape = hashed_id_regex();
    let u = quote(subject);
    let opaque = exists(&|c| format!("match({c}, '{shape}') AND {c} <> {u}"));

    // Only identifiers of the other form are read. In a deployment that never
    // changed its redaction setting that is no rows at all, where this used to
    // read every identifier the project held.
    let form = if hashed { "NOT " } else { "" };
    let arms = ATTRIBUTING_COLUMNS
        .iter()
        .map(|(t, c)| {
            format!(
                "SELECT {c} AS id FROM {}{} WHERE project = {p}
                 AND {c} <> '' AND {form}match({c}, '{shape}')",
                table(t),
                f(t)
            )
        })
        .collect::<Vec<_>>()
        .join(" UNION ALL ");

    [attributed, opaque, format!("SELECT DISTINCT id FROM ({arms})")]
}

/// A schema statement without the comment lines that introduce it.
///
/// The schema file explains a table above its `CREATE`, and splitting the file
/// on semicolons leaves that explanation at the head of the statement, where a
/// `starts_with` looking for the `CREATE` never finds it.
fn without_leading_comments(sql: &str) -> &str {
    let mut rest = sql.trim_start();
    while let Some(after) = rest.strip_prefix("--") {
        rest = after.split_once('\n').map_or("", |(_, tail)| tail).trim_start();
    }
    rest
}

/// Whether a table is a ReplacingMergeTree, and so needs `FINAL` to be counted.
///
/// Taken from `clickhouse_schema.sql`. Keep the two in step: a table that
/// becomes replacing and is not named here is counted with its duplicates, and
/// the count of an erasure is the one number a person is told.
fn is_replacing(table: &str) -> bool {
    matches!(
        table,
        "spans"
            | "traces"
            | "scores"
            | "feedback"
            | "datasets"
            | "dataset_items"
            | "aspect_notes"
            | "theme_snapshots"
            | "themes"
            | "theme_members"
    )
}

/// Remove a `TTL ...` clause from a `CREATE TABLE` statement.
///
/// The clause is the last thing before the terminating semicolon in every
/// statement in the schema file, so cutting from `TTL ` to the end is exact.
/// Done textually rather than by templating the schema, so the file stays a
/// file somebody can read and run by hand.
fn strip_ttl(sql: &str) -> String {
    match sql.find("\nTTL ") {
        Some(i) => {
            let mut out = sql[..i].to_owned();
            out.push(';');
            out
        }
        None => sql.to_owned(),
    }
}

/// A single `count()`.
#[derive(Deserialize)]
struct CountRow {
    n: u32,
}

/// Quote a string for SQL.
///
/// Every value reaching a query goes through this. ClickHouse's HTTP interface
/// takes SQL as text with no bind parameters, so this is the only thing between
/// a filter value and an injection, and it is applied without exception.
fn quote(v: &str) -> String {
    let mut out = String::with_capacity(v.len() + 2);
    out.push('\'');
    for c in v.chars() {
        match c {
            '\'' => out.push_str("\\'"),
            '\\' => out.push_str("\\\\"),
            // Control characters are never meaningful in a filter and are the
            // usual shape of an attempt to break out of one.
            c if (c as u32) < 0x20 => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('\'');
    out
}

/// Nanoseconds since the epoch, clamped rather than wrapped.
///
/// The other two backends store this number; here it only ever travels inside
/// a poll cursor, so the two ends of one cursor agree and nothing else reads it.
fn to_nanos(t: OffsetDateTime) -> i64 {
    t.unix_timestamp_nanos().clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
}

fn from_nanos(n: i64) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(n)).unwrap_or(OffsetDateTime::UNIX_EPOCH)
}

/// A timestamp as a literal that keeps every nanosecond.
///
/// [`ts`] renders the value through an `f64`, which holds about sixteen
/// significant digits where a nanosecond timestamp needs nineteen. That is
/// close enough for a window a person chose and not close enough for a
/// watermark: a comparison off by a few nanoseconds either replays the last row
/// of every poll or skips it. This goes through the text form instead, which is
/// exact.
fn ts_exact(t: OffsetDateTime) -> String {
    format!("toDateTime64({}, 9, 'UTC')", quote(&ts_str(t)))
}

/// Render a timestamp as ClickHouse's nanosecond datetime literal.
fn ts(t: OffsetDateTime) -> String {
    let nanos = t.unix_timestamp_nanos().max(0);
    format!("toDateTime64({}, 9, 'UTC')", nanos as f64 / 1e9)
}

/// The wire form used for inserts, which is also what a row reads back as.
fn ts_str(t: OffsetDateTime) -> String {
    let secs = t.unix_timestamp();
    let nanos = t.nanosecond();
    let d = OffsetDateTime::from_unix_timestamp(secs).unwrap_or(OffsetDateTime::UNIX_EPOCH);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:09}",
        d.year(),
        u8::from(d.month()),
        d.day(),
        d.hour(),
        d.minute(),
        d.second(),
        nanos
    )
}

fn parse_ts(s: &str) -> OffsetDateTime {
    // "2026-08-01 10:00:00.000000000"
    let fmt = time::macros::format_description!(
        "[year]-[month]-[day] [hour]:[minute]:[second].[subsecond digits:9]"
    );
    time::PrimitiveDateTime::parse(s, &fmt)
        .map(|d| d.assume_utc())
        .unwrap_or(OffsetDateTime::UNIX_EPOCH)
}

fn json_text(v: &Option<Value>) -> String {
    v.as_ref().map(|v| serde_json::to_string(v).unwrap_or_default()).unwrap_or_default()
}

fn parse_json(s: &str) -> Option<Value> {
    if s.is_empty() {
        return None;
    }
    serde_json::from_str(s).ok()
}

fn opt(s: String) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

// ------------------------------------------------------------- row shapes ---

#[derive(Debug, Deserialize)]
struct SpanRow {
    trace_id: String,
    span_id: String,
    parent_span_id: String,
    external_parent_span_id: String,
    name: String,
    kind: String,
    status: String,
    start_time: String,
    end_time: Option<String>,
    duration_ns: Option<u64>,
    is_root: u8,
    is_event: u8,
    entity_kind: String,
    entity_id: String,
    entity_name: String,
    input: String,
    output: String,
    attributes: String,
    metadata: String,
    usage_json: String,
    cost_json: String,
    error_json: String,
    links_json: String,
    tags: Vec<String>,
    project: String,
    environment: String,
    service_name: String,
    service_version: String,
    session_id: String,
    thread_id: String,
    user_id: String,
    resource_id: String,
    run_id: String,
    request_id: String,
    experiment_id: String,
    release: String,
    model: String,
    provider: String,
    received_at: String,
}

impl From<SpanRow> for Span {
    fn from(r: SpanRow) -> Self {
        Self {
            trace_id: r.trace_id,
            span_id: r.span_id,
            parent_span_id: opt(r.parent_span_id),
            external_parent_span_id: opt(r.external_parent_span_id),
            name: r.name,
            kind: SpanKind::from_str(&r.kind).unwrap_or(SpanKind::Generic),
            status: SpanStatus::from_str(&r.status).unwrap_or(SpanStatus::Unset),
            start_time: parse_ts(&r.start_time),
            end_time: r.end_time.as_deref().map(parse_ts),
            duration_ns: r.duration_ns,
            is_root: r.is_root != 0,
            is_event: r.is_event != 0,
            entity_kind: parse_json(&format!("\"{}\"", r.entity_kind))
                .and_then(|v| serde_json::from_value(v).ok()),
            entity_id: opt(r.entity_id),
            entity_name: opt(r.entity_name),
            input: parse_json(&r.input),
            output: parse_json(&r.output),
            attributes: parse_json(&r.attributes),
            metadata: parse_json(&r.metadata),
            usage: parse_json(&r.usage_json)
                .and_then(|v| serde_json::from_value::<TokenUsage>(v).ok()),
            cost: parse_json(&r.cost_json).and_then(|v| serde_json::from_value::<Cost>(v).ok()),
            // No column holds these yet. The spec carries the shape; the
            // storage column, the migration and the write path are the
            // model-parameter track's work.
            model_parameters: None,
            error: parse_json(&r.error_json)
                .and_then(|v| serde_json::from_value::<SpanError>(v).ok()),
            links: parse_json(&r.links_json)
                .and_then(|v| serde_json::from_value::<Vec<SpanLink>>(v).ok()),
            tags: (!r.tags.is_empty()).then_some(r.tags),
            correlation: Correlation {
                project: r.project,
                environment: opt(r.environment),
                service_name: opt(r.service_name),
                service_version: opt(r.service_version),
                session_id: opt(r.session_id),
                thread_id: opt(r.thread_id),
                user_id: opt(r.user_id),
                resource_id: opt(r.resource_id),
                run_id: opt(r.run_id),
                request_id: opt(r.request_id),
                experiment_id: opt(r.experiment_id),
                release: opt(r.release),
            },
            model: opt(r.model),
            provider: opt(r.provider),
            received_at: parse_ts(&r.received_at),
            ingest_key: None,
        }
    }
}

/// Render a span as one `JSONEachRow` line.
fn span_row(s: &Span) -> Value {
    let c = &s.correlation;
    json!({
        "trace_id": s.trace_id,
        "span_id": s.span_id,
        "parent_span_id": s.parent_span_id.clone().unwrap_or_default(),
        "external_parent_span_id": s.external_parent_span_id.clone().unwrap_or_default(),
        "name": s.name,
        "kind": s.kind.as_str(),
        "status": s.status.as_str(),
        "start_time": ts_str(s.start_time),
        "end_time": s.end_time.map(ts_str),
        "duration_ns": s.duration_ns,
        "is_root": u8::from(s.is_root),
        "is_event": u8::from(s.is_event),
        "entity_kind": s.entity_kind.map(|k| k.as_str().to_owned()).unwrap_or_default(),
        "entity_id": s.entity_id.clone().unwrap_or_default(),
        "entity_name": s.entity_name.clone().unwrap_or_default(),
        "input": json_text(&s.input),
        "output": json_text(&s.output),
        "attributes": json_text(&s.attributes),
        "metadata": json_text(&s.metadata),
        "usage_json": s.usage.as_ref().map(|u| serde_json::to_string(u).unwrap_or_default()).unwrap_or_default(),
        "input_tokens": s.usage.as_ref().and_then(|u| u.input_tokens),
        "output_tokens": s.usage.as_ref().and_then(|u| u.output_tokens),
        "cost_json": s.cost.as_ref().map(|c| serde_json::to_string(c).unwrap_or_default()).unwrap_or_default(),
        "total_cost": s.cost.as_ref().and_then(|c| c.total_cost),
        "error_json": s.error.as_ref().map(|e| serde_json::to_string(e).unwrap_or_default()).unwrap_or_default(),
        "links_json": s.links.as_ref().map(|l| serde_json::to_string(l).unwrap_or_default()).unwrap_or_default(),
        "tags": s.tags.clone().unwrap_or_default(),
        "project": c.project,
        "environment": c.environment.clone().unwrap_or_default(),
        "service_name": c.service_name.clone().unwrap_or_default(),
        "service_version": c.service_version.clone().unwrap_or_default(),
        "session_id": c.session_id.clone().unwrap_or_default(),
        "thread_id": c.thread_id.clone().unwrap_or_default(),
        "user_id": c.user_id.clone().unwrap_or_default(),
        "resource_id": c.resource_id.clone().unwrap_or_default(),
        "run_id": c.run_id.clone().unwrap_or_default(),
        "request_id": c.request_id.clone().unwrap_or_default(),
        "experiment_id": c.experiment_id.clone().unwrap_or_default(),
        "release": c.release.clone().unwrap_or_default(),
        "model": s.model.clone().unwrap_or_default(),
        "provider": s.provider.clone().unwrap_or_default(),
        "received_at": ts_str(s.received_at),
    })
}

fn log_row(l: &LogRecord) -> Value {
    let c = &l.correlation;
    json!({
        "log_id": l.log_id,
        "trace_id": l.trace_id.clone().unwrap_or_default(),
        "span_id": l.span_id.clone().unwrap_or_default(),
        "timestamp": ts_str(l.timestamp),
        "level": l.level.as_str(),
        "severity": l.level.severity(),
        "message": l.message,
        "fields": json_text(&l.fields),
        "logger": l.logger.clone().unwrap_or_default(),
        "project": c.project,
        "environment": c.environment.clone().unwrap_or_default(),
        "service_name": c.service_name.clone().unwrap_or_default(),
        "session_id": c.session_id.clone().unwrap_or_default(),
        "user_id": c.user_id.clone().unwrap_or_default(),
        "received_at": ts_str(l.received_at),
    })
}

fn score_row(s: &Score) -> Value {
    json!({
        "score_id": s.score_id,
        "trace_id": s.trace_id.clone().unwrap_or_default(),
        "span_id": s.span_id.clone().unwrap_or_default(),
        "scorer_id": s.scorer_id,
        "scorer_version": s.scorer_version.clone().unwrap_or_default(),
        "source": s.source.as_str(),
        "value": s.value,
        "min_value": s.min,
        "max_value": s.max,
        "label": s.label.clone().unwrap_or_default(),
        "reason": s.reason.clone().unwrap_or_default(),
        "score_trace_id": s.score_trace_id.clone().unwrap_or_default(),
        "cost_json": s.cost.as_ref().map(|c| serde_json::to_string(c).unwrap_or_default()).unwrap_or_default(),
        "metadata": json_text(&s.metadata),
        "timestamp": ts_str(s.timestamp),
        "project": s.correlation.project,
        "environment": s.correlation.environment.clone().unwrap_or_default(),
        "experiment_id": s.correlation.experiment_id.clone().unwrap_or_default(),
    })
}

/// Match one search term against one column, the way a word search means it.
///
/// `q` is specified as full-text search: a search for words. A bare substring
/// test is not that, and the difference is visible to whoever ran the search.
/// `sda` matched `Tuesday` here and `Leed` matched `Leeds`, and neither the
/// embedded backend, which asks FTS5 for a phrase, nor Postgres, which asks
/// `plainto_tsquery`, returns either. Three stores, three answers to one
/// query string.
///
/// `hasTokenCaseInsensitive` is the word test. It does NOT read the
/// `tokenbf_v1` index on `(input, output)`: on 24.8 `EXPLAIN indexes = 1`
/// shows no skip-index step for it and reads every granule, where the
/// case-sensitive `hasToken` prunes 25 granules to 1. Only the case-sensitive
/// function is in that index's supported set, and `q` is documented as case
/// insensitive, so the scan is the price of the documented behaviour rather
/// than an oversight. `positionCaseInsensitive` before it read no index
/// either, so nothing was traded away.
///
/// Case is folded across the whole of Unicode, by lowering both sides with
/// `lowerUTF8` before the case-sensitive `hasToken`. `hasTokenCaseInsensitive`
/// folded ASCII only, so a search for `CAFÉ` did not find `Café` here while
/// FTS5 and `plainto_tsquery` both did. Neither form reads the token index, as
/// above, so the fold costs nothing the old one did not.
///
/// It rejects a needle that itself holds a separator, with an exception rather
/// than a false: `cache_miss` would have failed the whole request. A term that
/// is not one token cannot be asked of a token index at all, so it falls back
/// to the substring test. That keeps a search for `cache_miss` answering, and
/// FTS5 reads the same string as a two-word phrase which matches the same rows.
fn text_match(column: &str, term: &str) -> String {
    if term.chars().all(char::is_alphanumeric) {
        format!("hasToken(lowerUTF8({column}), lowerUTF8({}))", quote(term))
    } else {
        format!("positionCaseInsensitiveUTF8({column}, {}) > 0", quote(term))
    }
}

/// The same term against several columns, matching any of them.
fn text_match_any(columns: &[&str], term: &str) -> String {
    let parts: Vec<String> = columns.iter().map(|c| text_match(c, term)).collect();
    format!("({})", parts.join(" OR "))
}

/// Build the `WHERE` clause for a trace list.
///
/// Kept as a free function so the SQL it produces can be tested without a
/// server, and executed against a real one in the integration test.
fn trace_where(q: &TraceQuery, spans_table: &str) -> String {
    let mut w = vec![format!("project = {}", quote(&q.project))];
    if let Some(s) = q.status {
        w.push(format!("status = {}", quote(s.as_str())));
    }
    if let Some(k) = q.kind {
        w.push(format!("kind = {}", quote(k.as_str())));
    }
    if let Some(n) = &q.name {
        w.push(format!("name = {}", quote(n)));
    }
    if let Some(v) = &q.session_id {
        w.push(format!("session_id = {}", quote(v)));
    }
    if let Some(v) = &q.thread_id {
        w.push(format!("thread_id = {}", quote(v)));
    }
    if let Some(v) = &q.user_id {
        w.push(format!("user_id = {}", quote(v)));
    }
    if let Some(v) = &q.environment {
        w.push(format!("environment = {}", quote(v)));
    }
    if let Some(f) = q.from {
        w.push(format!("start_time >= {}", ts(f)));
    }
    if let Some(t) = q.to {
        w.push(format!("start_time < {}", ts(t)));
    }
    if let Some(d) = q.min_duration_ms {
        w.push(format!("duration_ns >= {}", d.saturating_mul(1_000_000)));
    }
    if let Some(c) = q.min_cost {
        w.push(format!("total_cost >= {c}"));
    }
    for tag in &q.tags {
        w.push(format!("has(tags, {})", quote(tag)));
    }
    if let Some(m) = &q.model {
        w.push(format!(
            "trace_id IN (SELECT trace_id FROM {spans_table} WHERE project = {} AND model = {})",
            quote(&q.project),
            quote(m)
        ));
    }
    if let Some(raw) = &q.q {
        // Every term must match, each against any of the payload columns, which
        // is what FTS5 is asked for in the embedded backend.
        let terms: Vec<String> = raw
            .split_whitespace()
            .filter(|t| !t.is_empty())
            .map(|t| text_match_any(&["input", "output", "name"], t))
            .collect();
        if !terms.is_empty() {
            w.push(format!(
                "trace_id IN (SELECT trace_id FROM {spans_table} WHERE project = {} AND {})",
                quote(&q.project),
                terms.join(" AND ")
            ));
        }
    }
    w.join(" AND ")
}

fn order_by(sort: TraceSort, order: Order) -> String {
    // `min_score` is Nullable, and ClickHouse puts a NULL last whichever way it
    // is sorting. ifNull decides where an unscored run ranks here instead, so
    // this backend answers the same question as the other two.
    let col = match sort {
        TraceSort::StartTime => "start_time",
        TraceSort::Duration => "duration_ns",
        TraceSort::Cost => "total_cost",
        TraceSort::SpanCount => "span_count",
        TraceSort::Score => "ifNull(min_score, 0)",
    };
    let dir = match order {
        Order::Asc => "ASC",
        Order::Desc => "DESC",
    };
    format!("{col} {dir}, trace_id {dir}")
}

fn span_order_by(sort: SpanSort, order: Order) -> String {
    let col = match sort {
        SpanSort::StartTime => "start_time",
        SpanSort::Duration => "ifNull(duration_ns, 0)",
        SpanSort::Cost => "ifNull(total_cost, 0)",
    };
    let dir = match order {
        Order::Asc => "ASC",
        Order::Desc => "DESC",
    };
    format!("{col} {dir}, span_id {dir}")
}

#[async_trait::async_trait]
impl Store for ClickHouseStore {
    fn name(&self) -> &'static str {
        "clickhouse"
    }

    fn capabilities(&self) -> StoreCapabilities {
        // Read rather than assumed: `migrate` asks the server what the tables
        // are actually cut on, because ClickHouse cannot change a partition key
        // in place and a database created under an older schema keeps its own.
        let day = self.day_partitioned.load(std::sync::atomic::Ordering::Relaxed);
        StoreCapabilities {
            // Whole tokens, with case folded across Unicode by `lowerUTF8`, so
            // a search for `CAFÉ` finds `Café` here as it does on the other two.
            text_search: TextSearch::Words,
            unicode_case_folding: true,
            // A ClickHouse `DELETE` is a mutation: it is accepted, and applied
            // afterwards. The counts say what matched, not what has left disk.
            immediate_deletes: false,
            discovery: true,
            discovery_label_values: true,
            delta_polling: true,
            per_signal_retention: true,
            // The tables can carry their own TTL, so a deployment-wide window is
            // declared once and enforced by the engine rather than by a timer.
            declarative_retention: true,
            partitioning: if day { Partitioning::Day } else { Partitioning::Month },
            partition_drop_retention: day,
            idempotent_schema_init: true,
            theme_analysis: true,
            ..StoreCapabilities::minimal("clickhouse")
        }
    }

    async fn migrate(&self) -> Result<()> {
        // Fast path, and it issues no schema statement at all: both queries
        // below are reads. Every replica in a cluster calls this on start, and
        // each `CREATE TABLE IF NOT EXISTS` is metadata the whole cluster has
        // to agree on even when it creates nothing. Replaying fourteen of them
        // on every boot is churn that buys nothing, and on a Replicated engine
        // it is churn every other replica waits on.
        //
        // The fingerprint covers the statements this build would actually run,
        // retention rewriting included, so a changed schema file and a changed
        // `--retention-days` both miss and an unchanged pair both hit.
        let fingerprint = self.schema_fingerprint();
        if self.applied_fingerprint().await? == Some(fingerprint.clone()) {
            self.read_partitioning().await?;
            return Ok(());
        }

        // The HTTP interface takes one statement per request.
        for sql in self.resolved_schema() {
            self.execute(&sql)
                .await
                .map_err(|e| Error::Migration { version: 1, reason: format!("{e}") })?;
        }
        self.resort_traces().await?;
        self.read_partitioning().await?;

        // Recorded last. A crash part way through leaves no fingerprint, so the
        // next start replays the whole file, which is safe because every
        // statement in it is `IF NOT EXISTS`. Recording it first would let a
        // half-applied schema be skipped for ever.
        self.post(
            &format!("INSERT INTO {} FORMAT JSONEachRow", self.table("schema_version")),
            Some(
                json!({
                    "version": SCHEMA_VERSION,
                    "fingerprint": fingerprint,
                    "applied_at": ts_str(OffsetDateTime::now_utc()),
                })
                .to_string(),
            ),
        )
        .await
        .map_err(|e| Error::Migration { version: SCHEMA_VERSION, reason: format!("{e}") })?;
        Ok(())
    }

    async fn ping(&self) -> Result<()> {
        self.execute("SELECT 1").await
    }

    async fn ingest(&self, batch: Batch) -> Result<IngestResult> {
        let mut result = IngestResult { accepted: 0, rejected: 0, duplicate: 0, errors: None };

        if let Some(key) = &batch.ingest_key {
            let seen: Vec<Value> = self
                .select(&format!(
                    "SELECT 1 AS n FROM {} WHERE key = {} LIMIT 1",
                    self.table("ingest_keys"),
                    quote(key)
                ))
                .await?;
            if !seen.is_empty() {
                result.duplicate = batch.len() as u32;
                return Ok(result);
            }
        }

        // One request per table, each carrying the whole batch for that signal.
        // ClickHouse is happiest with few large inserts and unhappy with many
        // small ones, so batching here is not an optimisation, it is the
        // difference between working and falling over.
        let send = |table: &str, rows: Vec<Value>| {
            let sql = format!("INSERT INTO {} FORMAT JSONEachRow", self.table(table));
            let body = rows
                .iter()
                .map(|r| serde_json::to_string(r).unwrap_or_default())
                .collect::<Vec<_>>()
                .join("\n");
            (sql, body, rows.len() as u32)
        };

        if !batch.spans.is_empty() {
            let mut spans = batch.spans.clone();
            for s in &mut spans {
                s.normalise();
            }
            let (sql, body, n) = send("spans", spans.iter().map(span_row).collect());
            self.post(&sql, Some(body)).await?;
            result.accepted += n;
        }
        if !batch.logs.is_empty() {
            let (sql, body, n) = send("logs", batch.logs.iter().map(log_row).collect());
            self.post(&sql, Some(body)).await?;
            result.accepted += n;
        }
        if !batch.metrics.is_empty() {
            let rows: Vec<Value> = batch
                .metrics
                .iter()
                .map(|m| {
                    json!({
                        "metric_id": m.metric_id, "name": m.name, "kind": m.kind.as_str(),
                        "value": m.value, "unit": m.unit.clone().unwrap_or_default(),
                        "timestamp": ts_str(m.timestamp),
                        "labels": json_text(&m.labels), "buckets": json_text(&m.buckets),
                        "trace_id": m.trace_id.clone().unwrap_or_default(),
                        "span_id": m.span_id.clone().unwrap_or_default(),
                        "project": m.correlation.project,
                        "environment": m.correlation.environment.clone().unwrap_or_default(),
                        "received_at": ts_str(m.received_at),
                    })
                })
                .collect();
            let (sql, body, n) = send("metrics", rows);
            self.post(&sql, Some(body)).await?;
            result.accepted += n;
        }
        if !batch.scores.is_empty() {
            let (sql, body, n) = send("scores", batch.scores.iter().map(score_row).collect());
            self.post(&sql, Some(body)).await?;
            result.accepted += n;
        }
        if !batch.feedback.is_empty() {
            let rows: Vec<Value> = batch
                .feedback
                .iter()
                .map(|f| {
                    json!({
                        "feedback_id": f.feedback_id,
                        "trace_id": f.trace_id.clone().unwrap_or_default(),
                        "span_id": f.span_id.clone().unwrap_or_default(),
                        "kind": f.kind.as_str(),
                        "value": serde_json::to_string(&f.value).unwrap_or_default(),
                        "comment": f.comment.clone().unwrap_or_default(),
                        "author_id": f.author_id.clone().unwrap_or_default(),
                        "author_kind": f.author_kind.clone().unwrap_or_default(),
                        "timestamp": ts_str(f.timestamp),
                        "metadata": json_text(&f.metadata),
                        "project": f.correlation.project,
                        "environment": f.correlation.environment.clone().unwrap_or_default(),
                    })
                })
                .collect();
            let (sql, body, n) = send("feedback", rows);
            self.post(&sql, Some(body)).await?;
            result.accepted += n;
        }

        // Notes last of the six signals. A note is filed under the entity its
        // trace belongs to, and that is resolved with one read of the spans
        // this batch has just written rather than with a join, because nothing
        // in this backend joins.
        if !batch.aspect_notes.is_empty() {
            let ids: Vec<String> =
                batch.aspect_notes.iter().map(|n| quote(&n.trace_id)).collect::<Vec<_>>();
            // Keyed by project and trace id. A trace id two projects share is
            // a run in each, and a note filed by the id alone took its entity,
            // and so its agent's name, from whichever project's run came back.
            let mut entity: std::collections::HashMap<(String, String), (String, String, String)> =
                std::collections::HashMap::new();
            let roots: Vec<RootRow> = self
                .select(&format!(
                    "SELECT t.project AS project, t.trace_id AS trace_id,
                            s.entity_kind AS entity_kind, s.entity_name AS entity_name,
                            toString(t.start_time) AS start_time
                       FROM {traces} AS t FINAL
                       INNER JOIN {spans} AS s FINAL
                          ON s.project = t.project AND s.trace_id = t.trace_id
                         AND s.span_id = t.root_span_id
                      WHERE t.trace_id IN ({list})",
                    traces = self.table("traces"),
                    spans = self.table("spans"),
                    list = ids.join(","),
                ))
                .await?;
            for r in roots {
                entity
                    .insert((r.project, r.trace_id), (r.entity_kind, r.entity_name, r.start_time));
            }

            let rows: Vec<Value> = batch
                .aspect_notes
                .iter()
                .map(|n| {
                    // Writing a note lags ingestion, so a note whose trace is
                    // not stored yet is ordinary. It keeps an empty entity,
                    // stays readable by trace id, and is absent from the entity
                    // roll-ups.
                    let key = (n.correlation.project.clone(), n.trace_id.clone());
                    let (kind, name, started) = entity
                        .get(&key)
                        .cloned()
                        .unwrap_or_else(|| (String::new(), String::new(), ts_str(n.created_at)));
                    let c = &n.correlation;
                    json!({
                        "note_id": n.note_id, "trace_id": n.trace_id, "aspect": n.aspect,
                        "text": n.text, "definition_version": n.definition_version,
                        "model": n.model.clone().unwrap_or_default(),
                        "created_at": ts_str(n.created_at),
                        "entity_kind": kind, "entity_name": name,
                        "trace_started_at": started,
                        // Sketched here rather than handed in, so it is a
                        // sketch of the text that was actually stored: the
                        // redactor runs before this.
                        "sketch": hex::encode(mcpd_core::sketch::Sketch::of(&n.text).to_bytes()),
                        "project": c.project,
                        "environment": c.environment.clone().unwrap_or_default(),
                        "service_name": c.service_name.clone().unwrap_or_default(),
                        "service_version": c.service_version.clone().unwrap_or_default(),
                        "session_id": c.session_id.clone().unwrap_or_default(),
                        "thread_id": c.thread_id.clone().unwrap_or_default(),
                        "user_id": c.user_id.clone().unwrap_or_default(),
                        "resource_id": c.resource_id.clone().unwrap_or_default(),
                        "run_id": c.run_id.clone().unwrap_or_default(),
                        "request_id": c.request_id.clone().unwrap_or_default(),
                        "experiment_id": c.experiment_id.clone().unwrap_or_default(),
                        "release": c.release.clone().unwrap_or_default(),
                    })
                })
                .collect();
            let (sql, body, n) = send("aspect_notes", rows);
            self.post(&sql, Some(body)).await?;
            result.accepted += n;
        }

        // Rebuild the trace projection for every run this batch touched.
        let mut touched: Vec<String> = batch.spans.iter().map(|s| s.trace_id.clone()).collect();
        touched.extend(batch.scores.iter().filter_map(|s| s.trace_id.clone()));
        touched.extend(batch.feedback.iter().filter_map(|f| f.trace_id.clone()));
        touched.sort();
        touched.dedup();
        if !touched.is_empty() {
            self.rebuild_traces(&touched).await?;
        }

        // The filter box. One insert for the whole batch, and the table is a
        // ReplacingMergeTree keyed on the value with `last_seen` as its
        // version, so re-inserting a value that is already there is a row a
        // merge folds rather than a read-modify-write on the ingest path.
        let facets = crate::facet_values(&batch);
        if !facets.is_empty() {
            let seen = ts_str(OffsetDateTime::now_utc());
            let rows: Vec<Value> = facets
                .iter()
                .map(|f| {
                    json!({
                        "project": f.project, "facet": f.facet.as_str(),
                        "key": f.key, "value": f.value, "last_seen": seen,
                    })
                })
                .collect();
            let (sql, body, _) = send("discovery", rows);
            self.post(&sql, Some(body)).await?;
        }

        if let Some(key) = &batch.ingest_key {
            self.post(
                &format!("INSERT INTO {} FORMAT JSONEachRow", self.table("ingest_keys")),
                Some(
                    json!({ "key": key, "seen_at": ts_str(OffsetDateTime::now_utc()) }).to_string(),
                ),
            )
            .await?;
        }

        Ok(result)
    }

    async fn list_traces(&self, q: &TraceQuery) -> Result<Paged<Trace>> {
        let limit = q.limit.clamp(1, 500);
        let mut where_sql = trace_where(q, &self.table("spans"));

        if let Some(scorer) = &q.has_score {
            let mut cond = format!(
                "trace_id IN (SELECT trace_id FROM {} WHERE project = {} AND scorer_id = {}",
                self.table("scores"),
                quote(&q.project),
                quote(scorer)
            );
            if let Some(below) = q.score_below {
                cond.push_str(&format!(" AND value < {below}"));
            }
            cond.push(')');
            where_sql.push_str(&format!(" AND {cond}"));
        }

        let rows: Vec<TraceRow> = self
            .select(&format!(
                "SELECT * FROM {} FINAL WHERE {where_sql} ORDER BY {} LIMIT {limit}",
                self.table("traces"),
                order_by(q.sort, q.order),
            ))
            .await?;

        Ok(Paged {
            items: rows.into_iter().map(Into::into).collect(),
            next_cursor: None,
            total: None,
        })
    }

    async fn get_trace(
        &self,
        project: &str,
        trace_id: &str,
        include: &Include,
    ) -> Result<TraceDetail> {
        let traces: Vec<TraceRow> = self
            .select(&format!(
                "SELECT * FROM {} FINAL WHERE project = {} AND trace_id = {} LIMIT 1",
                self.table("traces"),
                quote(project),
                quote(trace_id)
            ))
            .await?;
        let trace: Trace = traces
            .into_iter()
            .next()
            .ok_or_else(|| Error::NotFound { kind: "trace", id: trace_id.to_owned() })?
            .into();

        let spans: Vec<SpanRow> = self
            .select(&format!(
                "SELECT * FROM {} FINAL WHERE project = {} AND trace_id = {} ORDER BY start_time ASC, span_id ASC",
                self.table("spans"),
                quote(project),
                quote(trace_id)
            ))
            .await?;

        let logs = if include.logs {
            let rows: Vec<LogRow> = self
                .select(&format!(
                    "SELECT * FROM {} WHERE project = {} AND trace_id = {} \
                     ORDER BY timestamp ASC LIMIT 5000",
                    self.table("logs"),
                    quote(project),
                    quote(trace_id)
                ))
                .await?;
            Some(rows.into_iter().map(Into::into).collect())
        } else {
            None
        };

        let scores = if include.scores {
            let rows: Vec<ScoreRow> = self
                .select(&format!(
                    "SELECT * FROM {} FINAL WHERE project = {} AND trace_id = {} \
                     ORDER BY timestamp ASC",
                    self.table("scores"),
                    quote(project),
                    quote(trace_id)
                ))
                .await?;
            Some(rows.into_iter().map(Into::into).collect())
        } else {
            None
        };

        let feedback = if include.feedback {
            let rows: Vec<FeedbackRow> = self
                .select(&format!(
                    "SELECT * FROM {} FINAL WHERE project = {} AND trace_id = {} \
                     ORDER BY timestamp ASC",
                    self.table("feedback"),
                    quote(project),
                    quote(trace_id)
                ))
                .await?;
            Some(rows.into_iter().map(Into::into).collect())
        } else {
            None
        };

        Ok(TraceDetail {
            trace,
            spans: spans.into_iter().map(Into::into).collect(),
            logs,
            scores,
            feedback,
        })
    }

    async fn delete_trace(&self, project: &str, trace_id: &str) -> Result<u32> {
        // ClickHouse deletes are asynchronous mutations. `DELETE FROM` is the
        // lightweight form and is the right tool for an erasure request, which
        // is rare and must be durable rather than fast.
        // The count is taken before the delete, not after. A ClickHouse
        // mutation is asynchronous and reports no row count over HTTP, so
        // counting afterwards would race the mutation and returning a fixed
        // number would be a lie about what an erasure request removed.
        let tables = ["spans", "traces", "logs", "metrics", "scores", "feedback", "aspect_notes"];
        let mut removed = 0_u32;
        for table in tables {
            let where_clause =
                format!("WHERE project = {} AND trace_id = {}", quote(project), quote(trace_id));
            // `toUInt32` matters: ClickHouse quotes 64-bit integers in JSON by
            // default, so a bare `count()` deserialises as a string, not a
            // number.
            let counted: Vec<CountRow> = self
                .select(&format!(
                    "SELECT toUInt32(count()) AS n FROM {} {where_clause}",
                    self.table(table)
                ))
                .await?;
            removed += counted.first().map_or(0, |c| c.n);
            self.execute(&format!("DELETE FROM {} {where_clause}", self.table(table))).await?;
        }
        // Membership rows are not counted. They are ids pointing at a note that
        // has just gone, so leaving one behind would make a theme claim a run
        // that is no longer stored; but they are bookkeeping and not a signal
        // somebody sent, and counting them would inflate what this endpoint
        // reports as deleted.
        self.execute(&format!(
            "DELETE FROM {} WHERE project = {} AND trace_id = {}",
            self.table("theme_members"),
            quote(project),
            quote(trace_id)
        ))
        .await?;
        Ok(removed)
    }

    async fn erase_subject(&self, project: &str, user_id: &str) -> Result<ErasureOutcome> {
        // No transaction here, and there is no honest way to fake one.
        //
        // ClickHouse transactions are experimental and do not cover the
        // `DELETE FROM` used below, so the statements that follow commit one at
        // a time. If the process dies halfway, some signals are gone and some
        // are not. What makes that acceptable is that the operation is
        // idempotent: the predicate is the subject, not a list of rows already
        // seen, so running the same erasure again finishes the job and reports
        // the smaller counts of the second pass. An operator who does not see a
        // response should repeat the request.
        //
        // The deletes are also asynchronous mutations. They are accepted here
        // and applied in the background, so this response says what matched,
        // not that every row has already left the disk. That is what
        // `deferred` on the outcome is for: the guarantee this backend gives is
        // weaker than the one SQLite and Postgres give, and the caller is told
        // rather than left to assume they are the same.
        #[derive(Deserialize)]
        struct TraceIdRow {
            trace_id: String,
        }
        #[derive(Deserialize)]
        struct SpanIdRow {
            span_id: String,
        }
        #[derive(Deserialize)]
        struct ItemRow {
            item_id: String,
            dataset_id: String,
        }

        let (p, u) = (quote(project), quote(user_id));

        // The runs in scope, resolved before anything is deleted. Only the root
        // span usually carries `user_id`, so a run is in scope when the trace
        // projection names the subject or when any of its spans does. Taking
        // the set first also stops the deletes below shrinking their own search
        // set as they run.
        //
        // `FINAL` on both sides: an un-merged duplicate would otherwise repeat
        // a trace id in the list, which costs nothing here, but the same
        // reasoning is what the counts depend on and the two must not diverge.
        let targets: Vec<TraceIdRow> = self
            .select(&format!(
                "SELECT DISTINCT trace_id FROM (
                   SELECT trace_id FROM {traces} FINAL WHERE project = {p} AND user_id = {u}
                   UNION ALL
                   SELECT trace_id FROM {spans} FINAL WHERE project = {p} AND user_id = {u}
                 )",
                traces = self.table("traces"),
                spans = self.table("spans"),
            ))
            .await?;
        let in_list = targets.iter().map(|t| quote(&t.trace_id)).collect::<Vec<_>>().join(", ");

        // The subject's span ids, taken while the spans still exist.
        //
        // A score, a piece of feedback, a metric point or a log line can name a
        // `span_id` and carry no `trace_id` at all. Those rows hang off the
        // subject's step and hold the subject's content, and matching on the
        // trace alone left every one of them on disk. What bounds the reach of
        // this set is decided where it is used, a few lines below: `project`
        // alone is not enough, because a span id is unique inside its trace and
        // another run in the same project can reuse it.
        let mut span_list = String::new();
        if !in_list.is_empty() {
            let spans: Vec<SpanIdRow> = self
                .select(&format!(
                    "SELECT DISTINCT span_id FROM {spans} FINAL
                     WHERE project = {p} AND trace_id IN ({in_list})",
                    spans = self.table("spans"),
                ))
                .await?;
            span_list = spans.iter().map(|r| quote(&r.span_id)).collect::<Vec<_>>().join(", ");
        }

        // An empty `IN ()` is not valid SQL, so each arm is only added when it
        // has something to match. With neither, the run-scoped tables are
        // skipped rather than queried with an empty list.
        //
        // A span id is unique inside its trace, not inside the project, so
        // `span_id IN (...)` on its own reached any other run in the same
        // project that happened to reuse the id and took that run's scores and
        // logs with it. The span arm is therefore limited to rows that name no
        // trace of their own. `trace_id` is a non-nullable `String` here, so a
        // row with no trace holds `''`. A row that names a trace is judged by
        // that trace; a row that names none is the one the span arm exists for,
        // and it goes even though two runs sharing the span id make it
        // ambiguous: it may hold the subject's content and carries nothing else
        // to decide on.
        let mut attached: Vec<String> = Vec::new();
        if !in_list.is_empty() {
            attached.push(format!("trace_id IN ({in_list})"));
        }
        if !span_list.is_empty() {
            attached.push(format!("(trace_id = '' AND span_id IN ({span_list}))"));
        }
        let attached = attached.join(" OR ");

        // Every predicate is anchored on `project`. That is the tenancy guard:
        // the same subject in a project this caller was not authorised for
        // keeps its rows and contributes nothing to the counts.
        let (mut spans, mut metrics, mut scores, mut traces) = (0, 0, 0, 0);
        if !in_list.is_empty() {
            let by_trace = format!("project = {p} AND trace_id IN ({in_list})");
            spans = self.count_then_delete("spans", &by_trace).await?;
            traces = self.count_then_delete("traces", &by_trace).await?;
        }
        if !attached.is_empty() {
            // Metrics and scores carry no identifier of their own. They are
            // reachable through the run or the step they hang off, which is why
            // both reaches are needed and neither is enough alone.
            let by_attachment = format!("project = {p} AND ({attached})");
            metrics = self.count_then_delete("metrics", &by_attachment).await?;
            scores = self.count_then_delete("scores", &by_attachment).await?;
        }

        // Feedback has an author, hashed by the same rule as `user_id`, and the
        // row holds that person's own words. Feedback the subject wrote about
        // somebody else's run is still the subject's prose, so the author is
        // matched wherever the row hangs.
        let feedback_pred = if attached.is_empty() {
            format!("project = {p} AND author_id = {u}")
        } else {
            format!("project = {p} AND (({attached}) OR author_id = {u})")
        };
        let feedback = self.count_then_delete("feedback", &feedback_pred).await?;

        // One predicate for both kinds of log line, so a log attached to an
        // erased run and also stamped with the subject is counted once. A log
        // can carry the subject with no trace to hang it on, which is why the
        // second arm exists at all.
        let log_pred = if attached.is_empty() {
            format!("project = {p} AND user_id = {u}")
        } else {
            format!("project = {p} AND (({attached}) OR user_id = {u})")
        };
        let logs = self.count_then_delete("logs", &log_pred).await?;

        // An aspect note is one sentence about what a user asked for, so it is
        // the most direct copy of a person's own words in this schema after the
        // span payloads themselves. It is matched by the run it describes and
        // by the subject stamped on it, exactly as a log line is, so a note
        // about somebody else's run that carries this subject goes too.
        //
        // Matched on the trace and the subject, and not through `attached`: a
        // note names a trace and never a step, so the span half of that
        // predicate would be a reference to a column this table does not have.
        let note_pred = if in_list.is_empty() {
            format!("project = {p} AND user_id = {u}")
        } else {
            format!("project = {p} AND (trace_id IN ({in_list}) OR user_id = {u})")
        };
        let aspect_notes = self.count_then_delete("aspect_notes", &note_pred).await?;

        if !in_list.is_empty() {
            // Membership rows hold no prose, but they hold the note id and the
            // trace id of a sentence that has just been erased. Left behind, a
            // theme would still claim a run nothing can open.
            self.execute(&format!(
                "DELETE FROM {} WHERE project = {p} AND trace_id IN ({in_list})",
                self.table("theme_members")
            ))
            .await?;

            // A theme's label is not a summary written for the screen: it is
            // the text of one of the theme's own notes, so a theme labelled
            // from an erased run holds that person's words in a column an
            // erasure would otherwise miss. The label is emptied rather than
            // the theme deleted, for the same reason a captured dataset case
            // is emptied.
            //
            // Emptied by writing the row again under the same key, which is
            // what a ReplacingMergeTree is for. An `ALTER TABLE ... UPDATE`
            // would rewrite whole parts in the background and the old text
            // would stay readable until it finished.
            let labelled: Vec<ThemeRow> = self
                .select(&format!(
                    "SELECT {THEME_COLUMNS} FROM {} FINAL
                      WHERE project = {p} AND label_trace_id IN ({in_list})",
                    self.table("themes")
                ))
                .await?;
            if !labelled.is_empty() {
                let body = labelled
                    .iter()
                    .map(|t| {
                        let mut row = theme_row_json(t);
                        row["label"] = json!(ERASED_ITEM);
                        row["label_note_id"] = json!("");
                        row["label_trace_id"] = json!("");
                        row.to_string()
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                self.post(
                    &format!("INSERT INTO {} FORMAT JSONEachRow", self.table("themes")),
                    Some(body),
                )
                .await?;
            }
        }

        // A captured case keeps a verbatim copy of a production input, so
        // erasing the run and leaving the copy would leave the subject's words
        // in the dataset. The case is emptied rather than deleted: somebody put
        // it there to keep a bug from coming back, and destroying it silently
        // is its own harm.
        //
        // Emptied by writing the row again under the same key, which is what a
        // ReplacingMergeTree is for. An `ALTER TABLE ... UPDATE` would be a
        // mutation that rewrites whole parts in the background, and the old
        // text would stay readable until it finished.
        let (mut dataset_items, mut datasets) = (0_u32, Vec::new());
        if !in_list.is_empty() {
            let items: Vec<ItemRow> = self
                .select(&format!(
                    "SELECT item_id, dataset_id FROM {items} FINAL
                     WHERE source_trace_id IN ({in_list})
                     AND dataset_id IN (
                       SELECT dataset_id FROM {sets} FINAL WHERE project = {p})",
                    items = self.table("dataset_items"),
                    sets = self.table("datasets"),
                ))
                .await?;
            if !items.is_empty() {
                let body = items
                    .iter()
                    .map(|i| {
                        json!({
                            "item_id": i.item_id, "dataset_id": i.dataset_id,
                            "input": ERASED_ITEM, "expected": "",
                            "metadata": ERASED_ITEM, "source_trace_id": "",
                        })
                        .to_string()
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                self.post(
                    &format!("INSERT INTO {} FORMAT JSONEachRow", self.table("dataset_items")),
                    Some(body),
                )
                .await?;
                dataset_items = u32::try_from(items.len()).unwrap_or(u32::MAX);
                datasets = items.iter().map(|i| i.dataset_id.clone()).collect();
                datasets.sort();
                datasets.dedup();
            }
        }

        let identifiers = self.probe_identifiers(project, user_id).await?;

        Ok(ErasureOutcome {
            result: ErasureResult {
                project: project.to_owned(),
                traces,
                spans,
                logs,
                metrics,
                scores,
                feedback,
                // The six fields this type has, and their sum. Aspect notes are
                // deliberately outside it: adding them would make `deleted`
                // disagree with the six numbers printed beside it.
                deleted: traces + spans + logs + metrics + scores + feedback,
                // The run store's, filled in by the server when it has one.
                runs_erased: None,
                runs_in_flight: None,
                // The rest of the reply, filled in by the server from the outcome.
                aspect_notes_erased: None,
                dataset_items_erased: None,
                datasets: Vec::new(),
                deletes_pending: None,
                unmatchable_identifiers: None,
                note: None,
            },
            dataset_items,
            aspect_notes,
            datasets,
            identifiers,
            // ClickHouse accepts the deletes and applies them afterwards.
            deferred: true,
        })
    }

    async fn list_spans(&self, q: &SpanQuery) -> Result<Paged<Span>> {
        let mut w = vec![format!("project = {}", quote(&q.project))];
        if let Some(k) = q.kind {
            w.push(format!("kind = {}", quote(k.as_str())));
        }
        if let Some(s) = q.status {
            w.push(format!("status = {}", quote(s.as_str())));
        }
        if let Some(n) = &q.name {
            w.push(format!("name = {}", quote(n)));
        }
        if let Some(m) = &q.model {
            w.push(format!("model = {}", quote(m)));
        }
        if let Some(t) = &q.trace_id {
            w.push(format!("trace_id = {}", quote(t)));
        }
        if let Some(f) = q.from {
            w.push(format!("start_time >= {}", ts(f)));
        }
        if let Some(t) = q.to {
            w.push(format!("start_time < {}", ts(t)));
        }
        if let Some(d) = q.min_duration_ms {
            w.push(format!("duration_ns >= {}", d.saturating_mul(1_000_000)));
        }
        if let Some(raw) = &q.q {
            for term in raw.split_whitespace() {
                w.push(text_match_any(&["input", "output", "name"], term));
            }
        }
        let rows: Vec<SpanRow> = self
            .select(&format!(
                "SELECT * FROM {} FINAL WHERE {} ORDER BY {} LIMIT {}",
                self.table("spans"),
                w.join(" AND "),
                span_order_by(q.sort, q.order),
                q.limit.clamp(1, 500)
            ))
            .await?;
        Ok(Paged {
            items: rows.into_iter().map(Into::into).collect(),
            next_cursor: None,
            total: None,
        })
    }

    async fn list_logs(&self, q: &LogQuery) -> Result<Paged<LogRecord>> {
        let mut w = vec![format!("project = {}", quote(&q.project))];
        if let Some(l) = q.level {
            w.push(format!("severity >= {}", l.severity()));
        }
        if let Some(t) = &q.trace_id {
            w.push(format!("trace_id = {}", quote(t)));
        }
        if let Some(s) = &q.span_id {
            w.push(format!("span_id = {}", quote(s)));
        }
        if let Some(f) = q.from {
            w.push(format!("timestamp >= {}", ts(f)));
        }
        if let Some(t) = q.to {
            w.push(format!("timestamp < {}", ts(t)));
        }
        if let Some(raw) = &q.q {
            for term in raw.split_whitespace() {
                w.push(text_match_any(&["message", "fields"], term));
            }
        }
        let dir = if q.order == Order::Asc { "ASC" } else { "DESC" };
        let rows: Vec<LogRow> = self
            .select(&format!(
                "SELECT * FROM {} WHERE {} ORDER BY timestamp {dir}, log_id {dir} LIMIT {}",
                self.table("logs"),
                w.join(" AND "),
                q.limit.clamp(1, 1000)
            ))
            .await?;
        Ok(Paged {
            items: rows.into_iter().map(Into::into).collect(),
            next_cursor: None,
            total: None,
        })
    }

    async fn query_metrics(&self, q: &MetricQuery) -> Result<MetricSeriesResponse> {
        let bucket = if q.bucket_seconds > 0 { q.bucket_seconds } else { 60 };

        // ClickHouse has exact quantile aggregates, so unlike the embedded
        // backend there is no window function to hand-roll here.
        let agg = match q.agg {
            Aggregation::Sum => "sum(value)".to_owned(),
            Aggregation::Count => "toFloat64(count())".to_owned(),
            Aggregation::Mean => "avg(value)".to_owned(),
            Aggregation::Min => "min(value)".to_owned(),
            Aggregation::Max => "max(value)".to_owned(),
            Aggregation::P50 => "quantileExact(0.5)(value)".to_owned(),
            Aggregation::P90 => "quantileExact(0.9)(value)".to_owned(),
            Aggregation::P95 => "quantileExact(0.95)(value)".to_owned(),
            Aggregation::P99 => "quantileExact(0.99)(value)".to_owned(),
        };

        let key = if q.group_by.is_empty() {
            "''".to_owned()
        } else {
            q.group_by
                .iter()
                .map(|k| format!("coalesce(JSONExtractString(labels, {}), '(none)')", quote(k)))
                .collect::<Vec<_>>()
                .join(" || char(31) || ")
        };

        let mut w = vec![
            format!("project = {}", quote(&q.project)),
            format!("name = {}", quote(&q.name)),
            format!("timestamp >= {}", ts(q.from)),
            format!("timestamp < {}", ts(q.to)),
        ];
        for (k, v) in &q.filter {
            w.push(format!("JSONExtractString(labels, {}) = {}", quote(k), quote(v)));
        }

        #[derive(Deserialize)]
        struct Row {
            bucket: String,
            series_key: String,
            v: f64,
            n: u64,
            exemplar: String,
        }

        let rows: Vec<Row> = self
            .select(&format!(
                "SELECT toString(toStartOfInterval(timestamp, INTERVAL {bucket} SECOND)) AS bucket,
                        {key} AS series_key, {agg} AS v, count() AS n, max(trace_id) AS exemplar
                 FROM {} WHERE {} GROUP BY bucket, series_key ORDER BY bucket ASC LIMIT 20000",
                self.table("metrics"),
                w.join(" AND ")
            ))
            .await?;

        let unit: Vec<Value> = self
            .select(&format!(
                "SELECT any(unit) AS unit FROM {} WHERE project = {} AND name = {} AND unit != ''",
                self.table("metrics"),
                quote(&q.project),
                quote(&q.name)
            ))
            .await?;

        let mut series: Vec<MetricSeries> = Vec::new();
        let mut index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for r in rows {
            let idx = *index.entry(r.series_key.clone()).or_insert_with(|| {
                let labels = if q.group_by.is_empty() {
                    None
                } else {
                    let parts: Vec<&str> = r.series_key.split('\u{1f}').collect();
                    Some(Value::Object(
                        q.group_by
                            .iter()
                            .cloned()
                            .zip(parts.iter().map(|p| Value::String((*p).to_owned())))
                            .collect(),
                    ))
                };
                series.push(MetricSeries { labels, points: Vec::new() });
                series.len() - 1
            });
            series[idx].points.push(MetricSeriesPoint {
                // ClickHouse returns a second-resolution string here.
                timestamp: parse_ts(&format!("{}.000000000", r.bucket)),
                value: r.v,
                count: Some(r.n),
                exemplar_trace_id: (!r.exemplar.is_empty()).then_some(r.exemplar),
            });
        }

        Ok(MetricSeriesResponse {
            name: q.name.clone(),
            unit: unit
                .first()
                .and_then(|v| v["unit"].as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_owned),
            bucket: format!("{bucket}s"),
            series,
        })
    }

    async fn list_scores(&self, q: &ScoreQuery) -> Result<Paged<Score>> {
        let mut w = vec![format!("project = {}", quote(&q.project))];
        if let Some(s) = &q.scorer_id {
            w.push(format!("scorer_id = {}", quote(s)));
        }
        if let Some(s) = q.source {
            w.push(format!("source = {}", quote(s.as_str())));
        }
        if let Some(t) = &q.trace_id {
            w.push(format!("trace_id = {}", quote(t)));
        }
        if let Some(e) = &q.experiment_id {
            w.push(format!("experiment_id = {}", quote(e)));
        }
        if let Some(v) = q.min_value {
            w.push(format!("value >= {v}"));
        }
        if let Some(v) = q.max_value {
            w.push(format!("value <= {v}"));
        }
        if let Some(f) = q.from {
            w.push(format!("timestamp >= {}", ts(f)));
        }
        if let Some(t) = q.to {
            w.push(format!("timestamp < {}", ts(t)));
        }
        let rows: Vec<ScoreRow> = self
            .select(&format!(
                "SELECT * FROM {} FINAL WHERE {} ORDER BY timestamp DESC, score_id DESC LIMIT {}",
                self.table("scores"),
                w.join(" AND "),
                q.limit.clamp(1, 500)
            ))
            .await?;
        Ok(Paged {
            items: rows.into_iter().map(Into::into).collect(),
            next_cursor: None,
            total: None,
        })
    }

    async fn analytics_summary(
        &self,
        project: &str,
        from: OffsetDateTime,
        to: OffsetDateTime,
        group_by: Option<&str>,
    ) -> Result<AnalyticsSummary> {
        #[derive(Deserialize)]
        struct Row {
            traces: u64,
            spans: u64,
            errors: u64,
            tin: u64,
            tout: u64,
            cost: f64,
            // Every quantile is nullable, and all six come back null over a
            // window holding no finished run. Read into `f64` that was not a
            // wrong latency, it was `invalid type: null, expected f64` and an
            // HTTP 500 for the overview screen of any project that had not
            // recorded anything yet. The embedded backend answers an empty
            // window with zeros and no latency, so this does.
            p50: Option<f64>,
            p90: Option<f64>,
            p95: Option<f64>,
            p99: Option<f64>,
            mean: Option<f64>,
            mx: Option<f64>,
        }

        let w = format!(
            "project = {} AND start_time >= {} AND start_time < {}",
            quote(project),
            ts(from),
            ts(to)
        );
        let rows: Vec<Row> = self
            .select(&format!(
                "SELECT count() AS traces, sum(span_count) AS spans,
                        countIf(status = 'error') AS errors,
                        sum(input_tokens) AS tin, sum(output_tokens) AS tout,
                        sum(total_cost) AS cost,
                        quantileExact(0.5)(duration_ns)/1e6 AS p50,
                        quantileExact(0.9)(duration_ns)/1e6 AS p90,
                        quantileExact(0.95)(duration_ns)/1e6 AS p95,
                        quantileExact(0.99)(duration_ns)/1e6 AS p99,
                        avg(duration_ns)/1e6 AS mean, max(duration_ns)/1e6 AS mx
                 FROM {} FINAL WHERE {w}",
                self.table("traces")
            ))
            .await?;

        let r = rows.into_iter().next().ok_or_else(|| Error::Backend("empty summary".into()))?;
        let groups = match group_by {
            Some(dim) => Some(self.grouped(project, from, to, dim).await?),
            None => None,
        };

        Ok(AnalyticsSummary {
            from,
            to,
            trace_count: r.traces,
            span_count: r.spans,
            error_count: r.errors,
            error_rate: if r.traces > 0 { r.errors as f64 / r.traces as f64 } else { 0.0 },
            latency: LatencyStats {
                p50: r.p50,
                p90: r.p90,
                p95: r.p95,
                p99: r.p99,
                mean: r.mean,
                max: r.mx,
            },
            input_tokens: r.tin,
            output_tokens: r.tout,
            total_cost: r.cost,
            currency: "USD".to_owned(),
            groups,
        })
    }

    async fn analytics_cost(
        &self,
        project: &str,
        from: OffsetDateTime,
        to: OffsetDateTime,
        group_by: &str,
        bucket_seconds: Option<u32>,
    ) -> Result<CostBreakdown> {
        let mut groups = self.grouped(project, from, to, group_by).await?;
        groups.sort_by(|a, b| {
            b.total_cost
                .unwrap_or(0.0)
                .partial_cmp(&a.total_cost.unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let total = groups.iter().filter_map(|g| g.total_cost).sum();

        let series = match bucket_seconds.filter(|b| *b > 0) {
            Some(bucket) => {
                // The same branch the totals above take, for the same reason.
                // A series built over `spans` for a run-scoped dimension drew
                // bars that did not add up to the table beside them, because
                // the table counted runs and the bars counted steps.
                let (col, table) = group_column(group_by)?;
                let scope = group_scope(col, table);
                #[derive(Deserialize)]
                struct Row {
                    bucket: String,
                    k: String,
                    c: f64,
                    tin: u64,
                    tout: u64,
                }
                let rows: Vec<Row> = self
                    .select(&format!(
                        "SELECT toString(toStartOfInterval(start_time, INTERVAL {bucket} SECOND)) AS bucket,
                                coalesce(nullIf({col}, ''), '(none)') AS k,
                                sum(coalesce(total_cost, 0)) AS c,
                                sum(coalesce(input_tokens, 0)) AS tin,
                                sum(coalesce(output_tokens, 0)) AS tout
                         FROM {} FINAL
                         WHERE project = {} AND start_time >= {} AND start_time < {}{scope}
                         GROUP BY bucket, k ORDER BY bucket ASC LIMIT 5000",
                        self.table(table), quote(project), ts(from), ts(to)
                    ))
                    .await?;
                Some(
                    rows.into_iter()
                        .map(|r| CostBucket {
                            timestamp: parse_ts(&format!("{}.000000000", r.bucket)),
                            key: r.k,
                            total_cost: r.c,
                            input_tokens: Some(r.tin),
                            output_tokens: Some(r.tout),
                        })
                        .collect(),
                )
            }
            None => None,
        };

        Ok(CostBreakdown {
            from,
            to,
            currency: "USD".to_owned(),
            total_cost: total,
            groups,
            series,
        })
    }

    async fn discover(&self, q: &DiscoveryQuery) -> Result<Vec<Discovered>> {
        if q.facet.needs_key() && q.key.as_deref().unwrap_or("").is_empty() {
            return Err(Error::InvalidQuery(
                "metric_label_value needs a key: ask for the values of one label, not for every \
                 label's values mixed together"
                    .to_owned(),
            ));
        }
        #[derive(Deserialize)]
        struct Row {
            value: String,
            last_seen: String,
        }
        let key =
            if q.facet.needs_key() { q.key.clone().unwrap_or_default() } else { String::new() };
        let mut w = format!(
            "project = {} AND facet = {} AND key = {}",
            quote(&q.project),
            quote(q.facet.as_str()),
            quote(&key)
        );
        // A range over the sort key rather than a `LIKE`, so typeahead reads
        // the answer and not the list. See `crate::prefix_upper_bound`.
        if let Some(prefix) = q.prefix.as_ref().filter(|p| !p.is_empty()) {
            w.push_str(&format!(" AND value >= {}", quote(prefix)));
            if let Some(upper) = crate::prefix_upper_bound(prefix) {
                w.push_str(&format!(" AND value < {}", quote(&upper)));
            }
        }
        // `GROUP BY` rather than `FINAL`. The table is a ReplacingMergeTree
        // whose version is `last_seen`, so folding the copies of a value is
        // exactly `max(last_seen)`, and asking for that directly costs nothing
        // like a merge over the whole part.
        let rows: Vec<Row> = self
            .select(&format!(
                "SELECT value, max(last_seen) AS last_seen FROM {} WHERE {w} \
                 GROUP BY value ORDER BY value ASC LIMIT {}",
                self.table("discovery"),
                q.effective_limit()
            ))
            .await?;
        Ok(rows
            .into_iter()
            .map(|r| Discovered { value: r.value, last_seen: parse_ts(&r.last_seen) })
            .collect())
    }

    async fn poll(&self, q: &DeltaQuery) -> Result<Delta<PolledSignal>> {
        let (table, id_col) = match q.signal {
            DeltaSignal::Traces => ("traces", "trace_id"),
            DeltaSignal::Spans => ("spans", "span_id"),
            DeltaSignal::Logs => ("logs", "log_id"),
        };
        // No cursor yet: start just before `since`, or at the beginning of
        // time. The id is empty, which sorts before every real id, so the first
        // row at exactly that nanosecond is included rather than skipped.
        let (watermark, after_id) = match &q.cursor {
            Some(c) => decode_delta_cursor(c)?,
            None => (q.since.map_or(0, |t| to_nanos(t).max(0)), String::new()),
        };
        let limit = match q.limit {
            0 => 100,
            n => n.min(1000),
        };
        // `ts_exact` and not `ts`: the watermark is a nanosecond and `ts`
        // renders one through an `f64`, which cannot hold nineteen significant
        // digits. A comparison off by a few nanoseconds either replays the last
        // row of every poll or skips it.
        let mark = ts_exact(from_nanos(watermark));
        let w = format!(
            "project = {} AND (received_at > {mark} OR (received_at = {mark} AND {id_col} > {}))",
            quote(&q.project),
            quote(&after_id)
        );
        // `FINAL` on the two ReplacingMergeTree tables. A run rebuilt by a
        // later batch sits in the table twice until a merge folds it, and a
        // poll that returned both would hand the caller the run it already has
        // beside the one it wants.
        let fin = if matches!(q.signal, DeltaSignal::Logs) { "" } else { " FINAL" };
        let sql = format!(
            "SELECT * FROM {}{fin} WHERE {w} ORDER BY received_at ASC, {id_col} ASC LIMIT {}",
            self.table(table),
            limit + 1
        );

        let (mut items, mut last): (Vec<PolledSignal>, Option<(i64, String)>) = (Vec::new(), None);
        let count;
        match q.signal {
            DeltaSignal::Traces => {
                let rows: Vec<TraceRow> = self.select(&sql).await?;
                count = rows.len();
                for r in rows.into_iter().take(limit as usize) {
                    let t: Trace = r.into();
                    last = Some((to_nanos(t.received_at), t.trace_id.clone()));
                    items.push(PolledSignal::Trace(Box::new(t)));
                }
            }
            DeltaSignal::Spans => {
                let rows: Vec<SpanRow> = self.select(&sql).await?;
                count = rows.len();
                for r in rows.into_iter().take(limit as usize) {
                    let sp: Span = r.into();
                    last = Some((to_nanos(sp.received_at), sp.span_id.clone()));
                    items.push(PolledSignal::Span(Box::new(sp)));
                }
            }
            DeltaSignal::Logs => {
                let rows: Vec<LogRow> = self.select(&sql).await?;
                count = rows.len();
                for r in rows.into_iter().take(limit as usize) {
                    let l: LogRecord = r.into();
                    last = Some((to_nanos(l.received_at), l.log_id.clone()));
                    items.push(PolledSignal::Log(Box::new(l)));
                }
            }
        }
        // Unchanged when nothing came back. Advancing an empty poll to "now" is
        // what would drop a row that lands a moment later with an earlier
        // arrival time.
        let cursor = last.map_or_else(
            || q.cursor.clone().unwrap_or_else(|| encode_delta_cursor(watermark, &after_id)),
            |(at, id)| encode_delta_cursor(at, &id),
        );
        Ok(Delta { items, cursor, has_more: count > limit as usize })
    }

    async fn apply_retention(
        &self,
        project: &str,
        policy: &RetentionPolicy,
        now: OffsetDateTime,
    ) -> Result<RetentionOutcome> {
        let mut out = RetentionOutcome::default();
        if policy.keeps_everything() {
            return Ok(out);
        }
        // The window is measured from when the thing happened, not from when
        // mcpd heard about it, and it is the same column each table is
        // partitioned on. That is what makes a whole-partition drop exact: a
        // window measured on arrival would not line up with the partitions.
        for (days, table, anchor) in [
            (policy.traces, "spans", "start_time"),
            (policy.traces, "traces", "start_time"),
            (policy.logs, "logs", "timestamp"),
            (policy.metrics, "metrics", "timestamp"),
            (policy.scores, "scores", "timestamp"),
            (policy.feedback, "feedback", "timestamp"),
            // Aspect notes go with the runs, not on a window of their own, and
            // no flag adds one. A note is a sentence about a trace: outliving
            // its trace it is prose about a run nobody can open. The anchor is
            // when the note was written rather than when its run started,
            // because writing a note lags the run, and it is the column the
            // table is partitioned on, which is what keeps a whole-partition
            // drop exact.
            (policy.traces, "aspect_notes", "created_at"),
            // The rest of the analysis chain, on the same window. A theme's
            // label is the text of one of its own notes, so the clustering has
            // to go with the notes or the sentence stays readable after the
            // note it came from has gone.
            (policy.traces, "themes", "cutoff_at"),
            (policy.traces, "theme_snapshots", "cutoff_at"),
        ] {
            let Some(cut) = RetentionPolicy::cutoff(days, now) else { continue };
            out.partitions_dropped +=
                self.drop_expired_partitions(table, anchor, project, cut).await?;
            // Then the rows the drops could not reach: the partition holding
            // the cutoff itself, and any partition another project still has
            // rows in.
            out.removed += u64::from(
                self.count_then_delete_now(
                    table,
                    &format!("project = {} AND {anchor} < {}", quote(project), ts_exact(cut)),
                )
                .await?,
            );
        }

        // Memberships are not partitioned by time, because they are read by
        // snapshot and never by date, so they are swept by naming the snapshots
        // that have just gone rather than by a cutoff of their own.
        if let Some(cut) = RetentionPolicy::cutoff(policy.traces, now) {
            out.removed += u64::from(
                self.count_then_delete_now(
                    "theme_members",
                    &format!(
                        "project = {p} AND snapshot_id NOT IN (
                           SELECT snapshot_id FROM {snapshots} WHERE project = {p})",
                        p = quote(project),
                        snapshots = self.table("theme_snapshots"),
                    ),
                )
                .await?,
            );
            let _ = cut;
        }

        // The filter box last, on the widest window: a value carried by a score
        // that is still stored is still a value the box should offer.
        let widest = policy
            .windows()
            .iter()
            .map(|(_, d)| *d)
            .max_by_key(|d| d.unwrap_or(u32::MAX))
            .flatten();
        if let Some(cut) = RetentionPolicy::cutoff(widest, now) {
            out.removed += u64::from(
                self.count_then_delete_now(
                    "discovery",
                    &format!("project = {} AND last_seen < {}", quote(project), ts_exact(cut)),
                )
                .await?,
            );
        }
        Ok(out)
    }

    async fn declare_retention(&self, policy: &RetentionPolicy) -> Result<RetentionOutcome> {
        // Deployment-wide, so this is the one place a table's own TTL may be
        // set. `apply_retention` is given one project and must never touch it:
        // every project shares these tables, and a window declared for one of
        // them would expire the rest on a window nobody chose for them.
        let mut out = RetentionOutcome::default();
        for (days, table, anchor) in [
            (policy.traces, "spans", "start_time"),
            (policy.traces, "traces", "start_time"),
            (policy.logs, "logs", "timestamp"),
            (policy.metrics, "metrics", "timestamp"),
            (policy.scores, "scores", "timestamp"),
            (policy.feedback, "feedback", "timestamp"),
        ] {
            // `None` means keep it. A signal with no window is left exactly as
            // it is rather than having a TTL removed: an operator who set no
            // window for scores has said nothing about the one the table was
            // created with, and reading silence as "delete the declaration"
            // would quietly disable a retention somebody does rely on.
            let Some(days) = days else { continue };
            out.ttl_changed |= self.set_table_ttl(table, anchor, days).await?;
        }
        Ok(out)
    }

    async fn enforce_retention(&self, project: &str, older_than: OffsetDateTime) -> Result<u64> {
        // The tables carry a TTL, so ClickHouse expires most data on its own.
        // This exists for an explicit request, and for a project whose retention
        // is shorter than the table's.
        for table in ["spans", "traces", "logs", "metrics"] {
            self.execute(&format!(
                "DELETE FROM {} WHERE project = {} AND received_at < {}",
                self.table(table),
                quote(project),
                ts(older_than)
            ))
            .await?;
        }
        for table in ["scores", "feedback"] {
            self.execute(&format!(
                "DELETE FROM {} WHERE project = {} AND timestamp < {}",
                self.table(table),
                quote(project),
                ts(older_than)
            ))
            .await?;
        }
        // The analysis chain, in dependency order and on the same cutoff. A
        // theme's label is the text of one of its own notes, so the clustering
        // has to go with the notes or the sentence stays readable after the note
        // it came from has gone.
        for (table, anchor) in [
            ("aspect_notes", "created_at"),
            ("themes", "cutoff_at"),
            ("theme_snapshots", "cutoff_at"),
        ] {
            self.execute(&format!(
                "DELETE FROM {} WHERE project = {} AND {anchor} < {}",
                self.table(table),
                quote(project),
                ts(older_than)
            ))
            .await?;
        }
        // Memberships are read by snapshot and never by date, so they are swept
        // by naming the snapshots that have just gone rather than by a cutoff
        // of their own.
        self.execute(&format!(
            "DELETE FROM {members} WHERE project = {p} AND snapshot_id NOT IN (
               SELECT snapshot_id FROM {snapshots} WHERE project = {p})",
            members = self.table("theme_members"),
            snapshots = self.table("theme_snapshots"),
            p = quote(project),
        ))
        .await?;
        Ok(0)
    }

    async fn put_dataset(&self, d: &Dataset) -> Result<()> {
        let row = json!({
            "dataset_id": d.dataset_id, "name": d.name,
            "description": d.description.clone().unwrap_or_default(),
            "item_count": d.item_count, "created_at": ts_str(d.created_at),
            "project": d.correlation.project,
            "environment": d.correlation.environment.clone().unwrap_or_default(),
        });
        self.post(
            &format!("INSERT INTO {} FORMAT JSONEachRow", self.table("datasets")),
            Some(row.to_string()),
        )
        .await
        .map(|_| ())
    }

    async fn dataset_project(&self, dataset_id: &str) -> Result<Option<String>> {
        #[derive(Deserialize)]
        struct Row {
            project: String,
        }
        // This value decides who may read a dataset, so it has to be total.
        //
        // The previous query took the first row a `LIMIT 1` reached and said
        // FINAL made that safe. It does not. `datasets` is sorted by
        // `(project, dataset_id)`, so two projects that pick the same dataset
        // id are two different keys and no merge will ever collapse them. FINAL
        // deduplicates within one key and cannot touch this, so the answer was
        // whichever part the scan hit first, and a caller could be authorised
        // against a tenant that is not the owner.
        //
        // So read every project the id resolves to instead of one of them, and
        // refuse when there is more than one. Ordering and picking a winner was
        // the other option and it is worse: it would make the answer stable
        // while still being the wrong tenant half the time. An ambiguous owner
        // has no safe answer except none. `ORDER BY` makes the single-owner
        // case deterministic, and `LIMIT 2` is all the evidence the refusal
        // needs. FINAL is gone with the `LIMIT 1` that made it look necessary:
        // `DISTINCT` already folds the rewritten copies of one key together,
        // and every copy of a key carries the same project by construction.
        let rows: Vec<Row> = self
            .select(&format!(
                "SELECT DISTINCT project FROM {} WHERE dataset_id = {}
                 ORDER BY project ASC LIMIT 2",
                self.table("datasets"),
                quote(dataset_id)
            ))
            .await?;
        if rows.len() > 1 {
            // The projects are left out of the message on purpose. The caller
            // supplied only a dataset id and holds no grant for either tenant,
            // so naming them here would answer a question they were not
            // allowed to ask.
            return Err(Error::Backend(format!(
                "dataset {dataset_id} is claimed by more than one project, so its owner \
                 is ambiguous and no read can be authorised. Remove the duplicate row."
            )));
        }
        Ok(rows.into_iter().next().map(|r| r.project))
    }

    async fn list_datasets(&self, project: &str) -> Result<Vec<Dataset>> {
        #[derive(Deserialize)]
        struct Row {
            dataset_id: String,
            name: String,
            description: String,
            item_count: u32,
            created_at: String,
            project: String,
            environment: String,
        }
        let rows: Vec<Row> = self
            .select(&format!(
                "SELECT * FROM {} FINAL WHERE project = {} ORDER BY created_at DESC",
                self.table("datasets"),
                quote(project)
            ))
            .await?;
        Ok(rows
            .into_iter()
            .map(|r| Dataset {
                dataset_id: r.dataset_id,
                name: r.name,
                description: opt(r.description),
                item_count: r.item_count,
                created_at: parse_ts(&r.created_at),
                correlation: Correlation {
                    project: r.project,
                    environment: opt(r.environment),
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
            })
            .collect())
    }

    async fn add_dataset_items(&self, dataset_id: &str, items: &[DatasetItem]) -> Result<u32> {
        if items.is_empty() {
            return Ok(0);
        }
        let body = items
            .iter()
            .map(|i| {
                json!({
                    "item_id": i.item_id, "dataset_id": dataset_id,
                    "input": serde_json::to_string(&i.input).unwrap_or_default(),
                    "expected": json_text(&i.expected),
                    "metadata": json_text(&i.metadata),
                    "source_trace_id": i.source_trace_id.clone().unwrap_or_default(),
                })
                .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n");
        self.post(
            &format!("INSERT INTO {} FORMAT JSONEachRow", self.table("dataset_items")),
            Some(body),
        )
        .await?;
        Ok(items.len() as u32)
    }

    async fn list_dataset_items(&self, dataset_id: &str, limit: u32) -> Result<Vec<DatasetItem>> {
        #[derive(Deserialize)]
        struct Row {
            item_id: String,
            dataset_id: String,
            input: String,
            expected: String,
            metadata: String,
            source_trace_id: String,
        }
        let rows: Vec<Row> = self
            .select(&format!(
                "SELECT * FROM {} FINAL WHERE dataset_id = {} LIMIT {}",
                self.table("dataset_items"),
                quote(dataset_id),
                limit.clamp(1, 5000)
            ))
            .await?;
        Ok(rows
            .into_iter()
            .map(|r| DatasetItem {
                item_id: r.item_id,
                dataset_id: r.dataset_id,
                input: parse_json(&r.input).unwrap_or(Value::Null),
                expected: parse_json(&r.expected),
                metadata: parse_json(&r.metadata),
                source_trace_id: opt(r.source_trace_id),
            })
            .collect())
    }

    async fn put_experiment(&self, e: &Experiment) -> Result<()> {
        let row = json!({
            "experiment_id": e.experiment_id, "name": e.name, "dataset_id": e.dataset_id,
            "status": e.status.as_str(),
            "started_at": e.started_at.map(ts_str), "finished_at": e.finished_at.map(ts_str),
            "item_count": e.item_count, "summary": json_text(&e.summary),
            "baseline_id": e.baseline_id.clone().unwrap_or_default(),
            "cost_json": e.cost.as_ref().map(|c| serde_json::to_string(c).unwrap_or_default()).unwrap_or_default(),
            "error": e.error.clone().unwrap_or_default(),
            "metadata": json_text(&e.metadata),
            "project": e.correlation.project,
            "environment": e.correlation.environment.clone().unwrap_or_default(),
        });
        self.post(
            &format!("INSERT INTO {} FORMAT JSONEachRow", self.table("experiments")),
            Some(row.to_string()),
        )
        .await
        .map(|_| ())
    }

    async fn list_experiments(&self, project: &str) -> Result<Vec<Experiment>> {
        #[derive(Deserialize)]
        struct Row {
            experiment_id: String,
            name: String,
            dataset_id: String,
            status: String,
            started_at: Option<String>,
            finished_at: Option<String>,
            item_count: u32,
            summary: String,
            baseline_id: String,
            cost_json: String,
            error: String,
            metadata: String,
            project: String,
            environment: String,
        }
        let rows: Vec<Row> = self
            .select(&format!(
                "SELECT * FROM {} FINAL WHERE project = {} ORDER BY started_at DESC",
                self.table("experiments"),
                quote(project)
            ))
            .await?;
        Ok(rows
            .into_iter()
            .map(|r| Experiment {
                experiment_id: r.experiment_id,
                name: r.name,
                dataset_id: r.dataset_id,
                status: ExperimentStatus::from_str(&r.status).unwrap_or(ExperimentStatus::Pending),
                started_at: r.started_at.as_deref().map(parse_ts),
                finished_at: r.finished_at.as_deref().map(parse_ts),
                item_count: r.item_count,
                summary: parse_json(&r.summary),
                baseline_id: opt(r.baseline_id),
                cost: parse_json(&r.cost_json).and_then(|v| serde_json::from_value(v).ok()),
                error: opt(r.error),
                metadata: parse_json(&r.metadata),
                correlation: Correlation {
                    project: r.project,
                    environment: opt(r.environment),
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
            })
            .collect())
    }

    async fn record_audit(&self, event: &AuditEvent) -> Result<()> {
        let row = json!({
            "audit_id": event.audit_id,
            "timestamp": ts_str(event.timestamp),
            "principal": event.principal,
            "subject": event.subject,
            "action": event.action,
            "project": event.project,
            "resource": event.resource.clone().unwrap_or_default(),
            "result": event.result.as_str(),
        });
        self.post(
            &format!("INSERT INTO {} FORMAT JSONEachRow", self.table("audit_log")),
            Some(row.to_string()),
        )
        .await
        .map(|_| ())
    }

    async fn list_audit(&self, q: &AuditQuery) -> Result<Vec<AuditEvent>> {
        #[derive(Deserialize)]
        struct Row {
            audit_id: String,
            timestamp: String,
            principal: String,
            subject: String,
            action: String,
            project: String,
            resource: String,
            result: String,
        }
        let mut w = vec![format!("project = {}", quote(&q.project))];
        if let Some(a) = &q.action {
            w.push(format!("action = {}", quote(a)));
        }
        if let Some(f) = q.from {
            w.push(format!("timestamp >= {}", ts(f)));
        }
        if let Some(t) = q.to {
            w.push(format!("timestamp < {}", ts(t)));
        }
        let rows: Vec<Row> = self
            .select(&format!(
                "SELECT audit_id, timestamp, principal, subject, action, project, resource, result
                 FROM {} FINAL WHERE {} ORDER BY timestamp DESC LIMIT {}",
                self.table("audit_log"),
                w.join(" AND "),
                q.limit.clamp(1, 1000)
            ))
            .await?;
        Ok(rows
            .into_iter()
            .map(|r| AuditEvent {
                audit_id: r.audit_id,
                timestamp: parse_ts(&r.timestamp),
                principal: r.principal,
                subject: r.subject,
                action: r.action,
                project: r.project,
                resource: opt(r.resource),
                // An unreadable result is a row written by a build that knew a
                // variant this one does not. Refused is the safe reading: it
                // never claims an access was served.
                result: AuditResult::from_str(&r.result).unwrap_or(AuditResult::Refused),
            })
            .collect())
    }

    async fn list_projects(&self) -> Result<Vec<String>> {
        #[derive(Deserialize)]
        struct Row {
            project: String,
        }
        let rows: Vec<Row> = self
            .select(&format!(
                "SELECT DISTINCT project FROM {} ORDER BY project",
                self.table("traces")
            ))
            .await?;
        Ok(rows.into_iter().map(|r| r.project).collect())
    }

    // ----------------------------------------------------- aspect analysis

    async fn list_aspect_notes(&self, q: &AspectNoteQuery) -> Result<Paged<StoredNote>> {
        let mut w = vec![format!("project = {}", quote(&q.project))];
        if let Some(t) = &q.trace_id {
            w.push(format!("trace_id = {}", quote(t)));
        }
        if let Some(a) = &q.aspect {
            w.push(format!("aspect = {}", quote(a)));
        }
        if let Some((kind, name)) = &q.entity {
            w.push(format!("entity_kind = {}", quote(kind.as_str())));
            w.push(format!("entity_name = {}", quote(name)));
        }
        if let Some(f) = q.from {
            w.push(format!("created_at >= {}", ts(f)));
        }
        if let Some(t) = q.to {
            w.push(format!("created_at < {}", ts(t)));
        }
        if !q.note_ids.is_empty() {
            let list = q.note_ids.iter().map(|i| quote(i)).collect::<Vec<_>>().join(",");
            w.push(format!("note_id IN ({list})"));
        }
        let desc = matches!(q.order, Order::Desc);
        let (cmp, dir) = if desc { ('<', "DESC") } else { ('>', "ASC") };
        if let Some(c) = &q.cursor {
            let (v, id) = decode_cursor(c)?;
            w.push(format!("(created_at, note_id) {cmp} ({}, {})", nanos_literal(v), quote(&id)));
        }

        let limit = q.limit.clamp(1, 1000);
        let rows: Vec<NoteRow> = self
            .select(&format!(
                "SELECT * FROM {} FINAL WHERE {} ORDER BY created_at {dir}, note_id {dir} \
                 LIMIT {}",
                self.table("aspect_notes"),
                w.join(" AND "),
                limit + 1
            ))
            .await?;
        let mut items: Vec<StoredNote> = rows.into_iter().map(StoredNote::from).collect();
        let next_cursor = if items.len() > limit as usize {
            items.truncate(limit as usize);
            items.last().map(|n| encode_cursor(n.note.created_at, &n.note.note_id))
        } else {
            None
        };
        Ok(Paged { items, next_cursor, total: None })
    }

    async fn aspect_note_rollup(&self, q: &AspectRollupQuery) -> Result<Vec<AspectNoteRollup>> {
        #[derive(Deserialize)]
        struct Row {
            entity_kind: String,
            entity_name: String,
            aspect: String,
            note_count: u64,
            trace_count: u64,
            last_note_at: String,
        }
        let mut w = vec![
            format!("project = {}", quote(&q.project)),
            // A note whose trace was not stored when it arrived has no entity,
            // and a roll-up is by entity, so those rows sit outside every group.
            "entity_name != ''".to_owned(),
            "entity_kind != ''".to_owned(),
        ];
        if let Some((kind, name)) = &q.entity {
            w.push(format!("entity_kind = {}", quote(kind.as_str())));
            w.push(format!("entity_name = {}", quote(name)));
        } else if let Some(kind) = q.entity_kind {
            w.push(format!("entity_kind = {}", quote(kind.as_str())));
        }
        let rows: Vec<Row> = self
            .select(&format!(
                "SELECT entity_kind, entity_name, aspect, toUInt64(count()) AS note_count, \
                        toUInt64(uniqExact(trace_id)) AS trace_count, \
                        toString(max(created_at)) AS last_note_at \
                   FROM {} FINAL WHERE {} \
                  GROUP BY entity_kind, entity_name, aspect \
                  ORDER BY last_note_at DESC, entity_name ASC, aspect ASC LIMIT {}",
                self.table("aspect_notes"),
                w.join(" AND "),
                q.max_groups.clamp(1, 10_000)
            ))
            .await?;
        Ok(rows
            .into_iter()
            .map(|r| AspectNoteRollup {
                entity_kind: EntityKind::from_str(&r.entity_kind).unwrap_or(EntityKind::Service),
                entity_name: r.entity_name,
                aspect: r.aspect,
                note_count: r.note_count,
                trace_count: r.trace_count,
                last_note_at: parse_ts(&r.last_note_at),
            })
            .collect())
    }

    async fn write_theme_snapshot(&self, write: &SnapshotWrite) -> Result<()> {
        // No transaction, and no honest way to fake one here: ClickHouse
        // transactions are experimental and do not cover these statements. What
        // makes that acceptable is that a pass is a pure function of the notes
        // it read, so a write interrupted halfway is finished by the next pass
        // writing the same snapshot id again over the top.
        let s = &write.snapshot;
        for table in ["themes", "theme_members", "theme_snapshots"] {
            self.execute(&format!(
                "DELETE FROM {} WHERE project = {} AND snapshot_id = {}",
                self.table(table),
                quote(&s.project),
                quote(&s.snapshot_id)
            ))
            .await?;
        }

        self.post(
            &format!("INSERT INTO {} FORMAT JSONEachRow", self.table("theme_snapshots")),
            Some(
                json!({
                    "snapshot_id": s.snapshot_id, "project": s.project,
                    "entity_kind": s.entity_kind.as_str(), "entity_name": s.entity_name,
                    "aspects": serde_json::to_string(&s.aspects).unwrap_or_else(|_| "[]".into()),
                    "ordinal": s.ordinal,
                    "cutoff_at": ts_str(s.cutoff_at),
                    "window_from": ts_str(s.window_from), "window_to": ts_str(s.window_to),
                    "trace_count": s.trace_count, "note_count": s.note_count,
                    "capped": u8::from(s.capped),
                })
                .to_string(),
            ),
        )
        .await?;

        if !write.themes.is_empty() {
            let body = write
                .themes
                .iter()
                .map(|t| stored_theme_json(t).to_string())
                .collect::<Vec<_>>()
                .join("\n");
            self.post(
                &format!("INSERT INTO {} FORMAT JSONEachRow", self.table("themes")),
                Some(body),
            )
            .await?;
        }
        if !write.members.is_empty() {
            let body = write
                .members
                .iter()
                .map(|m| {
                    json!({
                        "snapshot_id": m.snapshot_id, "aspect": m.aspect,
                        "theme_id": m.theme_id, "note_id": m.note_id,
                        "trace_id": m.trace_id, "project": m.project,
                        "similarity": m.similarity,
                        "trace_started_at": ts_str(m.trace_started_at),
                    })
                    .to_string()
                })
                .collect::<Vec<_>>()
                .join("\n");
            self.post(
                &format!("INSERT INTO {} FORMAT JSONEachRow", self.table("theme_members")),
                Some(body),
            )
            .await?;
        }
        Ok(())
    }

    async fn list_theme_snapshots(&self, q: &SnapshotQuery) -> Result<Paged<ThemeSnapshot>> {
        let mut w = vec![format!("project = {}", quote(&q.project))];
        if let Some(id) = &q.snapshot_id {
            w.push(format!("snapshot_id = {}", quote(id)));
        }
        if let Some((kind, name)) = &q.entity {
            w.push(format!("entity_kind = {}", quote(kind.as_str())));
            w.push(format!("entity_name = {}", quote(name)));
        }
        if let Some(f) = q.from {
            w.push(format!("cutoff_at >= {}", ts(f)));
        }
        if let Some(t) = q.to {
            w.push(format!("cutoff_at < {}", ts(t)));
        }
        for aspect in &q.aspects {
            // The column holds a JSON array of names, so membership is a search
            // for the quoted name. The quotes are what stop `goal` matching an
            // aspect called `goal_reached`.
            w.push(format!("position(aspects, {}) > 0", quote(&format!("\"{aspect}\""))));
        }

        // Counted before the cursor is added, so it is how many the filter
        // matched and not how many are left after this page.
        let counted: Vec<CountRow> = self
            .select(&format!(
                "SELECT toUInt32(count()) AS n FROM {} FINAL WHERE {}",
                self.table("theme_snapshots"),
                w.join(" AND ")
            ))
            .await?;
        let total = u64::from(counted.first().map_or(0, |c| c.n));

        let desc = matches!(q.order, Order::Desc);
        let (cmp, dir) = if desc { ('<', "DESC") } else { ('>', "ASC") };
        if let Some(c) = &q.cursor {
            let (v, id) = decode_cursor(c)?;
            w.push(format!(
                "(cutoff_at, snapshot_id) {cmp} ({}, {})",
                nanos_literal(v),
                quote(&id)
            ));
        }
        let limit = q.limit.clamp(1, 1000);
        let rows: Vec<SnapshotRow> = self
            .select(&format!(
                "SELECT * FROM {} FINAL WHERE {} ORDER BY cutoff_at {dir}, snapshot_id {dir} \
                 LIMIT {}",
                self.table("theme_snapshots"),
                w.join(" AND "),
                limit + 1
            ))
            .await?;
        let mut items: Vec<ThemeSnapshot> = rows.into_iter().map(ThemeSnapshot::from).collect();
        let next_cursor = if items.len() > limit as usize {
            items.truncate(limit as usize);
            items.last().map(|s| encode_cursor(s.cutoff_at, &s.snapshot_id))
        } else {
            None
        };
        Ok(Paged { items, next_cursor, total: Some(total) })
    }

    async fn list_themes(&self, q: &ThemeQuery) -> Result<Paged<StoredTheme>> {
        let mut w = vec![format!("project = {}", quote(&q.project))];
        if let Some(id) = &q.snapshot_id {
            w.push(format!("snapshot_id = {}", quote(id)));
        }
        if let Some((kind, name)) = &q.entity {
            w.push(format!("entity_kind = {}", quote(kind.as_str())));
            w.push(format!("entity_name = {}", quote(name)));
        }
        if let Some(a) = &q.aspect {
            w.push(format!("aspect = {}", quote(a)));
        }
        if let Some(t) = &q.theme_id {
            w.push(format!("theme_id = {}", quote(t)));
        }
        let desc = matches!(q.order, Order::Desc);
        let (cmp, dir) = if desc { ('<', "DESC") } else { ('>', "ASC") };
        if let Some(c) = &q.cursor {
            let (v, id) = decode_cursor(c)?;
            w.push(format!("(cutoff_at, theme_id) {cmp} ({}, {})", nanos_literal(v), quote(&id)));
        }
        let limit = q.limit.clamp(1, 1000);
        let rows: Vec<ThemeRow> = self
            .select(&format!(
                "SELECT {THEME_COLUMNS} FROM {} FINAL WHERE {} \
                 ORDER BY cutoff_at {dir}, theme_id {dir} LIMIT {}",
                self.table("themes"),
                w.join(" AND "),
                limit + 1
            ))
            .await?;
        let mut items: Vec<StoredTheme> = rows.into_iter().map(StoredTheme::from).collect();
        let next_cursor = if items.len() > limit as usize {
            items.truncate(limit as usize);
            items.last().map(|t| encode_cursor(t.cutoff_at, &t.theme.theme_id))
        } else {
            None
        };
        Ok(Paged { items, next_cursor, total: None })
    }

    async fn list_theme_members(&self, q: &ThemeMemberQuery) -> Result<Paged<ThemeMember>> {
        #[derive(Deserialize)]
        struct Row {
            snapshot_id: String,
            aspect: String,
            theme_id: String,
            note_id: String,
            trace_id: String,
            project: String,
            similarity: u32,
            trace_started_at: String,
        }
        let mut w = vec![
            format!("project = {}", quote(&q.project)),
            format!("snapshot_id = {}", quote(&q.snapshot_id)),
        ];
        if let Some(a) = &q.aspect {
            w.push(format!("aspect = {}", quote(a)));
        }
        if let Some(t) = &q.theme_id {
            w.push(format!("theme_id = {}", quote(t)));
        }
        if let Some(c) = &q.cursor {
            // The sort runs one way on similarity and the other on the note id,
            // so the resume condition is written out rather than borrowed from a
            // walk whose two keys point the same way.
            let (v, id) = decode_cursor(c)?;
            w.push(format!(
                "(similarity < {v} OR (similarity = {v} AND note_id > {}))",
                quote(&id)
            ));
        }
        let limit = q.limit.clamp(1, 20_000);
        let rows: Vec<Row> = self
            .select(&format!(
                "SELECT snapshot_id, aspect, theme_id, note_id, trace_id, project, similarity, \
                        toString(trace_started_at) AS trace_started_at \
                   FROM {} FINAL WHERE {} ORDER BY similarity DESC, note_id ASC LIMIT {}",
                self.table("theme_members"),
                w.join(" AND "),
                limit + 1
            ))
            .await?;
        let mut items: Vec<ThemeMember> = rows
            .into_iter()
            .map(|r| ThemeMember {
                snapshot_id: r.snapshot_id,
                aspect: r.aspect,
                theme_id: r.theme_id,
                note_id: r.note_id,
                trace_id: r.trace_id,
                project: r.project,
                similarity: r.similarity,
                trace_started_at: parse_ts(&r.trace_started_at),
            })
            .collect();
        let next_cursor = if items.len() > limit as usize {
            items.truncate(limit as usize);
            items.last().map(|m| hex::encode(format!("{}|{}", m.similarity, m.note_id)))
        } else {
            None
        };
        Ok(Paged { items, next_cursor, total: None })
    }
}

/// The theme columns, named so the `SELECT` and [`ThemeRow`] cannot drift.
///
/// `toString` on the three time columns because `JSONEachRow` renders a
/// `DateTime64` to second resolution unless it is asked for as text, and a
/// history ordered on a value that lost its subsecond part would reorder two
/// passes that ran in the same second.
const THEME_COLUMNS: &str = "snapshot_id, aspect, theme_id, project, entity_kind, entity_name, \
     label, label_note_id, label_trace_id, state, note_count, trace_count, coverage, \
     toString(cutoff_at) AS cutoff_at, toString(window_from) AS window_from, \
     toString(window_to) AS window_to, split_from, split_into, merged_from, merged_into";

/// The root span's identity for one trace, read once per ingest batch.
#[derive(Deserialize)]
struct RootRow {
    project: String,
    trace_id: String,
    entity_kind: String,
    entity_name: String,
    start_time: String,
}

/// One stored aspect note.
#[derive(Deserialize)]
struct NoteRow {
    note_id: String,
    trace_id: String,
    aspect: String,
    text: String,
    definition_version: u32,
    model: String,
    created_at: String,
    entity_kind: String,
    entity_name: String,
    trace_started_at: String,
    sketch: String,
    project: String,
    environment: String,
    service_name: String,
    service_version: String,
    session_id: String,
    thread_id: String,
    user_id: String,
    resource_id: String,
    run_id: String,
    request_id: String,
    experiment_id: String,
    release: String,
}

impl From<NoteRow> for StoredNote {
    fn from(r: NoteRow) -> Self {
        Self {
            note: AspectNote {
                note_id: r.note_id,
                trace_id: r.trace_id,
                aspect: r.aspect,
                text: r.text,
                definition_version: r.definition_version,
                model: opt(r.model),
                created_at: parse_ts(&r.created_at),
                correlation: Correlation {
                    project: r.project,
                    environment: opt(r.environment),
                    service_name: opt(r.service_name),
                    service_version: opt(r.service_version),
                    session_id: opt(r.session_id),
                    thread_id: opt(r.thread_id),
                    user_id: opt(r.user_id),
                    resource_id: opt(r.resource_id),
                    run_id: opt(r.run_id),
                    request_id: opt(r.request_id),
                    experiment_id: opt(r.experiment_id),
                    release: opt(r.release),
                },
            },
            entity_kind: opt(r.entity_kind).and_then(|k| EntityKind::from_str(&k).ok()),
            entity_name: opt(r.entity_name),
            trace_started_at: Some(parse_ts(&r.trace_started_at)),
            // Stored as hex, because the insert path is JSON and JSON carries
            // text. An unreadable value reads as absent and the pass sketches
            // the text again, which is why this cannot fail the read.
            sketch: hex::decode(r.sketch.trim_end_matches('\0'))
                .ok()
                .as_deref()
                .and_then(mcpd_core::sketch::Sketch::from_bytes),
        }
    }
}

/// One stored snapshot header.
#[derive(Deserialize)]
struct SnapshotRow {
    snapshot_id: String,
    project: String,
    entity_kind: String,
    entity_name: String,
    aspects: String,
    ordinal: u64,
    cutoff_at: String,
    window_from: String,
    window_to: String,
    trace_count: u64,
    note_count: u64,
    capped: u8,
}

impl From<SnapshotRow> for ThemeSnapshot {
    fn from(r: SnapshotRow) -> Self {
        Self {
            snapshot_id: r.snapshot_id,
            project: r.project,
            entity_kind: EntityKind::from_str(&r.entity_kind).unwrap_or(EntityKind::Service),
            entity_name: r.entity_name,
            aspects: serde_json::from_str(&r.aspects).unwrap_or_default(),
            ordinal: r.ordinal,
            cutoff_at: parse_ts(&r.cutoff_at),
            window_from: parse_ts(&r.window_from),
            window_to: parse_ts(&r.window_to),
            trace_count: r.trace_count,
            note_count: r.note_count,
            capped: r.capped != 0,
        }
    }
}

/// One stored theme.
#[derive(Deserialize)]
struct ThemeRow {
    snapshot_id: String,
    aspect: String,
    theme_id: String,
    project: String,
    entity_kind: String,
    entity_name: String,
    label: String,
    label_note_id: String,
    label_trace_id: String,
    state: String,
    note_count: u64,
    trace_count: u64,
    coverage: f64,
    cutoff_at: String,
    window_from: String,
    window_to: String,
    split_from: String,
    split_into: String,
    merged_from: String,
    merged_into: String,
}

impl From<ThemeRow> for StoredTheme {
    fn from(r: ThemeRow) -> Self {
        Self {
            theme: Theme {
                theme_id: r.theme_id,
                snapshot_id: r.snapshot_id,
                aspect: r.aspect,
                entity_kind: EntityKind::from_str(&r.entity_kind).unwrap_or(EntityKind::Service),
                entity_name: r.entity_name,
                label: r.label,
                label_note_id: r.label_note_id,
                label_trace_id: r.label_trace_id,
                state: ThemeState::from_str(&r.state).unwrap_or(ThemeState::Continue),
                note_count: r.note_count,
                trace_count: r.trace_count,
                coverage: r.coverage,
            },
            project: r.project,
            cutoff_at: parse_ts(&r.cutoff_at),
            window_from: parse_ts(&r.window_from),
            window_to: parse_ts(&r.window_to),
            lineage: ThemeLineage {
                split_from: list_in(&r.split_from),
                split_into: list_in(&r.split_into),
                merged_from: list_in(&r.merged_from),
                merged_into: list_in(&r.merged_into),
            },
        }
    }
}

/// One theme as it is written, from the type the daemon holds.
fn stored_theme_json(t: &StoredTheme) -> Value {
    let th = &t.theme;
    json!({
        "snapshot_id": th.snapshot_id, "aspect": th.aspect, "theme_id": th.theme_id,
        "project": t.project, "entity_kind": th.entity_kind.as_str(),
        "entity_name": th.entity_name, "label": th.label,
        "label_note_id": th.label_note_id, "label_trace_id": th.label_trace_id,
        "state": th.state.as_str(), "note_count": th.note_count,
        "trace_count": th.trace_count, "coverage": th.coverage,
        "cutoff_at": ts_str(t.cutoff_at),
        "window_from": ts_str(t.window_from), "window_to": ts_str(t.window_to),
        "split_from": list_out(&t.lineage.split_from),
        "split_into": list_out(&t.lineage.split_into),
        "merged_from": list_out(&t.lineage.merged_from),
        "merged_into": list_out(&t.lineage.merged_into),
    })
}

/// The same, from a row read back, so an erasure can rewrite one field of a
/// theme without having to rebuild the whole value first.
fn theme_row_json(r: &ThemeRow) -> Value {
    json!({
        "snapshot_id": r.snapshot_id, "aspect": r.aspect, "theme_id": r.theme_id,
        "project": r.project, "entity_kind": r.entity_kind, "entity_name": r.entity_name,
        "label": r.label, "label_note_id": r.label_note_id,
        "label_trace_id": r.label_trace_id, "state": r.state,
        "note_count": r.note_count, "trace_count": r.trace_count, "coverage": r.coverage,
        "cutoff_at": r.cutoff_at, "window_from": r.window_from, "window_to": r.window_to,
        "split_from": r.split_from, "split_into": r.split_into,
        "merged_from": r.merged_from, "merged_into": r.merged_into,
    })
}

/// A list of theme ids as stored: JSON text, empty when there is nothing in it.
fn list_out(ids: &[String]) -> String {
    if ids.is_empty() {
        String::new()
    } else {
        serde_json::to_string(ids).unwrap_or_default()
    }
}

fn list_in(raw: &str) -> Vec<String> {
    if raw.is_empty() {
        return Vec::new();
    }
    serde_json::from_str(raw).unwrap_or_default()
}

/// A page cursor: the sort value and the id it belongs to, hex encoded.
///
/// Opaque by construction, and the same shape the other two backends use, so a
/// caller that carries one between pages never has to know which backend wrote
/// it.
fn encode_cursor(at: OffsetDateTime, id: &str) -> String {
    hex::encode(format!("{}|{id}", at.unix_timestamp_nanos().clamp(0, i128::from(i64::MAX))))
}

fn decode_cursor(cursor: &str) -> Result<(i64, String)> {
    let raw = hex::decode(cursor)
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
        .ok_or_else(|| Error::InvalidQuery("malformed cursor".into()))?;
    let (v, id) =
        raw.split_once('|').ok_or_else(|| Error::InvalidQuery("malformed cursor".into()))?;
    let v = v.parse().map_err(|_| Error::InvalidQuery("malformed cursor".into()))?;
    Ok((v, id.to_owned()))
}

/// A nanosecond count as the literal a `DateTime64(9)` column compares against.
fn nanos_literal(nanos: i64) -> String {
    ts(OffsetDateTime::from_unix_timestamp_nanos(i128::from(nanos))
        .unwrap_or(OffsetDateTime::UNIX_EPOCH))
}

/// Map a caller-supplied dimension to a column and the table that answers it.
///
/// An allowlist, exactly as in the embedded backend: `group_by` arrives from a
/// query string, and ClickHouse's HTTP interface has no bind parameters, so an
/// unchecked value here would be an injection.
///
/// The table is half the answer, not an implementation detail. Six of these
/// nine dimensions describe the run, and `GroupedStat::count` is specified as
/// how many runs fell in the group, so they are read from `traces`, which holds
/// one row per run. Every dimension used to be read from the denormalised
/// `spans` table, which holds one row per step, so a run of forty steps landed
/// forty times in its own `environment` group while the same response reported
/// `trace_count` beside it. Group by `status` and the groups did not add up to
/// the total in the payload they arrived in.
///
/// The other three describe a step. A run that called two models belongs to
/// both, so those are read from `spans`.
///
/// `name` is where the disagreement needed no arithmetic to see: on `traces` it
/// is the run's name, on `spans` it is the step's, so the same request answered
/// with a list of runs on one backend and a list of step names on another.
fn group_column(dim: &str) -> Result<(&'static str, &'static str)> {
    Ok(match dim {
        "model" => ("model", "spans"),
        "provider" => ("provider", "spans"),
        "entity_name" => ("entity_name", "spans"),
        "environment" => ("environment", "traces"),
        "status" => ("status", "traces"),
        "session_id" => ("session_id", "traces"),
        "user_id" => ("user_id", "traces"),
        "service_name" => ("service_name", "traces"),
        "name" => ("name", "traces"),
        other => {
            return Err(Error::InvalidQuery(format!(
                "cannot group by {other:?}; supported: model, provider, entity_name, \
                 environment, status, session_id, user_id, service_name, name"
            )))
        }
    })
}

/// The extra predicate a span-scoped dimension needs, and nothing for a
/// run-scoped one.
///
/// A step that names no model is not a model. The embedded backend drops those
/// rows with `IS NOT NULL`; ClickHouse writes the same absence as an empty
/// string, so the test is against the empty string. Without it a cost
/// breakdown by model carried a `(none)` row holding every tool call in the
/// window, which is spend no model was responsible for.
///
/// A run-scoped dimension keeps its empty values and reports them as `(none)`,
/// again matching the embedded backend: a run really did happen in no named
/// environment, and dropping it would lose that run from a total the caller
/// expects to add up.
fn group_scope(col: &str, table: &str) -> String {
    if table == "traces" {
        String::new()
    } else {
        format!(" AND {col} != ''")
    }
}

impl ClickHouseStore {
    async fn grouped(
        &self,
        project: &str,
        from: OffsetDateTime,
        to: OffsetDateTime,
        dim: &str,
    ) -> Result<Vec<GroupedStat>> {
        let (col, table) = group_column(dim)?;
        #[derive(Deserialize)]
        struct Row {
            k: String,
            n: u64,
            errs: u64,
            cost: f64,
            tin: u64,
            tout: u64,
            // `duration_ns` is nullable, and `avg` over a group whose rows are
            // all null answers null, not zero. Read into `f64` that was not a
            // wrong number, it was a deserialisation failure that took the
            // whole request with it, and one group holding nothing but event
            // spans is enough to produce it. The embedded backend reports
            // absence as absence, so this does too.
            mean: Option<f64>,
            mx: Option<f64>,
        }
        let scope = group_scope(col, table);
        let rows: Vec<Row> = self
            .select(&format!(
                "SELECT coalesce(nullIf({col}, ''), '(none)') AS k, count() AS n,
                        countIf(status = 'error') AS errs,
                        sum(coalesce(total_cost, 0)) AS cost,
                        sum(coalesce(input_tokens, 0)) AS tin,
                        sum(coalesce(output_tokens, 0)) AS tout,
                        avg(duration_ns)/1e6 AS mean, max(duration_ns)/1e6 AS mx
                 FROM {} FINAL
                 WHERE project = {} AND start_time >= {} AND start_time < {}{scope}
                 GROUP BY k ORDER BY n DESC LIMIT 100",
                self.table(table),
                quote(project),
                ts(from),
                ts(to)
            ))
            .await?;
        Ok(rows
            .into_iter()
            .map(|r| GroupedStat {
                key: r.k,
                count: r.n,
                error_count: r.errs,
                total_cost: Some(r.cost),
                input_tokens: Some(r.tin),
                output_tokens: Some(r.tout),
                latency: Some(LatencyStats {
                    p50: None,
                    p90: None,
                    p95: None,
                    p99: None,
                    mean: r.mean,
                    max: r.mx,
                }),
            })
            .collect())
    }

    /// Move a `traces` table created under the old sort key on to the current
    /// one.
    ///
    /// `CREATE TABLE IF NOT EXISTS` leaves an existing table as it is, so a
    /// deployment that migrated before the key changed would keep
    /// `(project, start_time, trace_id)` for ever and keep double-counting
    /// every run whose steps arrived in two batches. ClickHouse cannot change a
    /// sort key in place, so this builds the table again under the current
    /// schema, copies every row across, and swaps the two in one statement.
    /// Rows that were duplicates under the old key share a key under the new
    /// one, and `FINAL` folds them from then on. A table already on the current
    /// key is left alone, so this costs one lookup on every start.
    /// The schema file, resolved into the statements this build would run.
    ///
    /// Shared by `migrate` and the fingerprint so the two cannot describe
    /// different things: a fingerprint taken over the file rather than over the
    /// resolved statements would match across two different retention settings
    /// and skip the migration that applies the second one.
    fn resolved_schema(&self) -> Vec<String> {
        SCHEMA
            .split(';')
            .map(str::trim)
            .filter(|sql| !sql.is_empty())
            .map(|sql| {
                // One substitution per statement, never two that can overlap.
                // The schema writes the database as `mcpd` throughout, and
                // rewriting it used to replace `mcpd.` and then
                // `IF NOT EXISTS mcpd`. Those two patterns overlap the moment
                // the configured name starts with `mcpd` and is longer, which
                // is what a test database called `mcpd_test_373` is: the first
                // pass produced `IF NOT EXISTS mcpd_test_373.spans` and the
                // second rewrote its prefix again into
                // `mcpd_test_373_test_373.spans`, so every table was created in
                // a database that does not exist. The default name is `mcpd`,
                // where both passes are a no-op, so it only ever broke for a
                // configured one.
                let sql = if sql.starts_with("CREATE DATABASE") {
                    format!("CREATE DATABASE IF NOT EXISTS {}", self.config.database)
                } else {
                    sql.replace("mcpd.", &format!("{}.", self.config.database))
                };
                // The schema carries a 90 day TTL as its written form. With a
                // retention set, rewrite the interval; with none, strip the
                // clause entirely so the table keeps everything.
                match self.config.retention_days {
                    Some(days) => sql.replace("INTERVAL 90 DAY", &format!("INTERVAL {days} DAY")),
                    None => strip_ttl(&sql),
                }
            })
            .collect()
    }

    /// A digest of the statements [`Self::resolved_schema`] would run.
    ///
    /// FNV over the joined text. Not a cryptographic hash and it does not need
    /// to be: nothing here defends against a schema chosen to collide, it only
    /// has to notice a schema that changed.
    fn schema_fingerprint(&self) -> String {
        let mut h: u128 = 0xcbf2_9ce4_8422_2325;
        for b in self.resolved_schema().join(";").bytes() {
            h = (h ^ u128::from(b)).wrapping_mul(0x0100_0000_01b3);
        }
        format!("{SCHEMA_VERSION}:{h:032x}")
    }

    /// The fingerprint already applied to this database, if any.
    ///
    /// Reads `system.tables` first rather than selecting from a table that may
    /// not exist, so the first start of an empty database takes this path
    /// without an error to swallow. Swallowing one would hide a server that is
    /// reachable but refusing.
    async fn applied_fingerprint(&self) -> Result<Option<String>> {
        #[derive(Deserialize)]
        struct Row {
            fingerprint: String,
        }
        if !self.table_exists("schema_version").await? {
            return Ok(None);
        }
        let rows: Vec<Row> = self
            .select(&format!(
                "SELECT fingerprint FROM {} ORDER BY applied_at DESC LIMIT 1",
                self.table("schema_version")
            ))
            .await?;
        Ok(rows.into_iter().next().map(|r| r.fingerprint))
    }

    /// Whether one table exists in the configured database.
    async fn table_exists(&self, name: &str) -> Result<bool> {
        let rows: Vec<CountRow> = self
            .select(&format!(
                "SELECT 1 AS n FROM system.tables WHERE database = {} AND name = {} LIMIT 1",
                quote(&self.config.database),
                quote(name)
            ))
            .await?;
        Ok(!rows.is_empty())
    }

    /// Ask the server how the signal tables are actually partitioned.
    ///
    /// The answer decides whether a retention pass may drop a partition, and
    /// getting it from the schema file instead is how a database created under
    /// month partitions has a month dropped in the belief it was a day. Every
    /// signal table has to agree: a mixed database takes the row-delete path,
    /// which is correct on both.
    async fn read_partitioning(&self) -> Result<()> {
        #[derive(Deserialize)]
        struct Named {
            name: String,
            partition_key: String,
        }
        let rows: Vec<Named> = self
            .select(&format!(
                "SELECT name, partition_key FROM system.tables WHERE database = {} \
                 AND name IN ('spans','traces','logs','metrics','scores','feedback')",
                quote(&self.config.database)
            ))
            .await?;
        // The exact expression, not merely "it mentions toYYYYMMDD". The drop
        // below recomputes the partition from the anchor it is given, and a
        // table cut on a different column would make that recomputation select
        // the wrong rows: the "does another project have rows in this day"
        // check would then answer about a day that is not the one being
        // dropped, and the drop would take another tenant's data with it.
        let want = |table: &str| match table {
            "spans" | "traces" => "toYYYYMMDD(start_time)",
            _ => "toYYYYMMDD(timestamp)",
        };
        let day = rows.len() == 6 && rows.iter().all(|r| r.partition_key == want(&r.name));
        self.day_partitioned.store(day, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }

    /// Drop every partition of `table` that lies wholly before `cutoff`.
    ///
    /// Returns how many went. Three conditions have to hold before a partition
    /// can go, and each one of them is a way to delete somebody else's data:
    ///
    /// 1. **The tables are cut by day.** `read_partitioning` asks the server,
    ///    because ClickHouse cannot change a partition key in place and a
    ///    database created under the old schema is partitioned by month. Its
    ///    partition ids are six digits, every one of them compares below an
    ///    eight-digit cutoff, and the pass would drop the whole table.
    /// 2. **The partition is wholly before the cutoff.** A partition holds one
    ///    day, so it qualifies only when its day is before the cutoff's day.
    ///    The partition holding the cutoff keeps rows on both sides of it and
    ///    is left to the row delete.
    /// 3. **No other project has rows in it.** Partitions are cut on time and
    ///    not on project, and retention is set per project. Dropping a day
    ///    because this project's window has passed would take every other
    ///    project's rows for that day with it.
    async fn drop_expired_partitions(
        &self,
        table: &str,
        anchor: &str,
        project: &str,
        cutoff: OffsetDateTime,
    ) -> Result<u32> {
        if !self.day_partitioned.load(std::sync::atomic::Ordering::Relaxed) {
            return Ok(0);
        }
        #[derive(Deserialize)]
        struct Part {
            partition_id: String,
        }
        // `toYYYYMMDD` of the cutoff, which is what a partition id is under
        // this key. Strictly less than, so the day the cutoff falls in stays.
        let day = u32::from(cutoff.year().unsigned_abs().min(9999) as u16) * 10_000
            + u32::from(u8::from(cutoff.month())) * 100
            + u32::from(cutoff.day());

        let parts: Vec<Part> = self
            .select(&format!(
                "SELECT DISTINCT partition_id FROM system.parts \
                 WHERE database = {} AND table = {} AND active",
                quote(&self.config.database),
                quote(table)
            ))
            .await?;

        let mut dropped = 0;
        for p in parts {
            let Ok(id) = p.partition_id.parse::<u32>() else { continue };
            if id >= day {
                continue;
            }
            let others: Vec<CountRow> = self
                .select(&format!(
                    "SELECT count() AS n FROM {} WHERE toYYYYMMDD({anchor}) = {id} \
                     AND project != {}",
                    self.table(table),
                    quote(project)
                ))
                .await?;
            if others.first().is_none_or(|c| c.n > 0) {
                continue;
            }
            self.execute(&format!(
                "ALTER TABLE {} DROP PARTITION ID {}",
                self.table(table),
                quote(&p.partition_id)
            ))
            .await?;
            dropped += 1;
        }
        Ok(dropped)
    }

    /// Declare a table's retention, and say whether that changed anything.
    ///
    /// A `MODIFY TTL` is metadata every replica of a Replicated table has to
    /// agree on, so issuing one on every retention tick is churn that buys
    /// nothing. The current definition is read first and the statement is only
    /// sent when it would say something different, which is what makes a second
    /// pass under the same policy report `ttl_changed: false`.
    async fn set_table_ttl(&self, table: &str, anchor: &str, days: u32) -> Result<bool> {
        #[derive(Deserialize)]
        struct Row {
            create_table_query: String,
        }
        let rows: Vec<Row> = self
            .select(&format!(
                "SELECT create_table_query FROM system.tables WHERE database = {} AND name = {}",
                quote(&self.config.database),
                quote(table)
            ))
            .await?;
        // Both renderings are accepted. The statement below says
        // `INTERVAL 7 DAY` and the server has rendered that back as
        // `toIntervalDay(7)` for several versions, so matching only the sent
        // form would find nothing and re-issue the statement on every pass,
        // which is the exact churn this function exists to avoid. Matching only
        // the server's current form would do the same on a version that renders
        // it the other way.
        let want = [
            format!("TTL toDateTime({anchor}) + toIntervalDay({days})"),
            format!("TTL toDateTime({anchor}) + INTERVAL {days} DAY"),
        ];
        if rows.first().is_some_and(|r| want.iter().any(|w| r.create_table_query.contains(w))) {
            return Ok(false);
        }
        self.execute(&format!(
            "ALTER TABLE {} MODIFY TTL toDateTime({anchor}) + INTERVAL {days} DAY",
            self.table(table)
        ))
        .await?;
        Ok(true)
    }

    async fn resort_traces(&self) -> Result<()> {
        #[derive(Deserialize)]
        struct Row {
            sorting_key: String,
        }
        let rows: Vec<Row> = self
            .select(&format!(
                "SELECT sorting_key FROM system.tables WHERE database = {} AND name = 'traces'",
                quote(&self.config.database)
            ))
            .await?;
        let Some(current) = rows.into_iter().next() else { return Ok(()) };
        if !current.sorting_key.starts_with("project, start_time") {
            return Ok(());
        }
        tracing::info!(
            database = %self.config.database,
            from = %current.sorting_key,
            "rebuilding the traces table under the current sort key"
        );
        let ddl = SCHEMA
            .split(';')
            .map(without_leading_comments)
            .find(|s| s.starts_with("CREATE TABLE IF NOT EXISTS mcpd.traces"))
            .ok_or_else(|| Error::Migration {
                version: 2,
                reason: "the schema no longer holds a traces table".to_owned(),
            })?
            .replace("mcpd.traces", &self.table("traces_resorted"));
        let fail = |e: Error| Error::Migration { version: 2, reason: e.to_string() };
        self.execute(&format!("DROP TABLE IF EXISTS {}", self.table("traces_resorted")))
            .await
            .map_err(fail)?;
        self.execute(&ddl).await.map_err(fail)?;
        self.execute(&format!(
            "INSERT INTO {} SELECT * FROM {}",
            self.table("traces_resorted"),
            self.table("traces")
        ))
        .await
        .map_err(fail)?;
        // One statement, so there is no moment with no traces table.
        self.execute(&format!(
            "EXCHANGE TABLES {} AND {}",
            self.table("traces"),
            self.table("traces_resorted")
        ))
        .await
        .map_err(fail)?;
        self.execute(&format!("DROP TABLE {}", self.table("traces_resorted"))).await.map_err(fail)
    }

    /// Recompute the trace projection for the given runs.
    ///
    /// Same reasoning as the embedded backend: spans arrive out of order, get
    /// retried and get updated, so an incremental counter drifts under all
    /// three. `ReplacingMergeTree` keyed on a version column lets the newest
    /// projection win without a delete.
    async fn rebuild_traces(&self, trace_ids: &[String]) -> Result<()> {
        let list = trace_ids.iter().map(|t| quote(t)).collect::<Vec<_>>().join(",");
        let version = OffsetDateTime::now_utc().unix_timestamp_nanos().max(0) as u64;

        // Two ClickHouse rules shape this query, and both were found by running
        // it rather than by reading about it:
        //
        // 1. An aggregate may not appear inside another aggregate, and
        //    `if(countIf(...) > 0, ..., argMin(...))` counts as exactly that.
        //    So the counts are computed in an inner query and the status is
        //    derived outside it.
        // 2. Aliasing an aggregate to the name of the column it reads
        //    (`min(start_time) AS start_time`) makes the next reference
        //    resolve to the alias, which is then also read as nesting. Every
        //    alias below is deliberately a different name from its source.
        self.execute(&format!(
            "INSERT INTO {traces}
             SELECT
               trace_id, root_span_id, nm, knd,
               if(err > 0, 'error', root_status) AS status,
               st, et, dur, sp, err, tin, tout, cost, 'USD',
               mc, tc, tg, prj, env, svc, sid, thr, uid, exp, rel,
               '', '', toNullable(toFloat64(0)), complete, rcv, version
             FROM (
               SELECT
                 trace_id,
                 project AS prj,
                 argMin(span_id, if(is_root, 0, 1)) AS root_span_id,
                 argMin(name, if(is_root, 0, 1)) AS nm,
                 argMin(kind, if(is_root, 0, 1)) AS knd,
                 argMin(status, if(is_root, 0, 1)) AS root_status,
                 min(start_time) AS st,
                 max(end_time) AS et,
                 toNullable(toUInt64(greatest(0, dateDiff('nanosecond', min(start_time), max(end_time))))) AS dur,
                 toUInt32(count()) AS sp,
                 toUInt32(countIf(status = 'error')) AS err,
                 sum(coalesce(input_tokens, 0)) AS tin,
                 sum(coalesce(output_tokens, 0)) AS tout,
                 sum(coalesce(total_cost, 0)) AS cost,
                 toUInt32(countIf(kind IN ('model_generation','model_inference'))) AS mc,
                 toUInt32(countIf(kind IN ('tool_call','mcp_tool_call','client_tool_call','provider_tool_call'))) AS tc,
                 argMin(tags, if(is_root, 0, 1)) AS tg,
                 any(environment) AS env,
                 any(service_name) AS svc,
                 any(session_id) AS sid,
                 any(thread_id) AS thr,
                 any(user_id) AS uid,
                 any(experiment_id) AS exp,
                 any(release) AS rel,
                 -- `is_complete` is specified as the root having ended and
                 -- no span still open. The second half alone called a run of
                 -- nothing but event spans complete, because an event carries
                 -- no end time and is excluded from the open count, so a run
                 -- that had not started yet was reported as finished. The
                 -- embedded backend requires an end time as well, so this does.
                 toUInt8(countIf(end_time IS NULL AND is_event = 0) = 0
                         AND max(end_time) IS NOT NULL) AS complete,
                 max(received_at) AS rcv,
                 {version} AS version
               FROM {spans} FINAL
               WHERE trace_id IN ({list})
               -- One row per project and trace id. A trace id is shared across
               -- a service boundary by design, and grouping on the id alone
               -- filed both projects' spans under whichever project `any`
               -- happened to pick.
               GROUP BY project, trace_id
             )",
            traces = self.table("traces"),
            spans = self.table("spans"),
        ))
        .await
    }
}

// Row shapes for the remaining signals.

#[derive(Debug, Deserialize)]
struct TraceRow {
    trace_id: String,
    root_span_id: String,
    name: String,
    kind: String,
    status: String,
    start_time: String,
    end_time: Option<String>,
    duration_ns: Option<u64>,
    span_count: u32,
    error_count: u32,
    input_tokens: u64,
    output_tokens: u64,
    total_cost: f64,
    currency: String,
    model_count: u32,
    tool_count: u32,
    tags: Vec<String>,
    project: String,
    environment: String,
    service_name: String,
    session_id: String,
    thread_id: String,
    user_id: String,
    experiment_id: String,
    release: String,
    score_summary: String,
    feedback_summary: String,
    is_complete: u8,
    received_at: String,
}

impl From<TraceRow> for Trace {
    fn from(r: TraceRow) -> Self {
        Self {
            trace_id: r.trace_id,
            root_span_id: opt(r.root_span_id),
            name: r.name,
            kind: SpanKind::from_str(&r.kind).unwrap_or(SpanKind::Generic),
            status: SpanStatus::from_str(&r.status).unwrap_or(SpanStatus::Unset),
            start_time: parse_ts(&r.start_time),
            end_time: r.end_time.as_deref().map(parse_ts),
            duration_ns: r.duration_ns,
            span_count: r.span_count,
            error_count: r.error_count,
            usage: (r.input_tokens > 0 || r.output_tokens > 0).then_some(TokenUsage {
                input_tokens: Some(r.input_tokens),
                output_tokens: Some(r.output_tokens),
                total_tokens: None,
                input_details: None,
                output_details: None,
            }),
            cost: (r.total_cost > 0.0).then_some(Cost {
                currency: r.currency,
                input_cost: None,
                output_cost: None,
                total_cost: Some(r.total_cost),
                price_book: None,
                is_estimated: true,
            }),
            model_count: r.model_count,
            tool_count: r.tool_count,
            tags: (!r.tags.is_empty()).then_some(r.tags),
            correlation: Correlation {
                project: r.project,
                environment: opt(r.environment),
                service_name: opt(r.service_name),
                service_version: None,
                session_id: opt(r.session_id),
                thread_id: opt(r.thread_id),
                user_id: opt(r.user_id),
                resource_id: None,
                run_id: None,
                request_id: None,
                experiment_id: opt(r.experiment_id),
                release: opt(r.release),
            },
            score_summary: parse_json(&r.score_summary),
            feedback_summary: parse_json(&r.feedback_summary),
            is_complete: r.is_complete != 0,
            received_at: parse_ts(&r.received_at),
        }
    }
}

#[derive(Debug, Deserialize)]
struct LogRow {
    log_id: String,
    trace_id: String,
    span_id: String,
    timestamp: String,
    level: String,
    message: String,
    fields: String,
    logger: String,
    project: String,
    environment: String,
    service_name: String,
    session_id: String,
    user_id: String,
    received_at: String,
}

impl From<LogRow> for LogRecord {
    fn from(r: LogRow) -> Self {
        Self {
            log_id: r.log_id,
            trace_id: opt(r.trace_id),
            span_id: opt(r.span_id),
            timestamp: parse_ts(&r.timestamp),
            level: LogLevel::from_str(&r.level).unwrap_or(LogLevel::Info),
            message: r.message,
            fields: parse_json(&r.fields),
            logger: opt(r.logger),
            correlation: Correlation {
                project: r.project,
                environment: opt(r.environment),
                service_name: opt(r.service_name),
                service_version: None,
                session_id: opt(r.session_id),
                thread_id: None,
                user_id: opt(r.user_id),
                resource_id: None,
                run_id: None,
                request_id: None,
                experiment_id: None,
                release: None,
            },
            received_at: parse_ts(&r.received_at),
        }
    }
}

#[derive(Debug, Deserialize)]
struct ScoreRow {
    score_id: String,
    trace_id: String,
    span_id: String,
    scorer_id: String,
    scorer_version: String,
    source: String,
    value: f64,
    min_value: Option<f64>,
    max_value: Option<f64>,
    label: String,
    reason: String,
    score_trace_id: String,
    cost_json: String,
    metadata: String,
    timestamp: String,
    project: String,
    environment: String,
    experiment_id: String,
}

impl From<ScoreRow> for Score {
    fn from(r: ScoreRow) -> Self {
        Self {
            score_id: r.score_id,
            trace_id: opt(r.trace_id),
            span_id: opt(r.span_id),
            scorer_id: r.scorer_id,
            scorer_version: opt(r.scorer_version),
            source: ScoreSource::from_str(&r.source).unwrap_or(ScoreSource::Manual),
            value: r.value,
            min: r.min_value,
            max: r.max_value,
            label: opt(r.label),
            reason: opt(r.reason),
            score_trace_id: opt(r.score_trace_id),
            cost: parse_json(&r.cost_json).and_then(|v| serde_json::from_value(v).ok()),
            metadata: parse_json(&r.metadata),
            timestamp: parse_ts(&r.timestamp),
            correlation: Correlation {
                project: r.project,
                environment: opt(r.environment),
                service_name: None,
                service_version: None,
                session_id: None,
                thread_id: None,
                user_id: None,
                resource_id: None,
                run_id: None,
                request_id: None,
                experiment_id: opt(r.experiment_id),
                release: None,
            },
        }
    }
}

#[derive(Debug, Deserialize)]
struct FeedbackRow {
    feedback_id: String,
    trace_id: String,
    span_id: String,
    kind: String,
    value: String,
    comment: String,
    author_id: String,
    author_kind: String,
    timestamp: String,
    metadata: String,
    project: String,
    environment: String,
}

impl From<FeedbackRow> for Feedback {
    fn from(r: FeedbackRow) -> Self {
        Self {
            feedback_id: r.feedback_id,
            trace_id: opt(r.trace_id),
            span_id: opt(r.span_id),
            kind: FeedbackKind::from_str(&r.kind).unwrap_or(FeedbackKind::Comment),
            value: parse_json(&r.value).unwrap_or(Value::Null),
            comment: opt(r.comment),
            author_id: opt(r.author_id),
            author_kind: opt(r.author_kind),
            timestamp: parse_ts(&r.timestamp),
            metadata: parse_json(&r.metadata),
            correlation: Correlation {
                project: r.project,
                environment: opt(r.environment),
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
        }
    }
}

/// Render the SQL a trace list would run, without executing it.
///
/// Exists so the generated SQL can be checked against a real ClickHouse in CI
/// rather than only asserted against in a unit test: a query that parses in a
/// string comparison and not in the engine is not verified.
#[must_use]
pub fn debug_trace_sql(q: &TraceQuery, spans_table: &str, traces_table: &str) -> String {
    format!(
        "SELECT * FROM {traces_table} FINAL WHERE {} ORDER BY {} LIMIT {}",
        trace_where(q, spans_table),
        order_by(q.sort, q.order),
        q.limit.clamp(1, 500)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting_closes_every_injection_route() {
        // ClickHouse's HTTP interface has no bind parameters, so this is the
        // only thing between a filter value and the query.
        assert_eq!(quote("plain"), "'plain'");
        assert_eq!(quote("it's"), "'it\\'s'");
        assert_eq!(quote("back\\slash"), "'back\\\\slash'");
        assert!(quote("line\nbreak").contains("\\x0a"));
        // The property that matters: the value is fully enclosed, and every
        // quote inside it is backslash-escaped, so it cannot terminate the
        // literal early. Checking for the absence of a substring is the wrong
        // test, because the correctly escaped form still contains one.
        let hostile = quote("' OR 1=1 --");
        assert_eq!(hostile, "'\\' OR 1=1 --'");
        let inner = &hostile[1..hostile.len() - 1];
        for (i, c) in inner.char_indices() {
            if c == '\'' {
                assert!(
                    i > 0 && inner.as_bytes()[i - 1] == b'\\',
                    "an unescaped quote at {i} would end the literal: {hostile}"
                );
            }
        }
    }

    #[test]
    fn group_by_is_an_allowlist() {
        assert!(group_column("model").is_ok());
        assert!(group_column("1) OR 1=1--").is_err());
        assert!(group_column("password").is_err());
    }

    #[test]
    fn every_dimension_is_read_from_the_table_that_makes_its_count_mean_runs() {
        // All nine, not one. The bug this replaces was that the three that
        // happened to be right were the three anybody checked.
        let expected = [
            ("model", ("model", "spans")),
            ("provider", ("provider", "spans")),
            ("entity_name", ("entity_name", "spans")),
            ("environment", ("environment", "traces")),
            ("status", ("status", "traces")),
            ("session_id", ("session_id", "traces")),
            ("user_id", ("user_id", "traces")),
            ("service_name", ("service_name", "traces")),
            ("name", ("name", "traces")),
        ];
        for (dim, want) in expected {
            assert_eq!(group_column(dim).expect("an allowed dimension"), want, "dimension {dim}");
        }
    }

    #[test]
    fn only_a_span_dimension_drops_the_rows_that_do_not_name_one() {
        // A run with no environment is still a run and still holds spend. A
        // step with no model is not a model, and counting it as `(none)` put
        // every tool call in the window into a cost-by-model breakdown.
        assert_eq!(group_scope("environment", "traces"), "");
        assert_eq!(group_scope("model", "spans"), " AND model != \'\'");
    }

    #[test]
    fn a_trace_filter_renders_every_predicate() {
        let q = TraceQuery {
            project: "default".into(),
            status: Some(SpanStatus::Error),
            model: Some("gpt-4o".into()),
            tags: vec!["support".into()],
            min_duration_ms: Some(1000),
            q: Some("leeds depot".into()),
            ..Default::default()
        };
        let sql = trace_where(&q, "mcpd.spans");
        assert!(sql.contains("project = 'default'"));
        assert!(sql.contains("status = 'error'"));
        assert!(sql.contains("has(tags, 'support')"));
        assert!(sql.contains("duration_ns >= 1000000000"));
        assert!(sql.contains("model = 'gpt-4o'"));
        // Two search terms, ANDed, each checked against three columns. Both
        // are single tokens, so all six are the word test rather than the
        // substring fallback.
        assert_eq!(sql.matches("hasToken(lowerUTF8(").count(), 6);
        assert!(!sql.contains("positionCaseInsensitive"), "{sql}");
    }

    #[test]
    fn a_word_search_uses_the_token_test_and_a_term_with_a_separator_does_not() {
        // The fallback is not a preference, it is the only thing ClickHouse
        // will answer: `hasToken` raises BAD_ARGUMENTS on a needle holding a
        // separator, which would have been a 500 for anyone searching a name
        // like `cache_miss`.
        assert_eq!(text_match("input", "Leeds"), "hasToken(lowerUTF8(input), lowerUTF8('Leeds'))");
        assert_eq!(text_match("input", "café"), "hasToken(lowerUTF8(input), lowerUTF8('café'))");
        assert_eq!(
            text_match("name", "cache_miss"),
            "positionCaseInsensitiveUTF8(name, 'cache_miss') > 0"
        );
        assert_eq!(
            text_match("name", "mcpd.spans;"),
            "positionCaseInsensitiveUTF8(name, 'mcpd.spans;') > 0"
        );
        assert_eq!(
            text_match_any(&["input", "name"], "Leeds"),
            "(hasToken(lowerUTF8(input), lowerUTF8('Leeds')) OR \
             hasToken(lowerUTF8(name), lowerUTF8('Leeds')))"
        );
    }

    #[test]
    fn a_hostile_search_term_cannot_escape_its_quotes() {
        let q = TraceQuery {
            project: "default".into(),
            q: Some("'; DROP TABLE mcpd.spans; --".into()),
            ..Default::default()
        };
        let sql = trace_where(&q, "mcpd.spans");
        assert!(!sql.contains("DROP TABLE mcpd.spans;"), "{sql}");
        assert!(sql.contains("\\'"));
    }

    #[test]
    fn timestamps_round_trip_through_the_wire_format() {
        let t = OffsetDateTime::from_unix_timestamp_nanos(1_786_000_000_123_456_789).unwrap();
        let rendered = ts_str(t);
        assert_eq!(rendered, "2026-08-06 07:06:40.123456789");
        // The round trip is the real property; the literal above just pins the
        // wire format so a change to it is deliberate.
        assert_eq!(parse_ts(&rendered), t);
    }

    #[test]
    fn ordering_maps_every_sort_key() {
        assert!(order_by(TraceSort::Cost, Order::Desc).starts_with("total_cost DESC"));
        assert!(order_by(TraceSort::Duration, Order::Asc).starts_with("duration_ns ASC"));
        assert!(order_by(TraceSort::SpanCount, Order::Desc).starts_with("span_count DESC"));
        assert!(order_by(TraceSort::StartTime, Order::Asc).starts_with("start_time ASC"));
        // An unscored run has a NULL here, and ClickHouse sorts a NULL last
        // whichever way it is going. ifNull is what makes it rank as a zero, as
        // it does in the other two backends.
        assert!(order_by(TraceSort::Score, Order::Desc).starts_with("ifNull(min_score, 0) DESC"));
    }

    #[test]
    fn the_span_ordering_maps_every_sort_key_and_breaks_ties_on_the_span_id() {
        assert_eq!(
            span_order_by(SpanSort::StartTime, Order::Desc),
            "start_time DESC, span_id DESC"
        );
        assert_eq!(
            span_order_by(SpanSort::Duration, Order::Asc),
            "ifNull(duration_ns, 0) ASC, span_id ASC"
        );
        assert_eq!(
            span_order_by(SpanSort::Cost, Order::Desc),
            "ifNull(total_cost, 0) DESC, span_id DESC"
        );
    }
    /// The probe's statements are formatted from four table names and a
    /// branch, and nothing in the type system says the result parses. This
    /// pins the parts that decide what the probe means; that they run is
    /// proved against a server.
    #[test]
    fn the_probe_reads_only_the_other_form_of_identifier() {
        let table = |t: &str| format!("mcpd.{t}");
        let [attributed, opaque, candidates] =
            probe_statements(&table, "p", "sha256:000000000000000000000000", true);
        for (t, c) in ATTRIBUTING_COLUMNS {
            assert!(attributed.contains(&format!("FROM mcpd.{t}")), "{t} is unseen: {attributed}");
            assert!(
                candidates.contains(&format!("SELECT {c} AS id FROM mcpd.{t}")),
                "{candidates}"
            );
        }
        // Every arm stops at its first row.
        assert_eq!(attributed.matches("LIMIT 1").count(), ATTRIBUTING_COLUMNS.len());
        assert!(
            opaque.contains("<> 'sha256:000000000000000000000000'"),
            "the subject is not excluded from its own probe: {opaque}"
        );
        // `FINAL` where the table is replacing, and nowhere else. Without it an
        // un-merged older version of a row answers for the current one.
        assert!(attributed.contains("FROM mcpd.traces FINAL"), "{attributed}");
        assert!(attributed.contains("FROM mcpd.logs WHERE"), "logs is not replacing: {attributed}");
        // A hashed subject cannot address a plaintext row, so those are the
        // candidates. With a plaintext subject it is the other way round.
        let shape = crate::hashed_id_regex();
        assert_eq!(shape, "^sha256:[0-9a-f]{24}$");
        assert!(candidates.contains(&format!("NOT match(user_id, '{shape}')")), "{candidates}");
        let [_, _, candidates] = probe_statements(&table, "p", "user-ann", false);
        assert!(candidates.contains(&format!("AND match(user_id, '{shape}')")), "{candidates}");
        // The whole shape, never the bare prefix. A prefix test drops a
        // caller's own digest out of the candidates, and the rule counts that
        // digest as the other form, so the count that exists to report an
        // unreachable row would never see it.
        assert!(!candidates.contains("LIKE"), "the probe narrowed on a prefix: {candidates}");
        // The subject reaches the statement quoted, like every other value.
        let [_, opaque, _] = probe_statements(&table, "p", "sha256:'; DROP", true);
        assert!(opaque.contains(r"\'; DROP"), "the subject reached the SQL unquoted: {opaque}");
    }
}

#[cfg(test)]
mod ttl_tests {
    use super::*;

    fn store_for(database: &str, retention_days: Option<u32>) -> ClickHouseStore {
        ClickHouseStore::new(ClickHouseConfig {
            database: database.to_owned(),
            retention_days,
            ..Default::default()
        })
        .expect("build a store")
    }

    #[test]
    fn every_signal_table_is_cut_one_partition_per_day() {
        // A month partition cannot be dropped for a window shorter than a
        // month, which is every window anybody sets, so the retention pass
        // falls back to reading and rewriting every row it removes. Proved
        // against a server too, in tests/clickhouse.rs; this catches the
        // regression without one.
        for table in ["spans", "traces", "logs", "metrics", "scores", "feedback", "audit_log"] {
            let ddl = SCHEMA
                .split(';')
                .find(|s| s.contains(&format!("mcpd.{table} (")))
                .unwrap_or_else(|| panic!("the schema no longer declares {table}"));
            assert!(ddl.contains("PARTITION BY toYYYYMMDD("), "{table} is not partitioned by day");
        }
    }

    #[test]
    fn every_correlation_identifier_carries_a_skipping_index() {
        // Without one, a filter by an identifier the caller already holds reads
        // every granule in the window.
        let spans = SCHEMA.split(';').find(|s| s.contains("mcpd.spans (")).expect("the spans ddl");
        for column in [
            "trace_id",
            "session_id",
            "thread_id",
            "user_id",
            "resource_id",
            "run_id",
            "request_id",
            "experiment_id",
            "release",
            "environment",
            "service_name",
        ] {
            assert!(
                spans.lines().any(|l| {
                    let l = l.trim();
                    l.starts_with("INDEX ") && l.split_whitespace().nth(2) == Some(column)
                }),
                "spans.{column} carries no skipping index"
            );
        }
    }

    #[test]
    fn the_fingerprint_moves_when_the_statements_do_and_not_otherwise() {
        // The fast path in `migrate` skips every statement when this matches, so
        // a fingerprint that ignored the retention rewriting would skip the
        // migration that applies a changed `--retention-days`.
        let a = store_for("mcpd", Some(30));
        assert_eq!(a.schema_fingerprint(), store_for("mcpd", Some(30)).schema_fingerprint());
        assert_ne!(a.schema_fingerprint(), store_for("mcpd", Some(31)).schema_fingerprint());
        assert_ne!(a.schema_fingerprint(), store_for("mcpd", None).schema_fingerprint());
        // The database name is rewritten into every statement, so two databases
        // are two different schemas as far as this is concerned. They are
        // separate databases with separate `schema_version` tables, so this
        // only ever matters if the name changes under one.
        assert_ne!(a.schema_fingerprint(), store_for("mcpd_other", Some(30)).schema_fingerprint());
    }

    #[test]
    fn a_watermark_literal_keeps_every_nanosecond() {
        // `ts` renders through an f64, which holds about sixteen significant
        // digits where a nanosecond timestamp needs nineteen. That is close
        // enough for a window a person chose and not close enough for a poll
        // cursor: a comparison off by a few nanoseconds either replays the last
        // row of every poll or skips it.
        let t = from_nanos(1_700_000_000_123_456_789);
        assert_eq!(ts_exact(t), "toDateTime64('2023-11-14 22:13:20.123456789', 9, 'UTC')");
        assert_ne!(ts_exact(t), ts(t), "the exact form is the one that keeps the digits");
        assert_eq!(to_nanos(t), 1_700_000_000_123_456_789);
    }

    /// `--retention-days` says "omit to keep everything" and the privacy page
    /// promises nothing is ever deleted. ClickHouse used to apply ninety days
    /// regardless, so a deployment that never asked for retention lost a
    /// quarter of its history without being told.
    #[test]
    fn no_retention_strips_the_ttl_clause_entirely() {
        let ddl = "CREATE TABLE IF NOT EXISTS mcpd.spans (\n  trace_id String\n)\nENGINE = MergeTree\nORDER BY (project, start_time)\nTTL toDateTime(received_at) + INTERVAL 90 DAY;";
        let out = strip_ttl(ddl);
        assert!(!out.contains("TTL"), "the clause must be gone: {out}");
        assert!(out.trim_end().ends_with(';'), "the statement must stay terminated: {out}");
        assert!(out.contains("ORDER BY (project, start_time)"), "the rest must survive: {out}");
    }

    /// The resort migration rewrites the traces DDL from the schema file, and
    /// that DDL sits behind a comment explaining its sort key. Losing it here
    /// is a migration that refuses to run on every deployment that needs it.
    #[test]
    fn the_traces_ddl_is_found_behind_the_comment_that_introduces_it() {
        let ddl = SCHEMA
            .split(';')
            .map(without_leading_comments)
            .find(|s| s.starts_with("CREATE TABLE IF NOT EXISTS mcpd.traces"))
            .expect("the traces table is in the schema");
        assert!(ddl.contains("ORDER BY (project, trace_id)"), "{ddl}");
        assert!(!ddl.contains("--"), "a comment survived: {ddl}");
        assert_eq!(without_leading_comments("-- a\n-- b\nSELECT 1"), "SELECT 1");
        assert_eq!(without_leading_comments("SELECT 1"), "SELECT 1");
    }

    #[test]
    fn a_statement_with_no_ttl_is_unchanged() {
        let ddl =
            "CREATE TABLE IF NOT EXISTS mcpd.datasets (id String) ENGINE = MergeTree ORDER BY id;";
        assert_eq!(strip_ttl(ddl), ddl);
    }

    /// And with a retention set, the interval is the configured one rather than
    /// the ninety the schema file happens to be written with.
    #[test]
    fn a_configured_retention_replaces_the_written_interval() {
        let ddl = "TTL toDateTime(received_at) + INTERVAL 90 DAY;";
        assert_eq!(
            ddl.replace("INTERVAL 90 DAY", "INTERVAL 7 DAY"),
            "TTL toDateTime(received_at) + INTERVAL 7 DAY;"
        );
    }
}
