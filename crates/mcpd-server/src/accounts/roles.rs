//! Roles, and how each one maps onto the six key scopes.
//!
//! A role is not a seventh kind of permission. It is a set of the scopes the
//! daemon already enforces, so an endpoint never asks "is this an admin": it
//! asks for a scope, as it always has, and a member's role on the project is
//! what decides whether their session or key carries it. That keeps one
//! authorisation rule for configured keys, account keys and sessions.
//!
//! | Role | Scopes |
//! | --- | --- |
//! | viewer | read |
//! | member | ingest, read, write, execute, worker |
//! | admin | all six |
//! | owner | all six |
//!
//! Admin and owner differ in what they may do to the organisation, not to its
//! data: only an owner may grant or remove `owner`, change quotas, or delete it.

use crate::auth::Scope;
use mcpd_core::{OrgRole, ProjectRole};

/// A role in an organisation, ordered by reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Role {
    /// Reads.
    Viewer,
    /// Everything but `admin`.
    Member,
    /// Everything, and manages members below owner, projects, SSO and SCIM.
    Admin,
    /// Everything, including owners, quotas and deleting the organisation.
    Owner,
}

impl Role {
    /// Every role, lowest first.
    pub const ALL: &'static [Self] = &[Self::Viewer, Self::Member, Self::Admin, Self::Owner];

    /// The stored and wire name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Viewer => "viewer",
            Self::Member => "member",
            Self::Admin => "admin",
            Self::Owner => "owner",
        }
    }

    /// The role a stored name means, or `None`. A name this build does not know
    /// grants nothing.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|r| r.as_str() == s.trim().to_ascii_lowercase())
    }

    /// Whether this role carries `scope`.
    #[must_use]
    pub const fn permits(self, scope: Scope) -> bool {
        match self {
            Self::Viewer => matches!(scope, Scope::Read),
            Self::Member => !matches!(scope, Scope::Admin),
            Self::Admin | Self::Owner => true,
        }
    }

    /// Whether this role may manage the organisation: members, projects, SSO,
    /// SCIM and other people's keys.
    #[must_use]
    pub const fn manages(self) -> bool {
        matches!(self, Self::Admin | Self::Owner)
    }

    /// The wire enum.
    #[must_use]
    pub const fn to_wire(self) -> OrgRole {
        match self {
            Self::Viewer => OrgRole::Viewer,
            Self::Member => OrgRole::Member,
            Self::Admin => OrgRole::Admin,
            Self::Owner => OrgRole::Owner,
        }
    }

    /// From the wire enum.
    #[must_use]
    pub const fn from_wire(r: OrgRole) -> Self {
        match r {
            OrgRole::Viewer => Self::Viewer,
            OrgRole::Member => Self::Member,
            OrgRole::Admin => Self::Admin,
            OrgRole::Owner => Self::Owner,
        }
    }
}

/// A role on one project, replacing the organisation role there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectAccess {
    /// No access to the project.
    None,
    /// This role on the project.
    Role(Role),
}

impl ProjectAccess {
    /// The stored and wire name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Role(r) => r.as_str(),
        }
    }

    /// Parse a stored name. `owner` is not a project role, and is refused like
    /// any other unknown name.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "none" => Some(Self::None),
            "owner" => None,
            other => Role::parse(other).map(Self::Role),
        }
    }

    /// The wire enum.
    #[must_use]
    pub const fn to_wire(self) -> ProjectRole {
        match self {
            Self::None => ProjectRole::None,
            Self::Role(Role::Viewer) => ProjectRole::Viewer,
            Self::Role(Role::Member) => ProjectRole::Member,
            Self::Role(Role::Admin | Role::Owner) => ProjectRole::Admin,
        }
    }

    /// From the wire enum.
    #[must_use]
    pub const fn from_wire(r: ProjectRole) -> Self {
        match r {
            ProjectRole::None => Self::None,
            ProjectRole::Viewer => Self::Role(Role::Viewer),
            ProjectRole::Member => Self::Role(Role::Member),
            ProjectRole::Admin => Self::Role(Role::Admin),
        }
    }
}

/// The role a member has on one project: their organisation role, unless a
/// project role replaces it. Admins and owners keep their reach everywhere, so
/// a project role cannot lock the people who manage the organisation out of
/// part of it.
#[must_use]
pub fn effective(org_role: Role, project: Option<ProjectAccess>) -> Option<Role> {
    if org_role.manages() {
        return Some(org_role);
    }
    match project {
        None => Some(org_role),
        Some(ProjectAccess::None) => None,
        Some(ProjectAccess::Role(r)) => Some(r),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole matrix, written out. A change to what a role reaches changes
    /// this table, which is the point: nobody widens a role by accident.
    #[test]
    fn every_role_reaches_exactly_its_scopes() {
        use Scope::*;
        let table: &[(Role, &[Scope])] = &[
            (Role::Viewer, &[Read]),
            (Role::Member, &[Ingest, Read, Write, Execute, Worker]),
            (Role::Admin, &[Ingest, Read, Write, Admin, Execute, Worker]),
            (Role::Owner, &[Ingest, Read, Write, Admin, Execute, Worker]),
        ];
        for (role, allowed) in table {
            for scope in Scope::ALL {
                assert_eq!(role.permits(*scope), allowed.contains(scope), "{role:?} {scope:?}");
            }
        }
    }

    #[test]
    fn a_project_role_replaces_a_lower_org_role_and_never_an_admin() {
        let pr = |r| Some(ProjectAccess::Role(r));
        assert_eq!(effective(Role::Viewer, pr(Role::Admin)), Some(Role::Admin));
        assert_eq!(effective(Role::Member, Some(ProjectAccess::None)), None);
        assert_eq!(effective(Role::Member, pr(Role::Viewer)), Some(Role::Viewer));
        assert_eq!(effective(Role::Admin, Some(ProjectAccess::None)), Some(Role::Admin));
        assert_eq!(effective(Role::Owner, pr(Role::Viewer)), Some(Role::Owner));
        assert_eq!(effective(Role::Member, None), Some(Role::Member));
    }

    #[test]
    fn names_round_trip_and_owner_is_no_project_role() {
        for r in Role::ALL {
            assert_eq!(Role::parse(r.as_str()), Some(*r));
            assert_eq!(Role::from_wire(r.to_wire()), *r);
        }
        assert_eq!(ProjectAccess::parse("owner"), None);
        assert_eq!(ProjectAccess::parse("none"), Some(ProjectAccess::None));
        assert_eq!(Role::parse("superuser"), None);
    }
}
