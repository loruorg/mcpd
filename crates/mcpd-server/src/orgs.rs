//! Organisations: members, roles, invitations, projects, keys and usage.
//!
//! Every door here takes a session, never an API key, and every door that
//! names an organisation answers 404 to an account that is not a member of it,
//! so the answer does not say which organisation ids exist. A single sign-on
//! session sees only the organisation it came from.
//!
//! Every change to membership, roles, projects and keys is written to the
//! audit log against the organisation's id, with the acting account's email as
//! the subject, through `query::audited_as`.

use crate::accounts::roles::{ProjectAccess, Role};
use crate::accounts::{self, blocking, store, AccountStore, Credential, OrgRow, SessionMeta};
use crate::auth::authorize_session;
use crate::error::{ApiError, ApiResult};
use crate::query::{audited_as, Peer};
use crate::state::{AppState, ProjectQuotas, QUOTA_MAX};
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::routing::{delete, get, patch, post, put};
use axum::{Json, Router};
use mcpd_core::{
    AccountKey, AuditEvent, CreateInviteRequest, CreateOrgRequest, CreateProjectRequest,
    DeleteResult, GroupRole, InviteAcceptRequest, InviteAccepted, InviteLookup, Org, OrgDeletion,
    OrgInvite, OrgMember, OrgProject, OrgUsage, SetProjectRoleRequest, TokenRequest,
    UpdateMemberRequest, UpdateOrgRequest,
};
use std::sync::Arc;
use time::OffsetDateTime;

/// Longest organisation name or project label, in characters.
pub const MAX_NAME_CHARS: usize = 100;

/// The caller's standing in one organisation.
pub(crate) struct Member {
    pub accounts: Arc<AccountStore>,
    pub account_id: String,
    pub email: String,
    pub role: Role,
}

/// The organisation named, as the caller sees it, or the 404 a non-member gets.
pub(crate) async fn member_of(
    state: &AppState,
    credential: &Credential,
    org_id: &str,
) -> ApiResult<Member> {
    let accounts = store(state)?;
    let not_found = || ApiError::NotFound(format!("you belong to no organisation {org_id:?}"));
    if credential.session_org().is_some_and(|o| o != org_id) {
        return Err(not_found());
    }
    let role = accounts.role_in(&credential.account_id, org_id).ok_or_else(not_found)?;
    let a = Arc::clone(&accounts);
    let id = credential.account_id.clone();
    let email = blocking(move || a.account(&id)).await?.email;
    Ok(Member { accounts, account_id: credential.account_id.clone(), email, role })
}

fn need_manager(m: &Member) -> ApiResult<()> {
    if m.role.manages() {
        Ok(())
    } else {
        Err(ApiError::Forbidden(
            "this needs the admin or owner role in the organisation".to_owned(),
        ))
    }
}

fn need_owner(m: &Member) -> ApiResult<()> {
    if m.role == Role::Owner {
        Ok(())
    } else {
        Err(ApiError::Forbidden("only an owner of the organisation may do this".to_owned()))
    }
}

/// An organisation as the wire shows it, with the defaults it inherits filled in.
pub(crate) fn org_to_wire(state: &AppState, row: OrgRow, role: Role) -> Org {
    let settings = &state.config.account_settings;
    Org {
        id: row.id,
        name: row.name,
        role: role.to_wire(),
        created_at: OffsetDateTime::from_unix_timestamp(row.created_at)
            .unwrap_or(OffsetDateTime::UNIX_EPOCH),
        quota_per_minute: row.quota_per_minute.or(settings.org_quota_per_minute),
        retention_days: row
            .retention_days
            .or(settings.org_retention_days)
            .or(state.config.retention_days),
        members: row.members,
        projects: row.projects,
        managed_by_config: row.config_managed,
    }
}

/// Push every organisation's ceiling into the ingest counters.
pub fn sync_quotas_with(quotas: &ProjectQuotas, accounts: &AccountStore, default: Option<u32>) {
    match accounts.org_ceilings(default) {
        Ok(c) => quotas.set_org_ceilings(c),
        Err(e) => tracing::error!("organisation ceilings could not be read: {e}"),
    }
}

/// [`sync_quotas_with`], from the daemon's state, after a change to projects
/// or ceilings.
pub fn sync_quotas(state: &AppState) {
    if let Some(accounts) = &state.accounts {
        sync_quotas_with(
            &state.quotas,
            accounts,
            state.config.account_settings.org_quota_per_minute,
        );
    }
}

/// Erase a project's stored signals and runs, through the same paths retention
/// uses. A run still going is cancelled first, because retention leaves
/// unfinished runs alone and a deleted project must not keep any.
pub async fn erase_project(state: &AppState, project: &str) {
    let far = OffsetDateTime::now_utc() + time::Duration::days(1);
    if let Some(runs) = &state.runs {
        for _ in 0..100 {
            let page = runs
                .list_runs(
                    mcpd_runs::RunFilter {
                        project: project.to_owned(),
                        status: Some(mcpd_runs::RunStatus::Running),
                        definition_id: None,
                    },
                    200,
                    None,
                )
                .await;
            let Ok(page) = page else { break };
            if page.runs.is_empty() {
                break;
            }
            for run in page.runs {
                let _ = runs.cancel(&run.run_id, OffsetDateTime::now_utc()).await;
            }
        }
        if let Err(e) = runs.apply_retention(project, far).await {
            tracing::error!(project, "erasing the project's runs failed: {e}");
        }
    }
    match state.store.enforce_retention(project, far).await {
        Ok(n) => tracing::info!(project, "erased {n} rows of a deleted project"),
        Err(e) => tracing::error!(project, "erasing the project's signals failed: {e}"),
    }
}

