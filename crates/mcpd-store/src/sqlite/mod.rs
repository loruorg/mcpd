//! The embedded SQLite backend.
//!
//! This is the backend that makes `docker run mcpd` produce a working
//! observability stack with no other moving parts. SQLite is compiled in, runs
//! in WAL mode so readers never block the writer, and carries a real FTS5 index
//! over prompts and completions.
//!
//! Concurrency model: one writer connection behind a mutex, and a small pool of
//! reader connections. That is exactly what WAL is designed for, and it keeps
//! the ingest path from ever being blocked by a slow dashboard query.

use crate::{
    decode_delta_cursor, encode_delta_cursor, hashed_id_glob, is_hashed_identifier,
    unmatchable_identifiers, Aggregation, AspectNoteQuery, AspectNoteRollup, AspectRollupQuery,
    AuditQuery, Batch, Delta, DeltaQuery, DeltaSignal, Discovered, DiscoveryQuery, ErasureOutcome,
    Error, IdentifierProbe, Include, LogQuery, MetricQuery, Order, Paged, Partitioning,
    PolledSignal, Result, RetentionOutcome, RetentionPolicy, ScoreQuery, SnapshotQuery,
    SnapshotWrite, SpanQuery, SpanSort, Store, StoreCapabilities, StoredNote, StoredTheme,
    TextSearch, ThemeMember, ThemeMemberQuery, ThemeQuery, TraceQuery, TraceSort, ERASED_ITEM,
};
use mcpd_core::{
    AnalyticsSummary, AuditEvent, Correlation, Cost, CostBreakdown, CostBucket, Dataset,
    DatasetItem, ErasureResult, Experiment, Feedback, FeedbackKind, GroupedStat, IngestResult,
    LatencyStats, LogLevel, LogRecord, MetricSeries, MetricSeriesPoint, MetricSeriesResponse,
    Score, ScoreSource, Span, SpanError, SpanKind, SpanLink, SpanStatus, ThemeSnapshot, TokenUsage,
    Trace, TraceDetail,
};
use rusqlite::{params_from_iter, Connection, OpenFlags, OptionalExtension, ToSql};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, Mutex, PoisonError};
use time::OffsetDateTime;

mod aspects;
mod convert;
mod queries;
use aspects::*;
use convert::*;
use queries::*;

/// The current schema version. Bumping this and adding a migration is the only
/// supported way to change the tables.
///
/// 2 adds `audit_log`. It is a new table declared `IF NOT EXISTS`, and this file
/// is executed in full when the recorded version is behind, so an existing
/// database gains the table on its next start with nothing to apply by hand.
/// That is only true of an additive table: changing a column in place still
/// needs a migration step.
///
/// 3 adds `discovery` and the correlation indexes, both additive in the same
/// sense. Bumping it is what makes an existing database run the file again: the
/// fast path in `migrate` skips the whole batch when the recorded version
/// matches, so a table added to `schema.sql` without bumping this would reach
/// new databases and no existing one.
///
/// 4 adds `aspect_notes`, `theme_snapshots`, `themes` and `theme_members`, four
/// new tables and additive in the same sense again. An existing database gains
/// them on its next start with nothing to apply by hand, and gains no rows: a
/// deployment that writes no notes carries four empty tables and nothing runs
/// against them.
///
/// 5 keys `traces` by project and trace id together. It is the first step that
/// is not additive, and the first that `CREATE TABLE IF NOT EXISTS` cannot
/// apply, so `migrate` drops the old table before the file runs and rebuilds
/// every row from the spans after it. That is safe only because every column
/// of `traces` is derived; a step that changes a table holding data a sender
/// wrote needs a copy, not a rebuild.
const SCHEMA_VERSION: u32 = 5;

/// The first version whose `traces` table is keyed by project as well.
const TRACES_BY_PROJECT: u32 = 5;

const SCHEMA: &str = include_str!("schema.sql");

/// An embedded SQLite store.
pub struct SqliteStore {
    write: Arc<Mutex<Connection>>,
    readers: Arc<Vec<Mutex<Connection>>>,
    path: PathBuf,
    /// When the write in progress took the writer, in nanoseconds since the
    /// epoch, or zero when no write is running. Readiness reads it.
    busy_since: Arc<std::sync::atomic::AtomicI64>,
    /// How long one write may hold the writer before readiness reports it.
    stall_after: std::time::Duration,
    /// One permit per write that may be running or waiting for the writer.
    write_permits: Arc<tokio::sync::Semaphore>,
    /// The same for audit rows, which do not compete with ingest for a place.
    audit_permits: Arc<tokio::sync::Semaphore>,
}

/// How many writes may run or wait for the writer at once.
///
/// Every write is one transaction on one connection, so the rest wait in the
/// blocking pool, each holding its batch. A request that times out does not
/// take its write back out of that queue. Sixty-four is far above what a
/// healthy writer ever has waiting, and it bounds what a stalled one can
/// collect.
const MAX_QUEUED_WRITES: usize = 64;

/// How many audit rows may run or wait for the writer at once.
///
/// As many as for every other write together, so the two queues hold at most
/// 128 threads of the blocking pool, well inside the 512 tokio allows, and
/// reads still find one free behind a stalled writer.
const MAX_QUEUED_AUDIT_WRITES: usize = 64;

/// How long one write may hold the writer before `/readyz` says so.
///
/// Thirty seconds is far past any ingest batch and inside what a large
/// retention pass takes on a slow disk, so it reports a writer that is stuck
/// rather than one that is busy.
const WRITER_STALL: std::time::Duration = std::time::Duration::from_secs(30);

/// Clears the writer's busy mark when a write ends, by any route out of it.
struct BusyMark(Arc<std::sync::atomic::AtomicI64>);

impl Drop for BusyMark {
    fn drop(&mut self) {
        self.0.store(0, std::sync::atomic::Ordering::Relaxed);
    }
}

impl SqliteStore {
    /// Open, or create, a store at `path`.
    ///
    /// Pass `:memory:` for an ephemeral store; the CLI uses that for `mcpd
    /// demo` so a first run leaves nothing behind.
    ///
    /// # Errors
    /// Fails when the file cannot be opened or the pragmas cannot be set.
    pub fn open(path: impl AsRef<Path>, reader_count: usize) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let in_memory = path.to_string_lossy() == ":memory:";

