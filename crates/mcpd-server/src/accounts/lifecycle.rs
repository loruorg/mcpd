//! The account lifecycle in the store: one-time tokens, confirmation,
//! passwords, addresses, sessions, two-step log-in and deletion.

use super::{at, digest, digest_hex, now_secs, parse_digest, secret, AccountError, AccountStore};
use mcpd_core::SessionInfo;
use rusqlite::{params, OptionalExtension};

/// A used one-time token: its account and its data.
pub type TokenRow = (Option<String>, Option<String>);

/// What a one-time token is for. A token of one purpose is never accepted for
/// another, so a link from an invitation cannot reset a password.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenPurpose {
    /// Confirms the address after sign-up, and logs in.
    Verify,
    /// Sets a new password.
    Reset,
    /// Moves the account to a new address, held in the token's data.
    EmailChange,
    /// The state of a single sign-on flow in progress.
    SsoState,
}

impl TokenPurpose {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Verify => "verify",
            Self::Reset => "reset",
            Self::EmailChange => "email",
            Self::SsoState => "sso",
        }
    }

    const fn prefix(self) -> &'static str {
        match self {
            Self::Verify => "mcpdv_",
            Self::Reset => "mcpdr_",
            Self::EmailChange => "mcpde_",
            Self::SsoState => "",
        }
    }
}

/// A login's stored half: what to verify the password against, and what to
/// decide once it matches.
pub struct LoginRecord {
    /// The account.
    pub account_id: String,
    /// Its argon2id hash.
    pub password_hash: String,
    /// Whether the address is confirmed.
    pub email_verified: bool,
    /// Whether two-step log-in is on.
    pub totp_enabled: bool,
    /// Deactivated by its organisation.
    pub disabled: bool,
}

/// The outcome of confirming a new authenticator secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TotpEnable {
    /// Two-step log-in is on.
    Enabled,
    /// It was on already.
    AlreadyOn,
    /// No secret is waiting.
    NothingPending,
    /// The code did not match.
    WrongCode,
}

/// What deleting an account takes with it.
#[derive(Debug, Default)]
pub struct DeletionPlan {
    /// Organisations it is the only member of, with their projects.
    pub orgs: Vec<(String, Vec<String>)>,
    /// The name of an organisation it is the only owner of with other members.
    /// Deletion is refused while this is set.
    pub blocked: Option<String>,
}

impl AccountStore {
    /// The stored half of a log-in, or `None` for an email with no account.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be read.
    pub fn find_login(&self, email: &str) -> Result<Option<LoginRecord>, AccountError> {
        let conn = self.conn()?;
        Ok(conn
            .query_row(
                "SELECT id, password_hash, email_verified_at, totp_secret, disabled_at
                 FROM accounts WHERE email = ?1",
                [email],
                |r| {
                    Ok(LoginRecord {
                        account_id: r.get(0)?,
                        password_hash: r.get(1)?,
                        email_verified: r.get::<_, Option<i64>>(2)?.is_some(),
                        totp_enabled: r.get::<_, Option<String>>(3)?.is_some(),
                        disabled: r.get::<_, Option<i64>>(4)?.is_some(),
                    })
                },
            )
            .optional()?)
    }

    /// The password hash of an account. Starts with `!` when it has none.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be read.
    pub fn password_hash(&self, account_id: &str) -> Result<String, AccountError> {
        let conn = self.conn()?;
        Ok(conn.query_row(
            "SELECT password_hash FROM accounts WHERE id = ?1",
            [account_id],
            |r| r.get(0),
        )?)
    }

