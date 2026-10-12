//! Open sign-up, driven through the real router.
//!
//! The property that matters most is the last group: two accounts on one
//! daemon, and every endpoint family tried with one account's credentials
//! against the other's project. A daemon that lets strangers sign up is only
//! safe if none of them can read, write, delete or run anything outside their
//! own project, so each family is named here rather than sampled.

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use mcpd_runs::{RunStore, SqliteRunStore};
use mcpd_server::accounts::AccountStore;
use mcpd_server::{router, AppState, ServerConfig};
use mcpd_store::sqlite::SqliteStore;
use mcpd_store::Store;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Arc;
use tower::ServiceExt;

const PASSWORD: &str = "a long enough password";
const OPERATOR_KEY: &str = "operator-admin-key-for-tests";

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
        "mcpd-accounts-http-{}-{stamp}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).expect("create the test directory");
    dir
}

fn signup_config() -> ServerConfig {
    ServerConfig { signup: true, ..ServerConfig::default() }
}

/// The router over private files: signals, runs and accounts.
async fn app(config: ServerConfig) -> Router {
    let dir = temp_dir();
    let store = SqliteStore::open(dir.join("signals.db"), 2).expect("open the store");
    store.migrate().await.expect("migrate the store");
    let runs = SqliteRunStore::open(dir.join("runs.db"), config.run_config).expect("open runs");
    runs.migrate().await.expect("migrate runs");
    let accounts =
        AccountStore::open(dir.join("accounts.db").to_str().expect("a utf-8 path")).expect("open");
    let state = AppState::new(Arc::new(store) as Arc<dyn Store>, config)
        .with_run_store(Arc::new(runs) as Arc<dyn RunStore>)
        .with_account_store(Arc::new(accounts));
    router(Arc::new(state))
}

struct Reply {
    status: StatusCode,
    body: Value,
    headers: axum::http::HeaderMap,
}

#[derive(Default)]
struct Call<'a> {
    key: Option<&'a str>,
    body: Option<Value>,
    peer: Option<&'a str>,
    headers: Vec<(&'a str, &'a str)>,
}

async fn send(app: &Router, method: &str, path: &str, c: Call<'_>) -> Reply {
    let mut req = Request::builder().method(method).uri(path);
    if let Some(k) = c.key {
        req = req.header("authorization", format!("Bearer {k}"));
    }
    for (name, value) in &c.headers {
        req = req.header(*name, *value);
    }
    let mut req = match c.body {
        Some(b) => req.header("content-type", "application/json").body(Body::from(b.to_string())),
        None => req.body(Body::empty()),
    }
    .expect("build the request");
    if let Some(peer) = c.peer {
        let addr: SocketAddr = peer.parse().expect("a socket address");
        req.extensions_mut().insert(ConnectInfo(addr));
    }
    let res = app.clone().oneshot(req).await.expect("route the request");
    let status = res.status();
    let headers = res.headers().clone();
    let bytes = res.into_body().collect().await.expect("read the body").to_bytes();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| json!({ "raw": String::from_utf8_lossy(&bytes) }))
    };
    Reply { status, body, headers }
}

async fn get(app: &Router, path: &str, key: Option<&str>) -> Reply {
    send(app, "GET", path, Call { key, ..Call::default() }).await
}

async fn post(app: &Router, path: &str, key: Option<&str>, body: Value) -> Reply {
    send(app, "POST", path, Call { key, body: Some(body), ..Call::default() }).await
}

async fn delete(app: &Router, path: &str, key: Option<&str>) -> Reply {
    send(app, "DELETE", path, Call { key, ..Call::default() }).await
}

/// A signed-up account: its session token and its project.
struct Who {
    token: String,
    project: String,
}

async fn signup_from(app: &Router, email: &str, peer: &str) -> Reply {
    send(
        app,
        "POST",
        "/v1/api/account/signup",
        Call {
            body: Some(json!({ "email": email, "password": PASSWORD })),
            peer: Some(peer),
            ..Call::default()
        },
    )
    .await
}

async fn signup(app: &Router, email: &str) -> Who {
    // A peer per account, so a test that makes several does not meet the
    // per-address limit, which has a test of its own.
    let peer = format!("192.0.2.{}:5000", email.len() + email.as_bytes()[0] as usize % 100);
    let r = signup_from(app, email, &peer).await;
    assert_eq!(r.status, StatusCode::OK, "sign-up failed: {}", r.body);
    Who {
        token: r.body["token"].as_str().expect("a token").to_owned(),
        project: r.body["project"].as_str().expect("a project").to_owned(),
    }
}

