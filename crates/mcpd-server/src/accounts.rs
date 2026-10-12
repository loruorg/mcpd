//! Accounts, for a daemon started with `--signup`.
//!
//! Without `--signup` none of this runs: every endpoint here answers 404, no
//! file is opened, and keys come from the configuration exactly as before.
//!
//! With it, anyone may create an account. An account owns one project with a
//! random name, and nothing else. Its session token grants every scope on that
//! project; the API keys it mints grant one scope each on that project. Neither
//! reaches any other project, which is what lets strangers share one daemon.
//!
//! ## Where accounts live
//!
//! In their own small SQLite file, not in the `Store` trait, so the three signal
//! backends do not change and a Postgres or ClickHouse deployment needs no new
//! table. The daemon is the only writer.
//!
//! Every live credential is also held in memory, keyed by its SHA-256 digest,
//! so authorising a request is a map lookup and never a disk read. The secret
//! itself is never stored, on disk or in memory: a token or key is shown once,
//! and from then on the daemon holds only its digest. A copy of the file
//! therefore opens nothing.
//!
//! Looking a digest up in a map is not a constant-time comparison, and it does
//! not need to be. What a timing difference could reveal is how much of a
//! *digest* a guess matched, and SHA-256 gives no way to turn a matching digest
//! prefix into a matching secret.
//!
//! ## Passwords
//!
//! Hashed with argon2id at the crate's default cost, which is the OWASP
//! recommendation. Hashing is deliberately slow, so it runs off the async
//! threads and at most [`MAX_CONCURRENT_HASHES`] at a time: a flood of log-in
//! attempts queues rather than starving every other request of CPU.

use crate::auth::{authorize_session, Scope};
use crate::error::{ApiError, ApiResult, QuotaRefusal};
use crate::query::Peer;
use crate::state::AppState;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::Json;
use mcpd_core::{
    Account, AccountCredentials, AccountKey, AccountSession, CreateAccountKeyRequest,
    CreatedAccountKey, DeleteResult, KeyScope,
};
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};
use time::OffsetDateTime;

/// Shortest password accepted, in characters.
pub const MIN_PASSWORD_CHARS: usize = 10;

/// Longest password accepted, in bytes. Bounds what one request can make the
/// hasher read.
pub const MAX_PASSWORD_BYTES: usize = 1024;

/// Longest email address accepted, in bytes, which is the RFC 5321 limit.
pub const MAX_EMAIL_BYTES: usize = 254;

/// Most API keys one account may hold.
pub const MAX_KEYS_PER_ACCOUNT: usize = 50;

/// Longest key label accepted, in characters.
pub const MAX_KEY_NAME_CHARS: usize = 100;

/// How long a session lasts.
pub const SESSION_DAYS: i64 = 30;

/// Most live sessions one account keeps. Logging in past this ends the oldest,
/// so a script that logs in on every run cannot grow the table without bound.
pub const MAX_SESSIONS_PER_ACCOUNT: usize = 20;

/// Most password hashes computed at once.
pub const MAX_CONCURRENT_HASHES: usize = 4;

/// Sign-ups one client address may make per hour.
pub const SIGNUPS_PER_HOUR: u32 = 5;

/// Log-in attempts one client address may make per quarter hour.
pub const LOGINS_PER_QUARTER_HOUR: u32 = 20;

/// Failed log-ins one email may take per quarter hour. Counted per email as
/// well as per address, so a guesser spread across many addresses still meets
/// a limit. Only failures count, so nobody can lock an account out by logging
/// it in.
pub const FAILED_LOGINS_PER_EMAIL: u32 = 10;

/// The tables. Times are whole seconds since the Unix epoch.
const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS accounts (
    id            TEXT PRIMARY KEY,
    email         TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    project       TEXT NOT NULL UNIQUE,
    created_at    INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS account_keys (
    id         TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    scope      TEXT NOT NULL,
    name       TEXT,
    prefix     TEXT NOT NULL,
    digest     TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS account_keys_account ON account_keys (account_id);
CREATE TABLE IF NOT EXISTS account_sessions (
    digest     TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS account_sessions_account ON account_sessions (account_id);
";

/// A failure inside the account store. Never shown to a caller as is: the
/// handlers turn it into a 500 that names the store, not the query.
#[derive(Debug, thiserror::Error)]
pub enum AccountError {
    /// SQLite refused.
    #[error("the account store failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// The password hasher refused.
    #[error("hashing the password failed: {0}")]
    Hash(String),
    /// A thread panicked while holding the store's lock.
    #[error("the account store's lock was poisoned by a panic")]
    Poisoned,
}

/// What a presented token or key is, once its digest is found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credential {
    /// The account that owns it.
    pub account_id: String,
    /// That account's project: the only one this credential reaches.
    pub project: String,
    /// A session or a key.
    pub kind: CredentialKind,
}

/// The two kinds of account credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialKind {
    /// An API key with one scope.
    Key {
        /// The key's id.
        id: String,
        /// What it may do.
        scope: Scope,
    },
    /// A session from logging in. Every scope, until it expires.
    Session {
        /// When it stops working.
        expires_at: OffsetDateTime,
    },
}

impl Credential {
    /// Whether this credential satisfies `scope`. An admin key also satisfies
    /// the two run scopes, the same rule configured keys follow.
    #[must_use]
    pub fn allows(&self, scope: Scope) -> bool {
        match &self.kind {
            CredentialKind::Session { .. } => true,
            CredentialKind::Key { scope: held, .. } => {
                *held == scope || (*held == Scope::Admin && scope.admin_implies())
            }
        }
    }
}

/// The accounts, their keys and their sessions.
pub struct AccountStore {
    conn: Mutex<Connection>,
    /// Every live credential by the SHA-256 of its secret.
    live: RwLock<HashMap<[u8; 32], Credential>>,
}

/// A login's stored half: what to verify the password against, and what to
/// issue the session for.
pub struct LoginRecord {
    /// The account.
    pub account_id: String,
    /// Its argon2id hash.
    pub password_hash: String,
}

fn now_secs() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}

