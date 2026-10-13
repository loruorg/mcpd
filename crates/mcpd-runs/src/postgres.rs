//! The Postgres backend for the run core.
//!
//! The engine is the same one SQLite runs; this file only teaches it to reach
//! Postgres. Two decisions here carry the weight.
//!
//! Every transaction is `SERIALIZABLE`. SQLite gives the engine a single
//! writer, and every function in it is written as if its transaction ran
//! alone: a claim reads the runnable set and then leases from it, a deadline
//! reads a run and then moves it. Under `SERIALIZABLE` that stays true with any
//! number of replicas on one database, because two transactions that would
//! have seen each other's writes cannot both commit. The one that loses is
//! rolled back and run again from the start, which the engine already does for
//! a lost compare-and-set. A unique violation is treated the same way, since
//! under this isolation it only happens when a concurrent transaction inserted
//! the same key first, and the rerun then finds that row.
//!
//! The engine's functions are synchronous, so each closure runs on a blocking
//! thread and waits on each statement there. It holds one pooled connection for
//! the whole closure, so its statements and its transaction are on one session.
//!
//! Timestamps are `BIGINT` nanoseconds and text is compared byte for byte
//! (`COLLATE "C"`), as in SQLite, so a page cursor or an ordering written on one
//! backend means the same thing on the other.

use crate::db::{Arg, Conn, Db, DbError, DbResult, Dialect, Row, Tx, Val};
use crate::engine::{retries_spent, Backend, Engine};
use crate::store::RunConfig;
use crate::{Error, Result};
use async_trait::async_trait;
use bytes::BytesMut;
use deadpool_postgres::{Object, Pool};
use tokio::runtime::Handle;
use tokio_postgres::error::SqlState;
use tokio_postgres::types::{to_sql_checked, FromSql, IsNull, ToSql, Type};
use tokio_postgres::IsolationLevel;

const SCHEMA: &str = include_str!("postgres_schema.sql");

/// The schema this build writes.
///
/// `migrate` skips the DDL when the recorded version matches, so a change to
/// `postgres_schema.sql`, an index included, has to raise this as well or no
/// existing database gains it.
pub const SCHEMA_VERSION: i64 = 1;

/// The advisory-lock key migration takes. Distinct from the signal store's, so
/// the two schemas can migrate in one database without waiting on each other.
const MIGRATION_LOCK: i64 = 0x6d63_7064_0002;

/// How many times a closure, and separately one transaction inside it, is run
/// again after losing to a concurrent one. Losses come in bursts when many
/// workers claim at once, so this is more generous than the single-writer
/// count: each attempt is one short transaction, and giving up hands a worker
/// an error for work that would have gone through a moment later.
const ATTEMPTS: usize = 12;

type BoxError = Box<dyn std::error::Error + Sync + Send>;

/// The durable run state, in Postgres.
pub struct PostgresRunStore {
    pub(crate) engine: Engine<Postgres>,
}

impl PostgresRunStore {
    /// A store on `pool`. Nothing connects until the first call.
    ///
    /// The pool is the caller's to build, so the daemon builds it the way it
    /// builds the signal store's, with the same TLS settings and timeouts.
    #[must_use]
    pub fn new(pool: Pool, config: RunConfig) -> Self {
        Self { engine: Engine::new(Postgres { pool }, config) }
    }
}

crate::engine::forward_run_store!(PostgresRunStore);

/// The pool, and nothing else: every connection is taken per call.
pub(crate) struct Postgres {
    pool: Pool,
}

/// Wrap a driver error, keeping what the server said. The driver's own text
/// is "db error", and the message, the constraint and the detail are further
/// down its chain.
fn describe(e: &tokio_postgres::Error) -> String {
    let mut msg = e.to_string();
    let mut src = std::error::Error::source(e);
    while let Some(s) = src {
        msg.push_str(": ");
        msg.push_str(&s.to_string());
        src = s.source();
    }
    format!("postgres: {msg}")
}

fn pge(e: tokio_postgres::Error) -> DbError {
    let lost = e.code().is_some_and(|c| {
        *c == SqlState::T_R_SERIALIZATION_FAILURE
            || *c == SqlState::T_R_DEADLOCK_DETECTED
            || *c == SqlState::UNIQUE_VIOLATION
    });
    if lost {
        DbError::Backend(Error::Retry(describe(&e)))
    } else {
        DbError::Backend(Error::Backend(describe(&e)))
    }
}

fn be(e: tokio_postgres::Error) -> Error {
    Error::Backend(describe(&e))
}

fn unreachable(e: deadpool_postgres::PoolError) -> Error {
    Error::Backend(format!("the postgres run store is unreachable: {e}"))
}