async fn mint(app: &Router, who: &Who, scope: &str) -> String {
    let r = post(app, "/v1/api/account/keys", Some(&who.token), json!({ "scope": scope })).await;
    assert_eq!(r.status, StatusCode::OK, "minting a {scope} key failed: {}", r.body);
    r.body["secret"].as_str().expect("a secret").to_owned()
}

fn span(trace_id: &str, project: &str) -> Value {
    json!({
        "trace_id": trace_id,
        "span_id": "0123456789abcdef",
        "name": "checkout",
        "kind": "agent_run",
        "status": "ok",
        "start_time": "2026-01-01T00:00:00Z",
        "end_time": "2026-01-01T00:00:01Z",
        "is_root": true,
        "is_event": false,
        "correlation": { "project": project, "user_id": "user-1" },
        "received_at": "2026-01-01T00:00:01Z"
    })
}

fn definition() -> Value {
    json!({
        "id": "one_step",
        "kind": "workflow",
        "body": {
            "limits": { "max_steps": 5, "run_timeout_ms": 600000 },
            "steps": [ { "kind": "task", "id": "only", "input": {} } ],
            "output": {}
        }
    })
}

const TRACE_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

// ------------------------------------------------------ sign-up off ---

#[tokio::test]
async fn without_signup_the_account_doors_are_404_and_nothing_else_changes() {
    let app = app(ServerConfig::default()).await;
    // The laptop path: no keys configured, so reading and writing are open.
    assert_eq!(get(&app, "/v1/api/traces", None).await.status, StatusCode::OK);
    let r = post(&app, "/v1/ingest", None, json!({ "spans": [span(TRACE_A, "default")] })).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["accepted"], json!(1));
    for (method, path) in [
        ("POST", "/v1/api/account/signup"),
        ("POST", "/v1/api/account/login"),
        ("POST", "/v1/api/account/logout"),
        ("GET", "/v1/api/account"),
        ("GET", "/v1/api/account/keys"),
        ("POST", "/v1/api/account/keys"),
        ("DELETE", "/v1/api/account/keys/key_x"),
    ] {
        let body = Some(json!({ "email": "a@example.com", "password": PASSWORD, "scope": "read" }));
        let r = send(&app, method, path, Call { body, ..Call::default() }).await;
        assert_eq!(r.status, StatusCode::NOT_FOUND, "{method} {path}: {}", r.body);
    }
    // The health probe says nothing about sign-up, not even `false`.
    let health = get(&app, "/healthz", None).await;
    assert!(health.body.get("signup").is_none(), "{}", health.body);
}

// ---------------------------------------------------- sign-up, log-in ---

#[tokio::test]
async fn signup_creates_an_account_with_its_own_project_and_logs_it_in() {
    let app = app(signup_config()).await;
    let health = get(&app, "/healthz", None).await;
    assert_eq!(health.body["signup"], json!(true));

    let a = signup(&app, " Ana@Example.com ").await;
    assert!(a.project.starts_with("prj_"), "{}", a.project);
    assert!(a.token.starts_with("mcpds_"));
    let me = get(&app, "/v1/api/account", Some(&a.token)).await;
    assert_eq!(me.status, StatusCode::OK, "{}", me.body);
    assert_eq!(me.body["email"], json!("ana@example.com"));
    assert_eq!(me.body["project"], json!(a.project));

    let b = signup(&app, "bo@example.com").await;
    assert_ne!(a.project, b.project, "two accounts were given one project");
}