fn at(secs: i64) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(secs).unwrap_or(OffsetDateTime::UNIX_EPOCH)
}

/// The SHA-256 of a secret, which is all the daemon ever keeps of it.
#[must_use]
pub fn digest(secret: &str) -> [u8; 32] {
    Sha256::digest(secret.as_bytes()).into()
}

/// A random secret with a recognisable prefix. 32 bytes from the thread's
/// CSPRNG, so nothing about one secret says anything about another.
fn secret(prefix: &str) -> String {
    let bytes: [u8; 32] = rand::random();
    format!("{prefix}{}", hex::encode(bytes))
}

/// A random identifier that is not a secret.
fn identifier(prefix: &str, bytes: usize) -> String {
    let raw: [u8; 16] = rand::random();
    format!("{prefix}{}", hex::encode(&raw[..bytes.min(16)]))
}

fn scope_of(text: &str) -> Option<Scope> {
    Scope::ALL.iter().copied().find(|s| s.as_str() == text)
}

fn scope_from_wire(s: KeyScope) -> Scope {
    match s {
        KeyScope::Ingest => Scope::Ingest,
        KeyScope::Read => Scope::Read,
        KeyScope::Write => Scope::Write,
        KeyScope::Admin => Scope::Admin,
        KeyScope::Execute => Scope::Execute,
        KeyScope::Worker => Scope::Worker,
    }
}

fn scope_to_wire(s: Scope) -> KeyScope {
    match s {
        Scope::Ingest => KeyScope::Ingest,
        Scope::Read => KeyScope::Read,
        Scope::Write => KeyScope::Write,
        Scope::Admin => KeyScope::Admin,
        Scope::Execute => KeyScope::Execute,
        Scope::Worker => KeyScope::Worker,
    }
}

impl AccountStore {
    /// Open or create the file at `path`, or a store in memory for `:memory:`.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be opened or its tables
    /// cannot be created.
    pub fn open(path: &str) -> Result<Self, AccountError> {
        let conn = if path == ":memory:" {
            Connection::open_in_memory()?
        } else {
            Connection::open(path)?
        };
        conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;")?;
        if path != ":memory:" {
            // Returns the mode it set as a row, so it is read rather than run.
            conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
        }
        conn.execute_batch(SCHEMA)?;
        let store = Self { conn: Mutex::new(conn), live: RwLock::new(HashMap::new()) };
        store.reload()?;
        Ok(store)
    }