#[async_trait]
impl Backend for Postgres {
    async fn run<T, F>(&self, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: Fn(&mut dyn Conn) -> Result<T> + Send + 'static,
    {
        let mut client = self.pool.get().await.map_err(unreachable)?;
        let handle = Handle::current();
        tokio::task::spawn_blocking(move || {
            let mut c = PgConn { client: &mut client, handle };
            let mut last = None;
            for attempt in 0..ATTEMPTS {
                if attempt > 0 {
                    c.backoff(attempt);
                }
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
        let mut client = self.pool.get().await.map_err(unreachable)?;
        // Fast path: a start with nothing to apply takes no lock and runs no
        // DDL, so a replica coming up never stalls the ones already serving.
        if schema_is_current(&client).await? {
            return Ok(());
        }
        let tx = client.transaction().await.map_err(be)?;
        // Replicas share the database and all migrate on start, and two
        // concurrent `CREATE TABLE IF NOT EXISTS` on one relation can deadlock.
        // `_xact_` so the commit, or an abort, releases it.
        tx.execute("SELECT pg_advisory_xact_lock($1)", &[&MIGRATION_LOCK]).await.map_err(be)?;
        if schema_is_current(&tx).await? {
            return Ok(());
        }
        tx.batch_execute(SCHEMA).await.map_err(be)?;
        let now = i64::try_from(time::OffsetDateTime::now_utc().unix_timestamp_nanos())
            .unwrap_or(i64::MAX);
        tx.execute(
            "INSERT INTO run_schema_version (version, applied_at) VALUES ($1, $2)
             ON CONFLICT (version) DO NOTHING",
            &[&SCHEMA_VERSION, &now],
        )
        .await
        .map_err(be)?;
        tx.commit().await.map_err(be)?;
        Ok(())
    }
}

/// Whether the schema is already at the version this build writes.
/// `to_regclass` answers NULL for a table that does not exist, so this needs
/// no error path on an empty database. Both names resolve through the search
/// path, so a schema per deployment works.
async fn schema_is_current(c: &impl deadpool_postgres::GenericClient) -> Result<bool> {
    let row = c
        .query_one("SELECT to_regclass('run_schema_version') IS NOT NULL", &[])
        .await
        .map_err(be)?;
    if !row.get::<_, bool>(0) {
        return Ok(false);
    }
    let found = c
        .query_opt("SELECT 1 FROM run_schema_version WHERE version = $1", &[&SCHEMA_VERSION])
        .await
        .map_err(be)?;
    Ok(found.is_some())
}

/// The engine's `?N` placeholders as Postgres's `$N`. Text inside single
/// quotes is left alone, so a literal can never be rewritten.
fn pg_sql(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut quoted = false;
    let mut chars = sql.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                quoted = !quoted;
                out.push(c);
            }
            '?' if !quoted && chars.peek().is_some_and(char::is_ascii_digit) => out.push('$'),
            _ => out.push(c),
        }
    }
    out
}

/// Bind a value as whatever type the server inferred for its parameter. The
/// engine writes one kind of integer and one kind of text, and Postgres infers
/// `integer` for `?2 = 0` and `text` for a parameter it has no other clue for,
/// so the conversion is decided here rather than at every statement.
impl ToSql for Arg<'_> {
    fn to_sql(&self, ty: &Type, out: &mut BytesMut) -> std::result::Result<IsNull, BoxError> {
        match *self {
            Arg::Null => Ok(IsNull::Yes),
            Arg::Int(v) => match *ty {
                Type::INT8 => v.to_sql(ty, out),
                Type::INT4 => i32::try_from(v)?.to_sql(ty, out),
                Type::INT2 => i16::try_from(v)?.to_sql(ty, out),
                #[allow(clippy::cast_precision_loss)]
                Type::FLOAT8 => (v as f64).to_sql(ty, out),
                Type::BOOL => (v != 0).to_sql(ty, out),
                _ => text(&v.to_string(), ty, out),
            },
            Arg::Real(v) => match *ty {
                Type::FLOAT8 => v.to_sql(ty, out),
                #[allow(clippy::cast_possible_truncation)]
                Type::FLOAT4 => (v as f32).to_sql(ty, out),
                _ => text(&v.to_string(), ty, out),
            },
            Arg::Text(s) => text(s, ty, out),
        }
    }

    fn accepts(_: &Type) -> bool {
        true
    }

    to_sql_checked!();
}

fn text(s: &str, ty: &Type, out: &mut BytesMut) -> std::result::Result<IsNull, BoxError> {
    match *ty {
        Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME | Type::UNKNOWN => s.to_sql(ty, out),
        _ => Err(format!("a text value cannot bind as a parameter of type {ty}").into()),
    }
}

