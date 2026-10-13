//! The nine `Intelligence` endpoints, from the outside.
//!
//! These are written against the ways this surface can be wrong, in descending
//! order of how badly:
//!
//! 1. It reads as busy when nothing is wired. "No notes at all" and "notes are
//!    arriving and the floor has not been reached" look identical on a screen
//!    and mean different things, so a screen that cannot tell them apart sends
//!    an operator looking for a broken pipeline that was never switched on.
//! 2. It hands back a sentence a user wrote without recording that it did. A
//!    note is the shortest thing in this daemon that says what somebody asked
//!    for.
//! 3. It answers a question it was not asked: a flow whose stages are in the
//!    order the daemon felt like, a theme that a snapshot did not hold reported
//!    as one that never existed, examples in an order that changes on a reload.
//! 4. It reads a snapshot back differently from the way it was written.
//!
//! Requests go through `mcpd_server::router`, so the handler, the
//! authorisation and the SQL are under test together. The snapshot the theme
//! routes read is written through the `Store` trait rather than by running the
//! clustering pass: what the pass decides is a pure function tested in
//! `aspects.rs`, and what these tests are about is what the routes do with a
//! snapshot that exists.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use mcpd_core::{EntityKind, Theme, ThemeSnapshot, ThemeState};
use mcpd_server::{router, AppState, ServerConfig};
use mcpd_store::sqlite::SqliteStore;
use mcpd_store::{
    AuditQuery, SnapshotWrite, Store, StoredTheme, ThemeLineage, ThemeMember, NOISE_THEME_ID,
};
use serde_json::{json, Value};
use std::sync::Arc;
use time::{Duration, OffsetDateTime};
use tower::ServiceExt;

const AGENT: &str = "support";
const READ_KEY: &str = "read-key";
const INGEST_KEY: &str = "ingest-key";

/// A trace id for run `n` of the fixture.
///
/// Offset off zero, because an all-zero id is not a valid trace id and the
/// ingest door refuses one. A fixture that quietly lost its first run would
/// make every count here one short of what this file says it is.
fn run_id(n: usize) -> String {
    format!("{:032x}", n + 0xa1)
}

/// A span id for run `n`, offset for the same reason.
fn span_id(n: usize) -> String {
    format!("{:016x}", n + 0xb1)
}

/// The sentence note `n` carries. Six about a refund and four about a parcel,
/// so a clustering would find two groups; nothing here runs one.
fn sentence(n: usize) -> &'static str {
    if n < 6 {
        "the customer wants a refund for a late delivery"
    } else {
        "the customer is asking where their parcel has got to"
    }
}

/// A timestamp as the wire carries it.
///
/// `time`'s own `Serialize` writes a sequence of integers, which the spec's
/// types do not accept: every timestamp on the wire is RFC 3339. Rendering it
/// here keeps the fixture speaking the same language as a real client.
fn rfc3339(t: OffsetDateTime) -> String {
    t.format(&time::format_description::well_known::Rfc3339).expect("a timestamp")
}

fn temp_db() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    // The pid alone is not unique: pids are recycled, and a recycled one
    // reopens the previous run's databases, so every count here would be off by
    // a whole prior suite. A per-process stamp, taken once, makes the directory
    // this run's own. The same construction is in `tests/http.rs`.
    static STAMP: std::sync::OnceLock<u128> = std::sync::OnceLock::new();
    let stamp = STAMP.get_or_init(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    });
    let dir =
        std::env::temp_dir().join(format!("mcpd-aspects-test-{}-{stamp}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create the test directory");
    dir.join(format!("{}.db", N.fetch_add(1, Ordering::Relaxed)))
}

fn config() -> ServerConfig {
    ServerConfig {
        read_keys: vec![READ_KEY.to_owned()],
        ingest_keys: vec![INGEST_KEY.to_owned()],
        // Armed, because these tests are about a deployment that uses this
        // surface. It changes exactly one thing in the answers below,
        // `pass_armed` on the catalogue, and it starts nothing: the timer is
        // spawned by `aspects::spawn`, which only `mcpd serve` calls, so the
        // fixtures stay the only thing in the store and every pass here is one
        // a test asked for by name.
        aspects: armed(),
        derived_metrics: mcpd_server::derive::DerivedMetrics::off(),
        ..ServerConfig::default()
    }
}

/// The analysis switched on, which is what a deployment that reads these
/// routes has done.
fn armed() -> mcpd_server::aspects::AspectSettings {
    mcpd_server::aspects::AspectSettings {
        enabled: true,
        ..mcpd_server::aspects::AspectSettings::default()
    }
}

/// A router over an empty store.
async fn empty() -> (Router, Arc<SqliteStore>) {
    let store = Arc::new(SqliteStore::open(temp_db(), 2).expect("open the store"));
    store.migrate().await.expect("migrate the store");
    let app = router(Arc::new(AppState::new(Arc::clone(&store) as Arc<dyn Store>, config())));
    (app, store)
}

/// Ten runs of one agent, and `notes` notes about the first `notes` of them.
async fn seeded(notes: usize) -> (Router, Arc<SqliteStore>) {
    let (app, store) = empty().await;
    let at = OffsetDateTime::now_utc() - Duration::minutes(30);

    let spans: Vec<Value> = (0..10)
        .map(|i| {
            json!({
                "trace_id": run_id(i),
                "span_id": span_id(i),
                "name": "support",
                "kind": "agent_run",
                "status": "ok",
                "is_root": true,
                "is_event": false,
                "start_time": rfc3339(at),
                "end_time": rfc3339(at + Duration::seconds(1)),
                "entity_kind": "agent",
                "entity_name": AGENT,
                "received_at": rfc3339(at),
                "correlation": { "project": "default" },
            })
        })
        .collect();
    let res = send(&app, post("/v1/ingest", json!({ "spans": spans }).to_string())).await;
    assert_eq!(res.status, StatusCode::OK, "seeding the runs failed: {}", res.text());

    if notes > 0 {
        let rows: Vec<Value> = (0..notes)
            .map(|i| {
                json!({
                    "note_id": format!("n{i}"),
                    "trace_id": run_id(i),
                    "aspect": "goal",
                    "text": sentence(i),
                    "definition_version": 1,
                    "model": "a-model",
                    "created_at": rfc3339(at + Duration::seconds(i as i64)),
                    "correlation": { "project": "default" },
                })
            })
            .collect();
        let res = send(&app, post("/v1/ingest", json!({ "aspect_notes": rows }).to_string())).await;
        assert_eq!(res.status, StatusCode::OK, "seeding the notes failed: {}", res.text());
        assert_eq!(res.json()["accepted"], json!(notes), "a note was refused: {}", res.text());
    }
    (app, store)
}

