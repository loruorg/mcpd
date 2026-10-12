//! Accounts, organisations and their credentials, for a daemon started with
//! `--signup` or `--accounts`.
//!
//! Without either, none of this runs: every endpoint here answers 404, no file
//! is opened, and keys come from the configuration exactly as before.
//!
//! `--signup` lets anyone create an account. `--accounts` turns on the same
//! accounts, organisations, roles and single sign-on without public sign-up:
//! people arrive by invitation, by single sign-on or by SCIM, which is what a
//! self-hosted deployment wants. Configured keys keep working beside both.
//!
//! ## Tenancy
//!
//! Every account project belongs to an organisation. An account created by
//! sign-up gets an organisation of its own with one project in it, so a person
//! who never invites anybody sees the same thing as before: one project, and
//! keys for it. Members of an organisation reach its projects through their
//! role (see `roles`), and nothing an account holds reaches a project of an
//! organisation it does not belong to. That is what lets strangers share one
//! daemon.
//!
//! ## Where accounts live
//!
//! In their own small SQLite file, not in the `Store` trait, so the three signal
//! backends do not change and a Postgres or ClickHouse deployment needs no new
//! table. The daemon is the only writer.
//!
//! Every live credential is also held in memory, keyed by its SHA-256 digest,
//! and so is every account's reach (see `access`), so authorising a request is
//! two map lookups and never a disk read. The secret itself is never stored, on
//! disk or in memory: a token or key is shown once, and from then on the daemon
//! holds only its digest. A copy of the file therefore opens nothing. One-time
//! tokens in mailed links, invitation tokens, recovery codes and SCIM tokens are
//! kept the same way.
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
//!
//! ## Answers that do not say which emails exist
//!
//! Sign-up, log-in and password reset answer the same way whether or not an
//! account uses the address, and spend the same hashing work either way. What
//! differs goes in a mail to the address, which only its owner reads.

pub(crate) mod access;
mod lifecycle;
pub mod org_store;
pub mod roles;
mod schema;
pub mod scim_store;
pub mod sso_store;

pub use lifecycle::{LoginRecord, TokenPurpose};
pub use org_store::{InviteRow, OrgRow};
pub use scim_store::{ScimGroup, ScimUser};
pub use sso_store::{SsoSettings, SsoStateData};

use crate::auth::{authorize_session, Scope};
use crate::error::{ApiError, ApiResult, QuotaRefusal};
use crate::query::Peer;
use crate::state::AppState;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use mcpd_core::{
    Account, AccountCredentials, AccountDeleteRequest, AccountDeletion, AccountKey, AccountNotice,
    AccountSession, CreateAccountKeyRequest, CreatedAccountKey, DeleteResult, EmailChangeRequest,
    KeyScope, PasswordChangeRequest, PasswordForgotRequest, PasswordResetRequest, SessionInfo,
    TokenRequest, TotpCodeRequest, TotpDisableRequest, TotpEnabled, TotpSetup,
};
use roles::Role;
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};
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

/// How long a session from a password lasts.
pub const SESSION_DAYS: i64 = 30;

/// How long a session from single sign-on lasts. Shorter than a password
/// session, so a person removed at the identity provider stops reaching the
/// organisation within a day even with no SCIM to say so.
pub const SSO_SESSION_HOURS: i64 = 24;

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

/// Mails one address may be sent per hour by sign-up, reset and email change,
/// so the daemon cannot be used to flood somebody's inbox.
pub const MAILS_PER_EMAIL_PER_HOUR: u32 = 3;

/// Requests that carry a one-time token, per client address per quarter hour.
pub const TOKEN_TRIES_PER_QUARTER_HOUR: u32 = 30;

/// Failed password or code checks one account may take per quarter hour inside
/// a session: changing the password, turning two-step log-in off, deleting.
pub const SENSITIVE_FAILURES_PER_ACCOUNT: u32 = 10;

/// How long a link to confirm an address works, in seconds.
pub const VERIFY_TTL_SECONDS: i64 = 24 * 3600;

/// How long a password reset link works, in seconds.
pub const RESET_TTL_SECONDS: i64 = 3600;

/// How many recovery codes turning two-step log-in on returns.
pub const RECOVERY_CODES: usize = 10;

/// Settings the accounts read, apart from `signup` itself.
#[derive(Clone, Debug, Default)]
pub struct AccountSettings {
    /// Accounts without open sign-up. `--accounts`.
    pub enabled: bool,
    /// Where people open the interface, for links in mail and the single
    /// sign-on redirect. `--public-url`.
    pub public_url: Option<String>,
    /// Where mail goes.
    pub mail: crate::mail::MailConfig,
    /// The signals-per-minute ceiling an organisation gets when it sets none.
    /// `--org-quota-per-minute`.
    pub org_quota_per_minute: Option<u32>,
    /// The retention an organisation gets when it sets none, in days.
    /// `--org-retention-days`.
    pub org_retention_days: Option<u32>,
    /// Whether an account may create organisations. `None` follows `--signup`.
    pub allow_org_creation: Option<bool>,
    /// Whether an organisation admin may point single sign-on at an `http`
    /// issuer or one on a private network. Off: on a shared daemon the admin
    /// chooses the URL the daemon fetches. For a test or a closed network.
    pub allow_private_issuers: bool,
    /// Organisations the configuration file declares.
    pub orgs: Vec<ConfigOrg>,
}

/// An organisation declared in the configuration file.
#[derive(Clone, Debug, Default, serde::Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ConfigOrg {
    /// A short stable name. The organisation's id is `org_` followed by it.
    pub slug: String,
    /// The display name. Defaults to the slug.
    #[serde(default)]
    pub name: Option<String>,
    /// Addresses that are owners. Each gets an invitation when it has no
    /// account, and is made an owner whenever it logs in.
    #[serde(default)]
    pub owners: Vec<String>,
    /// Existing projects this organisation owns, by name, such as `default`.
    #[serde(default)]
    pub projects: Vec<String>,
    /// Signals per minute across its projects.
    #[serde(default)]
    pub quota_per_minute: Option<u32>,
    /// Retention for its projects, in days.
    #[serde(default)]
    pub retention_days: Option<u32>,
    /// Single sign-on.
    #[serde(default)]
    pub sso: Option<ConfigSso>,
    /// Groups from single sign-on or SCIM, and the role each maps to.
    #[serde(default)]
    pub group_roles: BTreeMap<String, String>,
    /// The environment variable holding a SCIM token for this organisation.
    /// Keeps the token out of the file.
    #[serde(default)]
    pub scim_token_env: Option<String>,
}

/// Single sign-on declared in the configuration file.
#[derive(Clone, Default, serde::Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ConfigSso {
    /// `oidc` (the default) or `github`.
    #[serde(default)]
    pub provider: Option<String>,
    /// The OpenID Connect issuer URL.
    #[serde(default)]
    pub issuer: Option<String>,
    /// The client id.
    pub client_id: String,
    /// The environment variable holding the client secret.
    #[serde(default)]
    pub client_secret_env: Option<String>,
    /// Claimed email domains.
    #[serde(default)]
    pub domains: Vec<String>,
    /// Refuse password log-in for members in those domains.
    #[serde(default)]
    pub enforce: bool,
    /// The role for a member single sign-on creates.
    #[serde(default)]
    pub default_role: Option<String>,
    /// The claim that lists groups.
    #[serde(default)]
    pub groups_claim: Option<String>,
}

impl std::fmt::Debug for ConfigSso {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConfigSso")
            .field("provider", &self.provider)
            .field("issuer", &self.issuer)
            .field("client_id", &self.client_id)
            .field("domains", &self.domains)
            .field("enforce", &self.enforce)
            .finish_non_exhaustive()
    }
}

impl AccountSettings {
    /// The base every mailed link and the redirect URL start from.
    #[must_use]
    pub fn base_url(&self) -> String {
        self.public_url.as_deref().map_or_else(
            || "http://localhost:4317".to_owned(),
            |u| u.trim_end_matches('/').to_owned(),
        )
    }
}

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
    /// A session or a key.
    pub kind: CredentialKind,
}

/// The two kinds of account credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialKind {
    /// An API key with one scope on one project.
    Key {
        /// The key's id.
        id: String,
        /// What it may do.
        scope: Scope,
        /// The one project it reaches.
        project: String,
    },
    /// A session from logging in. Every scope the account's roles carry, until
    /// it expires.
    Session {
        /// The session's public id.
        id: String,
        /// When it stops working.
        expires_at: OffsetDateTime,
        /// The organisation a single sign-on session is limited to.
        org: Option<String>,
    },
}

impl Credential {
    /// Whether a key holding `held` may be used for `scope`. An admin key also
    /// satisfies the two run scopes, the same rule configured keys follow.
    #[must_use]
    pub fn key_allows(held: Scope, scope: Scope) -> bool {
        held == scope || (held == Scope::Admin && scope.admin_implies())
    }

