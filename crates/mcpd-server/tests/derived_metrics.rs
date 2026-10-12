//! What the daemon derives from a span, and what it deliberately does not.
//!
//! Three properties are pinned here, because each of them is a decision that
//! looks like an accident from the outside:
//!
//! 1. The five duration names and the span kinds that produce them, including
//!    the kinds that produce nothing.
//! 2. The thirteen token names, from usage mcpd already stores and prices.
//! 3. The emission rule: a total is written whenever it is defined, an explicit
//!    zero included, and a detail only when it is strictly positive.

use axum::body::Body;
use axum::http::Request;
use axum::Router;
use mcpd_core::{
    Correlation, InputTokenDetails, MetricKind, OutputTokenDetails, Span, SpanKind, SpanStatus,
    TokenUsage,
};
use mcpd_server::derive::{
    derive, duration_metric_name, status_label, token_samples, DerivedMetrics, DURATION_METRICS,
    TOKEN_METRICS,
};
use mcpd_server::{router, AppState, ServerConfig};
use mcpd_store::sqlite::SqliteStore;
use mcpd_store::{Aggregation, MetricQuery, Store};
use serde_json::json;
use std::process;
use std::sync::Arc;
use time::OffsetDateTime;
use tower::ServiceExt;

/// The instant every fixture span starts and every derived point is stamped
/// with, so a query window can be written around it.
fn t0() -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(1_760_000_000).expect("a valid instant")
}

fn correlation() -> Correlation {
    serde_json::from_value(json!({ "project": "default" })).expect("correlation")
}

/// A finished span of one kind, lasting `ms` milliseconds.
fn span_of(kind: SpanKind, ms: i64) -> Span {
    let mut s = Span {
        trace_id: "4bf92f3577b34da6a3ce929d0e0e4736".to_owned(),
        span_id: "00f067aa0ba902b7".to_owned(),
        parent_span_id: None,
        external_parent_span_id: None,
        name: "step".to_owned(),
        kind,
        status: SpanStatus::Ok,
        start_time: t0(),
        end_time: Some(t0() + time::Duration::milliseconds(ms)),
        duration_ns: None,
        is_root: true,
        is_event: false,
        entity_kind: None,
        entity_id: None,
        entity_name: None,
        input: None,
        output: None,
        attributes: None,
        metadata: None,
        usage: None,
        cost: None,
        model_parameters: None,
        error: None,
        links: None,
        tags: None,
        correlation: correlation(),
        model: Some("gpt-4o".to_owned()),
        provider: Some("openai".to_owned()),
        received_at: t0(),
        ingest_key: None,
    };
    // The daemon derives from `duration_ns`, which the intake pipeline fills in
    // by calling this. A fixture that skipped it would be testing a span shape
    // no door ever produces.
    s.normalise();
    s
}

/// A model generation carrying exactly this usage.
fn generation_with(usage: TokenUsage) -> Span {
    let mut s = span_of(SpanKind::ModelGeneration, 1_200);
    s.usage = Some(usage);
    s
}

/// The names of the points derived from one span, in order.
fn names(span: &Span) -> Vec<String> {
    derive(std::slice::from_ref(span), DerivedMetrics::default(), t0())
        .into_iter()
        .map(|p| p.name)
        .collect()
}

/// The value of one named point derived from one span.
fn value_of(span: &Span, name: &str) -> Option<f64> {
    derive(std::slice::from_ref(span), DerivedMetrics::default(), t0())
        .into_iter()
        .find(|p| p.name == name)
        .map(|p| p.value)
}

// ------------------------------------------------- P063: the durations ---