fn name_of(raw: &str, field: &str) -> ApiResult<String> {
    let n = raw.trim();
    if n.is_empty() || n.chars().count() > MAX_NAME_CHARS || n.chars().any(char::is_control) {
        return Err(ApiError::BadParam {
            field: field.to_owned(),
            message: format!(
                "a {field} is 1 to {MAX_NAME_CHARS} characters with no control characters"
            ),
        });
    }
    Ok(n.to_owned())
}

fn not_member(account_id: &str) -> ApiError {
    ApiError::NotFound(format!("the organisation has no member {account_id:?}"))
}

fn grants_from_wire(g: &[mcpd_core::ProjectRoleGrant]) -> Vec<(String, ProjectAccess)> {
    g.iter().map(|x| (x.project.clone(), ProjectAccess::from_wire(x.role))).collect()
}

fn invite_mail(state: &AppState, to: &str, org_name: &str, inviter: Option<&str>, token: &str) {
    let url = accounts::link(state, &format!("invite={token}"));
    let who = inviter.map_or_else(String::new, |i| format!("{i} invited you. "));
    accounts::mail(
        state,
        to,
        &format!("Join {org_name} on mcpd"),
        format!(
            "{who}Open this link to join the organisation {org_name:?} on mcpd:\n\n{url}\n\nIt \
             works once, for seven days.\n"
        ),
        Some(&url),
    );
}

/// Send an invitation from outside a request: SCIM, and owners the
/// configuration file names.
pub(crate) fn send_invite_mail(state: &AppState, to: &str, org_name: &str, token: &str) {
    invite_mail(state, to, org_name, None, token);
}

/// Apply the organisations the configuration file declares: create or update
/// each, attach its projects, store its single sign-on and SCIM token, set its
/// group roles, and make its owners owners, inviting the ones with no account.
///
/// Run at startup, before the first request. Everything here is idempotent, so
/// a restart with the same file changes nothing, and an invitation is sent to
/// an owner only when none is waiting.
///
/// # Errors
/// A sentence naming the organisation and what is wrong with its block.
pub async fn apply_config(state: &Arc<AppState>) -> Result<(), String> {
    let accounts = store(state).map_err(|e| e.to_string())?;
    let run = |e: ApiError| e.to_string();
    for o in state.config.account_settings.orgs.clone() {
        let slug_ok = !o.slug.is_empty()
            && o.slug.len() <= 40
            && o.slug.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if !slug_ok {
            return Err(format!(
                "the config file declares an organisation with the slug {:?}; a slug is 1 to 40 \
                 lowercase letters, digits and hyphens",
                o.slug
            ));
        }
        let org_id = format!("org_{}", o.slug);
        let name = o.name.clone().unwrap_or_else(|| o.slug.clone());
        if o.quota_per_minute.is_some_and(|q| q == 0 || q > QUOTA_MAX) {
            return Err(format!(
                "organisation {:?}: quota_per_minute must be 1 to {QUOTA_MAX}",
                o.slug
            ));
        }
        let a = Arc::clone(&accounts);
        let (id, n, q, r) = (org_id.clone(), name.clone(), o.quota_per_minute, o.retention_days);
        blocking(move || a.upsert_config_org(&id, &n, q, r)).await.map_err(run)?;
        for p in &o.projects {
            let a = Arc::clone(&accounts);
            let (id, proj) = (org_id.clone(), p.clone());
            if !blocking(move || a.attach_project(&id, &proj)).await.map_err(run)? {
                return Err(format!(
                    "organisation {:?}: the project {p:?} belongs to another organisation",
                    o.slug
                ));
            }
        }
        match &o.sso {
            Some(sso) => {
                let provider = match sso.provider.as_deref().unwrap_or("oidc") {
                    "oidc" => mcpd_core::SsoProvider::Oidc,
                    "github" => mcpd_core::SsoProvider::Github,
                    other => {
                        return Err(format!(
                            "organisation {:?}: sso.provider must be oidc or github, got {other:?}",
                            o.slug
                        ))
                    }
                };
                let client_secret = match &sso.client_secret_env {
                    None => None,
                    Some(var) => Some(std::env::var(var).map_err(|_| {
                        format!(
                            "organisation {:?}: sso.client_secret_env names {var}, which is not set",
                            o.slug
                        )
                    })?),
                };
                let default_role = match sso.default_role.as_deref() {
                    None => mcpd_core::OrgRole::Member,
                    Some(r) => match Role::parse(r) {
                        Some(Role::Owner) | None => {
                            return Err(format!(
                            "organisation {:?}: sso.default_role must be admin, member or viewer",
                            o.slug
                        ))
                        }
                        Some(role) => role.to_wire(),
                    },
                };
                let req = mcpd_core::PutSsoConfigRequest {
                    provider,
                    issuer: sso.issuer.clone(),
                    client_id: sso.client_id.clone(),
                    client_secret,
                    domains: sso.domains.clone(),
                    enforced: sso.enforce,
                    default_role,
                    groups_claim: sso.groups_claim.clone(),
                };
                let settings = crate::sso::validate_settings(state, &org_id, &req, None, true)
                    .await
                    .map_err(|e| format!("organisation {:?}: {e}", o.slug))?;
                crate::sso::store_settings(state, settings)
                    .await
                    .map_err(|e| format!("organisation {:?}: {e}", o.slug))?;
            }
            None => {
                // Single sign-on removed from the file is removed here too.
                let a = Arc::clone(&accounts);
                let id = org_id.clone();
                blocking(move || {
                    if a.sso_settings(&id)?.is_some_and(|s| s.config_managed) {
                        a.delete_sso_settings(&id)?;
                    }
                    Ok(())
                })
                .await
                .map_err(run)?;
            }
        }
        if !o.group_roles.is_empty() {
            let mut wire = Vec::new();
            for (g, r) in &o.group_roles {
                let role = Role::parse(r).ok_or_else(|| {
                    format!(
                        "organisation {:?}: group_roles maps {g:?} to {r:?}, which is not a role",
                        o.slug
                    )
                })?;
                wire.push(GroupRole { group: g.clone(), role: role.to_wire() });
            }
            let roles = group_roles_from_wire(&wire)
                .map_err(|e| format!("organisation {:?}: {e}", o.slug))?;
            let a = Arc::clone(&accounts);
            let id = org_id.clone();
            blocking(move || a.put_group_roles(&id, &roles)).await.map_err(run)?;
        }
        if let Some(var) = &o.scim_token_env {
            let token = std::env::var(var).map_err(|_| {
                format!("organisation {:?}: scim_token_env names {var}, which is not set", o.slug)
            })?;
            if token.len() < 32 {
                return Err(format!(
                    "organisation {:?}: the SCIM token in {var} is shorter than 32 characters; use a \
                     long random value",
                    o.slug
                ));
            }
            let a = Arc::clone(&accounts);
            let id = org_id.clone();
            blocking(move || a.set_scim_token(&id, &token)).await.map_err(run)?;
        }
        for owner in &o.owners {
            let email = accounts::normalise_email(owner)
                .map_err(|e| format!("organisation {:?}: owners: {e}", o.slug))?;
            let a = Arc::clone(&accounts);
            let (id, e) = (org_id.clone(), email.clone());
            let token = blocking(move || {
                if let Some(acct) = a.account_id_for(&e)? {
                    a.upsert_member(&id, &acct, Role::Owner, "config")?;
                    return Ok(None);
                }
                if a.has_invite(&id, &e)? {
                    return Ok(None);
                }
                Ok(a.create_invite(&id, &e, Role::Owner, &[], None, None)?.map(|(_, t)| t))
            })
            .await
            .map_err(run)?;
            if let Some(token) = token {
                send_invite_mail(state, &email, &name, &token);
            }
        }
    }
    sync_quotas(state);
    Ok(())
}

