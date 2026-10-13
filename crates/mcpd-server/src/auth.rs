//! API-key authentication.
//!
//! Six scopes, because they carry genuinely different risk:
//!
//! * `ingest` writes signals and reads nothing.
//! * `read` reads stored signals.
//! * `write` changes stored data: scores, feedback, datasets, experiments and
//!   acknowledgements. Separate from `ingest` because several of those handlers
//!   read a payload on the way through. Capturing a run into a dataset reads
//!   that run's input and output, and the ingest key lives in every container
//!   that emits a span, including the ones running third-party code.
//! * `admin` destroys data and reads the audit log.
//! * `execute` starts, cancels and signals runs, and publishes events to them.
//!   Separate from `write` because starting a run makes somebody's worker do
//!   work and spend money, which annotating a trace does not.
//! * `worker` claims and finishes steps. Separate from everything else because
//!   a worker key lives in the process that runs the step bodies, and that
//!   process should not be able to read every stored prompt in the project.
//!
//! An admin key is accepted for `execute` and `worker` as well, so an operator
//! can drive a run by hand without minting a key for it.
//!
//! Which scope an endpoint takes is not decided here. It is declared in the
//! `auth` field of `spec/mcpd.spec.json`, the handler is tagged to match, and
//! `tools/check-consistency.mjs` compares the two on every run. That check
//! exists because they drifted: the reference documented `admin` on
//! `DELETE /v1/api/traces/{trace_id}` while the handler enforced `read`, and
//! nothing in the build compared a documented scope against an enforced one.
//!
//! ## Upgrading
//!
//! An unset write list falls back to the ingest list, and an unset admin list
//! falls back to the read list. A deployment configured before these two scopes
//! existed therefore keeps exactly the reach it had, and no running fleet breaks
//! on upgrade. Set `--write-key` and `--admin-key` to take the narrower grants.
//!
//! With no keys configured at all, every scope is open. That is deliberate,
//! because the first-run experience must not require a credential, and the
//! server logs a warning at startup when it is listening on anything but
//! loopback.
//!
//! ## Tenancy
//!
//! A key can also be scoped to one or more projects. That is what makes a
//! project a tenancy boundary rather than a label: a team's read key opens their
//! own prompts and nobody else's, so one mcpd can serve several teams without
//! each of them being able to read the others' payloads.
//!
//! A key configured at the top level grants every project. That is the
//! single-tenant path and it stays the simple one: an operator who never opens
//! the `projects` block never meets any of this.
//!
//! ## Accounts
//!
//! With `--signup`, a presented secret that matches no configured key is looked
//! up among the accounts' keys and sessions (see `crate::accounts`). Configured
//! keys are tried first and behave exactly as above. An account key grants its
//! one scope, a session every scope, and both only on the account's own
//! project. And the open rule above is off: with `--signup` a request with no
//! key is refused whatever the configuration holds, because a daemon that lets
//! strangers in must not also let in strangers with no credential at all.

use crate::error::ApiError;
use crate::state::AppState;
use axum::http::HeaderMap;
use sha2::{Digest, Sha256};

/// What a key is allowed to do.
///
/// The wire names match the `auth` values in `spec/mcpd.spec.json`, which is
/// what lets the consistency check compare a documented scope against an
/// enforced one by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// May write signals, and read nothing.
    Ingest,
    /// May read stored signals.
    Read,
    /// May change stored data: scores, feedback, datasets, experiments and
    /// acknowledgements.
    Write,
    /// May destroy data and read the audit log.
    Admin,
    /// May start, cancel and signal runs, and publish events to them.
    Execute,
    /// May claim steps and report their outcome.
    Worker,
}

impl Scope {
    /// Every scope, in order of the reach they grant.
    pub const ALL: &'static [Self] =
        &[Self::Ingest, Self::Read, Self::Write, Self::Admin, Self::Execute, Self::Worker];