/// One snapshot over the ten runs: two themes along `goal`, and one note in
/// noise. Written through the store, because what these tests are about is what
/// the routes do with a snapshot rather than how one is made.
async fn write_snapshot(
    store: &SqliteStore,
    snapshot_id: &str,
    cutoff: OffsetDateTime,
    ordinal: u64,
    themes: &[(&str, &str, usize, u64)],
) {
    let theme = |id: &str, label: &str, note: usize, count: u64| StoredTheme {
        theme: Theme {
            theme_id: id.to_owned(),
            snapshot_id: snapshot_id.to_owned(),
            aspect: "goal".to_owned(),
            entity_kind: EntityKind::Agent,
            entity_name: AGENT.to_owned(),
            label: label.to_owned(),
            label_note_id: format!("n{note}"),
            label_trace_id: run_id(note),
            state: ThemeState::Birth,
            note_count: count,
            trace_count: count,
            coverage: count as f64 / 10.0,
        },
        project: "default".to_owned(),
        cutoff_at: cutoff,
        window_from: cutoff - Duration::hours(1),
        window_to: cutoff,
        lineage: ThemeLineage::default(),
    };
    let member = |theme_id: &str, note: usize, similarity: u32| ThemeMember {
        snapshot_id: snapshot_id.to_owned(),
        project: "default".to_owned(),
        aspect: "goal".to_owned(),
        theme_id: theme_id.to_owned(),
        note_id: format!("n{note}"),
        trace_id: run_id(note),
        similarity,
        trace_started_at: cutoff - Duration::minutes(30),
    };

    let mut rows = Vec::new();
    let mut members = Vec::new();
    for (id, label, note, count) in themes {
        rows.push(theme(id, label, *note, *count));
        for k in 0..*count as usize {
            // Descending similarity with a tie in the middle, so the read has
            // to impose the documented order rather than return what it was
            // given.
            members.push(member(id, *note + k, 60 - (k as u32 / 2)));
        }
    }
    members.push(member(NOISE_THEME_ID, 9, 0));

    store
        .write_theme_snapshot(&SnapshotWrite {
            snapshot: ThemeSnapshot {
                snapshot_id: snapshot_id.to_owned(),
                project: "default".to_owned(),
                entity_kind: EntityKind::Agent,
                entity_name: AGENT.to_owned(),
                aspects: vec!["goal".to_owned()],
                ordinal,
                cutoff_at: cutoff,
                window_from: cutoff - Duration::hours(1),
                window_to: cutoff,
                trace_count: 10,
                note_count: 10,
                capped: false,
            },
            themes: rows,
            members,
        })
        .await
        .expect("a snapshot must write");
}

// ---------------------------------------------------------- the catalogue ---

#[tokio::test]
async fn the_catalogue_answers_before_anything_has_been_recorded() {
    let (app, _) = empty().await;
    let res = get(&app, "/v1/api/aspects?project=default").await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    let body = res.json();

    let aspects = body["aspects"].as_array().expect("a list of aspects").clone();
    assert_eq!(aspects.len(), 4, "four aspects ship: {}", res.text());
    let names: Vec<&str> = aspects.iter().map(|a| a["name"].as_str().unwrap_or("")).collect();
    assert_eq!(names, vec!["goal", "sentiment", "behaviour", "outcome"]);

    // The one aspect the daemon fills itself, and the field that says a model
    // is not involved.
    let outcome = aspects.iter().find(|a| a["name"] == json!("outcome")).expect("outcome");
    assert_eq!(outcome["derived"], json!(true));
    assert!(outcome.get("instruction").is_none(), "a derived aspect needs no instruction");

    // The bounds are published because each one changes what a reader is
    // looking at.
    assert_eq!(body["limits"]["min_notes_to_cluster"], json!(50));
    assert_eq!(body["limits"]["max_notes_per_pass"], json!(20_000));
    assert_eq!(body["limits"]["page_size"], json!(500));
    assert_eq!(body["limits"]["pass_armed"], json!(true));
}

#[tokio::test]
async fn a_daemon_with_the_pass_off_says_so_rather_than_looking_broken() {
    // The defect this field exists for. With nothing configured, every aspect
    // is still on, every readiness sits below `ready` for ever, and without
    // `pass_armed` nothing in the API says why. An operator reading the
    // catalogue to ask "is this on" was told yes.
    let store = Arc::new(SqliteStore::open(temp_db(), 2).expect("open the store"));
    store.migrate().await.expect("migrate the store");
    let unarmed =
        ServerConfig { aspects: mcpd_server::aspects::AspectSettings::default(), ..config() };
    let app = router(Arc::new(AppState::new(Arc::clone(&store) as Arc<dyn Store>, unarmed)));

    let res = get(&app, "/v1/api/aspects?project=default").await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    let body = res.json();
    assert_eq!(
        body["limits"]["pass_armed"],
        json!(false),
        "nothing clusters here and the catalogue has to say so: {}",
        res.text()
    );
    // And the aspects are still on, because a note along one is still
    // accepted, stored and handed back. The two are separate facts.
    assert!(
        body["aspects"].as_array().expect("aspects").iter().all(|a| a["enabled"] == json!(true)),
        "{}",
        res.text()
    );
}