        // A shared cache is the only way several connections can see the same
        // in-memory database; on disk it would only add contention.
        let (write, readers) = if in_memory {
            let uri = "file:mcpd-mem?mode=memory&cache=shared";
            let w = Connection::open_with_flags(
                uri,
                OpenFlags::SQLITE_OPEN_READ_WRITE
                    | OpenFlags::SQLITE_OPEN_CREATE
                    | OpenFlags::SQLITE_OPEN_URI,
            )
            .map_err(be)?;
            let mut rs = Vec::new();
            for _ in 0..reader_count.max(1) {
                rs.push(Mutex::new(
                    Connection::open_with_flags(
                        uri,
                        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_URI,
                    )
                    .map_err(be)?,
                ));
            }
            (w, rs)
        } else {
            if let Some(dir) = path.parent() {
                if !dir.as_os_str().is_empty() {
                    std::fs::create_dir_all(dir)
                        .map_err(|e| Error::Backend(format!("creating {}: {e}", dir.display())))?;
                }
            }
            let w = Connection::open(&path).map_err(be)?;
            let mut rs = Vec::new();
            for _ in 0..reader_count.max(1) {
                rs.push(Mutex::new(
                    Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
                        .map_err(be)?,
                ));
            }
            (w, rs)
        };

        // `journal_mode` answers with the mode it ended up in rather than
        // failing, so the answer is read. A file system without the shared
        // memory WAL needs, a network mount for instance, keeps the rollback
        // journal, and then a reader blocks the writer and the other way
        // round. That still works, more slowly, and it is said once at start
        // rather than left to be discovered as stalls under load.
        let mode: String =
            write.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0)).map_err(be)?;
        if !in_memory && !mode.eq_ignore_ascii_case("wal") {
            tracing::warn!(
                "{} could not use write-ahead logging and is in {mode} mode, so reads and \
                 writes will wait for each other; keep the data file on a local disk",
                path.display()
            );
        }
        write
            .execute_batch(
                "PRAGMA synchronous = NORMAL; PRAGMA busy_timeout = 5000; \
                 PRAGMA foreign_keys = ON;",
            )
            .map_err(be)?;
        // Readers wait for a lock too. Without a timeout of their own, a read
        // that met a checkpoint or a rollback journal failed at once with
        // "database is locked" rather than waiting a moment for it.
        for r in &readers {
            r.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .execute_batch("PRAGMA busy_timeout = 5000;")
                .map_err(be)?;
        }

        Ok(Self {
            write: Arc::new(Mutex::new(write)),
            readers: Arc::new(readers),
            path,
            busy_since: Arc::new(std::sync::atomic::AtomicI64::new(0)),
            stall_after: WRITER_STALL,
            write_permits: Arc::new(tokio::sync::Semaphore::new(MAX_QUEUED_WRITES)),
            audit_permits: Arc::new(tokio::sync::Semaphore::new(MAX_QUEUED_AUDIT_WRITES)),
        })
    }

    /// Where this store lives, for the health endpoint.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Run `f` on the writer connection in a blocking thread.
    async fn with_write<T, F>(&self, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T> + Send + 'static,
    {
        // Taken before the blocking task starts and held until it ends, so the
        // count is of writes that will really run, including one whose caller
        // has already given up on it.
        let permit = Arc::clone(&self.write_permits).try_acquire_owned().map_err(|_| {
            Error::Busy(format!(
                "{MAX_QUEUED_WRITES} writes are already waiting for the database; send this \
                 one again shortly"
            ))
        })?;
        self.run_write(permit, f).await
    }

    /// Run `f` on the writer connection with a place in the audit queue, not
    /// the one [`Self::with_write`] bounds.
    ///
    /// For the audit row alone. The server writes one for every request it
    /// serves, after it has decided to serve it, so when ingest filled the
    /// shared queue the row was refused while the read it records went ahead:
    /// the request was answered and the log said nothing about it.
    ///
    /// A queue of its own rather than none. Nothing above the store limits how
    /// many requests run at once, and every waiting write holds a thread of the
    /// blocking pool that reads run on too, so behind a stalled writer an
    /// unbounded queue would take every thread and stop the reads as well.
    async fn with_audit_write<T, F>(&self, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T> + Send + 'static,
    {
        let permit = Arc::clone(&self.audit_permits).try_acquire_owned().map_err(|_| {
            Error::Busy(format!(
                "{MAX_QUEUED_AUDIT_WRITES} audit rows are already waiting for the database"
            ))
        })?;
        self.run_write(permit, f).await
    }

    async fn run_write<T, F>(&self, permit: tokio::sync::OwnedSemaphorePermit, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T> + Send + 'static,
    {
        let conn = Arc::clone(&self.write);
        let busy = Arc::clone(&self.busy_since);
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            // Recover from a poisoned lock rather than refusing for ever.
            //
            // A panic anywhere inside a write used to poison this mutex, and
            // every later write in the process then failed with "writer
            // poisoned": one malformed span stopped all ingest until somebody
            // restarted the daemon. A SQLite connection is not left in a torn
            // state by a panic in our own row-building code, because the
            // transaction it was in rolls back when the guard drops, so the
            // connection is reusable and refusing to reuse it is strictly worse
            // than the fault it is guarding against.
            let mut guard = conn.lock().unwrap_or_else(PoisonError::into_inner);
            busy.store(now_nanos().max(1), std::sync::atomic::Ordering::Relaxed);
            let _mark = BusyMark(busy);
            f(&mut guard)
        })
        .await
        .map_err(|e| Error::Backend(format!("write task failed: {e}")))?
    }

    /// Run `f` on a reader connection in a blocking thread.
    async fn with_read<T, F>(&self, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
    {
        let readers = Arc::clone(&self.readers);
        tokio::task::spawn_blocking(move || {
            // Take the first free reader; fall back to waiting on the first.
            for r in readers.iter() {
                if let Ok(guard) = r.try_lock() {
                    return f(&guard);
                }
            }
            let guard = readers[0].lock().unwrap_or_else(PoisonError::into_inner);
            f(&guard)
        })
        .await
        .map_err(|e| Error::Backend(format!("read task failed: {e}")))?
    }
}

fn be(e: rusqlite::Error) -> Error {
    Error::Backend(e.to_string())
}

/// A WHERE-clause builder that keeps predicates and their bindings together,
/// so a filter can never be added without its parameter.
#[derive(Default)]
struct Where {
    clauses: Vec<String>,
    binds: Vec<Box<dyn ToSql + Send>>,
}

impl Where {
    fn push(&mut self, clause: &str, value: impl ToSql + Send + 'static) {
        self.clauses.push(clause.to_owned());
        self.binds.push(Box::new(value));
    }
    fn sql(&self) -> String {
        if self.clauses.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", self.clauses.join(" AND "))
        }
    }
    fn refs(&self) -> Vec<&dyn ToSql> {
        self.binds.iter().map(|b| b.as_ref() as &dyn ToSql).collect()
    }
}

