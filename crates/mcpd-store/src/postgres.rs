//! The Postgres backend.
//!
//! The third store, and the one most people already run. It sits between the
//! embedded file and ClickHouse: it removes the single-writer and single-replica
//! limits that bound SQLite, without asking a team to operate a database they do
//! not have. It will not match ClickHouse on a billion spans, and this file does
//! not pretend otherwise.
//!
//! Two things are worth knowing about the SQL here.
//!
//! Every value reaches Postgres as a bind parameter. That is not a stylistic
//! preference: it is the difference between this file and `clickhouse.rs`, whose
//! HTTP interface has no bind parameters at all and so has to quote by hand.
//! Nothing user-supplied is ever formatted into a statement.
//!
//! Timestamps are `BIGINT` nanoseconds, exactly as in the embedded schema, and
//! not `timestamptz`. Postgres timestamps hold microseconds, so storing them
//! natively would silently round every span's start time and make two spans a
//! few hundred nanoseconds apart sort arbitrarily.

use crate::{
    decode_delta_cursor, encode_delta_cursor, hashed_id_regex, is_hashed_identifier,
    unmatchable_identifiers, Aggregation, AspectNoteQuery, AspectNoteRollup, AspectRollupQuery,
    AuditQuery, Batch, Delta, DeltaQuery, DeltaSignal, Discovered, DiscoveryQuery, DocKind,
    DocumentLabel, ErasureOutcome, Error, IdentifierProbe, Include, LogQuery, MetricQuery,
    NewDocument, Order, Paged, Partitioning, PolledSignal, Result, RetentionOutcome,
    RetentionPolicy, ScoreQuery, SnapshotQuery, SnapshotWrite, SpanQuery, SpanSort, Store,
    StoreCapabilities, StoredDocument, StoredNote, StoredTheme, TextSearch, ThemeLineage,
    ThemeMember, ThemeMemberQuery, ThemeQuery, TraceQuery, TraceSort, ERASED_ITEM,
};
use deadpool_postgres::{Manager, ManagerConfig, Pool, RecyclingMethod, Runtime};
use mcpd_core::{
    AnalyticsSummary, AspectNote, AuditEvent, Correlation, Cost, CostBreakdown, CostBucket,
    Dataset, DatasetItem, EntityKind, ErasureResult, Experiment, Feedback, FeedbackKind,
    GroupedStat, IngestResult, LatencyStats, LogLevel, LogRecord, MetricSeries, MetricSeriesPoint,
    MetricSeriesResponse, Score, ScoreSource, Span, SpanError, SpanKind, SpanLink, SpanStatus,
    Theme, ThemeSnapshot, ThemeState, TokenUsage, Trace, TraceDetail,
};
use serde_json::Value;
use std::str::FromStr;
use time::OffsetDateTime;
use tokio_postgres::config::SslMode;
use tokio_postgres::types::ToSql;
use tokio_postgres::{NoTls, Row};

/// How to reach the server.
#[derive(Debug, Clone)]
pub struct PostgresConfig {
    /// A libpq connection string or URL, such as
    /// `postgres://mcpd:secret@db:5432/mcpd`.
    pub url: String,
    /// Most connections to hold open.
    pub pool_size: usize,
}

impl Default for PostgresConfig {
    fn default() -> Self {
        // Sixteen is enough to keep an ingest burst and a dashboard from
        // queueing behind each other, and small enough not to exhaust a managed
        // instance's connection limit when mcpd runs as several replicas.
        Self { url: "postgres://localhost/mcpd".to_owned(), pool_size: 16 }
    }
}

/// How long a caller waits for a pooled connection, or for a new one to open.
const POOL_WAIT: std::time::Duration = std::time::Duration::from_secs(10);

/// What the connection string asks of TLS, beyond what the driver reads.
#[derive(Debug, Default, PartialEq, Eq)]
struct TlsSettings {
    /// A PEM file of the certificates to trust, from `sslrootcert`.
    root_cert: Option<String>,
}

impl TlsSettings {
    /// The client configuration TLS connections use.
    ///
    /// The server certificate is always verified. libpq's `require` encrypts
    /// without checking who answered, which protects against a passive reader
    /// and not against anyone in the path; a daemon holding every prompt its
    /// agents sent is the wrong place to accept that. With `sslrootcert` the
    /// file is the whole trust store, as it is for libpq, which is what a
    /// managed service with its own certificate authority needs. Without it,
    /// the public roots compiled into the binary are used.
    fn client_config(&self) -> Result<rustls::ClientConfig> {
        use rustls::pki_types::{pem::PemObject, CertificateDer};
        let mut roots = rustls::RootCertStore::empty();
        match &self.root_cert {
            Some(path) => {
                let certs = CertificateDer::pem_file_iter(path)
                    .and_then(|it| it.collect::<std::result::Result<Vec<_>, _>>())
                    .map_err(|e| {
                        Error::Backend(format!(
                            "postgres: sslrootcert {path} could not be read as PEM \
                             certificates: {e}"
                        ))
                    })?;
                let (added, _) = roots.add_parsable_certificates(certs);
                if added == 0 {
                    return Err(Error::Backend(format!(
                        "postgres: sslrootcert {path} holds no certificate this build can use; \
                         give the PEM file of the certificate authority that signed the server"
                    )));
                }
            }
            None => roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned()),
        }
        let provider = std::sync::Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        Ok(rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|e| Error::Backend(format!("postgres: TLS is unavailable: {e}")))?
            .with_root_certificates(roots)
            .with_no_client_auth())
    }
}

/// Take the TLS settings the driver does not read out of a connection string.
///
/// `sslrootcert` is the libpq name for a trust store, and every managed
/// service's instructions use it, but the driver refuses any option it does
/// not know. `verify-ca` and `verify-full` are libpq's modes for a checked
/// certificate; the driver knows only `require`, and `require` here is always
/// checked, so both become it. Both forms of the string are handled: a URL with
/// a query, and `key=value` pairs separated by spaces.
fn split_tls(url: &str) -> Result<(String, TlsSettings)> {
    let mut tls = TlsSettings::default();
    let mut take = |key: &str, value: &str| -> Option<String> {
        match key {
            "sslrootcert" => {
                tls.root_cert = Some(value.to_owned());
                None
            }
            "sslmode" if matches!(value, "verify-ca" | "verify-full") => {
                Some(format!("{key}=require"))
            }
            _ => Some(format!("{key}={value}")),
        }
    };
    let rest = if url.starts_with("postgres://") || url.starts_with("postgresql://") {
        match url.split_once('?') {
            Some((base, query)) => {
                // Only the trust store's path is decoded, because it leaves the
                // string and is opened as a file. Every other value goes back in
                // as written, and the driver decodes it. Decoding it here as
                // well turned `password=a%26b` into a second `&` that cut the
                // password short, and `%2541` into `A` rather than `%41`.
                let kept: Vec<String> = query
                    .split('&')
                    .filter(|p| !p.is_empty())
                    .filter_map(|p| match p.split_once('=') {
                        Some((k @ "sslrootcert", v)) => take(k, &percent_decode(v)),
                        Some((k, v)) => take(k, v),
                        None => Some(p.to_owned()),
                    })
                    .collect();
                if kept.is_empty() {
                    base.to_owned()
                } else {
                    format!("{base}?{}", kept.join("&"))
                }
            }
            None => url.to_owned(),
        }
    } else {
        url.split_whitespace()
            .filter_map(|p| match p.split_once('=') {
                Some((k, v)) => take(k, v),
                None => Some(p.to_owned()),
            })
            .collect::<Vec<_>>()
            .join(" ")
    };
    Ok((rest, tls))
}

/// Decode the `%XX` escapes a URL query value may carry, such as `%2F` in a
/// path. Anything that is not a valid escape is kept as written.
fn percent_decode(v: &str) -> String {
    let bytes = v.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        // Read from the bytes, never by slicing the string: an escape cut off
        // next to a multi-byte character would put the slice inside it.
        let escaped = (bytes[i] == b'%' && i + 2 < bytes.len())
            .then(|| std::str::from_utf8(&bytes[i + 1..i + 3]).ok())
            .flatten()
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        if let Some(b) = escaped {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The advisory-lock key migration takes.
///
/// An arbitrary constant, but a fixed one: every mcpd process has to choose the
/// same number or the lock protects nothing. It is namespaced by being large and
/// specific rather than by a scheme Postgres does not have.
const MIGRATION_LOCK: i64 = 0x6d63_7064_0001;

/// The schema version this build writes.
///
/// The fast path in `migrate` skips the DDL entirely when the recorded version
/// matches, so adding a table to `postgres_schema.sql` without bumping this
/// would leave every existing database without it.
///
/// 3 adds `discovery` and the partial indexes on every correlation identifier.
///
/// 4 adds `aspect_notes`, `theme_snapshots`, `themes` and `theme_members`. All
/// four are new tables declared `IF NOT EXISTS`, so an existing database gains
/// them on the next start with nothing to apply by hand, and gains no rows: a
/// deployment that writes no notes carries four empty tables.
///
/// 5 keys `traces` by project and trace id together. It is the first step that
/// `CREATE TABLE IF NOT EXISTS` cannot apply, so `migrate` drops the old table
/// inside the migrating transaction and rebuilds every row from the spans. That
/// is safe only because every column of `traces` is derived.
///
/// 7 adds `documents` and `document_labels`, two new tables.
///
/// 8 adds `leases`, one new table.
const SCHEMA_VERSION: i32 = 8;

/// The first version whose `traces` table is keyed by project as well.
const TRACES_BY_PROJECT: i32 = 5;

/// The aggregates of one run's projection over its spans.
///
/// One text for the per-run recompute on every write and the set-based
/// rebuild in `migrate`, so the two cannot count differently.
const TRACE_TOTALS: &str = "
    COUNT(*)::bigint,
    COUNT(*) FILTER (WHERE status = 'error')::bigint,
    MIN(start_time), MAX(end_time),
    COALESCE(SUM(input_tokens), 0)::bigint,
    COALESCE(SUM(output_tokens), 0)::bigint,
    COALESCE(SUM(total_cost), 0)::double precision,
    COUNT(*) FILTER (WHERE kind IN ('model_generation','model_inference'))::bigint,
    COUNT(*) FILTER (WHERE kind IN (
        'tool_call','mcp_tool_call','client_tool_call','provider_tool_call'))::bigint,
    COUNT(*) FILTER (WHERE end_time IS NULL AND NOT is_event)::bigint";

/// Which span a run takes its name and identity from: the real root first,
/// then the earliest span. The span id settles a tie, which two spans that
/// started in the same nanosecond produce. Without it the choice was whatever
/// order the server read the rows in, and a rebuild could name a run
/// differently from the write that first built it.
const ROOT_ORDER: &str = "is_root DESC, start_time ASC, span_id ASC";

/// `avg_value` as the JSON number `serde_json` writes for the same `f64`.
///
/// The rebuild writes the score summary in SQL, and `recompute_scores` writes
/// it from Rust, so the two must read back as the same value. Postgres writes
/// an integral double as `1`, which reads back as an integer; `serde_json`
/// writes `1.0`, which reads back as a float. And `serde_json` writes NaN or an
/// infinity, which JSON cannot hold, as `null`. The digits are exact only with
/// `extra_float_digits` above zero, which the rebuild sets for its transaction.
const SCORE_JSON_NUMBER: &str = "
    CASE WHEN avg_value IN ('NaN'::float8, 'Infinity'::float8, '-Infinity'::float8) THEN 'null'
         WHEN avg_value::text ~ '[.e]' THEN avg_value::text
         ELSE avg_value::text || '.0' END";

/// A store backed by Postgres.
pub struct PostgresStore {
    pool: Pool,
}

impl std::fmt::Debug for PostgresStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PostgresStore").finish_non_exhaustive()
    }
}

// --------------------------------------------------------------- helpers ---

/// Wrap a driver error, keeping what the server actually said.
///
/// `tokio_postgres::Error` displays as "db error" and hides the real message,
/// the constraint name and the position in its source. Reporting only the outer
/// text turns every failure into the same useless line, so the chain is walked
/// and appended.
fn be<E: std::error::Error>(e: E) -> Error {
    let mut msg = e.to_string();
    let mut src = e.source();
    while let Some(s) = src {
        msg.push_str(": ");
        msg.push_str(&s.to_string());
        src = s.source();
    }
    Error::Backend(format!("postgres: {msg}"))
}

/// Why no connection could be had, with what the server or the handshake said.
///
/// The pool's own message is "Error occurred while creating a new object: db
/// error", which names neither the refusal nor the fix. The driver's error is
/// read through to the bottom instead, and the two refusals an operator meets
/// first when moving to a managed server are named with the setting that ends
/// them.
fn unreachable(e: deadpool_postgres::PoolError) -> Error {
    use deadpool_postgres::PoolError;
    let detail = match e {
        PoolError::Backend(e) => match be(e) {
            Error::Backend(m) => m.trim_start_matches("postgres: ").to_owned(),
            other => other.to_string(),
        },
        PoolError::Timeout(_) => format!(
            "no connection within {} s; the server accepted the connection but did not answer",
            POOL_WAIT.as_secs()
        ),
        other => other.to_string(),
    };
    let hint = if detail.contains("no encryption") || detail.contains("SSL off") {
        "; the server accepts only TLS connections, so add sslmode=require to the connection \
         string"
    } else if detail.contains("UnknownIssuer") || detail.contains("invalid peer certificate") {
        "; the server's certificate is not signed by an authority this daemon trusts, so give \
         that authority's PEM file as sslrootcert=/path/to/ca.pem"
    } else {
        ""
    };
    Error::Backend(format!("postgres is unreachable: {detail}{hint}"))
}

/// Nanoseconds since the Unix epoch, as stored.
/// Make a literal string safe to use as the fixed part of a `LIKE` pattern.
///
/// A facet value is caller-supplied text and a filter prefix is whatever
/// somebody typed. Without this, typing `%` in the filter box matches every
/// value in the project and typing `_` matches every value one character
/// longer, which reads as a broken filter rather than as a search for those
/// characters.
///
/// The backslash is escaped first, or escaping the wildcards would then have
/// their escape characters escaped in turn.
fn escape_like(raw: &str) -> String {
    raw.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

/// Put the root step's input and output on each trace of a page, as previews.
///
/// One read for the whole page, of at most [`crate::PREVIEW_READ_BYTES`] of
/// each payload.
async fn pg_fill_previews(
    conn: &tokio_postgres::Client,
    project: &str,
    traces: &mut [Trace],
) -> Result<()> {
    let (ids, roots): (Vec<String>, Vec<String>) = traces
        .iter()
        .filter_map(|t| t.root_span_id.clone().map(|r| (t.trace_id.clone(), r)))
        .unzip();
    if ids.is_empty() {
        return Ok(());
    }
    let rows = conn
        .query(
            &format!(
                "SELECT s.trace_id, left(s.input, {n}), left(s.output, {n})
                 FROM spans s JOIN unnest($2::text[], $3::text[]) AS r(trace_id, span_id)
                   ON s.trace_id = r.trace_id AND s.span_id = r.span_id
                 WHERE s.project = $1",
                n = crate::PREVIEW_READ_BYTES
            ),
            &[&project, &ids, &roots],
        )
        .await
        .map_err(be)?;
    for r in &rows {
        let id: String = r.get(0);
        let input: Option<String> = r.get(1);
        let output: Option<String> = r.get(2);
        if let Some(t) = traces.iter_mut().find(|t| t.trace_id == id) {
            t.input_preview = input.as_deref().and_then(crate::payload_preview);
            t.output_preview = output.as_deref().and_then(crate::payload_preview);
        }
    }
    Ok(())
}

/// A `documents` row read as `project, id, version, body, note, created_by,
/// created_at`.
fn pg_document(kind: DocKind, r: &Row) -> StoredDocument {
    let body: String = r.get(3);
    StoredDocument {
        project: r.get(0),
        kind,
        id: r.get(1),
        version: r.get::<_, i64>(2).clamp(0, i64::from(u32::MAX)) as u32,
        body: serde_json::from_str(&body).unwrap_or(Value::Null),
        note: r.get(4),
        created_by: r.get(5),
        created_at: from_nanos(r.get(6)),
    }
}

async fn pg_latest_document(
    conn: &tokio_postgres::Client,
    project: &str,
    kind: DocKind,
    id: &str,
) -> Result<Option<StoredDocument>> {
    Ok(conn
        .query_opt(
            "SELECT project, id, version, body, note, created_by, created_at
             FROM documents WHERE project = $1 AND kind = $2 AND id = $3
             ORDER BY version DESC LIMIT 1",
            &[&project, &kind.as_str(), &id],
        )
        .await
        .map_err(be)?
        .map(|r| pg_document(kind, &r)))
}

fn to_nanos(t: OffsetDateTime) -> i64 {
    t.unix_timestamp_nanos().clamp(i64::MIN as i128, i64::MAX as i128) as i64
}

fn from_nanos(n: i64) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(n)).unwrap_or(OffsetDateTime::UNIX_EPOCH)
}

fn now_nanos() -> i64 {
    to_nanos(OffsetDateTime::now_utc())
}

/// Serialise a JSON value for a TEXT column, dropping `null`.
fn json_out(v: &Option<Value>) -> Option<String> {
    match v {
        Some(Value::Null) | None => None,
        Some(v) => serde_json::to_string(v).ok(),
    }
}

/// Parse a TEXT column back to JSON, tolerating malformed content.
///
/// One unreadable payload must not make a whole trace unreadable, so a row that
/// will not parse yields `None` rather than failing the query.
fn json_in(s: Option<String>) -> Option<Value> {
    s.and_then(|s| serde_json::from_str(&s).ok())
}

fn enc<T: serde::Serialize>(v: &Option<T>) -> Option<String> {
    v.as_ref().and_then(|v| serde_json::to_string(v).ok())
}

fn dec<T: serde::de::DeserializeOwned>(s: Option<String>) -> Option<T> {
    s.and_then(|s| serde_json::from_str(&s).ok())
}

fn tags_out(tags: &Option<Vec<String>>) -> Option<String> {
    tags.as_ref().filter(|t| !t.is_empty()).and_then(|t| serde_json::to_string(t).ok())
}

fn tags_in(s: Option<String>) -> Option<Vec<String>> {
    dec::<Vec<String>>(s).filter(|v| !v.is_empty())
}

/// The sort value is written as text, so a float sort carries every digit it
/// had. Cost used to be scaled to an integer, and the digits below the scale
/// were lost: a walk that resumed from a value under the row it came from was
/// handed that row again on every page, and one that resumed from a value over
/// it dropped every row inside the gap.
fn encode_cursor(sort_value: impl std::fmt::Display, id: &str) -> String {
    hex::encode(format!("{sort_value}|{id}"))
}

fn cursor_parts(cursor: &str) -> Result<(String, String)> {
    let raw = hex::decode(cursor)
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
        .ok_or_else(|| Error::InvalidQuery("malformed cursor".into()))?;
    let (v, id) =
        raw.split_once('|').ok_or_else(|| Error::InvalidQuery("malformed cursor".into()))?;
    Ok((v.to_owned(), id.to_owned()))
}

fn decode_cursor(cursor: &str) -> Result<(i64, String)> {
    let (v, id) = cursor_parts(cursor)?;
    let v = v.parse().map_err(|_| Error::InvalidQuery("malformed cursor".into()))?;
    Ok((v, id))
}

/// The same cursor, read back for a sort whose key is a float.
fn decode_cursor_f64(cursor: &str) -> Result<(f64, String)> {
    let (v, id) = cursor_parts(cursor)?;
    let v = v.parse().map_err(|_| Error::InvalidQuery("malformed cursor".into()))?;
    Ok((v, id))
}

/// A `WHERE` clause under construction, with its bind parameters.
///
/// The parameter numbers are assigned as clauses are pushed, so a filter can be
/// added in the middle of the builder without renumbering everything after it,
/// which is how an off-by-one silently binds the wrong value to the wrong
/// column.
#[derive(Default)]
struct Where {
    clauses: Vec<String>,
    binds: Vec<Box<dyn ToSql + Sync + Send>>,
}