// ------------------------------------------------------- the empty answers ---

#[tokio::test]
async fn nothing_wired_is_an_empty_list_and_never_a_row_that_says_collecting() {
    // The distinction this whole surface exists to make. An entity nothing has
    // written about has no row at all; `collecting` is a row with a count on it.
    let (app, _) = seeded(0).await;
    let res = get(&app, "/v1/api/aspects/entities?project=default").await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    assert_eq!(res.json()["entities"].as_array().map(Vec::len), Some(0), "{}", res.text());

    let res = get(
        &app,
        &format!("/v1/api/aspects/progress?project=default&entity_kind=agent&entity_name={AGENT}"),
    )
    .await;
    assert_eq!(
        res.status,
        StatusCode::NOT_FOUND,
        "an entity nothing has written about is a 404 and not a zeroed row: {}",
        res.text()
    );
}

#[tokio::test]
async fn a_row_appears_on_the_first_note_and_reads_collecting() {
    let (app, _) = seeded(3).await;
    let res = get(&app, "/v1/api/aspects/entities?project=default").await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    let rows = res.json()["entities"].as_array().expect("rows").clone();
    assert_eq!(rows.len(), 1, "a row appears as soon as the first note is recorded");
    assert_eq!(rows[0]["entity_name"], json!(AGENT));
    assert_eq!(rows[0]["entity_kind"], json!("agent"));
    assert_eq!(rows[0]["note_count"], json!(3));
    assert_eq!(rows[0]["trace_count"], json!(3));
    assert_eq!(rows[0]["ready_aspects"], json!(0));
    assert_eq!(rows[0]["enabled_aspects"], json!(4));
    assert_eq!(
        rows[0]["readiness"],
        json!("collecting"),
        "three notes is under the floor, so it is still collecting"
    );
    assert!(
        rows[0].get("latest_snapshot_at").is_none(),
        "no pass has run, so there is no snapshot to name"
    );
}

#[tokio::test]
async fn progress_turns_the_wait_into_a_number_to_watch() {
    let (app, _) = seeded(3).await;
    let res = get(
        &app,
        &format!("/v1/api/aspects/progress?project=default&entity_kind=agent&entity_name={AGENT}"),
    )
    .await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    let body = res.json();
    assert_eq!(body["readiness"], json!("collecting"));
    assert_eq!(body["trace_count"], json!(3));

    let rows = body["aspects"].as_array().expect("one row per enabled aspect").clone();
    assert_eq!(rows.len(), 4, "one row per enabled aspect, in catalogue order");
    let goal = rows.iter().find(|r| r["aspect"] == json!("goal")).expect("goal");
    assert_eq!(goal["note_count"], json!(3));
    assert_eq!(goal["notes_needed"], json!(47), "50 - 3, which is the number a reader watches");
    assert_eq!(goal["readiness"], json!("collecting"));

    // An aspect nothing has written about at all is still a row, with the whole
    // floor left to go. Leaving it out would make three of the four columns
    // vanish rather than say they are waiting.
    let sentiment = rows.iter().find(|r| r["aspect"] == json!("sentiment")).expect("sentiment");
    assert_eq!(sentiment["note_count"], json!(0));
    assert_eq!(sentiment["notes_needed"], json!(50));
    assert!(sentiment.get("last_note_at").is_none());
    assert!(body.get("latest_snapshot").is_none());
}

#[tokio::test]
async fn a_snapshot_makes_the_aspect_it_covers_read_ready() {
    let (app, store) = seeded(10).await;
    let now = OffsetDateTime::now_utc();
    write_snapshot(&store, "snap-1", now, 1, &[("t-refund", "refund", 0, 3)]).await;

    let res = get(
        &app,
        &format!("/v1/api/aspects/progress?project=default&entity_kind=agent&entity_name={AGENT}"),
    )
    .await;
    let body = res.json();
    let goal = body["aspects"]
        .as_array()
        .expect("rows")
        .iter()
        .find(|r| r["aspect"] == json!("goal"))
        .expect("goal")
        .clone();
    assert_eq!(goal["readiness"], json!("ready"), "a snapshot covers it: {}", res.text());
    assert_eq!(
        body["readiness"],
        json!("collecting"),
        "the entity is the least advanced of its aspects and never the best case"
    );
    assert_eq!(body["latest_snapshot"]["snapshot_id"], json!("snap-1"));
}

// --------------------------------------------------------- one trace's notes ---

#[tokio::test]
async fn one_traces_notes_come_back_in_catalogue_order() {
    let (app, _) = seeded(3).await;
    // A second note about run 0 along a later aspect, under an id of its own: a
    // note id identifies the sentence and never the run.
    let res = send(
        &app,
        post(
            "/v1/ingest",
            json!({ "aspect_notes": [{
                "note_id": "n0-behaviour",
                "trace_id": run_id(0),
                "aspect": "behaviour",
                "text": "the agent looked the order up and apologised",
                "definition_version": 1,
                "created_at": rfc3339(OffsetDateTime::now_utc()),
                "correlation": { "project": "default" },
            }]})
            .to_string(),
        ),
    )
    .await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());

    let res = get(&app, &format!("/v1/api/traces/{}/aspects", run_id(0))).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    let body = res.json();
    assert_eq!(body["trace_id"], json!(run_id(0)));
    let aspects: Vec<&str> = body["notes"]
        .as_array()
        .expect("notes")
        .iter()
        .map(|n| n["aspect"].as_str().unwrap_or(""))
        .collect();
    assert_eq!(
        aspects,
        vec!["goal", "behaviour"],
        "catalogue order, not the order a worker happened to write them in: {}",
        res.text()
    );
}

