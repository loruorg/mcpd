//! Single sign-on: OpenID Connect with any provider, and GitHub over OAuth.
//!
//! The flow is the authorization code flow with PKCE, and three values tie
//! each callback to the request that started it:
//!
//! * `state` is a 256-bit one-time token the daemon issued, kept as a digest
//!   for ten minutes and used up by the callback. A callback with a state the
//!   daemon did not issue, or issued and has seen used, is refused.
//! * `nonce` goes into the authorization request and must come back inside the
//!   signed ID token, so an ID token minted for another login cannot be
//!   replayed into this one.
//! * The PKCE verifier stays in the daemon; only its SHA-256 goes to the
//!   provider, so an intercepted code cannot be exchanged by anyone else.
//!
//! One more, because the redirect lands in a browser and not at the daemon: a
//! `binding` secret the starting browser keeps and sends back with the
//! callback. Without it, somebody could start a login, stop at the redirect and
//! send the link to somebody else, who would then be logged in as the sender
//! (login CSRF). The daemon keeps only its digest.
//!
//! The ID token's signature is checked against the provider's published keys,
//! and so are its issuer, audience and expiry. Symmetric algorithms and `none`
//! are refused outright: a token signed with the client secret proves only that
//! somebody holds the secret.
//!
//! ## Who a login may become
//!
//! A person is found by their identity at the provider (issuer and subject),
//! never by email alone. The first time an identity is seen, an address is
//! only good for a new account when its domain is one the organisation claims,
//! and an existing account is linked only when it is already a member of the
//! organisation. An identity provider therefore cannot log in as an account it
//! does not own, and the session it produces reaches only its own
//! organisation's projects.

use crate::accounts::roles::Role;
use crate::accounts::{
    self, blocking, email_domain, is_domain, store, SsoSettings, SsoStateData, TokenPurpose,
};
use crate::auth::authorize_session;
use crate::error::{ApiError, ApiResult};
use crate::orgs::member_of;
use crate::query::{audited_as, Peer};
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use base64::Engine;
use mcpd_core::{
    AccountSession, CreatedScimToken, DeleteResult, PutSsoConfigRequest, ScimTokenInfo,
    SsoCallbackRequest, SsoConfig, SsoProvider, SsoStart, SsoStartRequest,
};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use time::OffsetDateTime;

/// How long a flow may take from start to callback, in seconds.
pub const STATE_TTL_SECONDS: i64 = 600;

/// How long a provider's discovery document and keys are trusted before they
/// are fetched again.
pub const PROVIDER_TTL: Duration = Duration::from_secs(3600);

/// Most claimed domains per organisation.
pub const MAX_DOMAINS: usize = 20;

const GITHUB_AUTHORIZE: &str = "https://github.com/login/oauth/authorize";
const GITHUB_TOKEN: &str = "https://github.com/login/oauth/access_token";
const GITHUB_API: &str = "https://api.github.com";

/// What the daemon needs from a provider's discovery document.
#[derive(Debug, Clone, serde::Deserialize, PartialEq, Eq)]
pub struct Discovery {
    /// The issuer, which must equal the configured one.
    pub issuer: String,
    /// Where the browser goes.
    pub authorization_endpoint: String,
    /// Where the code is exchanged.
    pub token_endpoint: String,
    /// The signing keys.
    pub jwks_uri: String,
}

/// Discovery documents and signing keys, per issuer.
#[derive(Default)]
pub struct ProviderCache {
    entries: Mutex<HashMap<String, (Instant, Discovery, jsonwebtoken::jwk::JwkSet)>>,
}

impl ProviderCache {
    fn get(&self, issuer: &str) -> Option<(Discovery, jsonwebtoken::jwk::JwkSet)> {
        let e = self.entries.lock().ok()?;
        let (at, d, k) = e.get(issuer)?;
        (at.elapsed() < PROVIDER_TTL).then(|| (d.clone(), k.clone()))
    }

    fn put(&self, issuer: &str, d: Discovery, k: jsonwebtoken::jwk::JwkSet) {
        if let Ok(mut e) = self.entries.lock() {
            // Bounded by the number of organisations with single sign-on, and
            // cleared of stale entries whenever it grows.
            if e.len() > 1000 {
                e.retain(|_, (at, _, _)| at.elapsed() < PROVIDER_TTL);
            }
            e.insert(issuer.to_owned(), (Instant::now(), d, k));
        }
    }

    fn forget(&self, issuer: &str) {
        if let Ok(mut e) = self.entries.lock() {
            e.remove(issuer);
        }
    }
}

/// A form body, URL-encoded.
fn form(pairs: &[(&str, &str)]) -> String {
    let mut f = url::form_urlencoded::Serializer::new(String::new());
    for (k, v) in pairs {
        f.append_pair(k, v);
    }
    f.finish()
}