#[tokio::test]
async fn signup_refuses_a_bad_email_a_short_password_and_a_taken_email() {
    let app = app(signup_config()).await;
    let r = post(
        &app,
        "/v1/api/account/signup",
        None,
        json!({ "email": "not-an-email", "password": PASSWORD }),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(r.body["field"], json!("email"));
    let r = post(
        &app,
        "/v1/api/account/signup",
        None,
        json!({ "email": "a@example.com", "password": "123456789" }),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(r.body["field"], json!("password"));

    signup(&app, "a@example.com").await;
    let r = signup_from(&app, "A@EXAMPLE.COM", "198.51.100.9:1").await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{}", r.body);
    assert_eq!(r.body["error"], json!("account_exists"));
}

#[tokio::test]
async fn login_gives_one_answer_for_a_wrong_password_and_an_unknown_email() {
    let app = app(signup_config()).await;
    signup(&app, "ana@example.com").await;

    let wrong = post(
        &app,
        "/v1/api/account/login",
        None,
        json!({ "email": "ana@example.com", "password": "not the password" }),
    )
    .await;
    let unknown = post(
        &app,
        "/v1/api/account/login",
        None,
        json!({ "email": "nobody@example.com", "password": PASSWORD }),
    )
    .await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);
    assert_eq!(wrong.body, unknown.body, "the two failures are distinguishable");
    assert_eq!(unknown.status, StatusCode::UNAUTHORIZED);

    let ok = post(
        &app,
        "/v1/api/account/login",
        None,
        json!({ "email": "ANA@example.com", "password": PASSWORD }),
    )
    .await;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.body);
    let token = ok.body["token"].as_str().expect("a token").to_owned();
    assert_eq!(get(&app, "/v1/api/account", Some(&token)).await.status, StatusCode::OK);
}

#[tokio::test]
async fn logout_ends_that_session_and_leaves_the_others_and_the_keys() {
    let app = app(signup_config()).await;
    let a = signup(&app, "ana@example.com").await;
    let key = mint(&app, &a, "read").await;
    let second = post(
        &app,
        "/v1/api/account/login",
        None,
        json!({ "email": "ana@example.com", "password": PASSWORD }),
    )
    .await
    .body["token"]
        .as_str()
        .expect("a token")
        .to_owned();

    let r = post(&app, "/v1/api/account/logout", Some(&a.token), json!({})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["deleted"], json!(1));
    assert_eq!(get(&app, "/v1/api/account", Some(&a.token)).await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(get(&app, "/v1/api/account", Some(&second)).await.status, StatusCode::OK);
    let read = get(&app, &format!("/v1/api/traces?project={}", a.project), Some(&key)).await;
    assert_eq!(read.status, StatusCode::OK, "{}", read.body);
}

// ------------------------------------------------------- rate limits ---

#[tokio::test]
async fn signup_is_limited_per_client_address() {
    let app = app(signup_config()).await;
    for i in 0..mcpd_server::accounts::SIGNUPS_PER_HOUR {
        let r = signup_from(&app, &format!("user{i}@example.com"), "203.0.113.7:4000").await;
        assert_eq!(r.status, StatusCode::OK, "sign-up {i} failed: {}", r.body);
    }
    let r = signup_from(&app, "one-more@example.com", "203.0.113.7:4001").await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS, "{}", r.body);
    assert_eq!(r.body["error"], json!("rate_limited"));
    let wait: u64 = r.headers["retry-after"].to_str().unwrap().parse().unwrap();
    assert!(wait >= 1);
    // Another address is not affected.
    let r = signup_from(&app, "one-more@example.com", "203.0.113.8:4000").await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
}

#[tokio::test]
async fn failed_logins_are_limited_per_email_from_any_address() {
    let app = app(signup_config()).await;
    signup(&app, "ana@example.com").await;
    let attempt = |peer: String, password: &'static str| {
        let app = app.clone();
        async move {
            send(
                &app,
                "POST",
                "/v1/api/account/login",
                Call {
                    body: Some(json!({ "email": "ana@example.com", "password": password })),
                    peer: Some(&peer),
                    ..Call::default()
                },
            )
            .await
        }
    };
    for i in 0..mcpd_server::accounts::FAILED_LOGINS_PER_EMAIL {
        let r = attempt(format!("198.51.100.{i}:1"), "wrong password").await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED, "attempt {i}: {}", r.body);
    }
    // The right password, from a fresh address, is still refused for now.
    let r = attempt("198.51.100.200:1".to_owned(), PASSWORD).await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS, "{}", r.body);
}

#[tokio::test]
async fn logins_are_limited_per_client_address() {
    let app = app(signup_config()).await;
    for i in 0..mcpd_server::accounts::LOGINS_PER_QUARTER_HOUR {
        // Unknown emails, so the per-email limit is not what stops it.
        let r = send(
            &app,
            "POST",
            "/v1/api/account/login",
            Call {
                body: Some(json!({ "email": format!("u{i}@example.com"), "password": PASSWORD })),
                peer: Some("203.0.113.50:1"),
                ..Call::default()
            },
        )
        .await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED, "{}", r.body);
    }
    let r = send(
        &app,
        "POST",
        "/v1/api/account/login",
        Call {
            body: Some(json!({ "email": "last@example.com", "password": PASSWORD })),
            peer: Some("203.0.113.50:2"),
            ..Call::default()
        },
    )
    .await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS, "{}", r.body);
}