/// Two projects that call each other share a trace id, and each can have
/// notes about its own run under it. `project` picks one; the walk without it
/// stops at the first project that holds notes.
#[tokio::test]
async fn a_trace_id_two_projects_share_is_read_per_project() {
    let (app, _) = seeded(1).await;
    let at = OffsetDateTime::now_utc() - Duration::minutes(10);
    let body = json!({
        "spans": [{
            "trace_id": run_id(0),
            "span_id": "bbbbbbbbbbbbbbb1",
            "parent_span_id": span_id(0),
            "name": "billing",
            "kind": "tool_call",
            "status": "ok",
            "is_root": false,
            "is_event": false,
            "start_time": rfc3339(at),
            "end_time": rfc3339(at + Duration::seconds(1)),
            "received_at": rfc3339(at),
            "correlation": { "project": "billing" },
        }],
        "aspect_notes": [{
            "note_id": "billing-n0",
            "trace_id": run_id(0),
            "aspect": "goal",
            "text": "the caller wanted an invoice reissued",
            "definition_version": 1,
            "created_at": rfc3339(at),
            "correlation": { "project": "billing" },
        }],
    });
    let res = send(&app, post("/v1/ingest", body.to_string())).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());

    let text = |res: &Res| -> Vec<String> {
        res.json()["notes"]
            .as_array()
            .expect("notes")
            .iter()
            .map(|n| n["text"].as_str().unwrap_or("").to_owned())
            .collect()
    };
    let billing = get(&app, &format!("/v1/api/traces/{}/aspects?project=billing", run_id(0))).await;
    assert_eq!(billing.status, StatusCode::OK, "{}", billing.text());
    assert_eq!(text(&billing), vec!["the caller wanted an invoice reissued"]);

    let default = get(&app, &format!("/v1/api/traces/{}/aspects?project=default", run_id(0))).await;
    assert_eq!(default.status, StatusCode::OK, "{}", default.text());
    assert_eq!(text(&default), vec![sentence(0).to_owned()]);

    let unknown = get(&app, &format!("/v1/api/traces/{}/aspects?projekt=x", run_id(0))).await;
    assert_eq!(unknown.status, StatusCode::BAD_REQUEST, "{}", unknown.text());
}

#[tokio::test]
async fn a_trace_with_no_notes_is_an_empty_list_and_an_unknown_one_is_a_404() {
    let (app, _) = seeded(3).await;
    // Run 7 exists and nothing has written about it. That is the ordinary state
    // of a recent trace, and it is not a 404.
    let res = get(&app, &format!("/v1/api/traces/{}/aspects", run_id(7))).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    assert_eq!(res.json()["notes"].as_array().map(Vec::len), Some(0));

    let res = get(&app, "/v1/api/traces/ffffffffffffffffffffffffffffffff/aspects").await;
    assert_eq!(res.status, StatusCode::NOT_FOUND, "{}", res.text());
}

#[tokio::test]
async fn reading_a_note_is_recorded_in_the_audit_ledger() {
    // A note is a sentence about what a user asked for, so handing one back is
    // an access to a payload and is recorded like every other.
    let (app, store) = seeded(3).await;
    let before = store
        .list_audit(&AuditQuery { project: "default".to_owned(), limit: 100, ..Default::default() })
        .await
        .expect("the ledger");
    let res = get(&app, &format!("/v1/api/traces/{}/aspects", run_id(0))).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    let after = store
        .list_audit(&AuditQuery { project: "default".to_owned(), limit: 100, ..Default::default() })
        .await
        .expect("the ledger");
    assert_eq!(after.len(), before.len() + 1, "the read was not recorded");
    assert_eq!(after[0].action, "read_trace_aspect_notes");
    assert_eq!(after[0].resource.as_deref(), Some(run_id(0).as_str()));
}

// ------------------------------------------------------------- the themes ---

#[tokio::test]
async fn a_snapshot_series_comes_back_oldest_first() {
    let (app, store) = seeded(10).await;
    let now = OffsetDateTime::now_utc();
    for (i, id) in ["snap-1", "snap-2", "snap-3"].iter().enumerate() {
        write_snapshot(
            &store,
            id,
            now - Duration::hours(3 - i as i64),
            i as u64 + 1,
            &[("t-refund", "refund", 0, 3)],
        )
        .await;
    }

    let res = get(
        &app,
        &format!("/v1/api/themes/snapshots?project=default&entity_kind=agent&entity_name={AGENT}"),
    )
    .await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    let body = res.json();
    let ids: Vec<&str> = body["snapshots"]
        .as_array()
        .expect("snapshots")
        .iter()
        .map(|s| s["snapshot"]["snapshot_id"].as_str().unwrap_or(""))
        .collect();
    assert_eq!(ids, vec!["snap-1", "snap-2", "snap-3"], "oldest first: {}", res.text());
    assert_eq!(body["total"], json!(3));
    assert!(
        body["snapshots"][0].get("reason").is_none(),
        "an inventory chooses nothing, so it reports no reason"
    );

    // A landmark selection always holds both ends and says why each row is in
    // it, which is what stops a timeline being read as the whole story.
    let res = get(
        &app,
        &format!(
            "/v1/api/themes/snapshots?project=default&entity_kind=agent&entity_name={AGENT}\
             &presentation=landmarks&limit=2"
        ),
    )
    .await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    let body = res.json();
    let reasons: Vec<&str> = body["snapshots"]
        .as_array()
        .expect("snapshots")
        .iter()
        .map(|s| s["reason"].as_str().unwrap_or(""))
        .collect();
    assert_eq!(reasons, vec!["range_start", "range_end"]);
    assert_eq!(body["total"], json!(3), "the selection says what it stands in for");
}

