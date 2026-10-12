//! SCIM 2.0 provisioning (RFC 7643 and RFC 7644), one organisation per token.
//!
//! An identity provider creates, changes and removes the organisation's users
//! and groups here. What that does to accounts:
//!
//! * A user whose address is in a domain the organisation claims, with no
//!   account, gets an account the organisation manages, and becomes a member.
//! * A user whose address already has an account becomes a member only when
//!   that account accepts the invitation sent to it. SCIM never takes over an
//!   account it did not create, so one organisation's identity provider cannot
//!   reach another account's own projects or other organisations.
//! * `active: false`, or deleting the user, deprovisions: the membership ends,
//!   the account's keys on the organisation's projects are revoked and its
//!   sessions for the organisation end. An account the organisation manages is
//!   deactivated, every session of it ends, and deleting the user deletes it.
//! * Groups map onto roles through the organisation's group roles. A member
//!   gets the highest role among their groups, or the single sign-on default
//!   when no group maps. Owners keep their role: SCIM never grants or removes
//!   `owner`.
//!
//! A SCIM token reaches its own organisation's users and groups and nothing
//! else. Every id a request names is looked up inside that organisation, so an
//! id from another organisation is a 404 like any unknown id.

use crate::accounts::roles::Role;
use crate::accounts::scim_store::{ScimTerm, ScimUserFields};
use crate::accounts::{self, blocking, email_domain, store, AccountStore, ScimGroup, ScimUser};
use crate::auth::authorize_scim;
use crate::error::ApiError;
use crate::query::{audited_as, Peer};
use crate::state::AppState;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post, put};
use axum::Router;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use time::OffsetDateTime;

const USER_SCHEMA: &str = "urn:ietf:params:scim:schemas:core:2.0:User";
const GROUP_SCHEMA: &str = "urn:ietf:params:scim:schemas:core:2.0:Group";
const LIST_SCHEMA: &str = "urn:ietf:params:scim:api:messages:2.0:ListResponse";
const ERROR_SCHEMA: &str = "urn:ietf:params:scim:api:messages:2.0:Error";
const PATCH_SCHEMA: &str = "urn:ietf:params:scim:api:messages:2.0:PatchOp";

/// Most resources one page returns.
pub const MAX_PAGE: u32 = 200;

/// Largest SCIM request body read, in bytes.
pub const MAX_BODY: usize = 1024 * 1024;

/// A SCIM answer, sent as `application/scim+json`.
pub struct Scim(StatusCode, Value);

impl IntoResponse for Scim {
    fn into_response(self) -> Response {
        let mut r = (self.0, axum::Json(self.1)).into_response();
        r.headers_mut().insert(
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderValue::from_static("application/scim+json"),
        );
        r
    }
}

/// A SCIM error, in the shape RFC 7644 section 3.12 gives.
#[derive(Debug)]
pub struct ScimError {
    status: StatusCode,
    scim_type: Option<&'static str>,
    detail: String,
    retry_after: Option<u64>,
}

impl ScimError {
    fn new(status: StatusCode, scim_type: Option<&'static str>, detail: impl Into<String>) -> Self {
        Self { status, scim_type, detail: detail.into(), retry_after: None }
    }
    fn bad(scim_type: &'static str, detail: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, Some(scim_type), detail)
    }
    fn not_found(what: &str, id: &str) -> Self {
        Self::new(StatusCode::NOT_FOUND, None, format!("no {what} {id:?} in this organisation"))
    }
}

impl From<ApiError> for ScimError {
    fn from(e: ApiError) -> Self {
        let detail = e.to_string();
        let retry_after = match &e {
            ApiError::RateLimited(r) => Some(r.retry_after_seconds),
            _ => None,
        };
        let status = e.into_response().status();
        Self { status, scim_type: None, detail, retry_after }
    }
}

impl IntoResponse for ScimError {
    fn into_response(self) -> Response {
        let mut body = json!({
            "schemas": [ERROR_SCHEMA],
            "status": self.status.as_u16().to_string(),
            "detail": self.detail,
        });
        if let Some(t) = self.scim_type {
            body["scimType"] = json!(t);
        }
        let mut r = Scim(self.status, body).into_response();
        if let Some(s) = self.retry_after {
            r.headers_mut()
                .insert(axum::http::header::RETRY_AFTER, axum::http::HeaderValue::from(s));
        }
        r
    }
}

type ScimResult = Result<Scim, ScimError>;

fn ts(secs: i64) -> String {
    OffsetDateTime::from_unix_timestamp(secs)
        .ok()
        .and_then(|t| t.format(&time::format_description::well_known::Rfc3339).ok())
        .unwrap_or_default()
}

fn user_json(u: &ScimUser) -> Value {
    let mut v = json!({
        "schemas": [USER_SCHEMA],
        "id": u.id,
        "userName": u.user_name,
        "active": u.active,
        "emails": [{ "value": u.email, "primary": true, "type": "work" }],
        "groups": u.groups.iter().map(|(id, name)| json!({
            "value": id, "display": name, "$ref": format!("/scim/v2/Groups/{id}")
        })).collect::<Vec<_>>(),
        "meta": {
            "resourceType": "User",
            "created": ts(u.created_at),
            "lastModified": ts(u.updated_at),
            "location": format!("/scim/v2/Users/{}", u.id),
        },
    });
    if let Some(x) = &u.external_id {
        v["externalId"] = json!(x);
    }
    if let Some(x) = &u.display_name {
        v["displayName"] = json!(x);
    }
    if u.given_name.is_some() || u.family_name.is_some() {
        v["name"] = json!({ "givenName": u.given_name, "familyName": u.family_name });
    }
    v
}