#[tokio::test]
async fn the_client_address_header_is_believed_only_from_a_trusted_proxy() {
    let config = ServerConfig {
        client_ip: mcpd_server::identity::ClientIp::new(
            Some("cf-connecting-ip"),
            &["10.0.0.0/8".to_owned()],
        )
        .expect("a valid configuration"),
        ..signup_config()
    };
    let app = app(config).await;
    let from = |peer: &'static str, client: &'static str, n: u32| {
        let app = app.clone();
        async move {
            send(
                &app,
                "POST",
                "/v1/api/account/signup",
                Call {
                    body: Some(
                        json!({ "email": format!("x{n}@example.com"), "password": PASSWORD }),
                    ),
                    peer: Some(peer),
                    headers: vec![("cf-connecting-ip", client)],
                    ..Call::default()
                },
            )
            .await
            .status
        }
    };
    // Through the proxy: the limit counts the client the header names.
    let mut n = 0;
    for _ in 0..mcpd_server::accounts::SIGNUPS_PER_HOUR {
        n += 1;
        assert_eq!(from("10.1.2.3:1", "192.0.2.1", n).await, StatusCode::OK);
    }
    n += 1;
    assert_eq!(from("10.1.2.3:1", "192.0.2.1", n).await, StatusCode::TOO_MANY_REQUESTS);
    n += 1;
    assert_eq!(from("10.1.2.3:1", "192.0.2.2", n).await, StatusCode::OK, "another client");

    // Not through the proxy: the header is ignored and the peer is counted, so
    // changing it on every request does not escape the limit.
    for i in 0..mcpd_server::accounts::SIGNUPS_PER_HOUR {
        n += 1;
        let client = if i % 2 == 0 { "192.0.2.10" } else { "192.0.2.11" };
        assert_eq!(from("198.51.100.1:1", client, n).await, StatusCode::OK);
    }
    n += 1;
    assert_eq!(from("198.51.100.1:1", "192.0.2.12", n).await, StatusCode::TOO_MANY_REQUESTS);
}

// --------------------------------------------------------- credentials ---

