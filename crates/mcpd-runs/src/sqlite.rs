//! The SQLite backend for the run core.
//!
//! A SQLite deployment is one daemon by definition, so the runnable set is
//! taken with a conditional `UPDATE` under a single writer.
//!
//! There is one connection, behind one mutex. The reads on the trait are
//! short and the writes are whole transactions, and a run store is a small,
//! hot set of rows rather than a reporting database, so a reader pool would
//! buy contention on the mutex and nothing else.

use crate::db::{Arg, Conn, Db, DbError, DbResult, Dialect, Row, Tx, Val};
use crate::engine::{retries_spent, Backend, Engine, CAS_RETRIES};
use crate::store::RunConfig;
use crate::{Error, Result};
use async_trait::async_trait;
use rusqlite::types::{ToSqlOutput, ValueRef};
use rusqlite::Connection;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use time::OffsetDateTime;

const SCHEMA: &str = include_str!("schema.sql");

/// The schema this build writes.
pub const SCHEMA_VERSION: u32 = 2;

/// The durable run state, in SQLite.
pub struct SqliteRunStore {
    pub(crate) engine: Engine<Sqlite>,
}

impl SqliteRunStore {
    /// Open, or create, a store at `path`. Pass `:memory:` for an ephemeral
    /// one.
    ///
    /// # Errors
    /// Fails when the file cannot be opened or the pragmas cannot be set.
    pub fn open(path: impl AsRef<Path>, config: RunConfig) -> Result<Self> {
        let path = path.as_ref();
        let conn = if path.to_string_lossy() == ":memory:" {
            Connection::open_in_memory().map_err(be)?
        } else {
            Connection::open(path).map_err(be)?
        };
        Ok(Self { engine: Engine::new(Sqlite { conn: Arc::new(Mutex::new(conn)) }, config) })
    }
}

crate::engine::forward_run_store!(SqliteRunStore);

fn be(e: rusqlite::Error) -> Error {
    Error::Backend(e.to_string())
}

fn dbe(e: rusqlite::Error) -> DbError {
    DbError::Backend(be(e))
}

/// The one connection, and the lock that makes it the single writer.
pub(crate) struct Sqlite {
    conn: Arc<Mutex<Connection>>,
}

#[async_trait]
impl Backend for Sqlite {
    async fn run<T, F>(&self, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: Fn(&mut dyn Conn) -> Result<T> + Send + 'static,
    {
        let conn = Arc::clone(&self.conn);
        tokio::task::spawn_blocking(move || {
            // A panic in row-building code rolls its transaction back and
            // leaves the connection usable, so refusing to reuse it after one
            // bad row is strictly worse than the fault it guards against.
            let mut guard = conn.lock().unwrap_or_else(PoisonError::into_inner);
            let mut c = SqliteConn(&mut guard);
            let mut last = None;
            for _ in 0..CAS_RETRIES {
                match f(&mut c) {
                    Err(Error::Retry(reason)) => last = Some(reason),
                    other => return other,
                }
            }
            Err(retries_spent(last))
        })
        .await
        .map_err(|e| Error::Backend(format!("run store task failed: {e}")))?
    }
    async fn migrate(&self) -> Result<()> {
        let conn = Arc::clone(&self.conn);
        tokio::task::spawn_blocking(move || {
            let guard = conn.lock().unwrap_or_else(PoisonError::into_inner);
            guard.execute_batch(SCHEMA).map_err(be)?;
            guard
                .execute(
                    "INSERT OR IGNORE INTO run_schema_version (version, applied_at) VALUES (?1, ?2)",
                    rusqlite::params![SCHEMA_VERSION, now_ns()],
                )
                .map_err(be)?;
            Ok(())
        })
        .await
        .map_err(|e| Error::Backend(format!("run store task failed: {e}")))?
    }
}

fn now_ns() -> i64 {
    i64::try_from(OffsetDateTime::now_utc().unix_timestamp_nanos()).unwrap_or(i64::MAX)
}

impl rusqlite::ToSql for Arg<'_> {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(match *self {
            Arg::Null => ToSqlOutput::Borrowed(ValueRef::Null),
            Arg::Int(i) => ToSqlOutput::Borrowed(ValueRef::Integer(i)),
            Arg::Real(f) => ToSqlOutput::Borrowed(ValueRef::Real(f)),
            Arg::Text(s) => ToSqlOutput::Borrowed(ValueRef::Text(s.as_bytes())),
        })
    }
}

fn read_row(r: &rusqlite::Row<'_>, width: usize) -> rusqlite::Result<Row> {
    let mut vals = Vec::with_capacity(width);
    for i in 0..width {
        vals.push(match r.get_ref(i)? {
            ValueRef::Null => Val::Null,
            ValueRef::Integer(v) => Val::Int(v),
            ValueRef::Real(v) => Val::Real(v),
            ValueRef::Text(t) => Val::Text(String::from_utf8_lossy(t).into_owned()),
            // No column of the run schema holds a blob. One that somehow does
            // reads as text rather than failing the whole statement.
            ValueRef::Blob(b) => Val::Text(String::from_utf8_lossy(b).into_owned()),
        });
    }
    Ok(Row(vals))
}

fn query(c: &Connection, sql: &str, params: &[Arg<'_>]) -> DbResult<Vec<Row>> {
    let mut stmt = c.prepare_cached(sql).map_err(dbe)?;
    let width = stmt.column_count();
    let rows = stmt
        .query_map(rusqlite::params_from_iter(params.iter()), |r| read_row(r, width))
        .map_err(dbe)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(dbe)
}

fn execute(c: &Connection, sql: &str, params: &[Arg<'_>]) -> DbResult<usize> {
    c.prepare_cached(sql)
        .and_then(|mut s| s.execute(rusqlite::params_from_iter(params.iter())))
        .map_err(dbe)
}

/// The locked connection, as the engine sees it.
struct SqliteConn<'a>(&'a mut Connection);

impl Db for SqliteConn<'_> {
    fn execute(&self, sql: &str, params: &[Arg<'_>]) -> DbResult<usize> {
        execute(self.0, sql, params)
    }

    fn query(&self, sql: &str, params: &[Arg<'_>]) -> DbResult<Vec<Row>> {
        query(self.0, sql, params)
    }

    fn dialect(&self) -> Dialect {
        Dialect::Sqlite
    }
}

impl Conn for SqliteConn<'_> {
    fn transaction(&mut self) -> DbResult<Box<dyn Tx + '_>> {
        Ok(Box::new(SqliteTx(self.0.transaction().map_err(dbe)?)))
    }

    fn tx_attempts(&self) -> usize {
        // A single writer never loses a transaction to another one.
        1
    }

    fn backoff(&self, _attempt: usize) {}
}

struct SqliteTx<'a>(rusqlite::Transaction<'a>);

impl Db for SqliteTx<'_> {
    fn execute(&self, sql: &str, params: &[Arg<'_>]) -> DbResult<usize> {
        execute(&self.0, sql, params)
    }

    fn query(&self, sql: &str, params: &[Arg<'_>]) -> DbResult<Vec<Row>> {
        query(&self.0, sql, params)
    }

    fn dialect(&self) -> Dialect {
        Dialect::Sqlite
    }
}

impl Tx for SqliteTx<'_> {
    fn commit(self: Box<Self>) -> DbResult<()> {
        self.0.commit().map_err(dbe)
    }
}