/// Encode a pagination cursor. Opaque by construction: callers must never
/// build or parse one, so the encoding can change without breaking them.
///
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

/// Turn a user's search words into an FTS5 query.
///
/// User input goes nowhere near FTS5 syntax: a stray quote or `NEAR` would
/// otherwise be a syntax error rather than a search. Each word is quoted and
/// the terms are ANDed, which is what a person typing two words expects.
fn fts_query(raw: &str) -> Option<String> {
    let terms: Vec<String> = raw
        .split_whitespace()
        .map(|w| w.replace('"', ""))
        .filter(|w| !w.is_empty())
        .map(|w| format!("\"{w}\""))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" AND "))
}

/// Delete every row this erasure reaches, and empty the cases it captured.
///
/// A named function rather than a closure, like every other statement-heavy
/// piece of SQL in this file, so the caller reads as the three steps it is:
/// delete, probe, report.
fn erase_rows_sync(
    conn: &mut Connection,
    p: &str,
    u: &str,
) -> Result<(ErasureResult, u32, u32, Vec<String>)> {
    let tx = conn.transaction().map_err(be)?;

    // The runs in scope, resolved once and held in a temporary table.
    //
    // Resolving them up front is what makes the rest of this correct.
    // The set is derived from `traces.user_id` and `spans.user_id`, so
    // a `DELETE ... WHERE trace_id IN (SELECT ...)` against those same
    // two tables would shrink its own search set as it ran, and every
    // statement after the first would see a different set of runs.
    //
    // The tables are dropped again below rather than left on the
    // connection: the writer connection is long-lived and reused for
    // every later write, so a leftover temporary table would still be
    // there, populated, on the next erasure.
    tx.execute_batch(
        "DROP TABLE IF EXISTS temp.erasure_targets;
         DROP TABLE IF EXISTS temp.erasure_spans;
         CREATE TEMP TABLE erasure_targets (trace_id TEXT PRIMARY KEY);
         CREATE TEMP TABLE erasure_spans (span_id TEXT PRIMARY KEY);",
    )
    .map_err(be)?;
    for sql in [
        "INSERT OR IGNORE INTO erasure_targets (trace_id)
         SELECT trace_id FROM traces WHERE project = ?1 AND user_id = ?2",
        "INSERT OR IGNORE INTO erasure_targets (trace_id)
         SELECT trace_id FROM spans WHERE project = ?1 AND user_id = ?2",
    ] {
        tx.execute(sql, rusqlite::params![&p, &u]).map_err(be)?;
    }

    // The subject's span ids, taken while the spans still exist.
    //
    // A score, a piece of feedback, a metric point or a log line can
    // name a `span_id` and no `trace_id`. Those rows hang off the
    // subject's step and hold the subject's content, and matching on
    // the trace alone left every one of them on disk.
    tx.execute(
        "INSERT OR IGNORE INTO erasure_spans (span_id)
         SELECT span_id FROM spans WHERE project = ?1
         AND trace_id IN (SELECT trace_id FROM erasure_targets)",
        rusqlite::params![&p],
    )
    .map_err(be)?;

    // What the attached signals are matched on. Written once so the
    // count and the delete cannot drift apart, and so a signal cannot
    // be given a narrower reach than its neighbour by accident.
    //
    // A span id is unique inside its trace, not inside the project, so
    // `span_id IN (...)` on its own reached any other run in the same
    // project that happened to reuse the id and took that run's scores
    // and logs with it. The span reach is therefore limited to rows
    // that name no trace of their own. A row that names a trace is
    // judged by that trace, and a row that names none is the one the
    // span reach exists for.
    //
    // A row that names only a span id is still ambiguous when two runs
    // share that id. It goes, because it may hold the subject's content
    // and the row carries nothing else to decide on. Over-deleting one
    // unattributed row is the safer side of an erasure; leaving the
    // subject's content on disk is not.
    const ATTACHED: &str = "(trace_id IN (SELECT trace_id FROM erasure_targets)
         OR (ifnull(trace_id,'') = ''
             AND span_id IN (SELECT span_id FROM erasure_spans)))";

    // The full-text rows go first, while the rows that name them still
    // exist. Both indexes are cleared, not only the span one: a log
    // message is copied into `logs_fts` at ingest, so leaving that row
    // behind would keep the erased text searchable by its content,
    // which is the one thing an erasure request has to stop.
    tx.execute(
        &format!(
            "DELETE FROM logs_fts WHERE project = ?1 AND log_id IN
             (SELECT log_id FROM logs WHERE project = ?1
              AND ({ATTACHED} OR user_id = ?2))"
        ),
        rusqlite::params![&p, &u],
    )
    .map_err(be)?;
    tx.execute(
        "DELETE FROM spans_fts WHERE project = ?1
         AND trace_id IN (SELECT trace_id FROM erasure_targets)",
        rusqlite::params![&p],
    )
    .map_err(be)?;

    // Every statement carries `project = ?1`. That is the whole
    // tenancy guard: the same subject in a project this caller was not
    // authorised for keeps its rows and stays out of the counts below.
    let count = |sql: &str, with_user: bool| -> Result<u32> {
        let n = if with_user {
            tx.execute(sql, rusqlite::params![&p, &u]).map_err(be)?
        } else {
            tx.execute(sql, rusqlite::params![&p]).map_err(be)?
        };
        Ok(u32::try_from(n).unwrap_or(u32::MAX))
    };

    let spans = count(
        "DELETE FROM spans WHERE project = ?1
         AND trace_id IN (SELECT trace_id FROM erasure_targets)",
        false,
    )?;
    // One statement per signal, so a row that is both attached to an
    // erased run and stamped with the subject is counted once rather
    // than twice.
    let logs = count(
        &format!("DELETE FROM logs WHERE project = ?1 AND ({ATTACHED} OR user_id = ?2)"),
        true,
    )?;
    // Metrics and scores carry no identifier of their own. They are
    // reachable through the run or the step they hang off, which is why
    // both reaches are needed and neither is enough alone.
    let metrics = count(&format!("DELETE FROM metrics WHERE project = ?1 AND {ATTACHED}"), false)?;
    let scores = count(&format!("DELETE FROM scores WHERE project = ?1 AND {ATTACHED}"), false)?;
    // Feedback has an author, hashed by the same rule as `user_id`, and
    // the row holds that person's own words. Feedback the subject wrote
    // about somebody else's run is still the subject's prose, so the
    // author is matched wherever the row hangs.
    let feedback = count(
        &format!(
            "DELETE FROM feedback WHERE project = ?1
             AND ({ATTACHED} OR author_id = ?2)"
        ),
        true,
    )?;
    // An aspect note is one sentence about what a user asked for, so it is the
    // most direct copy of a person's own words in this schema after the span
    // payloads themselves. It is matched by the run it describes and by the
    // subject stamped on it, exactly as a log line is, so a note about somebody
    // else's run that carries this subject goes too.
    //
    // Not reported in [`ErasureResult`], which has a field per signal and was
    // generated before notes existed. The count is carried out separately in
    // [`ErasureOutcome::aspect_notes`] and named in the reply's advisory, so a
    // person asking what was deleted is told rather than left to infer it from
    // a total that does not move.
    //
    // Matched on the trace and the subject, and not through `ATTACHED`: a note
    // names a trace and never a step, so the span half of that predicate would
    // be a reference to a column this table does not have.
    let aspect_notes = count(
        "DELETE FROM aspect_notes WHERE project = ?1
         AND (trace_id IN (SELECT trace_id FROM erasure_targets) OR user_id = ?2)",
        true,
    )?;

    // Membership rows hold no prose, but they hold the note id and the trace id
    // of a sentence that has just been erased. Left behind, a theme would still
    // claim a run nothing can open, so they go with the notes rather than being
    // left as a dangling reference.
    tx.execute(
        "DELETE FROM theme_members WHERE project = ?1
         AND trace_id IN (SELECT trace_id FROM erasure_targets)",
        rusqlite::params![&p],
    )
    .map_err(be)?;

    // A theme's label is not a summary written for the screen: it is the text
    // of one of the theme's own notes, so a theme whose label came from an
    // erased run holds that person's words in a column an erasure would
    // otherwise miss entirely. The label is emptied rather than the theme
    // deleted, for the same reason a captured dataset case is emptied: the
    // clustering is a record of what the daemon found, and destroying it
    // silently is its own harm. `label_trace_id` is cleared with the text,
    // which is also what makes a repeat erasure a no-op here.
    tx.execute(
        &format!(
            "UPDATE themes SET label = '{ERASED_ITEM}', label_note_id = '', label_trace_id = ''
              WHERE project = ?1
                AND label_trace_id IN (SELECT trace_id FROM erasure_targets)"
        ),
        rusqlite::params![&p],
    )
    .map_err(be)?;

    let traces = count(
        "DELETE FROM traces WHERE project = ?1
         AND trace_id IN (SELECT trace_id FROM erasure_targets)",
        false,
    )?;

    // A captured case keeps a verbatim copy of a production input, so
    // erasing the run and leaving the copy would leave the subject's
    // words in the dataset. The case is emptied rather than deleted:
    // somebody put it there to keep a bug from coming back, and
    // destroying it silently is its own harm. `source_trace_id` is
    // cleared with the payload, which is also what makes a repeat
    // erasure a no-op instead of reporting the same cases again.
    //
    // Only datasets in this project are reached. The tenancy guard is
    // absolute here as everywhere else, so a case captured into another
    // project's dataset is out of scope and the caller is told the
    // erasure was bounded to one project.
    let owned = "dataset_id IN (SELECT dataset_id FROM datasets WHERE project = ?1)";
    let captured = format!("source_trace_id IN (SELECT trace_id FROM erasure_targets) AND {owned}");
    let mut stmt = tx
        .prepare(&format!("SELECT DISTINCT dataset_id FROM dataset_items WHERE {captured}"))
        .map_err(be)?;
    let datasets: Vec<String> = stmt
        .query_map(rusqlite::params![&p], |r| r.get(0))
        .map_err(be)?
        .collect::<std::result::Result<Vec<String>, _>>()
        .map_err(be)?;
    drop(stmt);
    let dataset_items = count(
        &format!(
            "UPDATE dataset_items
                SET input = '{ERASED_ITEM}', expected = NULL,
                    metadata = '{ERASED_ITEM}', source_trace_id = NULL
              WHERE {captured}"
        ),
        false,
    )?;

    tx.execute_batch(
        "DROP TABLE IF EXISTS temp.erasure_targets;
         DROP TABLE IF EXISTS temp.erasure_spans;",
    )
    .map_err(be)?;
    tx.commit().map_err(be)?;

    Ok((
        ErasureResult {
            project: p.to_owned(),
            traces,
            spans,
            logs,
            metrics,
            scores,
            feedback,
            // The six fields this type has, and their sum. Aspect notes are
            // deliberately outside it: adding them would make `deleted`
            // disagree with the six numbers printed beside it, which is worse
            // than a total that is explained.
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
    ))
}

