//! SCIM users and groups in the store.
//!
//! A SCIM user is the identity provider's record of a person in one
//! organisation. It points at an account once there is one: at once for an
//! address in a claimed domain, or when the person accepts the invitation SCIM
//! sent for an address that already had an account. Keeping the two apart is
//! what stops an identity provider from taking over an account it does not own.

use super::{now_secs, AccountError, AccountStore};
use rusqlite::{params, OptionalExtension};

/// A provisioned user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScimUser {
    /// The SCIM id.
    pub id: String,
    /// `userName`, lowercased.
    pub user_name: String,
    /// `externalId`.
    pub external_id: Option<String>,
    /// `displayName`.
    pub display_name: Option<String>,
    /// `name.givenName`.
    pub given_name: Option<String>,
    /// `name.familyName`.
    pub family_name: Option<String>,
    /// The primary email.
    pub email: String,
    /// `active`.
    pub active: bool,
    /// The account it points at.
    pub account_id: Option<String>,
    /// When it was provisioned.
    pub created_at: i64,
    /// When it last changed.
    pub updated_at: i64,
    /// Its groups, as id and display name.
    pub groups: Vec<(String, String)>,
}

/// A provisioned group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScimGroup {
    /// The SCIM id.
    pub id: String,
    /// `displayName`.
    pub display_name: String,
    /// `externalId`.
    pub external_id: Option<String>,
    /// Its members, as user id and `userName`.
    pub members: Vec<(String, String)>,
    /// When it was provisioned.
    pub created_at: i64,
    /// When it last changed.
    pub updated_at: i64,
}

/// The writable fields of a user.
#[derive(Debug, Clone, Default)]
pub struct ScimUserFields {
    /// `userName`.
    pub user_name: String,
    /// `externalId`.
    pub external_id: Option<String>,
    /// `displayName`.
    pub display_name: Option<String>,
    /// `name.givenName`.
    pub given_name: Option<String>,
    /// `name.familyName`.
    pub family_name: Option<String>,
    /// The primary email.
    pub email: String,
    /// `active`.
    pub active: bool,
}

/// One `attribute eq "value"` term of a filter, with the attribute already
/// mapped to a column this module chose. Never a column the caller named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScimTerm {
    /// `userName`.
    UserName(String),
    /// `externalId`.
    ExternalId(String),
    /// `emails.value` or `emails`.
    Email(String),
    /// `displayName`, for groups.
    DisplayName(String),
    /// `id`.
    Id(String),
}

const USER_COLUMNS: &str = "id, user_name, external_id, display_name, given_name, family_name,
     email, active, account_id, created_at, updated_at";

fn user_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ScimUser> {
    Ok(ScimUser {
        id: r.get(0)?,
        user_name: r.get(1)?,
        external_id: r.get(2)?,
        display_name: r.get(3)?,
        given_name: r.get(4)?,
        family_name: r.get(5)?,
        email: r.get(6)?,
        active: r.get::<_, i64>(7)? != 0,
        account_id: r.get(8)?,
        created_at: r.get(9)?,
        updated_at: r.get(10)?,
        groups: Vec::new(),
    })
}

fn terms_sql(terms: &[ScimTerm], users: bool) -> (String, Vec<String>) {
    let mut sql = String::new();
    let mut args = Vec::new();
    for t in terms {
        let (col, v) = match (t, users) {
            (ScimTerm::UserName(v), true) => ("user_name", v.to_lowercase()),
            (ScimTerm::ExternalId(v), _) => ("external_id", v.clone()),
            (ScimTerm::Email(v), true) => ("email", v.to_lowercase()),
            (ScimTerm::DisplayName(v), false) => ("display_name", v.clone()),
            (ScimTerm::DisplayName(v), true) => ("display_name", v.clone()),
            (ScimTerm::Id(v), _) => ("id", v.clone()),
            // A term that cannot apply to this resource matches nothing.
            _ => ("1", "0".to_owned()),
        };
        args.push(v);
        sql.push_str(&format!(" AND {col} = ?{}", args.len() + 1));
    }
    (sql, args)
}