#[test]
fn each_span_kind_that_has_a_duration_metric_names_the_right_one() {
    let expected = [
        (SpanKind::AgentRun, Some("mcpd.agent.duration")),
        (SpanKind::ToolCall, Some("mcpd.tool.duration")),
        (SpanKind::McpToolCall, Some("mcpd.tool.duration")),
        (SpanKind::ProviderToolCall, Some("mcpd.tool.duration")),
        (SpanKind::WorkflowRun, Some("mcpd.workflow.duration")),
        (SpanKind::ModelGeneration, Some("mcpd.model.duration")),
        (SpanKind::ProcessorRun, Some("mcpd.processor.duration")),
    ];
    for (kind, name) in expected {
        assert_eq!(duration_metric_name(kind), name, "{} maps to the wrong metric", kind.as_str());
    }
}

#[test]
fn the_five_duration_names_are_the_only_ones_any_span_kind_produces() {
    // Pins the catalogue against the enum rather than against a list written
    // beside it: a span kind added to the spec later cannot quietly invent a
    // sixth duration series that no documentation names.
    for kind in SpanKind::ALL {
        if let Some(name) = duration_metric_name(*kind) {
            assert!(
                DURATION_METRICS.contains(&name),
                "{} produces {name}, which is not in the published list",
                kind.as_str()
            );
        }
    }
}

#[test]
fn a_span_kind_with_no_duration_metric_derives_nothing() {
    // Twenty-six of the thirty-three kinds are in this position. A metric per
    // kind would be a series nobody asked for on every one of them.
    for kind in [
        SpanKind::ModelStep,
        SpanKind::ModelInference,
        SpanKind::WorkflowStep,
        SpanKind::MemoryOperation,
        SpanKind::RagEmbedding,
        SpanKind::Guardrail,
        SpanKind::Generic,
    ] {
        assert_eq!(duration_metric_name(kind), None, "{} must derive no duration", kind.as_str());
        assert!(names(&span_of(kind, 10)).is_empty(), "{} derived a point", kind.as_str());
    }
}

#[test]
fn a_client_tool_call_derives_no_duration_because_it_measures_the_wrong_thing() {
    // The server span covers emitting the marker; the tool then runs in the
    // caller's process. Folding it into `mcpd.tool.duration` would mix a
    // microsecond distribution into a second-scale one and hide the slow tools.
    assert_eq!(duration_metric_name(SpanKind::ClientToolCall), None);
    assert!(names(&span_of(SpanKind::ClientToolCall, 3)).is_empty());
}

#[test]
fn a_duration_point_carries_the_milliseconds_the_span_lasted() {
    let points = derive(&[span_of(SpanKind::AgentRun, 1_500)], DerivedMetrics::default(), t0());
    assert_eq!(points.len(), 1);
    let p = &points[0];
    assert_eq!(p.name, "mcpd.agent.duration");
    assert!((p.value - 1_500.0).abs() < f64::EPSILON, "value was {}", p.value);
    assert_eq!(p.unit.as_deref(), Some("ms"));
    assert_eq!(p.kind, MetricKind::Histogram, "a latency reading is a distribution");
    assert_eq!(p.trace_id.as_deref(), Some("4bf92f3577b34da6a3ce929d0e0e4736"));
    assert_eq!(p.span_id.as_deref(), Some("00f067aa0ba902b7"));
    assert_eq!(p.timestamp, t0(), "the point lands when the span happened, not when it arrived");
}

#[test]
fn every_duration_point_carries_a_status_label() {
    for kind in SpanKind::ALL {
        let Some(_) = duration_metric_name(*kind) else { continue };
        let points = derive(&[span_of(*kind, 5)], DerivedMetrics::default(), t0());
        let labels = points[0].labels.as_ref().expect("a duration point has labels");
        assert!(labels.get("status").is_some(), "{} has no status label", kind.as_str());
    }
}