    /// Issue a one-time token and return its secret. Any earlier token of the
    /// same purpose for the same account stops working, so only the newest
    /// link in somebody's inbox is live.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn issue_token(
        &self,
        purpose: TokenPurpose,
        account_id: Option<&str>,
        data: Option<&str>,
        ttl_seconds: i64,
    ) -> Result<String, AccountError> {
        let token = secret(purpose.prefix());
        let now = now_secs();
        let conn = self.conn()?;
        if let Some(id) = account_id {
            conn.execute(
                "DELETE FROM account_tokens WHERE account_id = ?1 AND purpose = ?2",
                params![id, purpose.as_str()],
            )?;
        }
        conn.execute(
            "INSERT INTO account_tokens (digest, purpose, account_id, data, created_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![digest_hex(&token), purpose.as_str(), account_id, data, now, now + ttl_seconds],
        )?;
        Ok(token)
    }

    fn token_row(
        &self,
        purpose: TokenPurpose,
        presented: &str,
        consume: bool,
    ) -> Result<Option<TokenRow>, AccountError> {
        let d = digest_hex(presented.trim());
        let conn = self.conn()?;
        let row: Option<(Option<String>, Option<String>, i64)> = conn
            .query_row(
                "SELECT account_id, data, expires_at FROM account_tokens
                 WHERE digest = ?1 AND purpose = ?2",
                params![d, purpose.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let Some((account, data, expires)) = row else { return Ok(None) };
        if consume || expires <= now_secs() {
            conn.execute("DELETE FROM account_tokens WHERE digest = ?1", [&d])?;
        }
        if expires <= now_secs() {
            return Ok(None);
        }
        Ok(Some((account, data)))
    }

    /// Use a one-time token: its account and data, once, or `None` when it is
    /// unknown, of another purpose, used or expired.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn take_token(
        &self,
        purpose: TokenPurpose,
        presented: &str,
    ) -> Result<Option<TokenRow>, AccountError> {
        self.token_row(purpose, presented, true)
    }

    /// Read a one-time token without using it.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be read.
    pub fn peek_token(
        &self,
        purpose: TokenPurpose,
        presented: &str,
    ) -> Result<Option<TokenRow>, AccountError> {
        self.token_row(purpose, presented, false)
    }

    /// Mark an account's address confirmed.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn mark_verified(&self, account_id: &str) -> Result<(), AccountError> {
        self.conn()?.execute(
            "UPDATE accounts SET email_verified_at = COALESCE(email_verified_at, ?2) WHERE id = ?1",
            params![account_id, now_secs()],
        )?;
        Ok(())
    }

    /// Store a new password hash. Every reset link outstanding stops working.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn set_password(&self, account_id: &str, hash: &str) -> Result<(), AccountError> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE accounts SET password_hash = ?2 WHERE id = ?1",
            params![account_id, hash],
        )?;
        conn.execute(
            "DELETE FROM account_tokens WHERE account_id = ?1 AND purpose = 'reset'",
            [account_id],
        )?;
        Ok(())
    }

    /// Move an account to a new address. `false` when another account has it.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn change_email(&self, account_id: &str, email: &str) -> Result<bool, AccountError> {
        let conn = self.conn()?;
        let taken: Option<String> = conn
            .query_row("SELECT id FROM accounts WHERE email = ?1", [email], |r| r.get(0))
            .optional()?;
        if taken.is_some_and(|t| t != account_id) {
            return Ok(false);
        }
        conn.execute(
            "UPDATE accounts SET email = ?2, email_verified_at = ?3 WHERE id = ?1",
            params![account_id, email, now_secs()],
        )?;
        Ok(true)
    }

    /// Every live session of an account, newest first.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be read.
    pub fn list_sessions(
        &self,
        account_id: &str,
        presented: &str,
    ) -> Result<Vec<SessionInfo>, AccountError> {
        let current = digest_hex(presented);
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, created_at, expires_at, digest, via, org_id, user_agent, address
             FROM account_sessions WHERE account_id = ?1 AND expires_at > ?2
             ORDER BY created_at DESC, rowid DESC",
        )?;
        let rows = stmt.query_map(params![account_id, now_secs()], |r| {
            Ok(SessionInfo {
                id: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                created_at: at(r.get(1)?),
                expires_at: at(r.get(2)?),
                current: r.get::<_, String>(3)? == current,
                via: r.get(4)?,
                org_id: r.get(5)?,
                user_agent: r.get(6)?,
                address: r.get(7)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// End one session by its public id. The account is part of the match.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn end_session_by_id(&self, account_id: &str, id: &str) -> Result<u32, AccountError> {
        self.end_sessions_where(account_id, "id = ?2", Some(id))
    }

    /// Hold a new authenticator secret until a code confirms it.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn set_totp_pending(&self, account_id: &str, secret_b32: &str) -> Result<(), AccountError> {
        self.conn()?.execute(
            "UPDATE accounts SET totp_pending = ?2 WHERE id = ?1",
            params![account_id, secret_b32],
        )?;
        Ok(())
    }

    /// Confirm the waiting secret with a code and turn two-step log-in on, with
    /// these recovery code digests.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn enable_totp(
        &self,
        account_id: &str,
        code: &str,
        recovery_digests: &[String],
        unix_now: i64,
    ) -> Result<TotpEnable, AccountError> {
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        let (on, pending): (Option<String>, Option<String>) = tx.query_row(
            "SELECT totp_secret, totp_pending FROM accounts WHERE id = ?1",
            [account_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if on.is_some() {
            return Ok(TotpEnable::AlreadyOn);
        }
        let Some(pending) = pending else { return Ok(TotpEnable::NothingPending) };
        let Some(step) = crate::totp::verify(&pending, code, u64::try_from(unix_now).unwrap_or(0))
        else {
            return Ok(TotpEnable::WrongCode);
        };
        tx.execute(
            "UPDATE accounts SET totp_secret = totp_pending, totp_pending = NULL,
                                 totp_last_step = ?2 WHERE id = ?1",
            params![account_id, i64::try_from(step).unwrap_or(i64::MAX)],
        )?;
        tx.execute("DELETE FROM recovery_codes WHERE account_id = ?1", [account_id])?;
        for d in recovery_digests {
            tx.execute(
                "INSERT INTO recovery_codes (account_id, digest) VALUES (?1, ?2)",
                params![account_id, d],
            )?;
        }
        tx.commit()?;
        Ok(TotpEnable::Enabled)
    }

    /// Check a code from the app, or a recovery code, and use it up. A code
    /// from a step at or before the last one used is refused, so it cannot be
    /// replayed. `false` for anything that does not match.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn use_second_factor(
        &self,
        account_id: &str,
        code: &str,
        unix_now: i64,
    ) -> Result<bool, AccountError> {
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        let (secret_b32, last): (Option<String>, Option<i64>) = tx.query_row(
            "SELECT totp_secret, totp_last_step FROM accounts WHERE id = ?1",
            [account_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let Some(secret_b32) = secret_b32 else { return Ok(false) };
        let code = code.trim();
        if code.contains('-') {
            // A recovery code. Compared by digest, so the comparison is of
            // hashes, never of the codes themselves.
            let n = tx.execute(
                "DELETE FROM recovery_codes WHERE account_id = ?1 AND digest = ?2",
                params![account_id, digest_hex(&code.to_ascii_lowercase())],
            )?;
            tx.commit()?;
            return Ok(n == 1);
        }
        let Some(step) =
            crate::totp::verify(&secret_b32, code, u64::try_from(unix_now).unwrap_or(0))
        else {
            return Ok(false);
        };
        let step = i64::try_from(step).unwrap_or(i64::MAX);
        if last.is_some_and(|l| step <= l) {
            return Ok(false);
        }
        tx.execute(
            "UPDATE accounts SET totp_last_step = ?2 WHERE id = ?1",
            params![account_id, step],
        )?;
        tx.commit()?;
        Ok(true)
    }

    /// Turn two-step log-in off and drop the recovery codes.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn disable_totp(&self, account_id: &str) -> Result<(), AccountError> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE accounts SET totp_secret = NULL, totp_pending = NULL, totp_last_step = NULL
             WHERE id = ?1",
            [account_id],
        )?;
        conn.execute("DELETE FROM recovery_codes WHERE account_id = ?1", [account_id])?;
        Ok(())
    }

    /// What deleting an account would take with it, and whether it is refused.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be read.
    pub fn deletion_plan(&self, account_id: &str) -> Result<DeletionPlan, AccountError> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT o.id, o.name, o.config_managed, m.role,
                    (SELECT COUNT(*) FROM org_members x WHERE x.org_id = o.id),
                    (SELECT COUNT(*) FROM org_members x WHERE x.org_id = o.id AND x.role = 'owner')
             FROM org_members m JOIN orgs o ON o.id = m.org_id WHERE m.account_id = ?1",
        )?;
        let rows = stmt.query_map([account_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)? != 0,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, i64>(5)?,
            ))
        })?;
        let mut plan = DeletionPlan::default();
        for row in rows {
            let (org, name, config_managed, role, members, owners) = row?;
            // An organisation the configuration declares outlives any one of
            // its members; the file brings it back on the next start anyway.
            if config_managed {
                continue;
            }
            if members <= 1 {
                let mut ps = conn.prepare("SELECT project FROM org_projects WHERE org_id = ?1")?;
                let projects: Vec<String> =
                    ps.query_map([&org], |r| r.get::<_, String>(0))?.collect::<Result<_, _>>()?;
                plan.orgs.push((org, projects));
            } else if role == "owner" && owners <= 1 {
                plan.blocked = Some(name);
            }
        }
        Ok(plan)
    }

    /// Delete an account and the organisations `orgs`. Their projects' data has
    /// been erased by the caller already.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn delete_account(&self, account_id: &str, orgs: &[String]) -> Result<(), AccountError> {
        let (key_digests, session_digests, others) = {
            let mut conn = self.conn()?;
            let tx = conn.transaction()?;
            let collect = |sql: &str, arg: &str| -> rusqlite::Result<Vec<String>> {
                let mut stmt = tx.prepare(sql)?;
                let rows = stmt.query_map([arg], |r| r.get::<_, String>(0))?;
                rows.collect()
            };
            let mut keys =
                collect("SELECT digest FROM account_keys WHERE account_id = ?1", account_id)?;
            let sessions =
                collect("SELECT digest FROM account_sessions WHERE account_id = ?1", account_id)?;
            let mut others = Vec::new();
            for org in orgs {
                keys.extend(collect(
                    "SELECT k.digest FROM account_keys k JOIN org_projects p ON p.project = k.project
                     WHERE p.org_id = ?1",
                    org,
                )?);
                others
                    .extend(collect("SELECT account_id FROM org_members WHERE org_id = ?1", org)?);
                tx.execute(
                    "DELETE FROM account_keys WHERE project IN
                     (SELECT project FROM org_projects WHERE org_id = ?1)",
                    [org],
                )?;
                tx.execute("DELETE FROM orgs WHERE id = ?1", [org])?;
            }
            // Keys the account made on organisations that outlive it go too:
            // the cascade below takes them, and they are dropped from memory.
            tx.execute("DELETE FROM accounts WHERE id = ?1", [account_id])?;
            tx.commit()?;
            (keys, sessions, others)
        };
        {
            let mut live = self.live_mut()?;
            for d in key_digests.iter().chain(session_digests.iter()) {
                if let Some(d) = parse_digest(d) {
                    live.remove(&d);
                }
            }
        }
        let mut ids = others;
        ids.push(account_id.to_owned());
        self.refresh(&ids)
    }

    /// Whether `presented` is the digest of a live session, for tests that hold
    /// only the token.
    #[must_use]
    pub fn is_live(&self, presented: &str) -> bool {
        self.live.read().is_ok_and(|l| l.contains_key(&digest(presented)))
    }
}