impl AccountStore {
    fn user_groups(
        &self,
        conn: &rusqlite::Connection,
        user_id: &str,
    ) -> rusqlite::Result<Vec<(String, String)>> {
        let mut stmt = conn.prepare_cached(
            "SELECT g.id, g.display_name FROM scim_group_members gm JOIN scim_groups g ON g.id = gm.group_id
             WHERE gm.user_id = ?1 ORDER BY g.display_name",
        )?;
        let rows = stmt.query_map([user_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect()
    }

    /// A page of an organisation's users matching every term, and the total.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be read.
    pub fn scim_users(
        &self,
        org_id: &str,
        terms: &[ScimTerm],
        start: u32,
        count: u32,
    ) -> Result<(u32, Vec<ScimUser>), AccountError> {
        let conn = self.conn()?;
        let (filter, args) = terms_sql(terms, true);
        let mut bind: Vec<&dyn rusqlite::ToSql> = vec![&org_id];
        for a in &args {
            bind.push(a);
        }
        let total: i64 = conn.query_row(
            &format!("SELECT COUNT(*) FROM scim_users WHERE org_id = ?1{filter}"),
            bind.as_slice(),
            |r| r.get(0),
        )?;
        let sql = format!(
            "SELECT {USER_COLUMNS} FROM scim_users WHERE org_id = ?1{filter}
             ORDER BY created_at, id LIMIT {count} OFFSET {}",
            start.saturating_sub(1)
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut users: Vec<ScimUser> =
            stmt.query_map(bind.as_slice(), user_row)?.collect::<Result<_, _>>()?;
        for u in &mut users {
            u.groups = self.user_groups(&conn, &u.id)?;
        }
        Ok((u32::try_from(total).unwrap_or(0), users))
    }

    /// One user of an organisation.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be read.
    pub fn scim_user(&self, org_id: &str, id: &str) -> Result<Option<ScimUser>, AccountError> {
        Ok(self.scim_users(org_id, &[ScimTerm::Id(id.to_owned())], 1, 1)?.1.into_iter().next())
    }

    /// Provision a user. `None` when the organisation has one by that
    /// `userName` already.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn insert_scim_user(
        &self,
        org_id: &str,
        f: &ScimUserFields,
        account_id: Option<&str>,
    ) -> Result<Option<ScimUser>, AccountError> {
        let id = super::identifier("usr_", 12);
        let now = now_secs();
        {
            let conn = self.conn()?;
            let taken: Option<i64> = conn
                .query_row(
                    "SELECT 1 FROM scim_users WHERE org_id = ?1 AND user_name = ?2",
                    params![org_id, f.user_name.to_lowercase()],
                    |r| r.get(0),
                )
                .optional()?;
            if taken.is_some() {
                return Ok(None);
            }
            conn.execute(
                "INSERT INTO scim_users (id, org_id, user_name, external_id, display_name, given_name,
                                         family_name, email, active, account_id, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
                params![
                    id,
                    org_id,
                    f.user_name.to_lowercase(),
                    f.external_id,
                    f.display_name,
                    f.given_name,
                    f.family_name,
                    f.email.to_lowercase(),
                    f.active,
                    account_id,
                    now
                ],
            )?;
        }
        self.scim_user(org_id, &id)
    }

    /// Replace a user's fields. `false` when the organisation has no such user,
    /// or another user holds the new `userName`.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn update_scim_user(
        &self,
        org_id: &str,
        id: &str,
        f: &ScimUserFields,
    ) -> Result<bool, AccountError> {
        let conn = self.conn()?;
        let clash: Option<String> = conn
            .query_row(
                "SELECT id FROM scim_users WHERE org_id = ?1 AND user_name = ?2",
                params![org_id, f.user_name.to_lowercase()],
                |r| r.get(0),
            )
            .optional()?;
        if clash.is_some_and(|c| c != id) {
            return Ok(false);
        }
        let n = conn.execute(
            "UPDATE scim_users SET user_name = ?3, external_id = ?4, display_name = ?5,
                given_name = ?6, family_name = ?7, email = ?8, active = ?9, updated_at = ?10
             WHERE id = ?1 AND org_id = ?2",
            params![
                id,
                org_id,
                f.user_name.to_lowercase(),
                f.external_id,
                f.display_name,
                f.given_name,
                f.family_name,
                f.email.to_lowercase(),
                f.active,
                now_secs()
            ],
        )?;
        Ok(n > 0)
    }

    /// Point a user at an account.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn link_scim_user(
        &self,
        org_id: &str,
        id: &str,
        account_id: &str,
    ) -> Result<(), AccountError> {
        self.conn()?.execute(
            "UPDATE scim_users SET account_id = ?3 WHERE id = ?1 AND org_id = ?2",
            params![id, org_id, account_id],
        )?;
        Ok(())
    }