impl Where {
    /// Add a clause. `{}` in `tmpl` is replaced by the next parameter number.
    fn push<T: ToSql + Sync + Send + 'static>(&mut self, tmpl: &str, v: T) {
        self.binds.push(Box::new(v));
        self.clauses.push(tmpl.replace("{}", &format!("${}", self.binds.len())));
    }

    /// Add a clause with no parameter of its own.
    fn push_raw(&mut self, clause: String) {
        self.clauses.push(clause);
    }

    /// The next parameter number, for a clause that binds more than one value.
    fn next(&self) -> usize {
        self.binds.len() + 1
    }

    fn bind<T: ToSql + Sync + Send + 'static>(&mut self, v: T) {
        self.binds.push(Box::new(v));
    }

    fn sql(&self) -> String {
        if self.clauses.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", self.clauses.join(" AND "))
        }
    }

    fn params(&self) -> Vec<&(dyn ToSql + Sync)> {
        self.binds.iter().map(|b| &**b as &(dyn ToSql + Sync)).collect()
    }
}

/// Turn a user's search words into a `tsquery`.
///
/// User input never reaches `to_tsquery`, whose operators would make a stray
/// `&` or `!` a syntax error rather than a search. `plainto_tsquery` parses the
/// text as plain words and ANDs them, which is what a person typing two words
/// expects, and it takes the phrase as a bind parameter.
fn search_text(raw: &str) -> Option<String> {
    let t = raw.trim();
    (!t.is_empty()).then(|| t.to_owned())
}

/// A correlation carrying only the two fields these tables store.
///
/// `Correlation` has no `Default`, deliberately: `project` is required, and a
/// backend that could omit it would write rows nobody can scope.
fn correlation_of(project: String, environment: Option<String>) -> Correlation {
    Correlation {
        project,
        environment,
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
    }
}

fn kind_of(s: &str) -> SpanKind {
    SpanKind::from_str(s).unwrap_or(SpanKind::Generic)
}

fn status_of(s: &str) -> SpanStatus {
    SpanStatus::from_str(s).unwrap_or(SpanStatus::Unset)
}

const SPAN_COLUMNS: &str = "\
    trace_id, span_id, parent_span_id, external_parent_span_id, name, kind, status, \
    start_time, end_time, duration_ns, is_root, is_event, entity_kind, entity_id, entity_name, \
    input, output, attributes, metadata, usage_json, cost_json, error_json, links_json, tags, \
    project, environment, service_name, service_version, session_id, thread_id, user_id, \
    resource_id, run_id, request_id, experiment_id, release, model, provider, received_at";

fn row_to_span(r: &Row) -> Span {
    Span {
        trace_id: r.get(0),
        span_id: r.get(1),
        parent_span_id: r.get(2),
        external_parent_span_id: r.get(3),
        name: r.get(4),
        kind: kind_of(r.get::<_, &str>(5)),
        status: status_of(r.get::<_, &str>(6)),
        start_time: from_nanos(r.get(7)),
        end_time: r.get::<_, Option<i64>>(8).map(from_nanos),
        duration_ns: r.get::<_, Option<i64>>(9).map(|v| v.max(0) as u64),
        is_root: r.get(10),
        is_event: r.get(11),
        entity_kind: r.get::<_, Option<String>>(12).and_then(|s| dec(Some(format!("\"{s}\"")))),
        entity_id: r.get(13),
        entity_name: r.get(14),
        input: json_in(r.get(15)),
        output: json_in(r.get(16)),
        attributes: json_in(r.get(17)),
        metadata: json_in(r.get(18)),
        usage: dec::<TokenUsage>(r.get(19)),
        cost: dec::<Cost>(r.get(20)),
        // No column holds these yet. The spec carries the shape; the storage
        // column, the migration and the write path are the model-parameter
        // track's work.
        model_parameters: None,
        error: dec::<SpanError>(r.get(21)),
        links: dec::<Vec<SpanLink>>(r.get(22)),
        tags: tags_in(r.get(23)),
        correlation: Correlation {
            project: r.get(24),
            environment: r.get(25),
            service_name: r.get(26),
            service_version: r.get(27),
            session_id: r.get(28),
            thread_id: r.get(29),
            user_id: r.get(30),
            resource_id: r.get(31),
            run_id: r.get(32),
            request_id: r.get(33),
            experiment_id: r.get(34),
            release: r.get(35),
        },
        model: r.get(36),
        provider: r.get(37),
        received_at: from_nanos(r.get(38)),
        ingest_key: None,
    }
}

const TRACE_COLUMNS: &str = "\
    trace_id, root_span_id, name, kind, status, start_time, end_time, duration_ns, \
    span_count, error_count, input_tokens, output_tokens, total_cost, currency, \
    model_count, tool_count, tags, project, environment, service_name, session_id, \
    thread_id, user_id, experiment_id, release, score_summary, feedback_summary, \
    is_complete, received_at";

fn row_to_trace(r: &Row) -> Trace {
    let input_tokens: i64 = r.get(10);
    let output_tokens: i64 = r.get(11);
    let total_cost: f64 = r.get(12);
    Trace {
        trace_id: r.get(0),
        root_span_id: r.get(1),
        name: r.get(2),
        kind: kind_of(r.get::<_, &str>(3)),
        status: status_of(r.get::<_, &str>(4)),
        start_time: from_nanos(r.get(5)),
        end_time: r.get::<_, Option<i64>>(6).map(from_nanos),
        duration_ns: r.get::<_, Option<i64>>(7).map(|v| v.max(0) as u64),
        span_count: r.get::<_, i64>(8).max(0) as u32,
        error_count: r.get::<_, i64>(9).max(0) as u32,
        // Zero tokens and zero cost are reported as absent rather than as a
        // figure: "we did not price this" and "this was free" are different
        // claims and the interface renders them differently.
        usage: (input_tokens > 0 || output_tokens > 0).then_some(TokenUsage {
            input_tokens: Some(input_tokens.max(0) as u64),
            output_tokens: Some(output_tokens.max(0) as u64),
            total_tokens: None,
            input_details: None,
            output_details: None,
        }),
        cost: (total_cost > 0.0).then_some(Cost {
            currency: r.get::<_, String>(13),
            input_cost: None,
            output_cost: None,
            total_cost: Some(total_cost),
            price_book: None,
            is_estimated: true,
        }),
        model_count: r.get::<_, i64>(14).max(0) as u32,
        tool_count: r.get::<_, i64>(15).max(0) as u32,
        input_preview: None,
        output_preview: None,
        tags: tags_in(r.get(16)),
        correlation: Correlation {
            project: r.get(17),
            environment: r.get(18),
            service_name: r.get(19),
            service_version: None,
            session_id: r.get(20),
            thread_id: r.get(21),
            user_id: r.get(22),
            resource_id: None,
            run_id: None,
            request_id: None,
            experiment_id: r.get(23),
            release: r.get(24),
        },
        score_summary: json_in(r.get(25)),
        feedback_summary: json_in(r.get(26)),
        is_complete: r.get(27),
        received_at: from_nanos(r.get(28)),
    }
}

const LOG_COLUMNS: &str = "\
    log_id, trace_id, span_id, timestamp, level, message, fields, logger, \
    project, environment, service_name, session_id, user_id, received_at, \
    service_version, thread_id, resource_id, run_id, request_id, experiment_id, release";

fn row_to_log(r: &Row) -> LogRecord {
    LogRecord {
        log_id: r.get(0),
        trace_id: r.get(1),
        span_id: r.get(2),
        timestamp: from_nanos(r.get(3)),
        level: LogLevel::from_str(r.get::<_, &str>(4)).unwrap_or(LogLevel::Info),
        message: r.get(5),
        fields: json_in(r.get(6)),
        logger: r.get(7),
        correlation: Correlation {
            project: r.get(8),
            environment: r.get(9),
            service_name: r.get(10),
            service_version: r.get(14),
            session_id: r.get(11),
            thread_id: r.get(15),
            user_id: r.get(12),
            resource_id: r.get(16),
            run_id: r.get(17),
            request_id: r.get(18),
            experiment_id: r.get(19),
            release: r.get(20),
        },
        received_at: from_nanos(r.get(13)),
    }
}

const SCORE_COLUMNS: &str = "\
    score_id, trace_id, span_id, scorer_id, scorer_version, source, value, min_value, \
    max_value, label, reason, score_trace_id, cost_json, metadata, timestamp, \
    project, environment, experiment_id";

fn row_to_score(r: &Row) -> Score {
    Score {
        score_id: r.get(0),
        trace_id: r.get(1),
        span_id: r.get(2),
        scorer_id: r.get(3),
        scorer_version: r.get(4),
        source: ScoreSource::from_str(r.get::<_, &str>(5)).unwrap_or(ScoreSource::Offline),
        value: r.get(6),
        min: r.get(7),
        max: r.get(8),
        label: r.get(9),
        reason: r.get(10),
        score_trace_id: r.get(11),
        cost: dec::<Cost>(r.get(12)),
        metadata: json_in(r.get(13)),
        timestamp: from_nanos(r.get(14)),
        correlation: Correlation {
            project: r.get(15),
            environment: r.get(16),
            service_name: None,
            service_version: None,
            session_id: None,
            thread_id: None,
            user_id: None,
            resource_id: None,
            run_id: None,
            request_id: None,
            experiment_id: r.get(17),
            release: None,
        },
    }
}

const FEEDBACK_COLUMNS: &str = "\
    feedback_id, trace_id, span_id, kind, value, comment, author_id, author_kind, \
    timestamp, metadata, project, environment";

fn row_to_feedback(r: &Row) -> Feedback {
    Feedback {
        feedback_id: r.get(0),
        trace_id: r.get(1),
        span_id: r.get(2),
        kind: FeedbackKind::from_str(r.get::<_, &str>(3)).unwrap_or(FeedbackKind::Comment),
        value: json_in(r.get(4)).unwrap_or(Value::Null),
        comment: r.get(5),
        author_id: r.get(6),
        author_kind: r.get(7),
        timestamp: from_nanos(r.get(8)),
        metadata: json_in(r.get(9)),
        correlation: Correlation {
            project: r.get(10),
            environment: r.get(11),
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

impl PostgresStore {
    /// Open a pool. No connection is made until the first query.
    ///
    /// # Errors
    /// Returns [`Error::Backend`] if the connection string cannot be parsed or
    /// the pool cannot be built.
    pub fn new(config: PostgresConfig) -> Result<Self> {
        Ok(Self { pool: connect_pool(&config)? })
    }
}

/// Build a connection pool the way this store builds its own: the TLS
/// settings read out of the connection string, and bounded waits.
///
/// Public so the run store, which is a separate crate with its own schema,
/// reaches the same server through the same rules rather than a second copy of
/// them. No connection is made until the first query.
///
/// # Errors
/// Returns [`Error::Backend`] if the connection string cannot be parsed, its
/// `sslrootcert` cannot be read, or the pool cannot be built.
pub fn connect_pool(config: &PostgresConfig) -> Result<Pool> {
    let (url, tls) = split_tls(&config.url)?;
    // The connection string is not repeated in the error: it carries the
    // password, and this message reaches the start-up log.
    let pg: tokio_postgres::Config = url.parse().map_err(|e| {
        Error::Backend(format!("postgres: the connection string is not usable: {e}"))
    })?;

    // Built from the parsed configuration as a whole, so every option the
    // string carries reaches the connection: `connect_timeout`, a second
    // host, `application_name`. Copying five fields out of it dropped the
    // rest without a word.
    let manager_config = ManagerConfig { recycling_method: RecyclingMethod::Fast };
    let manager = if pg.get_ssl_mode() == SslMode::Require {
        let connector = tokio_postgres_rustls::MakeRustlsConnect::new(tls.client_config()?);
        Manager::from_config(pg, connector, manager_config)
    } else {
        Manager::from_config(pg, NoTls, manager_config)
    };

    // Bounded waits. A pool with none holds a request, or a background
    // loop, for ever on a server that accepts the connection and never
    // answers, and the request timeout above it is then the only thing
    // that ends it.
    Pool::builder(manager)
        .max_size(config.pool_size.max(2))
        .wait_timeout(Some(POOL_WAIT))
        .create_timeout(Some(POOL_WAIT))
        .recycle_timeout(Some(POOL_WAIT))
        .runtime(Runtime::Tokio1)
        .build()
        .map_err(|e| Error::Backend(format!("postgres: could not build a pool: {e}")))
}

impl PostgresStore {
    /// Whether the schema is already at the version this build writes.
    ///
    /// `to_regclass` returns NULL rather than raising for a table that does not
    /// exist, so this answers on an empty database without an error path.
    async fn schema_is_current(c: &tokio_postgres::Client) -> Result<bool> {
        let row = c
            .query_one(
                "SELECT to_regclass('schema_version') IS NOT NULL
                        AND EXISTS (SELECT 1 FROM pg_class WHERE relname = 'schema_version')",
                &[],
            )
            .await
            .map_err(be)?;
        if !row.get::<_, bool>(0) {
            return Ok(false);
        }
        let v = c
            .query_opt("SELECT 1 FROM schema_version WHERE version = $1", &[&SCHEMA_VERSION])
            .await
            .map_err(be)?;
        Ok(v.is_some())
    }

    async fn conn(&self) -> Result<deadpool_postgres::Object> {
        self.pool.get().await.map_err(unreachable)
    }

    /// Rebuild the projection for one trace from its spans.
    ///
    /// Recomputed rather than incremented, for the same reason as in the
    /// embedded store: spans arrive out of order, get retried, and get updated
    /// in place. An incremental counter drifts under all three, and a wrong span
    /// count in the list view destroys trust in every other number on the page.
    ///
    /// One row per project and trace id. A trace id is shared across a service
    /// boundary by design, and folding two projects' spans into one row let the
    /// project that wrote the earliest span own the whole run and read the
    /// other project's payloads through it.
    async fn recompute_trace(
        tx: &tokio_postgres::Transaction<'_>,
        project: &str,
        trace_id: &str,
    ) -> Result<()> {
        let totals = tx
            .query_opt(
                &format!("SELECT {TRACE_TOTALS} FROM spans WHERE project = $1 AND trace_id = $2"),
                &[&project, &trace_id],
            )
            .await
            .map_err(be)?;

        let Some(t) = totals else { return Ok(()) };
        let span_count: i64 = t.get(0);
        if span_count == 0 {
            return Ok(());
        }

        // Prefer the real root. Falling back to the earliest span means a trace
        // whose root has not arrived yet is still listed, with the best name we
        // have, instead of disappearing until it does.
        let root = tx
            .query_opt(
                &format!(
                    "SELECT span_id, name, kind, status, tags, project, environment,
                            service_name, session_id, thread_id, user_id, experiment_id,
                            release, received_at
                     FROM spans WHERE project = $1 AND trace_id = $2
                     ORDER BY {ROOT_ORDER} LIMIT 1"
                ),
                &[&project, &trace_id],
            )
            .await
            .map_err(be)?;
        let Some(root) = root else { return Ok(()) };

        let error_count: i64 = t.get(1);
        let start_time: i64 = t.get(2);
        let end_time: Option<i64> = t.get(3);
        let open: i64 = t.get(9);
        // `is_complete` is specified as the root having ended and no span still
        // open. Reporting a duration for a run that is still going would be a
        // measurement of nothing.
        //
        // An event span is excluded from the open count because it is a point
        // in time and has no end to wait for. Counting one as unfinished left
        // any run holding an event reading as still running for ever, with no
        // duration, however much else arrived afterwards.
        //
        // The end time is then required on its own, because the exclusion alone
        // called a run of nothing but events complete: it has nothing open, and
        // nothing finished either, since its root has not arrived yet.
        let is_complete = open == 0 && end_time.is_some();
        // The time elapsed so far, for a run in flight as well as a finished
        // one: the latest end any step has reported, less the earliest start.
        // The specification calls this the wall-clock span of the whole trace,
        // and the other two backends have always reported it this way; a run
        // that has been going for an hour is the run an operator is looking
        // for, and showing it with no duration hid it from the sort.
        //
        // Clamped, because a producer whose clock ran backwards can put the
        // latest end before the earliest start, and a negative duration
        // corrupts every aggregate that reads the column.
        let duration = end_time.map(|e| (e - start_time).max(0));
        let status: &str = if error_count > 0 { "error" } else { root.get(3) };

        tx.execute(
            "INSERT INTO traces (
                trace_id, root_span_id, name, kind, status, start_time, end_time, duration_ns,
                span_count, error_count, input_tokens, output_tokens, total_cost, currency,
                model_count, tool_count, tags, project, environment, service_name, session_id,
                thread_id, user_id, experiment_id, release, is_complete, received_at)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,'USD',$14,$15,$16,$17,$18,
                     $19,$20,$21,$22,$23,$24,$25,$26)
             ON CONFLICT (project, trace_id) DO UPDATE SET
                root_span_id = EXCLUDED.root_span_id, name = EXCLUDED.name,
                kind = EXCLUDED.kind, status = EXCLUDED.status,
                start_time = EXCLUDED.start_time, end_time = EXCLUDED.end_time,
                duration_ns = EXCLUDED.duration_ns, span_count = EXCLUDED.span_count,
                error_count = EXCLUDED.error_count, input_tokens = EXCLUDED.input_tokens,
                output_tokens = EXCLUDED.output_tokens, total_cost = EXCLUDED.total_cost,
                model_count = EXCLUDED.model_count, tool_count = EXCLUDED.tool_count,
                tags = EXCLUDED.tags, environment = EXCLUDED.environment, service_name = EXCLUDED.service_name,
                session_id = EXCLUDED.session_id, thread_id = EXCLUDED.thread_id,
                user_id = EXCLUDED.user_id, experiment_id = EXCLUDED.experiment_id,
                release = EXCLUDED.release, is_complete = EXCLUDED.is_complete,
                received_at = EXCLUDED.received_at",
            &[
                &trace_id,
                &root.get::<_, String>(0),
                &root.get::<_, String>(1),
                &root.get::<_, String>(2),
                &status,
                &start_time,
                &end_time,
                &duration,
                &span_count,
                &error_count,
                &t.get::<_, i64>(4),
                &t.get::<_, i64>(5),
                &t.get::<_, f64>(6),
                &t.get::<_, i64>(7),
                &t.get::<_, i64>(8),
                &root.get::<_, Option<String>>(4),
                &root.get::<_, String>(5),
                &root.get::<_, Option<String>>(6),
                &root.get::<_, Option<String>>(7),
                &root.get::<_, Option<String>>(8),
                &root.get::<_, Option<String>>(9),
                &root.get::<_, Option<String>>(10),
                &root.get::<_, Option<String>>(11),
                &root.get::<_, Option<String>>(12),
                &is_complete,
                &root.get::<_, i64>(13),
            ],
        )
        .await
        .map_err(be)?;

        // The rollups are rebuilt here as well as where a score or a piece of
        // feedback lands, because the projection row may not have existed when
        // they did. Scoring a run while its spans are still streaming in is an
        // ordinary ordering, and the `UPDATE traces` those two do reaches no
        // row until the first span creates it. Without this the score was lost
        // for good: the run listed with no score, and sorted worst-first as if
        // it had never been judged. The embedded store rebuilds both here for
        // the same reason.
        Self::recompute_scores(tx, project, trace_id).await?;
        Self::recompute_feedback(tx, project, trace_id).await?;

        Ok(())
    }

    /// Refresh the score summary a trace carries, after scores are written.
    async fn recompute_scores(
        tx: &tokio_postgres::Transaction<'_>,
        project: &str,
        trace_id: &str,
    ) -> Result<()> {
        let rows = tx
            .query(
                "SELECT scorer_id, AVG(value)::double precision, MIN(value)::double precision
                 FROM scores WHERE project = $1 AND trace_id = $2 GROUP BY scorer_id",
                &[&project, &trace_id],
            )
            .await
            .map_err(be)?;
        if rows.is_empty() {
            return Ok(());
        }
        let mut summary = serde_json::Map::new();
        let mut min_score = f64::MAX;
        for r in &rows {
            // A bare number per scorer, which is what the spec calls this field
            // and what the run list reads. An object per scorer here was not a
            // richer shape, it was a shape the reader discards, so a Postgres
            // deployment showed no score at all on a run that had them.
            summary.insert(r.get::<_, String>(0), serde_json::json!(r.get::<_, f64>(1)));
            // The worst single score, not the worst average. Sorting a run list
            // by quality is a search for the run that failed a check, and an
            // average hides the one bad answer among four good ones.
            min_score = min_score.min(r.get::<_, f64>(2));
        }
        tx.execute(
            "UPDATE traces SET score_summary = $1, min_score = $2
             WHERE project = $3 AND trace_id = $4",
            &[
                &serde_json::to_string(&Value::Object(summary)).ok(),
                &(min_score < f64::MAX).then_some(min_score),
                &project,
                &trace_id,
            ],
        )
        .await
        .map_err(be)?;
        Ok(())
    }

    /// Refresh the feedback summary a trace carries.
    async fn recompute_feedback(
        tx: &tokio_postgres::Transaction<'_>,
        project: &str,
        trace_id: &str,
    ) -> Result<()> {
        let rows = tx
            .query(
                "SELECT kind, value, COUNT(*)::bigint FROM feedback
                 WHERE project = $1 AND trace_id = $2 GROUP BY kind, value",
                &[&project, &trace_id],
            )
            .await
            .map_err(be)?;
        if rows.is_empty() {
            return Ok(());
        }
        let mut summary = serde_json::Map::new();
        for r in &rows {
            let key = format!("{}:{}", r.get::<_, String>(0), r.get::<_, String>(1));
            summary.insert(key, serde_json::json!(r.get::<_, i64>(2)));
        }
        tx.execute(
            "UPDATE traces SET feedback_summary = $1 WHERE project = $2 AND trace_id = $3",
            &[&serde_json::to_string(&Value::Object(summary)).ok(), &project, &trace_id],
        )
        .await
        .map_err(be)?;
        Ok(())
    }

    /// Build the projection of every trace from its spans, one row per project
    /// and trace id. Used once, by the migration that changed the key, inside
    /// its transaction.
    ///
    /// One statement over the whole table, not `recompute_trace` once per run.
    /// The loop ran several queries a run, so 50,000 runs took over ten minutes
    /// in a debug build. The dropped table's exclusive lock stopped every
    /// reader and writer for all of it, and a startup probe restarted the
    /// daemon before it finished.
    ///
    /// Each column is what the recompute writes: the aggregates and the root
    /// order are the same text, a run with no scores or no feedback keeps a
    /// NULL summary, and the summaries read back as the JSON `recompute_scores`
    /// and `recompute_feedback` write. A test compares the rows each one writes.
    async fn rebuild_traces(tx: &tokio_postgres::Transaction<'_>) -> Result<u64> {
        // The shortest digits that read back as the same double. A server or a
        // role configured with zero or less writes fewer, and the summary would
        // then hold a rounded average.
        tx.batch_execute("SET LOCAL extra_float_digits = 3").await.map_err(be)?;
        let sql = format!(
            "WITH totals (project, trace_id, span_count, error_count, start_time, end_time,
                          input_tokens, output_tokens, total_cost, model_count, tool_count,
                          open)
             AS (SELECT project, trace_id, {TRACE_TOTALS} FROM spans GROUP BY project, trace_id),
             roots AS (
                SELECT DISTINCT ON (project, trace_id)
                       project, trace_id, span_id, name, kind, status, tags, environment,
                       service_name, session_id, thread_id, user_id, experiment_id, release,
                       received_at
                FROM spans ORDER BY project, trace_id, {ROOT_ORDER}),
             score AS (
                SELECT project, trace_id,
                       '{{' || string_agg(to_json(scorer_id)::text || ':' || {SCORE_JSON_NUMBER},
                                          ',' ORDER BY scorer_id) || '}}' AS summary,
                       MIN(min_value) AS min_score
                FROM (SELECT project, trace_id, scorer_id,
                             AVG(value)::double precision AS avg_value,
                             MIN(value)::double precision AS min_value
                      FROM scores WHERE trace_id IS NOT NULL
                      GROUP BY project, trace_id, scorer_id) per_scorer
                GROUP BY project, trace_id),
             fb AS (
                SELECT project, trace_id,
                       '{{' || string_agg(to_json(kind || ':' || value)::text || ':' || n::text,
                                          ',' ORDER BY kind, value) || '}}' AS summary
                FROM (SELECT project, trace_id, kind, value, COUNT(*)::bigint AS n
                      FROM feedback WHERE trace_id IS NOT NULL
                      GROUP BY project, trace_id, kind, value) per_value
                GROUP BY project, trace_id)
             INSERT INTO traces (
                trace_id, root_span_id, name, kind, status, start_time, end_time, duration_ns,
                span_count, error_count, input_tokens, output_tokens, total_cost, currency,
                model_count, tool_count, tags, project, environment, service_name, session_id,
                thread_id, user_id, experiment_id, release, score_summary, feedback_summary,
                min_score, is_complete, received_at)
             SELECT t.trace_id, r.span_id, r.name, r.kind,
                    CASE WHEN t.error_count > 0 THEN 'error' ELSE r.status END,
                    t.start_time, t.end_time,
                    CASE WHEN t.end_time IS NULL THEN NULL
                         ELSE GREATEST(t.end_time - t.start_time, 0) END,
                    t.span_count, t.error_count, t.input_tokens, t.output_tokens,
                    t.total_cost, 'USD', t.model_count, t.tool_count, r.tags, t.project,
                    r.environment, r.service_name, r.session_id, r.thread_id, r.user_id,
                    r.experiment_id, r.release, s.summary, f.summary, s.min_score,
                    t.open = 0 AND t.end_time IS NOT NULL, r.received_at
             FROM totals t
             JOIN roots r ON r.project = t.project AND r.trace_id = t.trace_id
             LEFT JOIN score s ON s.project = t.project AND s.trace_id = t.trace_id
             LEFT JOIN fb f ON f.project = t.project AND f.trace_id = t.trace_id"
        );
        tx.execute(&sql, &[]).await.map_err(be)
    }
}

