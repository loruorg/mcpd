//! The trusted-proxy identity feature, over a real connection.
//!
//! `http.rs` drives the router with `oneshot`. No socket exists there, so
//! `ConnectInfo` is always `None`, every request looks like it came from an
//! unknown peer, and the whole identity path is invisible: that suite stays
//! green whether or not `main.rs` still calls
//! `into_make_service_with_connect_info`. A comment in `main.rs` says out loud
//! that removing it would leave the feature dead in production. This file is
//! the guard that comment asks for.
//!
//! Every request here goes over a real TCP connection to a real listener,
//! served the way `main.rs` serves it, so the peer address is established by
//! the connection and not by a fixture. That is the only way to tell a trusted
//! peer from an untrusted one, which is the property the feature rests on.

use mcpd_server::identity::TrustedIdentity;
use mcpd_server::{router, AppState, ServerConfig};
use mcpd_store::sqlite::SqliteStore;
use mcpd_store::Store;
use std::net::SocketAddr;
use std::sync::Arc;

/// The header a proxy would set. Nothing about the name is special; what makes
/// it trustworthy is the range that is allowed to send it.
const HEADER: &str = "x-forwarded-user";

/// What the proxy asserts, and what an untrusted caller sends to try to claim.
const SUBJECT: &str = "ana@example.com";

const READ_KEY: &str = "read-only-key";

/// A range that covers the loopback address every test client connects from.
const TRUSTED: &str = "127.0.0.1/32";

/// A range that covers nothing this test can connect from, which is what makes
/// the client untrusted. Documentation range, so a stray packet cannot reach a
/// real host either.
const UNTRUSTED: &str = "192.0.2.0/24";

// ------------------------------------------------------------- fixtures ---

/// A private database file per test.
///
/// Same reason as `http.rs`: `SqliteStore::open` gives every in-memory store
/// the same shared-cache URI, so two tests on two threads would share one
/// database and race on `migrate`.
fn temp_db() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!("mcpd-connected-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create the test directory");
    dir.join(format!("{}.db", N.fetch_add(1, Ordering::Relaxed)))
}

fn config(ranges: &[&str]) -> ServerConfig {
    let ranges: Vec<String> = ranges.iter().map(|r| (*r).to_owned()).collect();
    let trusted_identity = if ranges.is_empty() {
        // Both halves unset is the default, and the default is off.
        TrustedIdentity::default()
    } else {
        TrustedIdentity::new(Some(HEADER), &ranges).expect("the test ranges should parse")
    };
    ServerConfig {
        read_keys: vec![READ_KEY.to_owned()],
        trusted_identity,
        ..ServerConfig::default()
    }
}

/// Serve the real router on a real listener, and hand back where to reach it.
///
/// The port is whatever the OS assigns, because a fixed one collides with
/// whatever else is listening on this machine and turns somebody else's server
/// into a convincing failure of this one.
///
/// `into_make_service_with_connect_info` is the line under test. Without it the
/// handlers see no peer address, `subject` is `None` for every request, and the
/// first test below fails.
async fn serve(config: ServerConfig) -> (SocketAddr, Arc<SqliteStore>) {
    let store = Arc::new(SqliteStore::open(temp_db(), 2).expect("open the store"));
    store.migrate().await.expect("migrate the store");
    let app = router(Arc::new(AppState::new(Arc::clone(&store) as Arc<dyn Store>, config)));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind a listener on a port the OS picked");
    let addr = listener.local_addr().expect("read back the bound address");
    tokio::spawn(async move {
        let _ =
            axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await;
    });
    (addr, store)
}

/// Read an audited endpoint over a real connection, optionally asserting an
/// identity, and return the status.
///
/// `/v1/api/logs` is audited and needs no fixture data, so a failure here is
/// about the connection and the identity, not about seeding.
async fn read_logs(addr: SocketAddr, asserted: Option<&str>) -> reqwest::StatusCode {
    let mut req = reqwest::Client::new()
        .get(format!("http://{addr}/v1/api/logs"))
        .header("authorization", format!("Bearer {READ_KEY}"));
    if let Some(subject) = asserted {
        req = req.header(HEADER, subject);
    }
    req.send().await.expect("the daemon answered").status()
}

/// The one audit row the request above left.
///
/// Asserting the row exists is what stops every subject assertion from passing
/// vacuously: an empty subject proves nothing if no row was written at all.
async fn only_audit_row(store: &SqliteStore) -> mcpd_core::AuditEvent {
    let rows = store
        .list_audit(&mcpd_store::AuditQuery {
            project: "default".to_owned(),
            limit: 100,
            ..Default::default()
        })
        .await
        .expect("read the audit log");
    assert_eq!(rows.len(), 1, "expected exactly one audited read, got {rows:#?}");
    rows.into_iter().next().expect("the row that was just counted")
}

// -------------------------------------------------------------- the tests ---

#[tokio::test]
async fn a_trusted_peer_asserting_a_subject_has_it_recorded() {
    let (addr, store) = serve(config(&[TRUSTED])).await;

    let status = read_logs(addr, Some(SUBJECT)).await;
    assert_eq!(status, reqwest::StatusCode::OK, "the read should have been served");

    let row = only_audit_row(&store).await;
    assert_eq!(
        row.subject, SUBJECT,
        "a peer inside {TRUSTED} sent {HEADER}, so the audit row should name the person. \
         An empty subject here means the peer address never reached the handler, which is \
         what happens when the router is served without into_make_service_with_connect_info."
    );
}

#[tokio::test]
async fn an_untrusted_peer_sending_the_same_header_is_not_believed() {
    let (addr, store) = serve(config(&[UNTRUSTED])).await;

    // The identical request as the test above. The only difference is that no
    // configured range covers the address it arrives from.
    let status = read_logs(addr, Some(SUBJECT)).await;
    assert_eq!(status, reqwest::StatusCode::OK, "the read should still have been served");

    let row = only_audit_row(&store).await;
    assert!(
        row.subject.is_empty(),
        "SECURITY FAILURE: a peer that no trusted range covers sent {HEADER} and mcpd wrote \
         {:?} into the audit log as fact. The only thing separating an assertion by your \
         ingress from an assertion by anyone who can open a socket to this port is the \
         address check, and it did not hold. Every subject in this deployment's audit log \
         is now unattributable.",
        row.subject
    );
}

#[tokio::test]
async fn the_feature_is_off_until_it_is_configured() {
    // No header name and no ranges: the shape every deployment has until an
    // operator names both halves.
    let (addr, store) = serve(config(&[])).await;

    let status = read_logs(addr, Some(SUBJECT)).await;
    assert_eq!(status, reqwest::StatusCode::OK, "the read should have been served");

    let row = only_audit_row(&store).await;
    assert!(
        row.subject.is_empty(),
        "with no identity header configured the feature is off, so {HEADER} must be ignored \
         however trusted the peer looks; the row recorded {:?}",
        row.subject
    );
}