#[tokio::test]
async fn a_theme_absent_from_a_snapshot_is_an_answer_and_a_theme_that_never_existed_is_a_404() {
    let (app, store) = seeded(10).await;
    let now = OffsetDateTime::now_utc();
    write_snapshot(&store, "snap-1", now - Duration::hours(1), 1, &[("t-refund", "refund", 0, 3)])
        .await;
    write_snapshot(
        &store,
        "snap-2",
        now,
        2,
        &[("t-refund", "refund", 0, 3), ("t-parcel", "parcel", 6, 2)],
    )
    .await;

    let url = |theme: &str, snapshot: &str| {
        format!(
            "/v1/api/themes/{theme}?project=default&entity_kind=agent&entity_name={AGENT}\
             &aspect=goal&snapshot_id={snapshot}"
        )
    };

    let res = get(&app, &url("t-parcel", "snap-2")).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    assert_eq!(res.json()["theme"]["label"], json!("parcel"));
    assert_eq!(res.json()["snapshot"]["note_count"], json!(10), "the window's own counts too");

    // Scrubbing back past a theme's birth is a reasonable question, and the
    // answer is that the theme was not there yet.
    let res = get(&app, &url("t-parcel", "snap-1")).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    assert!(res.json().get("theme").is_none(), "absent is an answer, not an error: {}", res.text());

    // A theme id this entity has never had is a different mistake.
    let res = get(&app, &url("t-nothing", "snap-2")).await;
    assert_eq!(res.status, StatusCode::NOT_FOUND, "{}", res.text());
}

#[tokio::test]
async fn examples_come_back_most_alike_the_label_first_and_noise_uses_the_same_route() {
    let (app, store) = seeded(10).await;
    let now = OffsetDateTime::now_utc();
    write_snapshot(&store, "snap-1", now, 1, &[("t-refund", "refund", 0, 3)]).await;

    let res = get(
        &app,
        &format!(
            "/v1/api/themes/t-refund/examples?project=default&entity_kind=agent\
             &entity_name={AGENT}&aspect=goal&snapshot_id=snap-1"
        ),
    )
    .await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    let examples = res.json()["examples"].as_array().expect("examples").clone();
    assert_eq!(examples.len(), 3);
    let scores: Vec<u64> = examples.iter().map(|e| e["similarity"].as_u64().unwrap_or(0)).collect();
    assert!(scores.windows(2).all(|w| w[0] >= w[1]), "most alike first, got {scores:?}");
    // The tie in the middle is broken on the note id ascending, so the order is
    // total and the same on every read and every replica.
    assert_eq!(
        examples.iter().map(|e| e["note_id"].as_str().unwrap_or("")).collect::<Vec<_>>(),
        vec!["n0", "n1", "n2"]
    );
    assert_eq!(examples[0]["text"], json!(sentence(0)), "the sentence itself, after redaction");

    // Noise is a node in the flow and its notes are read here, because a second
    // pair of routes returning an identically shaped answer would be two more
    // rows in the reference and no new question answered.
    let res = get(
        &app,
        &format!(
            "/v1/api/themes/noise/examples?project=default&entity_kind=agent\
             &entity_name={AGENT}&aspect=goal&snapshot_id=snap-1"
        ),
    )
    .await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    assert_eq!(res.json()["examples"].as_array().map(Vec::len), Some(1));
}

#[tokio::test]
async fn a_history_is_one_point_per_snapshot_oldest_first() {
    let (app, store) = seeded(10).await;
    let now = OffsetDateTime::now_utc();
    for (i, id) in ["snap-1", "snap-2"].iter().enumerate() {
        write_snapshot(
            &store,
            id,
            now - Duration::hours(2 - i as i64),
            i as u64 + 1,
            &[("t-refund", "refund", 0, 3)],
        )
        .await;
    }
    let res = get(
        &app,
        &format!(
            "/v1/api/themes/t-refund/history?project=default&entity_kind=agent\
             &entity_name={AGENT}&aspect=goal"
        ),
    )
    .await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    let body = res.json();
    let ids: Vec<&str> = body["points"]
        .as_array()
        .expect("points")
        .iter()
        .map(|p| p["snapshot_id"].as_str().unwrap_or(""))
        .collect();
    assert_eq!(ids, vec!["snap-1", "snap-2"], "oldest first: {}", res.text());
    assert_eq!(body["label"], json!("refund"), "the label at the newest point");

    let res = get(
        &app,
        &format!(
            "/v1/api/themes/t-nothing/history?project=default&entity_kind=agent\
             &entity_name={AGENT}&aspect=goal"
        ),
    )
    .await;
    assert_eq!(res.status, StatusCode::NOT_FOUND, "{}", res.text());
}

#[tokio::test]
async fn a_flow_of_fewer_than_two_stages_is_refused_by_name() {
    let (app, store) = seeded(10).await;
    write_snapshot(&store, "snap-1", OffsetDateTime::now_utc(), 1, &[("t-refund", "refund", 0, 3)])
        .await;
    let res = get(
        &app,
        &format!(
            "/v1/api/themes/flow?project=default&entity_kind=agent&entity_name={AGENT}\
             &snapshot_id=snap-1&aspects=goal"
        ),
    )
    .await;
    assert_eq!(
        res.status,
        StatusCode::BAD_REQUEST,
        "one stage has no links in it, so it is refused rather than answered empty: {}",
        res.text()
    );
    assert!(res.text().contains("two to four stages"), "{}", res.text());
}

