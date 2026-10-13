//! Organisations, roles, the account lifecycle, single sign-on and SCIM,
//! driven through the real router.
//!
//! The groups that matter most are the role matrix, which asks the daemon for
//! a key of every scope as every role and compares the answer with the table
//! in `accounts::roles`, and the isolation group, which tries every
//! organisation door with one organisation's credentials against another's.

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use axum::Router;
use base64::Engine;
use http_body_util::BodyExt;
use mcpd_runs::{RunStore, SqliteRunStore};
use mcpd_server::accounts::{AccountStore, ConfigOrg, ConfigSso};
use mcpd_server::mail::{MailConfig, Outbox};
use mcpd_server::{router, AppState, ServerConfig};
use mcpd_store::sqlite::SqliteStore;
use mcpd_store::Store;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

const PASSWORD: &str = "a long enough password";

fn temp_dir() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    // The pid and a per-process stamp: a recycled pid alone would reopen a
    // previous run's files. See AGENTS.md.
    static STAMP: std::sync::OnceLock<u128> = std::sync::OnceLock::new();
    let stamp = STAMP.get_or_init(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    });
    let dir = std::env::temp_dir().join(format!(
        "mcpd-orgs-http-{}-{stamp}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).expect("create the test directory");
    dir
}

/// One daemon, its mail, and the state behind it.
struct Daemon {
    app: Router,
    outbox: Outbox,
    state: Arc<AppState>,
}

fn config(signup: bool) -> (ServerConfig, Outbox) {
    let outbox = Outbox::default();
    let mut c = ServerConfig { signup, ..ServerConfig::default() };
    c.account_settings.enabled = true;
    c.account_settings.mail = MailConfig::Memory(outbox.clone());
    c.account_settings.allow_private_issuers = true;
    (c, outbox)
}

async fn daemon_with(c: ServerConfig, outbox: Outbox) -> Daemon {
    let dir = temp_dir();
    let store = SqliteStore::open(dir.join("signals.db"), 2).expect("open the store");
    store.migrate().await.expect("migrate the store");
    let runs = SqliteRunStore::open(dir.join("runs.db"), c.run_config).expect("open runs");
    runs.migrate().await.expect("migrate runs");
    let accounts =
        AccountStore::open(dir.join("accounts.db").to_str().expect("a utf-8 path")).expect("open");
    let state = Arc::new(
        AppState::new(Arc::new(store) as Arc<dyn Store>, c)
            .with_run_store(Arc::new(runs) as Arc<dyn RunStore>)
            .with_account_store(Arc::new(accounts)),
    );
    mcpd_server::orgs::apply_config(&state).await.expect("apply the config organisations");
    Daemon { app: router(Arc::clone(&state)), outbox, state }
}

async fn daemon() -> Daemon {
    let (c, o) = config(true);
    daemon_with(c, o).await
}

struct Reply {
    status: StatusCode,
    body: Value,
}

async fn send(
    app: &Router,
    method: &str,
    path: &str,
    key: Option<&str>,
    body: Option<Value>,
) -> Reply {
    let mut req = Request::builder().method(method).uri(path);
    if let Some(k) = key {
        req = req.header("authorization", format!("Bearer {k}"));
    }
    let mut req = match body {
        Some(b) => req.header("content-type", "application/json").body(Body::from(b.to_string())),
        None => req.body(Body::empty()),
    }
    .expect("build the request");
    // A fresh address per request, so the per-address limits, which have
    // tests of their own, never decide one of these.
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(1);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let addr: SocketAddr =
        format!("10.{}.{}.{}:1000", (n >> 16) & 255, (n >> 8) & 255, n & 255).parse().unwrap();
    req.extensions_mut().insert(ConnectInfo(addr));
    let res = app.clone().oneshot(req).await.expect("route the request");
    let status = res.status();
    let bytes = res.into_body().collect().await.expect("read the body").to_bytes();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| json!({ "raw": String::from_utf8_lossy(&bytes) }))
    };
    Reply { status, body }
}

async fn get(d: &Daemon, path: &str, key: Option<&str>) -> Reply {
    send(&d.app, "GET", path, key, None).await
}
async fn post(d: &Daemon, path: &str, key: Option<&str>, body: Value) -> Reply {
    send(&d.app, "POST", path, key, Some(body)).await
}

fn mailed_token(d: &Daemon, email: &str, kind: &str) -> String {
    let mail = d.outbox.last_to(email).unwrap_or_else(|| panic!("no mail to {email}"));
    let at = mail
        .body
        .find(&format!("#{kind}="))
        .unwrap_or_else(|| panic!("no {kind} link in {}", mail.body));
    mail.body[at + kind.len() + 2..].split_whitespace().next().expect("a token").to_owned()
}

/// A confirmed account: its session token and its own project.
struct Who {
    token: String,
    project: String,
    _email: String,
}

async fn signup(d: &Daemon, email: &str) -> Who {
    let r =
        post(d, "/v1/api/account/signup", None, json!({ "email": email, "password": PASSWORD }))
            .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let token = mailed_token(d, email, "verify");
    let r = post(d, "/v1/api/account/verify", None, json!({ "token": token })).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    Who {
        token: r.body["token"].as_str().unwrap().to_owned(),
        project: r.body["project"].as_str().unwrap().to_owned(),
        _email: email.to_owned(),
    }
}

/// An organisation `owner` created, and its first project.
async fn org_of(d: &Daemon, owner: &Who, name: &str) -> (String, String) {
    let r = post(d, "/v1/api/orgs", Some(&owner.token), json!({ "name": name })).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let id = r.body["id"].as_str().unwrap().to_owned();
    assert_eq!(r.body["role"], json!("owner"));
    let p = get(d, &format!("/v1/api/orgs/{id}/projects"), Some(&owner.token)).await;
    (id, p.body[0]["project"].as_str().unwrap().to_owned())
}