/// The columns a caller may group by, and the table each one describes.
///
/// An allowlist, because this is the one value in this file that cannot be a
/// bind parameter: a column name is part of the statement, not data in it.
///
/// The table is the point. Model, provider and entity belong to a step, so a
/// run that called two models falls in both groups. The other six describe the
/// whole run, and `GroupedStat::count` is specified as how many runs fell in a
/// group, so they are read from `traces`. Reading them from `spans` counted
/// steps, which is how a deployment on Postgres reported three times as many
/// production runs as the same data on the other two backends.
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

/// Nanoseconds to milliseconds, keeping absence as absence.
fn ms(v: Option<f64>) -> Option<f64> {
    v.map(|n| n / 1_000_000.0)
}

impl PostgresStore {
    /// Per-group totals over a window, counted in runs for a run dimension and
    /// in steps for a step dimension, by [`group_column`].
    async fn grouped(
        &self,
        project: &str,
        from: OffsetDateTime,
        to: OffsetDateTime,
        dim: &str,
    ) -> Result<Vec<GroupedStat>> {
        let (col, table) = group_column(dim)?;
        // A step that names no value for a step dimension is left out, as the
        // other two backends leave it out. Counting it as a group called
        // `(none)` put every tool call into a breakdown by model.
        let named =
            if table == "spans" { format!("AND coalesce({col}, '') <> ''") } else { String::new() };
        let conn = self.conn().await?;
        let rows = conn
            .query(
                &format!(
                    "SELECT COALESCE(NULLIF({col}, ''), '(none)') AS k, COUNT(*)::bigint,
                            COUNT(*) FILTER (WHERE status = 'error')::bigint,
                            COALESCE(SUM(total_cost), 0)::double precision,
                            COALESCE(SUM(input_tokens), 0)::bigint,
                            COALESCE(SUM(output_tokens), 0)::bigint,
                            AVG(duration_ns)::double precision,
                            MAX(duration_ns)::double precision
                     FROM {table}
                     WHERE project = $1 AND start_time >= $2 AND start_time < $3 {named}
                     GROUP BY k ORDER BY 2 DESC LIMIT 100"
                ),
                &[&project, &to_nanos(from), &to_nanos(to)],
            )
            .await
            .map_err(be)?;

        Ok(rows
            .iter()
            .map(|r| GroupedStat {
                key: r.get(0),
                count: r.get::<_, i64>(1).max(0) as u64,
                error_count: r.get::<_, i64>(2).max(0) as u64,
                total_cost: Some(r.get(3)),
                input_tokens: Some(r.get::<_, i64>(4).max(0) as u64),
                output_tokens: Some(r.get::<_, i64>(5).max(0) as u64),
                latency: Some(LatencyStats {
                    p50: None,
                    p90: None,
                    p95: None,
                    p99: None,
                    mean: ms(r.get(6)),
                    max: ms(r.get(7)),
                }),
            })
            .collect())
    }
}

/// The four columns that name a person. A column missing from this list is a
/// person the identifier probe cannot see.
const ATTRIBUTING_COLUMNS: [(&str, &str); 4] =
    [("traces", "user_id"), ("spans", "user_id"), ("logs", "user_id"), ("feedback", "author_id")];

/// The three statements the identifier probe runs, built in one place.
///
/// Extracted so a test can read them without a server. Nothing in the type
/// system says a formatted statement parses, and this SQL is assembled from
/// four table names and a branch, which is exactly the shape that goes wrong
/// unseen.
///
/// `hashed` is whether the subject this erasure carried is a digest, by
/// [`is_hashed_identifier`]. It decides which form is the other one.
fn probe_statements(hashed: bool) -> [String; 3] {
    // The whole shape, not the prefix. `LIKE 'sha256:%'` decides the question
    // rather than narrowing it, and decides it wrongly: an identifier a caller
    // hashed themselves carries the prefix, [`unmatchable_identifiers`] counts
    // it as the other form, and `NOT LIKE` dropped it before the rule could see
    // it. `~` is anchored by the pattern and case sensitive, so it selects
    // exactly the rows the rule puts on the other side.
    let shape = hashed_id_regex();
    let ors = |per_column: &dyn Fn(&str, &str) -> String| -> String {
        ATTRIBUTING_COLUMNS.iter().map(|(t, c)| per_column(t, c)).collect::<Vec<_>>().join(" OR ")
    };
    let attributed = ors(&|t, c| {
        format!("EXISTS(SELECT 1 FROM {t} WHERE project = $1 AND coalesce({c},'') <> '')")
    });
    // A digest other than the subject's own. Excluding the subject is what
    // stops this reporting the rows this erasure has just addressed, and it is
    // why the answer says nothing about whether the erasure worked.
    let opaque = ors(&|t, c| {
        format!(
            "EXISTS(SELECT 1 FROM {t} WHERE project = $1
             AND {c} ~ '{shape}' AND {c} <> $2)"
        )
    });
    // Only identifiers of the other form are read. In a deployment that never
    // changed its redaction setting that is no rows at all, where this used to
    // read every identifier the project held.
    let form = if hashed { "!~" } else { "~" };
    let candidates = ATTRIBUTING_COLUMNS
        .iter()
        .map(|(t, c)| {
            format!(
                "SELECT {c} AS id FROM {t} WHERE project = $1
                 AND coalesce({c},'') <> '' AND {c} {form} '{shape}'"
            )
        })
        .collect::<Vec<_>>()
        .join(" UNION ");
    [format!("SELECT {attributed}"), format!("SELECT {opaque}"), candidates]
}

/// What the project still holds that names a person, read after an erasure.
///
/// The same three questions the embedded store asks, in the same order and by
/// the same rule. Two are existence checks that stop at the first row, and only
/// the third reads anything in bulk: the single union that answered all three
/// at once read every identifier in the project whatever the answer was.
///
/// The form test is `~` against [`hashed_id_regex`] rather than a `LIKE` on the
/// prefix, so the narrowing selects exactly the rows [`unmatchable_identifiers`]
/// would put on the other side. What an identifier means is still decided
/// there, in one place for all three backends; the SQL only has to agree with
/// it row for row, which a prefix test did not.
async fn probe_identifiers(
    conn: &deadpool_postgres::Object,
    project: &str,
    subject: &str,
) -> Result<IdentifierProbe> {
    let hashed = is_hashed_identifier(subject);
    let [attributed, opaque, candidates] = probe_statements(hashed);

    let nothing_attributed: bool =
        !conn.query_one(&attributed, &[&project]).await.map_err(be)?.get::<_, bool>(0);

    let opaque_remaining = if hashed {
        conn.query_one(&opaque, &[&project, &subject]).await.map_err(be)?.get::<_, bool>(0)
    } else {
        false
    };

    let candidates: Vec<String> = conn
        .query(&candidates, &[&project])
        .await
        .map_err(be)?
        .iter()
        .map(|r| r.get::<_, String>(0))
        .collect();

    Ok(IdentifierProbe {
        nothing_attributed,
        unmatchable: unmatchable_identifiers(subject, &candidates),
        opaque_remaining,
    })
}