    /// The organisation a session is limited to, when it is.
    #[must_use]
    pub fn session_org(&self) -> Option<&str> {
        match &self.kind {
            CredentialKind::Session { org, .. } => org.as_deref(),
            CredentialKind::Key { .. } => None,
        }
    }
}

/// How a session started, and what it is limited to.
#[derive(Debug, Clone, Default)]
pub struct SessionMeta {
    /// `password`, `sso`, `invite`, `verify` or `reset`.
    pub via: &'static str,
    /// The organisation a single sign-on session is limited to.
    pub org: Option<String>,
    /// The client's description of itself.
    pub user_agent: Option<String>,
    /// The client address.
    pub address: Option<String>,
}

/// The accounts, their organisations, keys and sessions.
pub struct AccountStore {
    conn: Mutex<Connection>,
    /// Every live credential by the SHA-256 of its secret.
    live: RwLock<HashMap<[u8; 32], Credential>>,
    /// Every account's reach.
    access: RwLock<HashMap<String, access::AccountAccess>>,
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

fn digest_hex(secret: &str) -> String {
    hex::encode(digest(secret))
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

/// A password hash that no password matches, for an account with no password.
/// Starts with `!`, which no PHC string does, so it cannot parse as one.
fn no_password() -> String {
    format!("!{}", hex::encode(rand::random::<[u8; 16]>()))
}

impl AccountStore {
    /// Open or create the file at `path`, or a store in memory for `:memory:`,
    /// and bring an older file up to date.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be opened or its tables
    /// cannot be created.
    pub fn open(path: &str) -> Result<Self, AccountError> {
        let mut conn = if path == ":memory:" {
            Connection::open_in_memory()?
        } else {
            Connection::open(path)?
        };
        conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;")?;
        if path != ":memory:" {
            // Returns the mode it set as a row, so it is read rather than run.
            conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
        }
        schema::migrate(&mut conn)?;
        let store = Self {
            conn: Mutex::new(conn),
            live: RwLock::new(HashMap::new()),
            access: RwLock::new(HashMap::new()),
        };
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

    /// Rebuild both in-memory maps from the file, dropping expired sessions and
    /// one-time tokens.
    fn reload(&self) -> Result<(), AccountError> {
        let conn = self.conn()?;
        conn.execute("DELETE FROM account_sessions WHERE expires_at <= ?1", [now_secs()])?;
        conn.execute("DELETE FROM account_tokens WHERE expires_at <= ?1", [now_secs()])?;
        conn.execute("DELETE FROM org_invites WHERE expires_at <= ?1", [now_secs()])?;
        let mut live = HashMap::new();
        let mut keys = conn.prepare(
            "SELECT k.digest, k.id, k.scope, k.account_id, k.project FROM account_keys k
             WHERE k.project IS NOT NULL",
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
                Credential { account_id, kind: CredentialKind::Key { id, scope, project } },
            );
        }
        let mut sessions = conn
            .prepare("SELECT digest, expires_at, account_id, id, org_id FROM account_sessions")?;
        let rows = sessions.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
            ))
        })?;
        for row in rows {
            let (hex_digest, expires, account_id, id, org) = row?;
            let Some(d) = parse_digest(&hex_digest) else { continue };
            live.insert(
                d,
                Credential {
                    account_id,
                    kind: CredentialKind::Session {
                        id: id.unwrap_or_default(),
                        expires_at: at(expires),
                        org,
                    },
                },
            );
        }
        drop(keys);
        drop(sessions);
        let reach = access::load_all(&conn)?;
        drop(conn);
        *self.live_mut()? = live;
        *self.access.write().map_err(|_| AccountError::Poisoned)? = reach;
        Ok(())
    }

    /// Re-read the reach of these accounts after a change that concerns them.
    fn refresh(&self, ids: &[String]) -> Result<(), AccountError> {
        let mut loaded = Vec::with_capacity(ids.len());
        {
            let conn = self.conn()?;
            for id in ids {
                loaded.push((id.clone(), access::load(&conn, id)?));
            }
        }
        let mut map = self.access.write().map_err(|_| AccountError::Poisoned)?;
        for (id, a) in loaded {
            match a {
                Some(a) => {
                    map.insert(id, a);
                }
                None => {
                    map.remove(&id);
                }
            }
        }
        Ok(())
    }

    /// Re-read every member of an organisation, after a change to its projects.
    fn refresh_org(&self, org_id: &str) -> Result<(), AccountError> {
        let ids: Vec<String> = {
            let conn = self.conn()?;
            let mut stmt = conn.prepare("SELECT account_id FROM org_members WHERE org_id = ?1")?;
            let rows = stmt.query_map([org_id], |r| r.get::<_, String>(0))?;
            rows.collect::<Result<_, _>>()?
        };
        self.refresh(&ids)
    }

    fn reach_of(&self, account_id: &str) -> Option<access::AccountAccess> {
        self.access.read().ok()?.get(account_id).cloned()
    }

    /// What a presented secret is, or `None` when it is no live credential.
    ///
    /// An expired session is `None` here even before anything has removed it,
    /// and so is every credential of a deactivated account.
    #[must_use]
    pub fn lookup(&self, presented: &str) -> Option<Credential> {
        let found = self.live.read().ok()?.get(&digest(presented))?.clone();
        if let CredentialKind::Session { expires_at, .. } = &found.kind {
            if *expires_at <= OffsetDateTime::now_utc() {
                return None;
            }
        }
        let disabled = self.access.read().ok()?.get(&found.account_id).is_none_or(|a| a.disabled);
        if disabled {
            return None;
        }
        Some(found)
    }

    /// The projects a credential reaches for `scope`. Empty means refused.
    ///
    /// A key reaches its one project, and only while the role of the account
    /// that made it still carries the scope there, so demoting or removing a
    /// member disarms their keys at once. A session reaches every project the
    /// account's roles carry the scope on, inside its organisation when it is
    /// limited to one.
    #[must_use]
    pub fn reach(&self, c: &Credential, scope: Scope) -> BTreeSet<String> {
        let Some(a) = self.reach_of(&c.account_id) else { return BTreeSet::new() };
        match &c.kind {
            CredentialKind::Key { scope: held, project, .. } => {
                let role_ok = !a.disabled
                    && a.projects.get(project).is_some_and(|(_, role)| role.permits(scope));
                if role_ok && Credential::key_allows(*held, scope) {
                    BTreeSet::from([project.clone()])
                } else {
                    BTreeSet::new()
                }
            }
            CredentialKind::Session { org, .. } => a.reach(scope, org.as_deref()),
        }
    }

    /// The project a request means when it names none: a key's project, else
    /// the account's own, else the first project its session reaches.
    #[must_use]
    pub fn home_project(&self, c: &Credential) -> Option<String> {
        match &c.kind {
            CredentialKind::Key { project, .. } => Some(project.clone()),
            CredentialKind::Session { org, .. } => {
                let a = self.reach_of(&c.account_id)?;
                if org.is_none() {
                    if let Some(home) = a.home.clone().filter(|h| a.projects.contains_key(h)) {
                        return Some(home);
                    }
                }
                a.reach(Scope::Read, org.as_deref()).into_iter().next()
            }
        }
    }

    /// The account's role in an organisation, or `None` when it is not a member.
    #[must_use]
    pub fn role_in(&self, account_id: &str, org_id: &str) -> Option<Role> {
        let a = self.reach_of(account_id)?;
        if a.disabled {
            return None;
        }
        a.orgs.get(org_id).copied()
    }

    /// The account's effective role on a project, or `None`.
    #[must_use]
    pub fn role_on(&self, account_id: &str, project: &str) -> Option<(String, Role)> {
        self.reach_of(account_id)?.projects.get(project).cloned()
    }

    /// Every project an account-owned organisation holds, with the retention
    /// it keeps, for the retention pass. `default_days` applies to an
    /// organisation that sets none.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be read.
    pub fn retention_plan(
        &self,
        default_days: Option<u32>,
    ) -> Result<Vec<(String, u32)>, AccountError> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT p.project, o.retention_days FROM org_projects p JOIN orgs o ON o.id = p.org_id
             ORDER BY p.created_at",
        )?;
        let rows =
            stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<i64>>(1)?)))?;
        let mut out = Vec::new();
        for row in rows {
            let (project, days) = row?;
            let days = days.and_then(|d| u32::try_from(d).ok()).or(default_days);
            if let Some(d) = days {
                out.push((project, d));
            }
        }
        Ok(out)
    }

    /// Every project an account owns, oldest first.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be read.
    pub fn projects(&self) -> Result<Vec<String>, AccountError> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT project FROM org_projects ORDER BY created_at")?;
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

    /// Create an account by sign-up, with an organisation and a project of its
    /// own. The address is not confirmed yet. Returns the account id, or `None`
    /// when the email already has an account.
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
    ) -> Result<Option<String>, AccountError> {
        let account_id = identifier("acct_", 12);
        let org_id = format!("org_{}", &account_id[5..]);
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
            tx.execute(
                "INSERT INTO orgs (id, name, created_at) VALUES (?1, 'Personal', ?2)",
                params![org_id, now],
            )?;
            tx.execute(
                "INSERT INTO org_members (org_id, account_id, role, via, created_at)
                 VALUES (?1, ?2, 'owner', 'created', ?3)",
                params![org_id, account_id, now],
            )?;
            tx.execute(
                "INSERT INTO org_projects (project, org_id, created_at) VALUES (?1, ?2, ?3)",
                params![project, org_id, now],
            )?;
            tx.commit()?;
        }
        self.refresh(std::slice::from_ref(&account_id))?;
        Ok(Some(account_id))
    }

    /// Create an account that joins by invitation, single sign-on or SCIM: no
    /// organisation or project of its own. `password_hash` of `None` means it
    /// has no password. Returns `None` when the email already has an account.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn create_member_account(
        &self,
        email: &str,
        password_hash: Option<&str>,
        managed_by: Option<&str>,
    ) -> Result<Option<String>, AccountError> {
        let account_id = identifier("acct_", 12);
        let now = now_secs();
        let hash = password_hash.map_or_else(no_password, str::to_owned);
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        let taken: Option<i64> = tx
            .query_row("SELECT 1 FROM accounts WHERE email = ?1", [email], |r| r.get(0))
            .optional()?;
        if taken.is_some() {
            return Ok(None);
        }
        tx.execute(
            "INSERT INTO accounts (id, email, password_hash, project, created_at,
                                   email_verified_at, managed_by)
             VALUES (?1, ?2, ?3, NULL, ?4, ?4, ?5)",
            params![account_id, email, hash, now, managed_by],
        )?;
        tx.commit()?;
        drop(conn);
        self.refresh(std::slice::from_ref(&account_id))?;
        Ok(Some(account_id))
    }

    /// The account id behind an email, when there is one.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be read.
    pub fn account_id_for(&self, email: &str) -> Result<Option<String>, AccountError> {
        let conn = self.conn()?;
        Ok(conn
            .query_row("SELECT id FROM accounts WHERE email = ?1", [email], |r| r.get(0))
            .optional()?)
    }

    /// Start a session for an account, ending its oldest past
    /// [`MAX_SESSIONS_PER_ACCOUNT`].
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn issue_session(
        &self,
        account_id: &str,
        meta: &SessionMeta,
    ) -> Result<AccountSession, AccountError> {
        let token = secret("mcpds_");
        let d = digest(&token);
        let id = identifier("ses_", 8);
        let now = now_secs();
        let lifetime =
            if meta.org.is_some() { SSO_SESSION_HOURS * 3600 } else { SESSION_DAYS * 86_400 };
        let expires = now + lifetime;
        let (email, ended) = {
            let mut conn = self.conn()?;
            let tx = conn.transaction()?;
            let email: String =
                tx.query_row("SELECT email FROM accounts WHERE id = ?1", [account_id], |r| {
                    r.get(0)
                })?;
            tx.execute(
                "INSERT INTO account_sessions (digest, account_id, created_at, expires_at, id,
                                               org_id, via, user_agent, address)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    hex::encode(d),
                    account_id,
                    now,
                    expires,
                    id,
                    meta.org,
                    meta.via,
                    meta.user_agent,
                    meta.address
                ],
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
            (email, ended)
        };
        let credential = Credential {
            account_id: account_id.to_owned(),
            kind: CredentialKind::Session { id, expires_at: at(expires), org: meta.org.clone() },
        };
        {
            let mut live = self.live_mut()?;
            for old in &ended {
                if let Some(d) = parse_digest(old) {
                    live.remove(&d);
                }
            }
            live.insert(d, credential.clone());
        }
        Ok(AccountSession {
            token,
            expires_at: at(expires),
            email,
            project: self.home_project(&credential),
            org_id: meta.org.clone(),
        })
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

    /// End sessions of an account that match `filter`, a SQL condition on the
    /// `account_sessions` row with `?2` bound to `arg`.
    fn end_sessions_where(
        &self,
        account_id: &str,
        filter: &str,
        arg: Option<&str>,
    ) -> Result<u32, AccountError> {
        let digests: Vec<String> = {
            let conn = self.conn()?;
            let sql =
                format!("SELECT digest FROM account_sessions WHERE account_id = ?1 AND ({filter})");
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params![account_id, arg], |r| r.get::<_, String>(0))?;
            let found: Vec<String> = rows.collect::<Result<_, _>>()?;
            for d in &found {
                conn.execute("DELETE FROM account_sessions WHERE digest = ?1", [d])?;
            }
            found
        };
        let mut live = self.live_mut()?;
        for d in &digests {
            if let Some(d) = parse_digest(d) {
                live.remove(&d);
            }
        }
        Ok(u32::try_from(digests.len()).unwrap_or(u32::MAX))
    }

    /// End every session of an account except the one presented, if any.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn end_other_sessions(
        &self,
        account_id: &str,
        keep: Option<&str>,
    ) -> Result<u32, AccountError> {
        let keep = keep.map(digest_hex);
        self.end_sessions_where(account_id, "?2 IS NULL OR digest <> ?2", keep.as_deref())
    }

    /// End every session of an account limited to one organisation.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn end_org_sessions(&self, account_id: &str, org_id: &str) -> Result<u32, AccountError> {
        self.end_sessions_where(account_id, "org_id = ?2", Some(org_id))
    }

    /// End every password session of an account, for an organisation that has
    /// just started enforcing single sign-on.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn end_password_sessions(&self, account_id: &str) -> Result<u32, AccountError> {
        self.end_sessions_where(account_id, "org_id IS NULL AND ?2 IS NULL", None)
    }

    /// The account behind an id.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be read or the account
    /// is gone.
    pub fn account(&self, account_id: &str) -> Result<Account, AccountError> {
        let conn = self.conn()?;
        Ok(conn.query_row(
            "SELECT email, project, created_at, email_verified_at, totp_secret, password_hash,
                    managed_by
             FROM accounts WHERE id = ?1",
            [account_id],
            |r| {
                Ok(Account {
                    email: r.get(0)?,
                    project: r.get(1)?,
                    created_at: at(r.get::<_, i64>(2)?),
                    email_verified: r.get::<_, Option<i64>>(3)?.is_some(),
                    totp_enabled: r.get::<_, Option<String>>(4)?.is_some(),
                    has_password: !r.get::<_, String>(5)?.starts_with('!'),
                    managed_by: r.get(6)?,
                })
            },
        )?)
    }

    fn key_rows(&self, sql: &str, arg: &str) -> Result<Vec<AccountKey>, AccountError> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map([arg], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, Option<String>>(6)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, scope, name, prefix, created, project, created_by) = row?;
            let (Some(scope), Some(project)) = (scope_of(&scope), project) else { continue };
            out.push(AccountKey {
                id,
                scope: scope_to_wire(scope),
                project,
                name,
                prefix,
                created_at: at(created),
                created_by,
            });
        }
        Ok(out)
    }

    /// Every key an account holds, newest first.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be read.
    pub fn list_keys(&self, account_id: &str) -> Result<Vec<AccountKey>, AccountError> {
        self.key_rows(
            "SELECT id, scope, name, prefix, created_at, project, NULL FROM account_keys
             WHERE account_id = ?1 ORDER BY created_at DESC, rowid DESC",
            account_id,
        )
    }

    /// Mint a key on a project. `None` when the account already holds
    /// [`MAX_KEYS_PER_ACCOUNT`]. The caller has checked that the account's role
    /// on `project` carries `scope`.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn create_key(
        &self,
        account_id: &str,
        project: &str,
        scope: Scope,
        name: Option<String>,
    ) -> Result<Option<CreatedAccountKey>, AccountError> {
        let secret = secret("mcpdk_");
        let d = digest(&secret);
        let id = identifier("key_", 8);
        let prefix: String = secret.chars().take(14).collect();
        let now = now_secs();
        {
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
            tx.execute(
                "INSERT INTO account_keys (id, account_id, scope, name, prefix, digest,
                                           created_at, project)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![id, account_id, scope.as_str(), name, prefix, hex::encode(d), now, project],
            )?;
            tx.commit()?;
        }
        self.live_mut()?.insert(
            d,
            Credential {
                account_id: account_id.to_owned(),
                kind: CredentialKind::Key { id: id.clone(), scope, project: project.to_owned() },
            },
        );
        Ok(Some(CreatedAccountKey {
            key: AccountKey {
                id,
                scope: scope_to_wire(scope),
                project: project.to_owned(),
                name,
                prefix,
                created_at: at(now),
                created_by: None,
            },
            secret,
        }))
    }

    /// Delete key rows matching a condition, and drop them from memory.
    fn delete_keys_where(&self, filter: &str, a: &str, b: &str) -> Result<u32, AccountError> {
        let digests: Vec<String> = {
            let conn = self.conn()?;
            let mut stmt =
                conn.prepare(&format!("SELECT digest FROM account_keys WHERE {filter}"))?;
            let rows = stmt.query_map(params![a, b], |r| r.get::<_, String>(0))?;
            let found: Vec<String> = rows.collect::<Result<_, _>>()?;
            conn.execute(&format!("DELETE FROM account_keys WHERE {filter}"), params![a, b])?;
            found
        };
        let mut live = self.live_mut()?;
        for d in &digests {
            if let Some(d) = parse_digest(d) {
                live.remove(&d);
            }
        }
        Ok(u32::try_from(digests.len()).unwrap_or(u32::MAX))
    }

    /// Revoke one of an account's keys. Returns whether it held one by that id.
    ///
    /// The account is part of the match, so one account cannot revoke another's
    /// key by knowing its id.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn revoke_key(&self, account_id: &str, key_id: &str) -> Result<bool, AccountError> {
        Ok(self.delete_keys_where("id = ?1 AND account_id = ?2", key_id, account_id)? > 0)
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
/// matches nothing, which is how an account with no password refuses every
/// password.
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
    if !is_domain(domain) {
        return Err(bad("the part after the @ is not a domain name"));
    }
    Ok(email)
}