#[test]
fn the_status_label_reads_error_cancelled_or_ok_and_never_unset() {
    let mut ok = span_of(SpanKind::ToolCall, 1);
    assert_eq!(status_label(&ok), "ok");

    // A producer that never set the field is not a producer that failed.
    ok.status = SpanStatus::Unset;
    assert_eq!(status_label(&ok), "ok");

    let mut failed = span_of(SpanKind::ToolCall, 1);
    failed.status = SpanStatus::Error;
    assert_eq!(status_label(&failed), "error");

    // Cancelled stays its own value. A deliberate abandonment counted as a
    // failure makes every timeout look like an outage.
    let mut cancelled = span_of(SpanKind::ToolCall, 1);
    cancelled.status = SpanStatus::Cancelled;
    assert_eq!(status_label(&cancelled), "cancelled");

    // An error payload outranks a status claiming otherwise.
    let mut lying = span_of(SpanKind::ToolCall, 1);
    lying.status = SpanStatus::Ok;
    lying.error = Some(
        serde_json::from_value(json!({ "message": "connection reset" })).expect("a span error"),
    );
    assert_eq!(status_label(&lying), "error");
}

#[test]
fn an_unfinished_span_derives_no_duration() {
    // A span recorded at its start has no end time, so there is nothing to
    // measure. Writing a zero would put every in-flight call at the fast end of
    // the histogram.
    let mut running = span_of(SpanKind::AgentRun, 100);
    running.end_time = None;
    running.normalise();
    assert!(names(&running).is_empty());
}

#[test]
fn an_event_derives_no_duration() {
    let mut event = span_of(SpanKind::AgentRun, 100);
    event.is_event = true;
    event.normalise();
    assert!(names(&event).is_empty(), "an event has no duration by definition");
}

// ---------------------------------------------------- P064: the tokens ---

#[test]
fn a_generation_with_full_usage_derives_all_thirteen_token_metrics() {
    let usage = TokenUsage {
        input_tokens: Some(900),
        output_tokens: Some(120),
        total_tokens: Some(1_020),
        input_details: Some(InputTokenDetails {
            text: Some(500),
            cache_read: Some(300),
            cache_write: Some(100),
            cache_write_5m: Some(60),
            cache_write_1h: Some(40),
            audio: Some(20),
            image: Some(10),
        }),
        output_details: Some(OutputTokenDetails {
            text: Some(80),
            reasoning: Some(30),
            audio: Some(7),
            image: Some(3),
        }),
    };
    let span = generation_with(usage);
    let derived = names(&span);

    for name in TOKEN_METRICS {
        assert!(derived.iter().any(|n| n == name), "{name} was not derived");
    }
    // The duration comes with it, so a generation is fourteen points at most.
    assert_eq!(derived.len(), TOKEN_METRICS.len() + 1, "derived {derived:?}");
}

#[test]
fn each_token_metric_carries_the_number_it_names() {
    let usage = TokenUsage {
        input_tokens: Some(900),
        output_tokens: Some(120),
        total_tokens: None,
        input_details: Some(InputTokenDetails {
            text: Some(500),
            cache_read: Some(300),
            cache_write: Some(100),
            cache_write_5m: Some(60),
            cache_write_1h: Some(40),
            audio: Some(20),
            image: Some(10),
        }),
        output_details: Some(OutputTokenDetails {
            text: Some(80),
            reasoning: Some(30),
            audio: Some(7),
            image: Some(3),
        }),
    };
    let span = generation_with(usage);
    let expected = [
        ("mcpd.model.total_input_tokens", 900.0),
        ("mcpd.model.total_output_tokens", 120.0),
        ("mcpd.model.input_text_tokens", 500.0),
        ("mcpd.model.input_cache_read_tokens", 300.0),
        ("mcpd.model.input_cache_write_tokens", 100.0),
        ("mcpd.model.input_cache_write_5m_tokens", 60.0),
        ("mcpd.model.input_cache_write_1h_tokens", 40.0),
        ("mcpd.model.input_audio_tokens", 20.0),
        ("mcpd.model.input_image_tokens", 10.0),
        ("mcpd.model.output_text_tokens", 80.0),
        ("mcpd.model.output_reasoning_tokens", 30.0),
        ("mcpd.model.output_audio_tokens", 7.0),
        ("mcpd.model.output_image_tokens", 3.0),
    ];
    for (name, want) in expected {
        assert_eq!(value_of(&span, name), Some(want), "{name} carried the wrong value");
    }
}

