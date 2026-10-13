//! Storage for mcpd.
//!
//! Two backends behind one trait:
//!
//! * **SQLite**, compiled in, which is why `mcpd` is a single binary with no
//!   services to stand up. It carries a real full-text index and is the default.
//! * **Postgres**, for the many teams that already run one. It removes the
//!   single-writer and single-replica limits without asking anyone to operate a
//!   database they do not have.
//! * **ClickHouse**, for deployments where retention is measured in billions of
//!   spans rather than millions.
//!
//! The trait is deliberately narrow. Everything above it — the HTTP API, the
//! UI, the CLI — is written against these methods only, so a third backend is a
//! matter of implementing [`Store`] and nothing else.

#![forbid(unsafe_code)]
#![warn(missing_docs)]
// The ClickHouse row writers build a wide object literal per signal; the span
// row alone has forty columns, which is past `json!`'s default expansion depth.
#![recursion_limit = "512"]

use mcpd_core::{
    AnalyticsSummary, AspectNote, AuditEvent, CostBreakdown, Dataset, DatasetItem, EntityKind,
    ErasureResult, Experiment, Feedback, IngestResult, LogLevel, LogRecord, MetricPoint,
    MetricSeriesResponse, Score, ScoreSource, Span, SpanKind, SpanStatus, Theme, ThemeSnapshot,
    Trace, TraceDetail,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;

#[cfg(feature = "clickhouse")]
pub mod clickhouse;
#[cfg(feature = "sqlite")]
pub mod sqlite;

#[cfg(feature = "clickhouse")]
pub use clickhouse::{ClickHouseConfig, ClickHouseStore};

#[cfg(feature = "postgres")]
pub mod postgres;
#[cfg(feature = "postgres")]
pub use postgres::{PostgresConfig, PostgresStore};
#[cfg(feature = "sqlite")]
pub use sqlite::SqliteStore;

/// Anything that can go wrong talking to storage.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The backend rejected or failed a statement.
    #[error("storage backend error: {0}")]
    Backend(String),

    /// A migration could not be applied.
    #[error("migration {version} failed: {reason}")]
    Migration {
        /// Which migration.
        version: u32,
        /// What went wrong.
        reason: String,
    },

    /// The caller asked for something that does not exist.
    #[error("{kind} {id} not found")]
    NotFound {
        /// What kind of thing.
        kind: &'static str,
        /// Its identifier.
        id: String,
    },

    /// A filter or cursor was not usable.
    #[error("invalid query: {0}")]
    InvalidQuery(String),

    /// The store has as many writes waiting as it will hold, and refused this
    /// one rather than queue it.
    ///
    /// A write that waits holds its whole batch in memory, so a queue with no
    /// bound behind a slow writer grows until the process is killed for it.
    /// The refusal is a 503 the SDKs already retry with backoff.
    #[error("{0}")]
    Busy(String),

    /// The write names an id that another owner already holds.
    ///
    /// An experiment id belongs to one project and a dataset item id to one
    /// dataset. Replacing the row regardless moved it to whoever wrote last,
    /// so the write is refused instead and the row stays where it was.
    #[error("{0}")]
    Conflict(String),

    /// This backend does not implement the capability that was asked for.
    ///
    /// Separate from [`Error::Backend`] because it is not a failure: the store
    /// is healthy and the request was well formed. It carries the capability
    /// name so a caller can answer "not implemented" and name the thing, and
    /// the answer agrees with [`StoreCapabilities`] by construction, since a
    /// method returning this is a method whose capability field is false.
    #[error("{backend} does not support {capability}: {reason}")]
    Unsupported {
        /// Which backend refused, the same string as [`Store::name`].
        backend: &'static str,
        /// The capability, named as its field in [`StoreCapabilities`].
        capability: &'static str,
        /// Why it cannot, in a sentence a reader can act on.
        reason: &'static str,
    },
}

/// Convenience alias.
pub type Result<T> = std::result::Result<T, Error>;

/// How to order a list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Order {
    /// Oldest, cheapest or fastest first.
    Asc,
    /// Newest, dearest or slowest first. The default, because investigation
    /// almost always starts from what just happened.
    #[default]
    Desc,
}

/// What to sort a trace list by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TraceSort {
    /// When the run started.
    #[default]
    StartTime,
    /// How long it took.
    Duration,
    /// What it cost.
    Cost,
    /// How many steps it had.
    SpanCount,
    /// The lowest score the run received.
    ///
    /// Read from the `min_score` column of the trace projection, which is
    /// maintained as scores arrive, so this costs no join. A run nothing has
    /// scored yet sorts as if it scored zero, because a keyset cursor carries
    /// one value per row and cannot hold "absent" as a separate rank.
    Score,
}

impl TraceSort {
    /// The value a page cursor carries for `trace` under this sort, as text.
    ///
    /// `min_score` is read from the projection column of that name, because it
    /// is not a field of the trace the caller gets back. The same rule serves
    /// every backend, so a cursor written by one is read the same way by all.
    ///
    /// Cost and score keep every digit they had. Rounding either of them to fit
    /// an integer is what made a cost walk hand back page one for ever.
    #[must_use]
    pub fn cursor_value(self, trace: &Trace, min_score: f64) -> String {
        match self {
            Self::StartTime => trace
                .start_time
                .unix_timestamp_nanos()
                .clamp(i64::MIN.into(), i64::MAX.into())
                .to_string(),
            Self::Duration => trace.duration_ns.unwrap_or(0).to_string(),
            Self::Cost => trace.cost.as_ref().and_then(|c| c.total_cost).unwrap_or(0.0).to_string(),
            Self::SpanCount => trace.span_count.to_string(),
            Self::Score => min_score.to_string(),
        }
    }
}

/// What to sort a span list by.
///
/// Narrower than [`TraceSort`]: a span has no span count, and a score belongs
/// to a run rather than to one step of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SpanSort {
    /// When the step started.
    #[default]
    StartTime,
    /// How long it took.
    Duration,
    /// What it cost.
    Cost,
}

impl SpanSort {
    /// The value a page cursor carries for `span` under this sort, as text.
    ///
    /// Every backend orders on the matching column and resumes from this, so
    /// the two have to agree: a cursor built from one field and compared
    /// against another either repeats a span or skips one. It lives here so
    /// there is one rule rather than one per backend.
    ///
    /// Cost keeps every digit it had. Rounding it to fit an integer is what
    /// made the trace cost cursor hand back page one for ever.
    #[must_use]
    pub fn cursor_value(self, span: &Span) -> String {
        match self {
            Self::StartTime => span
                .start_time
                .unix_timestamp_nanos()
                .clamp(i64::MIN.into(), i64::MAX.into())
                .to_string(),
            Self::Duration => span.duration_ns.unwrap_or(0).to_string(),
            Self::Cost => span.cost.as_ref().and_then(|c| c.total_cost).unwrap_or(0.0).to_string(),
        }
    }
}

/// Filters for a trace list.
///
/// Every field is optional and they combine with AND. An empty filter, apart
/// from `project`, means "everything in this project".
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TraceQuery {
    /// The project to read. The only required field.
    pub project: String,
    /// Full-text search across span names, prompts and completions.
    pub q: Option<String>,
    /// Only runs with this outcome.
    pub status: Option<SpanStatus>,
    /// Only runs whose root is this kind.
    pub kind: Option<SpanKind>,
    /// Exact root span name.
    pub name: Option<String>,
    /// Runs carrying all of these tags.
    pub tags: Vec<String>,
    /// Restrict to one conversation.
    pub session_id: Option<String>,
    /// Restrict to one thread.
    pub thread_id: Option<String>,
    /// Restrict to one end user.
    pub user_id: Option<String>,
    /// Restrict to one environment.
    pub environment: Option<String>,
    /// Only runs that used this model.
    pub model: Option<String>,
    /// Window start, inclusive.
    pub from: Option<OffsetDateTime>,
    /// Window end, exclusive.
    pub to: Option<OffsetDateTime>,
    /// Only runs at least this slow.
    pub min_duration_ms: Option<u64>,
    /// Only runs at least this expensive.
    pub min_cost: Option<f64>,
    /// Only runs carrying a score from this scorer.
    pub has_score: Option<String>,
    /// With `has_score`: only runs whose score came in under this value.
    pub score_below: Option<f64>,
    /// Sort key.
    pub sort: TraceSort,
    /// Sort direction.
    pub order: Order,
    /// Page size.
    pub limit: u32,
    /// Opaque cursor from a previous page.
    pub cursor: Option<String>,
}

/// Filters for a span search.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SpanQuery {
    /// The project to read.
    pub project: String,
    /// Full-text search across name, input and output.
    pub q: Option<String>,
    /// Only this kind of step.
    pub kind: Option<SpanKind>,
    /// Only this outcome.
    pub status: Option<SpanStatus>,
    /// Exact span name.
    pub name: Option<String>,
    /// Only spans against this model.
    pub model: Option<String>,
    /// Restrict to one trace.
    pub trace_id: Option<String>,
    /// Window start.
    pub from: Option<OffsetDateTime>,
    /// Window end.
    pub to: Option<OffsetDateTime>,
    /// Only spans at least this slow.
    pub min_duration_ms: Option<u64>,
    /// Sort key.
    pub sort: SpanSort,
    /// Sort direction.
    pub order: Order,
    /// Page size.
    pub limit: u32,
    /// Opaque cursor.
    pub cursor: Option<String>,
}

/// Filters for a log search.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LogQuery {
    /// The project to read.
    pub project: String,
    /// Full-text search across the message and structured fields.
    pub q: Option<String>,
    /// Minimum severity, inclusive.
    pub level: Option<LogLevel>,
    /// Only logs from this run.
    pub trace_id: Option<String>,
    /// Only logs from this step.
    pub span_id: Option<String>,
    /// Window start.
    pub from: Option<OffsetDateTime>,
    /// Window end.
    pub to: Option<OffsetDateTime>,
    /// Sort direction.
    pub order: Order,
    /// Page size.
    pub limit: u32,
    /// Opaque cursor.
    pub cursor: Option<String>,
}

/// Filters for a metric query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricQuery {
    /// The project to read.
    pub project: String,
    /// Metric name.
    pub name: String,
    /// Window start.
    pub from: OffsetDateTime,
    /// Window end.
    pub to: OffsetDateTime,
    /// Bucket width in seconds. Chosen by the caller, or by the store when zero.
    pub bucket_seconds: u32,
    /// Aggregation to apply within each bucket.
    pub agg: Aggregation,
    /// Label keys to split series on.
    pub group_by: Vec<String>,
    /// Label equality filters, applied before bucketing. Restricting to one
    /// model is far cheaper than returning every series and discarding most of
    /// them in the client.
    pub filter: Vec<(String, String)>,
}