/// What the project still holds that names a person, read after an erasure.
///
/// Three questions, each with its own statement. Two are existence checks that
/// stop at the first row, and only the third has to read anything in bulk. The
/// single union that answered all three at once read every identifier in the
/// project whatever the answer turned out to be.
///
/// `GLOB` rather than `LIKE`, because SQLite's `LIKE` ignores ASCII case: a
/// stored `SHA256:...` would be filtered out of the candidates before the
/// shared rule in [`unmatchable_identifiers`] ever saw it.
///
/// The pattern is [`hashed_id_glob`], the whole shape, not the prefix. A prefix
/// test decides the question rather than narrowing it, and decides it wrongly:
/// an identifier a caller hashed themselves carries the prefix, the rule counts
/// it as the other form, and `NOT GLOB 'sha256:*'` dropped it before the rule
/// could. The narrowing now selects exactly the rows the rule puts on the other
/// side, so what an identifier means is still decided in one place.
fn probe_identifiers(conn: &Connection, project: &str, subject: &str) -> Result<IdentifierProbe> {
    // The four columns that name a person, listed once. A column missing from
    // this list is a person the probe cannot see.
    const COLUMNS: [(&str, &str); 4] = [
        ("traces", "user_id"),
        ("spans", "user_id"),
        ("logs", "user_id"),
        ("feedback", "author_id"),
    ];

    let ors = |per_column: &dyn Fn(&str, &str) -> String| -> String {
        COLUMNS.iter().map(|(t, c)| per_column(t, c)).collect::<Vec<_>>().join(" OR ")
    };
    let yes_no = |sql: &str, binds: &[&dyn ToSql]| -> Result<bool> {
        conn.query_row(&format!("SELECT {sql}"), binds, |r| r.get::<_, i64>(0))
            .map(|n| n != 0)
            .map_err(be)
    };

    let attributed = ors(&|t, c| {
        format!("EXISTS(SELECT 1 FROM {t} WHERE project = ?1 AND ifnull({c},'') <> '')")
    });
    let nothing_attributed = !yes_no(&attributed, &[&project])?;

    let hashed = is_hashed_identifier(subject);
    let shape = hashed_id_glob();

    // A digest other than the subject's own. Excluding the subject is what
    // stops this reporting the rows this erasure has just addressed, and it is
    // why the answer says nothing about whether the erasure worked.
    let opaque_remaining = hashed && {
        let sql = ors(&|t, c| {
            format!(
                "EXISTS(SELECT 1 FROM {t} WHERE project = ?1
                 AND {c} GLOB '{shape}' AND {c} <> ?2)"
            )
        });
        yes_no(&sql, &[&project, &subject])?
    };

    // Only identifiers of the other form are read. In a deployment that never
    // changed its redaction setting that is no rows at all, where this used to
    // read every identifier the project held.
    let form = if hashed { "NOT GLOB" } else { "GLOB" };
    let sql = COLUMNS
        .iter()
        .map(|(t, c)| {
            format!(
                "SELECT {c} AS id FROM {t} WHERE project = ?1
                 AND ifnull({c},'') <> '' AND {c} {form} '{shape}'"
            )
        })
        .collect::<Vec<_>>()
        .join(" UNION ");
    let mut stmt = conn.prepare(&sql).map_err(be)?;
    let candidates: Vec<String> = stmt
        .query_map(rusqlite::params![project], |r| r.get(0))
        .map_err(be)?
        .collect::<std::result::Result<Vec<String>, _>>()
        .map_err(be)?;
    drop(stmt);

    Ok(IdentifierProbe {
        nothing_attributed,
        unmatchable: unmatchable_identifiers(subject, &candidates),
        opaque_remaining,
    })
}