fn b64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn random_b64(n: usize) -> String {
    let bytes: Vec<u8> = (0..n).map(|_| rand::random::<u8>()).collect();
    b64url(&bytes)
}

/// The PKCE S256 challenge for a verifier.
#[must_use]
pub fn pkce_challenge(verifier: &str) -> String {
    b64url(&Sha256::digest(verifier.as_bytes()))
}

fn constant_eq(a: &str, b: &str) -> bool {
    let (ha, hb) = (Sha256::digest(a.as_bytes()), Sha256::digest(b.as_bytes()));
    ha.iter().zip(hb.iter()).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

/// The redirect URL to register with a provider.
#[must_use]
pub fn redirect_url(state: &AppState) -> String {
    format!("{}/", state.config.account_settings.base_url())
}

fn sso_failed(why: impl Into<String>) -> ApiError {
    ApiError::BadParam { field: "code".to_owned(), message: why.into() }
}

/// Whether a URL may be fetched as an issuer. `https` only, and not an address
/// on a private, loopback or link-local network, unless the configuration
/// allows it: on a shared daemon an organisation admin chooses this URL, and
/// the daemon must not become a way to reach its own internal network.
async fn check_issuer(state: &AppState, issuer: &str, trusted: bool) -> ApiResult<()> {
    let bad = |m: String| ApiError::BadParam { field: "issuer".to_owned(), message: m };
    let parsed = url::Url::parse(issuer).map_err(|_| bad(format!("{issuer:?} is not a URL")))?;
    let private_ok = trusted || state.config.account_settings.allow_private_issuers;
    if parsed.scheme() != "https" && !(private_ok && parsed.scheme() == "http") {
        return Err(bad("the issuer must be an https URL".to_owned()));
    }
    let Some(host) = parsed.host_str() else {
        return Err(bad("the issuer has no host".to_owned()));
    };
    if private_ok {
        return Ok(());
    }
    let port = parsed.port_or_known_default().unwrap_or(443);
    let addrs = tokio::net::lookup_host((host, port))
        .await
        .map_err(|_| bad(format!("the issuer host {host:?} does not resolve")))?;
    for a in addrs {
        let ip = a.ip();
        let private = match ip {
            std::net::IpAddr::V4(v) => {
                v.is_private()
                    || v.is_loopback()
                    || v.is_link_local()
                    || v.is_unspecified()
                    || v.is_broadcast()
                    || v.octets()[0] == 100 && (v.octets()[1] & 0xc0) == 64
            }
            std::net::IpAddr::V6(v) => {
                v.is_loopback()
                    || v.is_unspecified()
                    || (v.segments()[0] & 0xfe00) == 0xfc00
                    || (v.segments()[0] & 0xffc0) == 0xfe80
                    || v.to_ipv4_mapped().is_some_and(|m| m.is_private() || m.is_loopback())
            }
        };
        if private {
            return Err(bad(format!("the issuer host {host:?} is on a private network")));
        }
    }
    Ok(())
}

/// The provider's discovery document and keys, from the cache or the network.
async fn provider(
    state: &AppState,
    issuer: &str,
    fresh: bool,
) -> ApiResult<(Discovery, jsonwebtoken::jwk::JwkSet)> {
    let cache = &state.account_runtime.providers;
    if !fresh {
        if let Some(hit) = cache.get(issuer) {
            return Ok(hit);
        }
    }
    let url = format!("{}/.well-known/openid-configuration", issuer.trim_end_matches('/'));
    let fetch = |u: String| async move {
        let r = state.outbound.get(&u).send().await.map_err(crate::outbound_error)?;
        if !r.status().is_success() {
            return Err(format!("answered {}", r.status()));
        }
        r.bytes().await.map_err(crate::outbound_error)
    };
    let body = fetch(url).await.map_err(|e| {
        sso_failed(format!("the issuer's discovery document could not be read: {e}"))
    })?;
    let d: Discovery = serde_json::from_slice(&body)
        .map_err(|e| sso_failed(format!("the issuer's discovery document is not valid: {e}")))?;
    if d.issuer.trim_end_matches('/') != issuer.trim_end_matches('/') {
        return Err(sso_failed(format!(
            "the discovery document names the issuer {:?}, not {issuer:?}",
            d.issuer
        )));
    }
    let keys = fetch(d.jwks_uri.clone())
        .await
        .map_err(|e| sso_failed(format!("the issuer's signing keys could not be read: {e}")))?;
    let keys: jsonwebtoken::jwk::JwkSet = serde_json::from_slice(&keys)
        .map_err(|e| sso_failed(format!("the issuer's signing keys are not valid: {e}")))?;
    cache.put(issuer, d.clone(), keys.clone());
    Ok((d, keys))
}

/// What a verified ID token says about the person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    /// The stable identity: issuer and subject, or `github|<id>`.
    pub subject: String,
    /// The address, lowercased, when the provider gave one.
    pub email: Option<String>,
    /// False only when the provider said outright that the address is not
    /// verified.
    pub email_verified: bool,
    /// Groups, when a groups claim is configured and present.
    pub groups: Vec<String>,
}