/// Filters for listing stored metric points.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MetricPointQuery {
    /// The project to read.
    pub project: String,
    /// Only this metric.
    pub name: Option<String>,
    /// Window start.
    pub from: Option<OffsetDateTime>,
    /// Window end.
    pub to: Option<OffsetDateTime>,
    /// Label equality filters.
    pub filter: Vec<(String, String)>,
    /// Only points whose exemplar names this run.
    pub trace_id: Option<String>,
    /// Sort direction, by timestamp.
    pub order: Order,
    /// Page size.
    pub limit: u32,
    /// Opaque cursor.
    pub cursor: Option<String>,
}

/// How to combine the raw observations in a bucket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Aggregation {
    /// Add them up. Correct for counters.
    Sum,
    /// How many there were.
    Count,
    /// Arithmetic mean.
    #[default]
    Mean,
    /// Smallest.
    Min,
    /// Largest.
    Max,
    /// Median.
    P50,
    /// Ninetieth percentile.
    P90,
    /// Ninety-fifth percentile.
    P95,
    /// Ninety-ninth percentile.
    P99,
}

/// Filters for a score list.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScoreQuery {
    /// The project to read.
    pub project: String,
    /// Only this scorer.
    pub scorer_id: Option<String>,
    /// Only scores produced this way: live, offline, experiment or manual. A
    /// manual score and a live one answer different questions and are rarely
    /// worth mixing.
    pub source: Option<ScoreSource>,
    /// Only scores for this run.
    pub trace_id: Option<String>,
    /// Only scores recorded under this experiment.
    pub experiment_id: Option<String>,
    /// Lower bound, inclusive.
    pub min_value: Option<f64>,
    /// Upper bound, inclusive.
    pub max_value: Option<f64>,
    /// Window start.
    pub from: Option<OffsetDateTime>,
    /// Window end.
    pub to: Option<OffsetDateTime>,
    /// Page size.
    pub limit: u32,
    /// Opaque cursor.
    pub cursor: Option<String>,
}

/// Filters for an audit-log read.
///
/// Deliberately smaller than the signal queries. The audit log answers a
/// narrow question, and a filter set as wide as `TraceQuery` would invite the
/// log to be used as a second analytics store.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuditQuery {
    /// The project to read.
    pub project: String,
    /// Only rows recording this action, such as `delete_trace`.
    pub action: Option<String>,
    /// Window start.
    pub from: Option<OffsetDateTime>,
    /// Window end.
    pub to: Option<OffsetDateTime>,
    /// Page size.
    pub limit: u32,
}

/// A page of results plus the cursor for the next one.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Paged<T> {
    /// The rows.
    pub items: Vec<T>,
    /// Cursor for the next page, absent on the last.
    pub next_cursor: Option<String>,
    /// Total matching rows, when the backend can count them cheaply.
    pub total: Option<u64>,
}

impl<T> Paged<T> {
    /// Whether another page exists.
    #[must_use]
    pub fn has_more(&self) -> bool {
        self.next_cursor.is_some()
    }
}

/// One batch of signals to persist.
///
/// Passed whole so a backend can write everything in one transaction: a batch
/// that half-lands is worse than one that is rejected, because the gap looks
/// like a bug in the traced system.
#[derive(Debug, Clone, Default)]
pub struct Batch {
    /// Spans to write.
    pub spans: Vec<Span>,
    /// Log records to write.
    pub logs: Vec<LogRecord>,
    /// Metric points to write.
    pub metrics: Vec<MetricPoint>,
    /// Scores to write.
    pub scores: Vec<Score>,
    /// Feedback to write.
    pub feedback: Vec<Feedback>,
    /// Aspect notes to write.
    ///
    /// A note is a sentence about a trace, so it travels with that trace's
    /// other signals, under the same idempotency key and through the same
    /// redactor. A backend files each one under the entity its trace belongs
    /// to, which is why the note itself carries no entity of its own.
    pub aspect_notes: Vec<AspectNote>,
    /// Idempotency key for the whole batch.
    pub ingest_key: Option<String>,
}

impl Batch {
    /// How many signals the batch carries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.spans.len()
            + self.logs.len()
            + self.metrics.len()
            + self.scores.len()
            + self.feedback.len()
            + self.aspect_notes.len()
    }

    /// Whether the batch carries nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/* ------------------------------------------------------- aspect analysis -- */

/// The theme id reserved for the notes that joined no theme.
///
/// A real id and not a null, because it is the id the examples route takes and
/// the id a flow filter names. Storing noise membership under a sentinel keeps
/// "read this theme's notes" one lookup for both cases, where a nullable column
/// would need a second statement and a second code path that nothing exercises
/// until somebody clicks the noise node.
pub const NOISE_THEME_ID: &str = "noise";

/// Filters for an aspect-note read.
///
/// Three callers share it and each fixes a different field: the trace route
/// names a trace, the examples route names a set of note ids, and the
/// clustering pass names a project, an aspect and an entity and walks newest
/// first. One query type rather than three, because they are one index scan
/// with different bounds on it.
#[derive(Debug, Clone)]
pub struct AspectNoteQuery {
    /// The project to read. Always set: it is the tenancy boundary.
    pub project: String,
    /// Only notes about this trace.
    pub trace_id: Option<String>,
    /// Only these notes, by id. Empty means no such restriction.
    pub note_ids: Vec<String>,
    /// Only notes filed under this aspect.
    pub aspect: Option<String>,
    /// Only notes about traces belonging to this entity.
    pub entity: Option<(EntityKind, String)>,
    /// Only notes recorded at or after this.
    pub from: Option<OffsetDateTime>,
    /// Only notes recorded before this.
    pub to: Option<OffsetDateTime>,
    /// Which end of the window to read from. The clustering pass reads
    /// descending, because a cap it reaches must drop the oldest notes and not
    /// the newest.
    pub order: Order,
    /// Page size.
    pub limit: u32,
    /// Opaque cursor from a previous page.
    pub cursor: Option<String>,
}

impl Default for AspectNoteQuery {
    fn default() -> Self {
        Self {
            project: String::new(),
            trace_id: None,
            note_ids: Vec::new(),
            aspect: None,
            entity: None,
            from: None,
            to: None,
            order: Order::Desc,
            limit: 50,
            cursor: None,
        }
    }
}

/// One stored aspect note, with what the row carries beyond the wire entity.
///
/// The entity is denormalised onto the row when the note is written, resolved
/// from the trace's root span. It is not a field of [`AspectNote`] and must not
/// become one: a note is a sentence about a trace, and which agent that trace
/// belongs to is a property of the trace. Denormalising it here is what keeps
/// every read of it a single-table scan, which is what ClickHouse needs, since
/// nothing else in that backend joins.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredNote {
    /// The note itself, exactly as the read API returns it.
    pub note: AspectNote,
    /// The kind of entity its trace belongs to, when that trace was already
    /// stored when the note arrived.
    pub entity_kind: Option<EntityKind>,
    /// The name of that entity.
    pub entity_name: Option<String>,
    /// When the run began, for reading an example against the period it came
    /// from rather than against when somebody wrote about it.
    pub trace_started_at: Option<OffsetDateTime>,
    /// The sketch of the note's wording, as it was stored.
    ///
    /// `None` when the stored bytes are absent or not the shape a sketch takes,
    /// which is what a row written before the column existed looks like. The
    /// clustering pass then sketches the text again rather than failing: the
    /// sketch is a pure function of the text beside it, so recomputing costs a
    /// little arithmetic and never changes an answer.
    pub sketch: Option<mcpd_core::sketch::Sketch>,
}

/// What a roll-up over aspect notes is asked for.
#[derive(Debug, Clone, Default)]
pub struct AspectRollupQuery {
    /// The project to read.
    pub project: String,
    /// Only entities of this kind.
    pub entity_kind: Option<EntityKind>,
    /// Only this one entity, which is what the progress route asks for.
    pub entity: Option<(EntityKind, String)>,
    /// The most groups to return.
    ///
    /// A group is one (entity, aspect) pair, so the number of them is bounded
    /// by how many entities a project has multiplied by how many aspects it
    /// defines. Both are small in every deployment anyone has, and this is here
    /// so that "small" is a bound rather than an assumption.
    pub max_groups: u32,
}

/// How many notes one entity has along one aspect, and when the newest arrived.
///
/// Counted from stored rows every time it is asked for. Nothing caches it,
/// because a cached count is what makes a progress screen say a number the
/// database disagrees with, and this screen exists to be believed.
#[derive(Debug, Clone, PartialEq)]
pub struct AspectNoteRollup {
    /// The kind of entity this row is about.
    pub entity_kind: EntityKind,
    /// Its name.
    pub entity_name: String,
    /// Which aspect the notes are filed under.
    pub aspect: String,
    /// How many notes there are.
    pub note_count: u64,
    /// How many distinct traces they were written about.
    pub trace_count: u64,
    /// When the newest of them was recorded.
    pub last_note_at: OffsetDateTime,
}

/// A theme's lineage at one snapshot, in both directions.
///
/// Both directions are stored rather than derived on read. Deriving `split_into`
/// would mean reading every other theme's `split_from` in the same snapshot,
/// which is a scan of the whole clustering to answer a question about one
/// theme.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThemeLineage {
    /// The themes this one came out of.
    pub split_from: Vec<String>,
    /// The themes that came out of this one.
    pub split_into: Vec<String>,
    /// The themes that became this one.
    pub merged_from: Vec<String>,
    /// The theme this one became.
    pub merged_into: Vec<String>,
}

/// One theme row, with the snapshot facts a history point needs beside it.
///
/// `cutoff_at` and the window bounds are copied from the snapshot onto every
/// theme. That is a denormalisation and it is deliberate: a history is a read
/// of one theme across every snapshot that held it, and joining themes to
/// snapshots to get their times is exactly the join ClickHouse does not do
/// anywhere else in this crate.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredTheme {
    /// The theme, as the read API returns it.
    pub theme: Theme,
    /// Which project it belongs to, which is the tenancy guard on every read.
    pub project: String,
    /// The cutoff of the snapshot it belongs to, which is what a history is
    /// ordered by.
    pub cutoff_at: OffsetDateTime,
    /// The start of that snapshot's window, inclusive.
    pub window_from: OffsetDateTime,
    /// The end of that snapshot's window, exclusive.
    pub window_to: OffsetDateTime,
    /// What happened to it between the previous snapshot and this one.
    pub lineage: ThemeLineage,
}

