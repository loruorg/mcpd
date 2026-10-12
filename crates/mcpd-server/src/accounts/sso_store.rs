//! Single sign-on settings, claimed domains, linked identities and SCIM
//! tokens in the store.

use super::roles::Role;
use super::{at, digest_hex, email_domain, now_secs, secret, AccountError, AccountStore};
use rusqlite::{params, OptionalExtension};
use time::OffsetDateTime;

/// An organisation's single sign-on settings, with the client secret.
#[derive(Clone, PartialEq, Eq)]
pub struct SsoSettings {
    /// The organisation.
    pub org_id: String,
    /// `oidc` or `github`.
    pub provider: String,
    /// The issuer, for `oidc`.
    pub issuer: Option<String>,
    /// The client id.
    pub client_id: String,
    /// The client secret.
    pub client_secret: Option<String>,
    /// Claimed domains.
    pub domains: Vec<String>,
    /// Password log-in refused for members in those domains.
    pub enforced: bool,
    /// The role for a member single sign-on creates.
    pub default_role: Role,
    /// The claim listing groups.
    pub groups_claim: Option<String>,
    /// Declared in the configuration file.
    pub config_managed: bool,
    /// Last changed.
    pub updated_at: i64,
}

impl std::fmt::Debug for SsoSettings {
    // By hand, so the client secret cannot reach a log line.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SsoSettings")
            .field("org_id", &self.org_id)
            .field("provider", &self.provider)
            .field("issuer", &self.issuer)
            .field("client_id", &self.client_id)
            .field("domains", &self.domains)
            .field("enforced", &self.enforced)
            .finish_non_exhaustive()
    }
}

/// What a single sign-on flow in progress keeps between start and callback.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct SsoStateData {
    /// The organisation.
    pub org_id: String,
    /// The nonce sent in the authorization request, checked in the ID token.
    pub nonce: String,
    /// The PKCE verifier.
    pub verifier: String,
    /// The SHA-256 of the browser binding, hex.
    pub binding: String,
}

/// Why a set of domains cannot be claimed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomainClaim {
    /// Every domain is free or already this organisation's.
    Ok,
    /// Another organisation claims this one.
    Taken(String),
}