/// `GET /v1/api/orgs`
pub async fn list_orgs(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> ApiResult<Json<Vec<Org>>> {
    let (credential, _) = authorize_session(&state, &headers)?;
    let accounts = store(&state)?;
    let a = Arc::clone(&accounts);
    let id = credential.account_id.clone();
    let rows = blocking(move || a.orgs_of(&id)).await?;
    let only = credential.session_org().map(str::to_owned);
    Ok(Json(
        rows.into_iter()
            .filter(|r| only.as_deref().is_none_or(|o| o == r.id))
            .filter_map(|r| {
                let role = accounts.role_in(&credential.account_id, &r.id)?;
                Some(org_to_wire(&state, r, role))
            })
            .collect(),
    ))
}

/// `POST /v1/api/orgs`
pub async fn create_org(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Json(req): Json<CreateOrgRequest>,
) -> ApiResult<Json<Org>> {
    let (credential, _) = authorize_session(&state, &headers)?;
    let accounts = store(&state)?;
    let name = name_of(&req.name, "name")?;
    let allowed = state.config.account_settings.allow_org_creation.unwrap_or(state.config.signup);
    if !allowed || credential.session_org().is_some() {
        return Err(ApiError::Forbidden(
            "this daemon does not let accounts create organisations; ask the operator, who sets \
             allow_org_creation in the accounts block of the configuration file"
                .to_owned(),
        ));
    }
    let a = Arc::clone(&accounts);
    let id = credential.account_id.clone();
    let row = blocking(move || a.create_org(&id, &name)).await?;
    let a = Arc::clone(&accounts);
    let id = credential.account_id.clone();
    let email = blocking(move || a.account(&id)).await?.email;
    let decision = row.ok_or_else(|| {
        ApiError::Forbidden(format!(
            "an account belongs to at most {} organisations",
            accounts::org_store::MAX_ORGS_PER_ACCOUNT
        ))
    });
    let org_id = decision.as_ref().map(|r| r.id.clone()).unwrap_or_default();
    let row =
        audited_as(&state, &headers, peer.0, decision, "org_create", &org_id, None, &email).await?;
    sync_quotas(&state);
    Ok(Json(org_to_wire(&state, row, Role::Owner)))
}

/// `GET /v1/api/orgs/{org_id}`
pub async fn get_org(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(org_id): Path<String>,
) -> ApiResult<Json<Org>> {
    let (credential, _) = authorize_session(&state, &headers)?;
    let m = member_of(&state, &credential, &org_id).await?;
    let a = Arc::clone(&m.accounts);
    let row = blocking(move || a.org_row(&org_id))
        .await?
        .ok_or_else(|| ApiError::NotFound("the organisation is gone".to_owned()))?;
    Ok(Json(org_to_wire(&state, row, m.role)))
}

/// `PATCH /v1/api/orgs/{org_id}`
pub async fn update_org(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Path(org_id): Path<String>,
    Json(req): Json<UpdateOrgRequest>,
) -> ApiResult<Json<Org>> {
    let (credential, _) = authorize_session(&state, &headers)?;
    let m = member_of(&state, &credential, &org_id).await?;
    let name = req.name.as_deref().map(|n| name_of(n, "name")).transpose()?;
    let settings = &state.config.account_settings;
    let quota = match req.quota_per_minute {
        None => None,
        Some(0) => Some(None),
        Some(n) => {
            let cap = settings.org_quota_per_minute.unwrap_or(QUOTA_MAX);
            if n > cap {
                return Err(ApiError::BadParam {
                    field: "quota_per_minute".to_owned(),
                    message: format!(
                        "this daemon allows an organisation at most {cap} signals per minute"
                    ),
                });
            }
            Some(Some(n))
        }
    };
    let retention = match req.retention_days {
        None => None,
        Some(0) => Some(None),
        Some(n) => {
            if let Some(cap) = settings.org_retention_days.or(state.config.retention_days) {
                if n > cap {
                    return Err(ApiError::BadParam {
                        field: "retention_days".to_owned(),
                        message: format!("this daemon keeps data at most {cap} days"),
                    });
                }
            }
            Some(Some(n))
        }
    };
    let decision =
        if quota.is_some() || retention.is_some() { need_owner(&m) } else { need_manager(&m) };
    audited_as(&state, &headers, peer.0, decision, "org_update", &org_id, None, &m.email).await?;
    let a = Arc::clone(&m.accounts);
    let id = org_id.clone();
    let row = blocking(move || {
        a.update_org(&id, name.as_deref(), quota, retention)?;
        a.org_row(&id)
    })
    .await?
    .ok_or_else(|| ApiError::NotFound("the organisation is gone".to_owned()))?;
    sync_quotas(&state);
    Ok(Json(org_to_wire(&state, row, m.role)))
}

/// `DELETE /v1/api/orgs/{org_id}`
pub async fn delete_org(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Path(org_id): Path<String>,
) -> ApiResult<Json<OrgDeletion>> {
    let (credential, _) = authorize_session(&state, &headers)?;
    let m = member_of(&state, &credential, &org_id).await?;
    let a = Arc::clone(&m.accounts);
    let id = org_id.clone();
    let (row, projects) = blocking(move || Ok((a.org_row(&id)?, a.org_projects(&id)?))).await?;
    let row = row.ok_or_else(|| ApiError::NotFound("the organisation is gone".to_owned()))?;
    let mut decision = need_owner(&m);
    if decision.is_ok() && row.config_managed {
        decision = Err(ApiError::Conflict(
            "the configuration file declares this organisation; remove it there".to_owned(),
        ));
    }
    if decision.is_ok() {
        for p in &projects {
            let a = Arc::clone(&m.accounts);
            let id = org_id.clone();
            let name = p.project.clone();
            // Any reason other than "last project" stops the deletion: a
            // project that is an account's own goes when that account does.
            if let Some(why) = blocking(move || a.project_delete_blocker(&id, &name)).await? {
                if !why.contains("at least one project") {
                    decision = Err(ApiError::Conflict(format!(
                        "the project {:?} cannot go with the organisation: {why}",
                        p.project
                    )));
                    break;
                }
            }
        }
    }
    audited_as(&state, &headers, peer.0, decision, "org_delete", &org_id, None, &m.email).await?;
    let a = Arc::clone(&m.accounts);
    let id = org_id.clone();
    let erased = blocking(move || a.delete_org(&id)).await?;
    for p in &erased {
        erase_project(&state, p).await;
    }
    sync_quotas(&state);
    Ok(Json(OrgDeletion { projects_erased: erased }))
}

/// `GET /v1/api/orgs/{org_id}/members`
pub async fn list_members(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(org_id): Path<String>,
) -> ApiResult<Json<Vec<OrgMember>>> {
    let (credential, _) = authorize_session(&state, &headers)?;
    let m = member_of(&state, &credential, &org_id).await?;
    let a = Arc::clone(&m.accounts);
    Ok(Json(blocking(move || a.list_members(&org_id)).await?))
}

/// `PATCH /v1/api/orgs/{org_id}/members/{account_id}`
pub async fn update_member(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Path((org_id, account_id)): Path<(String, String)>,
    Json(req): Json<UpdateMemberRequest>,
) -> ApiResult<Json<OrgMember>> {
    let (credential, _) = authorize_session(&state, &headers)?;
    let m = member_of(&state, &credential, &org_id).await?;
    let new = Role::from_wire(req.role);
    let current = m.accounts.role_in(&account_id, &org_id);
    let mut decision = need_manager(&m);
    if decision.is_ok() {
        decision = match current {
            None => Err(not_member(&account_id)),
            Some(cur) if (cur == Role::Owner || new == Role::Owner) && m.role != Role::Owner => {
                Err(ApiError::Forbidden(
                    "only an owner may grant or remove the owner role".to_owned(),
                ))
            }
            Some(Role::Owner) if new != Role::Owner => {
                let a = Arc::clone(&m.accounts);
                let id = org_id.clone();
                if blocking(move || a.owner_count(&id)).await? <= 1 {
                    Err(ApiError::Conflict(
                        "this is the organisation's last owner; make another member an owner first"
                            .to_owned(),
                    ))
                } else {
                    Ok(())
                }
            }
            Some(_) => Ok(()),
        };
    }
    audited_as(
        &state,
        &headers,
        peer.0,
        decision,
        "member_role",
        &org_id,
        Some(&account_id),
        &m.email,
    )
    .await?;
    let a = Arc::clone(&m.accounts);
    let member = blocking(move || {
        a.set_role(&org_id, &account_id, new)?;
        a.prune_keys(&org_id, &account_id)?;
        a.member(&org_id, &account_id)
    })
    .await?;
    member.map(Json).ok_or_else(|| ApiError::NotFound("the member is gone".to_owned()))
}

/// `DELETE /v1/api/orgs/{org_id}/members/{account_id}`
pub async fn remove_member(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Path((org_id, account_id)): Path<(String, String)>,
) -> ApiResult<Json<DeleteResult>> {
    let (credential, _) = authorize_session(&state, &headers)?;
    let m = member_of(&state, &credential, &org_id).await?;
    let target = m.accounts.role_in(&account_id, &org_id);
    let leaving = account_id == m.account_id;
    let mut decision = match target {
        None => Err(not_member(&account_id)),
        Some(_) if leaving => Ok(()),
        Some(Role::Owner) => need_owner(&m),
        Some(_) => need_manager(&m),
    };
    if decision.is_ok() && target == Some(Role::Owner) {
        let a = Arc::clone(&m.accounts);
        let id = org_id.clone();
        if blocking(move || a.owner_count(&id)).await? <= 1 {
            decision = Err(ApiError::Conflict(
                "this is the organisation's last owner; make another member an owner first, or \
                 delete the organisation"
                    .to_owned(),
            ));
        }
    }
    audited_as(
        &state,
        &headers,
        peer.0,
        decision,
        "member_remove",
        &org_id,
        Some(&account_id),
        &m.email,
    )
    .await?;
    let a = Arc::clone(&m.accounts);
    let removed = blocking(move || a.remove_member(&org_id, &account_id)).await?;
    Ok(Json(DeleteResult { deleted: u32::from(removed) }))
}

/// `POST /v1/api/orgs/{org_id}/members/{account_id}/project-roles`
pub async fn set_project_role(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Path((org_id, account_id)): Path<(String, String)>,
    Json(req): Json<SetProjectRoleRequest>,
) -> ApiResult<Json<OrgMember>> {
    let (credential, _) = authorize_session(&state, &headers)?;
    let m = member_of(&state, &credential, &org_id).await?;
    let decision = need_manager(&m);
    audited_as(
        &state,
        &headers,
        peer.0,
        decision,
        "member_project_role",
        &org_id,
        Some(&account_id),
        &m.email,
    )
    .await?;
    let role = req.role.map(ProjectAccess::from_wire);
    let a = Arc::clone(&m.accounts);
    let project = req.project;
    let (ok, member) = blocking(move || {
        let ok = a.set_project_role(&org_id, &account_id, &project, role)?;
        a.prune_keys(&org_id, &account_id)?;
        Ok((ok, a.member(&org_id, &account_id)?))
    })
    .await?;
    if !ok {
        return Err(ApiError::NotFound(
            "the organisation owns no such project, or has no such member".to_owned(),
        ));
    }
    member.map(Json).ok_or_else(|| ApiError::NotFound("the member is gone".to_owned()))
}

/// `GET /v1/api/orgs/{org_id}/invites`
pub async fn list_invites(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(org_id): Path<String>,
) -> ApiResult<Json<Vec<OrgInvite>>> {
    let (credential, _) = authorize_session(&state, &headers)?;
    let m = member_of(&state, &credential, &org_id).await?;
    need_manager(&m)?;
    let a = Arc::clone(&m.accounts);
    Ok(Json(blocking(move || a.list_invites(&org_id)).await?))
}

/// `POST /v1/api/orgs/{org_id}/invites`
pub async fn create_invite(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Path(org_id): Path<String>,
    Json(req): Json<CreateInviteRequest>,
) -> ApiResult<Json<OrgInvite>> {
    let (credential, _) = authorize_session(&state, &headers)?;
    let m = member_of(&state, &credential, &org_id).await?;
    accounts::require_mail(&state)?;
    let email = accounts::normalise_email(&req.email)?;
    let role = Role::from_wire(req.role);
    let grants = grants_from_wire(req.project_roles.as_deref().unwrap_or_default());
    let mut decision = need_manager(&m);
    if decision.is_ok() && role == Role::Owner && m.role != Role::Owner {
        decision = Err(ApiError::Forbidden("only an owner may invite an owner".to_owned()));
    }
    if decision.is_ok() {
        let a = Arc::clone(&m.accounts);
        let (id, wanted) = (org_id.clone(), email.clone());
        let projects: Vec<String> =
            blocking(move || a.org_projects(&id)).await?.into_iter().map(|p| p.project).collect();
        if let Some((bad, _)) = grants.iter().find(|(p, _)| !projects.contains(p)) {
            decision = Err(ApiError::BadParam {
                field: "project_roles".to_owned(),
                message: format!("the organisation owns no project {bad:?}"),
            });
        } else {
            let a = Arc::clone(&m.accounts);
            let id = org_id.clone();
            let existing = blocking(move || a.account_id_for(&wanted)).await?;
            if existing.is_some_and(|acct| m.accounts.role_in(&acct, &id).is_some()) {
                decision = Err(ApiError::Conflict("that address is already a member".to_owned()));
            }
        }
    }
    audited_as(&state, &headers, peer.0, decision, "member_invite", &org_id, None, &m.email)
        .await?;
    let a = Arc::clone(&m.accounts);
    let (id, wanted, by) = (org_id.clone(), email.clone(), m.account_id.clone());
    let made = blocking(move || {
        let made = a.create_invite(&id, &wanted, role, &grants, Some(&by), None)?;
        Ok((made, a.org_row(&id)?))
    })
    .await?;
    let (Some((mut invite, token)), Some(row)) = made else {
        return Err(ApiError::Forbidden(format!(
            "the organisation has {} invitations waiting; revoke some first",
            accounts::org_store::MAX_INVITES_PER_ORG
        )));
    };
    invite_mail(&state, &email, &row.name, Some(&m.email), &token);
    invite.invited_by = Some(m.email);
    Ok(Json(invite))
}

/// `DELETE /v1/api/orgs/{org_id}/invites/{invite_id}`
pub async fn revoke_invite(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Path((org_id, invite_id)): Path<(String, String)>,
) -> ApiResult<Json<DeleteResult>> {
    let (credential, _) = authorize_session(&state, &headers)?;
    let m = member_of(&state, &credential, &org_id).await?;
    audited_as(
        &state,
        &headers,
        peer.0,
        need_manager(&m),
        "invite_revoke",
        &org_id,
        Some(&invite_id),
        &m.email,
    )
    .await?;
    let a = Arc::clone(&m.accounts);
    let wanted = invite_id.clone();
    if blocking(move || a.revoke_invite(&org_id, &wanted)).await? {
        Ok(Json(DeleteResult { deleted: 1 }))
    } else {
        Err(ApiError::NotFound(format!("the organisation has no invitation {invite_id:?}")))
    }
}

/// `POST /v1/api/invites/lookup`
pub async fn lookup_invite(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Json(req): Json<TokenRequest>,
) -> ApiResult<Json<InviteLookup>> {
    let accounts = store(&state)?;
    state
        .account_guards
        .tokens
        .hit(&accounts::client(&state, &headers, &peer))
        .map_err(|s| accounts::limited(s, "too many link attempts from this address"))?;
    let a = Arc::clone(&accounts);
    let token = req.token;
    let invite = blocking(move || a.find_invite(&token)).await?.ok_or_else(bad_invite)?;
    let a = Arc::clone(&accounts);
    let wanted = invite.email.clone();
    let exists = blocking(move || a.account_id_for(&wanted)).await?.is_some();
    Ok(Json(InviteLookup {
        org_name: invite.org_name,
        email: invite.email,
        role: invite.role.to_wire(),
        account_exists: exists,
        expires_at: OffsetDateTime::from_unix_timestamp(invite.expires_at)
            .unwrap_or(OffsetDateTime::UNIX_EPOCH),
    }))
}

fn bad_invite() -> ApiError {
    ApiError::BadParam {
        field: "token".to_owned(),
        message: "that invitation is unknown, already used or expired; ask for a new one"
            .to_owned(),
    }
}

/// `POST /v1/api/invites/accept`
pub async fn accept_invite(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Json(req): Json<InviteAcceptRequest>,
) -> ApiResult<Json<InviteAccepted>> {
    let accounts = store(&state)?;
    state
        .account_guards
        .tokens
        .hit(&accounts::client(&state, &headers, &peer))
        .map_err(|s| accounts::limited(s, "too many link attempts from this address"))?;
    let a = Arc::clone(&accounts);
    let token = req.token;
    let invite = blocking(move || a.find_invite(&token)).await?.ok_or_else(bad_invite)?;
    let a = Arc::clone(&accounts);
    let wanted = invite.email.clone();
    let existing = blocking(move || a.account_id_for(&wanted)).await?;
    let (account_id, created) = match existing {
        Some(id) => {
            // The account must be the one holding the session: an invitation
            // to an address proves nothing about who is logged in.
            let session = crate::auth::presented(&headers).and_then(|p| accounts.lookup(&p));
            let ok = session.as_ref().is_some_and(|c| {
                c.account_id == id
                    && matches!(c.kind, accounts::CredentialKind::Session { org: None, .. })
            });
            if !ok {
                return Err(ApiError::Unauthorized(format!(
                    "an account uses {}; log in as it and open the link again",
                    invite.email
                )));
            }
            (id, false)
        }
        None => {
            let Some(password) = req.password else {
                return Err(ApiError::BadParam {
                    field: "password".to_owned(),
                    message: "no account uses this address yet; choose a password to create it"
                        .to_owned(),
                });
            };
            accounts::check_password(&password)?;
            let hash = accounts::hash_bounded(&state, password).await?;
            let a = Arc::clone(&accounts);
            let wanted = invite.email.clone();
            let made =
                blocking(move || a.create_member_account(&wanted, Some(&hash), None)).await?;
            let Some(id) = made else {
                return Err(ApiError::Conflict(
                    "an account took this address a moment ago; log in as it and open the link again"
                        .to_owned(),
                ));
            };
            (id, true)
        }
    };
    let a = Arc::clone(&accounts);
    let (inv, id) = (invite.clone(), account_id.clone());
    let accepted = blocking(move || a.accept_invite(&inv, &id)).await?;
    let decision = if accepted { Ok(()) } else { Err(bad_invite()) };
    audited_as(
        &state,
        &headers,
        peer.0,
        decision,
        "invite_accept",
        &invite.org_id,
        Some(&invite.id),
        &invite.email,
    )
    .await?;
    let session = if created {
        let meta: SessionMeta = accounts::session_meta(&state, &headers, &peer, "invite", None);
        let a = Arc::clone(&accounts);
        let id = account_id.clone();
        Some(blocking(move || a.issue_session(&id, &meta)).await?)
    } else {
        None
    };
    let a = Arc::clone(&accounts);
    let org_id = invite.org_id.clone();
    let row = blocking(move || a.org_row(&org_id))
        .await?
        .ok_or_else(|| ApiError::NotFound("the organisation is gone".to_owned()))?;
    let role = accounts.role_in(&account_id, &invite.org_id).unwrap_or(invite.role);
    Ok(Json(InviteAccepted { org: org_to_wire(&state, row, role), session }))
}

/// `GET /v1/api/orgs/{org_id}/projects`
pub async fn list_org_projects(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(org_id): Path<String>,
) -> ApiResult<Json<Vec<OrgProject>>> {
    let (credential, _) = authorize_session(&state, &headers)?;
    let m = member_of(&state, &credential, &org_id).await?;
    let a = Arc::clone(&m.accounts);
    let all = blocking(move || a.org_projects(&org_id)).await?;
    Ok(Json(
        all.into_iter()
            .filter(|p| m.accounts.role_on(&m.account_id, &p.project).is_some())
            .collect(),
    ))
}

/// `POST /v1/api/orgs/{org_id}/projects`
pub async fn create_org_project(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Path(org_id): Path<String>,
    Json(req): Json<CreateProjectRequest>,
) -> ApiResult<Json<OrgProject>> {
    let (credential, _) = authorize_session(&state, &headers)?;
    let m = member_of(&state, &credential, &org_id).await?;
    let label = req.label.as_deref().map(|l| name_of(l, "label")).transpose()?;
    audited_as(
        &state,
        &headers,
        peer.0,
        need_manager(&m),
        "project_create",
        &org_id,
        None,
        &m.email,
    )
    .await?;
    let a = Arc::clone(&m.accounts);
    let made = blocking(move || a.create_project(&org_id, label.as_deref())).await?;
    sync_quotas(&state);
    made.map(Json).ok_or_else(|| {
        ApiError::Forbidden(format!(
            "an organisation owns at most {} projects",
            accounts::org_store::MAX_PROJECTS_PER_ORG
        ))
    })
}

/// `DELETE /v1/api/orgs/{org_id}/projects/{project}`
pub async fn delete_org_project(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Path((org_id, project)): Path<(String, String)>,
) -> ApiResult<Json<OrgDeletion>> {
    let (credential, _) = authorize_session(&state, &headers)?;
    let m = member_of(&state, &credential, &org_id).await?;
    let mut decision = need_manager(&m);
    if decision.is_ok() {
        let a = Arc::clone(&m.accounts);
        let (id, p) = (org_id.clone(), project.clone());
        if let Some(why) = blocking(move || a.project_delete_blocker(&id, &p)).await? {
            decision = Err(ApiError::Conflict(why));
        }
    }
    audited_as(
        &state,
        &headers,
        peer.0,
        decision,
        "project_delete",
        &org_id,
        Some(&project),
        &m.email,
    )
    .await?;
    let a = Arc::clone(&m.accounts);
    let (id, p) = (org_id.clone(), project.clone());
    if !blocking(move || a.delete_project(&id, &p)).await? {
        return Err(ApiError::NotFound(format!("the organisation owns no project {project:?}")));
    }
    erase_project(&state, &project).await;
    sync_quotas(&state);
    Ok(Json(OrgDeletion { projects_erased: vec![project] }))
}

/// `GET /v1/api/orgs/{org_id}/keys`
pub async fn list_org_keys(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(org_id): Path<String>,
) -> ApiResult<Json<Vec<AccountKey>>> {
    let (credential, _) = authorize_session(&state, &headers)?;
    let m = member_of(&state, &credential, &org_id).await?;
    need_manager(&m)?;
    let a = Arc::clone(&m.accounts);
    Ok(Json(blocking(move || a.org_keys(&org_id)).await?))
}

/// `DELETE /v1/api/orgs/{org_id}/keys/{key_id}`
pub async fn revoke_org_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Path((org_id, key_id)): Path<(String, String)>,
) -> ApiResult<Json<DeleteResult>> {
    let (credential, _) = authorize_session(&state, &headers)?;
    let m = member_of(&state, &credential, &org_id).await?;
    audited_as(
        &state,
        &headers,
        peer.0,
        need_manager(&m),
        "org_key_revoke",
        &org_id,
        Some(&key_id),
        &m.email,
    )
    .await?;
    let a = Arc::clone(&m.accounts);
    let wanted = key_id.clone();
    if blocking(move || a.revoke_org_key(&org_id, &wanted)).await? {
        Ok(Json(DeleteResult { deleted: 1 }))
    } else {
        Err(ApiError::NotFound(format!("no project of the organisation holds a key {key_id:?}")))
    }
}