#[test]
fn a_token_point_is_a_counter_of_tokens_labelled_by_model_and_provider() {
    let span = generation_with(TokenUsage { input_tokens: Some(5), ..TokenUsage::default() });
    let points = derive(&[span], DerivedMetrics::default(), t0());
    let p = points
        .iter()
        .find(|p| p.name == "mcpd.model.total_input_tokens")
        .expect("the total was derived");
    assert_eq!(p.kind, MetricKind::Counter, "token counts add up; they are not a distribution");
    assert_eq!(p.unit.as_deref(), Some("{token}"));
    let labels = p.labels.as_ref().expect("labels");
    assert_eq!(labels["model"], json!("gpt-4o"));
    assert_eq!(labels["provider"], json!("openai"));
}

#[test]
fn only_a_model_generation_derives_token_metrics() {
    // A generation's steps and inferences carry a share of the same tokens.
    // Counting those too would report one call's usage two or three times, and
    // every cost chart built on the series would be wrong by a hidden factor.
    for kind in [SpanKind::ModelStep, SpanKind::ModelInference, SpanKind::AgentRun] {
        let mut s = span_of(kind, 10);
        s.usage = Some(TokenUsage { input_tokens: Some(100), ..TokenUsage::default() });
        let derived = names(&s);
        assert!(
            !derived.iter().any(|n| n.contains("tokens")),
            "{} derived token metrics: {derived:?}",
            kind.as_str()
        );
    }
}

#[test]
fn a_generation_with_no_usage_derives_only_its_duration() {
    assert_eq!(names(&span_of(SpanKind::ModelGeneration, 10)), vec!["mcpd.model.duration"]);
}

// -------------------------------------------- P065: the emission rule ---

#[test]
fn a_total_is_emitted_when_it_is_defined_even_at_zero() {
    // The model was asked and returned nothing. That is a reading about the
    // call, not an absence, and suppressing it hides a run of empty completions
    // on the chart it should be most visible on.
    let usage =
        TokenUsage { input_tokens: Some(1_000), output_tokens: Some(0), ..TokenUsage::default() };
    let samples = token_samples(&usage);
    assert_eq!(
        samples,
        vec![("mcpd.model.total_input_tokens", 1_000), ("mcpd.model.total_output_tokens", 0)],
    );
}

#[test]
fn a_total_the_provider_never_reported_is_not_invented() {
    let usage = TokenUsage { input_tokens: Some(7), ..TokenUsage::default() };
    assert_eq!(token_samples(&usage), vec![("mcpd.model.total_input_tokens", 7)]);
}

#[test]
fn a_zero_detail_is_not_emitted() {
    // The asymmetry with the totals is the point. A zero here means no cache
    // was read and no audio was sent, which is true of most calls at most
    // providers: eleven rows per call saying nothing happened is most of this
    // feature's write cost for none of its value.
    let usage = TokenUsage {
        input_tokens: Some(10),
        output_tokens: Some(4),
        total_tokens: None,
        input_details: Some(InputTokenDetails {
            text: Some(10),
            cache_read: Some(0),
            cache_write: Some(0),
            cache_write_5m: Some(0),
            cache_write_1h: Some(0),
            audio: Some(0),
            image: Some(0),
        }),
        output_details: Some(OutputTokenDetails {
            text: Some(4),
            reasoning: Some(0),
            audio: Some(0),
            image: Some(0),
        }),
    };
    assert_eq!(
        token_samples(&usage),
        vec![
            ("mcpd.model.total_input_tokens", 10),
            ("mcpd.model.total_output_tokens", 4),
            ("mcpd.model.input_text_tokens", 10),
            ("mcpd.model.output_text_tokens", 4),
        ],
        "only the positive details survive"
    );
}