    /// The name this scope carries in the spec and in the reference docs.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ingest => "ingest",
            Self::Read => "read",
            Self::Write => "write",
            Self::Admin => "admin",
            Self::Execute => "execute",
            Self::Worker => "worker",
        }
    }

    /// What a refusal should say, phrased as the thing the caller was trying to
    /// do rather than as the name of an internal enum.
    const fn refusal(self) -> &'static str {
        match self {
            Self::Ingest => "that key is not valid for ingest",
            Self::Read => "that key is not valid for reading",
            Self::Write => "that key is not valid for writing; it needs the write scope",
            Self::Admin => "that key is not valid here; this endpoint needs the admin scope",
            Self::Execute => "that key cannot start or steer runs; it needs the execute scope",
            Self::Worker => "that key cannot claim or finish steps; it needs the worker scope",
        }
    }

    /// Whether an admin key also satisfies this scope. Only the two run scopes:
    /// widening `read` or `write` to admin keys would change what every
    /// existing deployment's keys reach.
    pub(crate) const fn admin_implies(self) -> bool {
        match self {
            Self::Execute | Self::Worker => true,
            Self::Ingest | Self::Read | Self::Write | Self::Admin => false,
        }
    }
}

/// Compare two secrets without leaking their contents through timing.
///
/// Hashing first makes the comparison fixed-length, so neither the length nor
/// the position of the first difference is observable.
fn constant_time_eq(a: &str, b: &str) -> bool {
    let ha = Sha256::digest(a.as_bytes());
    let hb = Sha256::digest(b.as_bytes());
    let mut diff = 0u8;
    for (x, y) in ha.iter().zip(hb.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// A short, non-secret identifier for a key, for the audit log.
///
/// The digest is truncated because the audit log answers "which key did this",
/// not "what was the key": a full digest is still a verifier for anyone who
/// guesses a candidate, and there is no reason to store one. Twelve hex
/// characters distinguishes every key any deployment realistically holds. It is
/// only as unguessable as the key itself, so keys should be random rather than
/// chosen.
#[must_use]
pub fn key_digest(key: &str) -> String {
    hex::encode(Sha256::digest(key.as_bytes()))[..12].to_owned()
}

/// The principal to record for a request, without ever recording the key.
///
/// A request with no key on an open daemon is `anonymous`, which is a fact
/// worth having in the log rather than a blank.
#[must_use]
pub fn principal(headers: &HeaderMap) -> String {
    presented_key(headers).map_or_else(|| "anonymous".to_owned(), |k| key_digest(&k))
}

/// The key a request presents, or `None` when it presents none.
///
/// An empty or whitespace-only value is no key. It used to be returned as an
/// empty string, and an empty string matched an empty entry in a configured
/// list, so `x-mcpd-key:` with nothing after it opened any scope whose list
/// held one. `ServerConfig::drop_blank_keys` removes those entries; this is the
/// other end, so neither half alone is what keeps the scope closed.
fn presented_key(headers: &HeaderMap) -> Option<String> {
    let non_empty = |s: &str| {
        let s = s.trim();
        (!s.is_empty()).then(|| s.to_owned())
    };
    if let Some(v) = headers.get("authorization").and_then(|v| v.to_str().ok()) {
        if let Some(rest) = v.strip_prefix("Bearer ").or_else(|| v.strip_prefix("bearer ")) {
            if let Some(key) = non_empty(rest) {
                return Some(key);
            }
        }
    }
    headers.get("x-mcpd-key").and_then(|v| v.to_str().ok()).and_then(non_empty)
}

/// What a presented key is allowed to reach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Grant {
    /// Every project. Either no keys are configured at all, or the key was
    /// configured at the top level rather than under a project.
    All,
    /// Exactly these projects, and no others.
    Only(std::collections::BTreeSet<String>),
}

impl Grant {
    /// Whether this grant reaches `project`.
    #[must_use]
    pub fn covers(&self, project: &str) -> bool {
        match self {
            Self::All => true,
            Self::Only(set) => set.contains(project),
        }
    }

    /// The projects this grant names, or `None` for every project.
    #[must_use]
    pub fn projects(&self) -> Option<&std::collections::BTreeSet<String>> {
        match self {
            Self::All => None,
            Self::Only(set) => Some(set),
        }
    }
}

/// Whether this key appears in any configured list, in any scope, at any level.
///
/// This is the difference between a 401 and a 403. A key nobody has heard of is
/// a credential problem and gets 401; a key that is configured but not for this
/// endpoint is a permission problem and gets 403. Collapsing the two into 401
/// sends an operator hunting for a typo in a key that is perfectly correct.
fn is_known_key(state: &AppState, presented: &str) -> bool {
    is_configured_key(state, presented) || account_credential(state, presented).is_some()
}

/// Check that the request carries a key valid for `scope`, and report what it
/// reaches.
///
/// # Errors
/// Returns [`ApiError::Unauthorized`] when no key is presented or the presented
/// one is in no list at all, and [`ApiError::Forbidden`] when the key is a
/// configured one that this scope does not include.
pub fn authorize(state: &AppState, headers: &HeaderMap, scope: Scope) -> Result<Grant, ApiError> {
    let global = state.config.keys_for(scope);

    // Are there any keys at all, anywhere? With none, the surface is open, which
    // is the laptop path. Never with sign-up on: see the module documentation.
    let any_project_key = state.config.projects.values().any(|p| !p.keys_for(scope).is_empty());
    if global.is_empty() && !any_project_key && !state.config.signup {
        return Ok(Grant::All);
    }

    let Some(presented) = presented_key(headers) else {
        return Err(ApiError::Unauthorized(
            "this endpoint needs an API key; send it as `Authorization: Bearer <key>`".to_owned(),
        ));
    };

    if global.iter().any(|k| constant_time_eq(k, &presented)) {
        return Ok(Grant::All);
    }
    if scope.admin_implies()
        && state.config.keys_for(Scope::Admin).iter().any(|k| constant_time_eq(k, &presented))
    {
        return Ok(Grant::All);
    }

    // Every project this key is listed under. A key may appear under more than
    // one, which is how a platform team gives itself a key that spans the
    // projects it operates without granting the whole server.
    let mut reachable = std::collections::BTreeSet::new();
    for (name, project) in &state.config.projects {
        if project.keys_for(scope).iter().any(|k| constant_time_eq(k, &presented))
            || (scope.admin_implies()
                && project.keys_for(Scope::Admin).iter().any(|k| constant_time_eq(k, &presented)))
        {
            reachable.insert(name.clone());
        }
    }

    // An account's key or session, tried only once no configured key matched,
    // so a configured key means exactly what it meant before accounts existed.
    if reachable.is_empty() {
        if let Some(credential) = account_credential(state, &presented) {
            if credential.allows(scope) {
                return Ok(Grant::Only(std::collections::BTreeSet::from([credential.project])));
            }
            return Err(ApiError::Forbidden(scope.refusal().to_owned()));
        }
    }

    if reachable.is_empty() {
        return Err(if is_known_key(state, &presented) {
            ApiError::Forbidden(scope.refusal().to_owned())
        } else {
            ApiError::Unauthorized("that key is not one this daemon knows".to_owned())
        });
    }
    Ok(Grant::Only(reachable))
}

/// Authorize, and require the grant to reach `project`.
///
/// The error deliberately does not distinguish "this project does not exist"
/// from "your key does not reach it". Telling an unauthorised caller which
/// project names are real is a small leak, and the projects on a shared mcpd are
/// often the names of teams or customers.
///
/// # Errors
/// Returns [`ApiError::Unauthorized`] when the key is missing or unknown, and
/// [`ApiError::Forbidden`] when it is valid but reaches other projects only.
pub fn authorize_project(
    state: &AppState,
    headers: &HeaderMap,
    scope: Scope,
    project: &str,
) -> Result<Grant, ApiError> {
    let grant = authorize(state, headers, scope)?;
    if grant.covers(project) {
        Ok(grant)
    } else {
        Err(ApiError::Forbidden(format!("that key does not reach the project {project:?}")))
    }
}

/// The account credential a secret is, when sign-up is on and it is one.
fn account_credential(state: &AppState, presented: &str) -> Option<crate::accounts::Credential> {
    if !state.config.signup {
        return None;
    }
    state.accounts.as_ref()?.lookup(presented)
}

/// The project every signal this request writes must land in, or `None` when
/// the request is not an account's.
///
/// An account's key reaches one project, so the ingest doors move every signal
/// it sends into that project rather than refusing the ones that name another.
/// Refusing would be as safe, and it would drop the spans of every
/// OpenTelemetry exporter that names no project and so arrives as the default
/// one. Moving them cannot reach another tenant, because the only project they
/// can move into is the key's own. A configured key is never pinned: it keeps
/// the refuse-what-it-does-not-reach rule it has always had.
#[must_use]
pub fn pinned_project(state: &AppState, headers: &HeaderMap) -> Option<String> {
    let presented = presented_key(headers)?;
    // A configured key wins over an account credential in `authorize`, so it
    // wins here too.
    if is_configured_key(state, &presented) {
        return None;
    }
    account_credential(state, &presented).map(|c| c.project)
}

/// Whether a secret is in any configured list.
fn is_configured_key(state: &AppState, presented: &str) -> bool {
    Scope::ALL.iter().any(|&s| {
        state.config.keys_for(s).iter().any(|k| constant_time_eq(k, presented))
            || state
                .config
                .projects
                .values()
                .any(|p| p.keys_for(s).iter().any(|k| constant_time_eq(k, presented)))
    })
}

/// Require an account session token, and return it with what it is.
///
/// Only a session from logging in passes. An API key does not, whatever its
/// scope, so a key that leaks out of a deployment cannot mint more keys or
/// revoke the ones that are watching it.
///
/// # Errors
/// [`ApiError::NotFound`] when sign-up is off, [`ApiError::Unauthorized`] for a
/// missing, unknown or expired token, and [`ApiError::Forbidden`] for an API key.
pub fn authorize_session(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(crate::accounts::Credential, String), ApiError> {
    if !state.config.signup {
        return Err(ApiError::NotFound(
            "this daemon does not offer accounts; start it with --signup to turn them on"
                .to_owned(),
        ));
    }
    let Some(presented) = presented_key(headers) else {
        return Err(ApiError::Unauthorized(
            "this endpoint needs the session token from logging in; send it as \
             `Authorization: Bearer <token>`"
                .to_owned(),
        ));
    };
    match account_credential(state, &presented) {
        Some(c) if matches!(c.kind, crate::accounts::CredentialKind::Session { .. }) => {
            Ok((c, presented))
        }
        Some(_) => Err(ApiError::Forbidden(
            "an API key cannot manage the account; log in and use the session token".to_owned(),
        )),
        None if is_configured_key(state, &presented) => Err(ApiError::Forbidden(
            "a configured key belongs to no account; log in and use the session token".to_owned(),
        )),
        None => Err(ApiError::Unauthorized(
            "that session is not one this daemon knows, or it has expired; log in again".to_owned(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers(v: Option<&str>) -> HeaderMap {
        let mut h = HeaderMap::new();
        if let Some(v) = v {
            h.insert("authorization", HeaderValue::from_str(v).unwrap());
        }
        h
    }

    fn state_with(ingest: Vec<String>, read: Vec<String>) -> AppState {
        state_from(crate::state::ServerConfig {
            ingest_keys: ingest,
            read_keys: read,
            ..Default::default()
        })
    }

    fn state_from(config: crate::state::ServerConfig) -> AppState {
        struct Null;
        #[async_trait::async_trait]
        impl mcpd_store::Store for Null {
            fn name(&self) -> &'static str {
                "null"
            }
            async fn migrate(&self) -> mcpd_store::Result<()> {
                Ok(())
            }
            async fn ping(&self) -> mcpd_store::Result<()> {
                Ok(())
            }
            async fn ingest(
                &self,
                _: mcpd_store::Batch,
            ) -> mcpd_store::Result<mcpd_core::IngestResult> {
                unimplemented!()
            }
            async fn list_traces(
                &self,
                _: &mcpd_store::TraceQuery,
            ) -> mcpd_store::Result<mcpd_store::Paged<mcpd_core::Trace>> {
                unimplemented!()
            }
            async fn get_trace(
                &self,
                _: &str,
                _: &str,
                _: &mcpd_store::Include,
            ) -> mcpd_store::Result<mcpd_core::TraceDetail> {
                unimplemented!()
            }
            async fn delete_trace(&self, _: &str, _: &str) -> mcpd_store::Result<u32> {
                unimplemented!()
            }
            async fn list_spans(
                &self,
                _: &mcpd_store::SpanQuery,
            ) -> mcpd_store::Result<mcpd_store::Paged<mcpd_core::Span>> {
                unimplemented!()
            }
            async fn list_logs(
                &self,
                _: &mcpd_store::LogQuery,
            ) -> mcpd_store::Result<mcpd_store::Paged<mcpd_core::LogRecord>> {
                unimplemented!()
            }
            async fn query_metrics(
                &self,
                _: &mcpd_store::MetricQuery,
            ) -> mcpd_store::Result<mcpd_core::MetricSeriesResponse> {
                unimplemented!()
            }
            async fn list_feedback(
                &self,
                _: &mcpd_store::FeedbackQuery,
            ) -> mcpd_store::Result<mcpd_store::Paged<mcpd_core::Feedback>> {
                unimplemented!()
            }
            async fn list_metric_points(
                &self,
                _: &mcpd_store::MetricPointQuery,
            ) -> mcpd_store::Result<mcpd_store::Paged<mcpd_core::MetricPoint>> {
                unimplemented!()
            }
            async fn list_scores(
                &self,
                _: &mcpd_store::ScoreQuery,
            ) -> mcpd_store::Result<mcpd_store::Paged<mcpd_core::Score>> {
                unimplemented!()
            }
            async fn analytics_summary(
                &self,
                _: &str,
                _: time::OffsetDateTime,
                _: time::OffsetDateTime,
                _: Option<&str>,
            ) -> mcpd_store::Result<mcpd_core::AnalyticsSummary> {
                unimplemented!()
            }
            async fn analytics_cost(
                &self,
                _: &str,
                _: time::OffsetDateTime,
                _: time::OffsetDateTime,
                _: &str,
                _: Option<u32>,
            ) -> mcpd_store::Result<mcpd_core::CostBreakdown> {
                unimplemented!()
            }
            async fn enforce_retention(
                &self,
                _: &str,
                _: time::OffsetDateTime,
            ) -> mcpd_store::Result<u64> {
                unimplemented!()
            }
            async fn list_projects(&self) -> mcpd_store::Result<Vec<String>> {
                unimplemented!()
            }
            async fn put_dataset(&self, _: &mcpd_core::Dataset) -> mcpd_store::Result<()> {
                unimplemented!()
            }
            async fn list_datasets(&self, _: &str) -> mcpd_store::Result<Vec<mcpd_core::Dataset>> {
                unimplemented!()
            }
            async fn dataset_project(&self, _: &str) -> mcpd_store::Result<Option<String>> {
                unimplemented!()
            }
            async fn erase_subject(
                &self,
                _: &str,
                _: &str,
            ) -> mcpd_store::Result<mcpd_store::ErasureOutcome> {
                unimplemented!()
            }
            async fn add_dataset_items(
                &self,
                _: &str,
                _: &[mcpd_core::DatasetItem],
            ) -> mcpd_store::Result<u32> {
                unimplemented!()
            }
            async fn list_dataset_items(
                &self,
                _: &str,
                _: u32,
            ) -> mcpd_store::Result<Vec<mcpd_core::DatasetItem>> {
                unimplemented!()
            }
            async fn put_experiment(&self, _: &mcpd_core::Experiment) -> mcpd_store::Result<()> {
                unimplemented!()
            }
            async fn list_experiments(
                &self,
                _: &str,
            ) -> mcpd_store::Result<Vec<mcpd_core::Experiment>> {
                unimplemented!()
            }
            async fn record_audit(&self, _: &mcpd_core::AuditEvent) -> mcpd_store::Result<()> {
                Ok(())
            }
            async fn list_audit(
                &self,
                _: &mcpd_store::AuditQuery,
            ) -> mcpd_store::Result<Vec<mcpd_core::AuditEvent>> {
                Ok(Vec::new())
            }
        }
        AppState::new(std::sync::Arc::new(Null), config)
    }

    #[test]
    fn no_keys_configured_means_open() {
        let s = state_with(vec![], vec![]);
        assert!(authorize(&s, &headers(None), Scope::Ingest).is_ok());
        assert!(authorize(&s, &headers(None), Scope::Read).is_ok());
    }

    #[test]
    fn a_valid_key_is_accepted() {
        let s = state_with(vec!["secret".into()], vec![]);
        assert!(authorize(&s, &headers(Some("Bearer secret")), Scope::Ingest).is_ok());
    }

    #[test]
    fn a_missing_or_wrong_key_is_refused() {
        let s = state_with(vec!["secret".into()], vec![]);
        assert!(authorize(&s, &headers(None), Scope::Ingest).is_err());
        assert!(authorize(&s, &headers(Some("Bearer nope")), Scope::Ingest).is_err());
    }

    #[test]
    fn scopes_do_not_leak_into_each_other() {
        // An ingest key must not be able to read back everyone's prompts.
        let s = state_with(vec!["write-key".into()], vec!["read-key".into()]);
        assert!(authorize(&s, &headers(Some("Bearer write-key")), Scope::Read).is_err());
        assert!(authorize(&s, &headers(Some("Bearer read-key")), Scope::Ingest).is_err());
    }

    #[test]
    fn a_configured_key_used_in_the_wrong_scope_is_forbidden_not_unauthorised() {
        // The distinction is the whole reason `Forbidden` exists: 401 sends an
        // operator hunting for a typo in a key that is perfectly correct.
        let s = state_with(vec!["write-key".into()], vec!["read-key".into()]);
        assert!(matches!(
            authorize(&s, &headers(Some("Bearer read-key")), Scope::Ingest),
            Err(ApiError::Forbidden(_))
        ));
        assert!(matches!(
            authorize(&s, &headers(Some("Bearer nobody-has-this")), Scope::Ingest),
            Err(ApiError::Unauthorized(_))
        ));
        assert!(matches!(
            authorize(&s, &headers(None), Scope::Ingest),
            Err(ApiError::Unauthorized(_))
        ));
    }

    #[test]
    fn an_unset_write_list_falls_back_to_the_ingest_keys() {
        // The upgrade guarantee. A deployment configured before the write scope
        // existed must keep working with exactly the reach it had.
        let s = state_with(vec!["fleet".into()], vec!["reader".into()]);
        assert!(authorize(&s, &headers(Some("Bearer fleet")), Scope::Write).is_ok());
        assert!(authorize(&s, &headers(Some("Bearer reader")), Scope::Write).is_err());
    }

    #[test]
    fn an_unset_admin_list_falls_back_to_the_read_keys() {
        let s = state_with(vec!["fleet".into()], vec!["reader".into()]);
        assert!(authorize(&s, &headers(Some("Bearer reader")), Scope::Admin).is_ok());
        assert!(authorize(&s, &headers(Some("Bearer fleet")), Scope::Admin).is_err());
    }

    #[test]
    fn a_set_admin_list_takes_deletion_away_from_the_read_keys() {
        let mut s = state_with(vec![], vec!["reader".into()]);
        // `state_with` builds the common two-scope case; this test is about the
        // third list, so it is set here rather than widening that helper.
        let config = crate::state::ServerConfig {
            read_keys: vec!["reader".to_owned()],
            admin_keys: vec!["root".to_owned()],
            ..Default::default()
        };
        s = AppState::new(std::sync::Arc::clone(&s.store), config);
        assert!(authorize(&s, &headers(Some("Bearer reader")), Scope::Read).is_ok());
        assert!(matches!(
            authorize(&s, &headers(Some("Bearer reader")), Scope::Admin),
            Err(ApiError::Forbidden(_))
        ));
        assert!(authorize(&s, &headers(Some("Bearer root")), Scope::Admin).is_ok());
    }

    #[test]
    fn the_recorded_principal_is_a_digest_and_never_the_key() {
        let h = headers(Some("Bearer super-secret"));
        let p = principal(&h);
        assert_eq!(p.len(), 12, "the digest prefix should be short: {p}");
        assert!(!p.contains("super-secret"), "the key leaked into the audit principal: {p}");
        assert_ne!(p, key_digest("another-key"), "two keys must not share a principal");
        assert_eq!(principal(&headers(None)), "anonymous");
    }

    #[test]
    fn every_scope_name_matches_the_spec_vocabulary() {
        // `tools/check-consistency.mjs` compares these names against the `auth`
        // field of each endpoint in the spec. If a name here changes without the
        // spec changing, that check is comparing two different vocabularies and
        // silently passing.
        let names: Vec<&str> = Scope::ALL.iter().map(|s| s.as_str()).collect();
        assert_eq!(names, vec!["ingest", "read", "write", "admin", "execute", "worker"]);
    }

    #[test]
    fn the_x_mcpd_key_header_also_works() {
        let s = state_with(vec!["secret".into()], vec![]);
        let mut h = HeaderMap::new();
        h.insert("x-mcpd-key", HeaderValue::from_static("secret"));
        assert!(authorize(&s, &h, Scope::Ingest).is_ok());
    }

    /// The three ways a request presents no key while sending a key header.
    fn empty_presentations() -> Vec<HeaderMap> {
        let mut out = vec![headers(Some("Bearer ")), headers(Some("Bearer    "))];
        let mut h = HeaderMap::new();
        h.insert("x-mcpd-key", HeaderValue::from_static(""));
        out.push(h);
        out
    }

    /// One list of the top-level configuration, by scope.
    fn list_of(config: &mut crate::state::ServerConfig, scope: Scope) -> &mut Vec<String> {
        match scope {
            Scope::Ingest => &mut config.ingest_keys,
            Scope::Read => &mut config.read_keys,
            Scope::Write => &mut config.write_keys,
            Scope::Admin => &mut config.admin_keys,
            Scope::Execute => &mut config.execute_keys,
            Scope::Worker => &mut config.worker_keys,
        }
    }

    #[test]
    fn an_empty_key_header_reaches_no_scope_whose_list_held_a_blank_entry() {
        // `MCPD_EXECUTE_KEYS=a-real-key,` is a trailing comma, and clap reads
        // it as the key and an empty entry. An empty key header used to match
        // that entry, so every scope was reachable with no key at all.
        for &scope in Scope::ALL {
            let mut config = crate::state::ServerConfig::default();
            *list_of(&mut config, scope) =
                vec!["a-real-key-for-this-scope".to_owned(), String::new(), "   ".to_owned()];
            let s = state_from(config);
            for h in empty_presentations() {
                assert!(
                    matches!(authorize(&s, &h, scope), Err(ApiError::Unauthorized(_))),
                    "an empty key header reached the {} scope",
                    scope.as_str()
                );
            }
            assert!(
                authorize(&s, &headers(Some("Bearer a-real-key-for-this-scope")), scope).is_ok(),
                "the real key beside the blank entry stopped working for {}",
                scope.as_str()
            );
        }
    }

    #[test]
    fn an_empty_presented_key_is_no_key_even_against_a_blank_configured_entry() {
        // The request half on its own. `AppState::new` drops blank entries, so
        // one is put back after construction, which is the state an embedder
        // that edits the configuration in place could reach.
        let mut s = state_with(vec!["fleet-key-long-enough".to_owned()], vec![]);
        s.config.execute_keys = vec![String::new()];
        for h in empty_presentations() {
            assert!(
                matches!(authorize(&s, &h, Scope::Execute), Err(ApiError::Unauthorized(_))),
                "an empty header matched an empty configured key"
            );
        }
        assert_eq!(principal(&headers(Some("Bearer "))), "anonymous");
    }

    #[test]
    fn a_list_holding_only_blank_entries_falls_back_like_an_unset_one() {
        // The compose file passes `MCPD_EXECUTE_KEYS: ${MCPD_EXECUTE_KEYS:-}`,
        // so an operator who sets only the ingest and read keys starts with
        // `[""]` here. Before, that list did not fall back, so the execute
        // scope accepted an empty header and refused the ingest key.
        let s = state_from(crate::state::ServerConfig {
            ingest_keys: vec!["fleet-key-long-enough".to_owned()],
            execute_keys: vec![String::new()],
            worker_keys: vec!["  ".to_owned()],
            ..Default::default()
        });
        for scope in [Scope::Execute, Scope::Worker] {
            assert!(
                authorize(&s, &headers(Some("Bearer fleet-key-long-enough")), scope).is_ok(),
                "the {} list did not fall back to the ingest keys",
                scope.as_str()
            );
        }
    }

    #[test]
    fn a_project_list_holding_a_blank_entry_is_cleaned_and_falls_back_too() {
        let project = crate::state::ProjectConfig {
            ingest_keys: vec!["checkout-ingest-key".to_owned()],
            execute_keys: vec![String::new()],
            worker_keys: vec!["checkout-worker-key".to_owned(), " ".to_owned()],
            ..Default::default()
        };
        let s = state_from(crate::state::ServerConfig {
            read_keys: vec!["platform-read-key".to_owned()],
            projects: std::collections::BTreeMap::from([("checkout".to_owned(), project)]),
            ..Default::default()
        });
        for scope in [Scope::Execute, Scope::Worker] {
            for h in empty_presentations() {
                assert!(
                    matches!(authorize(&s, &h, scope), Err(ApiError::Unauthorized(_))),
                    "an empty key header reached checkout's {} scope",
                    scope.as_str()
                );
            }
        }
        let only_checkout = Grant::Only(std::collections::BTreeSet::from(["checkout".to_owned()]));
        assert_eq!(
            authorize(&s, &headers(Some("Bearer checkout-ingest-key")), Scope::Execute).ok(),
            Some(only_checkout.clone()),
            "a blank execute list under the project did not fall back to its ingest keys"
        );
        assert_eq!(
            authorize(&s, &headers(Some("Bearer checkout-worker-key")), Scope::Worker).ok(),
            Some(only_checkout)
        );
    }

    #[test]
    fn comparison_does_not_depend_on_a_shared_prefix() {
        assert!(constant_time_eq("abc", "abc"));
        assert!(!constant_time_eq("abc", "abd"));
        assert!(!constant_time_eq("abc", "abcdef"));
    }
}