/// Verify an ID token against a provider's keys, and read the person from it.
///
/// # Errors
/// A 400 naming what failed: the algorithm, the key, the signature, the
/// issuer, the audience, the expiry or the nonce.
pub fn verify_id_token(
    token: &str,
    keys: &jsonwebtoken::jwk::JwkSet,
    issuer: &str,
    client_id: &str,
    nonce: &str,
    groups_claim: Option<&str>,
) -> ApiResult<Identity> {
    use jsonwebtoken::Algorithm::{EdDSA, ES256, ES384, PS256, PS384, PS512, RS256, RS384, RS512};
    let header = jsonwebtoken::decode_header(token)
        .map_err(|e| sso_failed(format!("the ID token is not a JWT: {e}")))?;
    if !matches!(header.alg, RS256 | RS384 | RS512 | PS256 | PS384 | PS512 | ES256 | ES384 | EdDSA)
    {
        return Err(sso_failed(format!(
            "the ID token is signed with {:?}, which is not accepted",
            header.alg
        )));
    }
    let jwk = match &header.kid {
        Some(kid) => keys.find(kid),
        None if keys.keys.len() == 1 => keys.keys.first(),
        None => None,
    }
    .ok_or_else(|| sso_failed("the ID token is signed with a key the issuer does not publish"))?;
    let key = jsonwebtoken::DecodingKey::from_jwk(jwk)
        .map_err(|e| sso_failed(format!("the issuer's key cannot be used: {e}")))?;
    let mut v = jsonwebtoken::Validation::new(header.alg);
    // Both spellings: some issuers publish a trailing slash and some do not,
    // and the discovery document has already been held to the configured one.
    let trimmed = issuer.trim_end_matches('/');
    v.set_issuer(&[trimmed.to_owned(), format!("{trimmed}/")]);
    v.set_audience(&[client_id]);
    v.required_spec_claims = ["exp", "iss", "aud", "sub"].iter().map(|s| (*s).to_owned()).collect();
    let data = jsonwebtoken::decode::<serde_json::Value>(token, &key, &v)
        .map_err(|e| sso_failed(format!("the ID token was refused: {e}")))?;
    let c = data.claims;
    let got_nonce = c.get("nonce").and_then(|n| n.as_str()).unwrap_or("");
    if !constant_eq(got_nonce, nonce) {
        return Err(sso_failed("the ID token's nonce is not the one this login sent"));
    }
    let sub = c
        .get("sub")
        .and_then(|s| s.as_str())
        .filter(|s| !s.is_empty() && s.len() <= 255)
        .ok_or_else(|| sso_failed("the ID token has no subject"))?;
    let email = c
        .get("email")
        .and_then(|e| e.as_str())
        .map(|e| e.trim().to_lowercase())
        .filter(|e| accounts::normalise_email(e).is_ok());
    let email_verified = match c.get("email_verified") {
        Some(serde_json::Value::Bool(b)) => *b,
        Some(serde_json::Value::String(s)) => s != "false",
        _ => true,
    };
    let groups = groups_claim
        .and_then(|g| c.get(g))
        .and_then(|g| g.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str()).take(500).map(str::to_owned).collect())
        .unwrap_or_default();
    Ok(Identity { subject: format!("{trimmed}|{sub}"), email, email_verified, groups })
}

/// `GET /v1/api/orgs/{org_id}/sso`
pub async fn get_sso(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(org_id): Path<String>,
) -> ApiResult<Json<SsoConfig>> {
    let (credential, _) = authorize_session(&state, &headers)?;
    let m = member_of(&state, &credential, &org_id).await?;
    need_manager(m.role)?;
    let a = Arc::clone(&m.accounts);
    let s = blocking(move || a.sso_settings(&org_id)).await?.ok_or_else(|| {
        ApiError::NotFound("this organisation has no single sign-on settings".to_owned())
    })?;
    Ok(Json(to_wire(&state, &s)))
}

fn need_manager(role: Role) -> ApiResult<()> {
    if role.manages() {
        Ok(())
    } else {
        Err(ApiError::Forbidden(
            "this needs the admin or owner role in the organisation".to_owned(),
        ))
    }
}

fn to_wire(state: &AppState, s: &SsoSettings) -> SsoConfig {
    SsoConfig {
        provider: if s.provider == "github" { SsoProvider::Github } else { SsoProvider::Oidc },
        issuer: s.issuer.clone(),
        client_id: s.client_id.clone(),
        has_client_secret: s.client_secret.is_some(),
        domains: s.domains.clone(),
        enforced: s.enforced,
        default_role: s.default_role.to_wire(),
        groups_claim: s.groups_claim.clone(),
        redirect_url: redirect_url(state),
        managed_by_config: s.config_managed,
        updated_at: OffsetDateTime::from_unix_timestamp(s.updated_at)
            .unwrap_or(OffsetDateTime::UNIX_EPOCH),
    }
}