fn group_json(g: &ScimGroup) -> Value {
    let mut v = json!({
        "schemas": [GROUP_SCHEMA],
        "id": g.id,
        "displayName": g.display_name,
        "members": g.members.iter().map(|(id, name)| json!({
            "value": id, "display": name, "$ref": format!("/scim/v2/Users/{id}")
        })).collect::<Vec<_>>(),
        "meta": {
            "resourceType": "Group",
            "created": ts(g.created_at),
            "lastModified": ts(g.updated_at),
            "location": format!("/scim/v2/Groups/{}", g.id),
        },
    });
    if let Some(x) = &g.external_id {
        v["externalId"] = json!(x);
    }
    v
}

fn list(total: u32, start: u32, items: Vec<Value>) -> Scim {
    Scim(
        StatusCode::OK,
        json!({
            "schemas": [LIST_SCHEMA],
            "totalResults": total,
            "startIndex": start,
            "itemsPerPage": items.len(),
            "Resources": items,
        }),
    )
}

/// Parse a filter: `attr eq "value"` terms joined by `and`. Anything else is
/// refused with `invalidFilter`, rather than ignored and answered with every
/// resource.
pub fn parse_filter(raw: &str) -> Result<Vec<ScimTerm>, ScimError> {
    let mut terms = Vec::new();
    let mut rest = raw.trim();
    while !rest.is_empty() {
        let (attr, after) = rest.split_once(char::is_whitespace).ok_or_else(|| {
            ScimError::bad("invalidFilter", "a filter term is `attribute eq \"value\"`")
        })?;
        let after = after.trim_start();
        let (op, after) = after.split_once(char::is_whitespace).ok_or_else(|| {
            ScimError::bad("invalidFilter", "a filter term is `attribute eq \"value\"`")
        })?;
        if !op.eq_ignore_ascii_case("eq") {
            return Err(ScimError::bad(
                "invalidFilter",
                format!("only eq is supported, not {op:?}"),
            ));
        }
        let after = after.trim_start();
        let Some(body) = after.strip_prefix('"') else {
            return Err(ScimError::bad("invalidFilter", "the value must be a quoted string"));
        };
        let mut value = String::new();
        let mut chars = body.char_indices();
        let mut end = None;
        while let Some((i, c)) = chars.next() {
            match c {
                '\\' => {
                    if let Some((_, n)) = chars.next() {
                        value.push(n);
                    }
                }
                '"' => {
                    end = Some(i);
                    break;
                }
                c => value.push(c),
            }
        }
        let end =
            end.ok_or_else(|| ScimError::bad("invalidFilter", "the value has no closing quote"))?;
        let term = match attr.to_ascii_lowercase().as_str() {
            "username" => ScimTerm::UserName(value),
            "externalid" => ScimTerm::ExternalId(value),
            "emails" | "emails.value" => ScimTerm::Email(value),
            "displayname" => ScimTerm::DisplayName(value),
            "id" => ScimTerm::Id(value),
            other => {
                return Err(ScimError::bad(
                    "invalidFilter",
                    format!("filtering on {other:?} is not supported"),
                ))
            }
        };
        terms.push(term);
        rest = body[end + 1..].trim_start();
        if rest.is_empty() {
            break;
        }
        let (and, after) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        if !and.eq_ignore_ascii_case("and") {
            return Err(ScimError::bad("invalidFilter", "terms are joined with `and`"));
        }
        rest = after.trim_start();
        if terms.len() > 8 {
            return Err(ScimError::bad("invalidFilter", "at most eight terms"));
        }
    }
    Ok(terms)
}

fn paging(q: &HashMap<String, String>) -> (u32, u32) {
    let start = q.get("startIndex").and_then(|s| s.parse::<u32>().ok()).unwrap_or(1).max(1);
    let count = q.get("count").and_then(|s| s.parse::<u32>().ok()).unwrap_or(100).min(MAX_PAGE);
    (start, count)
}

fn body(raw: &Bytes) -> Result<Value, ScimError> {
    if raw.len() > MAX_BODY {
        return Err(ScimError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            None,
            format!("a SCIM body is at most {MAX_BODY} bytes"),
        ));
    }
    serde_json::from_slice(raw)
        .map_err(|e| ScimError::bad("invalidSyntax", format!("the body is not JSON: {e}")))
}

fn str_of(v: &Value) -> Option<String> {
    v.as_str().map(|s| s.trim().to_owned()).filter(|s| !s.is_empty())
}