/// One note's membership of one theme in one snapshot.
///
/// It carries ids and an integer and no prose. The sentence stays in
/// `aspect_notes` and is read from there, so erasing a subject removes the
/// text from every theme's examples by removing it once, rather than by finding
/// every copy of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeMember {
    /// Which snapshot this membership belongs to.
    pub snapshot_id: String,
    /// Which project, which is the tenancy guard.
    pub project: String,
    /// Which aspect it was clustered along.
    pub aspect: String,
    /// The theme it joined, or [`NOISE_THEME_ID`] when it joined none.
    pub theme_id: String,
    /// The note.
    pub note_id: String,
    /// The trace that note was written about.
    pub trace_id: String,
    /// How many of the 64 sketch positions it shares with the theme's label.
    /// Zero for a noise member, which is like nothing in particular.
    pub similarity: u32,
    /// When that run began.
    pub trace_started_at: OffsetDateTime,
}

/// One clustering pass, written whole.
///
/// Passed as one value so a backend can write it in one transaction. A snapshot
/// whose themes landed and whose members did not is a screen that shows a theme
/// and refuses to say what is in it, which reads as a broken deployment rather
/// than as a half-written row.
#[derive(Debug, Clone, PartialEq)]
pub struct SnapshotWrite {
    /// The snapshot header.
    pub snapshot: ThemeSnapshot,
    /// Its themes, across every aspect it covered.
    pub themes: Vec<StoredTheme>,
    /// Every note's membership, noise included.
    pub members: Vec<ThemeMember>,
}

/// Filters for a snapshot read.
#[derive(Debug, Clone, Default)]
pub struct SnapshotQuery {
    /// The project to read.
    pub project: String,
    /// One snapshot by id, which is how every theme route resolves the window
    /// it was asked about.
    pub snapshot_id: Option<String>,
    /// Only snapshots for this entity.
    pub entity: Option<(EntityKind, String)>,
    /// Only snapshots that cover all of these aspects.
    pub aspects: Vec<String>,
    /// Only snapshots whose cutoff falls at or after this.
    pub from: Option<OffsetDateTime>,
    /// Only snapshots whose cutoff falls before this.
    pub to: Option<OffsetDateTime>,
    /// Which end to read from.
    pub order: Order,
    /// Page size.
    pub limit: u32,
    /// Opaque cursor from a previous page.
    pub cursor: Option<String>,
}

/// Filters for a theme read.
#[derive(Debug, Clone, Default)]
pub struct ThemeQuery {
    /// The project to read.
    pub project: String,
    /// One snapshot's themes. Unset walks every snapshot that holds the theme,
    /// which is what a history is.
    pub snapshot_id: Option<String>,
    /// Only themes built for this entity.
    pub entity: Option<(EntityKind, String)>,
    /// Only themes clustered along this aspect.
    pub aspect: Option<String>,
    /// One theme by id.
    pub theme_id: Option<String>,
    /// Which end of the series to read from. A history reads ascending, so the
    /// points come back oldest first and nothing has to be reordered before it
    /// is drawn.
    pub order: Order,
    /// Page size.
    pub limit: u32,
    /// Opaque cursor from a previous page.
    pub cursor: Option<String>,
}

/// Filters for a membership read.
#[derive(Debug, Clone, Default)]
pub struct ThemeMemberQuery {
    /// The project to read.
    pub project: String,
    /// Which snapshot.
    pub snapshot_id: String,
    /// Which aspect. Unset reads every aspect of the snapshot, which is what a
    /// flow needs to pair one trace's themes across stages.
    pub aspect: Option<String>,
    /// Which theme, or [`NOISE_THEME_ID`]. Unset reads every membership.
    pub theme_id: Option<String>,
    /// Page size.
    pub limit: u32,
    /// Opaque cursor from a previous page.
    pub cursor: Option<String>,
}

/* ------------------------------------------------- what a backend can do -- */

/// What one storage backend can and cannot do.
///
/// Three backends implement one trait, and they are not equal. Until this
/// existed the differences were discoverable only by running a query and
/// reading a wrong answer: a reader on ClickHouse who searched for `CAFÉ` and
/// found nothing had no way to learn that this backend folds ASCII case and no
/// more. That is the failure this type exists to prevent. Every field here is
/// a statement the backend makes about itself, the HTTP layer can hand it to
/// the interface, and the interface can say "this deployment cannot do that"
/// instead of drawing an empty list.
///
/// Two rules hold it honest, and both are measured in
/// `crates/mcpd-store/tests/conformance.rs`:
///
/// 1. A field that says `true` must be backed by behaviour. A backend that
///    advertises [`discovery`](Self::discovery) and then returns
///    [`Error::Unsupported`] from [`Store::discover`] fails the suite.
/// 2. A field that says `false` must be backed by a refusal. A method whose
///    capability is off returns [`Error::Unsupported`] naming that same field,
///    so the two can never drift apart in one direction only.
///
/// It is `Copy` and holds no allocation, so a handler can read it per request
/// without caching it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct StoreCapabilities {
    /// Which backend this describes. The same string as [`Store::name`].
    pub backend: &'static str,

    // --------------------------------------------------------------- reads
    /// What a `q` filter matches.
    pub text_search: TextSearch,
    /// Whether `q` folds case for letters outside ASCII.
    ///
    /// False means a search for `CAFÉ` does not find `Café`, which a reader
    /// otherwise discovers as an empty result set they cannot explain.
    pub unicode_case_folding: bool,
    /// Whether a delete is on disk when the call returns.
    ///
    /// False means the backend accepted the delete and applies it afterwards,
    /// so the counts say what matched rather than what has already gone. It is
    /// the same fact [`ErasureOutcome::deferred`] reports for one erasure,
    /// stated once for the backend.
    pub immediate_deletes: bool,

    // ----------------------------------------------------------- discovery
    /// Whether [`Store::discover`] answers.
    pub discovery: bool,
    /// Whether [`Facet::MetricLabelValue`] is answerable, not only the keys.
    ///
    /// Separate from [`discovery`](Self::discovery) because the values are the
    /// expensive half: a key set is small and bounded, and a value set is not.
    pub discovery_label_values: bool,

    // ------------------------------------------------------------- polling
    /// Whether [`Store::poll`] answers, so a list can refresh from a cursor
    /// rather than re-running the whole query.
    pub delta_polling: bool,

    // ----------------------------------------------------------- retention
    /// Whether [`Store::apply_retention`] takes a window per signal.
    pub per_signal_retention: bool,
    /// Whether [`Store::declare_retention`] can hand the window to the engine.
    ///
    /// True means the deployment's retention can be declared once and enforced
    /// by the database itself, so nothing has to run a delete on a timer. It is
    /// deployment-wide by nature: a table's declared retention cannot say
    /// anything about one project, which is why it is a separate method rather
    /// than something `apply_retention` does on the side.
    pub declarative_retention: bool,
    /// How the anchor time is partitioned, when it is.
    pub partitioning: Partitioning,
    /// Whether a retention pass can drop a whole partition.
    ///
    /// False means retention is a row delete: correct, and it reads and
    /// rewrites the rows it removes. True means expiring a day is a metadata
    /// operation on one part.
    pub partition_drop_retention: bool,

    // -------------------------------------------------------------- schema
    /// Whether a start with nothing to apply issues no schema statement at all.
    ///
    /// False means every process replays the DDL on every boot, which is
    /// harmless on one node and is metadata churn across a replicated
    /// deployment where each replica takes locks the others wait on.
    pub idempotent_schema_init: bool,

    // ------------------------------------------------------------- analysis
    /// Whether this backend holds aspect notes, snapshots and themes.
    ///
    /// False means the nine `Intelligence` endpoints refuse with 501 naming
    /// this field, and a caller that reads the capability set once knows not to
    /// ask again. It is one flag for the whole family on purpose: a deployment
    /// that can store a note but not a theme would answer some of those routes
    /// and not others, and a reader would have to learn the difference one
    /// refusal at a time.
    pub theme_analysis: bool,
}

impl StoreCapabilities {
    /// What a backend that has only the trait's required methods can do.
    ///
    /// Everything optional is off. This is what an implementation that has not
    /// declared its capabilities reports, and it is the safe direction to be
    /// wrong in: a caller offers less than the backend has rather than calling
    /// a method that then refuses.
    #[must_use]
    pub const fn minimal(backend: &'static str) -> Self {
        Self {
            backend,
            text_search: TextSearch::None,
            unicode_case_folding: false,
            immediate_deletes: true,
            discovery: false,
            discovery_label_values: false,
            delta_polling: false,
            per_signal_retention: false,
            declarative_retention: false,
            partitioning: Partitioning::None,
            partition_drop_retention: false,
            idempotent_schema_init: false,
            theme_analysis: false,
        }
    }

    /// Every capability, as `(field name, what it answers)`.
    ///
    /// Named the same as the fields, because [`Error::Unsupported`] carries a
    /// field name and a reader comparing the two should not have to translate.
    /// The conformance suite walks this rather than the struct, so a field
    /// added without a line here is a field nothing measures.
    #[must_use]
    pub fn fields(&self) -> [(&'static str, String); 12] {
        [
            ("text_search", self.text_search.as_str().to_owned()),
            ("unicode_case_folding", self.unicode_case_folding.to_string()),
            ("immediate_deletes", self.immediate_deletes.to_string()),
            ("discovery", self.discovery.to_string()),
            ("discovery_label_values", self.discovery_label_values.to_string()),
            ("delta_polling", self.delta_polling.to_string()),
            ("per_signal_retention", self.per_signal_retention.to_string()),
            ("declarative_retention", self.declarative_retention.to_string()),
            ("partitioning", self.partitioning.as_str().to_owned()),
            ("partition_drop_retention", self.partition_drop_retention.to_string()),
            ("idempotent_schema_init", self.idempotent_schema_init.to_string()),
            ("theme_analysis", self.theme_analysis.to_string()),
        ]
    }
}

/// What a `q` filter matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextSearch {
    /// `q` is ignored. Nothing here declares this, and a backend that did
    /// would be telling the interface to hide the search box rather than
    /// letting a reader type into one that silently returns everything.
    None,
    /// `q` matches anywhere inside a word, so `heck` finds `checkout`.
    Substring,
    /// `q` matches whole words only, which is what a person typing two words
    /// into a search box expects.
    Words,
}

impl TextSearch {
    /// The name this takes on the wire and in the conformance report.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Substring => "substring",
            Self::Words => "words",
        }
    }
}

/// How a signal table is partitioned on its anchor time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Partitioning {
    /// One table, no partitions.
    None,
    /// One partition per day.
    Day,
    /// One partition per month.
    Month,
}

impl Partitioning {
    /// The name this takes on the wire and in the conformance report.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Day => "day",
            Self::Month => "month",
        }
    }
}

/* --------------------------------------------------------------- discovery -- */