/// Validate settings for storing. `admin_email` is the address of whoever is
/// claiming the domains, or `None` for the configuration file, which may claim
/// any.
pub(crate) async fn validate_settings(
    state: &AppState,
    org_id: &str,
    req: &PutSsoConfigRequest,
    admin_email: Option<&str>,
    trusted: bool,
) -> ApiResult<SsoSettings> {
    let bad = |field: &str, m: String| ApiError::BadParam { field: field.to_owned(), message: m };
    let provider = match req.provider {
        SsoProvider::Oidc => "oidc",
        SsoProvider::Github => "github",
    };
    let issuer = match req.provider {
        SsoProvider::Github => None,
        SsoProvider::Oidc => {
            let i = req.issuer.as_deref().map(str::trim).filter(|i| !i.is_empty()).ok_or_else(
                || bad("issuer", "an OpenID Connect provider needs its issuer URL".to_owned()),
            )?;
            check_issuer(state, i, trusted).await?;
            Some(i.trim_end_matches('/').to_owned())
        }
    };
    let client_id = req.client_id.trim();
    if client_id.is_empty() || client_id.len() > 256 || client_id.chars().any(char::is_control) {
        return Err(bad("client_id", "the client id is 1 to 256 characters".to_owned()));
    }
    if req.client_secret.as_ref().is_some_and(|s| s.len() > 1024) {
        return Err(bad("client_secret", "the client secret is at most 1024 bytes".to_owned()));
    }
    if req.domains.len() > MAX_DOMAINS {
        return Err(bad(
            "domains",
            format!("an organisation claims at most {MAX_DOMAINS} domains"),
        ));
    }
    let mut domains: Vec<String> = Vec::new();
    for d in &req.domains {
        let d = d.trim().trim_start_matches('@').to_lowercase();
        if !is_domain(&d) {
            return Err(bad("domains", format!("{d:?} is not a domain name")));
        }
        if let Some(admin) = admin_email {
            if email_domain(admin) != d {
                return Err(ApiError::Forbidden(format!(
                    "you can claim the domain of your own address, {}, and not {d:?}; ask the \
                     operator to claim others in the configuration file",
                    email_domain(admin)
                )));
            }
        }
        if !domains.contains(&d) {
            domains.push(d);
        }
    }
    let default_role = Role::from_wire(req.default_role);
    if default_role == Role::Owner {
        return Err(bad(
            "default_role",
            "single sign-on cannot create owners; use admin at most".to_owned(),
        ));
    }
    let groups_claim =
        req.groups_claim.as_deref().map(str::trim).filter(|g| !g.is_empty()).map(str::to_owned);
    if groups_claim.as_ref().is_some_and(|g| g.len() > 100) {
        return Err(bad(
            "groups_claim",
            "the groups claim name is at most 100 characters".to_owned(),
        ));
    }
    // Checked now, so a typo is refused here rather than at somebody's first
    // log-in. Not for the configuration file: a provider that is down while
    // the daemon starts must not stop it starting.
    if let (Some(i), false) = (&issuer, trusted) {
        provider_for(state, i, true).await?;
    }
    Ok(SsoSettings {
        org_id: org_id.to_owned(),
        provider: provider.to_owned(),
        issuer,
        client_id: client_id.to_owned(),
        client_secret: req.client_secret.clone().filter(|s| !s.is_empty()),
        domains,
        enforced: req.enforced,
        default_role,
        groups_claim,
        config_managed: trusted,
        updated_at: 0,
    })
}

async fn provider_for(
    state: &AppState,
    issuer: &str,
    fresh: bool,
) -> ApiResult<(Discovery, jsonwebtoken::jwk::JwkSet)> {
    provider(state, issuer, fresh).await
}

/// Store settings: domains checked against other organisations, and password
/// sessions ended for members enforcement now covers.
pub(crate) async fn store_settings(state: &AppState, s: SsoSettings) -> ApiResult<()> {
    let accounts = store(state)?;
    let a = Arc::clone(&accounts);
    let (org, domains) = (s.org_id.clone(), s.domains.clone());
    if let accounts::sso_store::DomainClaim::Taken(d) =
        blocking(move || a.check_domains(&org, &domains)).await?
    {
        return Err(ApiError::Conflict(format!("another organisation claims the domain {d:?}")));
    }
    let a = Arc::clone(&accounts);
    let affected = blocking(move || a.put_sso_settings(&s)).await?;
    for id in affected {
        let a = Arc::clone(&accounts);
        blocking(move || a.end_password_sessions(&id)).await?;
    }
    Ok(())
}