#[test]
fn a_detail_breakdown_present_but_entirely_zero_adds_nothing_to_the_two_totals() {
    let usage = TokenUsage {
        input_tokens: Some(0),
        output_tokens: Some(0),
        total_tokens: None,
        input_details: Some(InputTokenDetails::default()),
        output_details: Some(OutputTokenDetails::default()),
    };
    let span = generation_with(usage);
    let derived = names(&span);
    assert_eq!(
        derived,
        vec![
            "mcpd.model.duration",
            "mcpd.model.total_input_tokens",
            "mcpd.model.total_output_tokens",
        ],
        "two defined totals at zero, and no detail at all"
    );
}

// ------------------------------------------------------ the off switch ---

#[test]
fn turning_both_halves_off_derives_nothing() {
    let span = generation_with(TokenUsage {
        input_tokens: Some(1),
        output_tokens: Some(1),
        ..TokenUsage::default()
    });
    assert!(derive(&[span], DerivedMetrics::off(), t0()).is_empty());
}

#[test]
fn each_half_turns_off_on_its_own() {
    let span = generation_with(TokenUsage { input_tokens: Some(1), ..TokenUsage::default() });

    let tokens_only = derive(
        std::slice::from_ref(&span),
        DerivedMetrics { durations: false, tokens: true },
        t0(),
    );
    assert_eq!(tokens_only.len(), 1);
    assert_eq!(tokens_only[0].name, "mcpd.model.total_input_tokens");

    let durations_only = derive(&[span], DerivedMetrics { durations: true, tokens: false }, t0());
    assert_eq!(durations_only.len(), 1);
    assert_eq!(durations_only[0].name, "mcpd.model.duration");
}

#[test]
fn an_absent_config_block_leaves_both_halves_on() {
    // The config file reads this. A deployment that never writes the block gets
    // the dashboards, and one that writes half of it keeps the other half.
    let all: DerivedMetrics = serde_json::from_value(json!({})).expect("an empty block parses");
    assert_eq!(all, DerivedMetrics::default());
    let half: DerivedMetrics =
        serde_json::from_value(json!({ "tokens": false })).expect("a half block parses");
    assert_eq!(half, DerivedMetrics { durations: true, tokens: false });
}

// -------------------------------------------------------- the id rule ---

#[test]
fn the_same_span_derives_the_same_ids_twice() {
    // A span is written when it starts and again when it ends, and OTLP
    // prescribes resending a whole export after a failure. Every backend stores
    // a metric by replacing on `metric_id`, so a derived id makes the second
    // write correct the first point rather than add one beside it. Allocated
    // ids would turn one slow call into three entries in the histogram.
    let span = generation_with(TokenUsage {
        input_tokens: Some(10),
        output_tokens: Some(2),
        ..TokenUsage::default()
    });
    let first = derive(std::slice::from_ref(&span), DerivedMetrics::default(), t0());
    let second = derive(&[span], DerivedMetrics::default(), t0() + time::Duration::hours(1));
    let ids =
        |v: &[mcpd_core::MetricPoint]| v.iter().map(|p| p.metric_id.clone()).collect::<Vec<_>>();
    assert_eq!(ids(&first), ids(&second), "a resend must replace, not duplicate");
}

#[test]
fn two_spans_in_the_same_trace_derive_different_ids() {
    let mut a = span_of(SpanKind::ToolCall, 5);
    let mut b = span_of(SpanKind::ToolCall, 9);
    b.span_id = "00f067aa0ba902b8".to_owned();
    a.normalise();
    b.normalise();
    let points = derive(&[a, b], DerivedMetrics::default(), t0());
    assert_eq!(points.len(), 2);
    assert_ne!(points[0].metric_id, points[1].metric_id, "one span must not overwrite another");
}