/// The schema version recorded in the database, or `None` when there is none.
///
/// Two statements rather than one. SQLite resolves table names when it prepares
/// a statement, not when it runs it, so a single query guarded by `WHERE EXISTS
/// (SELECT 1 FROM sqlite_master ...)` still fails to prepare on a database that
/// has no tables yet, which is every database on its first start.
fn read_schema_version(conn: &Connection) -> Result<Option<u32>> {
    let present: bool = conn
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM sqlite_master              WHERE type = 'table' AND name = 'schema_version')",
            [],
            |r| r.get(0),
        )
        .map_err(be)?;
    if !present {
        return Ok(None);
    }
    conn.query_row("SELECT MAX(version) FROM schema_version", [], |r| r.get::<_, Option<u32>>(0))
        .optional()
        .map_err(be)
        .map(Option::flatten)
}

#[async_trait::async_trait]
impl Store for SqliteStore {
    fn name(&self) -> &'static str {
        "sqlite"
    }

    fn capabilities(&self) -> StoreCapabilities {
        StoreCapabilities {
            // FTS5 with the `unicode61` tokeniser: whole words, and case
            // folding across the whole of Unicode rather than ASCII alone.
            text_search: TextSearch::Words,
            unicode_case_folding: true,
            // One writer, one transaction, and the rows are gone when it
            // commits.
            immediate_deletes: true,
            discovery: true,
            discovery_label_values: true,
            delta_polling: true,
            per_signal_retention: true,
            // No engine-level expiry here. Retention is a delete the daemon runs on
            // a timer, and there is nothing to hand the window to.
            declarative_retention: false,
            // SQLite has no partitions. Retention is a row delete, which is
            // correct and reads every row it removes. That is the right trade
            // at the scale a single file serves, and saying so here is what
            // stops a reader assuming an expiry is free.
            partitioning: Partitioning::None,
            partition_drop_retention: false,
            idempotent_schema_init: true,
            theme_analysis: true,
            ..StoreCapabilities::minimal("sqlite")
        }
    }

    async fn migrate(&self) -> Result<()> {
        self.with_write(|conn| {
            // The pragmas at the top of the schema file are not re-run on the
            // fast path, and do not need to be: `open` sets the same four on
            // this connection before anything calls this, and they are
            // connection state rather than anything stored.
            //
            // Fast path. Every statement below is `IF NOT EXISTS`, so replaying
            // them is correct, and it is still 60 statements of parsing and
            // catalogue lookup on a path that has nothing to do. It also writes:
            // `CREATE INDEX IF NOT EXISTS` takes the write lock even when it
            // creates nothing, so a start on a busy database queues behind
            // ingest for no reason. `read_schema_version` answers on a database
            // that has no tables at all, so there is no error path to take here.
            let before = read_schema_version(conn)?;
            if before == Some(SCHEMA_VERSION) {
                return Ok(());
            }

            // A database from before version 5 holds one projection row per
            // trace id, merged across projects. It is dropped here and the file
            // below creates it again with the new key. Each step commits on its
            // own, and that is safe: until the version is recorded at the end,
            // every start lands here again and repeats the rebuild from the
            // spans, which are never touched.
            let rebuild = before.is_some_and(|v| v < TRACES_BY_PROJECT);
            if rebuild {
                conn.execute("DROP TABLE IF EXISTS traces", []).map_err(|e| Error::Migration {
                    version: TRACES_BY_PROJECT,
                    reason: e.to_string(),
                })?;
            }

            conn.execute_batch(SCHEMA)
                .map_err(|e| Error::Migration { version: SCHEMA_VERSION, reason: e.to_string() })?;
            if rebuild {
                let rebuilt = rebuild_traces(conn).map_err(|e| Error::Migration {
                    version: TRACES_BY_PROJECT,
                    reason: e.to_string(),
                })?;
                tracing::info!("rebuilt {rebuilt} trace projections, one per project and trace id");
            }
            let current = read_schema_version(conn)?;
            if current.unwrap_or(0) < SCHEMA_VERSION {
                conn.execute(
                    "INSERT OR REPLACE INTO schema_version (version, applied_at) VALUES (?1, ?2)",
                    rusqlite::params![SCHEMA_VERSION, now_nanos()],
                )
                .map_err(be)?;
            }
            Ok(())
        })
        .await
    }

    async fn ping(&self) -> Result<()> {
        // A write that has held the writer this long is stuck, and every
        // ingest request is queued behind it. Readiness says so, so traffic
        // goes to a replica that can take it and the operator is told which
        // part is not answering.
        let since = self.busy_since.load(std::sync::atomic::Ordering::Relaxed);
        if since > 0 {
            let held = now_nanos().saturating_sub(since).max(0) as u64;
            let held = std::time::Duration::from_nanos(held);
            if held > self.stall_after {
                return Err(Error::Backend(format!(
                    "one write has held the database for {} s; ingest is waiting behind it",
                    held.as_secs()
                )));
            }
        }
        // A real page of the file, not `SELECT 1`, which SQLite answers
        // without reading anything and so says nothing about the file.
        self.with_read(|conn| {
            conn.query_row("SELECT version FROM schema_version LIMIT 1", [], |_| Ok(()))
                .map_err(be)?;
            Ok(())
        })
        .await
    }

    async fn ingest(&self, batch: Batch) -> Result<IngestResult> {
        self.with_write(move |conn| write_batch(conn, batch)).await
    }

    async fn list_traces(&self, q: &TraceQuery) -> Result<Paged<Trace>> {
        let q = q.clone();
        self.with_read(move |conn| list_traces_sync(conn, &q)).await
    }

    async fn get_trace(
        &self,
        project: &str,
        trace_id: &str,
        include: &Include,
    ) -> Result<TraceDetail> {
        let (p, t, inc) = (project.to_owned(), trace_id.to_owned(), *include);
        self.with_read(move |conn| get_trace_sync(conn, &p, &t, &inc)).await
    }

    async fn delete_trace(&self, project: &str, trace_id: &str) -> Result<u32> {
        let (p, t) = (project.to_owned(), trace_id.to_owned());
        self.with_write(move |conn| {
            let tx = conn.transaction().map_err(be)?;
            let mut n = 0u32;
            for sql in [
                "DELETE FROM spans_fts WHERE trace_id = ?1 AND project = ?2",
                "DELETE FROM spans WHERE trace_id = ?1 AND project = ?2",
                "DELETE FROM logs WHERE trace_id = ?1 AND project = ?2",
                "DELETE FROM metrics WHERE trace_id = ?1 AND project = ?2",
                "DELETE FROM scores WHERE trace_id = ?1 AND project = ?2",
                "DELETE FROM feedback WHERE trace_id = ?1 AND project = ?2",
                "DELETE FROM aspect_notes WHERE trace_id = ?1 AND project = ?2",
                "DELETE FROM traces WHERE trace_id = ?1 AND project = ?2",
            ] {
                // The index rows are not counted: they are copies of the span
                // rows the next statement deletes and counts.
                let count = tx.execute(sql, rusqlite::params![&t, &p]).map_err(be)?;
                if !sql.contains("spans_fts") {
                    n += count as u32;
                }
            }
            // Membership rows are not counted. They are ids pointing at a note
            // that has just gone, so leaving one behind would make a theme
            // claim a run that is no longer stored; but they are bookkeeping
            // and not a signal somebody sent, and counting them would inflate
            // the number this endpoint reports back as deleted.
            tx.execute(
                "DELETE FROM theme_members WHERE trace_id = ?1 AND project = ?2",
                rusqlite::params![&t, &p],
            )
            .map_err(be)?;
            tx.commit().map_err(be)?;
            Ok(n)
        })
        .await
    }

    async fn erase_subject(&self, project: &str, user_id: &str) -> Result<ErasureOutcome> {
        let (p, u) = (project.to_owned(), user_id.to_owned());
        let (result, dataset_items, aspect_notes, datasets) =
            self.with_write(move |conn| erase_rows_sync(conn, &p, &u)).await?;

        // The probe runs after the write has committed, on a reader.
        //
        // It used to run inside the write transaction, and it reads every row
        // in the project that names anybody. On a large project that held the
        // single writer connection for the whole scan, so every erasure stalled
        // ingest for as long as the scan took. WAL readers do not block the
        // writer, so moving it here costs ingest nothing.
        //
        // The trade is that the probe now describes the project a moment after
        // the erasure rather than at the instant it committed, so a write that
        // lands in between is included. That is the right way round: the probe
        // says what is still here, and a row written a moment ago is still
        // here. It says nothing about what this request deleted, which is what
        // the counts are for.
        let (p, u) = (project.to_owned(), user_id.to_owned());
        let probe = self.with_read(move |conn| probe_identifiers(conn, &p, &u)).await?;

        Ok(ErasureOutcome {
            result,
            dataset_items,
            aspect_notes,
            datasets,
            identifiers: probe,
            // SQLite deletes inside the transaction this call committed.
            deferred: false,
        })
    }

    async fn list_spans(&self, q: &SpanQuery) -> Result<Paged<Span>> {
        let q = q.clone();
        self.with_read(move |conn| list_spans_sync(conn, &q)).await
    }

    async fn list_logs(&self, q: &LogQuery) -> Result<Paged<LogRecord>> {
        let q = q.clone();
        self.with_read(move |conn| list_logs_sync(conn, &q)).await
    }

    async fn query_metrics(&self, q: &MetricQuery) -> Result<MetricSeriesResponse> {
        let q = q.clone();
        self.with_read(move |conn| query_metrics_sync(conn, &q)).await
    }

    async fn list_metric_points(
        &self,
        q: &crate::MetricPointQuery,
    ) -> Result<Paged<mcpd_core::MetricPoint>> {
        let q = q.clone();
        self.with_read(move |conn| list_metric_points_sync(conn, &q)).await
    }

    async fn list_scores(&self, q: &ScoreQuery) -> Result<Paged<Score>> {
        let q = q.clone();
        self.with_read(move |conn| list_scores_sync(conn, &q)).await
    }

    async fn analytics_summary(
        &self,
        project: &str,
        from: OffsetDateTime,
        to: OffsetDateTime,
        group_by: Option<&str>,
    ) -> Result<AnalyticsSummary> {
        let (p, g) = (project.to_owned(), group_by.map(str::to_owned));
        self.with_read(move |conn| summary_sync(conn, &p, from, to, g.as_deref())).await
    }

    async fn analytics_cost(
        &self,
        project: &str,
        from: OffsetDateTime,
        to: OffsetDateTime,
        group_by: &str,
        bucket_seconds: Option<u32>,
    ) -> Result<CostBreakdown> {
        let (p, g) = (project.to_owned(), group_by.to_owned());
        self.with_read(move |conn| cost_sync(conn, &p, from, to, &g, bucket_seconds)).await
    }

    async fn discover(&self, q: &DiscoveryQuery) -> Result<Vec<Discovered>> {
        let q = q.clone();
        self.with_read(move |conn| discover_sync(conn, &q)).await
    }

    async fn poll(&self, q: &DeltaQuery) -> Result<Delta<PolledSignal>> {
        let q = q.clone();
        self.with_read(move |conn| poll_sync(conn, &q)).await
    }

    async fn apply_retention(
        &self,
        project: &str,
        policy: &RetentionPolicy,
        now: OffsetDateTime,
    ) -> Result<RetentionOutcome> {
        let (p, policy) = (project.to_owned(), *policy);
        self.with_write(move |conn| apply_retention_sync(conn, &p, &policy, now)).await
    }

    async fn enforce_retention(&self, project: &str, older_than: OffsetDateTime) -> Result<u64> {
        let p = project.to_owned();
        let cutoff = to_nanos(older_than);
        self.with_write(move |conn| {
            let tx = conn.transaction().map_err(be)?;
            let mut n = 0u64;
            // Clear the FTS rows first, while the spans that name them still
            // exist; doing it after would orphan them.
            //
            // In this project only. A span id is unique within its trace, not
            // across projects, and matching the id alone removed another
            // project's search rows for a span that shared it, so that span
            // could no longer be found by its text.
            tx.execute(
                "DELETE FROM spans_fts WHERE project = ?1 AND span_id IN \
                 (SELECT span_id FROM spans WHERE project = ?1 AND received_at < ?2)",
                rusqlite::params![&p, cutoff],
            )
            .map_err(be)?;
            tx.execute(
                "DELETE FROM logs_fts WHERE log_id IN \
                 (SELECT log_id FROM logs WHERE project = ?1 AND received_at < ?2)",
                rusqlite::params![&p, cutoff],
            )
            .map_err(be)?;
            for table in ["spans", "logs", "metrics", "traces"] {
                n += tx
                    .execute(
                        &format!("DELETE FROM {table} WHERE project = ?1 AND received_at < ?2"),
                        rusqlite::params![&p, cutoff],
                    )
                    .map_err(be)? as u64;
            }
            for table in ["scores", "feedback"] {
                n += tx
                    .execute(
                        &format!("DELETE FROM {table} WHERE project = ?1 AND timestamp < ?2"),
                        rusqlite::params![&p, cutoff],
                    )
                    .map_err(be)? as u64;
            }
            // The analysis chain, in dependency order. A note is prose about a
            // run and must not outlive the window; a theme's label is the text
            // of one of its own notes, so the clustering has to go with them or
            // the sentence stays readable after the note has gone.
            for sql in [
                "DELETE FROM aspect_notes WHERE project = ?1 AND created_at < ?2",
                "DELETE FROM theme_members WHERE project = ?1 AND snapshot_id IN \
                 (SELECT snapshot_id FROM theme_snapshots WHERE project = ?1 AND cutoff_at < ?2)",
                "DELETE FROM themes WHERE project = ?1 AND cutoff_at < ?2",
                "DELETE FROM theme_snapshots WHERE project = ?1 AND cutoff_at < ?2",
            ] {
                n += tx.execute(sql, rusqlite::params![&p, cutoff]).map_err(be)? as u64;
            }
            n += tx
                .execute("DELETE FROM ingest_keys WHERE seen_at < ?1", rusqlite::params![cutoff])
                .map_err(be)? as u64;
            // The filter box goes with the rows. A value nothing has carried
            // since before the window has no row left to name it, so offering
            // it would filter the list to nothing.
            n += tx
                .execute(
                    "DELETE FROM discovery WHERE project = ?1 AND last_seen < ?2",
                    rusqlite::params![&p, cutoff],
                )
                .map_err(be)? as u64;
            tx.commit().map_err(be)?;
            Ok(n)
        })
        .await
    }

    async fn put_dataset(&self, dataset: &Dataset) -> Result<()> {
        let d = dataset.clone();
        self.with_write(move |conn| put_dataset_sync(conn, &d)).await
    }

    async fn list_datasets(&self, project: &str) -> Result<Vec<Dataset>> {
        let p = project.to_owned();
        self.with_read(move |conn| list_datasets_sync(conn, &p)).await
    }

    async fn dataset_project(&self, dataset_id: &str) -> Result<Option<String>> {
        let id = dataset_id.to_owned();
        self.with_read(move |conn| {
            conn.query_row("SELECT project FROM datasets WHERE dataset_id = ?1", [&id], |r| {
                r.get(0)
            })
            .optional()
            .map_err(be)
        })
        .await
    }

    async fn add_dataset_items(&self, dataset_id: &str, items: &[DatasetItem]) -> Result<u32> {
        let (id, items) = (dataset_id.to_owned(), items.to_vec());
        self.with_write(move |conn| add_items_sync(conn, &id, &items)).await
    }

    async fn list_dataset_items(&self, dataset_id: &str, limit: u32) -> Result<Vec<DatasetItem>> {
        let id = dataset_id.to_owned();
        self.with_read(move |conn| list_items_sync(conn, &id, limit)).await
    }

    async fn put_experiment(&self, experiment: &Experiment) -> Result<()> {
        let e = experiment.clone();
        self.with_write(move |conn| put_experiment_sync(conn, &e)).await
    }

    async fn list_experiments(&self, project: &str) -> Result<Vec<Experiment>> {
        let p = project.to_owned();
        self.with_read(move |conn| list_experiments_sync(conn, &p)).await
    }

    async fn record_audit(&self, event: &AuditEvent) -> Result<()> {
        let e = event.clone();
        self.with_audit_write(move |conn| record_audit_sync(conn, &e)).await
    }

    async fn list_audit(&self, q: &AuditQuery) -> Result<Vec<AuditEvent>> {
        let q = q.clone();
        self.with_read(move |conn| list_audit_sync(conn, &q)).await
    }

    async fn list_projects(&self) -> Result<Vec<String>> {
        self.with_read(|conn| {
            let mut stmt =
                conn.prepare("SELECT DISTINCT project FROM traces ORDER BY project").map_err(be)?;
            let rows = stmt
                .query_map([], |r| r.get::<_, String>(0))
                .map_err(be)?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(be)?;
            Ok(rows)
        })
        .await
    }

    async fn list_aspect_notes(&self, q: &AspectNoteQuery) -> Result<Paged<StoredNote>> {
        let q = q.clone();
        self.with_read(move |conn| list_notes_sync(conn, &q)).await
    }

    async fn aspect_note_rollup(&self, q: &AspectRollupQuery) -> Result<Vec<AspectNoteRollup>> {
        let q = q.clone();
        self.with_read(move |conn| rollup_sync(conn, &q)).await
    }

    async fn write_theme_snapshot(&self, write: &SnapshotWrite) -> Result<()> {
        let w = write.clone();
        self.with_write(move |conn| write_snapshot_sync(conn, &w)).await
    }

    async fn list_theme_snapshots(&self, q: &SnapshotQuery) -> Result<Paged<ThemeSnapshot>> {
        let q = q.clone();
        self.with_read(move |conn| list_snapshots_sync(conn, &q)).await
    }

    async fn list_themes(&self, q: &ThemeQuery) -> Result<Paged<StoredTheme>> {
        let q = q.clone();
        self.with_read(move |conn| list_themes_sync(conn, &q)).await
    }

    async fn list_theme_members(&self, q: &ThemeMemberQuery) -> Result<Paged<ThemeMember>> {
        let q = q.clone();
        self.with_read(move |conn| list_members_sync(conn, &q)).await
    }
}