#[async_trait::async_trait]
impl Store for PostgresStore {
    fn name(&self) -> &'static str {
        "postgres"
    }

    fn capabilities(&self) -> StoreCapabilities {
        StoreCapabilities {
            // `plainto_tsquery` over a `simple` tsvector: whole words, and the
            // `simple` configuration folds case for every letter it knows,
            // which is the whole database encoding rather than ASCII.
            text_search: TextSearch::Words,
            unicode_case_folding: true,
            // A `DELETE` inside a transaction. Committed means gone.
            immediate_deletes: true,
            discovery: true,
            discovery_label_values: true,
            delta_polling: true,
            per_signal_retention: true,
            // No engine-level expiry here. Retention is a delete the daemon runs on
            // a timer, and there is nothing to hand the window to.
            declarative_retention: false,
            // Declarative partitioning is deliberately not used here. It would
            // mean putting the partition key into every primary key, which the
            // trace projection is keyed on `trace_id` alone precisely because a
            // run's start time moves when a late batch lands, and it would make
            // the upgrade for an existing deployment a table rewrite. Retention
            // is therefore a row delete, and this says so rather than leaving a
            // reader to find out from a slow expiry.
            partitioning: Partitioning::None,
            partition_drop_retention: false,
            idempotent_schema_init: true,
            theme_analysis: true,
            ..StoreCapabilities::minimal("postgres")
        }
    }

    async fn migrate(&self) -> Result<()> {
        let mut conn = self.conn().await?;

        // Fast path. `ALTER TABLE` takes an `AccessExclusiveLock`, which blocks
        // every reader and writer of that table for as long as the migrating
        // transaction runs. Doing that on every start would mean a replica
        // coming up briefly stalls the ones already serving, so a start that has
        // nothing to apply must not touch the schema at all.
        if Self::schema_is_current(&conn).await? {
            return Ok(());
        }

        let tx = conn.transaction().await.map_err(be)?;

        // Serialise migration across every process that shares this database.
        //
        // Postgres is the backend people reach for precisely so mcpd can run as
        // several replicas, and they all call this on start. `CREATE TABLE IF
        // NOT EXISTS` is not safe to run concurrently against the same
        // relation: two of them take an `AccessExclusiveLock` in whatever order
        // they reach each statement and deadlock, which is what a rolling
        // deploy looks like from the database's side. An advisory lock costs
        // one round trip on start and makes the race impossible.
        //
        // `_xact_` so the lock is released by the commit below, including on the
        // path where an error aborts the transaction. A session lock would
        // survive a panic and wedge every later replica.
        tx.execute("SELECT pg_advisory_xact_lock($1)", &[&MIGRATION_LOCK]).await.map_err(be)?;

        // Checked again now the lock is held. Between the fast path above and
        // this line another replica may have finished the whole migration, and
        // re-running the DDL would take the exclusive locks for nothing.
        if Self::schema_is_current(tx.client()).await? {
            return Ok(());
        }

        // A database from before version 5 holds one projection row per trace
        // id, merged across projects. It is dropped and the file below creates
        // it again with the new key. All of it is one transaction, so a failure
        // leaves the old table where it was.
        // Two statements, because Postgres resolves every table a query names
        // before it runs any of it, so a single `CASE` over a table that may
        // not exist yet fails on a fresh database.
        let versioned: bool = tx
            .query_one("SELECT to_regclass('schema_version') IS NOT NULL", &[])
            .await
            .map_err(be)?
            .get(0);
        let before: Option<i32> = if versioned {
            tx.query_one("SELECT MAX(version) FROM schema_version", &[]).await.map_err(be)?.get(0)
        } else {
            None
        };
        let rebuild = before.is_some_and(|v| v < TRACES_BY_PROJECT);
        if rebuild {
            tx.batch_execute("DROP TABLE IF EXISTS traces").await.map_err(be)?;
        }

        tx.batch_execute(include_str!("postgres_schema.sql")).await.map_err(be)?;
        if rebuild {
            let rebuilt = Self::rebuild_traces(&tx).await?;
            tracing::info!("rebuilt {rebuilt} trace projections, one per project and trace id");
        }
        tx.execute(
            "INSERT INTO schema_version (version, applied_at) VALUES ($1, $2)
             ON CONFLICT (version) DO NOTHING",
            &[&SCHEMA_VERSION, &now_nanos()],
        )
        .await
        .map_err(be)?;

        tx.commit().await.map_err(be)?;
        Ok(())
    }

    async fn ping(&self) -> Result<()> {
        self.conn().await?.query_one("SELECT 1", &[]).await.map_err(be)?;
        Ok(())
    }

    async fn ingest(&self, mut batch: Batch) -> Result<IngestResult> {
        let mut conn = self.conn().await?;
        let tx = conn.transaction().await.map_err(be)?;

        // Idempotency first. A client that retried after a timeout must not
        // double count, and the cheapest place to notice is before any write.
        if let Some(key) = &batch.ingest_key {
            let seen = tx
                .execute(
                    "INSERT INTO ingest_keys (key, seen_at) VALUES ($1, $2)
                     ON CONFLICT (key) DO NOTHING",
                    &[key, &now_nanos()],
                )
                .await
                .map_err(be)?;
            if seen == 0 {
                return Ok(IngestResult {
                    accepted: 0,
                    rejected: 0,
                    duplicate: batch.len() as u32,
                    errors: None,
                });
            }
        }

        let now = now_nanos();
        // Collected before the loops below mutate the batch, and written at the
        // end of the same transaction.
        let facets = crate::facet_values(&batch);
        let mut accepted = 0_u32;
        let mut result = IngestResult { accepted: 0, rejected: 0, duplicate: 0, errors: None };
        // Keyed by project and trace id: a trace id two projects share is a run
        // in each, rebuilt from that project's own rows.
        type Touched = std::collections::BTreeSet<(String, String)>;
        let mut touched = Touched::new();
        let mut scored = Touched::new();
        let mut fed = Touched::new();

        // Every write below changes a row only when it already belongs to the
        // sender's project. A row another project holds is left alone, the
        // statement reports no row changed, and the signal is refused. Every id
        // here is chosen by the sender, and a span id travels to every
        // downstream service in `traceparent`, so knowing one takes no guessing.
        for (index, s) in batch.spans.iter_mut().enumerate() {
            // The store owns this, not its callers. `mcpd demo`, `mcpd doctor`
            // and the score and feedback routes all reach a backend directly
            // without passing through the ingest pipeline, so a backend that
            // trusted `is_root` and `duration_ns` as sent stored a different
            // row from the two that derive them. The embedded and ClickHouse
            // backends already do this; Postgres did not, and that is how one
            // deployment came to disagree with the other two about how long a
            // run took.
            s.normalise();
            let received = to_nanos(s.received_at);
            let received = if received == 0 { now } else { received };
            let stored = tx
                .execute(
                    "INSERT INTO spans (
                    trace_id, span_id, parent_span_id, external_parent_span_id, name, kind,
                    status, start_time, end_time, duration_ns, is_root, is_event, entity_kind,
                    entity_id, entity_name, input, output, attributes, metadata, usage_json,
                    input_tokens, output_tokens, cost_json, total_cost, error_json, links_json,
                    tags, project, environment, service_name, service_version, session_id,
                    thread_id, user_id, resource_id, run_id, request_id, experiment_id, release,
                    model, provider, received_at)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,
                         $20,$21,$22,$23,$24,$25,$26,$27,$28,$29,$30,$31,$32,$33,$34,$35,$36,
                         $37,$38,$39,$40,$41,$42)
                 ON CONFLICT (trace_id, span_id) DO UPDATE SET
                    parent_span_id = EXCLUDED.parent_span_id, name = EXCLUDED.name,
                    kind = EXCLUDED.kind, status = EXCLUDED.status,
                    start_time = EXCLUDED.start_time, end_time = EXCLUDED.end_time,
                    duration_ns = EXCLUDED.duration_ns, is_root = EXCLUDED.is_root,
                    is_event = EXCLUDED.is_event,
                    input = EXCLUDED.input, output = EXCLUDED.output,
                    attributes = EXCLUDED.attributes, metadata = EXCLUDED.metadata,
                    usage_json = EXCLUDED.usage_json, input_tokens = EXCLUDED.input_tokens,
                    output_tokens = EXCLUDED.output_tokens, cost_json = EXCLUDED.cost_json,
                    total_cost = EXCLUDED.total_cost, error_json = EXCLUDED.error_json,
                    links_json = EXCLUDED.links_json, tags = EXCLUDED.tags,
                    model = EXCLUDED.model, provider = EXCLUDED.provider,
                    received_at = EXCLUDED.received_at
                 WHERE spans.project = EXCLUDED.project",
                    &[
                        &s.trace_id,
                        &s.span_id,
                        &s.parent_span_id,
                        &s.external_parent_span_id,
                        &s.name,
                        &s.kind.as_str(),
                        &s.status.as_str(),
                        &to_nanos(s.start_time),
                        &s.end_time.map(to_nanos),
                        &s.duration_ns.map(|d| d as i64),
                        &s.is_root,
                        &s.is_event,
                        &s.entity_kind.as_ref().map(|k| {
                            serde_json::to_string(k)
                                .unwrap_or_default()
                                .trim_matches('"')
                                .to_owned()
                        }),
                        &s.entity_id,
                        &s.entity_name,
                        &json_out(&s.input),
                        &json_out(&s.output),
                        &json_out(&s.attributes),
                        &json_out(&s.metadata),
                        &enc(&s.usage),
                        &s.usage.as_ref().and_then(|u| u.input_tokens).map(|v| v as i64),
                        &s.usage.as_ref().and_then(|u| u.output_tokens).map(|v| v as i64),
                        &enc(&s.cost),
                        &s.cost.as_ref().and_then(|c| c.total_cost),
                        &enc(&s.error),
                        &enc(&s.links),
                        &tags_out(&s.tags),
                        &s.correlation.project,
                        &s.correlation.environment,
                        &s.correlation.service_name,
                        &s.correlation.service_version,
                        &s.correlation.session_id,
                        &s.correlation.thread_id,
                        &s.correlation.user_id,
                        &s.correlation.resource_id,
                        &s.correlation.run_id,
                        &s.correlation.request_id,
                        &s.correlation.experiment_id,
                        &s.correlation.release,
                        &s.model,
                        &s.provider,
                        &received,
                    ],
                )
                .await
                .map_err(be)?;
            if stored == 0 {
                crate::refuse_foreign(
                    &mut result,
                    mcpd_core::SignalType::Span,
                    index,
                    "span",
                    &s.span_id,
                );
                continue;
            }
            touched.insert((s.correlation.project.clone(), s.trace_id.clone()));
            accepted += 1;
        }

        // A replay from the same project keeps the row it already has, as it
        // always did; the no-op update is there so the statement reports a row
        // for it, and reports none when the id belongs to another project.
        for (index, l) in batch.logs.iter().enumerate() {
            let stored = tx
                .execute(
                    "INSERT INTO logs (log_id, trace_id, span_id, timestamp, level, severity,
                    message, fields, logger, project, environment, service_name, session_id,
                    user_id, received_at, service_version, thread_id, resource_id, run_id,
                    request_id, experiment_id, release)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,
                         $16,$17,$18,$19,$20,$21,$22)
                 ON CONFLICT (log_id) DO UPDATE SET project = EXCLUDED.project
                 WHERE logs.project = EXCLUDED.project",
                    &[
                        &l.log_id,
                        &l.trace_id,
                        &l.span_id,
                        &to_nanos(l.timestamp),
                        &l.level.as_str(),
                        &i64::from(l.level.severity()),
                        &l.message,
                        &json_out(&l.fields),
                        &l.logger,
                        &l.correlation.project,
                        &l.correlation.environment,
                        &l.correlation.service_name,
                        &l.correlation.session_id,
                        &l.correlation.user_id,
                        &now,
                        &l.correlation.service_version,
                        &l.correlation.thread_id,
                        &l.correlation.resource_id,
                        &l.correlation.run_id,
                        &l.correlation.request_id,
                        &l.correlation.experiment_id,
                        &l.correlation.release,
                    ],
                )
                .await
                .map_err(be)?;
            if stored == 0 {
                crate::refuse_foreign(
                    &mut result,
                    mcpd_core::SignalType::Log,
                    index,
                    "log",
                    &l.log_id,
                );
                continue;
            }
            accepted += 1;
        }

        for (index, m) in batch.metrics.iter().enumerate() {
            let stored = tx
                .execute(
                    "INSERT INTO metrics (metric_id, name, kind, value, unit, timestamp, labels,
                    buckets, trace_id, span_id, project, environment, received_at)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)
                 ON CONFLICT (metric_id) DO UPDATE SET project = EXCLUDED.project
                 WHERE metrics.project = EXCLUDED.project",
                    &[
                        &m.metric_id,
                        &m.name,
                        &m.kind.as_str(),
                        &m.value,
                        &m.unit,
                        &to_nanos(m.timestamp),
                        &enc(&m.labels),
                        &enc(&m.buckets),
                        &m.trace_id,
                        &m.span_id,
                        &m.correlation.project,
                        &m.correlation.environment,
                        &now,
                    ],
                )
                .await
                .map_err(be)?;
            if stored == 0 {
                let what = "metric point";
                crate::refuse_foreign(
                    &mut result,
                    mcpd_core::SignalType::Metric,
                    index,
                    what,
                    &m.metric_id,
                );
                continue;
            }
            accepted += 1;
        }

        for (index, s) in batch.scores.iter().enumerate() {
            let stored = tx
                .execute(
                    "INSERT INTO scores (score_id, trace_id, span_id, scorer_id, scorer_version,
                    source, value, min_value, max_value, label, reason, score_trace_id,
                    cost_json, metadata, timestamp, project, environment, experiment_id)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18)
                 ON CONFLICT (score_id) DO UPDATE SET project = EXCLUDED.project
                 WHERE scores.project = EXCLUDED.project",
                    &[
                        &s.score_id,
                        &s.trace_id,
                        &s.span_id,
                        &s.scorer_id,
                        &s.scorer_version,
                        &s.source.as_str(),
                        &s.value,
                        &s.min,
                        &s.max,
                        &s.label,
                        &s.reason,
                        &s.score_trace_id,
                        &enc(&s.cost),
                        &json_out(&s.metadata),
                        &to_nanos(s.timestamp),
                        &s.correlation.project,
                        &s.correlation.environment,
                        &s.correlation.experiment_id,
                    ],
                )
                .await
                .map_err(be)?;
            if stored == 0 {
                crate::refuse_foreign(
                    &mut result,
                    mcpd_core::SignalType::Score,
                    index,
                    "score",
                    &s.score_id,
                );
                continue;
            }
            if let Some(t) = &s.trace_id {
                scored.insert((s.correlation.project.clone(), t.clone()));
            }
            accepted += 1;
        }

        for (index, f) in batch.feedback.iter().enumerate() {
            let stored = tx
                .execute(
                    "INSERT INTO feedback (feedback_id, trace_id, span_id, kind, value, comment,
                    author_id, author_kind, timestamp, metadata, project, environment)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)
                 ON CONFLICT (feedback_id) DO UPDATE SET project = EXCLUDED.project
                 WHERE feedback.project = EXCLUDED.project",
                    &[
                        &f.feedback_id,
                        &f.trace_id,
                        &f.span_id,
                        &f.kind.as_str(),
                        // `value` is a JSON document, not a string: a thumb is
                        // `"up"` but a rating is `4`. It is serialised the way the
                        // embedded store serialises it so the two agree on what a
                        // row holds.
                        &serde_json::to_string(&f.value).unwrap_or_else(|_| "null".into()),
                        &f.comment,
                        &f.author_id,
                        &f.author_kind,
                        &to_nanos(f.timestamp),
                        &json_out(&f.metadata),
                        &f.correlation.project,
                        &f.correlation.environment,
                    ],
                )
                .await
                .map_err(be)?;
            if stored == 0 {
                let id = &f.feedback_id;
                crate::refuse_foreign(
                    &mut result,
                    mcpd_core::SignalType::Feedback,
                    index,
                    "feedback",
                    id,
                );
                continue;
            }
            if let Some(t) = &f.trace_id {
                fed.insert((f.correlation.project.clone(), t.clone()));
            }
            accepted += 1;
        }

        // Notes last of the six signals, and inside the same transaction. A
        // note resolves the entity it is filed under from its trace's root
        // span, so writing it after the spans of the same batch means a note
        // that travelled beside its own trace is filed correctly rather than
        // waiting for a later note about the same run to notice.
        for (index, n) in batch.aspect_notes.iter().enumerate() {
            // Resolved per note rather than per distinct trace. A batch carries
            // at most one note per aspect per run, so the repetition is bounded
            // by the number of aspects and the lookup is a primary-key read.
            let root = tx
                .query_opt(
                    "SELECT s.entity_kind, s.entity_name, t.start_time
                       FROM traces t
                       LEFT JOIN spans s
                         ON s.project = t.project AND s.trace_id = t.trace_id
                        AND s.span_id = t.root_span_id
                      WHERE t.trace_id = $1 AND t.project = $2",
                    &[&n.trace_id, &n.correlation.project],
                )
                .await
                .map_err(be)?;
            let (entity_kind, entity_name, started) = match &root {
                Some(r) => (
                    r.get::<_, Option<String>>(0),
                    r.get::<_, Option<String>>(1),
                    r.get::<_, Option<i64>>(2),
                ),
                // Writing a note lags ingestion, so a note whose trace is not
                // stored yet is ordinary. It keeps a NULL entity, stays
                // readable by trace id, and is absent from the entity roll-ups.
                None => (None, None, None),
            };
            // Sketched here rather than handed in, so the sketch is of the text
            // that was actually stored: the redactor runs before this, and
            // sketching what the sender wrote would cluster on words the daemon
            // deliberately did not keep.
            let sketch = mcpd_core::sketch::Sketch::of(&n.text).to_bytes();
            let c = &n.correlation;
            let stored = tx
                .execute(
                    "INSERT INTO aspect_notes (
                    note_id, trace_id, aspect, text, definition_version, model, created_at,
                    entity_kind, entity_name, trace_started_at, sketch, project, environment,
                    service_name, service_version, session_id, thread_id, user_id, resource_id,
                    run_id, request_id, experiment_id, release)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,
                         $20,$21,$22,$23)
                 ON CONFLICT (note_id) DO UPDATE SET
                    text = EXCLUDED.text, aspect = EXCLUDED.aspect,
                    definition_version = EXCLUDED.definition_version, model = EXCLUDED.model,
                    created_at = EXCLUDED.created_at, entity_kind = EXCLUDED.entity_kind,
                    entity_name = EXCLUDED.entity_name,
                    trace_started_at = EXCLUDED.trace_started_at, sketch = EXCLUDED.sketch
                 WHERE aspect_notes.project = EXCLUDED.project",
                    &[
                        &n.note_id,
                        &n.trace_id,
                        &n.aspect,
                        &n.text,
                        &i64::from(n.definition_version),
                        &n.model,
                        &to_nanos(n.created_at),
                        &entity_kind,
                        &entity_name,
                        &started,
                        &sketch,
                        &c.project,
                        &c.environment,
                        &c.service_name,
                        &c.service_version,
                        &c.session_id,
                        &c.thread_id,
                        &c.user_id,
                        &c.resource_id,
                        &c.run_id,
                        &c.request_id,
                        &c.experiment_id,
                        &c.release,
                    ],
                )
                .await
                .map_err(be)?;
            if stored == 0 {
                let id = &n.note_id;
                crate::refuse_foreign(
                    &mut result,
                    mcpd_core::SignalType::AspectNote,
                    index,
                    "aspect note",
                    id,
                );
                continue;
            }
            accepted += 1;
        }

        // The filter box, in the same transaction as the signals. A value it
        // offers that no row carries, or a row whose value it does not offer,
        // are both states a reader cannot make sense of.
        //
        // `GREATEST` of the two rather than the incoming value alone: signals
        // arrive out of order, and a batch exported late carries an older
        // arrival time than one already written, so taking the new value
        // unconditionally would walk `last_seen` backwards.
        let seen = now_nanos();
        for f in &facets {
            tx.execute(
                "INSERT INTO discovery (project, facet, key, value, last_seen)
                 VALUES ($1,$2,$3,$4,$5)
                 ON CONFLICT (project, facet, key, value)
                 DO UPDATE SET last_seen = GREATEST(discovery.last_seen, EXCLUDED.last_seen)",
                &[&f.project, &f.facet.as_str(), &f.key, &f.value, &seen],
            )
            .await
            .map_err(be)?;
        }

        for (project, t) in &touched {
            Self::recompute_trace(&tx, project, t).await?;
        }
        for (project, t) in &scored {
            Self::recompute_scores(&tx, project, t).await?;
        }
        for (project, t) in &fed {
            Self::recompute_feedback(&tx, project, t).await?;
        }

        tx.commit().await.map_err(be)?;
        result.accepted = accepted;
        Ok(result)
    }

    async fn list_traces(&self, q: &TraceQuery) -> Result<Paged<Trace>> {
        let conn = self.conn().await?;
        let mut w = Where::default();
        w.push("t.project = {}", q.project.clone());

        if let Some(s) = q.status {
            w.push("t.status = {}", s.as_str().to_owned());
        }
        if let Some(k) = q.kind {
            w.push("t.kind = {}", k.as_str().to_owned());
        }
        if let Some(n) = &q.name {
            w.push("t.name = {}", n.clone());
        }
        if let Some(v) = &q.session_id {
            w.push("t.session_id = {}", v.clone());
        }
        if let Some(v) = &q.thread_id {
            w.push("t.thread_id = {}", v.clone());
        }
        if let Some(v) = &q.user_id {
            w.push("t.user_id = {}", v.clone());
        }
        if let Some(v) = &q.environment {
            w.push("t.environment = {}", v.clone());
        }
        if let Some(v) = &q.experiment_id {
            w.push("t.experiment_id = {}", v.clone());
        }
        if let Some(t) = q.from {
            w.push("t.start_time >= {}", to_nanos(t));
        }
        if let Some(t) = q.to {
            w.push("t.start_time < {}", to_nanos(t));
        }
        if let Some(ms) = q.min_duration_ms {
            w.push("t.duration_ns >= {}", (ms as i64).saturating_mul(1_000_000));
        }
        if let Some(c) = q.min_cost {
            w.push("t.total_cost >= {}", c);
        }
        if let Some(ms) = q.max_duration_ms {
            w.push("t.duration_ns <= {}", (ms as i64).saturating_mul(1_000_000));
        }
        if let Some(c) = q.max_cost {
            w.push("t.total_cost <= {}", c);
        }
        if let Some(n) = q.min_tokens {
            w.push("(t.input_tokens + t.output_tokens) >= {}", n as i64);
        }
        if let Some(has) = q.has_feedback {
            w.push_raw(format!(
                "{}EXISTS (SELECT 1 FROM feedback f WHERE f.project = t.project
                         AND f.trace_id = t.trace_id)",
                if has { "" } else { "NOT " }
            ));
        }
        if let Some(m) = &q.model {
            w.push(
                "EXISTS (SELECT 1 FROM spans s WHERE s.project = t.project
                         AND s.trace_id = t.trace_id AND s.model = {})",
                m.clone(),
            );
        }
        for tag in &q.tags {
            // Tags are a JSON array in a TEXT column, matching the embedded
            // store. `@>` on a jsonb cast is an exact containment test, so a tag
            // named `pay` cannot match a trace tagged `payment`.
            w.push("t.tags::jsonb @> {}::jsonb", serde_json::json!([tag]).to_string());
        }
        if let Some(text) = q.q.as_deref().and_then(search_text) {
            w.push(
                "EXISTS (SELECT 1 FROM spans s WHERE s.project = t.project
                         AND s.trace_id = t.trace_id
                         AND s.search @@ plainto_tsquery('simple', {}))",
                text,
            );
        }
        if let Some(scorer) = &q.has_score {
            // Both bounds on the same score row, as on the embedded store.
            let n = w.next();
            let mut clause = format!(
                "EXISTS (SELECT 1 FROM scores sc WHERE sc.project = t.project
                         AND sc.trace_id = t.trace_id AND sc.scorer_id = ${n}"
            );
            w.bind(scorer.clone());
            if let Some(v) = q.score_below {
                clause.push_str(&format!(" AND sc.value < ${}", w.next()));
                w.bind(v);
            }
            if let Some(v) = q.score_above {
                clause.push_str(&format!(" AND sc.value > ${}", w.next()));
                w.bind(v);
            }
            clause.push(')');
            w.push_raw(clause);
        }

        // The score sort reads `min_score`, which the projection maintains as
        // scores arrive, so ordering by quality costs no join. COALESCE is what
        // makes an unscored run rank as a zero rather than sorting to whichever
        // end this particular database puts a NULL, which is not the same end
        // in Postgres as it is in SQLite.
        let sort_col = match q.sort {
            TraceSort::StartTime => "t.start_time",
            TraceSort::Duration => "COALESCE(t.duration_ns, 0)",
            TraceSort::Cost => "t.total_cost",
            TraceSort::SpanCount => "t.span_count",
            TraceSort::Tokens => "(t.input_tokens + t.output_tokens)",
            TraceSort::Score => "COALESCE(t.min_score, 0)",
        };
        let dir = match q.order {
            Order::Asc => "ASC",
            Order::Desc => "DESC",
        };

        // Keyset pagination. OFFSET would drift as new traces arrive during
        // paging: the reader would see the same row twice or miss one entirely.
        if let Some(c) = &q.cursor {
            let cmp = if q.order == Order::Asc { '>' } else { '<' };
            let n = w.next();
            // The sort value is compared in the column's own type. Cost and
            // score stay floats: the cursor used to carry cost scaled to an
            // integer, which dropped every digit under a millionth, so the walk
            // resumed from a value below the row it came from and was handed
            // that row again on every page.
            match q.sort {
                TraceSort::Cost | TraceSort::Score => {
                    let (v, id) = decode_cursor_f64(c)?;
                    w.push_raw(format!(
                        "({sort_col}, t.trace_id) {cmp} (${n}::double precision, ${})",
                        n + 1
                    ));
                    w.bind(v);
                    w.bind(id);
                }
                _ => {
                    let (v, id) = decode_cursor(c)?;
                    w.push_raw(format!(
                        "({sort_col}, t.trace_id) {cmp} (${n}::bigint, ${})",
                        n + 1
                    ));
                    w.bind(v);
                    w.bind(id);
                }
            }
        }

        let limit = q.limit.clamp(1, 500);
        // `min_score` is a column of the projection, not a field of the trace
        // the caller gets back, so the score cursor is selected beside the row.
        // Without it the next page would have to guess the value it resumes
        // from.
        let scoring = q.sort == TraceSort::Score;
        let extra =
            if scoring { ", COALESCE(t.min_score, 0::double precision) AS min_score" } else { "" };
        let sql = format!(
            "SELECT {TRACE_COLUMNS}{extra} FROM traces t{} \
             ORDER BY {sort_col} {dir}, t.trace_id {dir} LIMIT {}",
            w.sql(),
            limit + 1
        );

        let rows = conn.query(&sql, &w.params()).await.map_err(be)?;
        let mut items: Vec<(Trace, f64)> = rows
            .iter()
            .map(|r| (row_to_trace(r), if scoring { r.get("min_score") } else { 0.0 }))
            .collect();

        let next_cursor = if items.len() > limit as usize {
            items.truncate(limit as usize);
            items.last().map(|(t, min_score)| {
                encode_cursor(q.sort.cursor_value(t, *min_score), &t.trace_id)
            })
        } else {
            None
        };

        let mut items: Vec<Trace> = items.into_iter().map(|(t, _)| t).collect();
        if q.previews {
            pg_fill_previews(&conn, &q.project, &mut items).await?;
        }
        Ok(Paged { items, next_cursor, total: None })
    }

    async fn get_trace(
        &self,
        project: &str,
        trace_id: &str,
        include: &Include,
    ) -> Result<TraceDetail> {
        let conn = self.conn().await?;
        let row = conn
            .query_opt(
                &format!("SELECT {TRACE_COLUMNS} FROM traces WHERE trace_id = $1 AND project = $2"),
                &[&trace_id, &project],
            )
            .await
            .map_err(be)?;
        let Some(row) = row else {
            return Err(Error::NotFound { kind: "trace", id: trace_id.to_owned() });
        };
        let trace = row_to_trace(&row);

        let spans = conn
            .query(
                &format!(
                    "SELECT {SPAN_COLUMNS} FROM spans WHERE trace_id = $1 AND project = $2
                     ORDER BY start_time ASC, span_id ASC"
                ),
                &[&trace_id, &project],
            )
            .await
            .map_err(be)?
            .iter()
            .map(row_to_span)
            .collect();

        let logs = if include.logs {
            Some(
                conn.query(
                    &format!(
                        "SELECT {LOG_COLUMNS} FROM logs WHERE trace_id = $1 AND project = $2
                         ORDER BY timestamp ASC"
                    ),
                    &[&trace_id, &project],
                )
                .await
                .map_err(be)?
                .iter()
                .map(row_to_log)
                .collect(),
            )
        } else {
            None
        };

        let scores = if include.scores {
            Some(
                conn.query(
                    &format!(
                        "SELECT {SCORE_COLUMNS} FROM scores WHERE trace_id = $1 AND project = $2
                         ORDER BY timestamp ASC"
                    ),
                    &[&trace_id, &project],
                )
                .await
                .map_err(be)?
                .iter()
                .map(row_to_score)
                .collect(),
            )
        } else {
            None
        };

        let feedback = if include.feedback {
            Some(
                conn.query(
                    &format!(
                        "SELECT {FEEDBACK_COLUMNS} FROM feedback WHERE trace_id = $1 AND project = $2
                         ORDER BY timestamp ASC"
                    ),
                    &[&trace_id, &project],
                )
                .await
                .map_err(be)?
                .iter()
                .map(row_to_feedback)
                .collect(),
            )
        } else {
            None
        };

        Ok(TraceDetail { trace, spans, logs, scores, feedback })
    }

    async fn delete_trace(&self, project: &str, trace_id: &str) -> Result<u32> {
        let mut conn = self.conn().await?;
        let tx = conn.transaction().await.map_err(be)?;

        // Confirm the trace is in this project before deleting anything. Without
        // it, a trace id learned from one project would erase it from another.
        let owned = tx
            .query_opt(
                "SELECT 1 FROM traces WHERE trace_id = $1 AND project = $2",
                &[&trace_id, &project],
            )
            .await
            .map_err(be)?;
        if owned.is_none() {
            return Err(Error::NotFound { kind: "trace", id: trace_id.to_owned() });
        }

        // Every delete names the project. The ownership check above passing
        // does not make the trace id this project's alone: another project may
        // hold a run under the same id, and it keeps every row.
        let mut removed = 0_u64;
        for table in ["spans", "logs", "metrics", "scores", "feedback", "aspect_notes", "traces"] {
            removed += tx
                .execute(
                    &format!("DELETE FROM {table} WHERE trace_id = $1 AND project = $2"),
                    &[&trace_id, &project],
                )
                .await
                .map_err(be)?;
        }
        // Membership rows are not counted. They are ids pointing at a note that
        // has just gone, so leaving one behind would make a theme claim a run
        // that is no longer stored; but they are bookkeeping and not a signal
        // somebody sent, and counting them would inflate what this endpoint
        // reports as deleted.
        tx.execute(
            "DELETE FROM theme_members WHERE trace_id = $1 AND project = $2",
            &[&trace_id, &project],
        )
        .await
        .map_err(be)?;
        tx.commit().await.map_err(be)?;
        Ok(u32::try_from(removed).unwrap_or(u32::MAX))
    }

    async fn erase_subject(&self, project: &str, user_id: &str) -> Result<ErasureOutcome> {
        let mut conn = self.conn().await?;
        let tx = conn.transaction().await.map_err(be)?;

        // The runs in scope, resolved once before anything is deleted.
        //
        // The set comes from `traces.user_id` and `spans.user_id`, so a
        // subquery against those tables would shrink as the deletes ran and
        // each statement would see a smaller set than the one before it.
        //
        // `ON COMMIT DROP` is what keeps this safe on a pooled connection: the
        // temporary table belongs to the session, not the transaction, so
        // without it the next erasure on the same pooled connection would find
        // the previous subject's runs still sitting there.
        tx.execute(
            "CREATE TEMPORARY TABLE erasure_targets ON COMMIT DROP AS
             SELECT trace_id FROM traces WHERE project = $1 AND user_id = $2
             UNION
             SELECT trace_id FROM spans  WHERE project = $1 AND user_id = $2",
            &[&project, &user_id],
        )
        .await
        .map_err(be)?;

        // The subject's span ids, taken while the spans still exist.
        //
        // A score, a piece of feedback, a metric point or a log line can name a
        // `span_id` and no `trace_id`. Those rows hang off the subject's step
        // and hold the subject's content, and matching on the trace alone left
        // every one of them on disk.
        tx.execute(
            "CREATE TEMPORARY TABLE erasure_spans ON COMMIT DROP AS
             SELECT span_id FROM spans WHERE project = $1
             AND trace_id IN (SELECT trace_id FROM erasure_targets)",
            &[&project],
        )
        .await
        .map_err(be)?;

        // Every statement carries `project = $1`. That is the tenancy guard:
        // the same subject in a project this caller was not authorised for
        // keeps its rows and contributes nothing to the counts.
        //
        // Postgres keeps the searchable text in a generated `tsvector` column
        // on the row itself, so deleting the row removes it from the index.
        // There is no separate index table to clear, unlike SQLite.
        //
        // `ATTACHED` is written once so a signal cannot end up with a narrower
        // reach than its neighbour by accident.
        //
        // A span id is unique inside its trace, not inside the project, so
        // `span_id IN (...)` on its own reached any other run in the same
        // project that happened to reuse the id and took that run's scores and
        // logs with it. The span reach is therefore limited to rows that name
        // no trace of their own. A row that names a trace is judged by that
        // trace, and a row that names none is the one the span reach exists
        // for. Such a row is still ambiguous when two runs share the span id,
        // and it goes: it may hold the subject's content and carries nothing
        // else to decide on.
        const ATTACHED: &str = "(trace_id IN (SELECT trace_id FROM erasure_targets)
             OR (coalesce(trace_id,'') = ''
                 AND span_id IN (SELECT span_id FROM erasure_spans)))";
        let count = |n: u64| u32::try_from(n).unwrap_or(u32::MAX);

        let spans = count(
            tx.execute(
                "DELETE FROM spans WHERE project = $1
                 AND trace_id IN (SELECT trace_id FROM erasure_targets)",
                &[&project],
            )
            .await
            .map_err(be)?,
        );
        // One statement per signal, so a row that is attached to an erased run
        // and also stamped with the subject is counted once. A log can carry
        // the subject with no trace to hang it on, which is why the last arm
        // exists at all.
        let logs = count(
            tx.execute(
                &format!("DELETE FROM logs WHERE project = $1 AND ({ATTACHED} OR user_id = $2)"),
                &[&project, &user_id],
            )
            .await
            .map_err(be)?,
        );
        // Metrics and scores carry no identifier of their own. They are
        // reachable through the run or the step they hang off, which is why
        // both reaches are needed and neither is enough alone.
        let metrics = count(
            tx.execute(
                &format!("DELETE FROM metrics WHERE project = $1 AND {ATTACHED}"),
                &[&project],
            )
            .await
            .map_err(be)?,
        );
        let scores = count(
            tx.execute(
                &format!("DELETE FROM scores WHERE project = $1 AND {ATTACHED}"),
                &[&project],
            )
            .await
            .map_err(be)?,
        );
        // Feedback has an author, hashed by the same rule as `user_id`, and the
        // row holds that person's own words. Feedback the subject wrote about
        // somebody else's run is still the subject's prose, so the author is
        // matched wherever the row hangs.
        let feedback = count(
            tx.execute(
                &format!(
                    "DELETE FROM feedback WHERE project = $1
                     AND ({ATTACHED} OR author_id = $2)"
                ),
                &[&project, &user_id],
            )
            .await
            .map_err(be)?,
        );
        // An aspect note is one sentence about what a user asked for, so it is
        // the most direct copy of a person's own words in this schema after the
        // span payloads themselves. It is matched by the run it describes and
        // by the subject stamped on it, exactly as a log line is, so a note
        // about somebody else's run that carries this subject goes too.
        //
        // Matched on the trace and the subject, and not through `ATTACHED`: a
        // note names a trace and never a step, so the span half of that
        // predicate would be a reference to a column this table does not have.
        let aspect_notes = count(
            tx.execute(
                "DELETE FROM aspect_notes WHERE project = $1
                 AND (trace_id IN (SELECT trace_id FROM erasure_targets) OR user_id = $2)",
                &[&project, &user_id],
            )
            .await
            .map_err(be)?,
        );
        // Membership rows hold no prose, but they hold the note id and the
        // trace id of a sentence that has just been erased. Left behind, a
        // theme would still claim a run nothing can open.
        tx.execute(
            "DELETE FROM theme_members WHERE project = $1
             AND trace_id IN (SELECT trace_id FROM erasure_targets)",
            &[&project],
        )
        .await
        .map_err(be)?;
        // A theme's label is not a summary written for the screen: it is the
        // text of one of the theme's own notes, so a theme labelled from an
        // erased run holds that person's words in a column an erasure would
        // otherwise miss. The label is emptied rather than the theme deleted,
        // for the same reason a captured dataset case is emptied. Clearing
        // `label_trace_id` with it is what makes a repeat erasure a no-op.
        tx.execute(
            &format!(
                "UPDATE themes SET label = '{ERASED_ITEM}', label_note_id = '',
                        label_trace_id = ''
                  WHERE project = $1
                    AND label_trace_id IN (SELECT trace_id FROM erasure_targets)"
            ),
            &[&project],
        )
        .await
        .map_err(be)?;

        let traces = count(
            tx.execute(
                "DELETE FROM traces WHERE project = $1
                 AND trace_id IN (SELECT trace_id FROM erasure_targets)",
                &[&project],
            )
            .await
            .map_err(be)?,
        );

        // A captured case keeps a verbatim copy of a production input, so
        // erasing the run and leaving the copy would leave the subject's words
        // in the dataset. The case is emptied rather than deleted: somebody put
        // it there to keep a bug from coming back, and destroying it silently
        // is its own harm. `source_trace_id` is cleared with the payload, which
        // is also what makes a repeat erasure a no-op instead of reporting the
        // same cases again.
        const CAPTURED: &str = "source_trace_id IN (SELECT trace_id FROM erasure_targets)
             AND dataset_id IN (SELECT dataset_id FROM datasets WHERE project = $1)";
        let datasets: Vec<String> = tx
            .query(
                &format!("SELECT DISTINCT dataset_id FROM dataset_items WHERE {CAPTURED}"),
                &[&project],
            )
            .await
            .map_err(be)?
            .iter()
            .map(|r| r.get::<_, String>(0))
            .collect();
        let dataset_items = count(
            tx.execute(
                &format!(
                    "UPDATE dataset_items
                        SET input = '{ERASED_ITEM}', expected = NULL,
                            metadata = '{ERASED_ITEM}', source_trace_id = NULL
                      WHERE {CAPTURED}"
                ),
                &[&project],
            )
            .await
            .map_err(be)?,
        );

        tx.commit().await.map_err(be)?;

        // The probe runs after the commit, on the same connection.
        //
        // It used to run inside the transaction, and in the worst case it reads
        // every identifier in the project. Holding a write transaction open for
        // that scan takes row locks with it and blocks the ingest that wants
        // the same rows. Outside it, the scan is an ordinary read.
        //
        // The trade is that the probe describes the project a moment after the
        // erasure rather than at the instant it committed, so a write that
        // lands in between is included. That is the right way round: the probe
        // says what is still here, and a row written a moment ago is still
        // here. It says nothing about what this request deleted, which is what
        // the counts are for.
        let identifiers = probe_identifiers(&conn, project, user_id).await?;

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
            // Postgres deletes inside the transaction this call committed.
            deferred: false,
        })
    }

    async fn list_spans(&self, q: &SpanQuery) -> Result<Paged<Span>> {
        let conn = self.conn().await?;
        let mut w = Where::default();
        w.push("project = {}", q.project.clone());
        if let Some(k) = q.kind {
            w.push("kind = {}", k.as_str().to_owned());
        }
        if let Some(s) = q.status {
            w.push("status = {}", s.as_str().to_owned());
        }
        if let Some(n) = &q.name {
            w.push("name = {}", n.clone());
        }
        if let Some(m) = &q.model {
            w.push("model = {}", m.clone());
        }
        if let Some(t) = &q.trace_id {
            w.push("trace_id = {}", t.clone());
        }
        if let Some(t) = q.from {
            w.push("start_time >= {}", to_nanos(t));
        }
        if let Some(t) = q.to {
            w.push("start_time < {}", to_nanos(t));
        }
        if let Some(ms) = q.min_duration_ms {
            w.push("duration_ns >= {}", (ms as i64).saturating_mul(1_000_000));
        }
        // Compared as text, so a version an SDK sent as a number and one sent
        // as a string both match.
        if let Some(p) = &q.prompt {
            w.push(
                &format!("(attributes::jsonb ->> '{}') = {{}}", crate::PROMPT_NAME_ATTRIBUTE),
                p.clone(),
            );
            if let Some(v) = q.prompt_version {
                w.push(
                    &format!(
                        "(attributes::jsonb ->> '{}') = {{}}",
                        crate::PROMPT_VERSION_ATTRIBUTE
                    ),
                    v.to_string(),
                );
            }
        }
        if let Some(text) = q.q.as_deref().and_then(search_text) {
            w.push("search @@ plainto_tsquery('simple', {})", text);
        }
        // Both alternatives to `start_time` are nullable, and Postgres and
        // SQLite put a NULL at opposite ends of the same ORDER BY. COALESCE
        // decides it here instead, so the two backends answer the same question
        // the same way.
        let sort_col = match q.sort {
            SpanSort::StartTime => "start_time",
            SpanSort::Duration => "COALESCE(duration_ns, 0)",
            SpanSort::Cost => "COALESCE(total_cost, 0)",
        };

        if let Some(c) = &q.cursor {
            let cmp = if q.order == Order::Asc { '>' } else { '<' };
            let n = w.next();
            // Cost is compared as the float it is stored as, for the reason set
            // out over the trace listing: a rounded cursor either repeats a row
            // or skips one.
            match q.sort {
                SpanSort::Cost => {
                    let (v, id) = decode_cursor_f64(c)?;
                    w.push_raw(format!(
                        "({sort_col}, span_id) {cmp} (${n}::double precision, ${})",
                        n + 1
                    ));
                    w.bind(v);
                    w.bind(id);
                }
                _ => {
                    let (v, id) = decode_cursor(c)?;
                    w.push_raw(format!("({sort_col}, span_id) {cmp} (${n}::bigint, ${})", n + 1));
                    w.bind(v);
                    w.bind(id);
                }
            }
        }

        let dir = match q.order {
            Order::Asc => "ASC",
            Order::Desc => "DESC",
        };
        let limit = q.limit.clamp(1, 500);
        let sql = format!(
            "SELECT {SPAN_COLUMNS} FROM spans{} ORDER BY {sort_col} {dir}, span_id {dir} LIMIT {}",
            w.sql(),
            limit + 1
        );
        let rows = conn.query(&sql, &w.params()).await.map_err(be)?;
        let mut items: Vec<Span> = rows.iter().map(row_to_span).collect();

        let next_cursor = if items.len() > limit as usize {
            items.truncate(limit as usize);
            items.last().map(|s| encode_cursor(q.sort.cursor_value(s), &s.span_id))
        } else {
            None
        };
        Ok(Paged { items, next_cursor, total: None })
    }

    async fn list_logs(&self, q: &LogQuery) -> Result<Paged<LogRecord>> {
        let conn = self.conn().await?;
        let mut w = Where::default();
        w.push("project = {}", q.project.clone());
        if let Some(l) = q.level {
            w.push("severity >= {}", i64::from(l.severity()));
        }
        if let Some(t) = &q.trace_id {
            w.push("trace_id = {}", t.clone());
        }
        if let Some(s) = &q.span_id {
            w.push("span_id = {}", s.clone());
        }
        for (field, value) in &q.correlation {
            // Only a name from the fixed list reaches the SQL.
            if let Some(col) = crate::LOG_CORRELATION_FILTERS.iter().find(|c| **c == field.as_str())
            {
                w.push(&format!("{col} = {{}}"), value.clone());
            }
        }
        if let Some(t) = q.from {
            w.push("timestamp >= {}", to_nanos(t));
        }
        if let Some(t) = q.to {
            w.push("timestamp < {}", to_nanos(t));
        }
        if let Some(text) = q.q.as_deref().and_then(search_text) {
            w.push("search @@ plainto_tsquery('simple', {})", text);
        }
        if let Some(c) = &q.cursor {
            let (v, id) = decode_cursor(c)?;
            let cmp = if q.order == Order::Asc { '>' } else { '<' };
            let n = w.next();
            w.push_raw(format!("(timestamp, log_id) {cmp} (${n}::bigint, ${})", n + 1));
            w.bind(v);
            w.bind(id);
        }

        let dir = match q.order {
            Order::Asc => "ASC",
            Order::Desc => "DESC",
        };
        let limit = q.limit.clamp(1, 500);
        let sql = format!(
            "SELECT {LOG_COLUMNS} FROM logs{} ORDER BY timestamp {dir}, log_id {dir} LIMIT {}",
            w.sql(),
            limit + 1
        );
        let rows = conn.query(&sql, &w.params()).await.map_err(be)?;
        let mut items: Vec<LogRecord> = rows.iter().map(row_to_log).collect();

        let next_cursor = if items.len() > limit as usize {
            items.truncate(limit as usize);
            items.last().map(|l| encode_cursor(to_nanos(l.timestamp), &l.log_id))
        } else {
            None
        };
        Ok(Paged { items, next_cursor, total: None })
    }

    async fn list_feedback(&self, q: &crate::FeedbackQuery) -> Result<Paged<Feedback>> {
        let conn = self.conn().await?;
        let mut w = Where::default();
        w.push("project = {}", q.project.clone());
        if let Some(t) = &q.trace_id {
            w.push("trace_id = {}", t.clone());
        }
        if let Some(s) = &q.span_id {
            w.push("span_id = {}", s.clone());
        }
        if let Some(k) = &q.kind {
            w.push("kind = {}", k.clone());
        }
        if let Some(a) = &q.author_kind {
            w.push("author_kind = {}", a.clone());
        }
        if let Some(t) = q.from {
            w.push("timestamp >= {}", to_nanos(t));
        }
        if let Some(t) = q.to {
            w.push("timestamp < {}", to_nanos(t));
        }
        if let Some(c) = &q.cursor {
            let (v, id) = decode_cursor(c)?;
            let n = w.next();
            w.push_raw(format!("(timestamp, feedback_id) < (${n}::bigint, ${})", n + 1));
            w.bind(v);
            w.bind(id);
        }
        let limit = q.limit.clamp(1, 500);
        let sql = format!(
            "SELECT {FEEDBACK_COLUMNS} FROM feedback{} ORDER BY timestamp DESC, feedback_id DESC \
             LIMIT {}",
            w.sql(),
            limit + 1
        );
        let rows = conn.query(&sql, &w.params()).await.map_err(be)?;
        let mut items: Vec<Feedback> = rows.iter().map(row_to_feedback).collect();
        let next_cursor = if items.len() > limit as usize {
            items.truncate(limit as usize);
            items.last().map(|f| encode_cursor(to_nanos(f.timestamp), &f.feedback_id))
        } else {
            None
        };
        Ok(Paged { items, next_cursor, total: None })
    }

    async fn list_metric_points(
        &self,
        q: &crate::MetricPointQuery,
    ) -> Result<Paged<mcpd_core::MetricPoint>> {
        let conn = self.conn().await?;
        let mut w = Where::default();
        w.push("project = {}", q.project.clone());
        if let Some(n) = &q.name {
            w.push("name = {}", n.clone());
        }
        if let Some(t) = &q.trace_id {
            w.push("trace_id = {}", t.clone());
        }
        if let Some(t) = q.from {
            w.push("timestamp >= {}", to_nanos(t));
        }
        if let Some(t) = q.to {
            w.push("timestamp < {}", to_nanos(t));
        }
        for (k, v) in &q.filter {
            let n = w.next();
            w.push_raw(format!("labels::jsonb ->> ${n} = ${}", n + 1));
            w.bind(k.clone());
            w.bind(v.clone());
        }
        if let Some(c) = &q.cursor {
            let (v, id) = decode_cursor(c)?;
            let cmp = if q.order == Order::Asc { '>' } else { '<' };
            let n = w.next();
            w.push_raw(format!("(timestamp, metric_id) {cmp} (${n}::bigint, ${})", n + 1));
            w.bind(v);
            w.bind(id);
        }
        let dir = if q.order == Order::Asc { "ASC" } else { "DESC" };
        let limit = q.limit.clamp(1, 10_000);
        let sql = format!(
            "SELECT metric_id, name, kind, value, unit, timestamp, labels, buckets, trace_id, \
                    span_id, project, environment, received_at
             FROM metrics{} ORDER BY timestamp {dir}, metric_id {dir} LIMIT {}",
            w.sql(),
            limit + 1
        );
        let rows = conn.query(&sql, &w.params()).await.map_err(be)?;
        let mut items: Vec<mcpd_core::MetricPoint> = rows
            .iter()
            .map(|r| mcpd_core::MetricPoint {
                metric_id: r.get(0),
                name: r.get(1),
                kind: r.get::<_, &str>(2).parse().unwrap_or(mcpd_core::MetricKind::Gauge),
                value: r.get(3),
                unit: r.get(4),
                timestamp: from_nanos(r.get(5)),
                labels: json_in(r.get(6)),
                buckets: json_in(r.get(7)),
                trace_id: r.get(8),
                span_id: r.get(9),
                correlation: mcpd_core::Correlation::for_project(r.get(10), r.get(11)),
                received_at: from_nanos(r.get(12)),
            })
            .collect();
        let next_cursor = if items.len() > limit as usize {
            items.truncate(limit as usize);
            items.last().map(|m| encode_cursor(to_nanos(m.timestamp), &m.metric_id))
        } else {
            None
        };
        Ok(Paged { items, next_cursor, total: None })
    }

    async fn query_metrics(&self, q: &MetricQuery) -> Result<MetricSeriesResponse> {
        let conn = self.conn().await?;
        let bucket = if q.bucket_seconds > 0 { q.bucket_seconds } else { 60 };
        let bucket_ns = i64::from(bucket).saturating_mul(1_000_000_000);

        let mut w = Where::default();
        w.push("project = {}", q.project.clone());
        w.push("name = {}", q.name.clone());
        w.push("timestamp >= {}", to_nanos(q.from));
        w.push("timestamp < {}", to_nanos(q.to));
        for (k, v) in &q.filter {
            // The label name is a caller-supplied string, so it is bound as a
            // parameter and compared at an exact JSON key. Interpolating it
            // would make a label name into SQL.
            let n = w.next();
            w.push_raw(format!("labels::jsonb ->> ${n} = ${}", n + 1));
            w.bind(k.clone());
            w.bind(v.clone());
        }

        // Group keys are bound the same way. The series key is the group values
        // joined by a unit separator, which cannot occur in a label value.
        let key_expr = if q.group_by.is_empty() {
            "''".to_owned()
        } else {
            let mut parts = Vec::new();
            for k in &q.group_by {
                let n = w.next();
                parts.push(format!("COALESCE(labels::jsonb ->> ${n}, '(none)')"));
                w.bind(k.clone());
            }
            parts.join(" || chr(31) || ")
        };

        // The aggregate comes from a fixed set and never from input. Postgres
        // has exact quantiles, so unlike a sketch these are the real values.
        let agg = match q.agg {
            Aggregation::Sum => "SUM(value)",
            Aggregation::Count => "COUNT(*)::double precision",
            Aggregation::Mean => "AVG(value)",
            Aggregation::Min => "MIN(value)",
            Aggregation::Max => "MAX(value)",
            Aggregation::P50 => "percentile_cont(0.5) WITHIN GROUP (ORDER BY value)",
            Aggregation::P90 => "percentile_cont(0.9) WITHIN GROUP (ORDER BY value)",
            Aggregation::P95 => "percentile_cont(0.95) WITHIN GROUP (ORDER BY value)",
            Aggregation::P99 => "percentile_cont(0.99) WITHIN GROUP (ORDER BY value)",
        };

        let sql = format!(
            "SELECT (timestamp / {bucket_ns}) * {bucket_ns} AS b,
                    {key_expr} AS series_key,
                    ({agg})::double precision AS v,
                    COUNT(*)::bigint AS n,
                    MAX(trace_id) AS exemplar
             FROM metrics{}
             GROUP BY b, series_key ORDER BY b ASC LIMIT 20000",
            w.sql()
        );
        let rows = conn.query(&sql, &w.params()).await.map_err(be)?;

        // The unit is read from any row of this metric rather than aggregated:
        // a metric that changed unit mid-window is a bug in the producer, and
        // reporting one of them is more useful than reporting none.
        let unit: Option<String> = conn
            .query_opt(
                "SELECT unit FROM metrics WHERE project = $1 AND name = $2 AND unit IS NOT NULL
                 LIMIT 1",
                &[&q.project, &q.name],
            )
            .await
            .map_err(be)?
            .and_then(|r| r.get(0));

        let mut series: Vec<MetricSeries> = Vec::new();
        let mut index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for r in &rows {
            let series_key: String = r.get(1);
            let idx = *index.entry(series_key.clone()).or_insert_with(|| {
                let labels = if q.group_by.is_empty() {
                    None
                } else {
                    let parts: Vec<&str> = series_key.split('\u{1f}').collect();
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
            let v: Option<f64> = r.get(2);
            let exemplar: Option<String> = r.get(4);
            series[idx].points.push(MetricSeriesPoint {
                timestamp: from_nanos(r.get(0)),
                value: v.unwrap_or(0.0),
                count: Some(r.get::<_, i64>(3).max(0) as u64),
                exemplar_trace_id: exemplar.filter(|e| !e.is_empty()),
            });
        }

        Ok(MetricSeriesResponse {
            name: q.name.clone(),
            unit,
            bucket: format!("{bucket}s"),
            series,
        })
    }

    async fn list_scores(&self, q: &ScoreQuery) -> Result<Paged<Score>> {
        let conn = self.conn().await?;
        let mut w = Where::default();
        w.push("project = {}", q.project.clone());
        if let Some(s) = &q.scorer_id {
            w.push("scorer_id = {}", s.clone());
        }
        if let Some(s) = q.source {
            w.push("source = {}", s.as_str().to_owned());
        }
        if let Some(t) = &q.trace_id {
            w.push("trace_id = {}", t.clone());
        }
        if let Some(e) = &q.experiment_id {
            w.push("experiment_id = {}", e.clone());
        }
        if let Some(v) = q.min_value {
            w.push("value >= {}", v);
        }
        if let Some(v) = q.max_value {
            w.push("value <= {}", v);
        }
        if let Some(t) = q.from {
            w.push("timestamp >= {}", to_nanos(t));
        }
        if let Some(t) = q.to {
            w.push("timestamp < {}", to_nanos(t));
        }
        if let Some(c) = &q.cursor {
            let (v, id) = decode_cursor(c)?;
            let n = w.next();
            w.push_raw(format!("(timestamp, score_id) < (${n}::bigint, ${})", n + 1));
            w.bind(v);
            w.bind(id);
        }

        let limit = q.limit.clamp(1, 500);
        let sql = format!(
            "SELECT {SCORE_COLUMNS} FROM scores{} ORDER BY timestamp DESC, score_id DESC LIMIT {}",
            w.sql(),
            limit + 1
        );
        let rows = conn.query(&sql, &w.params()).await.map_err(be)?;
        let mut items: Vec<Score> = rows.iter().map(row_to_score).collect();

        let next_cursor = if items.len() > limit as usize {
            items.truncate(limit as usize);
            items.last().map(|s| encode_cursor(to_nanos(s.timestamp), &s.score_id))
        } else {
            None
        };
        Ok(Paged { items, next_cursor, total: None })
    }

    async fn analytics_summary(
        &self,
        project: &str,
        from: OffsetDateTime,
        to: OffsetDateTime,
        group_by: Option<&str>,
    ) -> Result<AnalyticsSummary> {
        let conn = self.conn().await?;
        let (f, t) = (to_nanos(from), to_nanos(to));

        // Postgres has exact quantiles, so these are the real percentiles and
        // not a sketch. `percentile_cont` returns NULL on an empty window,
        // which stays NULL rather than becoming a zero that reads as "fast".
        let row = conn
            .query_one(
                "SELECT COUNT(*)::bigint,
                        COALESCE(SUM(span_count), 0)::bigint,
                        COUNT(*) FILTER (WHERE status = 'error')::bigint,
                        COALESCE(SUM(input_tokens), 0)::bigint,
                        COALESCE(SUM(output_tokens), 0)::bigint,
                        COALESCE(SUM(total_cost), 0)::double precision,
                        percentile_cont(0.5) WITHIN GROUP (ORDER BY duration_ns),
                        percentile_cont(0.9) WITHIN GROUP (ORDER BY duration_ns),
                        percentile_cont(0.95) WITHIN GROUP (ORDER BY duration_ns),
                        percentile_cont(0.99) WITHIN GROUP (ORDER BY duration_ns),
                        AVG(duration_ns)::double precision,
                        MAX(duration_ns)::double precision
                 FROM traces
                 WHERE project = $1 AND start_time >= $2 AND start_time < $3",
                &[&project, &f, &t],
            )
            .await
            .map_err(be)?;

        let trace_count = row.get::<_, i64>(0).max(0) as u64;
        let error_count = row.get::<_, i64>(2).max(0) as u64;

        let groups = match group_by {
            Some(dim) => Some(self.grouped(project, from, to, dim).await?),
            None => None,
        };

        Ok(AnalyticsSummary {
            from,
            to,
            trace_count,
            span_count: row.get::<_, i64>(1).max(0) as u64,
            error_count,
            error_rate: if trace_count > 0 { error_count as f64 / trace_count as f64 } else { 0.0 },
            latency: LatencyStats {
                p50: ms(row.get(6)),
                p90: ms(row.get(7)),
                p95: ms(row.get(8)),
                p99: ms(row.get(9)),
                mean: ms(row.get(10)),
                max: ms(row.get(11)),
            },
            input_tokens: row.get::<_, i64>(3).max(0) as u64,
            output_tokens: row.get::<_, i64>(4).max(0) as u64,
            total_cost: row.get(5),
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
        let conn = self.conn().await?;
        let (f, t) = (to_nanos(from), to_nanos(to));

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
                // The same table as the totals above, so the bars and the table
                // beside them count the same thing.
                let (col, table) = group_column(group_by)?;
                let named = if table == "spans" {
                    format!("AND coalesce({col}, '') <> ''")
                } else {
                    String::new()
                };
                let bucket_ns = i64::from(bucket).saturating_mul(1_000_000_000);
                let rows = conn
                    .query(
                        &format!(
                            "SELECT (start_time / {bucket_ns}) * {bucket_ns} AS b,
                                    COALESCE(NULLIF({col}, ''), '(none)') AS k,
                                    COALESCE(SUM(total_cost), 0)::double precision,
                                    COALESCE(SUM(input_tokens), 0)::bigint,
                                    COALESCE(SUM(output_tokens), 0)::bigint
                             FROM {table}
                             WHERE project = $1 AND start_time >= $2 AND start_time < $3
                               {named}
                             GROUP BY b, k ORDER BY b ASC LIMIT 5000"
                        ),
                        &[&project, &f, &t],
                    )
                    .await
                    .map_err(be)?;
                Some(
                    rows.iter()
                        .map(|r| CostBucket {
                            timestamp: from_nanos(r.get(0)),
                            key: r.get(1),
                            total_cost: r.get(2),
                            input_tokens: Some(r.get::<_, i64>(3).max(0) as u64),
                            output_tokens: Some(r.get::<_, i64>(4).max(0) as u64),
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
        let key =
            if q.facet.needs_key() { q.key.clone().unwrap_or_default() } else { String::new() };
        let facet = q.facet.as_str();
        let conn = self.conn().await?;

        // `LIKE 'x%'` against the `text_pattern_ops` index, not the range the
        // other two backends use. A range is bounded by the column's collation,
        // and under a linguistic one the upper bound can sort before a value
        // that really does start with the prefix, so the match goes missing
        // from the list without anything failing. `LIKE` is defined on bytes,
        // and `discovery_prefix` indexes the column the same way, so this is
        // both exact and an index scan whatever the cluster's locale is.
        let prefix =
            q.prefix.as_ref().filter(|p| !p.is_empty()).map(|p| format!("{}%", escape_like(p)));
        let mut sql = String::from(
            "SELECT value, last_seen FROM discovery WHERE project = $1 AND facet = $2 AND key = $3",
        );
        let mut args: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> =
            vec![&q.project, &facet, &key];
        if let Some(p) = &prefix {
            sql.push_str(" AND value LIKE $4");
            args.push(p);
        }
        sql.push_str(&format!(" ORDER BY value ASC LIMIT {}", q.effective_limit()));

        let rows = conn.query(&sql, &args).await.map_err(be)?;
        Ok(rows
            .iter()
            .map(|r| Discovered { value: r.get(0), last_seen: from_nanos(r.get::<_, i64>(1)) })
            .collect())
    }

    async fn poll(&self, q: &DeltaQuery) -> Result<Delta<PolledSignal>> {
        let (table, id_col, columns) = match q.signal {
            DeltaSignal::Traces => ("traces", "trace_id", TRACE_COLUMNS),
            DeltaSignal::Spans => ("spans", "span_id", SPAN_COLUMNS),
            DeltaSignal::Logs => ("logs", "log_id", LOG_COLUMNS),
        };
        // No cursor yet: start just before `since`, or at the beginning of
        // time. The id is empty, which sorts before every real id, so the first
        // row at exactly that nanosecond is included rather than skipped.
        let (watermark, after_id) = match &q.cursor {
            Some(c) => decode_delta_cursor(c)?,
            None => (q.since.map_or(i64::MIN, to_nanos), String::new()),
        };
        let limit = match q.limit {
            0 => 100,
            n => n.min(1000),
        };

        // `received_at` and the id come back beside the row rather than out of
        // it: a `Trace` does not carry its arrival time, and a cursor built
        // from `start_time` would resume in the wrong order the first time a
        // run arrived late.
        let sql = format!(
            "SELECT {columns}, received_at AS poll_at, {id_col} AS poll_id FROM {table} \
             WHERE project = $1 AND (received_at > $2 OR (received_at = $2 AND {id_col} > $3)) \
             ORDER BY received_at ASC, {id_col} ASC LIMIT {}",
            limit + 1
        );
        let conn = self.conn().await?;
        let rows = conn.query(&sql, &[&q.project, &watermark, &after_id]).await.map_err(be)?;

        let has_more = rows.len() > limit as usize;
        let taken = rows.iter().take(limit as usize);
        let mut items = Vec::new();
        let mut last: Option<(i64, String)> = None;
        for r in taken {
            last = Some((r.get("poll_at"), r.get("poll_id")));
            items.push(match q.signal {
                DeltaSignal::Traces => PolledSignal::Trace(Box::new(row_to_trace(r))),
                DeltaSignal::Spans => PolledSignal::Span(Box::new(row_to_span(r))),
                DeltaSignal::Logs => PolledSignal::Log(Box::new(row_to_log(r))),
            });
        }
        // Unchanged when nothing came back. Advancing an empty poll to "now" is
        // what would drop a row that lands a moment later with an earlier
        // arrival time.
        let cursor = last.map_or_else(
            || q.cursor.clone().unwrap_or_else(|| encode_delta_cursor(watermark, &after_id)),
            |(at, id)| encode_delta_cursor(at, &id),
        );
        Ok(Delta { items, cursor, has_more })
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
        let mut conn = self.conn().await?;
        let tx = conn.transaction().await.map_err(be)?;

        // Spans and the trace projection go together: a run whose steps have
        // expired is a row that lists a duration and opens onto nothing.
        //
        // The window is measured from when the thing happened, not from when
        // mcpd heard about it: `start_time` for a step and a run, `timestamp`
        // for everything else. A run backfilled an hour after it ran is thirty
        // days old when the run is, which is what somebody setting "keep thirty
        // days of runs" means. `enforce_retention` below measures from arrival
        // instead, and the two are deliberately different methods rather than
        // one with a flag.
        for (days, table, col) in [
            (policy.traces, "spans", "start_time"),
            (policy.traces, "traces", "start_time"),
            (policy.logs, "logs", "timestamp"),
            (policy.metrics, "metrics", "timestamp"),
            (policy.scores, "scores", "timestamp"),
            (policy.feedback, "feedback", "timestamp"),
        ] {
            let Some(cut) = RetentionPolicy::cutoff(days, now).map(to_nanos) else { continue };
            out.removed += tx
                .execute(
                    &format!("DELETE FROM {table} WHERE project = $1 AND {col} < $2"),
                    &[&project, &cut],
                )
                .await
                .map_err(be)?;
        }
        if let Some(cut) = RetentionPolicy::cutoff(policy.traces, now).map(to_nanos) {
            // The idempotency keys expire with the runs they were keying. They
            // catch a retry seconds after the fact and nothing reads one older.
            out.removed += tx
                .execute("DELETE FROM ingest_keys WHERE seen_at < $1", &[&cut])
                .await
                .map_err(be)?;

            // Aspect notes go with the runs, not on a window of their own, and
            // no flag adds one. A note is a sentence about a trace: outliving
            // its trace it is prose about a run nobody can open. It is cut on
            // when the note was written rather than on when its run started,
            // because writing a note lags the run and a note recorded today
            // about a run from last month is still a sentence written today.
            //
            // The whole analysis chain goes on the same window and in
            // dependency order. A theme's label is the text of one of its own
            // notes, so leaving the clustering behind would keep that sentence
            // readable after the note it came from had gone.
            for sql in [
                "DELETE FROM aspect_notes WHERE project = $1 AND created_at < $2",
                "DELETE FROM theme_members WHERE project = $1 AND snapshot_id IN \
                 (SELECT snapshot_id FROM theme_snapshots WHERE project = $1 AND cutoff_at < $2)",
                "DELETE FROM themes WHERE project = $1 AND cutoff_at < $2",
                "DELETE FROM theme_snapshots WHERE project = $1 AND cutoff_at < $2",
            ] {
                out.removed += tx.execute(sql, &[&project, &cut]).await.map_err(be)?;
            }
        }

        // The filter box last, on the widest window: a value carried by a score
        // that is still stored is still a value the box should offer.
        let widest = policy
            .windows()
            .iter()
            .map(|(_, d)| *d)
            .max_by_key(|d| d.unwrap_or(u32::MAX))
            .flatten();
        if let Some(cut) = RetentionPolicy::cutoff(widest, now).map(to_nanos) {
            out.removed += tx
                .execute(
                    "DELETE FROM discovery WHERE project = $1 AND last_seen < $2",
                    &[&project, &cut],
                )
                .await
                .map_err(be)?;
        }

        tx.commit().await.map_err(be)?;
        Ok(out)
    }

    async fn enforce_retention(&self, project: &str, older_than: OffsetDateTime) -> Result<u64> {
        let mut conn = self.conn().await?;
        let tx = conn.transaction().await.map_err(be)?;
        let cut = to_nanos(older_than);

        let mut removed = 0_u64;
        for (table, col) in [
            ("spans", "received_at"),
            ("logs", "received_at"),
            ("metrics", "received_at"),
            ("scores", "timestamp"),
            ("feedback", "timestamp"),
            ("aspect_notes", "created_at"),
            ("traces", "received_at"),
        ] {
            removed += tx
                .execute(
                    &format!("DELETE FROM {table} WHERE project = $1 AND {col} < $2"),
                    &[&project, &cut],
                )
                .await
                .map_err(be)?;
        }
        // The analysis chain, in dependency order. A theme's label is the text
        // of one of its own notes, so the clustering has to go with the notes
        // or the sentence stays readable after the note has gone.
        for sql in [
            "DELETE FROM theme_members WHERE project = $1 AND snapshot_id IN \
             (SELECT snapshot_id FROM theme_snapshots WHERE project = $1 AND cutoff_at < $2)",
            "DELETE FROM themes WHERE project = $1 AND cutoff_at < $2",
            "DELETE FROM theme_snapshots WHERE project = $1 AND cutoff_at < $2",
        ] {
            removed += tx.execute(sql, &[&project, &cut]).await.map_err(be)?;
        }
        // Old idempotency keys go too. Keeping them forever would grow a table
        // that exists only to catch a retry seconds after the fact.
        tx.execute("DELETE FROM ingest_keys WHERE seen_at < $1", &[&cut]).await.map_err(be)?;
        // The filter box goes with the rows. A value nothing has carried since
        // before the window has no row left to name it, so offering it would
        // filter the list to nothing.
        removed += tx
            .execute(
                "DELETE FROM discovery WHERE project = $1 AND last_seen < $2",
                &[&project, &cut],
            )
            .await
            .map_err(be)?;
        tx.commit().await.map_err(be)?;
        Ok(removed)
    }

    async fn record_audit(&self, event: &AuditEvent) -> Result<()> {
        let conn = self.conn().await?;
        conn.execute(
            "INSERT INTO audit_log
                (audit_id, timestamp, principal, subject, action, project, resource, result)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
             ON CONFLICT (audit_id) DO NOTHING",
            &[
                &event.audit_id,
                &to_nanos(event.timestamp),
                &event.principal,
                &event.subject,
                &event.action,
                &event.project,
                &event.resource,
                &event.result.as_str(),
            ],
        )
        .await
        .map_err(be)?;
        Ok(())
    }

    async fn list_audit(&self, q: &AuditQuery) -> Result<Vec<AuditEvent>> {
        let conn = self.conn().await?;
        // Every optional filter is expressed as "the parameter is null, or it
        // matches", so one prepared statement serves every combination and no
        // value is ever formatted into the SQL.
        let rows = conn
            .query(
                "SELECT audit_id, timestamp, principal, subject, action, project, resource, result
                 FROM audit_log
                 WHERE project = $1
                   AND ($2::text IS NULL OR action = $2)
                   AND ($3::bigint IS NULL OR timestamp >= $3)
                   AND ($4::bigint IS NULL OR timestamp < $4)
                 ORDER BY timestamp DESC
                 LIMIT $5",
                &[
                    &q.project,
                    &q.action,
                    &q.from.map(to_nanos),
                    &q.to.map(to_nanos),
                    &i64::from(q.limit.clamp(1, 1000)),
                ],
            )
            .await
            .map_err(be)?;
        Ok(rows
            .iter()
            .map(|r| AuditEvent {
                audit_id: r.get(0),
                timestamp: from_nanos(r.get(1)),
                principal: r.get(2),
                subject: r.get(3),
                action: r.get(4),
                project: r.get(5),
                resource: r.get(6),
                // An unreadable result is a row written by a build that knew a
                // variant this one does not. Refused is the safe reading: it
                // never claims an access was served.
                result: mcpd_core::AuditResult::from_str(r.get::<_, &str>(7))
                    .unwrap_or(mcpd_core::AuditResult::Refused),
            })
            .collect())
    }

    async fn list_projects(&self) -> Result<Vec<String>> {
        let conn = self.conn().await?;
        Ok(conn
            .query("SELECT DISTINCT project FROM traces ORDER BY project", &[])
            .await
            .map_err(be)?
            .iter()
            .map(|r| r.get(0))
            .collect())
    }

    async fn put_dataset(&self, d: &Dataset) -> Result<()> {
        let conn = self.conn().await?;
        conn.execute(
            "INSERT INTO datasets (dataset_id, name, description, item_count, created_at,
                project, environment)
             VALUES ($1,$2,$3,$4,$5,$6,$7)
             ON CONFLICT (dataset_id) DO UPDATE SET
                name = EXCLUDED.name, description = EXCLUDED.description,
                project = EXCLUDED.project, environment = EXCLUDED.environment",
            &[
                &d.dataset_id,
                &d.name,
                &d.description,
                &i64::from(d.item_count),
                &to_nanos(d.created_at),
                &d.correlation.project,
                &d.correlation.environment,
            ],
        )
        .await
        .map_err(be)?;
        Ok(())
    }

    async fn dataset_project(&self, dataset_id: &str) -> Result<Option<String>> {
        let conn = self.conn().await?;
        Ok(conn
            .query_opt("SELECT project FROM datasets WHERE dataset_id = $1", &[&dataset_id])
            .await
            .map_err(be)?
            .map(|r| r.get(0)))
    }

    async fn list_datasets(&self, project: &str) -> Result<Vec<Dataset>> {
        let conn = self.conn().await?;
        Ok(conn
            .query(
                "SELECT dataset_id, name, description, item_count, created_at, project,
                        environment
                 FROM datasets WHERE project = $1 ORDER BY created_at DESC",
                &[&project],
            )
            .await
            .map_err(be)?
            .iter()
            .map(|r| Dataset {
                dataset_id: r.get(0),
                name: r.get(1),
                description: r.get(2),
                item_count: r.get::<_, i64>(3).max(0) as u32,
                created_at: from_nanos(r.get(4)),
                correlation: correlation_of(r.get(5), r.get(6)),
            })
            .collect())
    }

    async fn add_dataset_items(&self, dataset_id: &str, items: &[DatasetItem]) -> Result<u32> {
        let mut conn = self.conn().await?;
        let tx = conn.transaction().await.map_err(be)?;
        // An item id another dataset holds is refused before anything is
        // written. `DO NOTHING` below kept the other dataset's row, but answered
        // as if the request had nothing new in it, which is not what happened.
        let ids: Vec<&str> = items.iter().map(|it| it.item_id.as_str()).collect();
        let held = tx
            .query_opt(
                "SELECT item_id FROM dataset_items
                 WHERE item_id = ANY($1) AND dataset_id <> $2 LIMIT 1",
                &[&ids, &dataset_id],
            )
            .await
            .map_err(be)?;
        if let Some(row) = held {
            return Err(crate::item_taken(&row.get::<_, String>(0)));
        }
        let mut added = 0_u32;
        for it in items {
            added += u32::try_from(
                tx.execute(
                    "INSERT INTO dataset_items (item_id, dataset_id, input, expected, metadata,
                        source_trace_id)
                     VALUES ($1,$2,$3,$4,$5,$6)
                     ON CONFLICT (item_id) DO NOTHING",
                    &[
                        &it.item_id,
                        &dataset_id,
                        &serde_json::to_string(&it.input).unwrap_or_else(|_| "null".to_owned()),
                        &json_out(&it.expected),
                        &json_out(&it.metadata),
                        &it.source_trace_id,
                    ],
                )
                .await
                .map_err(be)?,
            )
            .unwrap_or(0);
        }
        // Recomputed rather than incremented, for the same reason the trace
        // projection is: a retried capture must not double the count.
        tx.execute(
            "UPDATE datasets SET item_count =
                (SELECT COUNT(*) FROM dataset_items WHERE dataset_id = $1)
             WHERE dataset_id = $1",
            &[&dataset_id],
        )
        .await
        .map_err(be)?;
        tx.commit().await.map_err(be)?;
        Ok(added)
    }

    async fn list_dataset_items(&self, dataset_id: &str, limit: u32) -> Result<Vec<DatasetItem>> {
        let conn = self.conn().await?;
        Ok(conn
            .query(
                "SELECT item_id, dataset_id, input, expected, metadata, source_trace_id
                 FROM dataset_items WHERE dataset_id = $1 ORDER BY item_id LIMIT $2",
                &[&dataset_id, &i64::from(limit.clamp(1, 5000))],
            )
            .await
            .map_err(be)?
            .iter()
            .map(|r| DatasetItem {
                item_id: r.get(0),
                dataset_id: r.get(1),
                input: json_in(r.get(2)).unwrap_or(Value::Null),
                expected: json_in(r.get(3)),
                metadata: json_in(r.get(4)),
                source_trace_id: r.get(5),
            })
            .collect())
    }

    async fn put_experiment(&self, e: &Experiment) -> Result<()> {
        let conn = self.conn().await?;
        let stored = conn
            .execute(
                "INSERT INTO experiments (experiment_id, name, dataset_id, status, started_at,
                finished_at, item_count, summary, baseline_id, cost_json, error, metadata,
                project, environment)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)
             ON CONFLICT (experiment_id) DO UPDATE SET
                name = EXCLUDED.name, status = EXCLUDED.status,
                finished_at = EXCLUDED.finished_at, item_count = EXCLUDED.item_count,
                summary = EXCLUDED.summary, cost_json = EXCLUDED.cost_json,
                error = EXCLUDED.error, metadata = EXCLUDED.metadata
             WHERE experiments.project = EXCLUDED.project",
                &[
                    &e.experiment_id,
                    &e.name,
                    &e.dataset_id,
                    &e.status.as_str(),
                    &e.started_at.map(to_nanos),
                    &e.finished_at.map(to_nanos),
                    &i64::from(e.item_count),
                    &json_out(&e.summary),
                    &e.baseline_id,
                    &enc(&e.cost),
                    &e.error,
                    &json_out(&e.metadata),
                    &e.correlation.project,
                    &e.correlation.environment,
                ],
            )
            .await
            .map_err(be)?;
        if stored == 0 {
            return Err(crate::experiment_taken(&e.experiment_id));
        }
        Ok(())
    }

    async fn list_experiments(&self, project: &str) -> Result<Vec<Experiment>> {
        let conn = self.conn().await?;
        Ok(conn
            .query(
                "SELECT experiment_id, name, dataset_id, status, started_at, finished_at,
                        item_count, summary, baseline_id, cost_json, error, metadata, project,
                        environment
                 FROM experiments WHERE project = $1 ORDER BY started_at DESC NULLS LAST",
                &[&project],
            )
            .await
            .map_err(be)?
            .iter()
            .map(|r| Experiment {
                experiment_id: r.get(0),
                name: r.get(1),
                dataset_id: r.get(2),
                status: mcpd_core::ExperimentStatus::from_str(r.get::<_, &str>(3))
                    .unwrap_or(mcpd_core::ExperimentStatus::Running),
                started_at: r.get::<_, Option<i64>>(4).map(from_nanos),
                finished_at: r.get::<_, Option<i64>>(5).map(from_nanos),
                item_count: r.get::<_, i64>(6).max(0) as u32,
                summary: json_in(r.get(7)),
                baseline_id: r.get(8),
                cost: dec::<Cost>(r.get(9)),
                error: r.get(10),
                metadata: json_in(r.get(11)),
                correlation: correlation_of(r.get(12), r.get(13)),
            })
            .collect())
    }

    // ------------------------------------------------- versioned documents

    async fn put_document(&self, doc: &NewDocument) -> Result<(StoredDocument, bool)> {
        let conn = self.conn().await?;
        // Two replicas can read the same latest version and both try the next
        // number. The primary key lets one of them in; the other sees no row
        // inserted, reads again and tries the number after, or finds its body
        // is now the latest and confirms it.
        for _ in 0..8 {
            let held = pg_latest_document(&conn, &doc.project, doc.kind, &doc.id).await?;
            if let Some(held) = held.as_ref().filter(|h| h.body == doc.body) {
                return Ok((held.clone(), false));
            }
            let version = held.map_or(1, |h| h.version.saturating_add(1));
            let n = conn
                .execute(
                    "INSERT INTO documents
                       (project, kind, id, version, body, note, created_by, created_at)
                     VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
                     ON CONFLICT (project, kind, id, version) DO NOTHING",
                    &[
                        &doc.project,
                        &doc.kind.as_str(),
                        &doc.id,
                        &i64::from(version),
                        &doc.body.to_string(),
                        &doc.note,
                        &doc.created_by,
                        &to_nanos(doc.created_at),
                    ],
                )
                .await
                .map_err(be)?;
            if n == 1 {
                return Ok((
                    StoredDocument {
                        project: doc.project.clone(),
                        kind: doc.kind,
                        id: doc.id.clone(),
                        version,
                        body: doc.body.clone(),
                        note: doc.note.clone(),
                        created_by: doc.created_by.clone(),
                        created_at: doc.created_at,
                    },
                    true,
                ));
            }
        }
        Err(Error::Backend(format!(
            "could not assign a version to {} {:?}: other writers took every number tried; \
             post it again",
            doc.kind.as_str(),
            doc.id
        )))
    }

    async fn list_documents(
        &self,
        project: &str,
        kind: DocKind,
        id: Option<&str>,
    ) -> Result<Vec<StoredDocument>> {
        let conn = self.conn().await?;
        let rows = match id {
            Some(id) => conn
                .query(
                    "SELECT project, id, version, body, note, created_by, created_at
                     FROM documents WHERE project = $1 AND kind = $2 AND id = $3
                     ORDER BY version DESC",
                    &[&project, &kind.as_str(), &id],
                )
                .await
                .map_err(be)?,
            None => conn
                .query(
                    "SELECT DISTINCT ON (id) project, id, version, body, note, created_by,
                            created_at
                     FROM documents WHERE project = $1 AND kind = $2
                     ORDER BY id, version DESC",
                    &[&project, &kind.as_str()],
                )
                .await
                .map_err(be)?,
        };
        let docs: Vec<StoredDocument> = rows.iter().map(|r| pg_document(kind, r)).collect();
        Ok(if id.is_some() { docs } else { crate::latest_per_id(docs) })
    }

    async fn get_document(
        &self,
        project: &str,
        kind: DocKind,
        id: &str,
        version: Option<u32>,
    ) -> Result<Option<StoredDocument>> {
        let conn = self.conn().await?;
        let Some(v) = version else {
            return pg_latest_document(&conn, project, kind, id).await;
        };
        Ok(conn
            .query_opt(
                "SELECT project, id, version, body, note, created_by, created_at
                 FROM documents WHERE project = $1 AND kind = $2 AND id = $3 AND version = $4",
                &[&project, &kind.as_str(), &id, &i64::from(v)],
            )
            .await
            .map_err(be)?
            .map(|r| pg_document(kind, &r)))
    }

    async fn set_document_label(
        &self,
        project: &str,
        kind: DocKind,
        id: &str,
        label: &str,
        version: Option<u32>,
    ) -> Result<bool> {
        let conn = self.conn().await?;
        let n = match version {
            None => conn
                .execute(
                    "DELETE FROM document_labels
                     WHERE project = $1 AND kind = $2 AND id = $3 AND label = $4",
                    &[&project, &kind.as_str(), &id, &label],
                )
                .await
                .map_err(be)?,
            // One statement, so the version is checked by the same snapshot
            // that writes the label.
            Some(v) => conn
                .execute(
                    "INSERT INTO document_labels (project, kind, id, label, version, updated_at)
                     SELECT $1, $2, $3, $4, $5, $6
                     WHERE EXISTS (SELECT 1 FROM documents
                                   WHERE project = $1 AND kind = $2 AND id = $3
                                     AND version = $5)
                     ON CONFLICT (project, kind, id, label)
                     DO UPDATE SET version = EXCLUDED.version, updated_at = EXCLUDED.updated_at",
                    &[&project, &kind.as_str(), &id, &label, &i64::from(v), &now_nanos()],
                )
                .await
                .map_err(be)?,
        };
        Ok(n > 0)
    }

    async fn list_document_labels(
        &self,
        project: &str,
        kind: DocKind,
        id: Option<&str>,
    ) -> Result<Vec<DocumentLabel>> {
        let conn = self.conn().await?;
        Ok(conn
            .query(
                "SELECT id, label, version FROM document_labels
                 WHERE project = $1 AND kind = $2 AND ($3::TEXT IS NULL OR id = $3)
                 ORDER BY id, label",
                &[&project, &kind.as_str(), &id],
            )
            .await
            .map_err(be)?
            .iter()
            .map(|r| DocumentLabel {
                id: r.get(0),
                label: r.get(1),
                version: r.get::<_, i64>(2).clamp(0, i64::from(u32::MAX)) as u32,
            })
            .collect())
    }

    // -------------------------------------------------------------- leases

    async fn claim_lease(&self, c: &crate::LeaseClaim<'_>) -> Result<crate::Lease> {
        let conn = self.conn().await?;
        let (now, expires) = (to_nanos(c.now), to_nanos(c.now + c.ttl));
        // One statement, so it is the compare-and-set: a concurrent claim on
        // the same row waits for this one's lock and then evaluates the
        // `WHERE` against the row this one wrote. Every column on the right of
        // `SET` reads the row as it was before.
        conn.execute(
            "INSERT INTO leases (job, holder, since, expires_at, progress)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (job) DO UPDATE SET
                 since = CASE WHEN leases.holder = excluded.holder AND leases.expires_at > $3
                              THEN leases.since ELSE excluded.since END,
                 holder = excluded.holder,
                 expires_at = excluded.expires_at,
                 progress = COALESCE(excluded.progress, leases.progress)
             WHERE leases.holder = excluded.holder OR leases.expires_at <= $3",
            &[&c.job, &c.holder, &now, &expires, &c.progress],
        )
        .await
        .map_err(be)?;
        let r = conn
            .query_one(
                "SELECT job, holder, since, expires_at, progress FROM leases WHERE job = $1",
                &[&c.job],
            )
            .await
            .map_err(be)?;
        Ok(crate::Lease {
            job: r.get(0),
            holder: r.get(1),
            since: from_nanos(r.get(2)),
            expires_at: from_nanos(r.get(3)),
            progress: r.get(4),
        })
    }

    async fn release_lease(&self, job: &str, holder: &str, now: OffsetDateTime) -> Result<()> {
        let now = to_nanos(now);
        self.conn()
            .await?
            .execute(
                "UPDATE leases SET expires_at = $3
                 WHERE job = $1 AND holder = $2 AND expires_at > $3",
                &[&job, &holder, &now],
            )
            .await
            .map_err(be)?;
        Ok(())
    }

    // ----------------------------------------------------- aspect analysis

    async fn list_aspect_notes(&self, q: &AspectNoteQuery) -> Result<Paged<StoredNote>> {
        let conn = self.conn().await?;
        let mut w = Where::default();
        w.push("project = {}", q.project.clone());
        if let Some(t) = &q.trace_id {
            w.push("trace_id = {}", t.clone());
        }
        if let Some(a) = &q.aspect {
            w.push("aspect = {}", a.clone());
        }
        if let Some((kind, name)) = &q.entity {
            w.push("entity_kind = {}", kind.as_str().to_owned());
            w.push("entity_name = {}", name.clone());
        }
        if let Some(from) = q.from {
            w.push("created_at >= {}", to_nanos(from));
        }
        if let Some(to) = q.to {
            w.push("created_at < {}", to_nanos(to));
        }
        if !q.note_ids.is_empty() {
            // `= ANY($n)` rather than a generated `IN` list, so the whole set
            // is one bind and the statement text is the same shape however many
            // ids are asked for, which is what lets Postgres reuse the plan.
            w.push("note_id = ANY({})", q.note_ids.clone());
        }

        let desc = matches!(q.order, Order::Desc);
        let (cmp, dir) = if desc { ('<', "DESC") } else { ('>', "ASC") };
        if let Some(c) = &q.cursor {
            let (v, id) = decode_cursor(c)?;
            let n = w.next();
            w.push_raw(format!("(created_at, note_id) {cmp} (${n}::bigint, ${})", n + 1));
            w.bind(v);
            w.bind(id);
        }

        let limit = q.limit.clamp(1, 1000);
        let sql = format!(
            "SELECT {NOTE_COLUMNS} FROM aspect_notes{} \
             ORDER BY created_at {dir}, note_id {dir} LIMIT {}",
            w.sql(),
            limit + 1
        );
        let rows = conn.query(&sql, &w.params()).await.map_err(be)?;
        let mut items: Vec<StoredNote> = rows.iter().map(row_to_note).collect();
        let next_cursor = if items.len() > limit as usize {
            items.truncate(limit as usize);
            items.last().map(|n| encode_cursor(to_nanos(n.note.created_at), &n.note.note_id))
        } else {
            None
        };
        Ok(Paged { items, next_cursor, total: None })
    }

    async fn aspect_note_rollup(&self, q: &AspectRollupQuery) -> Result<Vec<AspectNoteRollup>> {
        let conn = self.conn().await?;
        let mut w = Where::default();
        w.push("project = {}", q.project.clone());
        // A note whose trace was not stored when it arrived has no entity, and
        // a roll-up is by entity, so those rows sit outside every group here.
        w.push_raw("entity_name IS NOT NULL".to_owned());
        w.push_raw("entity_kind IS NOT NULL".to_owned());
        if let Some((kind, name)) = &q.entity {
            w.push("entity_kind = {}", kind.as_str().to_owned());
            w.push("entity_name = {}", name.clone());
        } else if let Some(kind) = q.entity_kind {
            w.push("entity_kind = {}", kind.as_str().to_owned());
        }
        let sql = format!(
            "SELECT entity_kind, entity_name, aspect, COUNT(*), COUNT(DISTINCT trace_id), \
                    MAX(created_at) \
               FROM aspect_notes{} \
              GROUP BY entity_kind, entity_name, aspect \
              ORDER BY MAX(created_at) DESC, entity_name ASC, aspect ASC LIMIT {}",
            w.sql(),
            q.max_groups.clamp(1, 10_000)
        );
        Ok(conn
            .query(&sql, &w.params())
            .await
            .map_err(be)?
            .iter()
            .map(|r| AspectNoteRollup {
                entity_kind: EntityKind::from_str(r.get::<_, &str>(0))
                    .unwrap_or(EntityKind::Service),
                entity_name: r.get(1),
                aspect: r.get(2),
                note_count: r.get::<_, i64>(3).max(0) as u64,
                trace_count: r.get::<_, i64>(4).max(0) as u64,
                last_note_at: from_nanos(r.get(5)),
            })
            .collect())
    }

    async fn write_theme_snapshot(&self, write: &SnapshotWrite) -> Result<()> {
        let mut conn = self.conn().await?;
        let tx = conn.transaction().await.map_err(be)?;
        let s = &write.snapshot;

        // A pass is a pure function of the notes it read, so rewriting an id
        // that already exists writes what is already there. The old rows go
        // first because a rerun can produce fewer themes than the run before
        // it, and an upsert alone would leave the extras behind.
        for sql in [
            "DELETE FROM themes WHERE snapshot_id = $1",
            "DELETE FROM theme_members WHERE snapshot_id = $1",
            "DELETE FROM theme_snapshots WHERE snapshot_id = $1",
        ] {
            tx.execute(sql, &[&s.snapshot_id]).await.map_err(be)?;
        }

        tx.execute(
            "INSERT INTO theme_snapshots (
                snapshot_id, project, entity_kind, entity_name, aspects, ordinal, cutoff_at,
                window_from, window_to, trace_count, note_count, capped)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
            &[
                &s.snapshot_id,
                &s.project,
                &s.entity_kind.as_str(),
                &s.entity_name,
                &serde_json::to_string(&s.aspects).unwrap_or_else(|_| "[]".into()),
                &(s.ordinal as i64),
                &to_nanos(s.cutoff_at),
                &to_nanos(s.window_from),
                &to_nanos(s.window_to),
                &(s.trace_count as i64),
                &(s.note_count as i64),
                &s.capped,
            ],
        )
        .await
        .map_err(be)?;

        for t in &write.themes {
            let th = &t.theme;
            tx.execute(
                "INSERT INTO themes (
                    snapshot_id, aspect, theme_id, project, entity_kind, entity_name, label,
                    label_note_id, label_trace_id, state, note_count, trace_count, coverage,
                    cutoff_at, window_from, window_to, split_from, split_into, merged_from,
                    merged_into)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20)",
                &[
                    &th.snapshot_id,
                    &th.aspect,
                    &th.theme_id,
                    &t.project,
                    &th.entity_kind.as_str(),
                    &th.entity_name,
                    &th.label,
                    &th.label_note_id,
                    &th.label_trace_id,
                    &th.state.as_str(),
                    &(th.note_count as i64),
                    &(th.trace_count as i64),
                    &th.coverage,
                    &to_nanos(t.cutoff_at),
                    &to_nanos(t.window_from),
                    &to_nanos(t.window_to),
                    &list_out(&t.lineage.split_from),
                    &list_out(&t.lineage.split_into),
                    &list_out(&t.lineage.merged_from),
                    &list_out(&t.lineage.merged_into),
                ],
            )
            .await
            .map_err(be)?;
        }

        for m in &write.members {
            tx.execute(
                "INSERT INTO theme_members (
                    snapshot_id, aspect, theme_id, note_id, trace_id, project, similarity,
                    trace_started_at)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
                 ON CONFLICT (snapshot_id, aspect, theme_id, note_id) DO NOTHING",
                &[
                    &m.snapshot_id,
                    &m.aspect,
                    &m.theme_id,
                    &m.note_id,
                    &m.trace_id,
                    &m.project,
                    &i64::from(m.similarity),
                    &to_nanos(m.trace_started_at),
                ],
            )
            .await
            .map_err(be)?;
        }

        tx.commit().await.map_err(be)?;
        Ok(())
    }

    async fn list_theme_snapshots(&self, q: &SnapshotQuery) -> Result<Paged<ThemeSnapshot>> {
        let conn = self.conn().await?;
        let mut w = Where::default();
        w.push("project = {}", q.project.clone());
        if let Some(id) = &q.snapshot_id {
            w.push("snapshot_id = {}", id.clone());
        }
        if let Some((kind, name)) = &q.entity {
            w.push("entity_kind = {}", kind.as_str().to_owned());
            w.push("entity_name = {}", name.clone());
        }
        if let Some(from) = q.from {
            w.push("cutoff_at >= {}", to_nanos(from));
        }
        if let Some(to) = q.to {
            w.push("cutoff_at < {}", to_nanos(to));
        }
        for aspect in &q.aspects {
            // The column holds a JSON array of names, so membership is a search
            // for the quoted name. Bound, never interpolated, and the quotes are
            // what stop `goal` matching an aspect called `goal_reached`.
            w.push("position({} in aspects) > 0", format!("\"{aspect}\""));
        }

        // Counted before the cursor is added, so it is how many the filter
        // matched and not how many are left after this page.
        let total: i64 = conn
            .query_one(&format!("SELECT COUNT(*) FROM theme_snapshots{}", w.sql()), &w.params())
            .await
            .map_err(be)?
            .get(0);

        let desc = matches!(q.order, Order::Desc);
        let (cmp, dir) = if desc { ('<', "DESC") } else { ('>', "ASC") };
        if let Some(c) = &q.cursor {
            let (v, id) = decode_cursor(c)?;
            let n = w.next();
            w.push_raw(format!("(cutoff_at, snapshot_id) {cmp} (${n}::bigint, ${})", n + 1));
            w.bind(v);
            w.bind(id);
        }

        let limit = q.limit.clamp(1, 1000);
        let sql = format!(
            "SELECT {SNAPSHOT_COLUMNS} FROM theme_snapshots{} \
             ORDER BY cutoff_at {dir}, snapshot_id {dir} LIMIT {}",
            w.sql(),
            limit + 1
        );
        let rows = conn.query(&sql, &w.params()).await.map_err(be)?;
        let mut items: Vec<ThemeSnapshot> = rows.iter().map(row_to_snapshot).collect();
        let next_cursor = if items.len() > limit as usize {
            items.truncate(limit as usize);
            items.last().map(|s| encode_cursor(to_nanos(s.cutoff_at), &s.snapshot_id))
        } else {
            None
        };
        Ok(Paged { items, next_cursor, total: Some(total.max(0) as u64) })
    }

    async fn list_themes(&self, q: &ThemeQuery) -> Result<Paged<StoredTheme>> {
        let conn = self.conn().await?;
        let mut w = Where::default();
        w.push("project = {}", q.project.clone());
        if let Some(id) = &q.snapshot_id {
            w.push("snapshot_id = {}", id.clone());
        }
        if let Some((kind, name)) = &q.entity {
            w.push("entity_kind = {}", kind.as_str().to_owned());
            w.push("entity_name = {}", name.clone());
        }
        if let Some(a) = &q.aspect {
            w.push("aspect = {}", a.clone());
        }
        if let Some(t) = &q.theme_id {
            w.push("theme_id = {}", t.clone());
        }

        let desc = matches!(q.order, Order::Desc);
        let (cmp, dir) = if desc { ('<', "DESC") } else { ('>', "ASC") };
        if let Some(c) = &q.cursor {
            let (v, id) = decode_cursor(c)?;
            let n = w.next();
            w.push_raw(format!("(cutoff_at, theme_id) {cmp} (${n}::bigint, ${})", n + 1));
            w.bind(v);
            w.bind(id);
        }

        let limit = q.limit.clamp(1, 1000);
        let sql = format!(
            "SELECT {THEME_COLUMNS} FROM themes{} \
             ORDER BY cutoff_at {dir}, theme_id {dir} LIMIT {}",
            w.sql(),
            limit + 1
        );
        let rows = conn.query(&sql, &w.params()).await.map_err(be)?;
        let mut items: Vec<StoredTheme> = rows.iter().map(row_to_stored_theme).collect();
        let next_cursor = if items.len() > limit as usize {
            items.truncate(limit as usize);
            items.last().map(|t| encode_cursor(to_nanos(t.cutoff_at), &t.theme.theme_id))
        } else {
            None
        };
        Ok(Paged { items, next_cursor, total: None })
    }

    async fn list_theme_members(&self, q: &ThemeMemberQuery) -> Result<Paged<ThemeMember>> {
        let conn = self.conn().await?;
        let mut w = Where::default();
        w.push("project = {}", q.project.clone());
        w.push("snapshot_id = {}", q.snapshot_id.clone());
        if let Some(a) = &q.aspect {
            w.push("aspect = {}", a.clone());
        }
        if let Some(t) = &q.theme_id {
            w.push("theme_id = {}", t.clone());
        }
        if let Some(c) = &q.cursor {
            // The sort runs one way on similarity and the other on the note id,
            // so the resume condition is written out rather than borrowed from a
            // walk whose two keys point the same way. Getting this wrong repeats
            // a row on every page.
            let (v, id) = decode_cursor(c)?;
            let n = w.next();
            w.push_raw(format!(
                "(similarity < ${n}::bigint OR (similarity = ${n}::bigint AND note_id > ${}))",
                n + 1
            ));
            w.bind(v);
            w.bind(id);
        }

        let limit = q.limit.clamp(1, 20_000);
        let sql = format!(
            "SELECT snapshot_id, aspect, theme_id, note_id, trace_id, project, similarity, \
                    trace_started_at FROM theme_members{} \
             ORDER BY similarity DESC, note_id ASC LIMIT {}",
            w.sql(),
            limit + 1
        );
        let rows = conn.query(&sql, &w.params()).await.map_err(be)?;
        let mut items: Vec<ThemeMember> = rows
            .iter()
            .map(|r| ThemeMember {
                snapshot_id: r.get(0),
                aspect: r.get(1),
                theme_id: r.get(2),
                note_id: r.get(3),
                trace_id: r.get(4),
                project: r.get(5),
                similarity: r.get::<_, i64>(6).clamp(0, i64::from(u32::MAX)) as u32,
                trace_started_at: from_nanos(r.get(7)),
            })
            .collect();
        let next_cursor = if items.len() > limit as usize {
            items.truncate(limit as usize);
            items.last().map(|m| encode_cursor(i64::from(m.similarity), &m.note_id))
        } else {
            None
        };
        Ok(Paged { items, next_cursor, total: None })
    }
}

