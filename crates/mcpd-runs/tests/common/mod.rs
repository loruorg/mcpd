//! One run store per test, on whichever backend the test is running against.
//!
//! Every conformance test is written once and runs twice: against SQLite
//! always, and against Postgres when `MCPD_TEST_POSTGRES_URL` names a server.
//! [`backend_tests!`] writes the two test functions, and [`TestStore::open`]
//! reads which one is running, so a test body never names a backend.
//!
//! Each Postgres test gets a schema of its own, reached through the search
//! path, so tests run in parallel on one database without seeing each other's
//! rows, exactly as each SQLite test gets a file of its own.
//!
//! With `MCPD_REQUIRE_LIVE` set, a missing URL fails the Postgres half rather
//! than skipping it. A skip there reports success having touched no server,
//! which is how a backend goes untested while CI stays green.

#![allow(dead_code)]

use mcpd_runs::store::{RunConfig, RunStore};
use mcpd_runs::SqliteRunStore;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Which backend a test body is running against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Sqlite,
    Postgres,
}

tokio::task_local! {
    static BACKEND: Backend;
}

/// The variable that names a Postgres server for these tests. The same one
/// the signal store's suites read, so one server runs both.
pub const POSTGRES_URL: &str = "MCPD_TEST_POSTGRES_URL";

/// The Postgres URL, or `None` to skip. Panics when `MCPD_REQUIRE_LIVE` is
/// set and the URL is not, because then a skip would read as a pass.
pub fn postgres_url() -> Option<String> {
    if cfg!(not(feature = "postgres")) {
        assert!(
            std::env::var("MCPD_REQUIRE_LIVE").is_err(),
            "MCPD_REQUIRE_LIVE is set but this build has no Postgres run store; build with \
             --features postgres"
        );
        return None;
    }
    match std::env::var(POSTGRES_URL) {
        Ok(url) if !url.trim().is_empty() => Some(url),
        _ => {
            assert!(
                std::env::var("MCPD_REQUIRE_LIVE").is_err(),
                "MCPD_REQUIRE_LIVE is set but {POSTGRES_URL} is not, so the Postgres run store \
                 suite would pass having contacted no server; set {POSTGRES_URL} to a server \
                 this test may create schemas on"
            );
            eprintln!("{POSTGRES_URL} not set, skipping the Postgres half");
            None
        }
    }
}

/// Run `body` with `backend` as the one [`TestStore::open`] opens.
pub async fn with_backend<F: std::future::Future<Output = ()>>(backend: Backend, body: F) {
    BACKEND.scope(backend, body).await;
}

/// The backend the running test body asked for.
pub fn current() -> Backend {
    BACKEND.try_with(|b| *b).expect(
        "TestStore::open was called outside a backend_tests! body, or inside a task spawned \
         from one; open the store before spawning",
    )
}

