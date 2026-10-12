//! The loops the process owns stop when it stops, and are waited for.
//!
//! The monitor and aspect loops were spawned bare, so a stop dropped them
//! wherever they stood: a webhook post or a snapshot write in flight was cut
//! off, and `AppState::drain`, which is how `main` waits for background work,
//! did not know they existed.

use mcpd_server::{AppState, ServerConfig};
use mcpd_store::sqlite::SqliteStore;
use mcpd_store::Store;
use std::sync::Arc;
use std::time::Duration;

/// A private database file, named from the pid and a stamp taken once, because
/// a recycled pid would otherwise reopen an earlier run's file.
fn temp_db() -> std::path::PathBuf {
    static STAMP: std::sync::OnceLock<u128> = std::sync::OnceLock::new();
    let stamp = STAMP.get_or_init(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    });
    let dir =
        std::env::temp_dir().join(format!("mcpd-shutdown-test-{}-{stamp}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create the test directory");
    dir.join("0.db")
}

#[tokio::test]
async fn the_monitor_and_aspect_loops_are_tracked_and_stop_when_the_token_is_cancelled() {
    let store = Arc::new(SqliteStore::open(temp_db(), 2).expect("open the store"));
    store.migrate().await.expect("migrate the store");
    let config = ServerConfig {
        monitors: vec![serde_json::from_value(serde_json::json!({
            "id": "errors", "name": "errors", "project": "default",
            "watch": "error_rate", "compare": "above", "threshold": 0.1
        }))
        .expect("a monitor")],
        aspects: mcpd_server::aspects::AspectSettings {
            enabled: true,
            ..mcpd_server::aspects::AspectSettings::default()
        },
        ..ServerConfig::default()
    };
    let state = Arc::new(AppState::new(store as Arc<dyn Store>, config));
    assert_eq!(state.tasks.len(), 0);

    mcpd_server::monitor::spawn(Arc::clone(&state));
    assert_eq!(
        state.tasks.len(),
        1,
        "the monitor loop is not tracked, so a stop cannot wait for it"
    );
    mcpd_server::aspects::spawn(Arc::clone(&state));
    assert_eq!(
        state.tasks.len(),
        2,
        "the aspect loop is not tracked, so a stop cannot wait for it"
    );

    // Both loops sleep for minutes between passes. A loop that did not watch
    // the token would hold this for the whole bound and be counted unfinished.
    let unfinished = state.drain(Duration::from_secs(5)).await;
    assert_eq!(unfinished, 0, "a loop kept running after the token was cancelled");
}
