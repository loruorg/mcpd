//! Organisations, members, projects and invitations in the store.
//!
//! Every write here ends by re-reading the reach of the accounts it touched,
//! so the next request is authorised against what was just written and never
//! against what was true a moment ago.

use super::roles::{ProjectAccess, Role};
use super::{
    at, digest_hex, identifier, now_secs, parse_digest, secret, AccountError, AccountStore,
};
use mcpd_core::{AccountKey, GroupRole, OrgInvite, OrgMember, OrgProject, ProjectRoleGrant};
use rusqlite::{params, OptionalExtension};
use std::collections::HashMap;

/// Most organisations one account may belong to.
pub const MAX_ORGS_PER_ACCOUNT: i64 = 20;

/// Most projects one organisation may own.
pub const MAX_PROJECTS_PER_ORG: i64 = 100;

/// Most invitations outstanding in one organisation.
pub const MAX_INVITES_PER_ORG: i64 = 200;

/// Most group mappings per organisation.
pub const MAX_GROUP_ROLES: usize = 100;

/// How long an invitation works, in seconds.
pub const INVITE_TTL_SECONDS: i64 = 7 * 86_400;

/// An organisation as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrgRow {
    /// Its id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// When it was created.
    pub created_at: i64,
    /// Its own ceiling, when it set one.
    pub quota_per_minute: Option<u32>,
    /// Its own retention, when it set one.
    pub retention_days: Option<u32>,
    /// Declared in the configuration file.
    pub config_managed: bool,
    /// How many members.
    pub members: u32,
    /// How many projects.
    pub projects: u32,
}

/// An invitation as stored, found by its token.
#[derive(Debug, Clone)]
pub struct InviteRow {
    /// Its id.
    pub id: String,
    /// The organisation.
    pub org_id: String,
    /// The organisation's name.
    pub org_name: String,
    /// The invited address.
    pub email: String,
    /// The role it grants.
    pub role: Role,
    /// Project roles it grants.
    pub project_roles: Vec<(String, ProjectAccess)>,
    /// The SCIM user it was sent for, when SCIM sent it.
    pub scim_user_id: Option<String>,
    /// When it stops working.
    pub expires_at: i64,
}

fn u32_of(v: Option<i64>) -> Option<u32> {
    v.and_then(|n| u32::try_from(n).ok())
}

fn grants_from_json(text: &str) -> Vec<(String, ProjectAccess)> {
    serde_json::from_str::<Vec<(String, String)>>(text)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(p, r)| ProjectAccess::parse(&r).map(|r| (p, r)))
        .collect()
}

fn grants_to_wire(g: &[(String, ProjectAccess)]) -> Vec<ProjectRoleGrant> {
    g.iter().map(|(p, r)| ProjectRoleGrant { project: p.clone(), role: r.to_wire() }).collect()
}

const ORG_SELECT: &str = "SELECT o.id, o.name, o.created_at, o.quota_per_minute, o.retention_days,
        o.config_managed,
        (SELECT COUNT(*) FROM org_members m WHERE m.org_id = o.id),
        (SELECT COUNT(*) FROM org_projects p WHERE p.org_id = o.id)
     FROM orgs o";

fn org_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<OrgRow> {
    Ok(OrgRow {
        id: r.get(0)?,
        name: r.get(1)?,
        created_at: r.get(2)?,
        quota_per_minute: u32_of(r.get(3)?),
        retention_days: u32_of(r.get(4)?),
        config_managed: r.get::<_, i64>(5)? != 0,
        members: u32_of(Some(r.get(6)?)).unwrap_or(0),
        projects: u32_of(Some(r.get(7)?)).unwrap_or(0),
    })
}