    fn conn(&self) -> Result<std::sync::MutexGuard<'_, Connection>, AccountError> {
        self.conn.lock().map_err(|_| AccountError::Poisoned)
    }

    fn live_mut(
        &self,
    ) -> Result<std::sync::RwLockWriteGuard<'_, HashMap<[u8; 32], Credential>>, AccountError> {
        self.live.write().map_err(|_| AccountError::Poisoned)
    }

    /// Rebuild the in-memory map from the file, dropping expired sessions.
    fn reload(&self) -> Result<(), AccountError> {
        let conn = self.conn()?;
        conn.execute("DELETE FROM account_sessions WHERE expires_at <= ?1", [now_secs()])?;
        let mut live = HashMap::new();
        let mut keys = conn.prepare(
            "SELECT k.digest, k.id, k.scope, a.id, a.project
             FROM account_keys k JOIN accounts a ON a.id = k.account_id",
        )?;
        let rows = keys.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?;
        for row in rows {
            let (hex_digest, id, scope, account_id, project) = row?;
            // A row this build cannot read grants nothing, rather than
            // guessing what it was meant to grant.
            let (Some(d), Some(scope)) = (parse_digest(&hex_digest), scope_of(&scope)) else {
                continue;
            };
            live.insert(
                d,
                Credential { account_id, project, kind: CredentialKind::Key { id, scope } },
            );
        }
        let mut sessions = conn.prepare(
            "SELECT s.digest, s.expires_at, a.id, a.project
             FROM account_sessions s JOIN accounts a ON a.id = s.account_id",
        )?;
        let rows = sessions.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        for row in rows {
            let (hex_digest, expires, account_id, project) = row?;
            let Some(d) = parse_digest(&hex_digest) else { continue };
            live.insert(
                d,
                Credential {
                    account_id,
                    project,
                    kind: CredentialKind::Session { expires_at: at(expires) },
                },
            );
        }
        drop(keys);
        drop(sessions);
        drop(conn);
        *self.live_mut()? = live;
        Ok(())
    }

    /// What a presented secret is, or `None` when it is no live credential.
    ///
    /// An expired session is `None` here even before anything has removed it.
    #[must_use]
    pub fn lookup(&self, presented: &str) -> Option<Credential> {
        let live = self.live.read().ok()?;
        let found = live.get(&digest(presented))?.clone();
        match found.kind {
            CredentialKind::Session { expires_at } if expires_at <= OffsetDateTime::now_utc() => {
                None
            }
            _ => Some(found),
        }
    }

    /// Every project an account owns. For the retention pass.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be read.
    pub fn projects(&self) -> Result<Vec<String>, AccountError> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT project FROM accounts ORDER BY created_at")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// How many accounts exist. For `mcpd config` and tests.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be read.
    pub fn count(&self) -> Result<u64, AccountError> {
        let conn = self.conn()?;
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM accounts", [], |r| r.get(0))?;
        Ok(u64::try_from(n).unwrap_or(0))
    }

    /// Create an account with a new project, and log it in. `None` when the
    /// email already has an account.
    ///
    /// `email` must already be normalised and `password_hash` already computed:
    /// neither happens under the store's lock.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn create_account(
        &self,
        email: &str,
        password_hash: &str,
    ) -> Result<Option<AccountSession>, AccountError> {
        let account_id = identifier("acct_", 12);
        // 80 random bits. Nobody chooses a project name, so nobody can choose
        // one that already holds somebody else's data.
        let project = identifier("prj_", 10);
        let now = now_secs();
        {
            let mut conn = self.conn()?;
            let tx = conn.transaction()?;
            let taken: Option<i64> = tx
                .query_row("SELECT 1 FROM accounts WHERE email = ?1", [email], |r| r.get(0))
                .optional()?;
            if taken.is_some() {
                return Ok(None);
            }
            tx.execute(
                "INSERT INTO accounts (id, email, password_hash, project, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![account_id, email, password_hash, project, now],
            )?;
            tx.commit()?;
        }
        self.issue_session(&account_id).map(Some)
    }

    /// The stored half of a log-in, or `None` for an email with no account.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be read.
    pub fn find_login(&self, email: &str) -> Result<Option<LoginRecord>, AccountError> {
        let conn = self.conn()?;
        Ok(conn
            .query_row("SELECT id, password_hash FROM accounts WHERE email = ?1", [email], |r| {
                Ok(LoginRecord { account_id: r.get(0)?, password_hash: r.get(1)? })
            })
            .optional()?)
    }

    /// Start a session for an account, ending its oldest past
    /// [`MAX_SESSIONS_PER_ACCOUNT`].
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn issue_session(&self, account_id: &str) -> Result<AccountSession, AccountError> {
        let token = secret("mcpds_");
        let d = digest(&token);
        let now = now_secs();
        let expires = now + SESSION_DAYS * 86_400;
        let (email, project, ended) = {
            let mut conn = self.conn()?;
            let tx = conn.transaction()?;
            let (email, project): (String, String) = tx.query_row(
                "SELECT email, project FROM accounts WHERE id = ?1",
                [account_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            tx.execute(
                "INSERT INTO account_sessions (digest, account_id, created_at, expires_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![hex::encode(d), account_id, now, expires],
            )?;
            // Expired ones, and everything past the newest few.
            let ended: Vec<String> = {
                let mut stmt = tx.prepare(
                    "SELECT digest FROM account_sessions WHERE account_id = ?1
                     AND (expires_at <= ?2 OR digest NOT IN (
                         SELECT digest FROM account_sessions WHERE account_id = ?1
                         ORDER BY created_at DESC, rowid DESC LIMIT ?3))",
                )?;
                let rows = stmt
                    .query_map(params![account_id, now, MAX_SESSIONS_PER_ACCOUNT as i64], |r| {
                        r.get::<_, String>(0)
                    })?;
                rows.collect::<Result<_, _>>()?
            };
            for old in &ended {
                tx.execute("DELETE FROM account_sessions WHERE digest = ?1", [old])?;
            }
            tx.commit()?;
            (email, project, ended)
        };
        let mut live = self.live_mut()?;
        for old in &ended {
            if let Some(d) = parse_digest(old) {
                live.remove(&d);
            }
        }
        live.insert(
            d,
            Credential {
                account_id: account_id.to_owned(),
                project: project.clone(),
                kind: CredentialKind::Session { expires_at: at(expires) },
            },
        );
        Ok(AccountSession { token, expires_at: at(expires), email, project })
    }

    /// End one session. Returns how many were ended, 0 or 1.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn end_session(&self, account_id: &str, presented: &str) -> Result<u32, AccountError> {
        let d = digest(presented);
        let n = self.conn()?.execute(
            "DELETE FROM account_sessions WHERE digest = ?1 AND account_id = ?2",
            params![hex::encode(d), account_id],
        )?;
        self.live_mut()?.remove(&d);
        Ok(u32::try_from(n).unwrap_or(0))
    }

    /// The account behind an id.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be read or the account
    /// is gone.
    pub fn account(&self, account_id: &str) -> Result<Account, AccountError> {
        let conn = self.conn()?;
        Ok(conn.query_row(
            "SELECT email, project, created_at FROM accounts WHERE id = ?1",
            [account_id],
            |r| {
                Ok(Account {
                    email: r.get(0)?,
                    project: r.get(1)?,
                    created_at: at(r.get::<_, i64>(2)?),
                })
            },
        )?)
    }

    /// Every key an account holds, newest first.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be read.
    pub fn list_keys(&self, account_id: &str) -> Result<Vec<AccountKey>, AccountError> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, scope, name, prefix, created_at FROM account_keys
             WHERE account_id = ?1 ORDER BY created_at DESC, rowid DESC",
        )?;
        let rows = stmt.query_map([account_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, scope, name, prefix, created) = row?;
            let Some(scope) = scope_of(&scope) else { continue };
            out.push(AccountKey {
                id,
                scope: scope_to_wire(scope),
                name,
                prefix,
                created_at: at(created),
            });
        }
        Ok(out)
    }

    /// Mint a key. `None` when the account already holds
    /// [`MAX_KEYS_PER_ACCOUNT`].
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn create_key(
        &self,
        account_id: &str,
        scope: Scope,
        name: Option<String>,
    ) -> Result<Option<CreatedAccountKey>, AccountError> {
        let secret = secret("mcpdk_");
        let d = digest(&secret);
        let id = identifier("key_", 8);
        let prefix: String = secret.chars().take(14).collect();
        let now = now_secs();
        let project = {
            let mut conn = self.conn()?;
            let tx = conn.transaction()?;
            let held: i64 = tx.query_row(
                "SELECT COUNT(*) FROM account_keys WHERE account_id = ?1",
                [account_id],
                |r| r.get(0),
            )?;
            if usize::try_from(held).unwrap_or(usize::MAX) >= MAX_KEYS_PER_ACCOUNT {
                return Ok(None);
            }
            let project: String =
                tx.query_row("SELECT project FROM accounts WHERE id = ?1", [account_id], |r| {
                    r.get(0)
                })?;
            tx.execute(
                "INSERT INTO account_keys (id, account_id, scope, name, prefix, digest, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![id, account_id, scope.as_str(), name, prefix, hex::encode(d), now],
            )?;
            tx.commit()?;
            project
        };
        self.live_mut()?.insert(
            d,
            Credential {
                account_id: account_id.to_owned(),
                project,
                kind: CredentialKind::Key { id: id.clone(), scope },
            },
        );
        Ok(Some(CreatedAccountKey {
            key: AccountKey { id, scope: scope_to_wire(scope), name, prefix, created_at: at(now) },
            secret,
        }))
    }

    /// Revoke one of an account's keys. Returns whether it held one by that id.
    ///
    /// The account is part of the match, so one account cannot revoke another's
    /// key by knowing its id.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn revoke_key(&self, account_id: &str, key_id: &str) -> Result<bool, AccountError> {
        let digest_hex: Option<String> = {
            let conn = self.conn()?;
            let found: Option<String> = conn
                .query_row(
                    "SELECT digest FROM account_keys WHERE id = ?1 AND account_id = ?2",
                    params![key_id, account_id],
                    |r| r.get(0),
                )
                .optional()?;
            if found.is_some() {
                conn.execute(
                    "DELETE FROM account_keys WHERE id = ?1 AND account_id = ?2",
                    params![key_id, account_id],
                )?;
            }
            found
        };
        let Some(hex_digest) = digest_hex else { return Ok(false) };
        if let Some(d) = parse_digest(&hex_digest) {
            self.live_mut()?.remove(&d);
        }
        Ok(true)
    }
}