/// `PUT /v1/api/orgs/{org_id}/sso`
pub async fn put_sso(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Path(org_id): Path<String>,
    Json(req): Json<PutSsoConfigRequest>,
) -> ApiResult<Json<SsoConfig>> {
    let (credential, _) = authorize_session(&state, &headers)?;
    let m = member_of(&state, &credential, &org_id).await?;
    let a = Arc::clone(&m.accounts);
    let id = org_id.clone();
    let existing = blocking(move || a.sso_settings(&id)).await?;
    let mut decision = need_manager(m.role);
    if decision.is_ok() && existing.as_ref().is_some_and(|e| e.config_managed) {
        decision = Err(ApiError::Conflict(
            "the configuration file sets this organisation's single sign-on; change it there"
                .to_owned(),
        ));
    }
    let settings = match decision {
        Ok(()) => {
            // On a daemon open to sign-up, an admin claims only their own
            // domain. Self-hosted, every account was let in by the operator.
            let claimer = state.config.signup.then_some(m.email.as_str());
            validate_settings(&state, &org_id, &req, claimer, false).await
        }
        Err(e) => Err(e),
    };
    let settings =
        audited_as(&state, &headers, peer.0, settings, "sso_update", &org_id, None, &m.email)
            .await?;
    store_settings(&state, settings).await?;
    let a = Arc::clone(&m.accounts);
    let s = blocking(move || a.sso_settings(&org_id))
        .await?
        .ok_or_else(|| ApiError::Internal("the settings just stored are gone".to_owned()))?;
    Ok(Json(to_wire(&state, &s)))
}

/// `DELETE /v1/api/orgs/{org_id}/sso`
pub async fn delete_sso(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Path(org_id): Path<String>,
) -> ApiResult<Json<DeleteResult>> {
    let (credential, _) = authorize_session(&state, &headers)?;
    let m = member_of(&state, &credential, &org_id).await?;
    let a = Arc::clone(&m.accounts);
    let id = org_id.clone();
    let existing = blocking(move || a.sso_settings(&id)).await?;
    let mut decision = need_manager(m.role);
    if decision.is_ok() && existing.as_ref().is_some_and(|e| e.config_managed) {
        decision = Err(ApiError::Conflict(
            "the configuration file sets this organisation's single sign-on; remove it there"
                .to_owned(),
        ));
    }
    audited_as(&state, &headers, peer.0, decision, "sso_delete", &org_id, None, &m.email).await?;
    let a = Arc::clone(&m.accounts);
    let n = blocking(move || a.delete_sso_settings(&org_id)).await?;
    Ok(Json(DeleteResult { deleted: u32::from(n) }))
}

/// The SCIM base URL, as the request reached the daemon. Shown to an admin to
/// copy, and never used to decide anything.
fn scim_base(headers: &HeaderMap) -> String {
    let h = |n: &str| headers.get(n).and_then(|v| v.to_str().ok()).map(str::to_owned);
    let host =
        h("x-forwarded-host").or_else(|| h("host")).unwrap_or_else(|| "localhost:4317".to_owned());
    let proto = h("x-forwarded-proto").unwrap_or_else(|| {
        if host.starts_with("localhost") || host.starts_with("127.") {
            "http".to_owned()
        } else {
            "https".to_owned()
        }
    });
    let host: String =
        host.chars().filter(|c| c.is_ascii_alphanumeric() || ".-:[]".contains(*c)).collect();
    let proto = if proto == "http" { "http" } else { "https" };
    format!("{proto}://{host}/scim/v2")
}

/// `GET /v1/api/orgs/{org_id}/scim-token`
pub async fn get_scim_token(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(org_id): Path<String>,
) -> ApiResult<Json<ScimTokenInfo>> {
    let (credential, _) = authorize_session(&state, &headers)?;
    let m = member_of(&state, &credential, &org_id).await?;
    need_manager(m.role)?;
    let a = Arc::clone(&m.accounts);
    let info = blocking(move || a.scim_token_info(&org_id)).await?;
    Ok(Json(ScimTokenInfo {
        configured: info.is_some(),
        prefix: info.as_ref().map(|(p, _)| p.clone()),
        created_at: info.map(|(_, at)| at),
        base_url: scim_base(&headers),
    }))
}

/// `POST /v1/api/orgs/{org_id}/scim-token`
pub async fn create_scim_token(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Path(org_id): Path<String>,
) -> ApiResult<Json<CreatedScimToken>> {
    let (credential, _) = authorize_session(&state, &headers)?;
    let m = member_of(&state, &credential, &org_id).await?;
    audited_as(
        &state,
        &headers,
        peer.0,
        need_manager(m.role),
        "scim_token_rotate",
        &org_id,
        None,
        &m.email,
    )
    .await?;
    let a = Arc::clone(&m.accounts);
    let (token, prefix, created_at) = blocking(move || a.rotate_scim_token(&org_id)).await?;
    Ok(Json(CreatedScimToken { token, prefix, created_at, base_url: scim_base(&headers) }))
}