/// Invite `email` with `role` and accept as a new account; its session.
async fn join(d: &Daemon, admin: &Who, org: &str, email: &str, role: &str) -> Who {
    let r = post(
        d,
        &format!("/v1/api/orgs/{org}/invites"),
        Some(&admin.token),
        json!({ "email": email, "role": role }),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let token = mailed_token(d, email, "invite");
    let look = post(d, "/v1/api/invites/lookup", None, json!({ "token": token })).await;
    assert_eq!(look.body["account_exists"], json!(false), "{}", look.body);
    let r =
        post(d, "/v1/api/invites/accept", None, json!({ "token": token, "password": PASSWORD }))
            .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["org"]["role"], json!(role));
    Who {
        token: r.body["session"]["token"].as_str().unwrap().to_owned(),
        project: r.body["session"]["project"].as_str().unwrap_or_default().to_owned(),
        _email: email.to_owned(),
    }
}

fn span(trace_id: &str, project: &str) -> Value {
    json!({
        "trace_id": trace_id, "span_id": "0123456789abcdef", "name": "checkout",
        "kind": "agent_run", "status": "ok",
        "start_time": "2026-01-01T00:00:00Z", "end_time": "2026-01-01T00:00:01Z",
        "is_root": true, "is_event": false,
        "correlation": { "project": project },
        "received_at": "2026-01-01T00:00:01Z"
    })
}

const SCOPES: [&str; 6] = ["ingest", "read", "write", "admin", "execute", "worker"];

// ------------------------------------------------------------ roles ---

#[tokio::test]
async fn every_role_gets_exactly_the_keys_its_scopes_allow() {
    let d = daemon().await;
    let owner = signup(&d, "owner@example.com").await;
    let (org, project) = org_of(&d, &owner, "Acme").await;
    let table: [(&str, &[&str]); 4] = [
        ("viewer", &["read"]),
        ("member", &["ingest", "read", "write", "execute", "worker"]),
        ("admin", &SCOPES),
        ("owner", &SCOPES),
    ];
    for (role, allowed) in table {
        let who = if role == "owner" {
            owner.token.clone()
        } else {
            join(&d, &owner, &org, &format!("{role}@example.com"), role).await.token
        };
        for scope in SCOPES {
            let r = post(
                &d,
                "/v1/api/account/keys",
                Some(&who),
                json!({ "scope": scope, "project": project }),
            )
            .await;
            let want =
                if allowed.contains(&scope) { StatusCode::OK } else { StatusCode::FORBIDDEN };
            assert_eq!(r.status, want, "{role} asking for a {scope} key: {}", r.body);
        }
        // And the session itself: every role reads, only admin and owner read
        // the audit log.
        let read = get(&d, &format!("/v1/api/traces?project={project}"), Some(&who)).await;
        assert_eq!(read.status, StatusCode::OK, "{role}: {}", read.body);
        let audit = get(&d, &format!("/v1/api/audit?project={project}"), Some(&who)).await;
        let want = if allowed.contains(&"admin") { StatusCode::OK } else { StatusCode::FORBIDDEN };
        assert_eq!(audit.status, want, "{role} reading the audit log");
    }
    // The audit log of the organisation names what was done and by whom.
    let log = get(&d, &format!("/v1/api/orgs/{org}/audit"), Some(&owner.token)).await;
    let rows = log.body.as_array().unwrap();
    assert!(
        rows.iter().any(|r| r["action"] == "member_invite" && r["subject"] == "owner@example.com"),
        "{}",
        log.body
    );
    assert!(rows.iter().any(|r| r["action"] == "invite_accept"), "{}", log.body);
}