fn parse_digest(text: &str) -> Option<[u8; 32]> {
    hex::decode(text).ok()?.try_into().ok()
}

/// Hash a password with argon2id and a fresh random salt.
///
/// # Errors
/// Returns [`AccountError::Hash`] if the hasher refuses, which it does only for
/// inputs past its own limits.
pub fn hash_password(password: &str) -> Result<String, AccountError> {
    let salt_bytes: [u8; 16] = rand::random();
    let salt =
        SaltString::encode_b64(&salt_bytes).map_err(|e| AccountError::Hash(e.to_string()))?;
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| AccountError::Hash(e.to_string()))
}

/// Whether `password` matches a stored hash. A hash that does not parse
/// matches nothing.
#[must_use]
pub fn verify_password(password: &str, stored: &str) -> bool {
    PasswordHash::new(stored)
        .is_ok_and(|h| Argon2::default().verify_password(password.as_bytes(), &h).is_ok())
}

/// A hash to verify against when the email has no account, so an unknown email
/// costs the same time as a wrong password and the timing does not say which
/// emails exist.
fn decoy_hash() -> &'static str {
    static DECOY: OnceLock<String> = OnceLock::new();
    DECOY.get_or_init(|| hash_password("not a password anyone has").unwrap_or_default())
}

/// The email as it is stored, or a 400 saying what is wrong with it.
///
/// Lowercased whole. Strictly the part before the `@` may be case sensitive,
/// but no mail provider in use treats it so, and two accounts that differ only
/// by case are a support problem and a phishing opportunity.
///
/// # Errors
/// Returns [`ApiError::BadParam`] naming `email`.
pub fn normalise_email(raw: &str) -> ApiResult<String> {
    let bad = |why: &str| ApiError::BadParam {
        field: "email".to_owned(),
        message: format!("{why}; send an address such as ana@example.com"),
    };
    let email = raw.trim().to_lowercase();
    if email.is_empty() {
        return Err(bad("the email is empty"));
    }
    if email.len() > MAX_EMAIL_BYTES {
        return Err(bad(&format!("the email is longer than {MAX_EMAIL_BYTES} bytes")));
    }
    if email.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(bad("the email contains a space or a control character"));
    }
    let Some((local, domain)) = email.split_once('@') else {
        return Err(bad("the email has no @"));
    };
    if domain.contains('@') {
        return Err(bad("the email has more than one @"));
    }
    if local.is_empty() || local.len() > 64 {
        return Err(bad("the part before the @ must be 1 to 64 characters"));
    }
    let labels: Vec<&str> = domain.split('.').collect();
    let label_ok = |l: &&str| {
        !l.is_empty()
            && !l.starts_with('-')
            && !l.ends_with('-')
            && l.chars().all(|c| c.is_alphanumeric() || c == '-')
    };
    if labels.len() < 2 || !labels.iter().all(label_ok) {
        return Err(bad("the part after the @ is not a domain name"));
    }
    Ok(email)
}