/// `DELETE /v1/api/orgs/{org_id}/scim-token`
pub async fn revoke_scim_token(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Path(org_id): Path<String>,
) -> ApiResult<Json<DeleteResult>> {
    let (credential, _) = authorize_session(&state, &headers)?;
    let m = member_of(&state, &credential, &org_id).await?;
    audited_as(
        &state,
        &headers,
        peer.0,
        need_manager(m.role),
        "scim_token_revoke",
        &org_id,
        None,
        &m.email,
    )
    .await?;
    let a = Arc::clone(&m.accounts);
    let n = blocking(move || a.revoke_scim_token(&org_id)).await?;
    Ok(Json(DeleteResult { deleted: u32::from(n) }))
}

/// `POST /v1/api/sso/start`
pub async fn sso_start(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Json(req): Json<SsoStartRequest>,
) -> ApiResult<Json<SsoStart>> {
    let accounts = store(&state)?;
    state
        .account_guards
        .sso
        .hit(&accounts::client(&state, &headers, &peer))
        .map_err(|s| accounts::limited(s, "too many single sign-on attempts from this address"))?;
    let none = || ApiError::BadParam {
        field: "email".to_owned(),
        message: "no organisation on this daemon logs in with single sign-on for that address"
            .to_owned(),
    };
    let org_id = match (
        req.org_id.as_deref().map(str::trim).filter(|o| !o.is_empty()),
        req.email.as_deref(),
    ) {
        (Some(o), _) => o.to_owned(),
        (None, Some(e)) => {
            let email = accounts::normalise_email(e)?;
            let a = Arc::clone(&accounts);
            let d = email_domain(&email).to_owned();
            blocking(move || a.org_for_domain(&d)).await?.ok_or_else(none)?
        }
        (None, None) => return Err(none()),
    };
    let a = Arc::clone(&accounts);
    let id = org_id.clone();
    let (settings, row) = blocking(move || Ok((a.sso_settings(&id)?, a.org_row(&id)?))).await?;
    let (Some(settings), Some(row)) = (settings, row) else { return Err(none()) };
    let verifier = random_b64(32);
    let nonce = random_b64(24);
    let binding = random_b64(32);
    let data = SsoStateData {
        org_id: org_id.clone(),
        nonce: nonce.clone(),
        verifier: verifier.clone(),
        binding: hex::encode(Sha256::digest(binding.as_bytes())),
    };
    let json = serde_json::to_string(&data).map_err(|e| ApiError::Internal(e.to_string()))?;
    let a = Arc::clone(&accounts);
    let st = blocking(move || {
        a.issue_token(TokenPurpose::SsoState, None, Some(&json), STATE_TTL_SECONDS)
    })
    .await?;
    let redirect = redirect_url(&state);
    let mut url = if settings.provider == "github" {
        url::Url::parse(GITHUB_AUTHORIZE).map_err(|e| ApiError::Internal(e.to_string()))?
    } else {
        let issuer = settings.issuer.clone().unwrap_or_default();
        let (d, _) = provider(&state, &issuer, false).await?;
        url::Url::parse(&d.authorization_endpoint)
            .map_err(|_| sso_failed("the issuer's authorization endpoint is not a URL"))?
    };
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("response_type", "code")
            .append_pair("client_id", &settings.client_id)
            .append_pair("redirect_uri", &redirect)
            .append_pair("state", &st)
            .append_pair("code_challenge", &pkce_challenge(&verifier))
            .append_pair("code_challenge_method", "S256");
        if settings.provider == "github" {
            q.append_pair("scope", "read:user user:email");
        } else {
            q.append_pair("scope", "openid email profile").append_pair("nonce", &nonce);
            if let Some(e) = req.email.as_deref() {
                q.append_pair("login_hint", e.trim());
            }
        }
    }
    Ok(Json(SsoStart { authorize_url: url.to_string(), binding, org_name: row.name }))
}