#[cfg(test)]
mod readiness_tests {
    use super::*;

    /// A file of its own. `:memory:` here is one shared cache for the whole
    /// process, so two tests running at once would read each other's tables.
    fn scratch() -> SqliteStore {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "mcpd-ready-{}-{}-{}",
            std::process::id(),
            now_nanos(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        SqliteStore::open(dir.join("store.db"), 1).unwrap()
    }

    #[tokio::test]
    async fn a_write_that_holds_the_writer_too_long_fails_readiness() {
        let mut store = scratch();
        store.migrate().await.unwrap();
        store.stall_after = std::time::Duration::from_millis(50);
        store.ping().await.expect("an idle store is ready");

        let store = Arc::new(store);
        let held = {
            let store = Arc::clone(&store);
            tokio::spawn(async move {
                store
                    .with_write(|_| {
                        std::thread::sleep(std::time::Duration::from_millis(400));
                        Ok(())
                    })
                    .await
            })
        };
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let err = store.ping().await.expect_err("a stuck writer is not ready").to_string();
        assert!(err.contains("held the database"), "{err}");
        held.await.unwrap().unwrap();
        store.ping().await.expect("ready again once the write ends");
    }

    #[tokio::test]
    async fn a_write_past_the_queue_is_refused_rather_than_held() {
        let mut store = scratch();
        store.migrate().await.unwrap();
        store.write_permits = Arc::new(tokio::sync::Semaphore::new(1));
        let store = Arc::new(store);

        let held = {
            let store = Arc::clone(&store);
            tokio::spawn(async move {
                store
                    .with_write(|_| {
                        std::thread::sleep(std::time::Duration::from_millis(300));
                        Ok(())
                    })
                    .await
            })
        };
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        match store.with_write(|_| Ok(())).await {
            Err(Error::Busy(m)) => assert!(m.contains("waiting for the database"), "{m}"),
            other => panic!("a full queue must refuse, got {other:?}"),
        }
        held.await.unwrap().unwrap();
        store.with_write(|_| Ok(())).await.expect("the permit comes back when the write ends");
    }

    /// The server records a read after it has decided to serve it, so an audit
    /// row refused by a queue full of ingest is a served read the log never
    /// mentions.
    #[tokio::test]
    async fn an_audit_row_is_written_while_the_write_queue_is_full() {
        let mut store = scratch();
        store.migrate().await.unwrap();
        store.write_permits = Arc::new(tokio::sync::Semaphore::new(1));
        let store = Arc::new(store);

        let held = {
            let store = Arc::clone(&store);
            tokio::spawn(async move {
                store
                    .with_write(|_| {
                        std::thread::sleep(std::time::Duration::from_millis(300));
                        Ok(())
                    })
                    .await
            })
        };
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert!(
            matches!(store.with_write(|_| Ok(())).await, Err(Error::Busy(_))),
            "the queue must be full for this test to say anything"
        );

        let event = AuditEvent {
            audit_id: "audit-under-load".into(),
            timestamp: OffsetDateTime::now_utc(),
            principal: "anonymous".into(),
            subject: String::new(),
            action: "get_trace".into(),
            project: "audited".into(),
            resource: Some("trace-1".into()),
            result: mcpd_core::AuditResult::Allowed,
        };
        store.record_audit(&event).await.expect("a full ingest queue must not refuse an audit row");
        held.await.unwrap().unwrap();

        let rows = store
            .list_audit(&AuditQuery { project: "audited".into(), limit: 10, ..Default::default() })
            .await
            .unwrap();
        assert_eq!(rows.len(), 1, "the audit row was not written");
    }

    #[tokio::test]
    async fn a_store_that_was_never_migrated_is_not_ready() {
        let store = scratch();
        assert!(store.ping().await.is_err(), "SELECT 1 answered without reading the file");
    }
}

#[cfg(test)]
mod probe_tests {
    use super::*;

