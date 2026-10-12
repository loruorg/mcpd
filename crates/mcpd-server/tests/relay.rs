//! Relay mode end to end: a relay and real daemons in one process.
//!
//! The daemons are the full router on a store of their own, bound to ports the
//! OS assigns. The relay ships to them over HTTP exactly as it would between
//! regions, so what is checked here is what an operator would see: the
//! request lands once, in the right place, through whichever upstream answers.

mod support;

use axum::http::StatusCode;
use mcpd_server::relay::{RelayConfig, RelayState};
use mcpd_server::{router, AppState, ServerConfig};
use mcpd_store::sqlite::SqliteStore;
use mcpd_store::Store;
use serde_json::json;
use std::sync::Arc;
use std::time::{Duration, Instant};
use support::{get, post, sample, temp_dir, Mock};

const WAIT: Duration = Duration::from_secs(10);

/// A daemon on its own store, serving on a port the OS assigns. With `path`
/// the store is that file, so two daemons can share one, as replicas do.
async fn daemon(path: Option<std::path::PathBuf>) -> (String, Arc<AppState>) {
    let path = path.unwrap_or_else(|| temp_dir("relay-db").join("mcpd.db"));
    let store = SqliteStore::open(path, 2).expect("open the store");
    store.migrate().await.expect("migrate");
    let state = Arc::new(AppState::new(Arc::new(store) as Arc<dyn Store>, ServerConfig::default()));
    let app = router(Arc::clone(&state));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await;
    });
    (format!("http://{addr}"), state)
}

fn relay(upstreams: Vec<String>, spool: Option<std::path::PathBuf>, max: u64) -> Arc<RelayState> {
    let state = RelayState::new(RelayConfig {
        upstreams,
        spool_dir: spool,
        spool_max_bytes: max,
        ingest_keys: vec!["edge-key".into()],
        ..RelayConfig::default()
    })
    .expect("a relay");
    let state = Arc::new(state);
    mcpd_server::relay::spawn(&state);
    state
}

fn batch(i: u32) -> Vec<u8> {
    json!({ "spans": [{
        "trace_id": format!("{i:032x}"),
        "span_id": format!("{i:016x}"),
        "name": "edge",
        "kind": "agent_run",
        "status": "ok",
        "start_time": "2026-01-01T00:00:00Z",
        "end_time": "2026-01-01T00:00:01Z",
        "is_root": true,
        "is_event": false,
        "correlation": { "project": "default" },
        "received_at": "2026-01-01T00:00:01Z"
    }] })
    .to_string()
    .into_bytes()
}

async fn send(relay: &Arc<RelayState>, i: u32) -> StatusCode {
    let app = mcpd_server::relay::router(Arc::clone(relay));
    post(&app, "/v1/ingest", "application/json", &[("authorization", "Bearer edge-key")], batch(i))
        .await
        .0
}

async fn stored(state: &AppState, i: u32) -> bool {
    state
        .store
        .get_trace("default", &format!("{i:032x}"), &mcpd_store::Include::default())
        .await
        .is_ok_and(|t| !t.spans.is_empty())
}