/// Exchange a code and read the person, for either provider kind.
async fn identify(
    state: &AppState,
    s: &SsoSettings,
    data: &SsoStateData,
    code: &str,
) -> ApiResult<Identity> {
    let redirect = redirect_url(state);
    let secret = s.client_secret.clone().unwrap_or_default();
    if s.provider == "github" {
        let r = state
            .outbound
            .post(GITHUB_TOKEN)
            .header("accept", "application/json")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(form(&[
                ("client_id", s.client_id.as_str()),
                ("client_secret", secret.as_str()),
                ("code", code),
                ("redirect_uri", redirect.as_str()),
                ("code_verifier", data.verifier.as_str()),
            ]))
            .send()
            .await
            .map_err(|e| {
                sso_failed(format!("GitHub could not be reached: {}", crate::outbound_error(e)))
            })?;
        let body: serde_json::Value =
            r.json().await.map_err(|_| sso_failed("GitHub's answer was not JSON"))?;
        let token = body
            .get("access_token")
            .and_then(|t| t.as_str())
            .ok_or_else(|| sso_failed("GitHub refused the code"))?;
        let get = |path: &'static str| {
            state
                .outbound
                .get(format!("{GITHUB_API}{path}"))
                .header("authorization", format!("Bearer {token}"))
                .header("accept", "application/vnd.github+json")
                .header("user-agent", "mcpd")
                .send()
        };
        let user: serde_json::Value = get("/user")
            .await
            .map_err(|_| sso_failed("GitHub could not be reached"))?
            .json()
            .await
            .map_err(|_| sso_failed("GitHub's user was not JSON"))?;
        let id = user
            .get("id")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| sso_failed("GitHub sent no user id"))?;
        let emails: serde_json::Value = get("/user/emails")
            .await
            .map_err(|_| sso_failed("GitHub could not be reached"))?
            .json()
            .await
            .map_err(|_| sso_failed("GitHub's emails were not JSON"))?;
        let email = emails
            .as_array()
            .into_iter()
            .flatten()
            .find(|e| {
                e.get("primary").and_then(serde_json::Value::as_bool) == Some(true)
                    && e.get("verified").and_then(serde_json::Value::as_bool) == Some(true)
            })
            .and_then(|e| e.get("email"))
            .and_then(|e| e.as_str())
            .map(str::to_lowercase);
        return Ok(Identity {
            subject: format!("github|{id}"),
            email,
            email_verified: true,
            groups: Vec::new(),
        });
    }
    let issuer = s.issuer.clone().unwrap_or_default();
    let (d, mut keys) = provider(state, &issuer, false).await?;
    let r = state
        .outbound
        .post(&d.token_endpoint)
        .basic_auth(
            url::form_urlencoded::byte_serialize(s.client_id.as_bytes()).collect::<String>(),
            Some(url::form_urlencoded::byte_serialize(secret.as_bytes()).collect::<String>()),
        )
        .header("content-type", "application/x-www-form-urlencoded")
        .body(form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect.as_str()),
            ("client_id", s.client_id.as_str()),
            ("code_verifier", data.verifier.as_str()),
        ]))
        .send()
        .await
        .map_err(|e| {
            sso_failed(format!("the provider could not be reached: {}", crate::outbound_error(e)))
        })?;
    let status = r.status();
    let body: serde_json::Value =
        r.json().await.map_err(|_| sso_failed("the provider's token answer was not JSON"))?;
    if !status.is_success() {
        let why = body.get("error").and_then(|e| e.as_str()).unwrap_or("no reason given");
        return Err(sso_failed(format!("the provider refused the code: {why}")));
    }
    let id_token = body
        .get("id_token")
        .and_then(|t| t.as_str())
        .ok_or_else(|| sso_failed("the provider sent no ID token"))?;
    // A key the cache does not hold may be a rotation since it was fetched:
    // fetch once more before refusing.
    let kid = jsonwebtoken::decode_header(id_token).ok().and_then(|h| h.kid);
    if kid.as_deref().is_some_and(|k| keys.find(k).is_none()) {
        state.account_runtime.providers.forget(&issuer);
        keys = provider(state, &issuer, true).await?.1;
    }
    verify_id_token(id_token, &keys, &issuer, &s.client_id, &data.nonce, s.groups_claim.as_deref())
}

/// Decide which account an identity becomes, creating or linking as the rules
/// in the module documentation allow, and set its role.
fn resolve(
    accounts: &accounts::AccountStore,
    s: &SsoSettings,
    who: &Identity,
    config_owners: &[String],
) -> Result<Result<String, String>, accounts::AccountError> {
    let org = &s.org_id;
    let email = who.email.clone();
    let linked = accounts.linked_account(org, &who.subject)?;
    let account_id = match linked {
        Some(id) => id,
        None => {
            let Some(email) = email.clone() else {
                return Ok(Err("the provider sent no email address for you".to_owned()));
            };
            if !who.email_verified {
                return Ok(Err("the provider says your email address is not verified".to_owned()));
            }
            let in_domain = s.domains.iter().any(|d| d == email_domain(&email));
            match accounts.account_id_for(&email)? {
                Some(id) => {
                    let member = accounts.role_in(&id, org).is_some();
                    let managed =
                        accounts.account(&id)?.managed_by.as_deref() == Some(org.as_str());
                    if !(member || managed) || !in_domain {
                        return Ok(Err(
                            "an account with your address exists and is not a member of this \
                             organisation; ask an admin to invite it, accept, and log in with \
                             single sign-on again"
                                .to_owned(),
                        ));
                    }
                    id
                }
                None if in_domain => {
                    match accounts.create_member_account(&email, None, Some(org))? {
                        Some(id) => id,
                        None => {
                            return Ok(Err(
                                "an account took your address a moment ago; try again".to_owned()
                            ))
                        }
                    }
                }
                None => {
                    return Ok(Err(format!(
                        "this organisation does not claim the domain {:?}, so single sign-on \
                         cannot create an account for it",
                        email_domain(&email)
                    )))
                }
            }
        }
    };
    if accounts.account(&account_id)?.managed_by.is_none()
        && accounts.role_in(&account_id, org).is_none()
    {
        // Linked earlier, removed since, and not created by this
        // organisation: removal stands until an admin invites them again.
        return Ok(Err("you are no longer a member of this organisation".to_owned()));
    }
    accounts.link_identity(org, &who.subject, &account_id)?;
    let current = accounts.role_in(&account_id, org);
    let owner_by_config =
        email.as_ref().is_some_and(|e| config_owners.iter().any(|o| o.eq_ignore_ascii_case(e)));
    let mapped = accounts.role_for_groups(org, &who.groups)?;
    // An owner stays an owner: groups from outside never remove the role.
    let wanted = if owner_by_config || current == Some(Role::Owner) {
        Role::Owner
    } else if s.groups_claim.is_some() && !who.groups.is_empty() {
        mapped.unwrap_or(s.default_role)
    } else {
        current.unwrap_or(s.default_role)
    };
    if current != Some(wanted) {
        accounts.upsert_member(org, &account_id, wanted, "sso")?;
        accounts.prune_keys(org, &account_id)?;
    }
    Ok(Ok(account_id))
}