    /// The narrowing the probe does in SQL has to select exactly the rows
    /// [`unmatchable_identifiers`] would put on the other side. It used to test
    /// the `sha256:` prefix, which is a wider set: a digest a caller hashed
    /// themselves carries the prefix, the rule reads it as the other form, and
    /// `NOT GLOB 'sha256:*'` dropped it before the rule could count it. Run
    /// against a real SQLite so the pattern is checked by the engine that has
    /// to read it, not by a second copy of the rule in Rust.
    #[test]
    fn the_glob_selects_exactly_what_the_rule_calls_hashed() {
        let conn = Connection::open_in_memory().expect("open an in-memory database");
        let glob = hashed_id_glob();
        for id in [
            "sha256:0123456789abcdef01234567",
            &format!("sha256:{}", "ab".repeat(32)),
            "sha256:",
            "sha256:0123456789ABCDEF01234567",
            "sha256:0123456789abcdef012345678",
            "SHA256:0123456789abcdef01234567",
            "user-ann",
            "",
        ] {
            let matched: bool = conn
                .query_row("SELECT ?1 GLOB ?2", rusqlite::params![id, &glob], |r| r.get(0))
                .expect("run the pattern");
            assert_eq!(matched, is_hashed_identifier(id), "the engine and the rule differ on {id}");
        }
    }
}
