//! The account store's tables, and the step that brings an older file up to
//! date.
//!
//! The version lives in `PRAGMA user_version`. Version 0 is a file from before
//! organisations existed: one account, one project, keys and sessions. The
//! original statements are kept exactly as they were and run first on every
//! open, because they are idempotent and they are what a fresh file needs before
//! the migration has anything to alter.

use rusqlite::Connection;

/// The tables as the first release made them. Never edited: a change goes in a
/// migration below.
pub(super) const SCHEMA_V1: &str = "
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

/// Version 2: organisations, roles, single sign-on, SCIM and the account
/// lifecycle.
///
/// `accounts` is rebuilt rather than altered, because its `project` column
/// loses `NOT NULL`: an account created by an invitation, single sign-on or
/// SCIM has no project of its own. SQLite cannot drop a constraint in place.
///
/// Every account that existed before gets an organisation of its own, owning
/// its project, with it as the owner. Its organisation id is its account id
/// under another prefix, so the step needs no random numbers and running it
/// against a copy gives the same file. Its address is taken as confirmed: it
/// was accepted before confirmation existed, and refusing those accounts their
/// next log-in would lock out everyone who signed up in good faith.
const MIGRATION_V2: &str = "
CREATE TABLE accounts_v2 (
    id                  TEXT PRIMARY KEY,
    email               TEXT NOT NULL UNIQUE,
    password_hash       TEXT NOT NULL,
    project             TEXT UNIQUE,
    created_at          INTEGER NOT NULL,
    email_verified_at   INTEGER,
    totp_secret         TEXT,
    totp_pending        TEXT,
    totp_last_step      INTEGER,
    disabled_at         INTEGER,
    managed_by          TEXT
);
INSERT INTO accounts_v2 (id, email, password_hash, project, created_at, email_verified_at)
    SELECT id, email, password_hash, project, created_at, created_at FROM accounts;
DROP TABLE accounts;
ALTER TABLE accounts_v2 RENAME TO accounts;

ALTER TABLE account_keys ADD COLUMN project TEXT;
UPDATE account_keys SET project = (SELECT a.project FROM accounts a WHERE a.id = account_keys.account_id);
CREATE INDEX account_keys_project ON account_keys (project);

ALTER TABLE account_sessions ADD COLUMN id TEXT;
ALTER TABLE account_sessions ADD COLUMN org_id TEXT;
ALTER TABLE account_sessions ADD COLUMN via TEXT NOT NULL DEFAULT 'password';
ALTER TABLE account_sessions ADD COLUMN user_agent TEXT;
ALTER TABLE account_sessions ADD COLUMN address TEXT;
UPDATE account_sessions SET id = 'ses_' || lower(hex(randomblob(8)));
CREATE UNIQUE INDEX account_sessions_id ON account_sessions (id);