fn bool_of(v: &Value) -> Option<bool> {
    match v {
        Value::Bool(b) => Some(*b),
        // Some identity providers send booleans as strings, in any case.
        Value::String(s) => match s.to_ascii_lowercase().as_str() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

fn primary_email(v: &Value) -> Option<String> {
    let emails = v.get("emails")?.as_array()?;
    emails
        .iter()
        .find(|e| e.get("primary").and_then(bool_of) == Some(true))
        .or_else(|| emails.first())
        .and_then(|e| e.get("value"))
        .and_then(str_of)
}

/// The writable fields of a user from a full resource, as POST and PUT send.
fn user_fields(v: &Value) -> Result<ScimUserFields, ScimError> {
    let user_name = v
        .get("userName")
        .and_then(str_of)
        .ok_or_else(|| ScimError::bad("invalidValue", "userName is required"))?;
    let email = primary_email(v).unwrap_or_else(|| user_name.clone());
    let email = accounts::normalise_email(&email).map_err(|_| {
        ScimError::bad("invalidValue", "the user needs an email address, in emails or as userName")
    })?;
    if user_name.len() > 254 {
        return Err(ScimError::bad("invalidValue", "userName is at most 254 bytes"));
    }
    let name = v.get("name");
    let cap = |s: Option<String>| s.map(|s| s.chars().take(200).collect::<String>());
    Ok(ScimUserFields {
        user_name,
        external_id: cap(v.get("externalId").and_then(str_of)),
        display_name: cap(v.get("displayName").and_then(str_of)),
        given_name: cap(name.and_then(|n| n.get("givenName")).and_then(str_of)),
        family_name: cap(name.and_then(|n| n.get("familyName")).and_then(str_of)),
        email,
        active: v.get("active").and_then(bool_of).unwrap_or(true),
    })
}

fn fields_of(u: &ScimUser) -> ScimUserFields {
    ScimUserFields {
        user_name: u.user_name.clone(),
        external_id: u.external_id.clone(),
        display_name: u.display_name.clone(),
        given_name: u.given_name.clone(),
        family_name: u.family_name.clone(),
        email: u.email.clone(),
        active: u.active,
    }
}

/// Apply PATCH operations to a user's fields.
fn patch_user(f: &mut ScimUserFields, ops: &Value) -> Result<(), ScimError> {
    for op in patch_ops(ops)? {
        let kind = op.get("op").and_then(|o| o.as_str()).unwrap_or("").to_ascii_lowercase();
        if !matches!(kind.as_str(), "add" | "replace" | "remove") {
            return Err(ScimError::bad("invalidSyntax", format!("unknown PATCH op {kind:?}")));
        }
        let value = op.get("value").cloned().unwrap_or(Value::Null);
        let path = op.get("path").and_then(|p| p.as_str()).map(str::to_ascii_lowercase);
        let mut set = |path: &str, v: &Value| -> Result<(), ScimError> {
            let s = || str_of(v);
            match path {
                "active" => {
                    f.active = bool_of(v)
                        .ok_or_else(|| ScimError::bad("invalidValue", "active must be a boolean"))?
                }
                "username" => {
                    f.user_name = s()
                        .ok_or_else(|| ScimError::bad("invalidValue", "userName cannot be empty"))?
                }
                "displayname" => f.display_name = s(),
                "externalid" => f.external_id = s(),
                "name.givenname" => f.given_name = s(),
                "name.familyname" => f.family_name = s(),
                "name" => {
                    f.given_name = v.get("givenName").and_then(str_of);
                    f.family_name = v.get("familyName").and_then(str_of);
                }
                p if p.starts_with("emails") => {
                    let e =
                        if p == "emails" { primary_email(&json!({ "emails": v })) } else { s() };
                    if let Some(e) = e {
                        f.email = accounts::normalise_email(&e).map_err(|_| {
                            ScimError::bad("invalidValue", "that is not an email address")
                        })?;
                    }
                }
                // Attributes this service does not store, such as title or an
                // enterprise extension, are accepted and dropped, because
                // identity providers send them whatever the schema says.
                _ => {}
            }
            Ok(())
        };
        match (path, kind.as_str()) {
            (Some(p), "remove") => set(&p, &Value::Null)?,
            (Some(p), _) => set(&p, &value)?,
            (None, _) => {
                let Some(obj) = value.as_object() else {
                    return Err(ScimError::bad(
                        "invalidSyntax",
                        "an op with no path needs an object value",
                    ));
                };
                for (k, v) in obj {
                    set(&k.to_ascii_lowercase(), v)?;
                }
            }
        }
    }
    Ok(())
}

fn patch_ops(v: &Value) -> Result<Vec<Value>, ScimError> {
    let schemas_ok = v
        .get("schemas")
        .and_then(|s| s.as_array())
        .is_none_or(|a| a.iter().any(|s| s == PATCH_SCHEMA));
    if !schemas_ok {
        return Err(ScimError::bad(
            "invalidSyntax",
            format!("a PATCH body carries the {PATCH_SCHEMA} schema"),
        ));
    }
    let ops = v
        .get("Operations")
        .or_else(|| v.get("operations"))
        .and_then(|o| o.as_array())
        .ok_or_else(|| ScimError::bad("invalidSyntax", "a PATCH body has Operations"))?;
    if ops.len() > 1000 {
        return Err(ScimError::bad("tooMany", "at most 1000 operations"));
    }
    Ok(ops.clone())
}

/// The role a provisioned member should have now, from their groups.
fn role_now(
    accounts: &AccountStore,
    org: &str,
    user: &ScimUser,
    default: Role,
    config_owners: &[String],
) -> Result<Role, accounts::AccountError> {
    if config_owners.iter().any(|o| o.eq_ignore_ascii_case(&user.email)) {
        return Ok(Role::Owner);
    }
    let names: Vec<String> = user.groups.iter().map(|(_, n)| n.clone()).collect();
    Ok(accounts.role_for_groups(org, &names)?.unwrap_or(default))
}

/// Bring one provisioned user's account in line with the resource: member with
/// the right role while active, deprovisioned while not.
fn reconcile(
    accounts: &AccountStore,
    org: &str,
    user: &ScimUser,
    default: Role,
    config_owners: &[String],
) -> Result<(), accounts::AccountError> {
    let Some(account_id) = &user.account_id else { return Ok(()) };
    let managed = accounts.account(account_id)?.managed_by.as_deref() == Some(org);
    if !user.active {
        if managed {
            accounts.set_disabled(account_id, true)?;
            accounts.revoke_all_keys(account_id)?;
        } else {
            accounts.remove_member(org, account_id)?;
        }
        return Ok(());
    }
    if managed {
        accounts.set_disabled(account_id, false)?;
    }
    let current = accounts.role_in(account_id, org);
    let wanted = if current == Some(Role::Owner) {
        Role::Owner
    } else {
        role_now(accounts, org, user, default, config_owners)?
    };
    if current != Some(wanted) {
        accounts.upsert_member(org, account_id, wanted, "scim")?;
        accounts.prune_keys(org, account_id)?;
    }
    Ok(())
}

/// What provisioning needs to know about the organisation.
struct OrgContext {
    domains: Vec<String>,
    default: Role,
    owners: Vec<String>,
    name: String,
}

async fn context(
    state: &AppState,
    accounts: &Arc<AccountStore>,
    org: &str,
) -> Result<OrgContext, ScimError> {
    let a = Arc::clone(accounts);
    let id = org.to_owned();
    let (sso, row) = blocking(move || Ok((a.sso_settings(&id)?, a.org_row(&id)?))).await?;
    let owners = state
        .config
        .account_settings
        .orgs
        .iter()
        .find(|o| format!("org_{}", o.slug) == org)
        .map(|o| o.owners.clone())
        .unwrap_or_default();
    Ok(OrgContext {
        domains: sso.as_ref().map(|s| s.domains.clone()).unwrap_or_default(),
        default: sso.map_or(Role::Member, |s| s.default_role),
        owners,
        name: row.map(|r| r.name).unwrap_or_default(),
    })
}

/// Find or create the account for a newly provisioned user, or send the
/// invitation that will link one. Returns the user as it now stands.
async fn provision(
    state: &AppState,
    accounts: &Arc<AccountStore>,
    org: &str,
    user: ScimUser,
) -> Result<ScimUser, ScimError> {
    let ctx = context(state, accounts, org).await?;
    let a = Arc::clone(accounts);
    let (o, u) = (org.to_owned(), user.clone());
    let domains = ctx.domains.clone();
    let invite = blocking(move || {
        let existing = a.account_id_for(&u.email)?;
        let linked = match existing {
            Some(id) => {
                let member = a.role_in(&id, &o).is_some();
                let managed = a.account(&id)?.managed_by.as_deref() == Some(o.as_str());
                (member || managed).then_some(id)
            }
            None if domains.iter().any(|d| d == email_domain(&u.email)) => {
                a.create_member_account(&u.email, None, Some(&o))?
            }
            None => None,
        };
        match linked {
            Some(id) => {
                a.link_scim_user(&o, &u.id, &id)?;
                Ok(None)
            }
            None if u.active => {
                let role = Role::Member;
                Ok(a.create_invite(&o, &u.email, role, &[], None, Some(&u.id))?.map(|(_, t)| t))
            }
            None => Ok(None),
        }
    })
    .await?;
    if let Some(token) = invite {
        crate::orgs::send_invite_mail(state, &user.email, &ctx.name, &token);
    }
    let a = Arc::clone(accounts);
    let (o, id) = (org.to_owned(), user.id.clone());
    let (default, owners) = (ctx.default, ctx.owners.clone());
    blocking(move || {
        let u = a.scim_user(&o, &id)?.ok_or(accounts::AccountError::Poisoned)?;
        reconcile(&a, &o, &u, default, &owners)?;
        Ok(a.scim_user(&o, &id)?.unwrap_or(u))
    })
    .await
    .map_err(ScimError::from)
}

/// Re-derive the role of every provisioned user in an organisation, after its
/// group mapping changed.
pub async fn recompute_org(state: &AppState, org: &str) {
    let Ok(accounts) = store(state) else { return };
    let Ok(ctx) = context(state, &accounts, org).await else { return };
    let o = org.to_owned();
    let result = blocking(move || {
        let (_, users) = accounts.scim_users(&o, &[], 1, u32::MAX)?;
        for u in users {
            reconcile(&accounts, &o, &u, ctx.default, &ctx.owners)?;
        }
        Ok(())
    })
    .await;
    if let Err(e) = result {
        tracing::error!(org, "roles could not be recomputed after a group change: {e}");
    }
}

async fn reconcile_ids(
    state: &AppState,
    accounts: &Arc<AccountStore>,
    org: &str,
    ids: Vec<String>,
) -> Result<(), ScimError> {
    let ctx = context(state, accounts, org).await?;
    let a = Arc::clone(accounts);
    let o = org.to_owned();
    blocking(move || {
        for id in ids {
            if let Some(u) = a.scim_user(&o, &id)? {
                reconcile(&a, &o, &u, ctx.default, &ctx.owners)?;
            }
        }
        Ok(())
    })
    .await?;
    Ok(())
}

fn limit(state: &AppState, org: &str) -> Result<(), ScimError> {
    state.account_guards.scim.hit(org).map_err(|s| {
        ScimError::from(accounts::limited(s, "too many SCIM requests for this organisation"))
    })
}

/// Write the audit row for a SCIM change, whichever way it went, and hand the
/// outcome back unchanged.
async fn audit<T>(
    state: &AppState,
    headers: &HeaderMap,
    peer: &Peer,
    action: &str,
    org: &str,
    resource: Option<&str>,
    r: Result<T, ScimError>,
) -> Result<T, ScimError> {
    let decision: Result<(), ApiError> = match &r {
        Ok(_) => Ok(()),
        Err(e) => Err(ApiError::Forbidden(e.detail.clone())),
    };
    let _ = audited_as(state, headers, peer.0, decision, action, org, resource, "scim").await;
    r
}

/// `GET /scim/v2/ServiceProviderConfig`
pub async fn scim_service_provider_config(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> ScimResult {
    let org = authorize_scim(&state, &headers)?;
    limit(&state, &org)?;
    Ok(Scim(
        StatusCode::OK,
        json!({
            "schemas": ["urn:ietf:params:scim:schemas:core:2.0:ServiceProviderConfig"],
            "patch": { "supported": true },
            "bulk": { "supported": false, "maxOperations": 0, "maxPayloadSize": 0 },
            "filter": { "supported": true, "maxResults": MAX_PAGE },
            "changePassword": { "supported": false },
            "sort": { "supported": false },
            "etag": { "supported": false },
            "authenticationSchemes": [{
                "type": "oauthbearertoken",
                "name": "Bearer token",
                "description": "The organisation's SCIM token, sent as Authorization: Bearer <token>.",
                "primary": true
            }],
            "meta": { "resourceType": "ServiceProviderConfig", "location": "/scim/v2/ServiceProviderConfig" }
        }),
    ))
}

fn attr(name: &str, kind: &str, required: bool, uniqueness: &str) -> Value {
    json!({
        "name": name, "type": kind, "multiValued": false, "required": required,
        "caseExact": false, "mutability": "readWrite", "returned": "default", "uniqueness": uniqueness
    })
}

fn schemas() -> Vec<Value> {
    vec![
        json!({
            "schemas": ["urn:ietf:params:scim:schemas:core:2.0:Schema"],
            "id": USER_SCHEMA, "name": "User", "description": "A member of the organisation.",
            "attributes": [
                attr("userName", "string", true, "server"),
                attr("displayName", "string", false, "none"),
                attr("externalId", "string", false, "none"),
                attr("active", "boolean", false, "none"),
                { "name": "name", "type": "complex", "multiValued": false, "required": false,
                  "subAttributes": [attr("givenName", "string", false, "none"), attr("familyName", "string", false, "none")] },
                { "name": "emails", "type": "complex", "multiValued": true, "required": false,
                  "subAttributes": [attr("value", "string", false, "none"), attr("primary", "boolean", false, "none")] },
                { "name": "groups", "type": "complex", "multiValued": true, "required": false, "mutability": "readOnly",
                  "subAttributes": [attr("value", "string", false, "none"), attr("display", "string", false, "none")] }
            ],
            "meta": { "resourceType": "Schema", "location": format!("/scim/v2/Schemas/{USER_SCHEMA}") }
        }),
        json!({
            "schemas": ["urn:ietf:params:scim:schemas:core:2.0:Schema"],
            "id": GROUP_SCHEMA, "name": "Group", "description": "A group, mapped onto a role.",
            "attributes": [
                attr("displayName", "string", true, "server"),
                attr("externalId", "string", false, "none"),
                { "name": "members", "type": "complex", "multiValued": true, "required": false,
                  "subAttributes": [attr("value", "string", false, "none"), attr("display", "string", false, "none")] }
            ],
            "meta": { "resourceType": "Schema", "location": format!("/scim/v2/Schemas/{GROUP_SCHEMA}") }
        }),
    ]
}

/// `GET /scim/v2/Schemas`
pub async fn scim_schemas(State(state): State<Arc<AppState>>, headers: HeaderMap) -> ScimResult {
    let org = authorize_scim(&state, &headers)?;
    limit(&state, &org)?;
    let s = schemas();
    Ok(list(2, 1, s))
}

/// `GET /scim/v2/Schemas/{id}`
pub async fn scim_schema(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ScimResult {
    let org = authorize_scim(&state, &headers)?;
    limit(&state, &org)?;
    schemas()
        .into_iter()
        .find(|s| s["id"] == id.as_str())
        .map(|s| Scim(StatusCode::OK, s))
        .ok_or_else(|| ScimError::not_found("schema", &id))
}

fn resource_types() -> Vec<Value> {
    vec![
        json!({
            "schemas": ["urn:ietf:params:scim:schemas:core:2.0:ResourceType"],
            "id": "User", "name": "User", "endpoint": "/Users", "schema": USER_SCHEMA,
            "meta": { "resourceType": "ResourceType", "location": "/scim/v2/ResourceTypes/User" }
        }),
        json!({
            "schemas": ["urn:ietf:params:scim:schemas:core:2.0:ResourceType"],
            "id": "Group", "name": "Group", "endpoint": "/Groups", "schema": GROUP_SCHEMA,
            "meta": { "resourceType": "ResourceType", "location": "/scim/v2/ResourceTypes/Group" }
        }),
    ]
}

/// `GET /scim/v2/ResourceTypes`
pub async fn scim_resource_types(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> ScimResult {
    let org = authorize_scim(&state, &headers)?;
    limit(&state, &org)?;
    Ok(list(2, 1, resource_types()))
}

/// `GET /scim/v2/ResourceTypes/{id}`
pub async fn scim_resource_type(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ScimResult {
    let org = authorize_scim(&state, &headers)?;
    limit(&state, &org)?;
    resource_types()
        .into_iter()
        .find(|s| s["id"] == id.as_str())
        .map(|s| Scim(StatusCode::OK, s))
        .ok_or_else(|| ScimError::not_found("resource type", &id))
}

/// `GET /scim/v2/Users`
pub async fn scim_list_users(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> ScimResult {
    let org = authorize_scim(&state, &headers)?;
    limit(&state, &org)?;
    let accounts = store(&state)?;
    let terms = q.get("filter").map(|f| parse_filter(f)).transpose()?.unwrap_or_default();
    let (start, count) = paging(&q);
    let (total, users) = blocking(move || accounts.scim_users(&org, &terms, start, count)).await?;
    Ok(list(total, start, users.iter().map(user_json).collect()))
}

/// `POST /scim/v2/Users`
pub async fn scim_create_user(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    raw: Bytes,
) -> ScimResult {
    let org = authorize_scim(&state, &headers)?;
    limit(&state, &org)?;
    let accounts = store(&state)?;
    let fields = user_fields(&body(&raw)?)?;
    let a = Arc::clone(&accounts);
    let o = org.clone();
    let made = blocking(move || a.insert_scim_user(&o, &fields, None)).await?;
    let made = made.ok_or_else(|| {
        ScimError::new(
            StatusCode::CONFLICT,
            Some("uniqueness"),
            "a user with that userName is already provisioned",
        )
    });
    let made = audit(&state, &headers, &peer, "scim_user_create", &org, None, made).await?;
    let user = provision(&state, &accounts, &org, made).await?;
    Ok(Scim(StatusCode::CREATED, user_json(&user)))
}

/// `GET /scim/v2/Users/{id}`
pub async fn scim_get_user(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ScimResult {
    let org = authorize_scim(&state, &headers)?;
    limit(&state, &org)?;
    let accounts = store(&state)?;
    let wanted = id.clone();
    let u = blocking(move || accounts.scim_user(&org, &wanted)).await?;
    u.map(|u| Scim(StatusCode::OK, user_json(&u))).ok_or_else(|| ScimError::not_found("user", &id))
}

async fn save_user(
    state: &AppState,
    headers: &HeaderMap,
    peer: &Peer,
    org: &str,
    id: &str,
    fields: ScimUserFields,
) -> ScimResult {
    let accounts = store(state)?;
    let a = Arc::clone(&accounts);
    let (o, i) = (org.to_owned(), id.to_owned());
    let ok = blocking(move || a.update_scim_user(&o, &i, &fields)).await?;
    let r = if ok {
        Ok(())
    } else {
        Err(ScimError::new(
            StatusCode::CONFLICT,
            Some("uniqueness"),
            "another user holds that userName",
        ))
    };
    audit(state, headers, peer, "scim_user_update", org, Some(id), r).await?;
    let a = Arc::clone(&accounts);
    let (o, i) = (org.to_owned(), id.to_owned());
    let user = blocking(move || a.scim_user(&o, &i))
        .await?
        .ok_or_else(|| ScimError::not_found("user", id))?;
    let user = if user.account_id.is_none() && user.active {
        provision(state, &accounts, org, user).await?
    } else {
        reconcile_ids(state, &accounts, org, vec![user.id.clone()]).await?;
        let a = Arc::clone(&accounts);
        let (o, i) = (org.to_owned(), id.to_owned());
        blocking(move || a.scim_user(&o, &i))
            .await?
            .ok_or_else(|| ScimError::not_found("user", id))?
    };
    Ok(Scim(StatusCode::OK, user_json(&user)))
}

/// `PUT /scim/v2/Users/{id}`
pub async fn scim_replace_user(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Path(id): Path<String>,
    raw: Bytes,
) -> ScimResult {
    let org = authorize_scim(&state, &headers)?;
    limit(&state, &org)?;
    let accounts = store(&state)?;
    let fields = user_fields(&body(&raw)?)?;
    let (o, i) = (org.clone(), id.clone());
    if blocking(move || accounts.scim_user(&o, &i)).await?.is_none() {
        return Err(ScimError::not_found("user", &id));
    }
    save_user(&state, &headers, &peer, &org, &id, fields).await
}

/// `PATCH /scim/v2/Users/{id}`
pub async fn scim_patch_user(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Path(id): Path<String>,
    raw: Bytes,
) -> ScimResult {
    let org = authorize_scim(&state, &headers)?;
    limit(&state, &org)?;
    let accounts = store(&state)?;
    let ops = body(&raw)?;
    let (o, i) = (org.clone(), id.clone());
    let user = blocking(move || accounts.scim_user(&o, &i))
        .await?
        .ok_or_else(|| ScimError::not_found("user", &id))?;
    let mut fields = fields_of(&user);
    patch_user(&mut fields, &ops)?;
    save_user(&state, &headers, &peer, &org, &id, fields).await
}

/// `DELETE /scim/v2/Users/{id}`
pub async fn scim_delete_user(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Path(id): Path<String>,
) -> Result<StatusCode, ScimError> {
    let org = authorize_scim(&state, &headers)?;
    limit(&state, &org)?;
    let accounts = store(&state)?;
    let a = Arc::clone(&accounts);
    let (o, i) = (org.clone(), id.clone());
    let user = blocking(move || a.scim_user(&o, &i)).await?;
    let user = audit(
        &state,
        &headers,
        &peer,
        "scim_user_delete",
        &org,
        Some(&id),
        user.ok_or_else(|| ScimError::not_found("user", &id)),
    )
    .await?;
    let o = org.clone();
    blocking(move || {
        if let Some(account_id) = &user.account_id {
            let managed = accounts.account(account_id)?.managed_by.as_deref() == Some(o.as_str());
            if managed {
                accounts.delete_managed_account(account_id)?;
            } else {
                accounts.remove_member(&o, account_id)?;
            }
        }
        accounts.delete_scim_user(&o, &user.id)?;
        Ok(())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /scim/v2/Groups`
pub async fn scim_list_groups(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> ScimResult {
    let org = authorize_scim(&state, &headers)?;
    limit(&state, &org)?;
    let accounts = store(&state)?;
    let terms = q.get("filter").map(|f| parse_filter(f)).transpose()?.unwrap_or_default();
    let (start, count) = paging(&q);
    let (total, groups) =
        blocking(move || accounts.scim_groups(&org, &terms, start, count)).await?;
    Ok(list(total, start, groups.iter().map(group_json).collect()))
}

fn member_ids(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|m| m.get("value").and_then(str_of).or_else(|| str_of(m)))
                .take(10_000)
                .collect()
        })
        .unwrap_or_default()
}

/// `POST /scim/v2/Groups`
pub async fn scim_create_group(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    raw: Bytes,
) -> ScimResult {
    let org = authorize_scim(&state, &headers)?;
    limit(&state, &org)?;
    let accounts = store(&state)?;
    let v = body(&raw)?;
    let name =
        v.get("displayName").and_then(str_of).filter(|n| n.chars().count() <= 200).ok_or_else(
            || ScimError::bad("invalidValue", "displayName is required, at most 200 characters"),
        )?;
    let external = v.get("externalId").and_then(str_of);
    let members = member_ids(v.get("members").unwrap_or(&Value::Null));
    let a = Arc::clone(&accounts);
    let o = org.clone();
    let made = blocking(move || a.insert_scim_group(&o, &name, external.as_deref())).await?;
    let made = made.ok_or_else(|| {
        ScimError::new(
            StatusCode::CONFLICT,
            Some("uniqueness"),
            "a group with that displayName is already provisioned",
        )
    });
    let gid = audit(&state, &headers, &peer, "scim_group_create", &org, None, made).await?;
    let a = Arc::clone(&accounts);
    let (o, g) = (org.clone(), gid.clone());
    let touched = blocking(move || a.change_group_members(&o, &g, false, &members, &[])).await?;
    reconcile_ids(&state, &accounts, &org, touched).await?;
    let o = org.clone();
    let group = blocking(move || accounts.scim_group(&o, &gid))
        .await?
        .ok_or_else(|| ScimError::not_found("group", "new"))?;
    Ok(Scim(StatusCode::CREATED, group_json(&group)))
}

/// `GET /scim/v2/Groups/{id}`
pub async fn scim_get_group(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ScimResult {
    let org = authorize_scim(&state, &headers)?;
    limit(&state, &org)?;
    let accounts = store(&state)?;
    let wanted = id.clone();
    let g = blocking(move || accounts.scim_group(&org, &wanted)).await?;
    g.map(|g| Scim(StatusCode::OK, group_json(&g)))
        .ok_or_else(|| ScimError::not_found("group", &id))
}

/// `PUT /scim/v2/Groups/{id}`
pub async fn scim_replace_group(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Path(id): Path<String>,
    raw: Bytes,
) -> ScimResult {
    let org = authorize_scim(&state, &headers)?;
    limit(&state, &org)?;
    let accounts = store(&state)?;
    let v = body(&raw)?;
    let name = v
        .get("displayName")
        .and_then(str_of)
        .ok_or_else(|| ScimError::bad("invalidValue", "displayName is required"))?;
    let external = v.get("externalId").and_then(str_of);
    let members = member_ids(v.get("members").unwrap_or(&Value::Null));
    let a = Arc::clone(&accounts);
    let (o, i) = (org.clone(), id.clone());
    let renamed = blocking(move || {
        if a.scim_group(&o, &i)?.is_none() {
            return Ok(None);
        }
        Ok(Some(a.update_scim_group(&o, &i, &name, external.as_deref())?))
    })
    .await?;
    let r = match renamed {
        None => Err(ScimError::not_found("group", &id)),
        Some(false) => Err(ScimError::new(
            StatusCode::CONFLICT,
            Some("uniqueness"),
            "another group holds that displayName",
        )),
        Some(true) => Ok(()),
    };
    audit(&state, &headers, &peer, "scim_group_update", &org, Some(&id), r).await?;
    let a = Arc::clone(&accounts);
    let (o, i) = (org.clone(), id.clone());
    let touched = blocking(move || a.change_group_members(&o, &i, true, &members, &[])).await?;
    reconcile_ids(&state, &accounts, &org, touched).await?;
    let o = org.clone();
    let g = blocking(move || accounts.scim_group(&o, &id))
        .await?
        .ok_or_else(|| ScimError::not_found("group", "gone"))?;
    Ok(Scim(StatusCode::OK, group_json(&g)))
}

/// Read a member id out of a path such as `members[value eq "usr_x"]`.
fn member_from_path(path: &str) -> Option<String> {
    let inner = path.split_once('[')?.1.strip_suffix(']')?;
    match parse_filter(inner).ok().as_deref() {
        Some([ScimTerm::Id(v)]) => Some(v.clone()),
        _ => {
            // `value eq "x"`, which parse_filter does not know as an
            // attribute of a user.
            let rest = inner.trim().strip_prefix("value")?.trim_start();
            let rest =
                rest.get(..2).filter(|op| op.eq_ignore_ascii_case("eq")).map(|_| &rest[2..])?;
            Some(rest.trim().trim_matches('"').to_owned())
        }
    }
}

/// `PATCH /scim/v2/Groups/{id}`
pub async fn scim_patch_group(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Path(id): Path<String>,
    raw: Bytes,
) -> ScimResult {
    let org = authorize_scim(&state, &headers)?;
    limit(&state, &org)?;
    let accounts = store(&state)?;
    let ops = patch_ops(&body(&raw)?)?;
    let a = Arc::clone(&accounts);
    let (o, i) = (org.clone(), id.clone());
    let group = blocking(move || a.scim_group(&o, &i))
        .await?
        .ok_or_else(|| ScimError::not_found("group", &id))?;
    let mut name = group.display_name.clone();
    let mut external = group.external_id.clone();
    let (mut replace, mut add, mut remove) = (false, Vec::new(), Vec::new());
    for op in &ops {
        let kind = op.get("op").and_then(|o| o.as_str()).unwrap_or("").to_ascii_lowercase();
        let path = op.get("path").and_then(|p| p.as_str()).unwrap_or("");
        let lpath = path.to_ascii_lowercase();
        let value = op.get("value").cloned().unwrap_or(Value::Null);
        match (kind.as_str(), lpath.as_str()) {
            ("add", "members") => add.extend(member_ids(&value)),
            ("remove", "members") => {
                if value.is_null() {
                    replace = true;
                    add.clear();
                } else {
                    remove.extend(member_ids(&value));
                }
            }
            ("remove", p) if p.starts_with("members[") => {
                if let Some(m) = member_from_path(path) {
                    remove.push(m);
                }
            }
            ("replace", "members") => {
                replace = true;
                add = member_ids(&value);
            }
            ("replace" | "add", "displayname") => {
                name = str_of(&value)
                    .ok_or_else(|| ScimError::bad("invalidValue", "displayName cannot be empty"))?;
            }
            ("replace" | "add", "externalid") => external = str_of(&value),
            ("replace" | "add", "") => {
                if let Some(n) = value.get("displayName").and_then(str_of) {
                    name = n;
                }
                if let Some(x) = value.get("externalId") {
                    external = str_of(x);
                }
                if let Some(ms) = value.get("members") {
                    replace = true;
                    add = member_ids(ms);
                }
            }
            ("add" | "replace" | "remove", _) => {
                return Err(ScimError::bad(
                    "invalidPath",
                    format!("the path {path:?} is not supported on a group"),
                ));
            }
            _ => return Err(ScimError::bad("invalidSyntax", format!("unknown PATCH op {kind:?}"))),
        }
    }
    let a = Arc::clone(&accounts);
    let (o, i) = (org.clone(), id.clone());
    let renamed = blocking(move || a.update_scim_group(&o, &i, &name, external.as_deref())).await?;
    let r = if renamed {
        Ok(())
    } else {
        Err(ScimError::new(
            StatusCode::CONFLICT,
            Some("uniqueness"),
            "another group holds that displayName",
        ))
    };
    audit(&state, &headers, &peer, "scim_group_update", &org, Some(&id), r).await?;
    let a = Arc::clone(&accounts);
    let (o, i) = (org.clone(), id.clone());
    let touched = blocking(move || a.change_group_members(&o, &i, replace, &add, &remove)).await?;
    reconcile_ids(&state, &accounts, &org, touched).await?;
    let o = org.clone();
    let g = blocking(move || accounts.scim_group(&o, &id))
        .await?
        .ok_or_else(|| ScimError::not_found("group", "gone"))?;
    Ok(Scim(StatusCode::OK, group_json(&g)))
}

/// `DELETE /scim/v2/Groups/{id}`
pub async fn scim_delete_group(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Path(id): Path<String>,
) -> Result<StatusCode, ScimError> {
    let org = authorize_scim(&state, &headers)?;
    limit(&state, &org)?;
    let accounts = store(&state)?;
    let a = Arc::clone(&accounts);
    let (o, i) = (org.clone(), id.clone());
    let members = blocking(move || a.delete_scim_group(&o, &i)).await?;
    let members = audit(
        &state,
        &headers,
        &peer,
        "scim_group_delete",
        &org,
        Some(&id),
        members.ok_or_else(|| ScimError::not_found("group", &id)),
    )
    .await?;
    reconcile_ids(&state, &accounts, &org, members).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The SCIM routes. No CORS: an identity provider is not a browser.
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/scim/v2/ServiceProviderConfig", get(scim_service_provider_config))
        .route("/scim/v2/Schemas", get(scim_schemas))
        .route("/scim/v2/Schemas/{id}", get(scim_schema))
        .route("/scim/v2/ResourceTypes", get(scim_resource_types))
        .route("/scim/v2/ResourceTypes/{id}", get(scim_resource_type))
        .route("/scim/v2/Users", get(scim_list_users))
        .route("/scim/v2/Users", post(scim_create_user))
        .route("/scim/v2/Users/{id}", get(scim_get_user))
        .route("/scim/v2/Users/{id}", put(scim_replace_user))
        .route("/scim/v2/Users/{id}", patch(scim_patch_user))
        .route("/scim/v2/Users/{id}", delete(scim_delete_user))
        .route("/scim/v2/Groups", get(scim_list_groups))
        .route("/scim/v2/Groups", post(scim_create_group))
        .route("/scim/v2/Groups/{id}", get(scim_get_group))
        .route("/scim/v2/Groups/{id}", put(scim_replace_group))
        .route("/scim/v2/Groups/{id}", patch(scim_patch_group))
        .route("/scim/v2/Groups/{id}", delete(scim_delete_group))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_filter_of_eq_terms_parses_and_anything_else_is_refused() {
        assert_eq!(
            parse_filter(r#"userName eq "Ana@Example.com""#).unwrap(),
            vec![ScimTerm::UserName("Ana@Example.com".into())]
        );
        assert_eq!(
            parse_filter(r#"externalId eq "x\"y" and emails.value EQ "a@b.co""#).unwrap(),
            vec![ScimTerm::ExternalId("x\"y".into()), ScimTerm::Email("a@b.co".into())]
        );
        for bad in [
            r#"userName co "a""#,
            r#"title eq "x""#,
            "userName eq unquoted",
            r#"userName eq "a" or id eq "b""#,
        ] {
            assert!(parse_filter(bad).is_err(), "{bad} was accepted");
        }
    }

    #[test]
    fn patch_reads_the_shapes_identity_providers_send() {
        let mut f = ScimUserFields {
            user_name: "a@b.co".into(),
            email: "a@b.co".into(),
            active: true,
            ..Default::default()
        };
        patch_user(&mut f, &json!({"schemas":[PATCH_SCHEMA],"Operations":[{"op":"Replace","path":"active","value":"False"}]})).unwrap();
        assert!(!f.active);
        patch_user(
            &mut f,
            &json!({"Operations":[{"op":"replace","value":{"active":true,"displayName":"Ana"}}]}),
        )
        .unwrap();
        assert!(f.active);
        assert_eq!(f.display_name.as_deref(), Some("Ana"));
        patch_user(&mut f, &json!({"Operations":[{"op":"replace","path":"title","value":"x"}]}))
            .unwrap();
        assert!(patch_user(&mut f, &json!({"Operations":[{"op":"move","path":"active"}]})).is_err());
        assert!(patch_user(&mut f, &json!({"schemas":["wrong"],"Operations":[]})).is_err());
    }

    #[test]
    fn a_member_path_names_its_user() {
        assert_eq!(member_from_path(r#"members[value eq "usr_1"]"#).as_deref(), Some("usr_1"));
        assert_eq!(member_from_path("members").as_deref(), None);
    }
}