/// The columns a stored note is read back from, in one place so the `SELECT`
/// and the row reader cannot drift apart.
const NOTE_COLUMNS: &str = "note_id, trace_id, aspect, text, definition_version, model, \
     created_at, entity_kind, entity_name, trace_started_at, project, environment, service_name, \
     service_version, session_id, thread_id, user_id, resource_id, run_id, request_id, \
     experiment_id, release, sketch";

/// The columns a theme is read back from.
const THEME_COLUMNS: &str = "snapshot_id, aspect, theme_id, project, entity_kind, entity_name, \
     label, label_note_id, label_trace_id, state, note_count, trace_count, coverage, cutoff_at, \
     window_from, window_to, split_from, split_into, merged_from, merged_into";

/// The columns a snapshot is read back from.
const SNAPSHOT_COLUMNS: &str = "snapshot_id, project, entity_kind, entity_name, aspects, ordinal, \
     cutoff_at, window_from, window_to, trace_count, note_count, capped";

fn row_to_note(r: &tokio_postgres::Row) -> StoredNote {
    StoredNote {
        note: AspectNote {
            note_id: r.get(0),
            trace_id: r.get(1),
            aspect: r.get(2),
            text: r.get(3),
            definition_version: r.get::<_, i64>(4).clamp(0, i64::from(u32::MAX)) as u32,
            model: r.get(5),
            created_at: from_nanos(r.get(6)),
            correlation: Correlation {
                project: r.get(10),
                environment: r.get(11),
                service_name: r.get(12),
                service_version: r.get(13),
                session_id: r.get(14),
                thread_id: r.get(15),
                user_id: r.get(16),
                resource_id: r.get(17),
                run_id: r.get(18),
                request_id: r.get(19),
                experiment_id: r.get(20),
                release: r.get(21),
            },
        },
        entity_kind: r.get::<_, Option<String>>(7).and_then(|s| EntityKind::from_str(&s).ok()),
        entity_name: r.get(8),
        trace_started_at: r.get::<_, Option<i64>>(9).map(from_nanos),
        sketch: r
            .get::<_, Option<Vec<u8>>>(22)
            .as_deref()
            .and_then(mcpd_core::sketch::Sketch::from_bytes),
    }
}