impl AccountStore {
    /// Create an organisation with one project, owned by `account_id`. `None`
    /// when the account belongs to [`MAX_ORGS_PER_ACCOUNT`] already.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn create_org(&self, account_id: &str, name: &str) -> Result<Option<OrgRow>, AccountError> {
        let org_id = identifier("org_", 12);
        let project = identifier("prj_", 10);
        let now = now_secs();
        {
            let mut conn = self.conn()?;
            let tx = conn.transaction()?;
            let held: i64 = tx.query_row(
                "SELECT COUNT(*) FROM org_members WHERE account_id = ?1",
                [account_id],
                |r| r.get(0),
            )?;
            if held >= MAX_ORGS_PER_ACCOUNT {
                return Ok(None);
            }
            tx.execute(
                "INSERT INTO orgs (id, name, created_at) VALUES (?1, ?2, ?3)",
                params![org_id, name, now],
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
        self.refresh(&[account_id.to_owned()])?;
        self.org_row(&org_id)
    }

    /// One organisation, whoever asks. The caller has checked membership.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be read.
    pub fn org_row(&self, org_id: &str) -> Result<Option<OrgRow>, AccountError> {
        let conn = self.conn()?;
        Ok(conn
            .query_row(&format!("{ORG_SELECT} WHERE o.id = ?1"), [org_id], org_row)
            .optional()?)
    }

    /// Every organisation an account belongs to, oldest first.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be read.
    pub fn orgs_of(&self, account_id: &str) -> Result<Vec<OrgRow>, AccountError> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(&format!(
            "{ORG_SELECT} JOIN org_members me ON me.org_id = o.id AND me.account_id = ?1
             ORDER BY o.created_at, o.id"
        ))?;
        let rows = stmt.query_map([account_id], org_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Rename an organisation, or set its ceiling and retention. A field left
    /// `None` keeps its value; `Some(None)` clears it.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn update_org(
        &self,
        org_id: &str,
        name: Option<&str>,
        quota: Option<Option<u32>>,
        retention: Option<Option<u32>>,
    ) -> Result<(), AccountError> {
        let conn = self.conn()?;
        if let Some(n) = name {
            conn.execute("UPDATE orgs SET name = ?2 WHERE id = ?1", params![org_id, n])?;
        }
        if let Some(q) = quota {
            conn.execute(
                "UPDATE orgs SET quota_per_minute = ?2 WHERE id = ?1",
                params![org_id, q],
            )?;
        }
        if let Some(d) = retention {
            conn.execute("UPDATE orgs SET retention_days = ?2 WHERE id = ?1", params![org_id, d])?;
        }
        Ok(())
    }

    /// The projects an organisation owns, oldest first.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be read.
    pub fn org_projects(&self, org_id: &str) -> Result<Vec<OrgProject>, AccountError> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT project, label, created_at FROM org_projects WHERE org_id = ?1
             ORDER BY created_at, project",
        )?;
        let rows = stmt.query_map([org_id], |r| {
            Ok(OrgProject { project: r.get(0)?, label: r.get(1)?, created_at: at(r.get(2)?) })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Create a project in an organisation. `None` past
    /// [`MAX_PROJECTS_PER_ORG`].
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn create_project(
        &self,
        org_id: &str,
        label: Option<&str>,
    ) -> Result<Option<OrgProject>, AccountError> {
        let project = identifier("prj_", 10);
        let now = now_secs();
        {
            let conn = self.conn()?;
            let held: i64 = conn.query_row(
                "SELECT COUNT(*) FROM org_projects WHERE org_id = ?1",
                [org_id],
                |r| r.get(0),
            )?;
            if held >= MAX_PROJECTS_PER_ORG {
                return Ok(None);
            }
            conn.execute(
                "INSERT INTO org_projects (project, org_id, label, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![project, org_id, label, now],
            )?;
        }
        self.refresh_org(org_id)?;
        Ok(Some(OrgProject { project, label: label.map(str::to_owned), created_at: at(now) }))
    }

    /// Attach an existing project by name, for an organisation the
    /// configuration declares. `false` when another organisation owns it.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn attach_project(&self, org_id: &str, project: &str) -> Result<bool, AccountError> {
        let owner: Option<String> = {
            let conn = self.conn()?;
            let owner = conn
                .query_row("SELECT org_id FROM org_projects WHERE project = ?1", [project], |r| {
                    r.get(0)
                })
                .optional()?;
            if owner.is_none() {
                conn.execute(
                    "INSERT INTO org_projects (project, org_id, created_at) VALUES (?1, ?2, ?3)",
                    params![project, org_id, now_secs()],
                )?;
            }
            owner
        };
        match owner {
            Some(o) => Ok(o == org_id),
            None => {
                self.refresh_org(org_id)?;
                Ok(true)
            }
        }
    }

    /// Why a project cannot be deleted, or `None` when it can.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be read.
    pub fn project_delete_blocker(
        &self,
        org_id: &str,
        project: &str,
    ) -> Result<Option<String>, AccountError> {
        let conn = self.conn()?;
        let home: Option<i64> = conn
            .query_row("SELECT 1 FROM accounts WHERE project = ?1", [project], |r| r.get(0))
            .optional()?;
        if home.is_some() {
            return Ok(Some(
                "this project was created with an account, and goes when that account is deleted"
                    .to_owned(),
            ));
        }
        let n: i64 =
            conn.query_row("SELECT COUNT(*) FROM org_projects WHERE org_id = ?1", [org_id], |r| {
                r.get(0)
            })?;
        if n <= 1 {
            return Ok(Some("an organisation keeps at least one project".to_owned()));
        }
        Ok(None)
    }

    /// Delete a project from an organisation and revoke its keys. `false` when
    /// the organisation owns no such project. Its data is the caller's to erase.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn delete_project(&self, org_id: &str, project: &str) -> Result<bool, AccountError> {
        let found: Option<i64> = {
            let conn = self.conn()?;
            conn.query_row(
                "SELECT 1 FROM org_projects WHERE project = ?1 AND org_id = ?2",
                params![project, org_id],
                |r| r.get(0),
            )
            .optional()?
        };
        if found.is_none() {
            return Ok(false);
        }
        self.delete_keys_where("project = ?1 AND ?2 = ?2", project, "")?;
        self.conn()?.execute("DELETE FROM org_projects WHERE project = ?1", [project])?;
        self.refresh_org(org_id)?;
        Ok(true)
    }