// ------------------------------------------------------ through a door ---

/// A private database file, named from the process **and** the instant it
/// started. A name from the pid alone is reopened when the operating system
/// recycles one, and the run then reads the previous run's database.
fn temp_db() -> std::path::PathBuf {
    // The clock alone is not unique: on macOS it moves in microseconds, so two
    // tests starting together opened one database and each counted the
    // other's rows. The counter is what makes the name this test's own.
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("mcpd-derived-test-{}-{unique}-{n}", process::id()));
    std::fs::create_dir_all(&dir).expect("create the test directory");
    dir.join("0.db")
}

/// The real router over a real store, with the derivation as configured.
async fn daemon(settings: DerivedMetrics) -> (Router, Arc<SqliteStore>) {
    let store = Arc::new(SqliteStore::open(temp_db(), 2).expect("open the store"));
    store.migrate().await.expect("migrate the store");
    let config = ServerConfig { derived_metrics: settings, ..ServerConfig::default() };
    let state = Arc::new(AppState::new(Arc::clone(&store) as Arc<dyn Store>, config));
    (router(state), store)
}

/// One model generation on the wire, ended and carrying usage.
fn wire_span() -> serde_json::Value {
    json!({
        "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736",
        "span_id": "00f067aa0ba902b7",
        "name": "answer",
        "kind": "model_generation",
        "status": "ok",
        "start_time": "2025-10-09T08:53:20Z",
        "end_time": "2025-10-09T08:53:21.500Z",
        "is_root": true,
        "is_event": false,
        "model": "gpt-4o",
        "provider": "openai",
        "usage": { "input_tokens": 900, "output_tokens": 0 },
        "correlation": { "project": "default", "user_id": "customer-9" },
        "received_at": "2025-10-09T08:53:21.500Z",
    })
}

async fn post_one_span(app: &Router) -> serde_json::Value {
    let body = json!({ "spans": [wire_span()] }).to_string();
    let request = Request::builder()
        .method("POST")
        .uri("/v1/ingest")
        .header("content-type", "application/json")
        .header("content-length", body.len().to_string())
        .body(Body::from(body))
        .expect("build the request");
    let response = app.clone().oneshot(request).await.expect("a response");
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.expect("read the body");
    serde_json::from_slice(&bytes).expect("a JSON response")
}

/// How many observations of `name` the store holds for the fixture window.
async fn count_of(store: &SqliteStore, name: &str) -> f64 {
    let q = MetricQuery {
        project: "default".to_owned(),
        name: name.to_owned(),
        from: t0() - time::Duration::hours(1),
        to: t0() + time::Duration::hours(1),
        bucket_seconds: 3_600,
        agg: Aggregation::Count,
        group_by: Vec::new(),
        filter: Vec::new(),
    };
    let series = store.query_metrics(&q).await.expect("query the metric");
    series.series.iter().flat_map(|s| s.points.iter()).map(|p| p.value).sum()
}

#[tokio::test]
async fn the_ingest_door_stores_the_metrics_it_derived() {
    let (app, store) = daemon(DerivedMetrics::default()).await;
    post_one_span(&app).await;

    assert_eq!(count_of(&store, "mcpd.model.duration").await, 1.0, "the duration was stored");
    assert_eq!(count_of(&store, "mcpd.model.total_input_tokens").await, 1.0);
    // The zero total is stored, because a zero total is a reading.
    assert_eq!(count_of(&store, "mcpd.model.total_output_tokens").await, 1.0);
    // The details were never reported, so nothing invented them.
    assert_eq!(count_of(&store, "mcpd.model.input_text_tokens").await, 0.0);
}

#[tokio::test]
async fn the_ingest_door_derives_nothing_when_the_setting_is_off() {
    let (app, store) = daemon(DerivedMetrics::off()).await;
    post_one_span(&app).await;

    for name in DURATION_METRICS.iter().chain(TOKEN_METRICS.iter()) {
        assert_eq!(count_of(&store, name).await, 0.0, "{name} was stored with the setting off");
    }
}