/// A dimension a filter bar can offer values for.
///
/// The set is closed on purpose. Each one is a column or a parsed field that a
/// backend maintains a distinct-value list for as data arrives, so answering is
/// a read of a small table rather than a `SELECT DISTINCT` over every span in
/// the retention window. Adding a facet means adding the write that feeds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Facet {
    /// The kind of thing a step was: agent, workflow, tool, and so on.
    EntityKind,
    /// The name of that thing, such as the tool that was called.
    EntityName,
    /// The service that reported the signal.
    ServiceName,
    /// The environment it came from.
    Environment,
    /// A tag on a run.
    Tag,
    /// A metric name.
    MetricName,
    /// A label key a metric point carries.
    MetricLabelKey,
    /// A value one label key takes. Needs [`DiscoveryQuery::key`].
    MetricLabelValue,
}

impl Facet {
    /// The name this takes on the wire.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EntityKind => "entity_kind",
            Self::EntityName => "entity_name",
            Self::ServiceName => "service_name",
            Self::Environment => "environment",
            Self::Tag => "tag",
            Self::MetricName => "metric_name",
            Self::MetricLabelKey => "metric_label_key",
            Self::MetricLabelValue => "metric_label_value",
        }
    }

    /// Read a facet back from its wire name.
    ///
    /// # Errors
    /// Fails when the name is not one of the eight, naming the ones that are.
    /// A typo in a query parameter is otherwise an empty list, which reads as
    /// "this project has no environments" rather than as a mistake.
    pub fn parse(name: &str) -> Result<Self> {
        Ok(match name {
            "entity_kind" => Self::EntityKind,
            "entity_name" => Self::EntityName,
            "service_name" => Self::ServiceName,
            "environment" => Self::Environment,
            "tag" => Self::Tag,
            "metric_name" => Self::MetricName,
            "metric_label_key" => Self::MetricLabelKey,
            "metric_label_value" => Self::MetricLabelValue,
            other => {
                return Err(Error::InvalidQuery(format!(
                    "unknown facet {other:?}: use entity_kind, entity_name, service_name, \
                     environment, tag, metric_name, metric_label_key or metric_label_value"
                )))
            }
        })
    }

    /// Whether this facet is a value of the key in [`DiscoveryQuery::key`].
    #[must_use]
    pub const fn needs_key(self) -> bool {
        matches!(self, Self::MetricLabelValue)
    }
}

/// What to offer in a filter box.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryQuery {
    /// The project to read. Every facet is scoped to one.
    pub project: String,
    /// Which dimension.
    pub facet: Facet,
    /// For [`Facet::MetricLabelValue`], the label key. Ignored otherwise.
    pub key: Option<String>,
    /// Offer only values starting with this, for typeahead. Matched literally
    /// and case sensitively, because the stored values are what a filter will
    /// be compared against and folding here would offer one that matches
    /// nothing.
    pub prefix: Option<String>,
    /// How many to return. Clamped by the backend to
    /// [`DiscoveryQuery::MAX_LIMIT`].
    pub limit: u32,
}

impl DiscoveryQuery {
    /// The most values one call returns, whatever it asks for.
    ///
    /// A filter box shows a list somebody reads. Handing back ten thousand
    /// environments is not a longer list, it is a slower request that renders
    /// the same first page.
    pub const MAX_LIMIT: u32 = 1_000;

    /// The limit to actually use, with zero meaning the default.
    #[must_use]
    pub fn effective_limit(&self) -> u32 {
        match self.limit {
            0 => 100,
            n => n.min(Self::MAX_LIMIT),
        }
    }
}

/// One value a facet takes, and when it was last seen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Discovered {
    /// The value, exactly as it is stored, so it can be used as a filter
    /// without any further mapping.
    pub value: String,
    /// When a signal last carried it.
    ///
    /// Carried so a caller can grey out or rank down a value nothing has
    /// reported for a month. Values come back ordered by `value`, not by this:
    /// a filter list a person reads alphabetically is stable between polls,
    /// and one ordered by recency reshuffles under them as data arrives.
    #[serde(with = "time::serde::rfc3339")]
    pub last_seen: OffsetDateTime,
}

/// The longest facet value worth remembering.
///
/// A facet feeds a list somebody reads, and nothing a person picks off a
/// dropdown is longer than this. The cap is a guard rather than a nicety: an
/// `entity_name` or a label value is caller-supplied text, and without a bound
/// a service that puts a whole prompt in one would have that prompt copied into
/// a table that retention does not reach on the same schedule as the span.
pub const MAX_FACET_VALUE: usize = 256;

/// One row for the distinct-value list, as the ingest path produces it.
///
/// `project` is on the row rather than on the call because a batch is not
/// necessarily one project's: every signal carries its own correlation, and a
/// collector fanning several services into one export writes them together.
/// Taking the project from the request instead is how a value from one tenant
/// ends up offered to another.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct FacetValue {
    /// Which project the signal that carried it belonged to.
    pub project: String,
    /// Which dimension.
    pub facet: Facet,
    /// The label key, for [`Facet::MetricLabelValue`], and empty otherwise.
    pub key: String,
    /// The value.
    pub value: String,
}

/// Whether a row with this id already belongs to a different project.
///
/// Every signal id is chosen by the sender, and no backend keys a signal by
/// its project. Replacing the row regardless let one project overwrite another
/// project's span, score or note by sending a record with the same id, and a
/// span id travels to every downstream service in `traceparent`, so knowing
/// one takes no guessing. An id belongs to the first project that stored it; a
/// replay from that project still replaces its own row.
pub(crate) fn foreign(owner: Option<&str>, project: &str) -> bool {
    owner.is_some_and(|o| o != project)
}

/// The refusal for an experiment id another project already holds.
pub(crate) fn experiment_taken(id: &str) -> Error {
    Error::Conflict(format!(
        "experiment {id} already belongs to another project; send the experiment with no id \
         and the daemon assigns one"
    ))
}

/// The refusal for a dataset item id another dataset already holds.
pub(crate) fn item_taken(id: &str) -> Error {
    Error::Conflict(format!(
        "dataset item {id} already belongs to another dataset; send the item with no id and \
         the daemon assigns one"
    ))
}

/// Count one signal refused because its id belongs to another project.
///
/// The message names the id and never the project that holds it: the sender
/// learns that its record was not stored and why, not who else uses the id.
pub(crate) fn refuse_foreign(
    result: &mut IngestResult,
    signal: mcpd_core::SignalType,
    index: usize,
    what: &str,
    id: &str,
) {
    result.rejected += 1;
    result.errors.get_or_insert_with(Vec::new).push(mcpd_core::IngestError {
        signal,
        index: u32::try_from(index).unwrap_or(u32::MAX),
        message: format!(
            "{what} {id} is already stored in another project; an id belongs to one \
             project, so send this record with a new id"
        ),
    });
}

/// The distinct facet values one batch carries.
///
/// Written once and shared by all three backends, so the answer to "what does
/// the filter box offer" cannot depend on where the data landed. The result is
/// sorted and deduplicated, which is what makes the write below it a fixed
/// number of upserts rather than one per row.
///
/// Empty values are dropped rather than stored as a blank entry: the columns
/// are optional and an absent environment is not an environment called "".
/// Values past [`MAX_FACET_VALUE`] are dropped too.
#[must_use]
pub fn facet_values(batch: &Batch) -> Vec<FacetValue> {
    let mut out: Vec<FacetValue> = Vec::new();
    let mut push = |project: &str, facet: Facet, key: &str, value: &str| {
        if project.is_empty() || value.is_empty() || value.len() > MAX_FACET_VALUE {
            return;
        }
        out.push(FacetValue {
            project: project.to_owned(),
            facet,
            key: key.to_owned(),
            value: value.to_owned(),
        });
    };

    for span in &batch.spans {
        let p = span.correlation.project.as_str();
        if let Some(k) = span.entity_kind {
            push(p, Facet::EntityKind, "", k.as_str());
        }
        if let Some(n) = &span.entity_name {
            push(p, Facet::EntityName, "", n);
        }
        if let Some(s) = &span.correlation.service_name {
            push(p, Facet::ServiceName, "", s);
        }
        if let Some(e) = &span.correlation.environment {
            push(p, Facet::Environment, "", e);
        }
        for tag in span.tags.iter().flatten() {
            push(p, Facet::Tag, "", tag);
        }
    }
    for log in &batch.logs {
        let p = log.correlation.project.as_str();
        if let Some(s) = &log.correlation.service_name {
            push(p, Facet::ServiceName, "", s);
        }
        if let Some(e) = &log.correlation.environment {
            push(p, Facet::Environment, "", e);
        }
    }
    for m in &batch.metrics {
        let p = m.correlation.project.as_str();
        push(p, Facet::MetricName, "", &m.name);
        if let Some(e) = &m.correlation.environment {
            push(p, Facet::Environment, "", e);
        }
        // Labels are a JSON object. A point that carries something else is not
        // rejected here: ingest already accepted it, and refusing to index it
        // is the whole of the harm.
        for (k, v) in m.labels.as_ref().and_then(Value::as_object).into_iter().flatten() {
            push(p, Facet::MetricLabelKey, "", k);
            // A label value is only worth offering when it is a scalar. An
            // object rendered as JSON is not something a filter can be set to.
            if let Some(v) = v.as_str() {
                push(p, Facet::MetricLabelValue, k, v);
            } else if v.is_number() || v.is_boolean() {
                push(p, Facet::MetricLabelValue, k, &v.to_string());
            }
        }
    }
    for s in &batch.scores {
        if let Some(e) = &s.correlation.environment {
            push(s.correlation.project.as_str(), Facet::Environment, "", e);
        }
    }
    for f in &batch.feedback {
        if let Some(e) = &f.correlation.environment {
            push(f.correlation.project.as_str(), Facet::Environment, "", e);
        }
    }

    out.sort();
    out.dedup();
    out
}

/// The smallest string that sorts after every string starting with `prefix`.
///
/// Typeahead is a prefix match, and a prefix match written as `LIKE 'x%'` or
/// `substr(value, 1, n) = 'x'` reads the whole list to answer. Written as
/// `value >= 'x' AND value < <this>` it is a range over the primary key, which
/// is the point of keeping the list in a table sorted by value.
///
/// `None` when there is no such string, which happens only for a prefix ending
/// in the highest code point there is. The caller then drops the upper bound
/// and keeps the lower, which is still correct and still a range scan.
#[must_use]
pub fn prefix_upper_bound(prefix: &str) -> Option<String> {
    let mut head = prefix.to_owned();
    while let Some(last) = head.pop() {
        // Skip the surrogate block: those code points are not chars, so the
        // successor of U+D7FF is U+E000 and not U+D800.
        let next = (u32::from(last) + 1..=u32::from(char::MAX)).find_map(char::from_u32);
        if let Some(next) = next {
            head.push(next);
            return Some(head);
        }
    }
    None
}