async fn wait_stored(state: &AppState, i: u32) {
    let started = Instant::now();
    while !stored(state, i).await {
        assert!(started.elapsed() < WAIT, "trace {i} never arrived upstream");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn a_request_is_spooled_answered_and_stored_upstream() {
    let (url, upstream) = daemon(None).await;
    let r = relay(vec![url], Some(temp_dir("relay-spool")), 1 << 20);
    assert_eq!(send(&r, 1).await, StatusCode::ACCEPTED);
    wait_stored(&upstream, 1).await;
    let m = r.render();
    assert_eq!(sample(&m, "mcpd_relay_accepted_total"), Some(1.0), "{m}");
}

#[tokio::test]
async fn a_request_without_the_relays_key_is_refused_before_it_is_spooled() {
    let r = relay(vec!["http://127.0.0.1:9".into()], None, 1 << 20);
    let app = mcpd_server::relay::router(Arc::clone(&r));
    let (status, _) = post(&app, "/v1/ingest", "application/json", &[], batch(1)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(r.spool.is_empty());
}

/// Ordered failover: with the primary down, the secondary takes the request,
/// and once the primary is back the next request goes to it again.
#[tokio::test]
async fn the_secondary_takes_over_while_the_primary_is_down_and_hands_back() {
    let primary = Mock::start(&[(503, None)]).await;
    let (secondary_url, secondary) = daemon(None).await;
    let r = relay(vec![primary.url.clone(), secondary_url], None, 1 << 20);

    send(&r, 1).await;
    wait_stored(&secondary, 1).await;
    assert_eq!(primary.seen().len(), 1, "the primary was tried first");
    assert_eq!(sample(&r.render(), "mcpd_relay_failovers_total"), Some(1.0));

    // The primary's cool-off is at most a second on its first failure.
    tokio::time::sleep(Duration::from_millis(1100)).await;
    send(&r, 2).await;
    primary.wait_accepted(1, WAIT).await;
    assert!(!stored(&secondary, 2).await, "the primary is preferred again once it answers");
}

/// At-least-once without double counting: the same request delivered to two
/// replicas sharing a store, as happens when the primary takes it and the
/// answer is lost, is stored once and the second delivery is a duplicate.
#[tokio::test]
async fn a_resend_to_another_replica_of_the_same_store_is_a_duplicate() {
    let db = temp_dir("replicas").join("shared.db");
    let (a, _) = daemon(Some(db.clone())).await;
    let (b, replica_b) = daemon(Some(db)).await;
    let key = [("idempotency-key", "relay-abc123")];
    let client = reqwest::Client::new();
    let mut answers = Vec::new();
    for url in [&a, &b] {
        let res = client
            .post(format!("{url}/v1/ingest"))
            .header("content-type", "application/json")
            .header(key[0].0, key[0].1)
            .body(batch(7))
            .send()
            .await
            .expect("an answer");
        answers.push(res.json::<serde_json::Value>().await.expect("json"));
    }
    assert_eq!(answers[0]["accepted"], 1, "{answers:?}");
    assert_eq!(answers[1]["accepted"], 0, "{answers:?}");
    assert_eq!(answers[1]["duplicate"], 1, "{answers:?}");
    let m = get(&router(Arc::clone(&replica_b)), "/metrics").await.1;
    assert_eq!(sample(&m, "mcpd_batches_duplicate_total"), Some(1.0), "{m}");
}

/// The relay's own key goes upstream when it has one, and the idempotency
/// key travels with every attempt.
#[tokio::test]
async fn every_attempt_carries_the_same_idempotency_key() {
    let up = Mock::start(&[(500, None)]).await;
    let state = Arc::new(
        RelayState::new(RelayConfig {
            upstreams: vec![up.url.clone()],
            upstream_key: Some("relay-upstream-key".into()),
            ..RelayConfig::default()
        })
        .expect("a relay"),
    );
    mcpd_server::relay::spawn(&state);
    let app = mcpd_server::relay::router(Arc::clone(&state));
    let (status, body) =
        post(&app, "/v1/traces", "application/json", &[], br#"{"resourceSpans":[]}"#.to_vec())
            .await;
    assert_eq!(status, StatusCode::OK, "an OTLP door answers 200");
    assert!(body.contains("partialSuccess"), "{body}");
    let seen = up.wait_seen(2, WAIT).await;
    let keys: Vec<_> =
        seen.iter().map(|s| s.header("idempotency-key").map(str::to_owned)).collect();
    assert!(keys[0].as_deref().is_some_and(|k| k.starts_with("relay-")), "{keys:?}");
    assert_eq!(keys[0], keys[1]);
    assert_eq!(seen[0].header("authorization"), Some("Bearer relay-upstream-key"));
    assert_eq!(seen[0].path, "/v1/traces");
}

/// Back-pressure: at the spool's bound the relay refuses with a 503 and a
/// Retry-After, and readiness says it should leave the load balancer.
#[tokio::test]
async fn a_full_spool_refuses_with_retry_after_and_reports_not_ready() {
    // Room for one request and most of a second, so the first leaves the spool
    // past the fill at which a relay with no upstream leaves the balancer.
    let r = relay(vec![format!("http://127.0.0.1:{}", support::dead_port())], None, 560);
    assert_eq!(send(&r, 1).await, StatusCode::ACCEPTED);
    let app = mcpd_server::relay::router(Arc::clone(&r));
    let mut refused = false;
    for i in 2..6 {
        let (status, _) = post(
            &app,
            "/v1/ingest",
            "application/json",
            &[("authorization", "Bearer edge-key")],
            batch(i),
        )
        .await;
        if status == StatusCode::SERVICE_UNAVAILABLE {
            refused = true;
            break;
        }
        assert_eq!(status, StatusCode::ACCEPTED);
    }
    assert!(refused, "the spool bound never refused");
    let (status, body) = get(&app, "/readyz").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
}

/// The spool is on disk: a relay stopped with work in it hands that work to
/// the next relay process.
#[tokio::test]
async fn a_relay_restarted_with_a_backlog_ships_it() {
    let spool = temp_dir("relay-restart");
    let dead = format!("http://127.0.0.1:{}", support::dead_port());
    {
        let r = relay(vec![dead], Some(spool.clone()), 1 << 20);
        send(&r, 3).await;
        send(&r, 4).await;
        assert_eq!(r.drain(Duration::from_secs(2)).await, 0);
        assert_eq!(r.spool.len(), 2);
    }
    let (url, upstream) = daemon(None).await;
    let _r = relay(vec![url], Some(spool), 1 << 20);
    wait_stored(&upstream, 3).await;
    wait_stored(&upstream, 4).await;
}