/// A 400 for a password that is too short or too long.
///
/// # Errors
/// Returns [`ApiError::BadParam`] naming `password`.
pub fn check_password(password: &str) -> ApiResult<()> {
    if password.chars().count() < MIN_PASSWORD_CHARS {
        return Err(ApiError::BadParam {
            field: "password".to_owned(),
            message: format!("the password must be at least {MIN_PASSWORD_CHARS} characters"),
        });
    }
    if password.len() > MAX_PASSWORD_BYTES {
        return Err(ApiError::BadParam {
            field: "password".to_owned(),
            message: format!("the password must be at most {MAX_PASSWORD_BYTES} bytes"),
        });
    }
    Ok(())
}

// ------------------------------------------------------------ rate limits ---

/// Most buckets one limiter holds. Past this, expired buckets are dropped, and
/// if none has expired a new key is refused: a flood of invented keys must not
/// grow memory without bound, and refusing is the safe side for a door that
/// creates accounts.
const MAX_BUCKETS: usize = 100_000;

/// A fixed-window counter per key: an address, or an email.
pub struct RateLimiter {
    limit: u32,
    window: Duration,
    buckets: Mutex<HashMap<String, (Instant, u32)>>,
}

impl RateLimiter {
    /// `limit` per `window`, per key.
    #[must_use]
    pub fn new(limit: u32, window: Duration) -> Self {
        Self { limit, window, buckets: Mutex::new(HashMap::new()) }
    }

    fn wait(&self, started: Instant, now: Instant) -> u64 {
        self.window.saturating_sub(now.duration_since(started)).as_secs().max(1)
    }

    /// Count one attempt for `key`. `Err` carries the seconds until the key may
    /// try again.
    ///
    /// # Errors
    /// The seconds to wait, when the key is over its limit.
    pub fn hit(&self, key: &str) -> Result<(), u64> {
        let now = Instant::now();
        let mut buckets = self.buckets.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((started, count)) = buckets.get_mut(key) {
            if now.duration_since(*started) < self.window {
                if *count >= self.limit {
                    return Err(self.wait(*started, now));
                }
                *count += 1;
                return Ok(());
            }
            *started = now;
            *count = 1;
            return Ok(());
        }
        if buckets.len() >= MAX_BUCKETS {
            let window = self.window;
            buckets.retain(|_, (started, _)| now.duration_since(*started) < window);
            if buckets.len() >= MAX_BUCKETS {
                return Err(self.window.as_secs().max(1));
            }
        }
        buckets.insert(key.to_owned(), (now, 1));
        Ok(())
    }

    /// Whether `key` is over its limit, without counting an attempt.
    ///
    /// # Errors
    /// The seconds to wait, when the key is over its limit.
    pub fn check(&self, key: &str) -> Result<(), u64> {
        let now = Instant::now();
        let buckets = self.buckets.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        match buckets.get(key) {
            Some((started, count))
                if now.duration_since(*started) < self.window && *count >= self.limit =>
            {
                Err(self.wait(*started, now))
            }
            _ => Ok(()),
        }
    }
}

/// The limits and the hashing bound the account doors share.
pub struct AccountGuards {
    /// Sign-ups per client address.
    pub signup: RateLimiter,
    /// Log-in attempts per client address.
    pub login_address: RateLimiter,
    /// Failed log-ins per email.
    pub login_email: RateLimiter,
    /// Password hashes in flight.
    pub hashing: tokio::sync::Semaphore,
}