fn row_to_snapshot(r: &tokio_postgres::Row) -> ThemeSnapshot {
    ThemeSnapshot {
        snapshot_id: r.get(0),
        project: r.get(1),
        entity_kind: EntityKind::from_str(r.get::<_, &str>(2)).unwrap_or(EntityKind::Service),
        entity_name: r.get(3),
        aspects: serde_json::from_str(r.get::<_, &str>(4)).unwrap_or_default(),
        ordinal: r.get::<_, i64>(5).max(0) as u64,
        cutoff_at: from_nanos(r.get(6)),
        window_from: from_nanos(r.get(7)),
        window_to: from_nanos(r.get(8)),
        trace_count: r.get::<_, i64>(9).max(0) as u64,
        note_count: r.get::<_, i64>(10).max(0) as u64,
        capped: r.get(11),
    }
}

fn row_to_stored_theme(r: &tokio_postgres::Row) -> StoredTheme {
    StoredTheme {
        theme: Theme {
            theme_id: r.get(2),
            snapshot_id: r.get(0),
            aspect: r.get(1),
            entity_kind: EntityKind::from_str(r.get::<_, &str>(4)).unwrap_or(EntityKind::Service),
            entity_name: r.get(5),
            label: r.get(6),
            label_note_id: r.get(7),
            label_trace_id: r.get(8),
            state: ThemeState::from_str(r.get::<_, &str>(9)).unwrap_or(ThemeState::Continue),
            note_count: r.get::<_, i64>(10).max(0) as u64,
            trace_count: r.get::<_, i64>(11).max(0) as u64,
            coverage: r.get(12),
        },
        project: r.get(3),
        cutoff_at: from_nanos(r.get(13)),
        window_from: from_nanos(r.get(14)),
        window_to: from_nanos(r.get(15)),
        lineage: ThemeLineage {
            split_from: list_in(r.get(16)),
            split_into: list_in(r.get(17)),
            merged_from: list_in(r.get(18)),
            merged_into: list_in(r.get(19)),
        },
    }
}