    /// Delete an organisation and everything it holds in this file. Returns
    /// the projects it owned, whose data is the caller's to erase.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn delete_org(&self, org_id: &str) -> Result<Vec<String>, AccountError> {
        let projects: Vec<String> =
            self.org_projects(org_id)?.into_iter().map(|p| p.project).collect();
        let members: Vec<String> = self.member_ids(org_id)?;
        for p in &projects {
            self.delete_keys_where("project = ?1 AND ?2 = ?2", p, "")?;
        }
        for m in &members {
            self.end_org_sessions(m, org_id)?;
        }
        self.conn()?.execute("DELETE FROM orgs WHERE id = ?1", [org_id])?;
        self.refresh(&members)?;
        Ok(projects)
    }

    fn member_ids(&self, org_id: &str) -> Result<Vec<String>, AccountError> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT account_id FROM org_members WHERE org_id = ?1")?;
        let rows = stmt.query_map([org_id], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Every member of an organisation, owners first.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be read.
    pub fn list_members(&self, org_id: &str) -> Result<Vec<OrgMember>, AccountError> {
        let conn = self.conn()?;
        let mut grants: HashMap<String, Vec<ProjectRoleGrant>> = HashMap::new();
        {
            let mut stmt = conn.prepare(
                "SELECT pr.account_id, pr.project, pr.role FROM project_roles pr
                 JOIN org_projects p ON p.project = pr.project WHERE p.org_id = ?1
                 ORDER BY pr.project",
            )?;
            let rows = stmt.query_map([org_id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
            })?;
            for row in rows {
                let (account, project, role) = row?;
                if let Some(role) = ProjectAccess::parse(&role) {
                    grants
                        .entry(account)
                        .or_default()
                        .push(ProjectRoleGrant { project, role: role.to_wire() });
                }
            }
        }
        let mut stmt = conn.prepare(
            "SELECT m.account_id, a.email, m.role, m.via, m.created_at, a.managed_by, a.totp_secret
             FROM org_members m JOIN accounts a ON a.id = m.account_id
             WHERE m.org_id = ?1 ORDER BY m.created_at, a.email",
        )?;
        let rows = stmt.query_map([org_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, Option<String>>(6)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (account_id, email, role, via, joined, managed_by, totp) = row?;
            let Some(role) = Role::parse(&role) else { continue };
            out.push(OrgMember {
                project_roles: grants.remove(&account_id).unwrap_or_default(),
                account_id,
                email,
                role: role.to_wire(),
                via,
                joined_at: at(joined),
                managed: managed_by.as_deref() == Some(org_id),
                totp_enabled: totp.is_some(),
            });
        }
        out.sort_by_key(|m| std::cmp::Reverse(Role::from_wire(m.role)));
        Ok(out)
    }

    /// One member, or `None`.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be read.
    pub fn member(
        &self,
        org_id: &str,
        account_id: &str,
    ) -> Result<Option<OrgMember>, AccountError> {
        Ok(self.list_members(org_id)?.into_iter().find(|m| m.account_id == account_id))
    }

    /// How many owners an organisation has.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be read.
    pub fn owner_count(&self, org_id: &str) -> Result<u32, AccountError> {
        let conn = self.conn()?;
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM org_members WHERE org_id = ?1 AND role = 'owner'",
            [org_id],
            |r| r.get(0),
        )?;
        Ok(u32::try_from(n).unwrap_or(0))
    }

    /// Add a member, or change the role of one who is already there.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn upsert_member(
        &self,
        org_id: &str,
        account_id: &str,
        role: Role,
        via: &str,
    ) -> Result<(), AccountError> {
        self.conn()?.execute(
            "INSERT INTO org_members (org_id, account_id, role, via, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (org_id, account_id) DO UPDATE SET role = excluded.role",
            params![org_id, account_id, role.as_str(), via, now_secs()],
        )?;
        self.refresh(&[account_id.to_owned()])
    }

    /// Change a member's role. `false` when they are not a member.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn set_role(
        &self,
        org_id: &str,
        account_id: &str,
        role: Role,
    ) -> Result<bool, AccountError> {
        let n = self.conn()?.execute(
            "UPDATE org_members SET role = ?3 WHERE org_id = ?1 AND account_id = ?2",
            params![org_id, account_id, role.as_str()],
        )?;
        self.refresh(&[account_id.to_owned()])?;
        Ok(n > 0)
    }

    /// Set or clear a member's role on one project of the organisation.
    /// `false` when the organisation owns no such project or has no such
    /// member.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn set_project_role(
        &self,
        org_id: &str,
        account_id: &str,
        project: &str,
        role: Option<ProjectAccess>,
    ) -> Result<bool, AccountError> {
        {
            let conn = self.conn()?;
            let ok: Option<i64> = conn
                .query_row(
                    "SELECT 1 FROM org_projects p JOIN org_members m ON m.org_id = p.org_id
                     WHERE p.project = ?1 AND p.org_id = ?2 AND m.account_id = ?3",
                    params![project, org_id, account_id],
                    |r| r.get(0),
                )
                .optional()?;
            if ok.is_none() {
                return Ok(false);
            }
            match role {
                None => {
                    conn.execute(
                        "DELETE FROM project_roles WHERE project = ?1 AND account_id = ?2",
                        params![project, account_id],
                    )?;
                }
                Some(r) => {
                    conn.execute(
                        "INSERT INTO project_roles (project, account_id, role) VALUES (?1, ?2, ?3)
                         ON CONFLICT (project, account_id) DO UPDATE SET role = excluded.role",
                        params![project, account_id, r.as_str()],
                    )?;
                }
            }
        }
        self.refresh(&[account_id.to_owned()])?;
        Ok(true)
    }

    /// Remove a member: their project roles, their keys on the organisation's
    /// projects, and their sessions limited to it. `false` when they were not
    /// a member.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn remove_member(&self, org_id: &str, account_id: &str) -> Result<bool, AccountError> {
        let n = {
            let conn = self.conn()?;
            conn.execute(
                "DELETE FROM project_roles WHERE account_id = ?1 AND project IN
                 (SELECT project FROM org_projects WHERE org_id = ?2)",
                params![account_id, org_id],
            )?;
            conn.execute(
                "DELETE FROM org_members WHERE org_id = ?1 AND account_id = ?2",
                params![org_id, account_id],
            )?
        };
        self.delete_keys_where(
            "account_id = ?1 AND project IN (SELECT project FROM org_projects WHERE org_id = ?2)",
            account_id,
            org_id,
        )?;
        self.end_org_sessions(account_id, org_id)?;
        self.refresh(&[account_id.to_owned()])?;
        Ok(n > 0)
    }

    /// Deactivate an account, or bring it back: every credential it holds stops
    /// or starts working, and deactivating ends its sessions.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn set_disabled(&self, account_id: &str, disabled: bool) -> Result<(), AccountError> {
        self.conn()?.execute(
            "UPDATE accounts SET disabled_at = CASE WHEN ?2 THEN COALESCE(disabled_at, ?3) ELSE NULL END
             WHERE id = ?1",
            params![account_id, disabled, now_secs()],
        )?;
        if disabled {
            self.end_other_sessions(account_id, None)?;
        }
        self.refresh(&[account_id.to_owned()])
    }

    /// Delete an account an organisation manages, for SCIM. The account's own
    /// organisations are left alone: a managed account has none.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn delete_managed_account(&self, account_id: &str) -> Result<(), AccountError> {
        self.delete_account(account_id, &[])
    }

    /// Store an invitation and return it with its token.
    /// `None` past [`MAX_INVITES_PER_ORG`].
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    #[allow(clippy::too_many_arguments)]
    pub fn create_invite(
        &self,
        org_id: &str,
        email: &str,
        role: Role,
        project_roles: &[(String, ProjectAccess)],
        invited_by: Option<&str>,
        scim_user_id: Option<&str>,
    ) -> Result<Option<(OrgInvite, String)>, AccountError> {
        let token = secret("mcpdi_");
        let id = identifier("inv_", 8);
        let now = now_secs();
        let expires = now + INVITE_TTL_SECONDS;
        let grants: Vec<(String, String)> =
            project_roles.iter().map(|(p, r)| (p.clone(), r.as_str().to_owned())).collect();
        let conn = self.conn()?;
        let held: i64 =
            conn.query_row("SELECT COUNT(*) FROM org_invites WHERE org_id = ?1", [org_id], |r| {
                r.get(0)
            })?;
        if held >= MAX_INVITES_PER_ORG {
            return Ok(None);
        }
        // One live invitation per address: a new one replaces the last.
        conn.execute(
            "DELETE FROM org_invites WHERE org_id = ?1 AND email = ?2",
            params![org_id, email],
        )?;
        conn.execute(
            "INSERT INTO org_invites (id, org_id, email, role, project_roles, digest, invited_by,
                                      scim_user_id, created_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                id,
                org_id,
                email,
                role.as_str(),
                serde_json::to_string(&grants).unwrap_or_else(|_| "[]".to_owned()),
                digest_hex(&token),
                invited_by,
                scim_user_id,
                now,
                expires
            ],
        )?;
        Ok(Some((
            OrgInvite {
                id,
                email: email.to_owned(),
                role: role.to_wire(),
                project_roles: grants_to_wire(project_roles),
                invited_by: invited_by.map(str::to_owned),
                created_at: at(now),
                expires_at: at(expires),
            },
            token,
        )))
    }

    /// Whether an address has an invitation outstanding in an organisation.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be read.
    pub fn has_invite(&self, org_id: &str, email: &str) -> Result<bool, AccountError> {
        let conn = self.conn()?;
        let n: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM org_invites WHERE org_id = ?1 AND email = ?2 AND expires_at > ?3",
                params![org_id, email, now_secs()],
                |r| r.get(0),
            )
            .optional()?;
        Ok(n.is_some())
    }

    /// The invitations outstanding in an organisation, newest first.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be read.
    pub fn list_invites(&self, org_id: &str) -> Result<Vec<OrgInvite>, AccountError> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT i.id, i.email, i.role, i.project_roles, a.email, i.created_at, i.expires_at
             FROM org_invites i LEFT JOIN accounts a ON a.id = i.invited_by
             WHERE i.org_id = ?1 AND i.expires_at > ?2 ORDER BY i.created_at DESC",
        )?;
        let rows = stmt.query_map(params![org_id, now_secs()], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, i64>(6)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, email, role, grants, by, created, expires) = row?;
            let Some(role) = Role::parse(&role) else { continue };
            out.push(OrgInvite {
                id,
                email,
                role: role.to_wire(),
                project_roles: grants_to_wire(&grants_from_json(&grants)),
                invited_by: by,
                created_at: at(created),
                expires_at: at(expires),
            });
        }
        Ok(out)
    }

    /// Revoke an invitation. The organisation is part of the match.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn revoke_invite(&self, org_id: &str, invite_id: &str) -> Result<bool, AccountError> {
        let n = self.conn()?.execute(
            "DELETE FROM org_invites WHERE id = ?1 AND org_id = ?2",
            params![invite_id, org_id],
        )?;
        Ok(n > 0)
    }

    /// The invitation a token is, when it is live.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be read.
    pub fn find_invite(&self, presented: &str) -> Result<Option<InviteRow>, AccountError> {
        let conn = self.conn()?;
        let row = conn
            .query_row(
                "SELECT i.id, i.org_id, o.name, i.email, i.role, i.project_roles, i.scim_user_id,
                        i.expires_at
                 FROM org_invites i JOIN orgs o ON o.id = i.org_id
                 WHERE i.digest = ?1 AND i.expires_at > ?2",
                params![digest_hex(presented.trim()), now_secs()],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                        r.get::<_, String>(5)?,
                        r.get::<_, Option<String>>(6)?,
                        r.get::<_, i64>(7)?,
                    ))
                },
            )
            .optional()?;
        Ok(row.and_then(|(id, org_id, org_name, email, role, grants, scim_user_id, expires_at)| {
            Some(InviteRow {
                id,
                org_id,
                org_name,
                email,
                role: Role::parse(&role)?,
                project_roles: grants_from_json(&grants),
                scim_user_id,
                expires_at,
            })
        }))
    }

    /// Accept an invitation for an account: membership, project roles, and the
    /// SCIM user it was sent for. Consumes the invitation. `false` when it was
    /// used in the meantime.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn accept_invite(
        &self,
        invite: &InviteRow,
        account_id: &str,
    ) -> Result<bool, AccountError> {
        {
            let mut conn = self.conn()?;
            let tx = conn.transaction()?;
            let n = tx.execute("DELETE FROM org_invites WHERE id = ?1", [&invite.id])?;
            if n == 0 {
                return Ok(false);
            }
            tx.execute(
                "INSERT INTO org_members (org_id, account_id, role, via, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT (org_id, account_id) DO NOTHING",
                params![
                    invite.org_id,
                    account_id,
                    invite.role.as_str(),
                    if invite.scim_user_id.is_some() { "scim" } else { "invite" },
                    now_secs()
                ],
            )?;
            for (project, role) in &invite.project_roles {
                tx.execute(
                    "INSERT INTO project_roles (project, account_id, role)
                     SELECT ?1, ?2, ?3 WHERE EXISTS
                       (SELECT 1 FROM org_projects WHERE project = ?1 AND org_id = ?4)
                     ON CONFLICT (project, account_id) DO UPDATE SET role = excluded.role",
                    params![project, account_id, role.as_str(), invite.org_id],
                )?;
            }
            if let Some(scim) = &invite.scim_user_id {
                tx.execute(
                    "UPDATE scim_users SET account_id = ?2 WHERE id = ?1 AND org_id = ?3",
                    params![scim, account_id, invite.org_id],
                )?;
            }
            tx.commit()?;
        }
        self.refresh(&[account_id.to_owned()])?;
        Ok(true)
    }

    /// Every key on an organisation's projects, whoever made it, newest first.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be read.
    pub fn org_keys(&self, org_id: &str) -> Result<Vec<AccountKey>, AccountError> {
        self.key_rows(
            "SELECT k.id, k.scope, k.name, k.prefix, k.created_at, k.project, a.email
             FROM account_keys k JOIN org_projects p ON p.project = k.project
             JOIN accounts a ON a.id = k.account_id
             WHERE p.org_id = ?1 ORDER BY k.created_at DESC, k.rowid DESC",
            org_id,
        )
    }

    /// Revoke any key on an organisation's projects.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn revoke_org_key(&self, org_id: &str, key_id: &str) -> Result<bool, AccountError> {
        Ok(self.delete_keys_where(
            "id = ?1 AND project IN (SELECT project FROM org_projects WHERE org_id = ?2)",
            key_id,
            org_id,
        )? > 0)
    }

    /// How many keys an organisation's projects hold.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be read.
    pub fn org_key_count(&self, org_id: &str) -> Result<u32, AccountError> {
        let conn = self.conn()?;
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM account_keys k JOIN org_projects p ON p.project = k.project
             WHERE p.org_id = ?1",
            [org_id],
            |r| r.get(0),
        )?;
        Ok(u32::try_from(n).unwrap_or(0))
    }

    /// Revoke the keys a member made on the organisation's projects that their
    /// role there no longer allows. Their reach already refuses them; this
    /// keeps the key list honest.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn prune_keys(&self, org_id: &str, account_id: &str) -> Result<u32, AccountError> {
        let keys: Vec<(String, String, String)> = {
            let conn = self.conn()?;
            let mut stmt = conn.prepare(
                "SELECT k.id, k.scope, k.project FROM account_keys k
                 JOIN org_projects p ON p.project = k.project
                 WHERE p.org_id = ?1 AND k.account_id = ?2",
            )?;
            let rows = stmt.query_map(params![org_id, account_id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?;
            rows.collect::<Result<_, _>>()?
        };
        let mut n = 0;
        for (id, scope, project) in keys {
            let allowed = super::scope_of(&scope).is_some_and(|s| {
                self.role_on(account_id, &project).is_some_and(|(_, role)| role.permits(s))
            });
            if !allowed {
                n += self.delete_keys_where("id = ?1 AND account_id = ?2", &id, account_id)?;
            }
        }
        Ok(n)
    }

    /// The group mapping of an organisation.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be read.
    pub fn group_roles(&self, org_id: &str) -> Result<Vec<GroupRole>, AccountError> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT group_name, role FROM org_group_roles WHERE org_id = ?1 ORDER BY group_name",
        )?;
        let rows =
            stmt.query_map([org_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        let mut out = Vec::new();
        for row in rows {
            let (group, role) = row?;
            if let Some(role) = Role::parse(&role) {
                out.push(GroupRole { group, role: role.to_wire() });
            }
        }
        Ok(out)
    }

    /// Replace the group mapping of an organisation. Group names are stored
    /// lowercased, because identity providers disagree about case.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn put_group_roles(
        &self,
        org_id: &str,
        roles: &[(String, Role)],
    ) -> Result<(), AccountError> {
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM org_group_roles WHERE org_id = ?1", [org_id])?;
        for (g, r) in roles {
            tx.execute(
                "INSERT INTO org_group_roles (org_id, group_name, role) VALUES (?1, ?2, ?3)",
                params![org_id, g.to_lowercase(), r.as_str()],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// The highest role among `groups`, by the organisation's mapping.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be read.
    pub fn role_for_groups(
        &self,
        org_id: &str,
        groups: &[String],
    ) -> Result<Option<Role>, AccountError> {
        if groups.is_empty() {
            return Ok(None);
        }
        let map: HashMap<String, Role> = self
            .group_roles(org_id)?
            .into_iter()
            .map(|g| (g.group, Role::from_wire(g.role)))
            .collect();
        Ok(groups
            .iter()
            .filter_map(|g| map.get(&g.to_lowercase()).copied())
            // Never owner, whatever was stored: a group from outside cannot
            // make somebody an owner.
            .map(|r| r.min(Role::Admin))
            .max())
    }

    /// Every organisation's ceiling and the projects it covers, for the ingest
    /// counters. `default` applies to an organisation that set none.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be read.
    pub fn org_ceilings(
        &self,
        default: Option<u32>,
    ) -> Result<Vec<(String, u32, Vec<String>)>, AccountError> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT o.id, o.quota_per_minute, p.project FROM orgs o
             JOIN org_projects p ON p.org_id = o.id ORDER BY o.id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<i64>>(1)?, r.get::<_, String>(2)?))
        })?;
        let mut out: Vec<(String, u32, Vec<String>)> = Vec::new();
        for row in rows {
            let (org, own, project) = row?;
            let Some(limit) = u32_of(own).or(default) else { continue };
            match out.last_mut() {
                Some((o, _, ps)) if *o == org => ps.push(project),
                _ => out.push((org, limit, vec![project])),
            }
        }
        Ok(out)
    }

    /// Create or update an organisation the configuration declares. Its id is
    /// fixed by its slug, so restarting finds the same one.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn upsert_config_org(
        &self,
        org_id: &str,
        name: &str,
        quota: Option<u32>,
        retention: Option<u32>,
    ) -> Result<(), AccountError> {
        self.conn()?.execute(
            "INSERT INTO orgs (id, name, created_at, quota_per_minute, retention_days, config_managed)
             VALUES (?1, ?2, ?3, ?4, ?5, 1)
             ON CONFLICT (id) DO UPDATE SET name = excluded.name,
                quota_per_minute = excluded.quota_per_minute,
                retention_days = excluded.retention_days, config_managed = 1",
            params![org_id, name, now_secs(), quota, retention],
        )?;
        Ok(())
    }

    /// Whether an organisation exists.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be read.
    pub fn org_exists(&self, org_id: &str) -> Result<bool, AccountError> {
        let conn = self.conn()?;
        let n: Option<i64> = conn
            .query_row("SELECT 1 FROM orgs WHERE id = ?1", [org_id], |r| r.get(0))
            .optional()?;
        Ok(n.is_some())
    }

    /// Drop every key of a member from memory and the file, whatever project.
    /// For a deactivated managed account.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn revoke_all_keys(&self, account_id: &str) -> Result<u32, AccountError> {
        self.delete_keys_where("account_id = ?1 AND ?2 = ?2", account_id, "")
    }

    /// Whether this digest is a live key, for tests.
    #[must_use]
    pub fn has_key_digest(&self, hex_digest: &str) -> bool {
        parse_digest(hex_digest).is_some_and(|d| self.live.read().is_ok_and(|l| l.contains_key(&d)))
    }
}