impl AccountStore {
    /// An organisation's settings, when it has any.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be read.
    pub fn sso_settings(&self, org_id: &str) -> Result<Option<SsoSettings>, AccountError> {
        let conn = self.conn()?;
        let row = conn
            .query_row(
                "SELECT provider, issuer, client_id, client_secret, enforced, default_role,
                        groups_claim, config_managed, updated_at
                 FROM org_sso WHERE org_id = ?1",
                [org_id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, Option<String>>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, Option<String>>(3)?,
                        r.get::<_, i64>(4)? != 0,
                        r.get::<_, String>(5)?,
                        r.get::<_, Option<String>>(6)?,
                        r.get::<_, i64>(7)? != 0,
                        r.get::<_, i64>(8)?,
                    ))
                },
            )
            .optional()?;
        let Some((
            provider,
            issuer,
            client_id,
            client_secret,
            enforced,
            role,
            groups_claim,
            cm,
            updated,
        )) = row
        else {
            return Ok(None);
        };
        let mut stmt =
            conn.prepare("SELECT domain FROM org_domains WHERE org_id = ?1 ORDER BY domain")?;
        let domains: Vec<String> =
            stmt.query_map([org_id], |r| r.get(0))?.collect::<Result<_, _>>()?;
        Ok(Some(SsoSettings {
            org_id: org_id.to_owned(),
            provider,
            issuer,
            client_id,
            client_secret,
            domains,
            enforced,
            default_role: Role::parse(&role).unwrap_or(Role::Member).min(Role::Admin),
            groups_claim,
            config_managed: cm,
            updated_at: updated,
        }))
    }

    /// Whether these domains can be claimed by `org_id`.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be read.
    pub fn check_domains(
        &self,
        org_id: &str,
        domains: &[String],
    ) -> Result<DomainClaim, AccountError> {
        let conn = self.conn()?;
        for d in domains {
            let owner: Option<String> = conn
                .query_row("SELECT org_id FROM org_domains WHERE domain = ?1", [d], |r| r.get(0))
                .optional()?;
            if owner.is_some_and(|o| o != org_id) {
                return Ok(DomainClaim::Taken(d.clone()));
            }
        }
        Ok(DomainClaim::Ok)
    }

    /// Store settings and their domains. `client_secret` of `None` keeps the
    /// stored one. Returns the members whose password sessions should end
    /// because enforcement now covers them.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written, including a
    /// domain another organisation claimed in the meantime.
    pub fn put_sso_settings(&self, s: &SsoSettings) -> Result<Vec<String>, AccountError> {
        let now = now_secs();
        let affected = {
            let mut conn = self.conn()?;
            let tx = conn.transaction()?;
            tx.execute(
                "INSERT INTO org_sso (org_id, provider, issuer, client_id, client_secret, enforced,
                                      default_role, groups_claim, config_managed, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT (org_id) DO UPDATE SET provider = excluded.provider,
                    issuer = excluded.issuer, client_id = excluded.client_id,
                    client_secret = COALESCE(excluded.client_secret, org_sso.client_secret),
                    enforced = excluded.enforced, default_role = excluded.default_role,
                    groups_claim = excluded.groups_claim, config_managed = excluded.config_managed,
                    updated_at = excluded.updated_at",
                params![
                    s.org_id,
                    s.provider,
                    s.issuer,
                    s.client_id,
                    s.client_secret,
                    s.enforced,
                    s.default_role.as_str(),
                    s.groups_claim,
                    s.config_managed,
                    now
                ],
            )?;
            tx.execute("DELETE FROM org_domains WHERE org_id = ?1", [&s.org_id])?;
            for d in &s.domains {
                tx.execute(
                    "INSERT INTO org_domains (domain, org_id) VALUES (?1, ?2)",
                    params![d, s.org_id],
                )?;
            }
            let mut affected = Vec::new();
            if s.enforced {
                let mut stmt = tx.prepare(
                    "SELECT a.id, a.email FROM org_members m JOIN accounts a ON a.id = m.account_id
                     WHERE m.org_id = ?1",
                )?;
                let rows = stmt.query_map([&s.org_id], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })?;
                for row in rows {
                    let (id, email) = row?;
                    if s.domains.iter().any(|d| d == email_domain(&email)) {
                        affected.push(id);
                    }
                }
            }
            tx.commit()?;
            affected
        };
        Ok(affected)
    }

    /// Remove an organisation's settings and release its domains.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn delete_sso_settings(&self, org_id: &str) -> Result<bool, AccountError> {
        let conn = self.conn()?;
        conn.execute("DELETE FROM org_domains WHERE org_id = ?1", [org_id])?;
        Ok(conn.execute("DELETE FROM org_sso WHERE org_id = ?1", [org_id])? > 0)
    }

    /// The organisation that claims an email's domain and logs in with single
    /// sign-on, when there is one.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be read.
    pub fn org_for_domain(&self, domain: &str) -> Result<Option<String>, AccountError> {
        let conn = self.conn()?;
        Ok(conn
            .query_row(
                "SELECT d.org_id FROM org_domains d JOIN org_sso s ON s.org_id = d.org_id
                 WHERE d.domain = ?1",
                [domain],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Whether any organisation logs in with single sign-on.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be read.
    pub fn any_sso(&self) -> Result<bool, AccountError> {
        let conn = self.conn()?;
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM org_sso", [], |r| r.get(0))?;
        Ok(n > 0)
    }

    /// The name of an organisation that requires single sign-on of this
    /// account, when one does: the account is a member, the organisation
    /// enforces it, and the address is in one of its claimed domains.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be read.
    pub fn sso_required_for(
        &self,
        account_id: &str,
        email: &str,
    ) -> Result<Option<String>, AccountError> {
        let conn = self.conn()?;
        Ok(conn
            .query_row(
                "SELECT o.name FROM org_members m
                 JOIN org_sso s ON s.org_id = m.org_id AND s.enforced = 1
                 JOIN org_domains d ON d.org_id = m.org_id AND d.domain = ?2
                 JOIN orgs o ON o.id = m.org_id
                 WHERE m.account_id = ?1 LIMIT 1",
                params![account_id, email_domain(email)],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// The account linked to an identity at an organisation's provider.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be read.
    pub fn linked_account(
        &self,
        org_id: &str,
        subject: &str,
    ) -> Result<Option<String>, AccountError> {
        let conn = self.conn()?;
        Ok(conn
            .query_row(
                "SELECT account_id FROM sso_identities WHERE org_id = ?1 AND subject = ?2",
                params![org_id, subject],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Link an identity at an organisation's provider to an account.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn link_identity(
        &self,
        org_id: &str,
        subject: &str,
        account_id: &str,
    ) -> Result<(), AccountError> {
        self.conn()?.execute(
            "INSERT INTO sso_identities (org_id, subject, account_id, created_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (org_id, subject) DO UPDATE SET account_id = excluded.account_id",
            params![org_id, subject, account_id, now_secs()],
        )?;
        Ok(())
    }

    /// Replace an organisation's SCIM token, returning the new secret, its
    /// prefix and when it was made.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn rotate_scim_token(
        &self,
        org_id: &str,
    ) -> Result<(String, String, OffsetDateTime), AccountError> {
        let token = secret("mcpdscim_");
        self.set_scim_token(org_id, &token)?;
        let prefix: String = token.chars().take(17).collect();
        Ok((token, prefix, OffsetDateTime::now_utc()))
    }

    /// Store a SCIM token chosen elsewhere, for one the configuration file names.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn set_scim_token(&self, org_id: &str, token: &str) -> Result<(), AccountError> {
        let prefix: String = token.chars().take(17).collect();
        self.conn()?.execute(
            "INSERT INTO org_scim (org_id, digest, prefix, created_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (org_id) DO UPDATE SET digest = excluded.digest, prefix = excluded.prefix,
                created_at = excluded.created_at",
            params![org_id, digest_hex(token), prefix, now_secs()],
        )?;
        Ok(())
    }

    /// Revoke an organisation's SCIM token.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn revoke_scim_token(&self, org_id: &str) -> Result<bool, AccountError> {
        Ok(self.conn()?.execute("DELETE FROM org_scim WHERE org_id = ?1", [org_id])? > 0)
    }

    /// The prefix and age of an organisation's SCIM token, when it has one.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be read.
    pub fn scim_token_info(
        &self,
        org_id: &str,
    ) -> Result<Option<(String, OffsetDateTime)>, AccountError> {
        let conn = self.conn()?;
        Ok(conn
            .query_row("SELECT prefix, created_at FROM org_scim WHERE org_id = ?1", [org_id], |r| {
                Ok((r.get::<_, String>(0)?, at(r.get(1)?)))
            })
            .optional()?)
    }

    /// The organisation a SCIM token belongs to. Looked up by digest, so the
    /// comparison is of hashes and a timing difference says nothing about the
    /// token.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be read.
    pub fn org_for_scim_token(&self, presented: &str) -> Result<Option<String>, AccountError> {
        let conn = self.conn()?;
        Ok(conn
            .query_row(
                "SELECT org_id FROM org_scim WHERE digest = ?1",
                [digest_hex(presented)],
                |r| r.get(0),
            )
            .optional()?)
    }
}