/// Read any column the run schema produces into the engine's four kinds.
impl<'a> FromSql<'a> for Val {
    fn from_sql(ty: &Type, raw: &'a [u8]) -> std::result::Result<Self, BoxError> {
        Ok(match *ty {
            Type::INT8 => Val::Int(i64::from_sql(ty, raw)?),
            Type::INT4 => Val::Int(i32::from_sql(ty, raw)?.into()),
            Type::INT2 => Val::Int(i16::from_sql(ty, raw)?.into()),
            Type::BOOL => Val::Int(bool::from_sql(ty, raw)?.into()),
            Type::FLOAT8 => Val::Real(f64::from_sql(ty, raw)?),
            Type::FLOAT4 => Val::Real(f32::from_sql(ty, raw)?.into()),
            Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME | Type::UNKNOWN => {
                Val::Text(<&str>::from_sql(ty, raw)?.to_owned())
            }
            _ => {
                return Err(format!("a column of type {ty} has no reading in the run store").into())
            }
        })
    }

    fn from_sql_null(_: &Type) -> std::result::Result<Self, BoxError> {
        Ok(Val::Null)
    }

    fn accepts(_: &Type) -> bool {
        true
    }
}

fn bind<'a>(params: &'a [Arg<'a>]) -> Vec<&'a (dyn ToSql + Sync)> {
    params.iter().map(|a| a as &(dyn ToSql + Sync)).collect()
}

fn rows(rows: Vec<tokio_postgres::Row>) -> DbResult<Vec<Row>> {
    rows.into_iter()
        .map(|r| (0..r.len()).map(|i| r.try_get::<_, Val>(i)).collect::<Result<Vec<_>, _>>())
        .map(|r| r.map(Row).map_err(pge))
        .collect()
}

/// One pooled connection, held for one closure.
struct PgConn<'a> {
    client: &'a mut Object,
    handle: Handle,
}

impl Db for PgConn<'_> {
    fn execute(&self, sql: &str, params: &[Arg<'_>]) -> DbResult<usize> {
        let client: &Object = self.client;
        self.handle.block_on(async {
            let stmt = client.prepare_cached(&pg_sql(sql)).await.map_err(pge)?;
            let n = client.execute(&stmt, &bind(params)).await.map_err(pge)?;
            Ok(usize::try_from(n).unwrap_or(usize::MAX))
        })
    }

    fn query(&self, sql: &str, params: &[Arg<'_>]) -> DbResult<Vec<Row>> {
        let client: &Object = self.client;
        self.handle.block_on(async {
            let stmt = client.prepare_cached(&pg_sql(sql)).await.map_err(pge)?;
            rows(client.query(&stmt, &bind(params)).await.map_err(pge)?)
        })
    }

    fn dialect(&self) -> Dialect {
        Dialect::Postgres
    }
}

impl Conn for PgConn<'_> {
    fn transaction(&mut self) -> DbResult<Box<dyn Tx + '_>> {
        let handle = self.handle.clone();
        let tx = handle
            .block_on(
                self.client
                    .build_transaction()
                    .isolation_level(IsolationLevel::Serializable)
                    .start(),
            )
            .map_err(pge)?;
        Ok(Box::new(PgTx { tx, handle }))
    }

    fn tx_attempts(&self) -> usize {
        ATTEMPTS
    }

    fn backoff(&self, attempt: usize) {
        // Random and growing, capped well under a request's patience, so two
        // transactions that collided do not collide again on the same beat.
        let ceiling = 1u64 << attempt.min(7);
        let ms = rand::random_range(0..=ceiling);
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }
}

struct PgTx<'a> {
    tx: deadpool_postgres::Transaction<'a>,
    handle: Handle,
}

impl Db for PgTx<'_> {
    fn execute(&self, sql: &str, params: &[Arg<'_>]) -> DbResult<usize> {
        self.handle.block_on(async {
            let stmt = self.tx.prepare_cached(&pg_sql(sql)).await.map_err(pge)?;
            let n = self.tx.execute(&stmt, &bind(params)).await.map_err(pge)?;
            Ok(usize::try_from(n).unwrap_or(usize::MAX))
        })
    }

    fn query(&self, sql: &str, params: &[Arg<'_>]) -> DbResult<Vec<Row>> {
        self.handle.block_on(async {
            let stmt = self.tx.prepare_cached(&pg_sql(sql)).await.map_err(pge)?;
            rows(self.tx.query(&stmt, &bind(params)).await.map_err(pge)?)
        })
    }

    fn dialect(&self) -> Dialect {
        Dialect::Postgres
    }
}

impl Tx for PgTx<'_> {
    fn commit(self: Box<Self>) -> DbResult<()> {
        let PgTx { tx, handle } = *self;
        handle.block_on(tx.commit()).map_err(pge)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_become_dollar_numbers_outside_literals() {
        assert_eq!(
            pg_sql("SELECT ?1, '?2' FROM t WHERE a = ?12 AND b LIKE '%?%' AND c = ?3"),
            "SELECT $1, '?2' FROM t WHERE a = $12 AND b LIKE '%?%' AND c = $3"
        );
    }
}