#[tokio::test]
async fn a_flow_stage_accounts_for_every_trace_in_it() {
    // The property the shares rest on: a stage's themes, its noise and its
    // overflow together account for every trace, so the coverages add to one.
    let (app, store) = seeded(10).await;
    let now = OffsetDateTime::now_utc();

    // One snapshot covering two aspects, written once. Writing the same
    // snapshot id twice would replace it rather than add to it, which is what
    // makes a rerun of a pass correct and would make a fixture built in two
    // calls silently lose its first half.
    let theme = |aspect: &str, id: &str, label: &str, note: usize, count: u64| StoredTheme {
        theme: Theme {
            theme_id: id.to_owned(),
            snapshot_id: "snap-1".to_owned(),
            aspect: aspect.to_owned(),
            entity_kind: EntityKind::Agent,
            entity_name: AGENT.to_owned(),
            label: label.to_owned(),
            label_note_id: format!("n{note}"),
            label_trace_id: run_id(note),
            state: ThemeState::Birth,
            note_count: count,
            trace_count: count,
            coverage: count as f64 / 10.0,
        },
        project: "default".to_owned(),
        cutoff_at: now,
        window_from: now - Duration::hours(1),
        window_to: now,
        lineage: ThemeLineage::default(),
    };
    let member = |aspect: &str, theme_id: &str, note: usize| ThemeMember {
        snapshot_id: "snap-1".to_owned(),
        project: "default".to_owned(),
        aspect: aspect.to_owned(),
        theme_id: theme_id.to_owned(),
        note_id: format!("n{note}"),
        trace_id: run_id(note),
        similarity: 64,
        trace_started_at: now,
    };

    let mut members: Vec<ThemeMember> = Vec::new();
    // goal: three runs in one theme, two in another, one in noise.
    for i in 0..3 {
        members.push(member("goal", "t-refund", i));
    }
    for i in 3..5 {
        members.push(member("goal", "t-parcel", i));
    }
    members.push(member("goal", NOISE_THEME_ID, 5));
    // outcome: the first three runs again, so the two stages overlap and there
    // is something for a link to join.
    for i in 0..3 {
        members.push(member("outcome", "t-ok", i));
    }

    store
        .write_theme_snapshot(&SnapshotWrite {
            snapshot: ThemeSnapshot {
                snapshot_id: "snap-1".to_owned(),
                project: "default".to_owned(),
                entity_kind: EntityKind::Agent,
                entity_name: AGENT.to_owned(),
                aspects: vec!["goal".to_owned(), "outcome".to_owned()],
                ordinal: 1,
                cutoff_at: now,
                window_from: now - Duration::hours(1),
                window_to: now,
                trace_count: 10,
                note_count: 10,
                capped: false,
            },
            themes: vec![
                theme("goal", "t-refund", "refund", 0, 3),
                theme("goal", "t-parcel", "parcel", 3, 2),
                theme("outcome", "t-ok", "the run finished with no error", 0, 3),
            ],
            members,
        })
        .await
        .expect("the snapshot must write");

    let res = get(
        &app,
        &format!(
            "/v1/api/themes/flow?project=default&entity_kind=agent&entity_name={AGENT}\
             &snapshot_id=snap-1&aspects=goal,outcome"
        ),
    )
    .await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    let body = res.json();
    let stages = body["stages"].as_array().expect("stages").clone();
    assert_eq!(stages.len(), 2, "one stage per aspect, in the order they were asked for");
    assert_eq!(stages[0]["aspect"], json!("goal"));
    assert_eq!(stages[1]["aspect"], json!("outcome"));
    assert_eq!(stages[0]["trace_count"], json!(6), "five in themes and one in noise");

    for stage in &stages {
        let nodes = stage["nodes"].as_array().expect("nodes").clone();
        let total: u64 = nodes.iter().map(|n| n["trace_count"].as_u64().unwrap_or(0)).sum();
        assert_eq!(
            total,
            stage["trace_count"].as_u64().unwrap_or(0),
            "a stage's nodes have to account for every trace in it: {}",
            res.text()
        );
        let shares: f64 = nodes.iter().map(|n| n["coverage"].as_f64().unwrap_or(0.0)).sum();
        assert!((shares - 1.0).abs() < 1e-9, "the shares add to one, got {shares}");
    }

    // Noise is a node of the stage and never a theme: it carries no theme id,
    // which is what makes it impossible to drill into by accident.
    let noise = stages[0]["nodes"]
        .as_array()
        .expect("nodes")
        .iter()
        .find(|n| n["kind"] == json!("noise"))
        .expect("a noise node");
    assert!(noise.get("theme_id").is_none());
    assert_eq!(noise["trace_count"], json!(1));

    // Links join neighbouring stages only, which is what makes the order of
    // `aspects` a question rather than a display preference.
    let links = body["links"].as_array().expect("links").clone();
    assert!(!links.is_empty(), "the two stages describe overlapping traces: {}", res.text());
    for link in &links {
        assert!(link["source_node_id"].as_str().unwrap_or("").starts_with("goal:"));
        assert!(link["target_node_id"].as_str().unwrap_or("").starts_with("outcome:"));
    }
    let joined =
        links.iter().find(|l| l["source_node_id"] == json!("goal:t-refund")).expect("a ribbon");
    assert_eq!(joined["trace_count"], json!(3));
    assert_eq!(joined["source_share"], json!(1.0));
    assert_eq!(joined["target_share"], json!(1.0));

    // Drilling in recomputes the counts over the traces that survive, so the
    // answer does not change shape with the size of the snapshot.
    let res = get(
        &app,
        &format!(
            "/v1/api/themes/flow?project=default&entity_kind=agent&entity_name={AGENT}\
             &snapshot_id=snap-1&aspects=goal,outcome&filter=outcome:t-ok"
        ),
    )
    .await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    let stages = res.json()["stages"].as_array().expect("stages").clone();
    assert_eq!(stages[0]["trace_count"], json!(3), "only the runs the filter kept: {}", res.text());
}

// ------------------------------------------------------------ the pass ---
//
// One forced pass, end to end: notes in, a snapshot out, and the routes above
// reading what it wrote. `run_once` is the same function the timer calls.

/// A router, a store and the state the pass runs against.
async fn with_state(notes: usize) -> (Router, Arc<SqliteStore>, Arc<AppState>) {
    let store = Arc::new(SqliteStore::open(temp_db(), 2).expect("open the store"));
    store.migrate().await.expect("migrate the store");
    let state = Arc::new(AppState::new(Arc::clone(&store) as Arc<dyn Store>, config()));
    let app = router(Arc::clone(&state));
    seed_into(&app, notes).await;
    (app, store, state)
}