/* ----------------------------------------------------------------- polling -- */

/// A signal that can be polled for what is new.
///
/// Only the three that carry a server-set arrival time. Scores and feedback
/// are stamped with the caller's `timestamp` and nothing else, so there is no
/// value on the row that a watermark could advance over without either
/// skipping a late row or repeating every row for ever.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DeltaSignal {
    /// The run projection, which is what a run list refreshes from.
    #[default]
    Traces,
    /// Steps, for a waterfall that is still filling in.
    Spans,
    /// Log lines.
    Logs,
}

impl DeltaSignal {
    /// The name this takes on the wire.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Traces => "traces",
            Self::Spans => "spans",
            Self::Logs => "logs",
        }
    }

    /// Read one back from its wire name.
    ///
    /// # Errors
    /// Fails when the name is not one of the three, naming them.
    pub fn parse(name: &str) -> Result<Self> {
        Ok(match name {
            "traces" => Self::Traces,
            "spans" => Self::Spans,
            "logs" => Self::Logs,
            other => {
                return Err(Error::InvalidQuery(format!(
                    "cannot poll {other:?}: poll traces, spans or logs. Scores and feedback \
                     carry no server-set arrival time, so a watermark over them would either \
                     skip a late row or repeat every row."
                )))
            }
        })
    }
}

/// What has arrived since the caller last looked.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaQuery {
    /// The project to read.
    pub project: String,
    /// Which signal.
    pub signal: DeltaSignal,
    /// The cursor the previous poll returned.
    ///
    /// Absent starts from [`since`](Self::since), or from the oldest row the
    /// project holds when that is absent too.
    pub cursor: Option<String>,
    /// Where to start when there is no cursor yet.
    pub since: Option<OffsetDateTime>,
    /// Page size.
    pub limit: u32,
}

/// One row a poll returned.
///
/// [`Store::poll`] is called through `dyn Store`, so it cannot be generic over
/// the signal. Every item in one [`Delta`] is the variant matching
/// [`DeltaQuery::signal`]; the enum exists to satisfy the object, not to invite
/// a mixed page.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolledSignal {
    /// A run projection.
    Trace(Box<Trace>),
    /// A step.
    Span(Box<Span>),
    /// A log line.
    Log(Box<LogRecord>),
}

impl PolledSignal {
    /// The run this row belongs to, which is the only field every variant has
    /// and the one a caller keys its list on.
    ///
    /// A log line need not name a run, so this is optional for that variant
    /// alone.
    #[must_use]
    pub fn trace_id(&self) -> Option<&str> {
        match self {
            Self::Trace(t) => Some(&t.trace_id),
            Self::Span(s) => Some(&s.trace_id),
            Self::Log(l) => l.trace_id.as_deref(),
        }
    }

    /// Which signal this is, matching [`DeltaSignal::as_str`].
    #[must_use]
    pub const fn signal(&self) -> DeltaSignal {
        match self {
            Self::Trace(_) => DeltaSignal::Traces,
            Self::Span(_) => DeltaSignal::Spans,
            Self::Log(_) => DeltaSignal::Logs,
        }
    }
}

/// A page of what is new, and where to resume.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Delta<T> {
    /// The rows, oldest arrival first. Ascending on purpose: a poll is applied
    /// to a list the caller already holds, and applying it in arrival order is
    /// what makes the result the same as having listed afresh.
    pub items: Vec<T>,
    /// Where the next poll resumes.
    ///
    /// Always present, and unchanged from the request when nothing came back.
    /// Moving it forward on an empty poll is what would drop a row that lands
    /// a moment later with an earlier arrival time.
    pub cursor: String,
    /// True when the page filled and more is waiting. A caller draining a
    /// backlog polls again at once rather than waiting for its next tick.
    pub has_more: bool,
}

/// Encode a delta watermark. Opaque by construction, and shared by every
/// backend so a cursor issued by one reads the same in all three.
///
/// The pair is the arrival time in nanoseconds and the row's own id, because
/// arrival time alone is not unique: a batch writes every row in it with one
/// timestamp, and a watermark that could only say "after this nanosecond"
/// would either replay the whole batch or skip the rest of it.
#[must_use]
pub fn encode_delta_cursor(received_at_nanos: i64, id: &str) -> String {
    hex::encode(format!("{received_at_nanos}|{id}"))
}

/// Read a watermark back.
///
/// # Errors
/// Fails when the cursor was not produced by [`encode_delta_cursor`]. A caller
/// must never build one: the encoding is free to change precisely because
/// nothing outside this file reads it.
pub fn decode_delta_cursor(cursor: &str) -> Result<(i64, String)> {
    let bad = || {
        Error::InvalidQuery(
            "malformed poll cursor: pass back the one the last poll returned, unchanged".to_owned(),
        )
    };
    let raw = hex::decode(cursor).ok().and_then(|b| String::from_utf8(b).ok()).ok_or_else(bad)?;
    let (v, id) = raw.split_once('|').ok_or_else(bad)?;
    Ok((v.parse().map_err(|_| bad())?, id.to_owned()))
}

/* --------------------------------------------------------------- retention -- */

/// How long to keep each signal, in days.
///
/// One number per project was the whole of retention here, and it cannot say
/// the thing every deployment actually wants: logs are the bulk of the bytes
/// and the least valuable after a week, while scores are tiny and are the
/// record of how a system behaved over a quarter. `None` for a signal means
/// keep it, which is the only honest reading of an unset window.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetentionPolicy {
    /// Days to keep runs and their steps. Spans and the trace projection go
    /// together: a run whose steps have expired is a row that lists a duration
    /// and opens onto nothing.
    pub traces: Option<u32>,
    /// Days to keep log lines.
    pub logs: Option<u32>,
    /// Days to keep metric points.
    pub metrics: Option<u32>,
    /// Days to keep scores.
    pub scores: Option<u32>,
    /// Days to keep feedback.
    pub feedback: Option<u32>,
}

impl RetentionPolicy {
    /// The same window for every signal, which is what one `--retention-days`
    /// meant before this existed.
    #[must_use]
    pub const fn uniform(days: u32) -> Self {
        let d = Some(days);
        Self { traces: d, logs: d, metrics: d, scores: d, feedback: d }
    }

    /// True when no signal has a window, so a pass would delete nothing.
    #[must_use]
    pub const fn keeps_everything(&self) -> bool {
        self.traces.is_none()
            && self.logs.is_none()
            && self.metrics.is_none()
            && self.scores.is_none()
            && self.feedback.is_none()
    }

    /// Every window, as `(signal name, days)`, in a fixed order.
    ///
    /// The names are the wire names and the field names, so a backend's SQL, a
    /// log line and this struct all call the same thing by the same word.
    #[must_use]
    pub const fn windows(&self) -> [(&'static str, Option<u32>); 5] {
        [
            ("traces", self.traces),
            ("logs", self.logs),
            ("metrics", self.metrics),
            ("scores", self.scores),
            ("feedback", self.feedback),
        ]
    }

    /// The cutoff for one window: everything anchored before this goes.
    ///
    /// `None` when the signal has no window. Saturating, so a window longer
    /// than the age of the epoch clamps rather than wrapping into the future
    /// and deleting everything, which is the one arithmetic mistake in a
    /// retention pass nobody can undo.
    #[must_use]
    pub fn cutoff(days: Option<u32>, now: OffsetDateTime) -> Option<OffsetDateTime> {
        let days = days?;
        let span = time::Duration::days(i64::from(days));
        Some(now.checked_sub(span).unwrap_or(OffsetDateTime::UNIX_EPOCH))
    }
}

/// What one retention pass did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RetentionOutcome {
    /// Rows removed, or accepted for removal on a backend whose deletes are
    /// deferred. Not counting rows that left with a dropped partition, which
    /// are counted as partitions: a partition drop reads no rows and therefore
    /// cannot honestly report how many were in it.
    pub removed: u64,
    /// Whole partitions dropped.
    pub partitions_dropped: u32,
    /// Whether this call changed a table's declared retention.
    ///
    /// The idempotency probe, and only [`Store::declare_retention`] ever sets
    /// it. A second call under the same policy must leave it false: on a
    /// replicated deployment an `ALTER ... MODIFY TTL` is metadata every
    /// replica has to agree on, and issuing one on every retention tick is
    /// churn that buys nothing.
    ///
    /// [`Store::apply_retention`] always leaves it false, because a declared
    /// retention is a property of a table and that method is given one project.
    /// Declaring one project's window on a table every project shares would
    /// expire the other tenants' data on a window nobody chose for them.
    pub ttl_changed: bool,
}