/// `GET /v1/api/orgs/{org_id}/usage`
pub async fn get_org_usage(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(org_id): Path<String>,
) -> ApiResult<Json<OrgUsage>> {
    let (credential, _) = authorize_session(&state, &headers)?;
    let m = member_of(&state, &credential, &org_id).await?;
    let a = Arc::clone(&m.accounts);
    let id = org_id.clone();
    let (row, keys) = blocking(move || Ok((a.org_row(&id)?, a.org_key_count(&id)?))).await?;
    let row = row.ok_or_else(|| ApiError::NotFound("the organisation is gone".to_owned()))?;
    let wire = org_to_wire(&state, row, m.role);
    let (limit, this_minute, admitted, refused) =
        state.quotas.org_usage(&org_id).map_or((None, 0, 0, 0), |(l, c, a, r)| (Some(l), c, a, r));
    Ok(Json(OrgUsage {
        org_id,
        quota_per_minute: limit,
        admitted_this_minute: this_minute,
        admitted_total: admitted,
        refused_total: refused,
        retention_days: wire.retention_days,
        members: wire.members,
        projects: wire.projects,
        keys,
        since: state.started_at,
    }))
}

/// `GET /v1/api/orgs/{org_id}/audit`
pub async fn list_org_audit(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(org_id): Path<String>,
) -> ApiResult<Json<Vec<AuditEvent>>> {
    let (credential, _) = authorize_session(&state, &headers)?;
    let m = member_of(&state, &credential, &org_id).await?;
    need_manager(&m)?;
    let q =
        mcpd_store::AuditQuery { project: org_id, action: None, from: None, to: None, limit: 500 };
    Ok(Json(state.store.list_audit(&q).await?))
}