/// `runs` runs of one agent, and `notes` notes about the first of them.
async fn seed_into(app: &Router, notes: usize) {
    let at = OffsetDateTime::now_utc() - Duration::minutes(30);
    let runs = notes.max(1);
    let spans: Vec<Value> = (0..runs)
        .map(|i| {
            json!({
                "trace_id": run_id(i),
                "span_id": span_id(i),
                "name": "support",
                "kind": "agent_run",
                "status": if i % 7 == 0 { "error" } else { "ok" },
                "is_root": true,
                "is_event": false,
                "start_time": rfc3339(at),
                "end_time": rfc3339(at + Duration::seconds(1)),
                "entity_kind": "agent",
                "entity_name": AGENT,
                "received_at": rfc3339(at),
                "correlation": { "project": "default" },
            })
        })
        .collect();
    // In batches, because the daemon's own signal ceiling applies to a request
    // and this fixture is larger than the pages it is read back in.
    for chunk in spans.chunks(200) {
        let res = send(app, post("/v1/ingest", json!({ "spans": chunk }).to_string())).await;
        assert_eq!(res.status, StatusCode::OK, "seeding the runs failed: {}", res.text());
    }
    if notes == 0 {
        return;
    }
    let rows: Vec<Value> = (0..notes)
        .map(|i| {
            json!({
                "note_id": format!("n{i}"),
                "trace_id": run_id(i),
                "aspect": "goal",
                "text": sentence(i % 10),
                "definition_version": 1,
                "model": "a-model",
                "created_at": rfc3339(at + Duration::seconds(i as i64)),
                "correlation": { "project": "default" },
            })
        })
        .collect();
    for chunk in rows.chunks(200) {
        let res = send(app, post("/v1/ingest", json!({ "aspect_notes": chunk }).to_string())).await;
        assert_eq!(res.status, StatusCode::OK, "seeding the notes failed: {}", res.text());
    }
}

#[tokio::test]
async fn a_pass_below_the_floor_writes_nothing_at_all() {
    // The floor is not a rounding: below it the daemon writes no snapshot,
    // rather than sorting a handful of sentences into groups nobody should draw
    // a conclusion from.
    let (app, _, state) = with_state(49).await;
    mcpd_server::aspects::run_once(&state, OffsetDateTime::now_utc()).await.expect("a pass");

    let res = get(
        &app,
        &format!("/v1/api/themes/snapshots?project=default&entity_kind=agent&entity_name={AGENT}"),
    )
    .await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    assert_eq!(res.json()["total"], json!(0), "no snapshot below the floor: {}", res.text());

    let res = get(
        &app,
        &format!("/v1/api/aspects/progress?project=default&entity_kind=agent&entity_name={AGENT}"),
    )
    .await;
    let goal = res.json()["aspects"]
        .as_array()
        .expect("rows")
        .iter()
        .find(|r| r["aspect"] == json!("goal"))
        .expect("goal")
        .clone();
    assert_eq!(goal["readiness"], json!("collecting"), "{}", res.text());
    assert_eq!(goal["notes_needed"], json!(1), "one more note and a pass will run");
}

#[tokio::test]
async fn a_pass_over_the_floor_writes_a_snapshot_the_routes_can_read() {
    let (app, _, state) = with_state(60).await;
    let now = OffsetDateTime::now_utc();
    mcpd_server::aspects::run_once(&state, now).await.expect("a pass");

    // The snapshot, and the aspect it covers.
    let res = get(
        &app,
        &format!("/v1/api/themes/snapshots?project=default&entity_kind=agent&entity_name={AGENT}"),
    )
    .await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    let snapshots = res.json()["snapshots"].as_array().expect("snapshots").clone();
    assert_eq!(snapshots.len(), 1, "one pass writes one snapshot: {}", res.text());
    let snapshot = snapshots[0]["snapshot"].clone();
    assert_eq!(snapshot["aspects"], json!(["goal"]));
    assert_eq!(snapshot["note_count"], json!(60));
    assert_eq!(snapshot["capped"], json!(false));
    let snapshot_id = snapshot["snapshot_id"].as_str().expect("an id").to_owned();

    // The clustering. Six sentences repeat and four repeat, so the wording
    // makes two groups.
    let res = get(
        &app,
        &format!("/v1/api/aspects/progress?project=default&entity_kind=agent&entity_name={AGENT}"),
    )
    .await;
    let goal = res.json()["aspects"]
        .as_array()
        .expect("rows")
        .iter()
        .find(|r| r["aspect"] == json!("goal"))
        .expect("goal")
        .clone();
    assert_eq!(goal["readiness"], json!("ready"), "a snapshot covers it: {}", res.text());

    // A theme's label is one of its own notes, never a summary written for the
    // screen, so it has to be one of the two sentences the fixture wrote.
    let res = get(
        &app,
        &format!(
            "/v1/api/themes/flow?project=default&entity_kind=agent&entity_name={AGENT}\
             &snapshot_id={snapshot_id}&aspects=goal,goal"
        ),
    )
    .await;
    assert_eq!(res.status, StatusCode::OK, "a flow of one aspect twice: {}", res.text());
    let nodes = res.json()["stages"][0]["nodes"].as_array().expect("nodes").clone();
    let themes: Vec<&Value> = nodes.iter().filter(|n| n["kind"] == json!("theme")).collect();
    assert_eq!(themes.len(), 2, "two wordings, two themes: {}", res.text());
    for t in &themes {
        let label = t["label"].as_str().unwrap_or("");
        assert!(
            label == sentence(0) || label == sentence(6),
            "a label has to be one of the theme's own notes, got {label:?}"
        );
    }
    // Six of every ten notes carry the first sentence and four the second, so
    // the larger theme comes first and the two account for every trace.
    assert_eq!(themes[0]["trace_count"], json!(36));
    assert_eq!(themes[1]["trace_count"], json!(24));
}