/// What every storage backend must do.
#[async_trait::async_trait]
pub trait Store: Send + Sync + 'static {
    /// A short name for this backend, reported on `/healthz`.
    fn name(&self) -> &'static str;

    /// What this backend can and cannot do.
    ///
    /// The default is [`StoreCapabilities::minimal`]: everything optional off.
    /// An implementation that has not thought about this therefore promises
    /// nothing, which is the direction to be wrong in. Every backend that ships
    /// overrides it, and the conformance suite proves each field against
    /// behaviour rather than against the other backends.
    fn capabilities(&self) -> StoreCapabilities {
        StoreCapabilities::minimal(self.name())
    }

    /// Apply any outstanding migrations. Safe to call on every start.
    async fn migrate(&self) -> Result<()>;

    /// Cheap liveness check for the readiness probe.
    async fn ping(&self) -> Result<()>;

    /// Persist a batch, atomically.
    async fn ingest(&self, batch: Batch) -> Result<IngestResult>;

    /// List traces.
    async fn list_traces(&self, q: &TraceQuery) -> Result<Paged<Trace>>;

    /// Fetch one trace with its spans, and optionally everything attached.
    async fn get_trace(
        &self,
        project: &str,
        trace_id: &str,
        include: &Include,
    ) -> Result<TraceDetail>;

    /// Delete a trace and every signal attached to it.
    async fn delete_trace(&self, project: &str, trace_id: &str) -> Result<u32>;

    /// Erase everything recorded about one person, within one project.
    ///
    /// `user_id` arrives already hashed, the same way it was hashed on the way
    /// in. A backend compares it to the stored column and never re-hashes.
    ///
    /// The unit of erasure is the run, not the row. Only the root span usually
    /// carries `user_id`, so deleting the rows that match it by hand would
    /// leave the child spans standing, and the child spans are the ones holding
    /// the prompt and the completion. A run is therefore in scope when its
    /// trace projection names the subject, or when any of its spans does, and
    /// then the whole run goes: spans, logs, metrics, scores and feedback.
    ///
    /// Log lines are also matched on their own `user_id`, because a log can be
    /// recorded against a person with no trace to hang it on.
    ///
    /// Three further reaches are part of the contract, and each one was a row
    /// left standing before it was:
    ///
    /// - **By span.** A score, a piece of feedback, a metric point or a log
    ///   line can name a `span_id` and no `trace_id` at all. Those rows hang
    ///   off the subject's step and carry the subject's content, so the span
    ///   ids of every run in scope are matched as well as the trace ids.
    /// - **By author.** `feedback.author_id` is hashed by the same rule as
    ///   `user_id`, and the row holds the person's own prose. It is matched on
    ///   the subject too, wherever it hangs.
    /// - **Captured cases.** A dataset item captured from a run in scope holds
    ///   a verbatim copy of the production input. The copy is emptied in place
    ///   rather than deleted, so a regression case is never destroyed without
    ///   anyone being told, and the count and the datasets come back in
    ///   [`ErasureOutcome`] so the answer to the person can say so.
    ///
    /// `project` bounds the whole operation. A subject present in another
    /// project must be left untouched and must not be counted: the caller has
    /// been authorised for this project only, and a count that moved would
    /// disclose that the subject exists in a project they cannot read.
    async fn erase_subject(&self, project: &str, user_id: &str) -> Result<ErasureOutcome>;

    /// Search spans across traces.
    async fn list_spans(&self, q: &SpanQuery) -> Result<Paged<Span>>;

    /// Search logs.
    async fn list_logs(&self, q: &LogQuery) -> Result<Paged<LogRecord>>;

    /// Query a metric as an aggregated time series.
    async fn query_metrics(&self, q: &MetricQuery) -> Result<MetricSeriesResponse>;

    /// List stored metric points as they were recorded, keyset paged on
    /// `(timestamp, metric_id)`.
    async fn list_metric_points(&self, q: &MetricPointQuery) -> Result<Paged<MetricPoint>>;

    /// List scores.
    async fn list_scores(&self, q: &ScoreQuery) -> Result<Paged<Score>>;

    /// Headline numbers for a window.
    async fn analytics_summary(
        &self,
        project: &str,
        from: OffsetDateTime,
        to: OffsetDateTime,
        group_by: Option<&str>,
    ) -> Result<AnalyticsSummary>;

    /// Cost, broken down.
    async fn analytics_cost(
        &self,
        project: &str,
        from: OffsetDateTime,
        to: OffsetDateTime,
        group_by: &str,
        bucket_seconds: Option<u32>,
    ) -> Result<CostBreakdown>;

    /// Delete signals older than the retention window. Returns how many rows
    /// went. Called on a timer by the server.
    async fn enforce_retention(&self, project: &str, older_than: OffsetDateTime) -> Result<u64>;

    /// Apply a window per signal, and say what changed.
    ///
    /// The wider [`enforce_retention`](Self::enforce_retention) above is one
    /// cutoff for everything and stays because it is what the daemon's timer
    /// calls today. This one is the shape a deployment actually asks for, and
    /// it is idempotent in the sense that matters on a cluster: a second pass
    /// under the same policy with nothing newly expired removes no rows and
    /// issues no schema statement, which
    /// [`RetentionOutcome::ttl_changed`] reports.
    ///
    /// `now` is passed rather than read, so a test can age data without
    /// waiting and two replicas running the pass at once compute the same
    /// cutoff.
    ///
    /// # Errors
    /// [`Error::Unsupported`] when
    /// [`StoreCapabilities::per_signal_retention`] is false.
    async fn apply_retention(
        &self,
        project: &str,
        policy: &RetentionPolicy,
        now: OffsetDateTime,
    ) -> Result<RetentionOutcome> {
        let _ = (project, policy, now);
        Err(Error::Unsupported {
            backend: self.name(),
            capability: "per_signal_retention",
            reason: "this backend expires every signal on one window, so a per-signal policy \
                     would silently apply the wrong one to four of them",
        })
    }

    /// Hand the deployment's retention to the engine, so nothing has to run a
    /// delete on a timer.
    ///
    /// Deployment-wide, with no project, and that is not an oversight. A
    /// declared retention is a property of a table, and every project shares the
    /// tables: declaring one project's window would expire every other tenant's
    /// data on a window nobody chose for them.
    /// [`apply_retention`](Self::apply_retention) is the per-project half, and
    /// it deletes rows.
    ///
    /// Idempotent, and [`RetentionOutcome::ttl_changed`] is how that is
    /// checked. A second call under the same policy must report false: the
    /// statement is metadata every replica of a replicated table has to agree
    /// on, and issuing it on every tick is churn that buys nothing.
    ///
    /// # Errors
    /// [`Error::Unsupported`] when
    /// [`StoreCapabilities::declarative_retention`] is false.
    async fn declare_retention(&self, policy: &RetentionPolicy) -> Result<RetentionOutcome> {
        let _ = policy;
        Err(Error::Unsupported {
            backend: self.name(),
            capability: "declarative_retention",
            reason: "this backend has no engine-level expiry, so retention runs as a delete on \
                     a timer",
        })
    }

    /// Every project that has data, for the project switcher.
    async fn list_projects(&self) -> Result<Vec<String>>;

    /// The values one facet takes in a project, for a filter box.
    ///
    /// Answered from a maintained distinct-value list rather than by scanning
    /// the signal tables, because the question is asked on every keystroke in a
    /// filter and the tables it would otherwise scan are the largest thing the
    /// deployment holds.
    ///
    /// Ordered by value, ascending, so a list a person is reading does not
    /// reshuffle under them between polls.
    ///
    /// # Errors
    /// [`Error::Unsupported`] when [`StoreCapabilities::discovery`] is false,
    /// and for [`Facet::MetricLabelValue`] when
    /// [`StoreCapabilities::discovery_label_values`] is. Also fails when that
    /// facet is asked without [`DiscoveryQuery::key`], because the answer would
    /// otherwise be every value of every label mixed together.
    async fn discover(&self, q: &DiscoveryQuery) -> Result<Vec<Discovered>> {
        let _ = q;
        Err(Error::Unsupported {
            backend: self.name(),
            capability: "discovery",
            reason: "this backend keeps no distinct-value list, so a filter box has to be                      filled from what is already on screen",
        })
    }

    /// What has arrived since a cursor, for a list that refreshes in place.
    ///
    /// A poll is not a page. Paging walks a fixed result set backwards in the
    /// sort the caller chose; polling walks forwards in arrival order and never
    /// ends, so the two cursors are different things and are deliberately not
    /// interchangeable.
    ///
    /// The watermark is the row's arrival time and its id. A row written with
    /// an arrival time behind the watermark, which a caller can cause by
    /// setting `received_at` itself, is not returned. That is the cost of a
    /// watermark and there is no cursor shape that avoids it.
    ///
    /// # Errors
    /// [`Error::Unsupported`] when [`StoreCapabilities::delta_polling`] is
    /// false, and [`Error::InvalidQuery`] for a cursor this store did not
    /// issue.
    async fn poll(&self, q: &DeltaQuery) -> Result<Delta<PolledSignal>> {
        let _ = q;
        Err(Error::Unsupported {
            backend: self.name(),
            capability: "delta_polling",
            reason: "this backend has no arrival-ordered read, so a list refreshes by running                      its query again",
        })
    }

    // ------------------------------------------------------------------ audit

    /// Record one access to stored payloads, or one deletion.
    ///
    /// The caller must never let a failure here fail the request being audited.
    /// An audit log that can take down the read path is a worse liability than
    /// the gap it is guarding against, so the server logs and counts a failure
    /// instead. That is a decision the server makes, not this trait: a backend
    /// still reports honestly whether the row landed.
    async fn record_audit(&self, event: &AuditEvent) -> Result<()>;

    /// Read the audit log, newest first.
    async fn list_audit(&self, q: &AuditQuery) -> Result<Vec<AuditEvent>>;

    // ------------------------------------------------- datasets and experiments

    /// Create or replace a dataset.
    async fn put_dataset(&self, dataset: &Dataset) -> Result<()>;

    /// Every dataset in a project.
    async fn list_datasets(&self, project: &str) -> Result<Vec<Dataset>>;

    /// The project a dataset belongs to, or `None` when no such dataset exists.
    ///
    /// Dataset items are addressed by dataset id alone, so a handler cannot tell
    /// from the request which tenant's data it is about to return. This is the
    /// lookup that makes that decidable. It returns the project and nothing
    /// else on purpose: the caller needs it to authorise, and a method that
    /// handed back the row would be a way to read a dataset's name and size
    /// without holding a grant for it.
    async fn dataset_project(&self, dataset_id: &str) -> Result<Option<String>>;

    /// Add cases to a dataset. Replaces an item with the same id.
    async fn add_dataset_items(&self, dataset_id: &str, items: &[DatasetItem]) -> Result<u32>;

    /// Read a dataset's cases.
    async fn list_dataset_items(&self, dataset_id: &str, limit: u32) -> Result<Vec<DatasetItem>>;

    /// Create or replace an experiment.
    async fn put_experiment(&self, experiment: &Experiment) -> Result<()>;

    /// Every experiment in a project, newest first.
    async fn list_experiments(&self, project: &str) -> Result<Vec<Experiment>>;

    // --------------------------------------------------------- aspect analysis

    /// Read aspect notes.
    ///
    /// The one read of the notes table, shared by the trace route, the examples
    /// route and the clustering pass. What separates them is which fields of
    /// the query are set.
    ///
    /// Every one of the six methods below carries a default that refuses. That
    /// is the same shape [`discover`](Self::discover) and [`poll`](Self::poll)
    /// have, and it is honest for the same reason: a backend that has not built
    /// the tables reports `theme_analysis: false`, the door refuses with a 501
    /// naming that capability, and a caller reads the capability set once
    /// instead of learning the difference between two backends one refusal at a
    /// time. It is not an invitation to leave a backend unimplemented.
    ///
    /// # Errors
    /// [`Error::Unsupported`] when [`StoreCapabilities::theme_analysis`] is
    /// false, or a storage error.
    async fn list_aspect_notes(&self, q: &AspectNoteQuery) -> Result<Paged<StoredNote>> {
        let _ = q;
        Err(no_theme_analysis(self.name()))
    }

    /// Count notes per entity and aspect.
    ///
    /// Returns the counts and nothing else. The sentences are payload and this
    /// is the answer to "how far has this got", so a method that handed back
    /// rows would be a way to read what users asked for without a grant that
    /// names it.
    ///
    /// # Errors
    /// [`Error::Unsupported`] when [`StoreCapabilities::theme_analysis`] is
    /// false, or a storage error.
    async fn aspect_note_rollup(&self, q: &AspectRollupQuery) -> Result<Vec<AspectNoteRollup>> {
        let _ = q;
        Err(no_theme_analysis(self.name()))
    }

    /// Write one clustering pass: its header, its themes and its memberships.
    ///
    /// Whole, in one transaction where the backend has them. Replacing a
    /// snapshot id that already exists is the correct behaviour: the pass is a
    /// pure function of the notes it read, so a rerun writes what is already
    /// there.
    ///
    /// # Errors
    /// [`Error::Unsupported`] when [`StoreCapabilities::theme_analysis`] is
    /// false, or a storage error.
    async fn write_theme_snapshot(&self, write: &SnapshotWrite) -> Result<()> {
        let _ = write;
        Err(no_theme_analysis(self.name()))
    }

    /// Read snapshot headers.
    ///
    /// [`Paged::total`] is how many the filter matched, which is the number a
    /// landmark selection was drawn from. A timeline showing two dozen points
    /// out of nine hundred has to be able to say so.
    ///
    /// # Errors
    /// [`Error::Unsupported`] when [`StoreCapabilities::theme_analysis`] is
    /// false, or a storage error.
    async fn list_theme_snapshots(&self, q: &SnapshotQuery) -> Result<Paged<ThemeSnapshot>> {
        let _ = q;
        Err(no_theme_analysis(self.name()))
    }

    /// Read themes, in one snapshot or across every snapshot that held one.
    ///
    /// # Errors
    /// [`Error::Unsupported`] when [`StoreCapabilities::theme_analysis`] is
    /// false, or a storage error.
    async fn list_themes(&self, q: &ThemeQuery) -> Result<Paged<StoredTheme>> {
        let _ = q;
        Err(no_theme_analysis(self.name()))
    }

    /// Read which notes joined which theme.
    ///
    /// Ordered most alike the theme's label first, with the note id breaking a
    /// tie, so the order is total and two replicas return the same page.
    ///
    /// # Errors
    /// [`Error::Unsupported`] when [`StoreCapabilities::theme_analysis`] is
    /// false, or a storage error.
    async fn list_theme_members(&self, q: &ThemeMemberQuery) -> Result<Paged<ThemeMember>> {
        let _ = q;
        Err(no_theme_analysis(self.name()))
    }
}