/// A list of theme ids as stored: JSON, or NULL when there is nothing in it.
///
/// NULL rather than `[]` so the common case, a theme that neither split nor
/// merged, costs one byte on every row rather than two.
fn list_out(ids: &[String]) -> Option<String> {
    (!ids.is_empty()).then(|| serde_json::to_string(ids).unwrap_or_else(|_| "[]".into()))
}

fn list_in(raw: Option<String>) -> Vec<String> {
    raw.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sslrootcert_is_taken_out_of_a_url_and_verify_full_becomes_require() {
        let (url, tls) = split_tls(
            "postgres://u:p@db.example:5432/mcpd?sslmode=verify-full&sslrootcert=%2Fetc%2Fca.pem\
             &application_name=mcpd",
        )
        .unwrap();
        assert_eq!(
            url,
            "postgres://u:p@db.example:5432/mcpd?sslmode=require&application_name=mcpd"
        );
        assert_eq!(tls.root_cert.as_deref(), Some("/etc/ca.pem"));
        let pg: tokio_postgres::Config = url.parse().expect("the driver accepts what is left");
        assert_eq!(pg.get_ssl_mode(), SslMode::Require);
        assert_eq!(pg.get_application_name(), Some("mcpd"));
    }

    #[test]
    fn only_the_trust_store_path_is_decoded_and_every_other_value_is_kept_as_written() {
        let (url, tls) = split_tls(
            "postgres://u@db.example/mcpd?password=a%26b&sslrootcert=%2Fetc%2Fca.pem\
             &application_name=x%2541",
        )
        .unwrap();
        assert_eq!(url, "postgres://u@db.example/mcpd?password=a%26b&application_name=x%2541");
        assert_eq!(tls.root_cert.as_deref(), Some("/etc/ca.pem"));
        let pg: tokio_postgres::Config = url.parse().expect("the driver accepts what is left");
        assert_eq!(pg.get_password(), Some(&b"a&b"[..]), "an escaped & is part of the password");
        assert_eq!(pg.get_application_name(), Some("x%41"), "a value is decoded once, not twice");
    }

    #[test]
    fn sslrootcert_is_taken_out_of_a_key_value_string() {
        let (rest, tls) =
            split_tls("host=db user=u sslmode=verify-ca sslrootcert=/ca.pem dbname=mcpd").unwrap();
        assert_eq!(rest, "host=db user=u sslmode=require dbname=mcpd");
        assert_eq!(tls.root_cert.as_deref(), Some("/ca.pem"));
        assert!(rest.parse::<tokio_postgres::Config>().is_ok());
    }

    #[test]
    fn a_string_with_nothing_to_take_is_unchanged() {
        for url in ["postgres://u@h/db", "postgres://u@h/db?sslmode=disable", "host=h user=u"] {
            assert_eq!(split_tls(url).unwrap(), (url.to_owned(), TlsSettings::default()));
        }
    }

    #[test]
    fn a_broken_escape_is_kept_rather_than_panicking() {
        assert_eq!(percent_decode("a%2Fb"), "a/b");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%é"), "%é");
        assert_eq!(percent_decode("%zz"), "%zz");
    }

    #[test]
    fn a_missing_trust_store_is_refused_by_name() {
        let tls = TlsSettings { root_cert: Some("/nonexistent/ca.pem".into()) };
        let err = tls.client_config().unwrap_err().to_string();
        assert!(err.contains("/nonexistent/ca.pem"), "{err}");
        assert!(TlsSettings::default().client_config().is_ok());
    }

    #[test]
    fn the_password_never_reaches_the_parse_error() {
        let err = PostgresStore::new(PostgresConfig {
            url: "postgres://u:hunter2@h:notaport/db".into(),
            ..PostgresConfig::default()
        })
        .unwrap_err()
        .to_string();
        assert!(!err.contains("hunter2"), "{err}");
    }

    #[test]
    fn a_filter_prefix_cannot_carry_like_syntax() {
        // A prefix is whatever somebody typed into a filter box. Without this,
        // typing `%` matches every value in the project and typing `_` matches
        // every value one character longer, which reads as a broken filter
        // rather than as a search for those characters.
        assert_eq!(escape_like("gpt"), "gpt");
        assert_eq!(escape_like("100%"), "100\\%");
        assert_eq!(escape_like("a_b"), "a\\_b");
        // The backslash is escaped first. Doing it last would escape the
        // escapes the two lines above just added, and `100%` would become a
        // literal `100\\%` that matches nothing.
        assert_eq!(escape_like("a\\%"), "a\\\\\\%");
    }

    /// The probe's statements are formatted from four table names and a
    /// branch, and nothing in the type system says the result parses. This
    /// pins the parts that decide what the probe means; that they run is
    /// proved against a server.
    #[test]
    fn the_probe_reads_only_the_other_form_of_identifier() {
        let [attributed, opaque, candidates] = probe_statements(true);
        for (table, column) in ATTRIBUTING_COLUMNS {
            assert!(
                attributed.contains(&format!("FROM {table} ")),
                "{table} is unseen: {attributed}"
            );
            assert!(
                candidates.contains(&format!("SELECT {column} AS id FROM {table} ")),
                "{candidates}"
            );
        }
        assert!(
            opaque.contains("<> $2"),
            "the subject is not excluded from its own probe: {opaque}"
        );
        // A hashed subject cannot address a plaintext row, so those are the
        // candidates. With a plaintext subject it is the other way round.
        let shape = mcpd_store_shape();
        assert!(candidates.contains(&format!("!~ '{shape}'")), "{candidates}");
        let [_, _, candidates] = probe_statements(false);
        assert!(candidates.contains(&format!("user_id ~ '{shape}'")), "{candidates}");
        // The whole shape, never the bare prefix. A prefix test drops a
        // caller's own digest out of the candidates, and the rule counts that
        // digest as the other form, so the count that exists to report an
        // unreachable row would never see it.
        assert!(!candidates.contains("LIKE"), "the probe narrowed on a prefix: {candidates}");
        assert_eq!(shape, "^sha256:[0-9a-f]{24}$");
    }

    /// Named so the assertions above read as one line each.
    fn mcpd_store_shape() -> String {
        crate::hashed_id_regex()
    }
}