/// Whether `domain` is a plausible domain name: two or more labels, each of
/// letters, digits and inner hyphens.
#[must_use]
pub fn is_domain(domain: &str) -> bool {
    let labels: Vec<&str> = domain.split('.').collect();
    let label_ok = |l: &&str| {
        !l.is_empty()
            && l.len() <= 63
            && !l.starts_with('-')
            && !l.ends_with('-')
            && l.chars().all(|c| c.is_alphanumeric() || c == '-')
    };
    labels.len() >= 2 && labels.iter().all(label_ok)
}

/// The domain of a normalised email.
#[must_use]
pub fn email_domain(email: &str) -> &str {
    email.rsplit_once('@').map_or("", |(_, d)| d)
}

/// A 400 for a password that is too short or too long.
///
/// # Errors
/// Returns [`ApiError::BadParam`] naming `password`.
pub fn check_password(password: &str) -> ApiResult<()> {
    check_password_field(password, "password")
}

fn check_password_field(password: &str, field: &str) -> ApiResult<()> {
    if password.chars().count() < MIN_PASSWORD_CHARS {
        return Err(ApiError::BadParam {
            field: field.to_owned(),
            message: format!("the password must be at least {MIN_PASSWORD_CHARS} characters"),
        });
    }
    if password.len() > MAX_PASSWORD_BYTES {
        return Err(ApiError::BadParam {
            field: field.to_owned(),
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
    /// Mails per recipient.
    pub mail_per_email: RateLimiter,
    /// Requests carrying a one-time token, and password reset requests, per
    /// client address.
    pub tokens: RateLimiter,
    /// Failed password or code checks inside a session, per account.
    pub sensitive: RateLimiter,
    /// Single sign-on starts and callbacks per client address.
    pub sso: RateLimiter,
    /// SCIM requests per organisation, and failed SCIM tokens per address.
    pub scim: RateLimiter,
    /// Password hashes in flight.
    pub hashing: tokio::sync::Semaphore,
}

impl Default for AccountGuards {
    fn default() -> Self {
        let q = Duration::from_secs(900);
        Self {
            signup: RateLimiter::new(SIGNUPS_PER_HOUR, Duration::from_secs(3600)),
            login_address: RateLimiter::new(LOGINS_PER_QUARTER_HOUR, q),
            login_email: RateLimiter::new(FAILED_LOGINS_PER_EMAIL, q),
            mail_per_email: RateLimiter::new(MAILS_PER_EMAIL_PER_HOUR, Duration::from_secs(3600)),
            tokens: RateLimiter::new(TOKEN_TRIES_PER_QUARTER_HOUR, q),
            sensitive: RateLimiter::new(SENSITIVE_FAILURES_PER_ACCOUNT, q),
            sso: RateLimiter::new(LOGINS_PER_QUARTER_HOUR * 2, q),
            scim: RateLimiter::new(1200, Duration::from_secs(60)),
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

pub(crate) fn limited(retry_after_seconds: u64, what: &str) -> ApiError {
    ApiError::RateLimited(QuotaRefusal {
        message: format!("{what}; try again in {retry_after_seconds} seconds"),
        retry_after_seconds,
    })
}

// --------------------------------------------------------------- handlers ---

/// The account store, or the 404 a daemon without accounts answers with.
pub(crate) fn store(state: &AppState) -> ApiResult<Arc<AccountStore>> {
    if !state.config.accounts_enabled() {
        return Err(ApiError::NotFound(
            "this daemon does not offer accounts; start it with --signup or --accounts to turn \
             them on"
                .to_owned(),
        ));
    }
    state.accounts.clone().ok_or_else(|| {
        ApiError::Unavailable(
            "accounts are on but the account store is not open; check --accounts-data".to_owned(),
        )
    })
}

/// Run account store work off the async threads.
pub(crate) async fn blocking<T, F>(work: F) -> ApiResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, AccountError> + Send + 'static,
{
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| ApiError::Internal(format!("account work stopped: {e}")))?
        .map_err(|e| ApiError::Internal(e.to_string()))
}

/// Hash a password off the async threads, inside the hashing bound.
pub(crate) async fn hash_bounded(state: &AppState, password: String) -> ApiResult<String> {
    let _permit = state
        .account_guards
        .hashing
        .acquire()
        .await
        .map_err(|_| ApiError::Unavailable("the daemon is stopping".to_owned()))?;
    blocking(move || hash_password(&password)).await
}

/// Check a password off the async threads, inside the hashing bound.
pub(crate) async fn verify_bounded(
    state: &AppState,
    password: String,
    stored: String,
) -> ApiResult<bool> {
    let _permit = state
        .account_guards
        .hashing
        .acquire()
        .await
        .map_err(|_| ApiError::Unavailable("the daemon is stopping".to_owned()))?;
    blocking(move || Ok(verify_password(&password, &stored))).await
}

pub(crate) fn client(state: &AppState, headers: &HeaderMap, peer: &Peer) -> String {
    address_bucket(state.config.client_ip.resolve(headers, peer.0.map(|p| p.ip())))
}

/// How a session started, from the request that started it.
pub(crate) fn session_meta(
    state: &AppState,
    headers: &HeaderMap,
    peer: &Peer,
    via: &'static str,
    org: Option<String>,
) -> SessionMeta {
    let user_agent = headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.chars().filter(|c| !c.is_control()).take(200).collect::<String>())
        .filter(|s| !s.is_empty());
    let address =
        state.config.client_ip.resolve(headers, peer.0.map(|p| p.ip())).map(|ip| ip.to_string());
    SessionMeta { via, org, user_agent, address }
}

/// A link into the interface, with the token in the fragment so it never
/// reaches a server log or a `Referer` header.
pub(crate) fn link(state: &AppState, fragment: &str) -> String {
    format!("{}/#{fragment}", state.config.account_settings.base_url())
}

/// Send a mail, unless this address has had its share this hour. Quietly: the
/// caller's answer must not change with whether the mail went.
pub(crate) fn mail(state: &AppState, to: &str, subject: &str, body: String, link: Option<&str>) {
    if state.account_guards.mail_per_email.hit(to).is_err() {
        tracing::info!(subject, "mail not sent: this address had its share this hour");
        return;
    }
    state.account_runtime.mailer.send(&state.tasks, to, subject, body, link);
}

/// The 503 every door that needs mail answers when there is no way to send it,
/// checked before anything else so it is the same for every address.
pub(crate) fn require_mail(state: &AppState) -> ApiResult<()> {
    if state.account_runtime.mailer.can_send() {
        Ok(())
    } else {
        Err(ApiError::Unavailable(
            "this daemon has no mail server, so it cannot send the link; the operator sets \
             --smtp-host and --smtp-from"
                .to_owned(),
        ))
    }
}

/// The one answer to every failed log-in, so it does not say which part failed.
fn login_refused() -> ApiError {
    ApiError::Unauthorized("that email and password do not match an account".to_owned())
}

/// Refuse a session limited to an organisation from changing the account
/// itself, unless that organisation manages the account. A single sign-on
/// session proves the identity provider vouched for the person, which is not
/// the same as proving they own the account's password or its address.
fn require_full_session(credential: &Credential, account: &Account) -> ApiResult<()> {
    match credential.session_org() {
        None => Ok(()),
        Some(org) if account.managed_by.as_deref() == Some(org) => Ok(()),
        Some(_) => Err(ApiError::Forbidden(
            "a single sign-on session cannot change the account itself; log in with your \
             password to do this"
                .to_owned(),
        )),
    }
}

/// Check a password inside a session, counting failures against the account.
async fn confirm_password(
    state: &AppState,
    accounts: &Arc<AccountStore>,
    account_id: &str,
    given: Option<String>,
) -> ApiResult<()> {
    let guards = &state.account_guards;
    guards
        .sensitive
        .check(account_id)
        .map_err(|s| limited(s, "too many failed checks for this account"))?;
    let a = Arc::clone(accounts);
    let id = account_id.to_owned();
    let stored = blocking(move || a.password_hash(&id)).await?;
    if stored.starts_with('!') {
        return Ok(());
    }
    let Some(given) = given.filter(|g| g.len() <= MAX_PASSWORD_BYTES) else {
        let _ = guards.sensitive.hit(account_id);
        return Err(ApiError::Forbidden("this needs your current password".to_owned()));
    };
    if verify_bounded(state, given, stored).await? {
        Ok(())
    } else {
        let _ = guards.sensitive.hit(account_id);
        Err(ApiError::Forbidden("that password is not right".to_owned()))
    }
}

/// Check a two-step code or a recovery code. `Ok(false)` for a wrong one.
async fn check_second_factor(
    accounts: &Arc<AccountStore>,
    account_id: &str,
    code: &str,
) -> ApiResult<bool> {
    let a = Arc::clone(accounts);
    let id = account_id.to_owned();
    let code = code.trim().to_owned();
    blocking(move || a.use_second_factor(&id, &code, OffsetDateTime::now_utc().unix_timestamp()))
        .await
}

/// `POST /v1/api/account/signup`
pub async fn signup(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Json(req): Json<AccountCredentials>,
) -> ApiResult<Json<AccountNotice>> {
    if !state.config.signup {
        return Err(ApiError::NotFound(
            "this daemon does not offer sign-up; start it with --signup to turn it on".to_owned(),
        ));
    }
    let accounts = store(&state)?;
    require_mail(&state)?;
    let email = normalise_email(&req.email)?;
    check_password(&req.password)?;
    state
        .account_guards
        .signup
        .hit(&client(&state, &headers, &peer))
        .map_err(|s| limited(s, "too many sign-ups from this address"))?;
    // Hashed whether or not the address is taken, so the two answers cost the
    // same time.
    let hash = hash_bounded(&state, req.password).await?;
    let a = Arc::clone(&accounts);
    let wanted = email.clone();
    let created = blocking(move || a.create_account(&wanted, &hash)).await?;
    match created {
        Some(account_id) => {
            let a = Arc::clone(&accounts);
            let token = blocking(move || {
                a.issue_token(TokenPurpose::Verify, Some(&account_id), None, VERIFY_TTL_SECONDS)
            })
            .await?;
            let url = link(&state, &format!("verify={token}"));
            mail(
                &state,
                &email,
                "Confirm your mcpd account",
                format!(
                    "Open this link to confirm your address and log in:\n\n{url}\n\nIt works \
                     once, for 24 hours. If you did not sign up, ignore this mail and no account \
                     will be usable.\n"
                ),
                Some(&url),
            );
        }
        None => {
            let url = link(&state, "forgot");
            mail(
                &state,
                &email,
                "Your mcpd account",
                format!(
                    "Somebody tried to sign up with this address, and it already has an \
                     account. If it was you, log in, or reset your password here:\n\n{url}\n\n\
                     If it was not you, nothing has changed.\n"
                ),
                Some(&url),
            );
        }
    }
    Ok(Json(AccountNotice {
        message: format!(
            "Check your email. We sent a link to {email}; open it to confirm the address and log in."
        ),
    }))
}

/// `POST /v1/api/account/verify`
pub async fn verify_email(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Json(req): Json<TokenRequest>,
) -> ApiResult<Json<AccountSession>> {
    let accounts = store(&state)?;
    state
        .account_guards
        .tokens
        .hit(&client(&state, &headers, &peer))
        .map_err(|s| limited(s, "too many link attempts from this address"))?;
    let a = Arc::clone(&accounts);
    let token = req.token;
    let taken = blocking(move || a.take_token(TokenPurpose::Verify, &token)).await?;
    let Some((Some(account_id), _)) = taken else {
        return Err(bad_token());
    };
    let a = Arc::clone(&accounts);
    let id = account_id.clone();
    let account = blocking(move || {
        a.mark_verified(&id)?;
        a.account(&id)
    })
    .await?;
    if account.totp_enabled {
        return Err(ApiError::TotpRequired(
            "your address is confirmed; log in with your password and the code from your \
             authenticator app"
                .to_owned(),
        ));
    }
    let meta = session_meta(&state, &headers, &peer, "verify", None);
    Ok(Json(blocking(move || accounts.issue_session(&account_id, &meta)).await?))
}

fn bad_token() -> ApiError {
    ApiError::BadParam {
        field: "token".to_owned(),
        message: "that link is unknown, already used or expired; ask for a new one".to_owned(),
    }
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
                Some(record) if verify_password(&password, &record.password_hash) => Some(record),
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
    // A deactivated account answers like a wrong password: its organisation
    // ended it, and saying so tells a stranger the address exists.
    let Some(record) = verified.filter(|r| !r.disabled) else {
        let _ = guards.login_email.hit(&email);
        return Err(login_refused());
    };
    // Everything below is said only to somebody who knows the password.
    if !record.email_verified {
        if state.account_runtime.mailer.can_send() {
            let a = Arc::clone(&accounts);
            let id = record.account_id.clone();
            let token = blocking(move || {
                a.issue_token(TokenPurpose::Verify, Some(&id), None, VERIFY_TTL_SECONDS)
            })
            .await?;
            let url = link(&state, &format!("verify={token}"));
            mail(
                &state,
                &email,
                "Confirm your mcpd account",
                format!("Open this link to confirm your address and log in:\n\n{url}\n"),
                Some(&url),
            );
        }
        return Err(ApiError::EmailUnverified(format!(
            "confirm your address first; we sent a new link to {email}"
        )));
    }
    let a = Arc::clone(&accounts);
    let id = record.account_id.clone();
    let mail_addr = email.clone();
    if let Some(org) = blocking(move || a.sso_required_for(&id, &mail_addr)).await? {
        return Err(ApiError::SsoRequired(format!(
            "your organisation {org:?} requires single sign-on for {}; use Log in with SSO",
            email_domain(&email)
        )));
    }
    if record.totp_enabled {
        let Some(code) = req.totp_code.as_deref().map(str::trim).filter(|c| !c.is_empty()) else {
            return Err(ApiError::TotpRequired(
                "this account has two-step log-in on; send the code from your authenticator app"
                    .to_owned(),
            ));
        };
        if !check_second_factor(&accounts, &record.account_id, code).await? {
            let _ = guards.login_email.hit(&email);
            return Err(ApiError::TotpRequired(
                "that code is not right; try the next one".to_owned(),
            ));
        }
    }
    let meta = session_meta(&state, &headers, &peer, "password", None);
    let account_id = record.account_id;
    let session = blocking(move || accounts.issue_session(&account_id, &meta)).await?;
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

/// A key name, trimmed, or a 400 saying what is wrong with it.
pub(crate) fn key_name(raw: Option<&str>) -> ApiResult<Option<String>> {
    match raw.map(str::trim).filter(|n| !n.is_empty()) {
        None => Ok(None),
        Some(n) if n.chars().count() > MAX_KEY_NAME_CHARS => Err(ApiError::BadParam {
            field: "name".to_owned(),
            message: format!("a key name is at most {MAX_KEY_NAME_CHARS} characters"),
        }),
        Some(n) if n.chars().any(char::is_control) => Err(ApiError::BadParam {
            field: "name".to_owned(),
            message: "a key name cannot hold a control character".to_owned(),
        }),
        Some(n) => Ok(Some(n.to_owned())),
    }
}

/// `POST /v1/api/account/keys`
pub async fn create_account_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<CreateAccountKeyRequest>,
) -> ApiResult<Json<CreatedAccountKey>> {
    let (credential, _) = authorize_session(&state, &headers)?;
    let accounts = store(&state)?;
    let name = key_name(req.name.as_deref())?;
    let scope = scope_from_wire(req.scope);
    let project = match req.project.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
        Some(p) => p.to_owned(),
        None => accounts.home_project(&credential).ok_or_else(|| ApiError::BadParam {
            field: "project".to_owned(),
            message: "this account has no project of its own; name the project the key reaches"
                .to_owned(),
        })?,
    };
    // The same answer for a project that does not exist and one this account
    // cannot reach, so the refusal does not say which project names are real.
    let allowed = accounts.role_on(&credential.account_id, &project).is_some_and(|(org, role)| {
        role.permits(scope) && credential.session_org().is_none_or(|o| o == org)
    });
    if !allowed {
        return Err(ApiError::Forbidden(format!(
            "your role on the project {project:?} does not allow the {} scope",
            scope.as_str()
        )));
    }
    let created =
        blocking(move || accounts.create_key(&credential.account_id, &project, scope, name))
            .await?;
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

/// `POST /v1/api/account/password/forgot`
pub async fn forgot_password(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Json(req): Json<PasswordForgotRequest>,
) -> ApiResult<Json<AccountNotice>> {
    let accounts = store(&state)?;
    require_mail(&state)?;
    let email = normalise_email(&req.email)?;
    state
        .account_guards
        .tokens
        .hit(&client(&state, &headers, &peer))
        .map_err(|s| limited(s, "too many reset requests from this address"))?;
    let a = Arc::clone(&accounts);
    let wanted = email.clone();
    let found = blocking(move || a.find_login(&wanted)).await?;
    if let Some(record) = found.filter(|r| !r.disabled) {
        let a = Arc::clone(&accounts);
        let token = blocking(move || {
            a.issue_token(TokenPurpose::Reset, Some(&record.account_id), None, RESET_TTL_SECONDS)
        })
        .await?;
        let url = link(&state, &format!("reset={token}"));
        mail(
            &state,
            &email,
            "Reset your mcpd password",
            format!(
                "Open this link to choose a new password:\n\n{url}\n\nIt works once, for one \
                 hour. If you did not ask for this, ignore this mail; your password has not \
                 changed.\n"
            ),
            Some(&url),
        );
    }
    Ok(Json(AccountNotice {
        message: format!("If an account uses {email}, we sent it a link to reset the password."),
    }))
}

/// `POST /v1/api/account/password/reset`
pub async fn reset_password(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Json(req): Json<PasswordResetRequest>,
) -> ApiResult<Json<AccountSession>> {
    let accounts = store(&state)?;
    state
        .account_guards
        .tokens
        .hit(&client(&state, &headers, &peer))
        .map_err(|s| limited(s, "too many link attempts from this address"))?;
    check_password(&req.password)?;
    let a = Arc::clone(&accounts);
    let token = req.token.clone();
    let Some((Some(account_id), _)) =
        blocking(move || a.peek_token(TokenPurpose::Reset, &token)).await?
    else {
        return Err(bad_token());
    };
    // A reset proves the address, not the second factor. The token is only
    // peeked until the code is right, so a wrong code costs the person nothing.
    let a = Arc::clone(&accounts);
    let id = account_id.clone();
    let account = blocking(move || a.account(&id)).await?;
    if account.totp_enabled {
        let Some(code) = req.totp_code.as_deref().map(str::trim).filter(|c| !c.is_empty()) else {
            return Err(ApiError::TotpRequired(
                "this account has two-step log-in on; send the code from your authenticator app \
                 with the new password"
                    .to_owned(),
            ));
        };
        state
            .account_guards
            .sensitive
            .check(&account_id)
            .map_err(|s| limited(s, "too many failed checks for this account"))?;
        if !check_second_factor(&accounts, &account_id, code).await? {
            let _ = state.account_guards.sensitive.hit(&account_id);
            return Err(ApiError::TotpRequired(
                "that code is not right; try the next one".to_owned(),
            ));
        }
    }
    let a = Arc::clone(&accounts);
    let token = req.token;
    if blocking(move || a.take_token(TokenPurpose::Reset, &token)).await?.is_none() {
        return Err(bad_token());
    }
    let hash = hash_bounded(&state, req.password).await?;
    let meta = session_meta(&state, &headers, &peer, "reset", None);
    let session = blocking(move || {
        accounts.set_password(&account_id, &hash)?;
        accounts.mark_verified(&account_id)?;
        accounts.end_other_sessions(&account_id, None)?;
        accounts.issue_session(&account_id, &meta)
    })
    .await?;
    Ok(Json(session))
}

/// `POST /v1/api/account/password`
pub async fn change_password(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<PasswordChangeRequest>,
) -> ApiResult<Json<AccountNotice>> {
    let (credential, presented) = authorize_session(&state, &headers)?;
    let accounts = store(&state)?;
    check_password_field(&req.new_password, "new_password")?;
    let a = Arc::clone(&accounts);
    let id = credential.account_id.clone();
    let account = blocking(move || a.account(&id)).await?;
    require_full_session(&credential, &account)?;
    confirm_password(&state, &accounts, &credential.account_id, req.current_password).await?;
    let hash = hash_bounded(&state, req.new_password).await?;
    let id = credential.account_id;
    let ended = blocking(move || {
        accounts.set_password(&id, &hash)?;
        accounts.end_other_sessions(&id, Some(&presented))
    })
    .await?;
    Ok(Json(AccountNotice {
        message: format!("Your password is changed. {ended} other sessions were ended."),
    }))
}

/// `POST /v1/api/account/email`
pub async fn change_email(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<EmailChangeRequest>,
) -> ApiResult<Json<AccountNotice>> {
    let (credential, _) = authorize_session(&state, &headers)?;
    let accounts = store(&state)?;
    require_mail(&state)?;
    let email = normalise_email(&req.email)?;
    let a = Arc::clone(&accounts);
    let id = credential.account_id.clone();
    let account = blocking(move || a.account(&id)).await?;
    require_full_session(&credential, &account)?;
    if account.managed_by.is_some() {
        return Err(ApiError::Forbidden(
            "your organisation manages this account's address; ask its admin to change it at \
             your identity provider"
                .to_owned(),
        ));
    }
    confirm_password(&state, &accounts, &credential.account_id, req.password).await?;
    let a = Arc::clone(&accounts);
    let wanted = email.clone();
    let taken = blocking(move || a.account_id_for(&wanted)).await?.is_some();
    if taken {
        mail(
            &state,
            &email,
            "Your mcpd account",
            "Somebody asked to move another mcpd account to this address. This address already \
             has an account, so nothing has changed.\n"
                .to_owned(),
            None,
        );
    } else {
        let a = Arc::clone(&accounts);
        let id = credential.account_id.clone();
        let new = email.clone();
        let token = blocking(move || {
            a.issue_token(TokenPurpose::EmailChange, Some(&id), Some(&new), VERIFY_TTL_SECONDS)
        })
        .await?;
        let url = link(&state, &format!("email={token}"));
        mail(
            &state,
            &email,
            "Confirm your new mcpd address",
            format!("Open this link to move your mcpd account to this address:\n\n{url}\n\nIt works once, for 24 hours.\n"),
            Some(&url),
        );
    }
    mail(
        &state,
        &account.email,
        "Your mcpd address is changing",
        format!(
            "Somebody logged in to your account asked to change its address to {email}. The \
             change happens only if the link sent there is opened. If this was not you, change \
             your password.\n"
        ),
        None,
    );
    Ok(Json(AccountNotice {
        message: format!("We sent a link to {email}. Your address changes when you open it."),
    }))
}

/// `POST /v1/api/account/email/confirm`
pub async fn confirm_email(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Json(req): Json<TokenRequest>,
) -> ApiResult<Json<Account>> {
    let accounts = store(&state)?;
    state
        .account_guards
        .tokens
        .hit(&client(&state, &headers, &peer))
        .map_err(|s| limited(s, "too many link attempts from this address"))?;
    let a = Arc::clone(&accounts);
    let token = req.token;
    let Some((Some(account_id), Some(new_email))) =
        blocking(move || a.take_token(TokenPurpose::EmailChange, &token)).await?
    else {
        return Err(bad_token());
    };
    let a = Arc::clone(&accounts);
    let id = account_id.clone();
    let changed = blocking(move || a.change_email(&id, &new_email)).await?;
    if !changed {
        return Err(ApiError::Conflict(
            "another account took that address in the meantime; nothing has changed".to_owned(),
        ));
    }
    Ok(Json(blocking(move || accounts.account(&account_id)).await?))
}

/// `POST /v1/api/account/delete`
pub async fn delete_account(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Json(req): Json<AccountDeleteRequest>,
) -> ApiResult<Json<AccountDeletion>> {
    let (credential, _) = authorize_session(&state, &headers)?;
    let accounts = store(&state)?;
    let a = Arc::clone(&accounts);
    let id = credential.account_id.clone();
    let account = blocking(move || a.account(&id)).await?;
    require_full_session(&credential, &account)?;
    confirm_password(&state, &accounts, &credential.account_id, req.password).await?;
    if account.totp_enabled {
        let code = req.totp_code.unwrap_or_default();
        if !check_second_factor(&accounts, &credential.account_id, &code).await? {
            let _ = state.account_guards.sensitive.hit(&credential.account_id);
            return Err(ApiError::Forbidden(
                "that code is not right; deleting needs the code from your authenticator app"
                    .to_owned(),
            ));
        }
    }
    let a = Arc::clone(&accounts);
    let id = credential.account_id.clone();
    let plan = blocking(move || a.deletion_plan(&id)).await?;
    if let Some(blocked) = plan.blocked {
        return Err(ApiError::Conflict(format!(
            "you are the only owner of {blocked:?}, which has other members; make one of them an \
             owner, or remove them, before deleting your account"
        )));
    }
    let mut erased = Vec::new();
    for (org_id, projects) in &plan.orgs {
        for project in projects {
            crate::orgs::erase_project(&state, project).await;
            erased.push(project.clone());
        }
        crate::query::audited_as(
            &state,
            &headers,
            peer.0,
            Ok(()),
            "org_delete",
            org_id,
            None,
            &account.email,
        )
        .await?;
    }
    let orgs: Vec<String> = plan.orgs.iter().map(|(o, _)| o.clone()).collect();
    let n = u32::try_from(orgs.len()).unwrap_or(u32::MAX);
    let a = Arc::clone(&accounts);
    let id = credential.account_id.clone();
    blocking(move || a.delete_account(&id, &orgs)).await?;
    crate::orgs::sync_quotas(&state);
    Ok(Json(AccountDeletion { orgs_deleted: n, projects_erased: erased }))
}

/// `GET /v1/api/account/sessions`
pub async fn list_sessions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> ApiResult<Json<Vec<SessionInfo>>> {
    let (credential, presented) = authorize_session(&state, &headers)?;
    let accounts = store(&state)?;
    Ok(Json(blocking(move || accounts.list_sessions(&credential.account_id, &presented)).await?))
}

/// `DELETE /v1/api/account/sessions/{session_id}`
pub async fn revoke_session(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
) -> ApiResult<Json<DeleteResult>> {
    let (credential, _) = authorize_session(&state, &headers)?;
    let accounts = store(&state)?;
    let wanted = session_id.clone();
    let n = blocking(move || accounts.end_session_by_id(&credential.account_id, &wanted)).await?;
    if n == 0 {
        return Err(ApiError::NotFound(format!("this account has no session {session_id:?}")));
    }
    Ok(Json(DeleteResult { deleted: n }))
}

/// `POST /v1/api/account/totp/setup`
pub async fn totp_setup(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> ApiResult<Json<TotpSetup>> {
    let (credential, _) = authorize_session(&state, &headers)?;
    let accounts = store(&state)?;
    let a = Arc::clone(&accounts);
    let id = credential.account_id.clone();
    let account = blocking(move || a.account(&id)).await?;
    require_full_session(&credential, &account)?;
    if account.totp_enabled {
        return Err(ApiError::Conflict(
            "two-step log-in is already on; turn it off first to change the app".to_owned(),
        ));
    }
    let secret = crate::totp::new_secret();
    let s = secret.clone();
    let id = credential.account_id;
    blocking(move || accounts.set_totp_pending(&id, &s)).await?;
    let uri = crate::totp::otpauth_uri("mcpd", &account.email, &secret);
    let qr_svg = crate::totp::qr_svg(&uri);
    Ok(Json(TotpSetup { secret, otpauth_uri: uri, qr_svg }))
}

/// `POST /v1/api/account/totp/enable`
pub async fn totp_enable(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<TotpCodeRequest>,
) -> ApiResult<Json<TotpEnabled>> {
    let (credential, _) = authorize_session(&state, &headers)?;
    let accounts = store(&state)?;
    let a = Arc::clone(&accounts);
    let id = credential.account_id.clone();
    let account = blocking(move || a.account(&id)).await?;
    require_full_session(&credential, &account)?;
    state
        .account_guards
        .sensitive
        .check(&credential.account_id)
        .map_err(|s| limited(s, "too many failed checks for this account"))?;
    let codes: Vec<String> = (0..RECOVERY_CODES)
        .map(|_| {
            let b: [u8; 5] = rand::random();
            let h = hex::encode(b);
            format!("{}-{}", &h[..5], &h[5..])
        })
        .collect();
    let digests: Vec<String> = codes.iter().map(|c| digest_hex(c)).collect();
    let a = Arc::clone(&accounts);
    let id = credential.account_id.clone();
    let code = req.code;
    let outcome = blocking(move || a.enable_totp(&id, &code, &digests, now_secs())).await?;
    match outcome {
        lifecycle::TotpEnable::Enabled => Ok(Json(TotpEnabled { recovery_codes: codes })),
        lifecycle::TotpEnable::AlreadyOn => {
            Err(ApiError::Conflict("two-step log-in is already on".to_owned()))
        }
        lifecycle::TotpEnable::NothingPending => Err(ApiError::Conflict(
            "there is no secret waiting to be confirmed; start again with \
             POST /v1/api/account/totp/setup"
                .to_owned(),
        )),
        lifecycle::TotpEnable::WrongCode => {
            let _ = state.account_guards.sensitive.hit(&credential.account_id);
            Err(ApiError::Forbidden(
                "that code is not right; check the time on the device running the app".to_owned(),
            ))
        }
    }
}

/// `POST /v1/api/account/totp/disable`
pub async fn totp_disable(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<TotpDisableRequest>,
) -> ApiResult<Json<AccountNotice>> {
    let (credential, _) = authorize_session(&state, &headers)?;
    let accounts = store(&state)?;
    let a = Arc::clone(&accounts);
    let id = credential.account_id.clone();
    let account = blocking(move || a.account(&id)).await?;
    require_full_session(&credential, &account)?;
    if !account.totp_enabled {
        return Ok(Json(AccountNotice { message: "Two-step log-in was already off.".to_owned() }));
    }
    confirm_password(&state, &accounts, &credential.account_id, req.password).await?;
    if !check_second_factor(&accounts, &credential.account_id, &req.code).await? {
        let _ = state.account_guards.sensitive.hit(&credential.account_id);
        return Err(ApiError::Forbidden("that code is not right".to_owned()));
    }
    let id = credential.account_id;
    blocking(move || accounts.disable_totp(&id)).await?;
    Ok(Json(AccountNotice { message: "Two-step log-in is off.".to_owned() }))
}

/// The account lifecycle routes beyond sign-up, log-in and keys. Merged into
/// the query API's router by `lib.rs`, so they share its origin policy.
pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/v1/api/account/verify", post(verify_email))
        .route("/v1/api/account/password/forgot", post(forgot_password))
        .route("/v1/api/account/password/reset", post(reset_password))
        .route("/v1/api/account/password", post(change_password))
        .route("/v1/api/account/email", post(change_email))
        .route("/v1/api/account/email/confirm", post(confirm_email))
        .route("/v1/api/account/delete", post(delete_account))
        .route("/v1/api/account/sessions", get(list_sessions))
        .route("/v1/api/account/sessions/{session_id}", delete(revoke_session))
        .route("/v1/api/account/totp/setup", post(totp_setup))
        .route("/v1/api/account/totp/enable", post(totp_enable))
        .route("/v1/api/account/totp/disable", post(totp_disable))
}

/// What the daemon holds for accounts beyond the store: the mailer, the
/// provider cache single sign-on reads, and when counting started.
pub struct AccountRuntime {
    /// Where mail goes.
    pub mailer: crate::mail::Mailer,
    /// Discovery documents and signing keys of single sign-on providers.
    pub providers: crate::sso::ProviderCache,
}

impl AccountRuntime {
    /// Build from the settings. A mail configuration that cannot be used
    /// refuses mail rather than writing links to the log, because `main.rs`
    /// has already refused to start with one and only an embedder reaches
    /// this with a bad one.
    #[must_use]
    pub fn new(settings: &AccountSettings) -> Self {
        let mailer = crate::mail::Mailer::new(settings.mail.clone()).unwrap_or_else(|e| {
            tracing::error!("mail is off: {e}");
            crate::mail::Mailer::new(crate::mail::MailConfig::Refuse)
                .unwrap_or_else(|_| crate::mail::Mailer::log())
        });
        Self { mailer, providers: crate::sso::ProviderCache::default() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta() -> SessionMeta {
        SessionMeta { via: "password", ..SessionMeta::default() }
    }

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
        assert!(!verify_password("anything", &no_password()));
    }

    fn signed_up(store: &AccountStore, email: &str) -> (String, AccountSession) {
        let id = store.create_account(email, "$argon2id$x").unwrap().unwrap();
        store.mark_verified(&id).unwrap();
        let s = store.issue_session(&id, &meta()).unwrap();
        (id, s)
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
        let (id, session) = signed_up(&store, "ana@example.com");
        let project = session.project.clone().unwrap();
        let key = store.create_key(&id, &project, Scope::Ingest, None).unwrap().unwrap();
        let token = store.issue_token(TokenPurpose::Reset, Some(&id), None, 60).unwrap();
        drop(store);
        let bytes = std::fs::read(path).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        let wal = std::fs::read(format!("{path}-wal")).unwrap_or_default();
        let wal = String::from_utf8_lossy(&wal);
        for secret in [&session.token, &key.secret, &token] {
            assert!(!text.contains(secret.as_str()) && !wal.contains(secret.as_str()));
        }
        // Reopened from the file, both still work.
        let store = AccountStore::open(path).unwrap();
        assert!(store.lookup(&session.token).is_some());
        assert!(store.lookup(&key.secret).is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_from_before_organisations_is_brought_up_to_date() {
        let dir = std::env::temp_dir().join(format!(
            "mcpd-accounts-v1-{}-{}",
            std::process::id(),
            OffsetDateTime::now_utc().unix_timestamp_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("old.db");
        let path = path.to_str().unwrap();
        let session_secret = "mcpds_old_session_secret";
        let key_secret = "mcpdk_old_key_secret";
        {
            let conn = Connection::open(path).unwrap();
            conn.execute_batch(schema::SCHEMA_V1).unwrap();
            conn.execute(
                "INSERT INTO accounts VALUES ('acct_aaaaaaaaaaaaaaaaaaaaaaaa', 'old@example.com', '$argon2id$x', 'prj_old', 1)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO account_keys VALUES ('key_1', 'acct_aaaaaaaaaaaaaaaaaaaaaaaa', 'read', NULL, 'mcpdk_old', ?1, 1)",
                [digest_hex(key_secret)],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO account_sessions VALUES (?1, 'acct_aaaaaaaaaaaaaaaaaaaaaaaa', 1, ?2)",
                params![digest_hex(session_secret), now_secs() + 3600],
            )
            .unwrap();
        }
        let store = AccountStore::open(path).unwrap();
        let session = store.lookup(session_secret).expect("the old session still works");
        let key = store.lookup(key_secret).expect("the old key still works");
        assert_eq!(store.reach(&key, Scope::Read), BTreeSet::from(["prj_old".to_owned()]));
        assert_eq!(store.reach(&session, Scope::Admin), BTreeSet::from(["prj_old".to_owned()]));
        assert_eq!(
            store.role_in("acct_aaaaaaaaaaaaaaaaaaaaaaaa", "org_aaaaaaaaaaaaaaaaaaaaaaaa"),
            Some(Role::Owner)
        );
        let account = store.account("acct_aaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
        assert!(
            account.email_verified,
            "an account from before confirmation is taken as confirmed"
        );
        drop(store);
        // Opening again changes nothing.
        let store = AccountStore::open(path).unwrap();
        assert!(store.lookup(key_secret).is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_revoked_key_and_an_ended_session_stop_working() {
        let store = AccountStore::open(":memory:").unwrap();
        let (id, s) = signed_up(&store, "ana@example.com");
        let k = store
            .create_key(&id, s.project.as_deref().unwrap(), Scope::Read, Some("ci".into()))
            .unwrap()
            .unwrap();
        assert!(store.lookup(&k.secret).is_some());
        assert!(store.revoke_key(&id, &k.key.id).unwrap());
        assert!(store.lookup(&k.secret).is_none());
        assert_eq!(store.end_session(&id, &s.token).unwrap(), 1);
        assert!(store.lookup(&s.token).is_none());
    }

    #[test]
    fn one_account_cannot_revoke_another_accounts_key() {
        let store = AccountStore::open(":memory:").unwrap();
        let (a_id, a) = signed_up(&store, "a@example.com");
        let (b_id, b) = signed_up(&store, "b@example.com");
        assert_ne!(a.project, b.project);
        let key = store
            .create_key(&a_id, a.project.as_deref().unwrap(), Scope::Ingest, None)
            .unwrap()
            .unwrap();
        assert!(!store.revoke_key(&b_id, &key.key.id).unwrap());
        assert!(store.lookup(&key.secret).is_some());
    }

    #[test]
    fn an_email_has_one_account() {
        let store = AccountStore::open(":memory:").unwrap();
        assert!(store.create_account("a@example.com", "$argon2id$x").unwrap().is_some());
        assert!(store.create_account("a@example.com", "$argon2id$y").unwrap().is_none());
        assert!(store.create_member_account("a@example.com", None, None).unwrap().is_none());
    }

    #[test]
    fn sessions_past_the_cap_end_the_oldest() {
        let store = AccountStore::open(":memory:").unwrap();
        let (id, first) = signed_up(&store, "a@example.com");
        for _ in 0..MAX_SESSIONS_PER_ACCOUNT {
            store.issue_session(&id, &meta()).unwrap();
        }
        assert!(store.lookup(&first.token).is_none(), "the oldest session outlived the cap");
    }

    #[test]
    fn the_key_cap_is_enforced() {
        let store = AccountStore::open(":memory:").unwrap();
        let (id, s) = signed_up(&store, "a@example.com");
        let p = s.project.unwrap();
        for _ in 0..MAX_KEYS_PER_ACCOUNT {
            assert!(store.create_key(&id, &p, Scope::Ingest, None).unwrap().is_some());
        }
        assert!(store.create_key(&id, &p, Scope::Ingest, None).unwrap().is_none());
    }

    #[test]
    fn a_key_grants_its_scope_and_an_admin_key_also_the_run_scopes() {
        assert!(Credential::key_allows(Scope::Ingest, Scope::Ingest));
        assert!(!Credential::key_allows(Scope::Ingest, Scope::Read));
        assert!(!Credential::key_allows(Scope::Read, Scope::Admin));
        assert!(Credential::key_allows(Scope::Admin, Scope::Execute));
        assert!(Credential::key_allows(Scope::Admin, Scope::Worker));
        assert!(!Credential::key_allows(Scope::Admin, Scope::Read));
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

    #[test]
    fn a_one_time_token_works_once_and_only_for_its_purpose() {
        let store = AccountStore::open(":memory:").unwrap();
        let (id, _) = signed_up(&store, "a@example.com");
        let t = store.issue_token(TokenPurpose::Reset, Some(&id), None, 60).unwrap();
        assert!(store.take_token(TokenPurpose::Verify, &t).unwrap().is_none());
        assert!(store.peek_token(TokenPurpose::Reset, &t).unwrap().is_some());
        assert_eq!(
            store.take_token(TokenPurpose::Reset, &t).unwrap().unwrap().0.as_deref(),
            Some(id.as_str())
        );
        assert!(store.take_token(TokenPurpose::Reset, &t).unwrap().is_none());
        let expired = store.issue_token(TokenPurpose::Reset, Some(&id), None, -1).unwrap();
        assert!(store.take_token(TokenPurpose::Reset, &expired).unwrap().is_none());
        // A new link replaces the last one of the same purpose.
        let first = store.issue_token(TokenPurpose::Reset, Some(&id), None, 60).unwrap();
        let _second = store.issue_token(TokenPurpose::Reset, Some(&id), None, 60).unwrap();
        assert!(store.take_token(TokenPurpose::Reset, &first).unwrap().is_none());
    }

    #[test]
    fn a_used_totp_step_cannot_be_replayed_and_a_recovery_code_works_once() {
        let store = AccountStore::open(":memory:").unwrap();
        let (id, _) = signed_up(&store, "a@example.com");
        let secret = crate::totp::new_secret();
        store.set_totp_pending(&id, &secret).unwrap();
        let now = 1_700_000_000;
        let raw = crate::totp::base32_decode(&secret).unwrap();
        let code = format!("{:06}", crate::totp::code_at(&raw, crate::totp::step_of(now as u64)));
        let digests = vec![digest_hex("aaaaa-bbbbb")];
        assert_eq!(
            store.enable_totp(&id, &code, &digests, now).unwrap(),
            lifecycle::TotpEnable::Enabled
        );
        assert!(
            !store.use_second_factor(&id, &code, now).unwrap(),
            "the code that enabled it was replayed"
        );
        let next =
            format!("{:06}", crate::totp::code_at(&raw, crate::totp::step_of(now as u64) + 1));
        assert!(store.use_second_factor(&id, &next, now + 30).unwrap());
        assert!(store.use_second_factor(&id, "aaaaa-bbbbb", now).unwrap());
        assert!(!store.use_second_factor(&id, "aaaaa-bbbbb", now).unwrap());
    }
}