#[tokio::test]
async fn demoting_a_member_disarms_their_keys_at_once() {
    let d = daemon().await;
    let owner = signup(&d, "owner@example.com").await;
    let (org, project) = org_of(&d, &owner, "Acme").await;
    let m = join(&d, &owner, &org, "m@example.com", "member").await;
    let key = post(
        &d,
        "/v1/api/account/keys",
        Some(&m.token),
        json!({ "scope": "ingest", "project": project }),
    )
    .await
    .body["secret"]
        .as_str()
        .unwrap()
        .to_owned();
    let batch = json!({ "spans": [span("11111111111111111111111111111111", &project)] });
    assert_eq!(post(&d, "/v1/ingest", Some(&key), batch.clone()).await.status, StatusCode::OK);
    let account = get(&d, &format!("/v1/api/orgs/{org}/members"), Some(&owner.token))
        .await
        .body
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["email"] == "m@example.com")
        .unwrap()["account_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let r = send(
        &d.app,
        "PATCH",
        &format!("/v1/api/orgs/{org}/members/{account}"),
        Some(&owner.token),
        Some(json!({ "role": "viewer" })),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let after = post(&d, "/v1/ingest", Some(&key), batch).await;
    assert!(
        after.status == StatusCode::FORBIDDEN || after.status == StatusCode::UNAUTHORIZED,
        "{}",
        after.body
    );
    // A project role of none takes the project away from a member.
    let r = send(
        &d.app,
        "PATCH",
        &format!("/v1/api/orgs/{org}/members/{account}"),
        Some(&owner.token),
        Some(json!({ "role": "member" })),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    let r = post(
        &d,
        &format!("/v1/api/orgs/{org}/members/{account}/project-roles"),
        Some(&owner.token),
        json!({ "project": project, "role": "none" }),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(
        get(&d, &format!("/v1/api/traces?project={project}"), Some(&m.token)).await.status,
        StatusCode::FORBIDDEN
    );
    // Removing ends the rest.
    let r = send(
        &d.app,
        "DELETE",
        &format!("/v1/api/orgs/{org}/members/{account}"),
        Some(&owner.token),
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(
        get(&d, &format!("/v1/api/orgs/{org}"), Some(&m.token)).await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn the_last_owner_stays_and_an_admin_cannot_touch_owners() {
    let d = daemon().await;
    let owner = signup(&d, "owner@example.com").await;
    let (org, _) = org_of(&d, &owner, "Acme").await;
    let admin = join(&d, &owner, &org, "admin@example.com", "admin").await;
    let members = get(&d, &format!("/v1/api/orgs/{org}/members"), Some(&owner.token)).await.body;
    let owner_id = members.as_array().unwrap().iter().find(|m| m["role"] == "owner").unwrap()
        ["account_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let r = send(
        &d.app,
        "DELETE",
        &format!("/v1/api/orgs/{org}/members/{owner_id}"),
        Some(&owner.token),
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{}", r.body);
    let r = send(
        &d.app,
        "PATCH",
        &format!("/v1/api/orgs/{org}/members/{owner_id}"),
        Some(&admin.token),
        Some(json!({ "role": "member" })),
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{}", r.body);
    let r = post(
        &d,
        &format!("/v1/api/orgs/{org}/invites"),
        Some(&admin.token),
        json!({ "email": "x@example.com", "role": "owner" }),
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{}", r.body);
    let r = send(
        &d.app,
        "PATCH",
        &format!("/v1/api/orgs/{org}"),
        Some(&admin.token),
        Some(json!({ "quota_per_minute": 10 })),
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "an admin changed the quota: {}", r.body);
    let r = send(&d.app, "DELETE", &format!("/v1/api/orgs/{org}"), Some(&admin.token), None).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{}", r.body);
}

// -------------------------------------------------------- isolation ---

#[tokio::test]
async fn one_organisation_reaches_nothing_of_another() {
    let d = daemon().await;
    let a = signup(&d, "a@example.com").await;
    let b = signup(&d, "b@example.com").await;
    let (org_a, project_a) = org_of(&d, &a, "A").await;
    let a_key = post(
        &d,
        "/v1/api/account/keys",
        Some(&a.token),
        json!({ "scope": "ingest", "project": project_a }),
    )
    .await
    .body["secret"]
        .as_str()
        .unwrap()
        .to_owned();
    post(
        &d,
        "/v1/ingest",
        Some(&a_key),
        json!({ "spans": [span("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", &project_a)] }),
    )
    .await;
    for (method, path) in [
        ("GET", format!("/v1/api/orgs/{org_a}")),
        ("PATCH", format!("/v1/api/orgs/{org_a}")),
        ("DELETE", format!("/v1/api/orgs/{org_a}")),
        ("GET", format!("/v1/api/orgs/{org_a}/members")),
        ("GET", format!("/v1/api/orgs/{org_a}/invites")),
        ("POST", format!("/v1/api/orgs/{org_a}/invites")),
        ("GET", format!("/v1/api/orgs/{org_a}/projects")),
        ("POST", format!("/v1/api/orgs/{org_a}/projects")),
        ("DELETE", format!("/v1/api/orgs/{org_a}/projects/{project_a}")),
        ("GET", format!("/v1/api/orgs/{org_a}/keys")),
        ("GET", format!("/v1/api/orgs/{org_a}/usage")),
        ("GET", format!("/v1/api/orgs/{org_a}/audit")),
        ("GET", format!("/v1/api/orgs/{org_a}/group-roles")),
        ("GET", format!("/v1/api/orgs/{org_a}/sso")),
        ("PUT", format!("/v1/api/orgs/{org_a}/sso")),
        ("GET", format!("/v1/api/orgs/{org_a}/scim-token")),
        ("POST", format!("/v1/api/orgs/{org_a}/scim-token")),
    ] {
        let body = Some(json!({ "email": "x@example.com", "role": "viewer", "name": "x",
            "provider": "oidc", "client_id": "x", "domains": [], "enforced": false, "default_role": "member" }));
        let r = send(&d.app, method, &path, Some(&b.token), body).await;
        assert_eq!(r.status, StatusCode::NOT_FOUND, "{method} {path}: {}", r.body);
    }
    let r = post(
        &d,
        "/v1/api/account/keys",
        Some(&b.token),
        json!({ "scope": "read", "project": project_a }),
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{}", r.body);
    let r = get(&d, &format!("/v1/api/traces?project={project_a}"), Some(&b.token)).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{}", r.body);
    let projects = get(&d, "/v1/api/projects", Some(&b.token)).await;
    assert!(!projects.body.to_string().contains(&project_a), "{}", projects.body);
    // And A's own project is not reachable by B's own organisation's key.
    let b_key = post(&d, "/v1/api/account/keys", Some(&b.token), json!({ "scope": "read" }))
        .await
        .body["secret"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        get(&d, &format!("/v1/api/traces?project={}", a.project), Some(&b_key)).await.status,
        StatusCode::FORBIDDEN
    );
}

// -------------------------------------------------------- lifecycle ---

#[tokio::test]
async fn an_unconfirmed_account_cannot_log_in_and_is_sent_a_new_link() {
    let d = daemon().await;
    post(
        &d,
        "/v1/api/account/signup",
        None,
        json!({ "email": "u@example.com", "password": PASSWORD }),
    )
    .await;
    let first = mailed_token(&d, "u@example.com", "verify");
    let r = post(
        &d,
        "/v1/api/account/login",
        None,
        json!({ "email": "u@example.com", "password": PASSWORD }),
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{}", r.body);
    assert_eq!(r.body["error"], json!("email_unverified"));
    let second = mailed_token(&d, "u@example.com", "verify");
    assert_ne!(first, second);
    // Only the newest link works.
    assert_eq!(
        post(&d, "/v1/api/account/verify", None, json!({ "token": first })).await.status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        post(&d, "/v1/api/account/verify", None, json!({ "token": second })).await.status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn password_reset_answers_alike_ends_sessions_and_works_once() {
    let d = daemon().await;
    let a = signup(&d, "ana@example.com").await;
    let known =
        post(&d, "/v1/api/account/password/forgot", None, json!({ "email": "ana@example.com" }))
            .await;
    let unknown =
        post(&d, "/v1/api/account/password/forgot", None, json!({ "email": "nobody@example.com" }))
            .await;
    assert_eq!(known.status, StatusCode::OK);
    assert_eq!(
        known.body["message"].as_str().unwrap().replace("ana@example.com", "X"),
        unknown.body["message"].as_str().unwrap().replace("nobody@example.com", "X")
    );
    assert!(d.outbox.last_to("nobody@example.com").is_none(), "an unknown address was mailed");
    let token = mailed_token(&d, "ana@example.com", "reset");
    let r = post(
        &d,
        "/v1/api/account/password/reset",
        None,
        json!({ "token": token, "password": "a brand new password" }),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(
        get(&d, "/v1/api/account", Some(&a.token)).await.status,
        StatusCode::UNAUTHORIZED,
        "an old session outlived the reset"
    );
    let again = post(
        &d,
        "/v1/api/account/password/reset",
        None,
        json!({ "token": token, "password": "another new password" }),
    )
    .await;
    assert_eq!(again.status, StatusCode::BAD_REQUEST);
    let r = post(
        &d,
        "/v1/api/account/login",
        None,
        json!({ "email": "ana@example.com", "password": "a brand new password" }),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
}

#[tokio::test]
async fn sessions_are_listed_and_ended_and_a_password_change_ends_the_others() {
    let d = daemon().await;
    let a = signup(&d, "ana@example.com").await;
    let second = post(
        &d,
        "/v1/api/account/login",
        None,
        json!({ "email": "ana@example.com", "password": PASSWORD }),
    )
    .await
    .body["token"]
        .as_str()
        .unwrap()
        .to_owned();
    let list = get(&d, "/v1/api/account/sessions", Some(&a.token)).await;
    let rows = list.body.as_array().unwrap();
    assert_eq!(rows.len(), 2, "{}", list.body);
    assert_eq!(rows.iter().filter(|s| s["current"] == true).count(), 1);
    assert!(!list.body.to_string().contains(&second), "a listing carried a token");
    let other =
        rows.iter().find(|s| s["current"] == false).unwrap()["id"].as_str().unwrap().to_owned();
    let r =
        send(&d.app, "DELETE", &format!("/v1/api/account/sessions/{other}"), Some(&a.token), None)
            .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(get(&d, "/v1/api/account", Some(&second)).await.status, StatusCode::UNAUTHORIZED);
    let third = post(
        &d,
        "/v1/api/account/login",
        None,
        json!({ "email": "ana@example.com", "password": PASSWORD }),
    )
    .await
    .body["token"]
        .as_str()
        .unwrap()
        .to_owned();
    let wrong = post(
        &d,
        "/v1/api/account/password",
        Some(&a.token),
        json!({ "current_password": "nope nope nope", "new_password": "a brand new password" }),
    )
    .await;
    assert_eq!(wrong.status, StatusCode::FORBIDDEN);
    let ok = post(
        &d,
        "/v1/api/account/password",
        Some(&a.token),
        json!({ "current_password": PASSWORD, "new_password": "a brand new password" }),
    )
    .await;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.body);
    assert_eq!(get(&d, "/v1/api/account", Some(&a.token)).await.status, StatusCode::OK);
    assert_eq!(get(&d, "/v1/api/account", Some(&third)).await.status, StatusCode::UNAUTHORIZED);
}

fn totp_now(secret: &str) -> String {
    let raw = mcpd_server::totp::base32_decode(secret).unwrap();
    let now = time::OffsetDateTime::now_utc().unix_timestamp() as u64;
    format!("{:06}", mcpd_server::totp::code_at(&raw, mcpd_server::totp::step_of(now)))
}

#[tokio::test]
async fn two_step_log_in_needs_the_code_and_a_recovery_code_works_once() {
    let d = daemon().await;
    let a = signup(&d, "ana@example.com").await;
    let setup = post(&d, "/v1/api/account/totp/setup", Some(&a.token), json!({})).await;
    assert_eq!(setup.status, StatusCode::OK, "{}", setup.body);
    assert!(setup.body["qr_svg"].as_str().unwrap().contains("<svg"));
    let secret = setup.body["secret"].as_str().unwrap().to_owned();
    let bad =
        post(&d, "/v1/api/account/totp/enable", Some(&a.token), json!({ "code": "000000" })).await;
    assert_eq!(bad.status, StatusCode::FORBIDDEN);
    let on = post(
        &d,
        "/v1/api/account/totp/enable",
        Some(&a.token),
        json!({ "code": totp_now(&secret) }),
    )
    .await;
    assert_eq!(on.status, StatusCode::OK, "{}", on.body);
    let recovery = on.body["recovery_codes"][0].as_str().unwrap().to_owned();
    let login = |code: Option<String>| {
        post(
            &d,
            "/v1/api/account/login",
            None,
            json!({ "email": "ana@example.com", "password": PASSWORD, "totp_code": code }),
        )
    };
    let r = login(None).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    assert_eq!(r.body["error"], json!("totp_required"));
    let r = login(Some("123456".into())).await;
    assert_eq!(r.body["error"], json!("totp_required"));
    // The code that turned it on was used, so it does not log in again.
    assert_eq!(login(Some(recovery.clone())).await.status, StatusCode::OK);
    assert_eq!(
        login(Some(recovery)).await.status,
        StatusCode::UNAUTHORIZED,
        "a recovery code worked twice"
    );
    let me = get(&d, "/v1/api/account", Some(&a.token)).await;
    assert_eq!(me.body["totp_enabled"], json!(true));
}

#[tokio::test]
async fn deleting_an_account_erases_what_it_alone_owned() {
    let d = daemon().await;
    let a = signup(&d, "ana@example.com").await;
    let key = post(&d, "/v1/api/account/keys", Some(&a.token), json!({ "scope": "ingest" }))
        .await
        .body["secret"]
        .as_str()
        .unwrap()
        .to_owned();
    post(
        &d,
        "/v1/ingest",
        Some(&key),
        json!({ "spans": [span("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", &a.project)] }),
    )
    .await;
    // An organisation with another member blocks it until ownership moves.
    let (org, _) = org_of(&d, &a, "Shared").await;
    join(&d, &a, &org, "bo@example.com", "member").await;
    let r =
        post(&d, "/v1/api/account/delete", Some(&a.token), json!({ "password": PASSWORD })).await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{}", r.body);
    let wrong = post(
        &d,
        "/v1/api/account/delete",
        Some(&a.token),
        json!({ "password": "not the password" }),
    )
    .await;
    assert_eq!(wrong.status, StatusCode::FORBIDDEN);
    assert_eq!(
        send(&d.app, "DELETE", &format!("/v1/api/orgs/{org}"), Some(&a.token), None).await.status,
        StatusCode::OK
    );
    let r =
        post(&d, "/v1/api/account/delete", Some(&a.token), json!({ "password": PASSWORD })).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(r.body["projects_erased"].to_string().contains(&a.project));
    assert_eq!(get(&d, "/v1/api/account", Some(&a.token)).await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        post(&d, "/v1/ingest", Some(&key), json!({ "spans": [] })).await.status,
        StatusCode::UNAUTHORIZED
    );
    let left = d
        .state
        .store
        .list_traces(&mcpd_store::TraceQuery { project: a.project.clone(), ..Default::default() })
        .await
        .unwrap();
    assert!(left.items.is_empty(), "the project's traces outlived the account");
}

// -------------------------------------------------------- quotas ---

#[tokio::test]
async fn an_organisation_shares_one_ceiling_across_its_projects() {
    let (mut c, o) = config(true);
    c.account_settings.org_quota_per_minute = Some(3);
    let d = daemon_with(c, o).await;
    let a = signup(&d, "ana@example.com").await;
    let (org, project) = org_of(&d, &a, "Small").await;
    let second = post(
        &d,
        &format!("/v1/api/orgs/{org}/projects"),
        Some(&a.token),
        json!({ "label": "two" }),
    )
    .await
    .body["project"]
        .as_str()
        .unwrap()
        .to_owned();
    let k1 = post(
        &d,
        "/v1/api/account/keys",
        Some(&a.token),
        json!({ "scope": "ingest", "project": project }),
    )
    .await
    .body["secret"]
        .as_str()
        .unwrap()
        .to_owned();
    let k2 = post(
        &d,
        "/v1/api/account/keys",
        Some(&a.token),
        json!({ "scope": "ingest", "project": second }),
    )
    .await
    .body["secret"]
        .as_str()
        .unwrap()
        .to_owned();
    let traces: Vec<String> = (1..=4).map(|i| format!("{i:032x}")).collect();
    let r1 = post(
        &d,
        "/v1/ingest",
        Some(&k1),
        json!({ "spans": [span(&traces[0], &project), span(&traces[1], &project)] }),
    )
    .await;
    assert_eq!(r1.body["accepted"], json!(2), "{}", r1.body);
    let r2 = post(
        &d,
        "/v1/ingest",
        Some(&k2),
        json!({ "spans": [span(&traces[2], &second), span(&traces[3], &second)] }),
    )
    .await;
    assert_eq!(
        r2.body["accepted"],
        json!(1),
        "a second project doubled the allowance: {}",
        r2.body
    );
    // Another tenant is not touched by this one's ceiling being spent.
    let b = signup(&d, "bo@example.com").await;
    let kb = post(&d, "/v1/api/account/keys", Some(&b.token), json!({ "scope": "ingest" }))
        .await
        .body["secret"]
        .as_str()
        .unwrap()
        .to_owned();
    let rb = post(
        &d,
        "/v1/ingest",
        Some(&kb),
        json!({ "spans": [span("feedfeedfeedfeedfeedfeedfeedfeed", &b.project)] }),
    )
    .await;
    assert_eq!(rb.body["accepted"], json!(1), "{}", rb.body);
    let b_org =
        get(&d, "/v1/api/orgs", Some(&b.token)).await.body[0]["id"].as_str().unwrap().to_owned();
    let b_usage = get(&d, &format!("/v1/api/orgs/{b_org}/usage"), Some(&b.token)).await;
    assert_eq!(
        b_usage.body["quota_per_minute"],
        json!(3),
        "a new tenant started with no ceiling: {}",
        b_usage.body
    );
    let usage = get(&d, &format!("/v1/api/orgs/{org}/usage"), Some(&a.token)).await;
    assert_eq!(usage.body["quota_per_minute"], json!(3), "{}", usage.body);
    assert_eq!(usage.body["admitted_total"], json!(3));
    assert_eq!(usage.body["refused_total"], json!(1));
}

// ------------------------------------------- accounts without sign-up ---

#[tokio::test]
async fn without_sign_up_people_arrive_by_invitation_and_configured_keys_keep_working() {
    let (mut c, o) = config(false);
    c.read_keys = vec!["operator-read-key".to_owned()];
    c.account_settings.orgs = vec![ConfigOrg {
        slug: "acme".into(),
        name: Some("Acme".into()),
        owners: vec!["boss@acme.test".into()],
        projects: vec!["default".into()],
        ..ConfigOrg::default()
    }];
    let d = daemon_with(c, o).await;
    let r = post(
        &d,
        "/v1/api/account/signup",
        None,
        json!({ "email": "x@acme.test", "password": PASSWORD }),
    )
    .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    assert_eq!(get(&d, "/healthz", None).await.body["accounts"], json!(true));
    let token = mailed_token(&d, "boss@acme.test", "invite");
    let r =
        post(&d, "/v1/api/invites/accept", None, json!({ "token": token, "password": PASSWORD }))
            .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["org"]["role"], json!("owner"));
    assert_eq!(r.body["org"]["managed_by_config"], json!(true));
    let session = r.body["session"]["token"].as_str().unwrap().to_owned();
    assert_eq!(
        get(&d, "/v1/api/traces?project=default", Some(&session)).await.status,
        StatusCode::OK
    );
    assert_eq!(
        get(&d, "/v1/api/traces?project=default", Some("operator-read-key")).await.status,
        StatusCode::OK
    );
    assert_eq!(
        get(&d, "/v1/api/traces?project=default", None).await.status,
        StatusCode::UNAUTHORIZED
    );
    let r = post(&d, "/v1/api/orgs", Some(&session), json!({ "name": "Side project" })).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{}", r.body);
    // A restart with the same file sends no second invitation.
    let before = d.outbox.all().len();
    mcpd_server::orgs::apply_config(&d.state).await.unwrap();
    assert_eq!(d.outbox.all().len(), before);
}

// ------------------------------------------------- single sign-on ---

/// A provider: discovery, keys and a token endpoint, on a port the operating
/// system picks. The ID token it hands out carries whatever claims the test
/// put in `next`.
struct Provider {
    issuer: String,
    next: Arc<Mutex<Value>>,
}

async fn provider() -> Provider {
    use aws_lc_rs::signature::{EcdsaKeyPair, KeyPair, ECDSA_P256_SHA256_FIXED_SIGNING};
    let rng = aws_lc_rs::rand::SystemRandom::new();
    let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng).unwrap();
    let pair = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8.as_ref()).unwrap();
    let point = pair.public_key().as_ref().to_vec();
    let b64 = |b: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b);
    let jwks = json!({ "keys": [{ "kty": "EC", "crv": "P-256", "kid": "k1", "alg": "ES256", "use": "sig",
        "x": b64(&point[1..33]), "y": b64(&point[33..65]) }] });
    let key = jsonwebtoken::EncodingKey::from_ec_der(pkcs8.as_ref());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let issuer = format!("http://{}", listener.local_addr().unwrap());
    let next = Arc::new(Mutex::new(json!({})));
    let (iss, n) = (issuer.clone(), Arc::clone(&next));
    let app = axum::Router::new()
        .route(
            "/.well-known/openid-configuration",
            axum::routing::get({
                let iss = iss.clone();
                move || async move {
                    axum::Json(
                        json!({ "issuer": iss, "authorization_endpoint": format!("{iss}/authorize"),
                    "token_endpoint": format!("{iss}/token"), "jwks_uri": format!("{iss}/jwks") }),
                    )
                }
            }),
        )
        .route(
            "/jwks",
            axum::routing::get(move || {
                let j = jwks.clone();
                async move { axum::Json(j) }
            }),
        )
        .route(
            "/token",
            axum::routing::post(move |body: String| {
                let (n, key, iss) = (Arc::clone(&n), key.clone(), iss.clone());
                async move {
                    assert!(body.contains("code_verifier="), "no PKCE verifier: {body}");
                    let mut claims = n.lock().unwrap().clone();
                    let now = time::OffsetDateTime::now_utc().unix_timestamp();
                    claims["iss"] = claims.get("iss").cloned().unwrap_or(json!(iss));
                    claims["aud"] = claims.get("aud").cloned().unwrap_or(json!("client-1"));
                    claims["exp"] = json!(now + 300);
                    claims["iat"] = json!(now);
                    let mut h = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::ES256);
                    h.kid = Some("k1".into());
                    let t = jsonwebtoken::encode(&h, &claims, &key).unwrap();
                    axum::Json(
                        json!({ "access_token": "x", "token_type": "Bearer", "id_token": t }),
                    )
                }
            }),
        );
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    Provider { issuer, next }
}

async fn sso_daemon(p: &Provider) -> Daemon {
    let (mut c, o) = config(true);
    // SAFETY of the variable: set once per process, read at apply_config.
    std::env::set_var("MCPD_TEST_OIDC_SECRET", "secret-1");
    c.account_settings.orgs = vec![ConfigOrg {
        slug: "acme".into(),
        owners: vec!["boss@acme.test".into()],
        group_roles: [("mcpd-admins".to_owned(), "admin".to_owned())].into_iter().collect(),
        sso: Some(ConfigSso {
            issuer: Some(p.issuer.clone()),
            client_id: "client-1".into(),
            client_secret_env: Some("MCPD_TEST_OIDC_SECRET".into()),
            domains: vec!["acme.test".into()],
            enforce: true,
            groups_claim: Some("groups".into()),
            ..ConfigSso::default()
        }),
        ..ConfigOrg::default()
    }];
    daemon_with(c, o).await
}

fn query_of(url: &str, name: &str) -> String {
    url::Url::parse(url)
        .unwrap()
        .query_pairs()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.into_owned())
        .unwrap()
}

/// Start a login for `email`, have the provider vouch for `claims`, and finish.
async fn sso_login(d: &Daemon, p: &Provider, email: &str, claims: Value) -> Reply {
    let start = post(d, "/v1/api/sso/start", None, json!({ "email": email })).await;
    assert_eq!(start.status, StatusCode::OK, "{}", start.body);
    let url = start.body["authorize_url"].as_str().unwrap();
    assert_eq!(query_of(url, "code_challenge_method"), "S256");
    let mut c = claims;
    if c.get("nonce").is_none() {
        c["nonce"] = json!(query_of(url, "nonce"));
    }
    *p.next.lock().unwrap() = c;
    let binding = start.body["binding"].as_str().unwrap();
    post(
        d,
        "/v1/api/sso/callback",
        None,
        json!({ "state": query_of(url, "state"), "code": "c", "binding": binding }),
    )
    .await
}

#[tokio::test]
async fn oidc_creates_a_member_in_a_claimed_domain_and_limits_the_session_to_the_org() {
    let p = provider().await;
    let d = sso_daemon(&p).await;
    assert_eq!(get(&d, "/healthz", None).await.body["sso"], json!(true));
    let r = sso_login(
        &d,
        &p,
        "ana@acme.test",
        json!({ "sub": "u1", "email": "ana@acme.test", "groups": ["mcpd-admins"] }),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["org_id"], json!("org_acme"));
    let session = r.body["token"].as_str().unwrap().to_owned();
    let orgs = get(&d, "/v1/api/orgs", Some(&session)).await;
    assert_eq!(orgs.body[0]["role"], json!("admin"), "the group did not map: {}", orgs.body);
    let me = get(&d, "/v1/api/account", Some(&session)).await;
    assert_eq!(me.body["managed_by"], json!("org_acme"));
    assert_eq!(me.body["has_password"], json!(false));
    // The same identity again finds the same account, and the owner the
    // config file names is an owner.
    let again =
        sso_login(&d, &p, "ana@acme.test", json!({ "sub": "u1", "email": "ana@acme.test" })).await;
    assert_eq!(again.status, StatusCode::OK);
    let boss =
        sso_login(&d, &p, "boss@acme.test", json!({ "sub": "u2", "email": "boss@acme.test" }))
            .await;
    let boss_orgs = get(&d, "/v1/api/orgs", Some(boss.body["token"].as_str().unwrap())).await;
    assert_eq!(boss_orgs.body[0]["role"], json!("owner"));
    let log = d
        .state
        .store
        .list_audit(&mcpd_store::AuditQuery {
            project: "org_acme".into(),
            action: Some("sso_login".into()),
            from: None,
            to: None,
            limit: 10,
        })
        .await
        .unwrap();
    assert!(log.len() >= 3);
}

#[tokio::test]
async fn oidc_never_takes_over_an_account_and_refuses_a_forged_or_replayed_callback() {
    let p = provider().await;
    let d = sso_daemon(&p).await;
    // An account with an address in the domain that is not a member.
    let outsider = signup(&d, "eve@acme.test").await;
    let r = sso_login(&d, &p, "eve@acme.test", json!({ "sub": "evil", "email": "eve@acme.test" }))
        .await;
    assert_eq!(
        r.status,
        StatusCode::FORBIDDEN,
        "an identity provider took over an account: {}",
        r.body
    );
    // Refused by the rule that an existing account is never linked, not by a
    // later check that happens to catch it too.
    assert!(
        r.body["message"].as_str().unwrap_or("").contains("ask an admin to invite"),
        "{}",
        r.body
    );
    let log = d
        .state
        .store
        .list_audit(&mcpd_store::AuditQuery {
            project: "org_acme".into(),
            action: Some("sso_login".into()),
            from: None,
            to: None,
            limit: 10,
        })
        .await
        .unwrap();
    assert!(
        log.iter().all(|r| r.result == mcpd_core::AuditResult::Refused),
        "a refused login was audited as allowed"
    );
    assert_eq!(get(&d, "/v1/api/account", Some(&outsider.token)).await.status, StatusCode::OK);
    // An address outside the claimed domains.
    let r =
        sso_login(&d, &p, "ana@acme.test", json!({ "sub": "u9", "email": "ana@elsewhere.test" }))
            .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{}", r.body);
    // A provider that says the address is not verified.
    let r = sso_login(
        &d,
        &p,
        "ana@acme.test",
        json!({ "sub": "u8", "email": "ana@acme.test", "email_verified": false }),
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{}", r.body);
    // A nonce from another login.
    let r = sso_login(
        &d,
        &p,
        "ana@acme.test",
        json!({ "sub": "u7", "email": "ana@acme.test", "nonce": "other" }),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.body);
    // Another audience, another issuer.
    let r = sso_login(
        &d,
        &p,
        "ana@acme.test",
        json!({ "sub": "u7", "email": "ana@acme.test", "aud": "someone-else" }),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.body);
    let r = sso_login(
        &d,
        &p,
        "ana@acme.test",
        json!({ "sub": "u7", "email": "ana@acme.test", "iss": "https://evil.test" }),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.body);
    // The binding from another browser, and a state used twice.
    let start = post(&d, "/v1/api/sso/start", None, json!({ "email": "ana@acme.test" })).await;
    let url = start.body["authorize_url"].as_str().unwrap().to_owned();
    *p.next.lock().unwrap() =
        json!({ "sub": "u1", "email": "ana@acme.test", "nonce": query_of(&url, "nonce") });
    let state = query_of(&url, "state");
    let r = post(
        &d,
        "/v1/api/sso/callback",
        None,
        json!({ "state": state, "code": "c", "binding": "someone else's" }),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.body);
    let r = post(
        &d,
        "/v1/api/sso/callback",
        None,
        json!({ "state": state, "code": "c", "binding": start.body["binding"] }),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "a state worked after a failed use: {}", r.body);
}

#[tokio::test]
async fn an_enforcing_org_refuses_its_members_password_log_in() {
    let p = provider().await;
    let d = sso_daemon(&p).await;
    // Invited with a password, then held to single sign-on by the domain.
    let boss =
        sso_login(&d, &p, "boss@acme.test", json!({ "sub": "u2", "email": "boss@acme.test" }))
            .await
            .body["token"]
            .as_str()
            .unwrap()
            .to_owned();
    let boss = Who { token: boss, project: String::new(), _email: "boss@acme.test".into() };
    let m = join(&d, &boss, "org_acme", "mo@acme.test", "member").await;
    let _ = m;
    let r = post(
        &d,
        "/v1/api/account/login",
        None,
        json!({ "email": "mo@acme.test", "password": PASSWORD }),
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{}", r.body);
    assert_eq!(r.body["error"], json!("sso_required"));
}

// ----------------------------------------------------------- SCIM ---

async fn scim(d: &Daemon, method: &str, path: &str, token: &str, body: Option<Value>) -> Reply {
    send(&d.app, method, path, Some(token), body).await
}

#[tokio::test]
async fn scim_provisions_maps_groups_and_deprovisions() {
    let p = provider().await;
    let d = sso_daemon(&p).await;
    let boss =
        sso_login(&d, &p, "boss@acme.test", json!({ "sub": "u2", "email": "boss@acme.test" }))
            .await
            .body["token"]
            .as_str()
            .unwrap()
            .to_owned();
    let made = post(&d, "/v1/api/orgs/org_acme/scim-token", Some(&boss), json!({})).await;
    assert_eq!(made.status, StatusCode::OK, "{}", made.body);
    let t = made.body["token"].as_str().unwrap().to_owned();

    // Discovery documents, and refusal without the token.
    assert_eq!(
        scim(&d, "GET", "/scim/v2/ServiceProviderConfig", &t, None).await.body["patch"]
            ["supported"],
        json!(true)
    );
    assert_eq!(scim(&d, "GET", "/scim/v2/Schemas", &t, None).await.body["totalResults"], json!(2));
    assert_eq!(
        scim(&d, "GET", "/scim/v2/ResourceTypes", &t, None).await.body["Resources"][0]["id"],
        json!("User")
    );
    let r = scim(&d, "GET", "/scim/v2/Users", "not-a-token", None).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    assert_eq!(r.body["schemas"][0], json!("urn:ietf:params:scim:api:messages:2.0:Error"));
    // A session is not a SCIM token.
    assert_eq!(
        scim(&d, "GET", "/scim/v2/Users", &boss, None).await.status,
        StatusCode::UNAUTHORIZED
    );

    let user = json!({ "schemas": ["urn:ietf:params:scim:schemas:core:2.0:User"], "userName": "Kim@acme.test",
        "externalId": "ext-1", "name": { "givenName": "Kim", "familyName": "Lee" },
        "emails": [{ "value": "kim@acme.test", "primary": true }], "active": true });
    let r = scim(&d, "POST", "/scim/v2/Users", &t, Some(user.clone())).await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    let uid = r.body["id"].as_str().unwrap().to_owned();
    assert_eq!(r.body["userName"], json!("kim@acme.test"));
    assert_eq!(
        scim(&d, "POST", "/scim/v2/Users", &t, Some(user)).await.body["scimType"],
        json!("uniqueness")
    );
    let found =
        scim(&d, "GET", "/scim/v2/Users?filter=userName%20eq%20%22kim%40acme.test%22", &t, None)
            .await;
    assert_eq!(found.body["totalResults"], json!(1), "{}", found.body);
    assert_eq!(
        scim(&d, "GET", "/scim/v2/Users?filter=title%20eq%20%22x%22", &t, None).await.body
            ["scimType"],
        json!("invalidFilter")
    );
    let role_of = |email: &'static str| {
        let d = &d;
        let boss = boss.clone();
        async move {
            let m = get(d, "/v1/api/orgs/org_acme/members", Some(&boss)).await;
            m.body
                .as_array()
                .unwrap()
                .iter()
                .find(|x| x["email"] == email)
                .map(|x| x["role"].clone())
        }
    };
    assert_eq!(role_of("kim@acme.test").await, Some(json!("member")));

    let g = scim(
        &d,
        "POST",
        "/scim/v2/Groups",
        &t,
        Some(json!({ "displayName": "mcpd-admins", "members": [{ "value": uid }] })),
    )
    .await;
    assert_eq!(g.status, StatusCode::CREATED, "{}", g.body);
    let gid = g.body["id"].as_str().unwrap().to_owned();
    assert_eq!(
        role_of("kim@acme.test").await,
        Some(json!("admin")),
        "the group did not map to a role"
    );
    let r = scim(
        &d,
        "PATCH",
        &format!("/scim/v2/Groups/{gid}"),
        &t,
        Some(json!({
        "schemas": ["urn:ietf:params:scim:api:messages:2.0:PatchOp"],
        "Operations": [{ "op": "remove", "path": format!("members[value eq \"{uid}\"]") }] })),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(role_of("kim@acme.test").await, Some(json!("member")));

    // Deprovisioning ends the account's reach and its sessions.
    let kim = sso_login(&d, &p, "kim@acme.test", json!({ "sub": "kim", "email": "kim@acme.test" }))
        .await
        .body["token"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(get(&d, "/v1/api/account", Some(&kim)).await.status, StatusCode::OK);
    let r = scim(
        &d,
        "PATCH",
        &format!("/scim/v2/Users/{uid}"),
        &t,
        Some(json!({
        "schemas": ["urn:ietf:params:scim:api:messages:2.0:PatchOp"],
        "Operations": [{ "op": "Replace", "path": "active", "value": "False" }] })),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["active"], json!(false));
    assert_eq!(
        get(&d, "/v1/api/account", Some(&kim)).await.status,
        StatusCode::UNAUTHORIZED,
        "a session outlived deprovisioning"
    );
    let r =
        sso_login(&d, &p, "kim@acme.test", json!({ "sub": "kim", "email": "kim@acme.test" })).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "a deactivated account logged in: {}", r.body);
    assert_eq!(
        scim(&d, "DELETE", &format!("/scim/v2/Users/{uid}"), &t, None).await.status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        scim(&d, "GET", &format!("/scim/v2/Users/{uid}"), &t, None).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(role_of("kim@acme.test").await, None);

    // An address outside the domain is invited, never created.
    let r = scim(
        &d,
        "POST",
        "/scim/v2/Users",
        &t,
        Some(json!({ "userName": "pat@else.test", "active": true })),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    assert!(d.outbox.last_to("pat@else.test").unwrap().body.contains("#invite="));
    assert_eq!(role_of("pat@else.test").await, None);

    // The token reaches its own organisation only.
    let other = signup(&d, "x@example.com").await;
    let (org_b, _) = org_of(&d, &other, "B").await;
    let tb = post(&d, &format!("/v1/api/orgs/{org_b}/scim-token"), Some(&other.token), json!({}))
        .await
        .body["token"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        scim(&d, "GET", &format!("/scim/v2/Groups/{gid}"), &tb, None).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(scim(&d, "GET", "/scim/v2/Users", &tb, None).await.body["totalResults"], json!(0));
    let log = get(&d, "/v1/api/orgs/org_acme/audit", Some(&boss)).await;
    for action in [
        "scim_token_rotate",
        "scim_user_create",
        "scim_group_create",
        "scim_user_update",
        "scim_user_delete",
    ] {
        assert!(log.body.to_string().contains(action), "no {action} row");
    }
}