#[tokio::test]
async fn with_signup_a_request_with_no_key_is_refused_even_with_no_keys_configured() {
    let app = app(signup_config()).await;
    for path in ["/v1/api/traces", "/v1/api/projects", "/v1/api/runs", "/v1/api/audit"] {
        assert_eq!(get(&app, path, None).await.status, StatusCode::UNAUTHORIZED, "{path}");
    }
    let r = post(&app, "/v1/ingest", None, json!({ "spans": [span(TRACE_A, "default")] })).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    let r = send(
        &app,
        "POST",
        "/v1/traces",
        Call { body: Some(json!({ "resourceSpans": [] })), ..Call::default() },
    )
    .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        post(&app, "/v1/api/runs", None, json!({ "definition_id": "x" })).await.status,
        StatusCode::UNAUTHORIZED
    );
    // The three doors that must stay open.
    assert_eq!(get(&app, "/healthz", None).await.status, StatusCode::OK);
    assert_eq!(get(&app, "/readyz", None).await.status, StatusCode::OK);
    assert_eq!(get(&app, "/metrics", None).await.status, StatusCode::OK);
    // A made-up key is not a credential either.
    assert_eq!(
        get(&app, "/v1/api/traces", Some("mcpdk_made_up")).await.status,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn an_account_key_grants_its_one_scope_and_cannot_manage_the_account() {
    let app = app(signup_config()).await;
    let a = signup(&app, "ana@example.com").await;
    let traces = format!("/v1/api/traces?project={}", a.project);
    let ingest = mint(&app, &a, "ingest").await;
    let read = mint(&app, &a, "read").await;
    let admin = mint(&app, &a, "admin").await;

    let batch = json!({ "spans": [span(TRACE_A, &a.project)] });
    assert_eq!(post(&app, "/v1/ingest", Some(&ingest), batch.clone()).await.status, StatusCode::OK);
    assert_eq!(get(&app, &traces, Some(&ingest)).await.status, StatusCode::FORBIDDEN);
    assert_eq!(get(&app, &traces, Some(&read)).await.status, StatusCode::OK);
    assert_eq!(post(&app, "/v1/ingest", Some(&read), batch).await.status, StatusCode::FORBIDDEN);
    let del = format!("/v1/api/traces/{TRACE_A}?project={}", a.project);
    assert_eq!(delete(&app, &del, Some(&read)).await.status, StatusCode::FORBIDDEN);
    assert_eq!(delete(&app, &del, Some(&admin)).await.status, StatusCode::OK);

    // No API key, of any scope, reaches the account endpoints.
    for key in [&ingest, &read, &admin] {
        assert_eq!(
            get(&app, "/v1/api/account/keys", Some(key)).await.status,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            post(&app, "/v1/api/account/keys", Some(key), json!({ "scope": "admin" })).await.status,
            StatusCode::FORBIDDEN
        );
    }
}

#[tokio::test]
async fn keys_are_listed_without_secrets_and_a_revoked_key_stops_working() {
    let app = app(signup_config()).await;
    let a = signup(&app, "ana@example.com").await;
    let r = post(
        &app,
        "/v1/api/account/keys",
        Some(&a.token),
        json!({ "scope": "read", "name": "dashboards" }),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let secret = r.body["secret"].as_str().unwrap().to_owned();
    let id = r.body["key"]["id"].as_str().unwrap().to_owned();
    assert!(secret.starts_with(r.body["key"]["prefix"].as_str().unwrap()));

    let list = get(&app, "/v1/api/account/keys", Some(&a.token)).await;
    assert_eq!(list.status, StatusCode::OK);
    assert_eq!(list.body.as_array().unwrap().len(), 1);
    assert_eq!(list.body[0]["name"], json!("dashboards"));
    assert_eq!(list.body[0]["scope"], json!("read"));
    assert!(!list.body.to_string().contains(&secret), "a listing carried the secret");

    let traces = format!("/v1/api/traces?project={}", a.project);
    assert_eq!(get(&app, &traces, Some(&secret)).await.status, StatusCode::OK);
    let r = delete(&app, &format!("/v1/api/account/keys/{id}"), Some(&a.token)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(get(&app, &traces, Some(&secret)).await.status, StatusCode::UNAUTHORIZED);
    let r = delete(&app, &format!("/v1/api/account/keys/{id}"), Some(&a.token)).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    let r = post(&app, "/v1/api/account/keys", Some(&a.token), json!({ "scope": "root" })).await;
    assert!(r.status.is_client_error(), "an unknown scope was accepted: {}", r.body);
}

#[tokio::test]
async fn an_account_key_writes_into_its_own_project_whatever_the_payload_names() {
    let app = app(signup_config()).await;
    let a = signup(&app, "ana@example.com").await;
    let b = signup(&app, "bo@example.com").await;
    let ingest = mint(&app, &a, "ingest").await;

    // One span naming the default project and one naming B's.
    let batch = json!({ "spans": [
        span("11111111111111111111111111111111", "default"),
        span("22222222222222222222222222222222", &b.project),
    ] });
    let r = post(&app, "/v1/ingest", Some(&ingest), batch).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["accepted"], json!(2), "{}", r.body);

    let mine = get(&app, &format!("/v1/api/traces?project={}", a.project), Some(&a.token)).await;
    assert_eq!(mine.body["traces"].as_array().unwrap().len(), 2, "{}", mine.body);
    let theirs = get(&app, &format!("/v1/api/traces?project={}", b.project), Some(&b.token)).await;
    assert_eq!(theirs.body["traces"].as_array().unwrap().len(), 0, "{}", theirs.body);

    // An idempotency key A used does not make B's batch with the same key a
    // replay.
    let key = "shared-idempotency-key";
    let a_batch =
        json!({ "ingest_key": key, "spans": [span("33333333333333333333333333333333", "x")] });
    assert_eq!(post(&app, "/v1/ingest", Some(&ingest), a_batch).await.body["accepted"], json!(1));
    let b_ingest = mint(&app, &b, "ingest").await;
    let b_batch =
        json!({ "ingest_key": key, "spans": [span("44444444444444444444444444444444", "x")] });
    let r = post(&app, "/v1/ingest", Some(&b_ingest), b_batch).await;
    assert_eq!(r.body["accepted"], json!(1), "B's batch was taken for A's replay: {}", r.body);
}

#[tokio::test]
async fn a_configured_key_keeps_its_reach_beside_accounts() {
    let config = ServerConfig { admin_keys: vec![OPERATOR_KEY.to_owned()], ..signup_config() };
    let app = app(config).await;
    let a = signup(&app, "ana@example.com").await;
    let ingest = mint(&app, &a, "ingest").await;
    post(&app, "/v1/ingest", Some(&ingest), json!({ "spans": [span(TRACE_A, &a.project)] })).await;
    let r = delete(
        &app,
        &format!("/v1/api/traces/{TRACE_A}?project={}", a.project),
        Some(OPERATOR_KEY),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    // And it is not an account, so it cannot manage one.
    assert_eq!(
        get(&app, "/v1/api/account", Some(OPERATOR_KEY)).await.status,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn a_browser_on_an_allowed_origin_can_preflight_the_account_doors() {
    let config =
        ServerConfig { cors_origins: vec!["https://mcpd.sh".to_owned()], ..signup_config() };
    let app = app(config).await;
    for path in ["/v1/api/account/login", "/v1/api/account/keys/key_1", "/v1/api/traces"] {
        let r = send(
            &app,
            "OPTIONS",
            path,
            Call {
                headers: vec![
                    ("origin", "https://mcpd.sh"),
                    ("access-control-request-method", "DELETE"),
                    ("access-control-request-headers", "authorization,content-type"),
                ],
                ..Call::default()
            },
        )
        .await;
        assert!(r.status.is_success(), "{path}: {}", r.status);
        assert_eq!(r.headers["access-control-allow-origin"], "https://mcpd.sh", "{path}");
        let allowed = r.headers["access-control-allow-headers"].to_str().unwrap().to_lowercase();
        assert!(allowed.contains("authorization"), "{path}: {allowed}");
    }
}

// ---------------------------------------------------------- isolation ---

/// Two accounts, with A holding a run, a trace, a dataset and a definition.
async fn two_tenants() -> (Router, Who, Who, String) {
    let app = app(signup_config()).await;
    let a = signup(&app, "ana@example.com").await;
    let b = signup(&app, "bo@example.com").await;
    let r =
        post(&app, "/v1/ingest", Some(&a.token), json!({ "spans": [span(TRACE_A, "x")] })).await;
    assert_eq!(r.body["accepted"], json!(1), "{}", r.body);
    let r = post(
        &app,
        "/v1/api/datasets",
        Some(&a.token),
        json!({ "dataset_id": "a-cases", "name": "cases", "item_count": 0,
                "created_at": "2026-01-01T00:00:00Z",
                "correlation": { "project": a.project } }),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let r = post(&app, "/v1/api/definitions", Some(&a.token), definition()).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let r =
        post(&app, "/v1/api/runs", Some(&a.token), json!({ "definition_id": "one_step" })).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let run_id = r.body["run_id"].as_str().unwrap().to_owned();
    (app, a, b, run_id)
}

#[tokio::test]
async fn a_read_that_names_no_project_means_the_accounts_own() {
    let (app, a, b, _) = two_tenants().await;
    let mine = get(&app, "/v1/api/traces", Some(&a.token)).await;
    assert_eq!(mine.status, StatusCode::OK, "{}", mine.body);
    assert_eq!(mine.body["traces"].as_array().unwrap().len(), 1, "{}", mine.body);
    let one = get(&app, &format!("/v1/api/traces/{TRACE_A}"), Some(&a.token)).await;
    assert_eq!(one.status, StatusCode::OK, "{}", one.body);

    // B's unnamed read is B's project, so A's trace is not in it.
    let theirs = get(&app, "/v1/api/traces", Some(&b.token)).await;
    assert_eq!(theirs.body["traces"].as_array().unwrap().len(), 0, "{}", theirs.body);
    let r = get(&app, &format!("/v1/api/traces/{TRACE_A}"), Some(&b.token)).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "{}", r.body);
}

#[tokio::test]
async fn an_account_cannot_read_another_accounts_project() {
    let (app, a, b, _) = two_tenants().await;
    let p = &a.project;
    let reads = [
        format!("/v1/api/traces?project={p}"),
        format!("/v1/api/traces/{TRACE_A}?project={p}"),
        format!("/v1/api/traces/{TRACE_A}/aspects?project={p}"),
        format!("/v1/api/spans?project={p}"),
        format!("/v1/api/logs?project={p}"),
        format!("/v1/api/metrics?project={p}&name=x"),
        format!("/v1/api/scores?project={p}"),
        format!("/v1/api/feedback?project={p}&trace_id={TRACE_A}"),
        format!("/v1/api/analytics/summary?project={p}"),
        format!("/v1/api/analytics/cost?project={p}"),
        format!("/v1/api/discovery?project={p}&facet=environment"),
        format!("/v1/api/deltas?project={p}&signal=traces"),
        format!("/v1/api/experiments/compare?project={p}&baseline=x&candidate=y"),
        format!("/v1/api/datasets?project={p}"),
        "/v1/api/datasets/a-cases/items".to_owned(),
        format!("/v1/api/experiments?project={p}"),
        format!("/v1/api/aspects?project={p}"),
        format!("/v1/api/aspects/entities?project={p}&aspect=outcome"),
        format!("/v1/api/aspects/progress?project={p}"),
        format!("/v1/api/themes/snapshots?project={p}&aspect=outcome"),
        format!("/v1/api/themes/flow?project={p}&aspect=outcome"),
        format!("/v1/api/themes/t1?project={p}"),
        format!("/v1/api/themes/t1/examples?project={p}"),
        format!("/v1/api/themes/t1/history?project={p}"),
        format!("/v1/api/audit?project={p}"),
        format!("/v1/api/definitions?project={p}"),
        format!("/v1/api/definitions/one_step?project={p}"),
        format!("/v1/api/runs?project={p}"),
    ];
    // B's session holds every scope on B's project, and none of these is in it.
    let b_read = mint(&app, &b, "read").await;
    for path in &reads {
        for key in [&b.token, &b_read] {
            let r = get(&app, path, Some(key)).await;
            assert!(
                r.status == StatusCode::FORBIDDEN || r.status == StatusCode::NOT_FOUND,
                "B reached A's project at {path}: {} {}",
                r.status,
                r.body
            );
            assert!(!r.body.to_string().contains(TRACE_A), "B read A's trace id at {path}");
        }
    }
    // The project list names B's project and never A's.
    let r = get(&app, "/v1/api/projects", Some(&b.token)).await;
    assert!(!r.body.to_string().contains(p.as_str()), "{}", r.body);
    // And A can read its own, so the refusals above are about B, not the paths.
    let r = get(&app, &format!("/v1/api/traces/{TRACE_A}?project={p}"), Some(&a.token)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
}

#[tokio::test]
async fn an_account_cannot_write_or_delete_in_another_accounts_project() {
    let (app, a, b, _) = two_tenants().await;
    let p = &a.project;
    let corr = json!({ "project": p, "trace_id": TRACE_A });
    let writes = [
        (
            "POST",
            "/v1/api/scores".to_owned(),
            json!({ "score_id": "s1", "scorer_id": "q",
            "trace_id": TRACE_A, "value": 1.0, "source": "manual",
            "timestamp": "2026-01-01T00:00:00Z", "correlation": corr }),
        ),
        (
            "POST",
            "/v1/api/feedback".to_owned(),
            json!({ "feedback_id": "f1", "kind": "thumbs",
            "trace_id": TRACE_A, "value": 1, "timestamp": "2026-01-01T00:00:00Z",
            "correlation": corr }),
        ),
        (
            "POST",
            "/v1/api/datasets".to_owned(),
            json!({ "dataset_id": "b-into-a", "name": "x",
            "item_count": 0, "created_at": "2026-01-01T00:00:00Z",
            "correlation": { "project": p } }),
        ),
        (
            "POST",
            "/v1/api/datasets/a-cases/items".to_owned(),
            json!([{ "item_id": "i1", "dataset_id": "a-cases", "input": {} }]),
        ),
        (
            "POST",
            format!("/v1/api/datasets/a-cases/capture?project={p}"),
            json!({ "trace_ids": [TRACE_A], "use_output_as_expected": false }),
        ),
        (
            "POST",
            "/v1/api/score".to_owned(),
            json!({ "project": p, "trace_ids": [TRACE_A],
            "scorers": ["latency"], "dry_run": false }),
        ),
        ("POST", format!("/v1/api/definitions?project={p}"), definition()),
        ("DELETE", format!("/v1/api/traces/{TRACE_A}?project={p}"), Value::Null),
        ("DELETE", format!("/v1/api/subjects/user-1?project={p}"), Value::Null),
    ];
    for (method, path, body) in writes {
        let body = (!body.is_null()).then_some(body);
        let r =
            send(&app, method, &path, Call { key: Some(&b.token), body, ..Call::default() }).await;
        assert!(
            r.status == StatusCode::FORBIDDEN
                || r.status == StatusCode::NOT_FOUND
                || r.status == StatusCode::BAD_REQUEST,
            "B wrote into A's project at {method} {path}: {} {}",
            r.status,
            r.body
        );
    }
    // A's trace is still there, whole.
    let r = get(&app, &format!("/v1/api/traces/{TRACE_A}?project={p}"), Some(&a.token)).await;
    assert_eq!(r.status, StatusCode::OK, "B's attempts removed A's trace: {}", r.body);
    // A dataset B tried to create in A's project does not exist there.
    let r = get(&app, &format!("/v1/api/datasets?project={p}"), Some(&a.token)).await;
    assert!(!r.body.to_string().contains("b-into-a"), "{}", r.body);
}

#[tokio::test]
async fn an_account_cannot_see_steer_or_work_another_accounts_runs() {
    let (app, a, b, run_id) = two_tenants().await;
    let p = &a.project;
    for path in [
        format!("/v1/api/runs/{run_id}"),
        format!("/v1/api/runs/{run_id}/events"),
        format!("/v1/api/runs/{run_id}/cause?step=only"),
    ] {
        let r = get(&app, &path, Some(&b.token)).await;
        assert_eq!(r.status, StatusCode::FORBIDDEN, "{path}: {}", r.body);
    }
    for (path, body) in [
        (format!("/v1/api/runs/{run_id}/cancel"), json!({})),
        (format!("/v1/api/runs/{run_id}/signal"), json!({ "event": "go", "payload": {} })),
        ("/v1/api/runs".to_owned(), json!({ "definition_id": "one_step", "project": p })),
        ("/v1/api/events".to_owned(), json!({ "event": "approved", "key": "k", "project": p })),
        (
            "/v1/api/workers/register".to_owned(),
            json!({ "worker_id": "w", "project": p,
            "definitions": [] }),
        ),
        (
            "/v1/api/steps/claim".to_owned(),
            json!({ "worker_id": "w", "max": 1, "wait_ms": 0,
            "project": p }),
        ),
    ] {
        let r = post(&app, &path, Some(&b.token), body).await;
        assert!(
            r.status == StatusCode::FORBIDDEN || r.status == StatusCode::BAD_REQUEST,
            "B steered A's runs at {path}: {} {}",
            r.status,
            r.body
        );
    }
    // A claim with no project is B's own project, so A's step is not offered,
    // even when B names A's run.
    let b_worker = mint(&app, &b, "worker").await;
    let r = post(
        &app,
        "/v1/api/steps/claim",
        Some(&b_worker),
        json!({ "worker_id": "w", "max": 1, "wait_ms": 0, "run_id": run_id }),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["leases"], json!([]), "B was leased A's step: {}", r.body);
    // B's list of runs is B's.
    let r = get(&app, "/v1/api/runs", Some(&b.token)).await;
    assert!(!r.body.to_string().contains(&run_id), "{}", r.body);

    // A's worker gets the step, and B cannot touch the lease by its id.
    let a_worker = mint(&app, &a, "worker").await;
    let r = post(
        &app,
        "/v1/api/steps/claim",
        Some(&a_worker),
        json!({ "worker_id": "w", "max": 1, "wait_ms": 0 }),
    )
    .await;
    let lease = r.body["leases"][0]["lease_id"].as_str().expect("A's lease").to_owned();
    for step in ["started", "heartbeat", "complete", "fail"] {
        let body = match step {
            "complete" => json!({ "output": {} }),
            "fail" => json!({ "error": { "class": "fatal", "message": "x" } }),
            _ => json!({}),
        };
        let r = post(&app, &format!("/v1/api/steps/{lease}/{step}"), Some(&b_worker), body).await;
        assert_eq!(r.status, StatusCode::FORBIDDEN, "{step}: {}", r.body);
    }
    // And the run is still A's to read.
    let r = get(&app, &format!("/v1/api/runs/{run_id}"), Some(&a.token)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
}