/// `GET /v1/api/orgs/{org_id}/group-roles`
pub async fn get_group_roles(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(org_id): Path<String>,
) -> ApiResult<Json<Vec<GroupRole>>> {
    let (credential, _) = authorize_session(&state, &headers)?;
    let m = member_of(&state, &credential, &org_id).await?;
    need_manager(&m)?;
    let a = Arc::clone(&m.accounts);
    Ok(Json(blocking(move || a.group_roles(&org_id)).await?))
}

/// `PUT /v1/api/orgs/{org_id}/group-roles`
pub async fn put_group_roles(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Path(org_id): Path<String>,
    Json(req): Json<Vec<GroupRole>>,
) -> ApiResult<Json<Vec<GroupRole>>> {
    let (credential, _) = authorize_session(&state, &headers)?;
    let m = member_of(&state, &credential, &org_id).await?;
    let roles = group_roles_from_wire(&req)?;
    audited_as(
        &state,
        &headers,
        peer.0,
        need_manager(&m),
        "group_roles_update",
        &org_id,
        None,
        &m.email,
    )
    .await?;
    let a = Arc::clone(&m.accounts);
    let id = org_id.clone();
    let stored = blocking(move || {
        a.put_group_roles(&id, &roles)?;
        a.group_roles(&id)
    })
    .await?;
    crate::scim::recompute_org(&state, &org_id).await;
    Ok(Json(stored))
}