CREATE TABLE orgs (
    id                TEXT PRIMARY KEY,
    name              TEXT NOT NULL,
    created_at        INTEGER NOT NULL,
    quota_per_minute  INTEGER,
    retention_days    INTEGER,
    config_managed    INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE org_members (
    org_id     TEXT NOT NULL REFERENCES orgs(id) ON DELETE CASCADE,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    role       TEXT NOT NULL,
    via        TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (org_id, account_id)
);
CREATE INDEX org_members_account ON org_members (account_id);
CREATE TABLE org_projects (
    project    TEXT PRIMARY KEY,
    org_id     TEXT NOT NULL REFERENCES orgs(id) ON DELETE CASCADE,
    label      TEXT,
    created_at INTEGER NOT NULL
);
CREATE INDEX org_projects_org ON org_projects (org_id);
CREATE TABLE project_roles (
    project    TEXT NOT NULL REFERENCES org_projects(project) ON DELETE CASCADE,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    role       TEXT NOT NULL,
    PRIMARY KEY (project, account_id)
);
CREATE TABLE org_invites (
    id            TEXT PRIMARY KEY,
    org_id        TEXT NOT NULL REFERENCES orgs(id) ON DELETE CASCADE,
    email         TEXT NOT NULL,
    role          TEXT NOT NULL,
    project_roles TEXT NOT NULL DEFAULT '[]',
    digest        TEXT NOT NULL UNIQUE,
    invited_by    TEXT,
    scim_user_id  TEXT,
    created_at    INTEGER NOT NULL,
    expires_at    INTEGER NOT NULL
);
CREATE INDEX org_invites_org ON org_invites (org_id);
CREATE TABLE account_tokens (
    digest     TEXT PRIMARY KEY,
    purpose    TEXT NOT NULL,
    account_id TEXT REFERENCES accounts(id) ON DELETE CASCADE,
    data       TEXT,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
);
CREATE INDEX account_tokens_account ON account_tokens (account_id, purpose);
CREATE TABLE recovery_codes (
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    digest     TEXT NOT NULL,
    PRIMARY KEY (account_id, digest)
);
CREATE TABLE org_sso (
    org_id         TEXT PRIMARY KEY REFERENCES orgs(id) ON DELETE CASCADE,
    provider       TEXT NOT NULL,
    issuer         TEXT,
    client_id      TEXT NOT NULL,
    client_secret  TEXT,
    enforced       INTEGER NOT NULL DEFAULT 0,
    default_role   TEXT NOT NULL,
    groups_claim   TEXT,
    config_managed INTEGER NOT NULL DEFAULT 0,
    updated_at     INTEGER NOT NULL
);
CREATE TABLE org_domains (
    domain TEXT PRIMARY KEY,
    org_id TEXT NOT NULL REFERENCES orgs(id) ON DELETE CASCADE
);
CREATE TABLE org_group_roles (
    org_id     TEXT NOT NULL REFERENCES orgs(id) ON DELETE CASCADE,
    group_name TEXT NOT NULL,
    role       TEXT NOT NULL,
    PRIMARY KEY (org_id, group_name)
);
CREATE TABLE sso_identities (
    org_id     TEXT NOT NULL REFERENCES orgs(id) ON DELETE CASCADE,
    subject    TEXT NOT NULL,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (org_id, subject)
);
CREATE TABLE org_scim (
    org_id     TEXT PRIMARY KEY REFERENCES orgs(id) ON DELETE CASCADE,
    digest     TEXT NOT NULL UNIQUE,
    prefix     TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE TABLE scim_users (
    id           TEXT PRIMARY KEY,
    org_id       TEXT NOT NULL REFERENCES orgs(id) ON DELETE CASCADE,
    user_name    TEXT NOT NULL,
    external_id  TEXT,
    display_name TEXT,
    given_name   TEXT,
    family_name  TEXT,
    email        TEXT NOT NULL,
    active       INTEGER NOT NULL DEFAULT 1,
    account_id   TEXT REFERENCES accounts(id) ON DELETE SET NULL,
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL,
    UNIQUE (org_id, user_name)
);
CREATE TABLE scim_groups (
    id           TEXT PRIMARY KEY,
    org_id       TEXT NOT NULL REFERENCES orgs(id) ON DELETE CASCADE,
    display_name TEXT NOT NULL,
    external_id  TEXT,
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL,
    UNIQUE (org_id, display_name)
);
CREATE TABLE scim_group_members (
    group_id TEXT NOT NULL REFERENCES scim_groups(id) ON DELETE CASCADE,
    user_id  TEXT NOT NULL REFERENCES scim_users(id) ON DELETE CASCADE,
    PRIMARY KEY (group_id, user_id)
);

INSERT INTO orgs (id, name, created_at)
    SELECT 'org_' || substr(id, 6), 'Personal', created_at FROM accounts;
INSERT INTO org_members (org_id, account_id, role, via, created_at)
    SELECT 'org_' || substr(id, 6), id, 'owner', 'created', created_at FROM accounts;
INSERT INTO org_projects (project, org_id, created_at)
    SELECT project, 'org_' || substr(id, 6), created_at FROM accounts WHERE project IS NOT NULL;
";

/// The version this build writes.
pub(super) const VERSION: i64 = 2;

/// Create or bring up to date. Foreign keys are switched off around the
/// rebuild, as SQLite's own procedure for changing a table requires: with them
/// on, dropping the old `accounts` table would cascade into every key and
/// session.
pub(super) fn migrate(conn: &mut Connection) -> rusqlite::Result<()> {
    conn.execute_batch(SCHEMA_V1)?;
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version >= VERSION {
        return Ok(());
    }
    conn.execute_batch("PRAGMA foreign_keys = OFF;")?;
    let result = (|| {
        let tx = conn.transaction()?;
        tx.execute_batch(MIGRATION_V2)?;
        // A broken reference here would be a key or a session pointing at an
        // account that is not there; refusing is better than carrying it.
        let broken: i64 =
            tx.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| r.get(0))?;
        if broken > 0 {
            return Err(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CONSTRAINT),
                Some(format!("{broken} rows reference an account that does not exist")),
            ));
        }
        tx.execute_batch(&format!("PRAGMA user_version = {VERSION};"))?;
        tx.commit()
    })();
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    result
}