/// `POST /v1/api/sso/callback`
pub async fn sso_callback(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Json(req): Json<SsoCallbackRequest>,
) -> ApiResult<Json<AccountSession>> {
    let accounts = store(&state)?;
    state
        .account_guards
        .sso
        .hit(&accounts::client(&state, &headers, &peer))
        .map_err(|s| accounts::limited(s, "too many single sign-on attempts from this address"))?;
    let a = Arc::clone(&accounts);
    let st = req.state.clone();
    let taken = blocking(move || a.take_token(TokenPurpose::SsoState, &st)).await?;
    let data: SsoStateData = taken
        .and_then(|(_, d)| d)
        .and_then(|d| serde_json::from_str(&d).ok())
        .ok_or_else(|| ApiError::BadParam {
            field: "state".to_owned(),
            message: "this single sign-on attempt is unknown or expired; start again".to_owned(),
        })?;
    if !constant_eq(&hex::encode(Sha256::digest(req.binding.as_bytes())), &data.binding) {
        return Err(ApiError::BadParam {
            field: "binding".to_owned(),
            message: "this single sign-on attempt was started in another browser; start again here"
                .to_owned(),
        });
    }
    let a = Arc::clone(&accounts);
    let org = data.org_id.clone();
    let settings = blocking(move || a.sso_settings(&org))
        .await?
        .ok_or_else(|| sso_failed("this organisation no longer logs in with single sign-on"))?;
    let who = identify(&state, &settings, &data, req.code.trim()).await?;
    let owners: Vec<String> = state
        .config
        .account_settings
        .orgs
        .iter()
        .find(|o| format!("org_{}", o.slug) == data.org_id)
        .map(|o| o.owners.clone())
        .unwrap_or_default();
    let a = Arc::clone(&accounts);
    let (s2, w2) = (settings.clone(), who.clone());
    let outcome = blocking(move || resolve(&a, &s2, &w2, &owners)).await?;
    let subject = who.email.clone().unwrap_or_else(|| who.subject.clone());
    let decision = outcome.map_err(ApiError::Forbidden);
    let account_id =
        audited_as(&state, &headers, peer.0, decision, "sso_login", &data.org_id, None, &subject)
            .await?;
    let a = Arc::clone(&accounts);
    let id = account_id.clone();
    let record = blocking(move || a.account(&id)).await?;
    let disabled = accounts.role_in(&account_id, &data.org_id).is_none();
    if disabled {
        return Err(ApiError::Forbidden("this account is deactivated".to_owned()));
    }
    let _ = record;
    let meta = accounts::session_meta(&state, &headers, &peer, "sso", Some(data.org_id.clone()));
    let session = blocking(move || accounts.issue_session(&account_id, &meta)).await?;
    Ok(Json(session))
}

/// The single sign-on routes. Merged into the query API's router by `lib.rs`.
pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/v1/api/orgs/{org_id}/sso", get(get_sso))
        .route("/v1/api/orgs/{org_id}/sso", put(put_sso))
        .route("/v1/api/orgs/{org_id}/sso", delete(delete_sso))
        .route("/v1/api/orgs/{org_id}/scim-token", get(get_scim_token))
        .route("/v1/api/orgs/{org_id}/scim-token", post(create_scim_token))
        .route("/v1/api/orgs/{org_id}/scim-token", delete(revoke_scim_token))
        .route("/v1/api/sso/start", post(sso_start))
        .route("/v1/api/sso/callback", post(sso_callback))
}
