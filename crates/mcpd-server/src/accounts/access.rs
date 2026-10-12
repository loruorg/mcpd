//! Who reaches which project, held in memory.
//!
//! Authorising a request must stay a map lookup, as it was when an account had
//! one project. So every account's organisations, its effective role on each
//! project and its own project are folded into one entry here, and the entry is
//! rebuilt from the file whenever something that changes it is written: a
//! membership, a role, a project role, a project, an organisation. Rebuilding
//! one account reads a handful of rows, so a change touches only the accounts
//! it concerns, never the whole map.

use super::roles::{effective, ProjectAccess, Role};
use rusqlite::{params, Connection};
use std::collections::{BTreeSet, HashMap};

/// One account's reach.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AccountAccess {
    /// Organisation id to the account's role in it.
    pub orgs: HashMap<String, Role>,
    /// Project to the organisation that owns it and the account's effective
    /// role there. A project the account cannot reach is absent.
    pub projects: HashMap<String, (String, Role)>,
    /// The project created with the account, when it has one.
    pub home: Option<String>,
    /// Deactivated by its organisation. Nothing it holds works.
    pub disabled: bool,
}

impl AccountAccess {
    /// The projects where the account's role carries `scope`, within one
    /// organisation when `org` is set.
    pub fn reach(&self, scope: crate::auth::Scope, org: Option<&str>) -> BTreeSet<String> {
        if self.disabled {
            return BTreeSet::new();
        }
        self.projects
            .iter()
            .filter(|(_, (o, role))| org.is_none_or(|want| want == o) && role.permits(scope))
            .map(|(p, _)| p.clone())
            .collect()
    }
}

/// Read one account's reach from the file. `None` when the account is gone.
pub(crate) fn load(conn: &Connection, account_id: &str) -> rusqlite::Result<Option<AccountAccess>> {
    let head: Option<(Option<String>, Option<i64>)> = {
        let mut stmt =
            conn.prepare_cached("SELECT project, disabled_at FROM accounts WHERE id = ?1")?;
        let mut rows = stmt.query([account_id])?;
        match rows.next()? {
            Some(r) => Some((r.get(0)?, r.get(1)?)),
            None => None,
        }
    };
    let Some((home, disabled)) = head else { return Ok(None) };
    let mut access =
        AccountAccess { home, disabled: disabled.is_some(), ..AccountAccess::default() };
    let mut stmt = conn.prepare_cached(
        "SELECT m.org_id, m.role, p.project, pr.role
         FROM org_members m
         LEFT JOIN org_projects p ON p.org_id = m.org_id
         LEFT JOIN project_roles pr ON pr.project = p.project AND pr.account_id = m.account_id
         WHERE m.account_id = ?1",
    )?;
    let rows = stmt.query_map(params![account_id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, Option<String>>(2)?,
            r.get::<_, Option<String>>(3)?,
        ))
    })?;
    for row in rows {
        let (org, role, project, project_role) = row?;
        // A role this build cannot read grants nothing, rather than a guess.
        let Some(role) = Role::parse(&role) else { continue };
        access.orgs.insert(org.clone(), role);
        let Some(project) = project else { continue };
        let pr = match project_role.as_deref() {
            None => None,
            Some(text) => match ProjectAccess::parse(text) {
                Some(p) => Some(p),
                None => Some(ProjectAccess::None),
            },
        };
        if let Some(eff) = effective(role, pr) {
            access.projects.insert(project, (org, eff));
        }
    }
    Ok(Some(access))
}

/// Every account's reach, for opening the file.
pub(crate) fn load_all(conn: &Connection) -> rusqlite::Result<HashMap<String, AccountAccess>> {
    let ids: Vec<String> = {
        let mut stmt = conn.prepare("SELECT id FROM accounts")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.collect::<Result<_, _>>()?
    };
    let mut out = HashMap::with_capacity(ids.len());
    for id in ids {
        if let Some(a) = load(conn, &id)? {
            out.insert(id, a);
        }
    }
    Ok(out)
}