    /// Forget a user. `false` when the organisation has no such user.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn delete_scim_user(&self, org_id: &str, id: &str) -> Result<bool, AccountError> {
        let conn = self.conn()?;
        conn.execute(
            "DELETE FROM org_invites WHERE org_id = ?1 AND scim_user_id = ?2",
            params![org_id, id],
        )?;
        Ok(conn
            .execute("DELETE FROM scim_users WHERE id = ?1 AND org_id = ?2", params![id, org_id])?
            > 0)
    }

    /// A page of an organisation's groups matching every term, and the total.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be read.
    pub fn scim_groups(
        &self,
        org_id: &str,
        terms: &[ScimTerm],
        start: u32,
        count: u32,
    ) -> Result<(u32, Vec<ScimGroup>), AccountError> {
        let conn = self.conn()?;
        let (filter, args) = terms_sql(terms, false);
        let mut bind: Vec<&dyn rusqlite::ToSql> = vec![&org_id];
        for a in &args {
            bind.push(a);
        }
        let total: i64 = conn.query_row(
            &format!("SELECT COUNT(*) FROM scim_groups WHERE org_id = ?1{filter}"),
            bind.as_slice(),
            |r| r.get(0),
        )?;
        let sql = format!(
            "SELECT id, display_name, external_id, created_at, updated_at FROM scim_groups
             WHERE org_id = ?1{filter} ORDER BY created_at, id LIMIT {count} OFFSET {}",
            start.saturating_sub(1)
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut groups: Vec<ScimGroup> = stmt
            .query_map(bind.as_slice(), |r| {
                Ok(ScimGroup {
                    id: r.get(0)?,
                    display_name: r.get(1)?,
                    external_id: r.get(2)?,
                    members: Vec::new(),
                    created_at: r.get(3)?,
                    updated_at: r.get(4)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        let mut members = conn.prepare(
            "SELECT u.id, u.user_name FROM scim_group_members gm JOIN scim_users u ON u.id = gm.user_id
             WHERE gm.group_id = ?1 ORDER BY u.user_name",
        )?;
        for g in &mut groups {
            g.members = members
                .query_map([&g.id], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<Result<_, _>>()?;
        }
        Ok((u32::try_from(total).unwrap_or(0), groups))
    }

    /// One group of an organisation.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be read.
    pub fn scim_group(&self, org_id: &str, id: &str) -> Result<Option<ScimGroup>, AccountError> {
        Ok(self.scim_groups(org_id, &[ScimTerm::Id(id.to_owned())], 1, 1)?.1.into_iter().next())
    }

    /// Provision a group. `None` when the organisation has one by that name.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn insert_scim_group(
        &self,
        org_id: &str,
        display_name: &str,
        external_id: Option<&str>,
    ) -> Result<Option<String>, AccountError> {
        let id = super::identifier("grp_", 12);
        let now = now_secs();
        let conn = self.conn()?;
        let taken: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM scim_groups WHERE org_id = ?1 AND display_name = ?2",
                params![org_id, display_name],
                |r| r.get(0),
            )
            .optional()?;
        if taken.is_some() {
            return Ok(None);
        }
        conn.execute(
            "INSERT INTO scim_groups (id, org_id, display_name, external_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            params![id, org_id, display_name, external_id, now],
        )?;
        Ok(Some(id))
    }

    /// Rename a group. `false` when there is no such group or the name is taken.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn update_scim_group(
        &self,
        org_id: &str,
        id: &str,
        display_name: &str,
        external_id: Option<&str>,
    ) -> Result<bool, AccountError> {
        let conn = self.conn()?;
        let clash: Option<String> = conn
            .query_row(
                "SELECT id FROM scim_groups WHERE org_id = ?1 AND display_name = ?2",
                params![org_id, display_name],
                |r| r.get(0),
            )
            .optional()?;
        if clash.is_some_and(|c| c != id) {
            return Ok(false);
        }
        Ok(conn.execute(
            "UPDATE scim_groups SET display_name = ?3, external_id = ?4, updated_at = ?5
             WHERE id = ?1 AND org_id = ?2",
            params![id, org_id, display_name, external_id, now_secs()],
        )? > 0)
    }

    /// Change a group's members. Ids that are not users of this organisation
    /// are ignored, so one organisation's token cannot reach another's users.
    /// Returns every user whose groups changed.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn change_group_members(
        &self,
        org_id: &str,
        group_id: &str,
        replace: bool,
        add: &[String],
        remove: &[String],
    ) -> Result<Vec<String>, AccountError> {
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        let mut touched: Vec<String> = Vec::new();
        if replace {
            let mut stmt =
                tx.prepare("SELECT user_id FROM scim_group_members WHERE group_id = ?1")?;
            let old: Vec<String> =
                stmt.query_map([group_id], |r| r.get(0))?.collect::<Result<_, _>>()?;
            drop(stmt);
            touched.extend(old);
            tx.execute("DELETE FROM scim_group_members WHERE group_id = ?1", [group_id])?;
        }
        for u in remove {
            if tx.execute(
                "DELETE FROM scim_group_members WHERE group_id = ?1 AND user_id = ?2",
                params![group_id, u],
            )? > 0
            {
                touched.push(u.clone());
            }
        }
        for u in add {
            let n = tx.execute(
                "INSERT INTO scim_group_members (group_id, user_id)
                 SELECT ?1, id FROM scim_users WHERE id = ?2 AND org_id = ?3
                 ON CONFLICT DO NOTHING",
                params![group_id, u, org_id],
            )?;
            if n > 0 || replace {
                touched.push(u.clone());
            }
        }
        tx.execute(
            "UPDATE scim_groups SET updated_at = ?2 WHERE id = ?1",
            params![group_id, now_secs()],
        )?;
        tx.commit()?;
        touched.sort();
        touched.dedup();
        Ok(touched)
    }

    /// Delete a group, returning its members.
    ///
    /// # Errors
    /// Returns an [`AccountError`] when the file cannot be written.
    pub fn delete_scim_group(
        &self,
        org_id: &str,
        id: &str,
    ) -> Result<Option<Vec<String>>, AccountError> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT gm.user_id FROM scim_group_members gm JOIN scim_groups g ON g.id = gm.group_id
             WHERE g.id = ?1 AND g.org_id = ?2",
        )?;
        let members: Vec<String> =
            stmt.query_map(params![id, org_id], |r| r.get(0))?.collect::<Result<_, _>>()?;
        drop(stmt);
        let n = conn.execute(
            "DELETE FROM scim_groups WHERE id = ?1 AND org_id = ?2",
            params![id, org_id],
        )?;
        Ok((n > 0).then_some(members))
    }
}