/// Validate a group mapping: no owner, no group twice, a sane size.
pub(crate) fn group_roles_from_wire(req: &[GroupRole]) -> ApiResult<Vec<(String, Role)>> {
    if req.len() > accounts::org_store::MAX_GROUP_ROLES {
        return Err(ApiError::BadParam {
            field: "group".to_owned(),
            message: format!("at most {} group mappings", accounts::org_store::MAX_GROUP_ROLES),
        });
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::with_capacity(req.len());
    for g in req {
        let name = name_of(&g.group, "group")?.to_lowercase();
        let role = Role::from_wire(g.role);
        if role == Role::Owner {
            return Err(ApiError::BadParam {
                field: "role".to_owned(),
                message: format!(
                    "the group {name:?} maps to owner; a group can grant at most admin"
                ),
            });
        }
        if !seen.insert(name.clone()) {
            return Err(ApiError::BadParam {
                field: "group".to_owned(),
                message: format!("the group {name:?} appears twice"),
            });
        }
        out.push((name, role));
    }
    Ok(out)
}

/// The organisation routes. Merged into the query API's router by `lib.rs`.
pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/v1/api/orgs", get(list_orgs))
        .route("/v1/api/orgs", post(create_org))
        .route("/v1/api/orgs/{org_id}", get(get_org))
        .route("/v1/api/orgs/{org_id}", patch(update_org))
        .route("/v1/api/orgs/{org_id}", delete(delete_org))
        .route("/v1/api/orgs/{org_id}/members", get(list_members))
        .route("/v1/api/orgs/{org_id}/members/{account_id}", patch(update_member))
        .route("/v1/api/orgs/{org_id}/members/{account_id}", delete(remove_member))
        .route("/v1/api/orgs/{org_id}/members/{account_id}/project-roles", post(set_project_role))
        .route("/v1/api/orgs/{org_id}/invites", get(list_invites))
        .route("/v1/api/orgs/{org_id}/invites", post(create_invite))
        .route("/v1/api/orgs/{org_id}/invites/{invite_id}", delete(revoke_invite))
        .route("/v1/api/invites/lookup", post(lookup_invite))
        .route("/v1/api/invites/accept", post(accept_invite))
        .route("/v1/api/orgs/{org_id}/projects", get(list_org_projects))
        .route("/v1/api/orgs/{org_id}/projects", post(create_org_project))
        .route("/v1/api/orgs/{org_id}/projects/{project}", delete(delete_org_project))
        .route("/v1/api/orgs/{org_id}/keys", get(list_org_keys))
        .route("/v1/api/orgs/{org_id}/keys/{key_id}", delete(revoke_org_key))
        .route("/v1/api/orgs/{org_id}/usage", get(get_org_usage))
        .route("/v1/api/orgs/{org_id}/audit", get(list_org_audit))
        .route("/v1/api/orgs/{org_id}/group-roles", get(get_group_roles))
        .route("/v1/api/orgs/{org_id}/group-roles", put(put_group_roles))
}