#[tokio::test]
async fn a_second_pass_keeps_a_theme_that_is_holding_together() {
    // The property a history rests on: a theme keeps its id while its wording
    // holds, so the same id in two snapshots is the same theme.
    let (app, _, state) = with_state(60).await;
    let now = OffsetDateTime::now_utc();
    mcpd_server::aspects::run_once(&state, now).await.expect("the first pass");
    mcpd_server::aspects::run_once(&state, now + Duration::minutes(5)).await.expect("the second");

    let res = get(
        &app,
        &format!("/v1/api/themes/snapshots?project=default&entity_kind=agent&entity_name={AGENT}"),
    )
    .await;
    let snapshots = res.json()["snapshots"].as_array().expect("snapshots").clone();
    assert_eq!(snapshots.len(), 2, "two passes, two snapshots: {}", res.text());

    let ids_of = |snapshot: &str| {
        let app = app.clone();
        let url = format!(
            "/v1/api/themes/flow?project=default&entity_kind=agent&entity_name={AGENT}\
             &snapshot_id={snapshot}&aspects=goal,goal"
        );
        async move {
            let res = get(&app, &url).await;
            let mut ids: Vec<String> = res.json()["stages"][0]["nodes"]
                .as_array()
                .expect("nodes")
                .iter()
                .filter_map(|n| n["theme_id"].as_str().map(str::to_owned))
                .collect();
            ids.sort();
            ids
        }
    };
    let first = ids_of(snapshots[0]["snapshot"]["snapshot_id"].as_str().expect("an id")).await;
    let second = ids_of(snapshots[1]["snapshot"]["snapshot_id"].as_str().expect("an id")).await;
    assert_eq!(first, second, "the same wording is the same theme, under the same id");

    // And the history reads as one theme across both, oldest first.
    let res = get(
        &app,
        &format!(
            "/v1/api/themes/{}/history?project=default&entity_kind=agent&entity_name={AGENT}\
             &aspect=goal",
            first[0]
        ),
    )
    .await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    let points = res.json()["points"].as_array().expect("points").clone();
    assert_eq!(points.len(), 2, "{}", res.text());
    assert_eq!(points[0]["state"], json!("birth"));
    assert_eq!(points[1]["state"], json!("continue"));
}

#[tokio::test]
async fn the_daemon_fills_the_outcome_aspect_with_no_model_anywhere() {
    // The claim that makes a themes screen work before anything is wired: the
    // daemon writes the `outcome` notes itself from facts it already holds.
    // Nothing in this test posts a note.
    let (app, _, state) = with_state(0).await;
    seed_into(&app, 0).await;
    // Sixty runs of the same agent, none of them written about.
    let at = OffsetDateTime::now_utc() - Duration::minutes(30);
    let spans: Vec<Value> = (0..60)
        .map(|i| {
            json!({
                "trace_id": run_id(i),
                "span_id": span_id(i),
                "name": "support",
                "kind": "agent_run",
                "status": if i % 7 == 0 { "error" } else { "ok" },
                "is_root": true,
                "is_event": false,
                "start_time": rfc3339(at),
                "end_time": rfc3339(at + Duration::seconds(1)),
                "entity_kind": "agent",
                "entity_name": AGENT,
                "received_at": rfc3339(at),
                "error_count": if i % 7 == 0 { 1 } else { 0 },
                "correlation": { "project": "default" },
            })
        })
        .collect();
    for chunk in spans.chunks(200) {
        let res = send(&app, post("/v1/ingest", json!({ "spans": chunk }).to_string())).await;
        assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    }

    mcpd_server::aspects::run_once(&state, OffsetDateTime::now_utc()).await.expect("a pass");

    let res = get(&app, &format!("/v1/api/traces/{}/aspects", run_id(1))).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    let notes = res.json()["notes"].as_array().expect("notes").clone();
    assert_eq!(notes.len(), 1, "one derived note per run: {}", res.text());
    assert_eq!(notes[0]["aspect"], json!("outcome"));
    assert!(
        notes[0].get("model").is_none(),
        "absent is the field that says no model wrote it: {}",
        res.text()
    );
    assert_eq!(notes[0]["text"], json!("the run finished with no error"));

    // A run that ended in an error is described as one, from the trace's own
    // status and error count.
    let res = get(&app, &format!("/v1/api/traces/{}/aspects", run_id(0))).await;
    assert_eq!(
        res.json()["notes"][0]["text"],
        json!("the run ended in an error at one step"),
        "{}",
        res.text()
    );

    // A second pass rewrites the same note rather than adding another beside
    // it, which is what lets this run on a timer with no record of what it has
    // already done.
    mcpd_server::aspects::run_once(&state, OffsetDateTime::now_utc()).await.expect("a second pass");
    let res = get(&app, &format!("/v1/api/traces/{}/aspects", run_id(1))).await;
    assert_eq!(res.json()["notes"].as_array().map(Vec::len), Some(1), "{}", res.text());

    // And the notes reach a snapshot. A derived note stamped with the pass's
    // own time sat outside the window that pass reads, and every later pass
    // moved it again, so the outcome aspect never left `processing`.
    let res =
        get(&app, &format!("/v1/api/aspects/progress?entity_kind=agent&entity_name={AGENT}")).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    let outcome = res.json()["aspects"]
        .as_array()
        .and_then(|a| a.iter().find(|r| r["aspect"] == json!("outcome")).cloned())
        .expect("an outcome row");
    assert_eq!(outcome["readiness"], json!("ready"), "{}", res.text());
}

// -------------------------------------------------------------- plumbing ---

struct Res {
    status: StatusCode,
    body: Vec<u8>,
}

impl Res {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or_else(|e| {
            panic!("expected a JSON body ({e}), got: {}", String::from_utf8_lossy(&self.body))
        })
    }
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

async fn send(app: &Router, req: Request<Body>) -> Res {
    let res = app.clone().oneshot(req).await.expect("the router returned a response");
    let status = res.status();
    let body = res.into_body().collect().await.expect("a whole body").to_bytes().to_vec();
    Res { status, body }
}

async fn get(app: &Router, path: &str) -> Res {
    let req = Request::builder()
        .uri(path)
        .header("authorization", format!("Bearer {READ_KEY}"))
        .body(Body::empty())
        .expect("a request");
    send(app, req).await
}

fn post(path: &str, body: String) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {INGEST_KEY}"))
        .body(Body::from(body))
        .expect("a request")
}