/// Write each test as one body that runs on both backends. Each name becomes
/// a module holding a `sqlite` and a `postgres` test.
#[macro_export]
macro_rules! backend_tests {
    ($( $(#[$meta:meta])* async fn $name:ident() $body:block )*) => {
        $(
            $(#[$meta])*
            mod $name {
                #[allow(unused_imports)]
                use super::*;

                #[tokio::test]
                async fn sqlite() {
                    $crate::common::with_backend($crate::common::Backend::Sqlite, async $body)
                        .await;
                }

                #[tokio::test]
                async fn postgres() {
                    if $crate::common::postgres_url().is_some() {
                        $crate::common::with_backend(
                            $crate::common::Backend::Postgres,
                            async $body,
                        )
                        .await;
                    }
                }
            }
        )*
    };
}

/// A name no other test in any process uses: the pid, a stamp taken once per
/// process, because pids are recycled, and a counter.
fn unique(prefix: &str) -> String {
    static N: AtomicU64 = AtomicU64::new(0);
    static STAMP: std::sync::OnceLock<u128> = std::sync::OnceLock::new();
    let stamp = STAMP.get_or_init(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    });
    format!("{prefix}_{}_{stamp}_{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed))
}

/// Where a test store's rows live.
enum Place {
    Sqlite(PathBuf),
    Postgres { url: String, schema: String },
}

/// A run store for one test, removed when the test ends.
pub struct TestStore {
    store: Arc<dyn RunStore>,
    place: Place,
}

impl std::ops::Deref for TestStore {
    type Target = dyn RunStore;

    fn deref(&self) -> &Self::Target {
        &*self.store
    }
}

impl TestStore {
    /// A fresh, migrated store on the running test's backend.
    pub async fn open(config: RunConfig) -> Self {
        let place = match current() {
            Backend::Sqlite => {
                let dir = std::env::temp_dir().join(unique("mcpd-runs"));
                std::fs::create_dir_all(&dir).expect("temp dir");
                Place::Sqlite(dir.join("runs.db"))
            }
            Backend::Postgres => {
                let url = postgres_url().expect("a Postgres URL");
                let schema = unique("mcpd_runs_t");
                let (client, conn) =
                    tokio_postgres::connect(&url, tokio_postgres::NoTls).await.expect("connect");
                let task = tokio::spawn(conn);
                client
                    .batch_execute(&format!("CREATE SCHEMA {schema}"))
                    .await
                    .expect("create a schema for this test");
                drop(client);
                let _ = task.await;
                Place::Postgres { url, schema }
            }
        };
        let store = Self::build(&place, config);
        store.migrate().await.expect("migrate");
        Self { store, place }
    }

    fn build(place: &Place, config: RunConfig) -> Arc<dyn RunStore> {
        match place {
            Place::Sqlite(path) => Arc::new(SqliteRunStore::open(path, config).expect("open")),
            Place::Postgres { url, schema } => {
                let mut pg: tokio_postgres::Config = url.parse().expect("a Postgres URL");
                pg.options(format!("-c search_path={schema}"));
                let manager = deadpool_postgres::Manager::from_config(
                    pg,
                    tokio_postgres::NoTls,
                    deadpool_postgres::ManagerConfig {
                        recycling_method: deadpool_postgres::RecyclingMethod::Fast,
                    },
                );
                let pool = deadpool_postgres::Pool::builder(manager)
                    .max_size(8)
                    .runtime(deadpool_postgres::Runtime::Tokio1)
                    .build()
                    .expect("pool");
                #[cfg(feature = "postgres")]
                return Arc::new(mcpd_runs::PostgresRunStore::new(pool, config));
                #[cfg(not(feature = "postgres"))]
                {
                    drop((pool, config));
                    panic!("this build has no Postgres run store; build with --features postgres")
                }
            }
        }
    }

    /// Which backend this store is on.
    pub fn backend(&self) -> Backend {
        match self.place {
            Place::Sqlite(_) => Backend::Sqlite,
            Place::Postgres { .. } => Backend::Postgres,
        }
    }

    /// The store, to hand to a spawned task.
    pub fn shared(&self) -> Arc<dyn RunStore> {
        Arc::clone(&self.store)
    }

    /// A second store over the same rows, as a restarted daemon opens it.
    pub async fn reopen(&self, config: RunConfig) -> Arc<dyn RunStore> {
        let store = Self::build(&self.place, config);
        store.migrate().await.expect("migrate");
        store
    }

    /// The SQLite file, for a test that reads SQLite's own query plan.
    pub fn sqlite_path(&self) -> &std::path::Path {
        match &self.place {
            Place::Sqlite(path) => path,
            Place::Postgres { .. } => panic!("this store is not SQLite"),
        }
    }

    /// Run a statement past the store, on a connection of its own, the way the
    /// crash tests lose the rows a crash would lose. `?N` placeholders, text
    /// values. Answers the rows it changed.
    pub async fn exec(&self, sql: &str, params: &[&str]) -> u64 {
        match &self.place {
            Place::Sqlite(path) => {
                let conn = rusqlite::Connection::open(path).expect("raw connection");
                conn.execute(sql, rusqlite::params_from_iter(params.iter()))
                    .unwrap_or_else(|e| panic!("{sql}: {e}")) as u64
            }
            Place::Postgres { .. } => {
                let (client, task) = self.pg().await;
                let n = client
                    .execute(&dollar(sql), &text_params(params))
                    .await
                    .unwrap_or_else(|e| panic!("{sql}: {e:?}"));
                drop(client);
                let _ = task.await;
                n
            }
        }
    }

    /// Run several statements, with no parameters, such as a trigger the
    /// test installs.
    pub async fn batch(&self, sql: &str) {
        match &self.place {
            Place::Sqlite(path) => {
                let conn = rusqlite::Connection::open(path).expect("raw connection");
                conn.execute_batch(sql).unwrap_or_else(|e| panic!("{sql}: {e}"));
            }
            Place::Postgres { .. } => {
                let (client, task) = self.pg().await;
                client.batch_execute(sql).await.unwrap_or_else(|e| panic!("{sql}: {e:?}"));
                drop(client);
                let _ = task.await;
            }
        }
    }

    /// The one integer a statement answers, such as a `COUNT(*)`.
    pub async fn count(&self, sql: &str, params: &[&str]) -> i64 {
        match &self.place {
            Place::Sqlite(path) => {
                let conn = rusqlite::Connection::open(path).expect("raw connection");
                conn.query_row(sql, rusqlite::params_from_iter(params.iter()), |r| r.get(0))
                    .unwrap_or_else(|e| panic!("{sql}: {e}"))
            }
            Place::Postgres { .. } => {
                let (client, task) = self.pg().await;
                let row = client
                    .query_one(&dollar(sql), &text_params(params))
                    .await
                    .unwrap_or_else(|e| panic!("{sql}: {e:?}"));
                drop(client);
                let _ = task.await;
                row.get::<_, i64>(0)
            }
        }
    }

    async fn pg(&self) -> (tokio_postgres::Client, tokio::task::JoinHandle<()>) {
        let Place::Postgres { url, schema } = &self.place else { unreachable!() };
        let mut pg: tokio_postgres::Config = url.parse().expect("a Postgres URL");
        pg.options(format!("-c search_path={schema}"));
        let (client, conn) = pg.connect(tokio_postgres::NoTls).await.expect("connect");
        let task = tokio::spawn(async move {
            let _ = conn.await;
        });
        (client, task)
    }
}

/// `?N` as `$N`. The test statements hold no `?` inside a literal.
fn dollar(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut chars = sql.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '?' && chars.peek().is_some_and(char::is_ascii_digit) {
            out.push('$');
        } else {
            out.push(c);
        }
    }
    out
}

fn text_params<'a>(params: &'a [&'a str]) -> Vec<&'a (dyn tokio_postgres::types::ToSql + Sync)> {
    params.iter().map(|p| p as &(dyn tokio_postgres::types::ToSql + Sync)).collect()
}

impl Drop for TestStore {
    fn drop(&mut self) {
        match &self.place {
            Place::Sqlite(path) => {
                if let Some(dir) = path.parent() {
                    let _ = std::fs::remove_dir_all(dir);
                }
            }
            Place::Postgres { url, schema } => {
                // On a thread with its own runtime, because this runs inside
                // the test's. Bounded by a lock timeout: a transaction the
                // test's pool left open would otherwise hold the drop for
                // ever, and a schema left behind costs nothing but space.
                let (url, schema) = (url.clone(), schema.clone());
                let _ = std::thread::spawn(move || {
                    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build();
                    let Ok(rt) = rt else { return };
                    rt.block_on(async move {
                        let Ok((client, conn)) =
                            tokio_postgres::connect(&url, tokio_postgres::NoTls).await
                        else {
                            return;
                        };
                        tokio::spawn(conn);
                        let _ = client
                            .batch_execute(&format!(
                                "SET lock_timeout = '5s'; DROP SCHEMA IF EXISTS {schema} CASCADE"
                            ))
                            .await;
                    });
                })
                .join();
            }
        }
    }
}