#[tokio::test]
async fn the_sender_is_told_about_the_batch_it_sent_not_the_rows_that_were_written() {
    // One span in, one signal accepted, whatever the daemon wrote beside it. A
    // client answered with three could not reconcile the number with anything.
    let (app, store) = daemon(DerivedMetrics::default()).await;
    let with = post_one_span(&app).await;
    assert_eq!(with["accepted"], json!(1), "derived rows must not inflate the count");

    let (plain, _) = daemon(DerivedMetrics::off()).await;
    let without = post_one_span(&plain).await;
    assert_eq!(with["accepted"], without["accepted"], "the count is the same either way");

    // And the rows really were written, so the assertion above is not passing
    // because nothing happened.
    assert_eq!(count_of(&store, "mcpd.model.duration").await, 1.0);
}

#[tokio::test]
async fn a_replay_is_reported_as_the_signals_the_sender_sent() {
    // `duplicate` is reported as the whole length of the batch that was
    // recognised, and the derived rows are in that batch. Without the same
    // correction `accepted` gets, a sender that resent one span was told two of
    // its signals were replays, and the second one did not exist.
    let (app, _store) = daemon(DerivedMetrics::default()).await;
    let keyed = json!({ "ingest_key": "batch-1", "spans": [wire_span()] }).to_string();
    let post = |body: String| {
        Request::builder()
            .method("POST")
            .uri("/v1/ingest")
            .header("content-type", "application/json")
            .header("content-length", body.len().to_string())
            .body(Body::from(body))
            .expect("build the request")
    };

    let first = app.clone().oneshot(post(keyed.clone())).await.expect("a response");
    assert_eq!(first.status(), axum::http::StatusCode::OK);

    let again = app.clone().oneshot(post(keyed)).await.expect("a response");
    let bytes = axum::body::to_bytes(again.into_body(), 1 << 20).await.expect("read the body");
    let body: serde_json::Value = serde_json::from_slice(&bytes).expect("JSON");
    assert_eq!(body["duplicate"], json!(1), "one span was resent, not three: {body}");
    assert_eq!(body["accepted"], json!(0), "{body}");
}

#[tokio::test]
async fn resending_the_same_span_leaves_one_observation_not_two() {
    let (app, store) = daemon(DerivedMetrics::default()).await;
    post_one_span(&app).await;
    post_one_span(&app).await;

    assert_eq!(
        count_of(&store, "mcpd.model.duration").await,
        1.0,
        "a resent export must replace the derived point, not add a second"
    );
}

#[tokio::test]
async fn a_derived_point_does_not_survive_the_erasure_of_the_run_it_came_from() {
    // A derived point carries the trace and the span it came from, which is
    // what makes it reachable from a run, and therefore what an erasure request
    // has to reach. A metric row pointing at a trace that was deleted for a
    // person is that person's data left behind, and it would also be a broken
    // link on every chart. The erasure suite runs with the derivation off so
    // its fixture arithmetic stays true, which is why the property is asserted
    // here instead of there.
    let (app, store) = daemon(DerivedMetrics::default()).await;
    post_one_span(&app).await;
    assert_eq!(count_of(&store, "mcpd.model.duration").await, 1.0, "the point was stored");

    let request = Request::builder()
        .method("DELETE")
        .uri("/v1/api/subjects/customer-9")
        .body(Body::empty())
        .expect("build the request");
    let response = app.clone().oneshot(request).await.expect("a response");
    assert_eq!(response.status(), axum::http::StatusCode::OK, "the erasure was refused");

    assert_eq!(
        count_of(&store, "mcpd.model.duration").await,
        0.0,
        "a derived point outlived the run it was derived from"
    );
    assert_eq!(count_of(&store, "mcpd.model.total_input_tokens").await, 0.0);
}