impl Default for AccountGuards {
    fn default() -> Self {
        Self {
            signup: RateLimiter::new(SIGNUPS_PER_HOUR, Duration::from_secs(3600)),
            login_address: RateLimiter::new(LOGINS_PER_QUARTER_HOUR, Duration::from_secs(900)),
            login_email: RateLimiter::new(FAILED_LOGINS_PER_EMAIL, Duration::from_secs(900)),
            hashing: tokio::sync::Semaphore::new(MAX_CONCURRENT_HASHES),
        }
    }
}

/// The bucket a client address counts against.
///
/// An IPv6 client is counted by its /64, because one host is routinely handed
/// a whole /64 and could otherwise take a fresh address per attempt. An unknown
/// peer, which only an in-process test produces, shares one bucket.
#[must_use]
pub fn address_bucket(ip: Option<IpAddr>) -> String {
    match ip {
        None => "unknown".to_owned(),
        Some(IpAddr::V4(a)) => a.to_string(),
        Some(IpAddr::V6(a)) => match a.to_ipv4_mapped() {
            Some(v4) => v4.to_string(),
            None => {
                let s = a.segments();
                format!("{:x}:{:x}:{:x}:{:x}::/64", s[0], s[1], s[2], s[3])
            }
        },
    }
}

fn limited(retry_after_seconds: u64, what: &str) -> ApiError {
    ApiError::RateLimited(QuotaRefusal {
        message: format!("{what}; try again in {retry_after_seconds} seconds"),
        retry_after_seconds,
    })
}

// --------------------------------------------------------------- handlers ---

/// The account store, or the 404 a daemon without `--signup` answers with.
fn store(state: &AppState) -> ApiResult<Arc<AccountStore>> {
    if !state.config.signup {
        return Err(ApiError::NotFound(
            "this daemon does not offer accounts; start it with --signup to turn them on"
                .to_owned(),
        ));
    }
    state.accounts.clone().ok_or_else(|| {
        ApiError::Unavailable(
            "sign-up is on but the account store is not open; check --accounts-data".to_owned(),
        )
    })
}

/// Run account store work off the async threads.
async fn blocking<T, F>(work: F) -> ApiResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, AccountError> + Send + 'static,
{
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| ApiError::Internal(format!("account work stopped: {e}")))?
        .map_err(|e| ApiError::Internal(e.to_string()))
}

fn client(state: &AppState, headers: &HeaderMap, peer: &Peer) -> String {
    address_bucket(state.config.client_ip.resolve(headers, peer.0.map(|p| p.ip())))
}

/// The one answer to every failed log-in, so it does not say which part failed.
fn login_refused() -> ApiError {
    ApiError::Unauthorized("that email and password do not match an account".to_owned())
}

/// `POST /v1/api/account/signup`
pub async fn signup(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Json(req): Json<AccountCredentials>,
) -> ApiResult<Json<AccountSession>> {
    let accounts = store(&state)?;
    let email = normalise_email(&req.email)?;
    check_password(&req.password)?;
    state
        .account_guards
        .signup
        .hit(&client(&state, &headers, &peer))
        .map_err(|s| limited(s, "too many sign-ups from this address"))?;
    let hash = {
        let _permit = state
            .account_guards
            .hashing
            .acquire()
            .await
            .map_err(|_| ApiError::Unavailable("the daemon is stopping".to_owned()))?;
        let password = req.password;
        blocking(move || hash_password(&password)).await?
    };
    let created = blocking(move || accounts.create_account(&email, &hash)).await?;
    created.map(Json).ok_or_else(|| {
        ApiError::AccountExists(
            "an account with this email already exists; log in instead".to_owned(),
        )
    })
}

/// `POST /v1/api/account/login`
pub async fn login(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Json(req): Json<AccountCredentials>,
) -> ApiResult<Json<AccountSession>> {
    let accounts = store(&state)?;
    let guards = &state.account_guards;
    guards
        .login_address
        .hit(&client(&state, &headers, &peer))
        .map_err(|s| limited(s, "too many log-in attempts from this address"))?;
    // Not validated beyond length: a malformed email is one with no account,
    // and saying so would be a different answer from a wrong password.
    let email = req.email.trim().to_lowercase();
    if email.is_empty() || email.len() > MAX_EMAIL_BYTES || req.password.len() > MAX_PASSWORD_BYTES
    {
        return Err(login_refused());
    }
    guards
        .login_email
        .check(&email)
        .map_err(|s| limited(s, "too many failed log-ins for this email"))?;
    let lookup = Arc::clone(&accounts);
    let wanted = email.clone();
    let found = blocking(move || lookup.find_login(&wanted)).await?;
    let verified = {
        let _permit = guards
            .hashing
            .acquire()
            .await
            .map_err(|_| ApiError::Unavailable("the daemon is stopping".to_owned()))?;
        let password = req.password;
        blocking(move || {
            Ok(match found {
                Some(record) if verify_password(&password, &record.password_hash) => {
                    Some(record.account_id)
                }
                Some(_) => None,
                None => {
                    // The same work as a real check, so the time taken does
                    // not say the email has no account.
                    let _ = verify_password(&password, decoy_hash());
                    None
                }
            })
        })
        .await?
    };
    let Some(account_id) = verified else {
        let _ = guards.login_email.hit(&email);
        return Err(login_refused());
    };
    let session = blocking(move || accounts.issue_session(&account_id)).await?;
    Ok(Json(session))
}