/// The refusal every unimplemented analysis method gives, written once.
fn no_theme_analysis(backend: &'static str) -> Error {
    Error::Unsupported {
        backend,
        capability: "theme_analysis",
        reason: "this backend does not hold aspect notes, snapshots or themes, so there is \
                 nothing for the intelligence endpoints to read",
    }
}

/// What an erasure removed, what it emptied, and what it could not reach.
///
/// [`ErasureResult`] is the wire shape and carries the per-signal counts. It
/// cannot carry the two things a caller most needs when the counts are small:
/// what was deliberately left behind, and whether a zero can be read as
/// "nothing about this person is stored". Both are here.
#[derive(Debug, Clone, PartialEq)]
pub struct ErasureOutcome {
    /// The per-signal counts, as returned to the caller.
    pub result: ErasureResult,
    /// Dataset cases captured from an erased run, emptied in place.
    pub dataset_items: u32,
    /// Aspect notes removed.
    ///
    /// Its own field because [`ErasureResult`] has one per signal and was
    /// generated before notes existed, so its `deleted` total cannot carry
    /// them. The reply names this number in prose instead of quietly folding it
    /// into a sum that would then disagree with the six counts printed beside
    /// it.
    pub aspect_notes: u32,
    /// The datasets holding those cases, so the reply can say where they are.
    pub datasets: Vec<String>,
    /// What the project still holds that names somebody, and what that can and
    /// cannot prove about this answer.
    pub identifiers: IdentifierProbe,
    /// True when the backend accepted the deletes and applies them afterwards,
    /// so the counts say what matched rather than what has already left disk.
    pub deferred: bool,
}

/// What is left in a project that names a person, after an erasure has run.
///
/// Every backend fills this in from its own SQL, so the rule is written here
/// once and each backend is read against it:
///
/// - `nothing_attributed` is true when no row in the project holds a `user_id`
///   or a `feedback.author_id` at all.
/// - `unmatchable` counts the distinct identifiers whose form differs from the
///   subject's, by [`is_hashed_identifier`].
/// - `opaque_remaining` is true when the subject was hashed and at least one
///   other identifier that is hashed, by that same [`is_hashed_identifier`],
///   is still stored.
///
/// The three answers are about the project, never about the subject. After a
/// successful erasure no row names the subject any more, so nothing here can be
/// read as "this person is still stored".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IdentifierProbe {
    /// True when nothing left in this project is attributed to anybody.
    ///
    /// This was a count of the distinct subjects still stored, and only its
    /// zero was ever used. The rest of the range was a population figure for
    /// the project, handed to a caller who asked about one person.
    ///
    /// What the zero proves is narrower than it reads. No row here names
    /// anybody, so there is no identifier left for a key change to be hiding.
    /// It does not prove nothing about this person was stored: a project that
    /// never sets `correlation.user_id` records runs all the same, and this
    /// endpoint does not search their payloads.
    pub nothing_attributed: bool,
    /// How many distinct stored identifiers are in a form this request could
    /// never have matched.
    ///
    /// Identifiers are hashed on the way in, and a hashed one is recognisable
    /// by the shape [`is_hashed_identifier`] tests for. A project holding
    /// plaintext identifiers when the request carried a digest, or the reverse,
    /// has rows written under a different redaction setting. Those rows may
    /// belong to the subject and this request cannot address them, so reporting
    /// a count without reporting this would tell the person something untrue.
    pub unmatchable: u32,
    /// True when a digest this request did not match is still stored here, and
    /// the subject was itself a digest.
    ///
    /// This is the case `unmatchable` cannot see, and it is the one a rotated
    /// redaction key leaves behind. Rotating the key changes what every
    /// identifier hashes to, so the subject's older rows keep a digest that no
    /// request can produce any more. Both forms are hashed, `unmatchable` stays
    /// at zero, and an erasure that reached only the newer rows reports exactly
    /// the same shape as one that reached everything.
    ///
    /// What a backend can honestly say is only this: a digest it did not match
    /// is still stored, and nothing records which key wrote it. So this is
    /// false in the two cases where there is nothing to say. The subject was
    /// plaintext, so no key stood between the request and the rows; or no other
    /// identifier is left, so there is nothing a key change could be hiding.
    ///
    /// It cannot tell a rotation from a second person, so it is true on any
    /// project holding more than one subject. It is a statement about what
    /// hashing costs, not a detection of a rotation. Detecting one needs the
    /// key's identity recorded beside the digest, which nothing writes today.
    pub opaque_remaining: bool,
}

/// The prefix [`mcpd_core::redact::Redactor::hash_identifier`] puts on a
/// digest. It is what makes a hashed identifier tellable from a plaintext one
/// in a column that holds both across the life of a deployment.
pub const HASHED_ID_PREFIX: &str = "sha256:";

/// How many hex characters follow that prefix. The redactor hex-encodes the
/// first 12 bytes of the digest, so the length is fixed and checkable.
const HASHED_ID_HEX: usize = 24;