/// `POST /v1/api/account/logout`
pub async fn logout(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> ApiResult<Json<DeleteResult>> {
    let (credential, presented) = authorize_session(&state, &headers)?;
    let accounts = store(&state)?;
    let deleted =
        blocking(move || accounts.end_session(&credential.account_id, &presented)).await?;
    Ok(Json(DeleteResult { deleted }))
}

/// `GET /v1/api/account`
pub async fn get_account(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> ApiResult<Json<Account>> {
    let (credential, _) = authorize_session(&state, &headers)?;
    let accounts = store(&state)?;
    Ok(Json(blocking(move || accounts.account(&credential.account_id)).await?))
}

/// `GET /v1/api/account/keys`
pub async fn list_account_keys(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> ApiResult<Json<Vec<AccountKey>>> {
    let (credential, _) = authorize_session(&state, &headers)?;
    let accounts = store(&state)?;
    Ok(Json(blocking(move || accounts.list_keys(&credential.account_id)).await?))
}

/// `POST /v1/api/account/keys`
pub async fn create_account_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<CreateAccountKeyRequest>,
) -> ApiResult<Json<CreatedAccountKey>> {
    let (credential, _) = authorize_session(&state, &headers)?;
    let accounts = store(&state)?;
    let name = match req.name.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        None => None,
        Some(n) if n.chars().count() > MAX_KEY_NAME_CHARS => {
            return Err(ApiError::BadParam {
                field: "name".to_owned(),
                message: format!("a key name is at most {MAX_KEY_NAME_CHARS} characters"),
            })
        }
        Some(n) if n.chars().any(char::is_control) => {
            return Err(ApiError::BadParam {
                field: "name".to_owned(),
                message: "a key name cannot hold a control character".to_owned(),
            })
        }
        Some(n) => Some(n.to_owned()),
    };
    let scope = scope_from_wire(req.scope);
    let created =
        blocking(move || accounts.create_key(&credential.account_id, scope, name)).await?;
    created.map(Json).ok_or_else(|| ApiError::BadParam {
        field: "scope".to_owned(),
        message: format!(
            "this account already holds {MAX_KEYS_PER_ACCOUNT} keys; revoke one before \
             creating another"
        ),
    })
}

/// `DELETE /v1/api/account/keys/{key_id}`
pub async fn revoke_account_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(key_id): Path<String>,
) -> ApiResult<Json<DeleteResult>> {
    let (credential, _) = authorize_session(&state, &headers)?;
    let accounts = store(&state)?;
    let wanted = key_id.clone();
    let revoked = blocking(move || accounts.revoke_key(&credential.account_id, &wanted)).await?;
    if revoked {
        Ok(Json(DeleteResult { deleted: 1 }))
    } else {
        Err(ApiError::NotFound(format!("this account holds no key {key_id:?}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emails_are_normalised_and_checked() {
        assert_eq!(normalise_email("  Ana@Example.COM ").unwrap(), "ana@example.com");
        for bad in [
            "",
            "ana",
            "ana@",
            "@example.com",
            "ana@example",
            "a b@example.com",
            "ana@@example.com",
            "ana@-x.com",
            "ana@example..com",
        ] {
            assert!(normalise_email(bad).is_err(), "{bad:?} was accepted");
        }
        let long = format!("{}@example.com", "a".repeat(250));
        assert!(normalise_email(&long).is_err());
    }

    #[test]
    fn passwords_need_ten_characters() {
        assert!(check_password("123456789").is_err());
        assert!(check_password("1234567890").is_ok());
        // Characters, not bytes: ten accented letters are ten characters.
        assert!(check_password("éééééééééé").is_ok());
        assert!(check_password(&"x".repeat(MAX_PASSWORD_BYTES + 1)).is_err());
    }

    #[test]
    fn a_password_verifies_against_its_own_hash_only() {
        let h = hash_password("correct horse battery").unwrap();
        assert!(h.starts_with("$argon2id$"), "{h}");
        assert!(verify_password("correct horse battery", &h));
        assert!(!verify_password("correct horse battery!", &h));
        assert!(!verify_password("correct horse battery", "not a hash"));
    }

    #[test]
    fn the_store_keeps_digests_and_never_secrets() {
        let dir = std::env::temp_dir().join(format!(
            "mcpd-accounts-{}-{}",
            std::process::id(),
            OffsetDateTime::now_utc().unix_timestamp_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.db");
        let path = path.to_str().unwrap();
        let store = AccountStore::open(path).unwrap();
        let session = store.create_account("ana@example.com", "$argon2id$x").unwrap().unwrap();
        let key = store
            .create_key(&lookup_id(&store, &session.token), Scope::Ingest, None)
            .unwrap()
            .unwrap();
        drop(store);
        let bytes = std::fs::read(path).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        let wal = std::fs::read(format!("{path}-wal")).unwrap_or_default();
        let wal = String::from_utf8_lossy(&wal);
        for secret in [&session.token, &key.secret] {
            assert!(!text.contains(secret.as_str()) && !wal.contains(secret.as_str()));
        }
        // Reopened from the file, both still work.
        let store = AccountStore::open(path).unwrap();
        assert!(store.lookup(&session.token).is_some());
        assert!(store.lookup(&key.secret).is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn lookup_id(store: &AccountStore, token: &str) -> String {
        store.lookup(token).unwrap().account_id
    }

    #[test]
    fn a_revoked_key_and_an_ended_session_stop_working() {
        let store = AccountStore::open(":memory:").unwrap();
        let s = store.create_account("ana@example.com", "$argon2id$x").unwrap().unwrap();
        let id = lookup_id(&store, &s.token);
        let k = store.create_key(&id, Scope::Read, Some("ci".into())).unwrap().unwrap();
        assert!(store.lookup(&k.secret).is_some());
        assert!(store.revoke_key(&id, &k.key.id).unwrap());
        assert!(store.lookup(&k.secret).is_none());
        assert_eq!(store.end_session(&id, &s.token).unwrap(), 1);
        assert!(store.lookup(&s.token).is_none());
    }

    #[test]
    fn one_account_cannot_revoke_another_accounts_key() {
        let store = AccountStore::open(":memory:").unwrap();
        let a = store.create_account("a@example.com", "$argon2id$x").unwrap().unwrap();
        let b = store.create_account("b@example.com", "$argon2id$x").unwrap().unwrap();
        assert_ne!(a.project, b.project);
        let a_id = lookup_id(&store, &a.token);
        let b_id = lookup_id(&store, &b.token);
        let key = store.create_key(&a_id, Scope::Ingest, None).unwrap().unwrap();
        assert!(!store.revoke_key(&b_id, &key.key.id).unwrap());
        assert!(store.lookup(&key.secret).is_some());
    }

    #[test]
    fn an_email_has_one_account() {
        let store = AccountStore::open(":memory:").unwrap();
        assert!(store.create_account("a@example.com", "$argon2id$x").unwrap().is_some());
        assert!(store.create_account("a@example.com", "$argon2id$y").unwrap().is_none());
    }

    #[test]
    fn sessions_past_the_cap_end_the_oldest() {
        let store = AccountStore::open(":memory:").unwrap();
        let first = store.create_account("a@example.com", "$argon2id$x").unwrap().unwrap();
        let id = lookup_id(&store, &first.token);
        for _ in 0..MAX_SESSIONS_PER_ACCOUNT {
            store.issue_session(&id).unwrap();
        }
        assert!(store.lookup(&first.token).is_none(), "the oldest session outlived the cap");
    }

    #[test]
    fn the_key_cap_is_enforced() {
        let store = AccountStore::open(":memory:").unwrap();
        let s = store.create_account("a@example.com", "$argon2id$x").unwrap().unwrap();
        let id = lookup_id(&store, &s.token);
        for _ in 0..MAX_KEYS_PER_ACCOUNT {
            assert!(store.create_key(&id, Scope::Ingest, None).unwrap().is_some());
        }
        assert!(store.create_key(&id, Scope::Ingest, None).unwrap().is_none());
    }

    #[test]
    fn a_key_grants_its_scope_and_an_admin_key_also_the_run_scopes() {
        let key = |scope| Credential {
            account_id: "a".into(),
            project: "p".into(),
            kind: CredentialKind::Key { id: "k".into(), scope },
        };
        assert!(key(Scope::Ingest).allows(Scope::Ingest));
        assert!(!key(Scope::Ingest).allows(Scope::Read));
        assert!(!key(Scope::Read).allows(Scope::Admin));
        assert!(key(Scope::Admin).allows(Scope::Execute));
        assert!(key(Scope::Admin).allows(Scope::Worker));
        assert!(!key(Scope::Admin).allows(Scope::Read));
    }

    #[test]
    fn the_limiter_counts_per_key_and_refuses_past_the_limit() {
        let l = RateLimiter::new(2, Duration::from_secs(60));
        assert!(l.hit("a").is_ok());
        assert!(l.hit("a").is_ok());
        assert!(l.check("a").is_err());
        let wait = l.hit("a").unwrap_err();
        assert!((1..=60).contains(&wait), "{wait}");
        assert!(l.hit("b").is_ok(), "one key's limit spilled onto another");
    }

    #[test]
    fn an_ipv6_client_counts_by_its_64() {
        let a: IpAddr = "2001:db8:1:2::1".parse().unwrap();
        let b: IpAddr = "2001:db8:1:2:ffff::9".parse().unwrap();
        let c: IpAddr = "2001:db8:1:3::1".parse().unwrap();
        assert_eq!(address_bucket(Some(a)), address_bucket(Some(b)));
        assert_ne!(address_bucket(Some(a)), address_bucket(Some(c)));
        let mapped: IpAddr = "::ffff:192.0.2.7".parse().unwrap();
        assert_eq!(address_bucket(Some(mapped)), "192.0.2.7");
    }
}