/// True when `id` has the exact shape the redactor writes.
///
/// The prefix alone is not the test. An identifier a caller hashed themselves
/// before sending it arrives with the same prefix and a digest of its own
/// length, and counting it as a redaction setting that changed made the probe
/// report a mixed deployment to somebody who has one form and always has had.
/// The full shape, prefix and 24 lowercase hex characters, is what this
/// redactor and no ordinary identifier produces.
///
/// A caller who hashes their own identifiers to exactly this shape is still
/// indistinguishable from the redactor's own output. There is no evidence in
/// the column that separates them.
#[must_use]
pub fn is_hashed_identifier(id: &str) -> bool {
    let Some(hex) = id.strip_prefix(HASHED_ID_PREFIX) else { return false };
    hex.len() == HASHED_ID_HEX && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// The same shape as a SQLite `GLOB` pattern.
///
/// A backend narrows the rows it reads before [`unmatchable_identifiers`] sees
/// them, and the narrowing has to select exactly what the rule would, not
/// merely something close to it. A bare `sha256:*` prefix test is not close
/// enough: it swallows an identifier a caller hashed themselves, which carries
/// the prefix and a digest of its own length, and the rule counts that as the
/// other form. Reading the prefix as "hashed" therefore dropped rows this
/// request could never match out of the count that exists to report them.
///
/// `GLOB` is anchored at both ends and is case sensitive, so this matches the
/// prefix and 24 lowercase hex characters and nothing else, exactly as
/// [`is_hashed_identifier`] does.
#[must_use]
pub fn hashed_id_glob() -> String {
    format!("{HASHED_ID_PREFIX}{}", "[0-9a-f]".repeat(HASHED_ID_HEX))
}

/// The same shape as a POSIX regular expression, for Postgres `~` and
/// ClickHouse `match`.
///
/// Anchored at both ends for the same reason [`hashed_id_glob`] is. See that
/// function for why a prefix test is the wrong narrowing.
#[must_use]
pub fn hashed_id_regex() -> String {
    format!("^{HASHED_ID_PREFIX}[0-9a-f]{{{HASHED_ID_HEX}}}$")
}

/// The value a captured dataset case is emptied to.
///
/// A tombstone rather than a deleted row, on purpose. The case was put in the
/// dataset by someone who wanted a permanent regression test, and destroying it
/// without a word is its own harm. The subject's words are gone and the case is
/// visibly empty, which an operator can act on.
pub const ERASED_ITEM: &str = r#"{"erased":true}"#;

/// Count the identifiers in `stored` that this request could never match.
///
/// Shared by every backend so the rule cannot drift between them: an identifier
/// is unmatchable when it is hashed and the subject is not, or the reverse.
///
/// `stored` may be every identifier the project holds or only the candidates a
/// backend narrowed to in SQL. Narrowing is only safe when the SQL predicate
/// selects exactly what this rule would put on the other side, which is why the
/// shape is published as [`hashed_id_glob`] and [`hashed_id_regex`] rather than
/// left for each backend to approximate. A prefix test is not that predicate:
/// it calls a caller's own digest hashed, and an identifier dropped by the
/// narrowing can never be counted here. Whatever is passed must be distinct,
/// because the caller is told a number of identifiers rather than a number of
/// rows.
#[must_use]
pub fn unmatchable_identifiers(subject: &str, stored: &[String]) -> u32 {
    let hashed = is_hashed_identifier(subject);
    let n = stored.iter().filter(|s| is_hashed_identifier(s) != hashed).count();
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// Which attached signals to load alongside a trace.
#[derive(Debug, Clone, Copy, Default)]
pub struct Include {
    /// Load the run's logs.
    pub logs: bool,
    /// Load its scores.
    pub scores: bool,
    /// Load its feedback.
    pub feedback: bool,
}

impl Include {
    /// Parse the comma-separated `include` query parameter.
    ///
    /// Unknown names are ignored rather than rejected, so a newer client asking
    /// for something this server does not have still gets its trace.
    #[must_use]
    pub fn parse(raw: Option<&str>) -> Self {
        let mut inc = Self::default();
        for part in raw.unwrap_or("").split(',') {
            match part.trim() {
                "logs" => inc.logs = true,
                "scores" => inc.scores = true,
                "feedback" => inc.feedback = true,
                "all" => {
                    inc.logs = true;
                    inc.scores = true;
                    inc.feedback = true;
                }
                _ => {}
            }
        }
        inc
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcpd_core::{Span, SpanKind, SpanStatus};

    #[test]
    fn a_prefix_bound_is_the_next_string_after_every_match() {
        // The point of the bound is that `value >= p AND value < bound` selects
        // exactly the values starting with `p`. These are the cases where a
        // naive "add one to the last byte" gets it wrong.
        assert_eq!(prefix_upper_bound("g").as_deref(), Some("h"));
        assert_eq!(prefix_upper_bound("gpt-").as_deref(), Some("gpt."));
        // The surrogate block holds no chars, so the successor of U+D7FF is
        // U+E000. Adding one to the code point and casting would have produced
        // an invalid char here.
        assert_eq!(prefix_upper_bound("a\u{d7ff}").as_deref(), Some("a\u{e000}"));
        // A last character with no successor carries into the one before it,
        // exactly as an odometer does.
        let carried = prefix_upper_bound(&format!("a{}", char::MAX)).expect("a bound");
        assert_eq!(carried, "b");
        // And a prefix made only of the highest character has no bound at all.
        // The caller drops the upper bound and keeps the lower, which is still
        // correct and still a range scan.
        assert!(prefix_upper_bound(&char::MAX.to_string()).is_none());
    }

    #[test]
    fn a_poll_cursor_survives_a_round_trip_and_rejects_anything_else() {
        let c = encode_delta_cursor(1_700_000_000_123_456_789, "abc|def");
        // The id can hold the separator, so the split has to be on the first
        // one. Splitting on the last would read the watermark as `...|abc`.
        assert_eq!(
            decode_delta_cursor(&c).expect("round trip"),
            (1_700_000_000_123_456_789, "abc|def".to_owned())
        );
        for bad in ["", "zz", "not-a-cursor", &hex::encode("no separator")] {
            assert!(
                matches!(decode_delta_cursor(bad), Err(Error::InvalidQuery(_))),
                "{bad:?} is not a cursor this store issued, so it must be refused rather than \
                 read as a watermark that silently skips every row before it"
            );
        }
    }

    #[test]
    fn a_retention_cutoff_clamps_rather_than_wrapping() {
        let now = OffsetDateTime::from_unix_timestamp(1_700_000_000).expect("a time");
        assert_eq!(RetentionPolicy::cutoff(None, now), None, "no window means keep it");
        assert_eq!(
            RetentionPolicy::cutoff(Some(1), now).expect("a cutoff"),
            now - time::Duration::days(1)
        );
        // A window longer than the age of the epoch must not wrap into the
        // future. That is the one arithmetic mistake in a retention pass that
        // nobody can undo.
        let far = RetentionPolicy::cutoff(Some(u32::MAX), now).expect("a cutoff");
        assert!(far <= now, "a cutoff in the future would delete everything");
    }

    #[test]
    fn the_widest_window_is_the_one_with_no_window_at_all() {
        // Every backend prunes the distinct-value list on the widest window, so
        // "one signal is kept for ever" has to read as wider than any number of
        // days rather than as zero.
        let p = RetentionPolicy { logs: Some(7), scores: None, ..RetentionPolicy::default() };
        let widest =
            p.windows().iter().map(|(_, d)| *d).max_by_key(|d| d.unwrap_or(u32::MAX)).flatten();
        assert_eq!(widest, None);
        let bounded =
            RetentionPolicy { logs: Some(7), metrics: Some(90), ..RetentionPolicy::uniform(1) };
        let widest = bounded
            .windows()
            .iter()
            .map(|(_, d)| *d)
            .max_by_key(|d| d.unwrap_or(u32::MAX))
            .flatten();
        assert_eq!(widest, Some(90));
    }

    #[test]
    fn a_facet_value_is_never_a_payload() {
        // The cap is a guard, not a nicety: `entity_name` and a label value are
        // caller-supplied text, and without it a service that puts a whole
        // prompt in one would have that prompt copied into a table retention
        // does not reach on the same schedule as the span.
        let mut span = Span {
            trace_id: "t".into(),
            span_id: "s".into(),
            parent_span_id: None,
            external_parent_span_id: None,
            name: "n".into(),
            kind: SpanKind::AgentRun,
            status: SpanStatus::Ok,
            start_time: OffsetDateTime::UNIX_EPOCH,
            end_time: None,
            duration_ns: None,
            is_root: true,
            is_event: false,
            entity_kind: None,
            entity_id: None,
            entity_name: Some("x".repeat(MAX_FACET_VALUE + 1)),
            input: None,
            output: None,
            attributes: None,
            metadata: None,
            usage: None,
            cost: None,
            model_parameters: None,
            error: None,
            links: None,
            tags: Some(vec![String::new(), "real".into()]),
            correlation: mcpd_core::Correlation {
                project: "p".into(),
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
            ingest_key: None,
            received_at: OffsetDateTime::UNIX_EPOCH,
        };
        let found = facet_values(&Batch { spans: vec![span.clone()], ..Batch::default() });
        assert_eq!(
            found,
            vec![FacetValue {
                project: "p".into(),
                facet: Facet::Tag,
                key: String::new(),
                value: "real".into()
            }],
            "the oversized name and the empty tag are both dropped; an absent value is not a \
             value called \"\""
        );

        // A span with no project belongs to nobody, so nothing it carries can
        // be offered to anybody.
        span.correlation.project = String::new();
        span.entity_name = Some("fine".into());
        assert!(facet_values(&Batch { spans: vec![span], ..Batch::default() }).is_empty());
    }

    #[test]
    fn every_capability_field_is_reported() {
        // `fields` is what the conformance suite walks, so a field added to the
        // struct without a line there is a field nothing measures.
        let caps = StoreCapabilities::minimal("test");
        let named: Vec<&str> = caps.fields().iter().map(|(n, _)| *n).collect();
        assert_eq!(named.len(), 12, "a capability was added without a line in `fields`");
        let mut sorted = named.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), named.len(), "two capabilities are reported under one name");
    }

    #[test]
    fn include_parses_and_ignores_unknown_names() {
        let i = Include::parse(Some("logs, scores, wat"));
        assert!(i.logs && i.scores && !i.feedback);
        let all = Include::parse(Some("all"));
        assert!(all.logs && all.scores && all.feedback);
        let none = Include::parse(None);
        assert!(!none.logs && !none.scores && !none.feedback);
    }

    #[test]
    fn batch_counts_every_signal() {
        let b = Batch { spans: vec![], logs: vec![], ..Batch::default() };
        assert!(b.is_empty());
        assert_eq!(b.len(), 0);
    }

    #[test]
    fn an_identifier_is_only_hashed_when_it_has_the_whole_shape() {
        assert!(is_hashed_identifier("sha256:0123456789abcdef01234567"));
        assert!(!is_hashed_identifier("user-ann"));
        // A caller who hashed the id themselves sends a full digest, which is
        // 64 characters. Counting that as a redaction setting that changed is
        // how the probe used to cry wolf at a deployment with one form.
        assert!(!is_hashed_identifier(&format!("sha256:{}", "a".repeat(64))));
        assert!(!is_hashed_identifier("sha256:"), "a bare prefix is not a digest");
        assert!(
            !is_hashed_identifier("sha256:0123456789ABCDEF01234567"),
            "the redactor writes lowercase"
        );
    }

    #[test]
    fn the_sql_shape_tests_select_exactly_what_the_rule_does() {
        // The narrowing each backend runs must agree with the rule row for
        // row. A prefix test does not: it calls the first two hashed and the
        // rule calls none of them, so an identifier this request could never
        // match was dropped before it could be counted.
        // Both patterns are built from the same two constants the rule reads,
        // so a change to the digest length moves all three together.
        assert_eq!(hashed_id_glob(), format!("sha256:{}", "[0-9a-f]".repeat(HASHED_ID_HEX)));
        assert_eq!(hashed_id_regex(), format!("^sha256:[0-9a-f]{{{HASHED_ID_HEX}}}$"));
        // Anchored at both ends, and no wildcard that would let a longer
        // string through. `GLOB` matches the whole string, so a `*` anywhere
        // in the pattern is what turned this back into a prefix test.
        assert!(!hashed_id_glob().contains('*'), "{}", hashed_id_glob());
        assert!(hashed_id_regex().starts_with('^') && hashed_id_regex().ends_with('$'));
        // One hex position per character the redactor writes, so the pattern
        // cannot match a caller's own 64-character digest.
        assert_eq!(hashed_id_glob().matches("[0-9a-f]").count(), HASHED_ID_HEX);
    }

    #[test]
    fn only_the_other_form_is_counted_as_unmatchable() {
        let hashed = format!("sha256:{}", "0".repeat(24));
        let other = format!("sha256:{}", "1".repeat(24));
        let plain = "user-ann".to_owned();
        // A hashed request cannot address a plaintext row.
        assert_eq!(unmatchable_identifiers(&hashed, &[plain.clone(), other.clone()]), 1);
        // A plaintext request cannot address a hashed row.
        assert_eq!(unmatchable_identifiers("user-bob", &[plain.clone(), other.clone()]), 1);
        // Two digests are the same form, whatever key wrote them. This is the
        // blind spot the count cannot cover and `opaque_remaining` reports.
        assert_eq!(unmatchable_identifiers(&hashed, &[other]), 0);
        // Filtering the rows first must not change the answer.
        assert_eq!(unmatchable_identifiers(&hashed, &[plain]), 1);
        // A caller's own digest carries the prefix and is still the other
        // form, so a narrowing that reads the prefix as "hashed" hides it.
        let callers_own = format!("sha256:{}", "ab".repeat(32));
        assert_eq!(unmatchable_identifiers(&hashed, &[callers_own]), 1);
    }

    #[test]
    fn defaults_favour_newest_first() {
        assert_eq!(Order::default(), Order::Desc);
        assert_eq!(TraceSort::default(), TraceSort::StartTime);
        assert_eq!(SpanSort::default(), SpanSort::StartTime);
    }
}
