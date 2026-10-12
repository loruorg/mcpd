//! The Postgres backend, against a real server.
//!
//! Skipped unless `MCPD_TEST_POSTGRES_URL` is set, so the ordinary test run
//! needs no services. CI sets it. Every bug this backend had while it was being
//! written was one that only appeared once the query reached the engine, so a
//! mock here would be worse than no test at all.
//!
//! A skip is silent, though. Every test here returns on its first line with
//! no server, and `cargo test` reports the whole file green, so a variable
//! that stops reaching the job looks exactly like a passing suite.
//! `MCPD_REQUIRE_LIVE` is the guard: with it set, a missing URL is a failure
//! rather than a skip. CI sets it in the job that starts a Postgres service container,
//! the one job where a skip can only mean something is wrong. The SDK live
//! suites carry the same guard for the same reason.

use mcpd_core::*;
use mcpd_store::{
    Batch, Include, Order, PostgresConfig, PostgresStore, Store, TraceQuery, TraceSort,
};
use serde_json::json;
use time::{Duration, OffsetDateTime};

fn endpoint() -> Option<String> {
    match std::env::var("MCPD_TEST_POSTGRES_URL") {
        Ok(url) => Some(url),
        Err(_) => {
            assert!(
                std::env::var("MCPD_REQUIRE_LIVE").is_err(),
                "MCPD_REQUIRE_LIVE is set but MCPD_TEST_POSTGRES_URL is not, so this suite \
                 would have reported green without reaching a server. That is the exact \
                 silence this variable exists to break."
            );
            None
        }
    }
}

fn corr(project: &str) -> Correlation {
    Correlation {
        project: project.into(),
        environment: Some("production".into()),
        service_name: Some("agent".into()),
        service_version: None,
        session_id: Some("s1".into()),
        thread_id: None,
        user_id: Some("u1".into()),
        resource_id: None,
        run_id: None,
        request_id: None,
        experiment_id: None,
        release: None,
    }
}

fn span(trace: &str, id: &str, parent: Option<&str>, kind: SpanKind, ms: i64) -> Span {
    let start = OffsetDateTime::now_utc() - Duration::minutes(1);
    let mut s = Span {
        trace_id: trace.into(),
        span_id: id.into(),
        parent_span_id: parent.map(Into::into),
        external_parent_span_id: None,
        name: format!("{}-{id}", kind.as_str()),
        kind,
        status: SpanStatus::Ok,
        start_time: start,
        end_time: Some(start + Duration::milliseconds(ms)),
        duration_ns: None,
        is_root: parent.is_none(),
        is_event: false,
        entity_kind: None,
        entity_id: None,
        entity_name: None,
        input: Some(json!({ "q": "where is my order" })),
        output: Some(json!({ "text": "shipped from Leeds on Tuesday" })),
        attributes: None,
        metadata: None,
        usage: kind.bills_tokens().then_some(TokenUsage {
            input_tokens: Some(1000),
            output_tokens: Some(200),
            total_tokens: None,
            input_details: None,
            output_details: None,
        }),
        cost: kind.bills_tokens().then(|| Cost {
            currency: "USD".into(),
            input_cost: None,
            output_cost: None,
            total_cost: Some(0.006),
            price_book: Some("builtin".into()),
            is_estimated: true,
        }),
        model_parameters: None,
        error: None,
        links: None,
        tags: parent.is_none().then(|| vec!["support".to_string()]),
        correlation: corr("pg-test"),
        model: kind.bills_tokens().then(|| "gpt-4o".to_string()),
        provider: kind.bills_tokens().then(|| "openai".to_string()),
        received_at: start,
        ingest_key: None,
    };
    s.normalise();
    s
}

async fn store() -> Option<PostgresStore> {
    let url = endpoint()?;
    let s = PostgresStore::new(PostgresConfig { url, ..Default::default() }).expect("build store");
    s.migrate().await.expect("migrate");
    Some(s)
}

#[tokio::test]
async fn a_round_trip_through_postgres_preserves_the_run() {
    let Some(s) = store().await else {
        eprintln!("MCPD_TEST_POSTGRES_URL not set, skipping");
        return;
    };
    let t = "70c0ffee0d15ea5e5b0a7f00dd15ea50";
    s.ingest(Batch {
        spans: vec![
            span(t, "1111111111111111", None, SpanKind::AgentRun, 8420),
            span(t, "2222222222222222", Some("1111111111111111"), SpanKind::ModelGeneration, 2400),
            span(t, "3333333333333333", Some("1111111111111111"), SpanKind::ToolCall, 90),
        ],
        ..Batch::default()
    })
    .await
    .expect("ingest");

    let d = s.get_trace("pg-test", t, &Include::default()).await.expect("read back");
    assert_eq!(d.spans.len(), 3);
    assert_eq!(d.trace.span_count, 3);
    assert_eq!(d.trace.model_count, 1);
    assert_eq!(d.trace.tool_count, 1);
    assert_eq!(d.trace.root_span_id.as_deref(), Some("1111111111111111"));
    assert_eq!(d.trace.kind, SpanKind::AgentRun);
    assert!(d.trace.cost.is_some(), "cost should roll up");
}

#[tokio::test]
async fn full_text_search_works_against_postgres() {
    let Some(s) = store().await else { return };
    let t = "71c0ffee0d15ea5e5b0a7f00dd15ea51";
    s.ingest(Batch {
        spans: vec![span(t, "4444444444444444", None, SpanKind::AgentRun, 100)],
        ..Batch::default()
    })
    .await
    .expect("ingest");

    let hit = s
        .list_traces(&TraceQuery {
            project: "pg-test".into(),
            q: Some("Leeds".into()),
            limit: 10,
            ..Default::default()
        })
        .await
        .expect("search");
    assert!(hit.items.iter().any(|x| x.trace_id == t));

    let miss = s
        .list_traces(&TraceQuery {
            project: "pg-test".into(),
            q: Some("refund".into()),
            limit: 10,
            ..Default::default()
        })
        .await
        .expect("search");
    assert!(!miss.items.iter().any(|x| x.trace_id == t));
}

#[tokio::test]
async fn a_hostile_filter_value_cannot_break_out_of_its_quotes() {
    let Some(s) = store().await else { return };
    // Everything in the Postgres backend is a bind parameter, so this is the
    // check that no filter value ever reaches the statement text. It also
    // guards the two places that genuinely cannot be bound, the group column
    // and the bucket width, by proving nothing else joined them.
    let r = s
        .list_traces(&TraceQuery {
            project: "pg-test".into(),
            q: Some("'; DROP TABLE mcpd.spans; --".into()),
            name: Some("' OR 1=1 --".into()),
            limit: 5,
            ..Default::default()
        })
        .await;
    assert!(r.is_ok(), "the query should run and match nothing: {r:?}");
    assert!(r.unwrap().items.is_empty());
}

#[tokio::test]
async fn keyset_pagination_returns_every_run_exactly_once() {
    let Some(s) = store().await else { return };
    // The failure keyset pagination exists to prevent is a page boundary that
    // repeats or skips a row, and it only shows up with more rows than a page.
    let mut ids = Vec::new();
    for i in 0..7 {
        let t = format!("72c0ffee0d15ea5e5b0a7f00dd15ea{i:02}");
        s.ingest(Batch {
            spans: vec![span(&t, &format!("55555555555555{i:02}"), None, SpanKind::AgentRun, 100)],
            ..Batch::default()
        })
        .await
        .expect("ingest");
        ids.push(t);
    }

    let mut seen = std::collections::BTreeSet::new();
    let mut cursor = None;
    for _ in 0..10 {
        let page = s
            .list_traces(&TraceQuery {
                project: "pg-test".into(),
                limit: 3,
                cursor: cursor.clone(),
                ..Default::default()
            })
            .await
            .expect("page");
        for t in &page.items {
            assert!(seen.insert(t.trace_id.clone()), "{} came back twice", t.trace_id);
        }
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    for id in &ids {
        assert!(seen.contains(id), "{id} was never returned");
    }
}

#[tokio::test]
async fn an_error_anywhere_makes_the_whole_run_an_error() {
    let Some(s) = store().await else { return };
    let t = "73c0ffee0d15ea5e5b0a7f00dd15ea52";
    let mut bad = span(t, "6666666666666602", Some("6666666666666601"), SpanKind::ToolCall, 90);
    bad.status = SpanStatus::Error;
    s.ingest(Batch {
        spans: vec![span(t, "6666666666666601", None, SpanKind::AgentRun, 500), bad],
        ..Batch::default()
    })
    .await
    .expect("ingest");

    let d = s.get_trace("pg-test", t, &Include::default()).await.expect("read back");
    // The root reported Ok. A run with a failed step did not succeed, and the
    // list view must say so without anyone opening the trace.
    assert_eq!(d.trace.status, SpanStatus::Error);
    assert_eq!(d.trace.error_count, 1);
}

#[tokio::test]
async fn a_retried_batch_is_not_counted_twice() {
    let Some(s) = store().await else { return };
    let t = "74c0ffee0d15ea5e5b0a7f00dd15ea53";
    // The key is unique per run. Idempotency is durable by design, so a fixed
    // key would make the SECOND run of this suite see the first call as the
    // replay, and the test would only pass on a database nobody had used.
    let key = format!("pg-idempotency-probe-{}", std::process::id());
    let batch = || Batch {
        spans: vec![span(t, "7777777777777701", None, SpanKind::AgentRun, 120)],
        ingest_key: Some(key.clone()),
        ..Batch::default()
    };

    let first = s.ingest(batch()).await.expect("first");
    let second = s.ingest(batch()).await.expect("retry");
    assert_eq!(first.accepted, 1);
    assert_eq!(second.accepted, 0, "a replay must not be accepted again");
    assert_eq!(second.duplicate, 1);

    let d = s.get_trace("pg-test", t, &Include::default()).await.expect("read back");
    assert_eq!(d.trace.span_count, 1);
}

#[tokio::test]
async fn deleting_a_run_needs_the_right_project() {
    let Some(s) = store().await else { return };
    let t = "75c0ffee0d15ea5e5b0a7f00dd15ea54";
    s.ingest(Batch {
        spans: vec![span(t, "8888888888888801", None, SpanKind::AgentRun, 100)],
        ..Batch::default()
    })
    .await
    .expect("ingest");

    // A trace id learned from one project must not erase it from another.
    assert!(s.delete_trace("someone-else", t).await.is_err());
    assert!(s.get_trace("pg-test", t, &Include::default()).await.is_ok());
    assert!(s.delete_trace("pg-test", t).await.is_ok());
    assert!(s.get_trace("pg-test", t, &Include::default()).await.is_err());
}

#[tokio::test]
async fn analytics_report_exact_quantiles_and_group_totals() {
    let Some(s) = store().await else { return };
    let base = OffsetDateTime::now_utc() - Duration::minutes(5);
    for i in 0..4 {
        let t = format!("76c0ffee0d15ea5e5b0a7f00dd15ea{i:02}");
        s.ingest(Batch {
            spans: vec![span(
                &t,
                &format!("99999999999999{i:02}"),
                None,
                SpanKind::ModelGeneration,
                (i + 1) * 1000,
            )],
            ..Batch::default()
        })
        .await
        .expect("ingest");
    }

    let sum = s
        .analytics_summary(
            "pg-test",
            base - Duration::hours(1),
            OffsetDateTime::now_utc(),
            Some("model"),
        )
        .await
        .expect("summary");
    assert!(sum.trace_count >= 4);
    assert!(sum.latency.p50.is_some(), "an exact quantile, not a sketch");
    assert!(sum.total_cost > 0.0, "model spans carry cost");
    let groups = sum.groups.expect("grouping was requested");
    assert!(groups.iter().any(|g| g.key == "gpt-4o"));

    // A column nobody allowed is refused rather than interpolated.
    assert!(s
        .analytics_summary("pg-test", base, OffsetDateTime::now_utc(), Some("1; DROP TABLE spans"),)
        .await
        .is_err());
}

#[tokio::test]
async fn a_run_holding_an_event_span_is_complete_and_carries_a_duration() {
    let Some(s) = store().await else { return };
    let t = "77c0ffee0d15ea5e5b0a7f00dd15ea55";
    // An event is a point in time. It has no end because there is nothing to
    // end, so counting it as an unfinished span left the run reading as still
    // running for ever, with no duration, whatever else arrived afterwards.
    // The root is built first so it owns the earliest start, which makes the
    // expected duration an exact figure rather than one with the gap between
    // two clock readings folded into it.
    let root = span(t, "aaaaaaaaaaaaaa00", None, SpanKind::AgentRun, 700);
    let mut marker = span(t, "aaaaaaaaaaaaaa01", Some("aaaaaaaaaaaaaa00"), SpanKind::Generic, 0);
    marker.start_time = root.start_time;
    marker.is_event = true;
    marker.end_time = None;
    marker.normalise();

    s.ingest(Batch { spans: vec![root, marker], ..Batch::default() }).await.expect("ingest");

    let d = s.get_trace("pg-test", t, &Include::default()).await.expect("read back");
    assert_eq!(d.trace.span_count, 2);
    assert!(d.trace.is_complete, "every span that can end has ended");
    assert_eq!(d.trace.duration_ns, Some(700_000_000), "the root ran for 700ms");
}

#[tokio::test]
async fn postgres_normalises_a_span_before_storing_it_like_the_other_backends() {
    let Some(s) = store().await else { return };
    let t = "78c0ffee0d15ea5e5b0a7f00dd15ea56";
    // Callers reach the store directly as well as through the ingest pipeline:
    // `mcpd demo`, `mcpd doctor` and the score and feedback routes all do. So
    // the store owns this, exactly as the embedded and ClickHouse backends do.
    let mut lying = span(t, "bbbbbbbbbbbbbb00", None, SpanKind::AgentRun, 250);
    lying.is_root = false;
    lying.duration_ns = Some(99);

    let mut event = span(t, "bbbbbbbbbbbbbb01", Some("bbbbbbbbbbbbbb00"), SpanKind::Generic, 40);
    event.is_event = true;

    s.ingest(Batch { spans: vec![lying, event], ..Batch::default() }).await.expect("ingest");

    let d = s.get_trace("pg-test", t, &Include::default()).await.expect("read back");
    let root = d.spans.iter().find(|x| x.span_id == "bbbbbbbbbbbbbb00").expect("root");
    assert!(root.is_root, "no parent means root, whatever the producer claimed");
    assert_eq!(root.duration_ns, Some(250_000_000), "the duration is derived, not trusted");

    let ev = d.spans.iter().find(|x| x.span_id == "bbbbbbbbbbbbbb01").expect("event");
    assert!(ev.end_time.is_none(), "an event span never carries an end");
    assert!(ev.duration_ns.is_none());
}

#[tokio::test]
async fn only_generation_and_inference_spans_count_as_model_calls() {
    let Some(s) = store().await else { return };
    let t = "79c0ffee0d15ea5e5b0a7f00dd15ea57";
    // A step and a chunk are structure around a model call, not extra calls.
    // Counting them made a Postgres deployment report four model calls where
    // the embedded and ClickHouse backends reported one for the same run.
    s.ingest(Batch {
        spans: vec![
            span(t, "cccccccccccccc00", None, SpanKind::AgentRun, 900),
            span(t, "cccccccccccccc01", Some("cccccccccccccc00"), SpanKind::ModelGeneration, 300),
            span(t, "cccccccccccccc02", Some("cccccccccccccc00"), SpanKind::ModelStep, 120),
            span(t, "cccccccccccccc03", Some("cccccccccccccc00"), SpanKind::ModelChunk, 10),
            span(t, "cccccccccccccc04", Some("cccccccccccccc00"), SpanKind::McpToolCall, 40),
        ],
        ..Batch::default()
    })
    .await
    .expect("ingest");

    let d = s.get_trace("pg-test", t, &Include::default()).await.expect("read back");
    assert_eq!(d.trace.model_count, 1, "one generation, whatever surrounds it");
    assert_eq!(d.trace.tool_count, 1);
}

#[tokio::test]
async fn the_score_rollup_is_a_mean_per_scorer_the_way_the_spec_describes_it() {
    let Some(s) = store().await else { return };
    let t = "7ac0ffee0d15ea5e5b0a7f00dd15ea58";
    s.ingest(Batch {
        spans: vec![span(t, "dddddddddddddd00", None, SpanKind::AgentRun, 200)],
        ..Batch::default()
    })
    .await
    .expect("ingest spans");

    let score = |id: &str, scorer: &str, value: f64| Score {
        score_id: id.into(),
        trace_id: Some(t.into()),
        span_id: None,
        scorer_id: scorer.into(),
        scorer_version: None,
        source: ScoreSource::Live,
        value,
        min: Some(0.0),
        max: Some(1.0),
        label: None,
        reason: None,
        score_trace_id: None,
        cost: None,
        metadata: None,
        timestamp: OffsetDateTime::now_utc(),
        correlation: corr("pg-test"),
    };
    s.ingest(Batch {
        scores: vec![
            score("pgscore-a", "answer_relevance", 0.2),
            score("pgscore-b", "answer_relevance", 1.0),
            score("pgscore-c", "toxicity", 0.9),
        ],
        ..Batch::default()
    })
    .await
    .expect("ingest scores");

    let d = s.get_trace("pg-test", t, &Include::default()).await.expect("read back");
    // The spec calls this "mean score per scorer id", and the run list reads it
    // as a number per scorer. An object per scorer here rendered as no score at
    // all in the operator UI, on Postgres only.
    let summary = d.trace.score_summary.expect("a scored run carries a summary");
    assert_eq!(summary.get("answer_relevance").and_then(serde_json::Value::as_f64), Some(0.6));
    assert_eq!(summary.get("toxicity").and_then(serde_json::Value::as_f64), Some(0.9));
}

#[tokio::test]
async fn sorting_by_score_ranks_a_run_by_its_worst_score_not_its_average() {
    let Some(s) = store().await else { return };
    // A project of its own, so the ranking under test is over these two runs
    // and nothing an earlier test left behind. The ids are fixed rather than
    // random: a repeat run rewrites the same two rows with the same values, so
    // the assertion means the same thing on a database that has seen this
    // suite before.
    let project = "pg-score-sort";
    let mixed = "7bc0ffee0d15ea5e5b0a7f00dd15ea59";
    let steady = "7cc0ffee0d15ea5e5b0a7f00dd15ea5a";

    for (t, id) in [(mixed, "eeeeeeeeeeeeee00"), (steady, "eeeeeeeeeeeeee01")] {
        let mut sp = span(t, id, None, SpanKind::AgentRun, 100);
        sp.correlation = corr(project);
        s.ingest(Batch { spans: vec![sp], ..Batch::default() }).await.expect("ingest spans");
    }

    let score = |id: &str, trace: &str, value: f64| Score {
        score_id: id.into(),
        trace_id: Some(trace.into()),
        span_id: None,
        scorer_id: "answer_relevance".into(),
        scorer_version: None,
        source: ScoreSource::Live,
        value,
        min: Some(0.0),
        max: Some(1.0),
        label: None,
        reason: None,
        score_trace_id: None,
        cost: None,
        metadata: None,
        timestamp: OffsetDateTime::now_utc(),
        correlation: corr(project),
    };
    s.ingest(Batch {
        scores: vec![
            // One bad answer among good ones. The mean is 0.6, the worst is 0.2.
            score("pgsort-a", mixed, 0.2),
            score("pgsort-b", mixed, 1.0),
            score("pgsort-c", steady, 0.5),
        ],
        ..Batch::default()
    })
    .await
    .expect("ingest scores");

    let page = s
        .list_traces(&TraceQuery {
            project: project.into(),
            sort: TraceSort::Score,
            order: Order::Asc,
            limit: 10,
            ..Default::default()
        })
        .await
        .expect("list");
    let order: Vec<&str> = page.items.iter().map(|t| t.trace_id.as_str()).collect();
    // Sorting by score is a search for the run that failed a check. Ranking on
    // the average buried it behind a run that was merely mediocre throughout.
    assert_eq!(order, vec![mixed, steady]);
}

#[tokio::test]
async fn a_run_whose_clock_ran_backwards_is_stored_with_no_duration_not_a_negative_one() {
    let Some(s) = store().await else { return };
    // Its own project and fixed ids, for the reason the score sort test gives.
    let project = "pg-duration-clamp";
    // Ordered so that the tie the clamp creates resolves to zero-then-skewed.
    // With the negative left in the column the skewed run sorts below every
    // real one, which is how a run that took no measurable time came to be
    // listed as the fastest thing in the project.
    let zero = "7dc0ffee0d15ea5e5b0a7f00dd15ea5b";
    let skewed = "7ec0ffee0d15ea5e5b0a7f00dd15ea5c";

    for (t, id, ms) in [(zero, "ffffffffffffff00", 0), (skewed, "ffffffffffffff01", -90)] {
        let mut sp = span(t, id, None, SpanKind::AgentRun, ms);
        sp.correlation = corr(project);
        s.ingest(Batch { spans: vec![sp], ..Batch::default() }).await.expect("ingest");
    }

    let page = s
        .list_traces(&TraceQuery {
            project: project.into(),
            sort: TraceSort::Duration,
            order: Order::Asc,
            limit: 10,
            ..Default::default()
        })
        .await
        .expect("list");
    let order: Vec<&str> = page.items.iter().map(|t| t.trace_id.as_str()).collect();
    assert_eq!(order, vec![zero, skewed], "a skewed run must not sort below a real zero");
}

#[tokio::test]
async fn a_run_of_nothing_but_an_event_has_not_finished() {
    let Some(s) = store().await else { return };
    let t = "7fc0ffee0d15ea5e5b0a7f00dd15ea5d";
    // The other side of excluding events from the open count. An event carries
    // no end, so a run holding only events has nothing open and nothing
    // finished either: its root has not arrived yet. `is_complete` is specified
    // as the root having ended AND no span still open, so the end time is
    // required as well.
    let mut only = span(t, "1212121212121200", None, SpanKind::Generic, 0);
    only.is_event = true;
    only.end_time = None;
    only.normalise();

    s.ingest(Batch { spans: vec![only], ..Batch::default() }).await.expect("ingest");

    let d = s.get_trace("pg-test", t, &Include::default()).await.expect("read back");
    assert!(!d.trace.is_complete, "a run that has not started has not finished");
    assert_eq!(d.trace.duration_ns, None, "and it has no duration to report");
}

#[tokio::test]
async fn a_score_that_arrives_before_its_spans_still_reaches_the_summary() {
    let Some(s) = store().await else { return };
    let t = "60c0ffee0d15ea5e5b0a7f00dd15ea60";
    // Judging a run while its spans are still streaming in is an ordinary
    // ordering, not a mistake. The rollup is written with `UPDATE traces`, and
    // that reaches no row until the first span creates it, so the score landed
    // in the scores table and never in the summary the run list reads. The
    // embedded store rebuilds the rollup whenever it rebuilds the projection,
    // which is why it never had this.
    // Cleared first, for the reason the corrected-event test gives: the
    // subject is a summary that is either built or missing, and a row left by
    // an earlier run of this suite already holds the built one.
    s.delete_trace("pg-test", t).await.ok();
    let score = Score {
        score_id: "pgscore-early".into(),
        trace_id: Some(t.into()),
        span_id: None,
        scorer_id: "answer_relevance".into(),
        scorer_version: None,
        source: ScoreSource::Live,
        value: 0.25,
        min: Some(0.0),
        max: Some(1.0),
        label: None,
        reason: None,
        score_trace_id: None,
        cost: None,
        metadata: None,
        timestamp: OffsetDateTime::now_utc(),
        correlation: corr("pg-test"),
    };
    s.ingest(Batch { scores: vec![score], ..Batch::default() }).await.expect("score first");
    s.ingest(Batch {
        spans: vec![span(t, "6060606060606000", None, SpanKind::AgentRun, 120)],
        ..Batch::default()
    })
    .await
    .expect("spans after");

    let d = s.get_trace("pg-test", t, &Include::default()).await.expect("read back");
    let summary = d.trace.score_summary.expect("a scored run carries a summary");
    assert_eq!(summary.get("answer_relevance").and_then(serde_json::Value::as_f64), Some(0.25));
}

#[tokio::test]
async fn feedback_that_arrives_before_its_spans_still_reaches_the_summary() {
    let Some(s) = store().await else { return };
    let t = "63c0ffee0d15ea5e5b0a7f00dd15ea63";
    // The same hole, through the other door. A thumb arrives from an operator
    // interface, and the run it names is still being written.
    // Cleared first, for the reason the corrected-event test gives: the
    // subject is a summary that is either built or missing, and a row left by
    // an earlier run of this suite already holds the built one.
    s.delete_trace("pg-test", t).await.ok();
    let f = Feedback {
        feedback_id: "pgfeedback-early".into(),
        trace_id: Some(t.into()),
        span_id: None,
        kind: FeedbackKind::Thumbs,
        value: json!("up"),
        comment: None,
        author_id: None,
        author_kind: None,
        timestamp: OffsetDateTime::now_utc(),
        metadata: None,
        correlation: corr("pg-test"),
    };
    s.ingest(Batch { feedback: vec![f], ..Batch::default() }).await.expect("feedback first");
    s.ingest(Batch {
        spans: vec![span(t, "6363636363636300", None, SpanKind::AgentRun, 120)],
        ..Batch::default()
    })
    .await
    .expect("spans after");

    let d = s.get_trace("pg-test", t, &Include::default()).await.expect("read back");
    let summary = d.trace.feedback_summary.expect("a run with feedback carries a summary");
    assert_eq!(summary.get("thumbs:\"up\"").and_then(serde_json::Value::as_i64), Some(1));
}

#[tokio::test]
async fn every_kind_of_tool_call_counts_as_a_tool_call() {
    let Some(s) = store().await else { return };
    let t = "61c0ffee0d15ea5e5b0a7f00dd15ea61";
    // Four kinds end in `tool_call`, and the count was rewritten from a pattern
    // to a list of names. A list is only equivalent to the pattern if it holds
    // all four, so all four are here rather than the one the rewrite was
    // checked against.
    s.ingest(Batch {
        spans: vec![
            span(t, "6161616161616100", None, SpanKind::AgentRun, 900),
            span(t, "6161616161616101", Some("6161616161616100"), SpanKind::ToolCall, 10),
            span(t, "6161616161616102", Some("6161616161616100"), SpanKind::McpToolCall, 10),
            span(t, "6161616161616103", Some("6161616161616100"), SpanKind::ClientToolCall, 10),
            span(t, "6161616161616104", Some("6161616161616100"), SpanKind::ProviderToolCall, 10),
        ],
        ..Batch::default()
    })
    .await
    .expect("ingest");

    let d = s.get_trace("pg-test", t, &Include::default()).await.expect("read back");
    assert_eq!(d.trace.tool_count, 4, "all four tool kinds are tool calls");
    assert_eq!(d.trace.model_count, 0);
}

#[tokio::test]
async fn a_span_corrected_from_an_event_to_a_real_span_reopens_the_run() {
    let Some(s) = store().await else { return };
    let t = "62c0ffee0d15ea5e5b0a7f00dd15ea62";
    // `is_event` decides whether a span with no end holds the run open, so a
    // stale one now reports a running trace as finished. The upsert rewrote
    // `kind` and left `is_event` at whatever the first copy of the span said,
    // where the embedded store replaces the whole row.
    // Cleared first. This is the one test here whose subject is a column
    // changing value, so a row an earlier run of the suite left behind would
    // already hold the corrected value and the assertion would mean nothing.
    s.delete_trace("pg-test", t).await.ok();
    let mut marker = span(t, "6262626262626200", None, SpanKind::Generic, 0);
    marker.is_event = true;
    marker.end_time = None;
    marker.normalise();
    s.ingest(Batch { spans: vec![marker.clone()], ..Batch::default() }).await.expect("first");

    let mut corrected = marker.clone();
    corrected.is_event = false;
    corrected.end_time = None;
    corrected.normalise();
    s.ingest(Batch { spans: vec![corrected], ..Batch::default() }).await.expect("second");

    let d = s.get_trace("pg-test", t, &Include::default()).await.expect("read back");
    assert!(!d.spans[0].is_event, "the correction was stored");
    assert!(!d.trace.is_complete, "an open span means the run is not finished");
}

/// Every correlation identifier that has to be indexed, per signal table.
///
/// The documented answer for this backend, written here rather than read from
/// `postgres_schema.sql`: comparing the schema to itself proves nothing. A
/// filter on any of these is a lookup by an identifier a caller already holds,
/// and without an index it reads the whole retention window.
const REQUIRED_INDEXES: &[(&str, &[&str])] = &[
    (
        "spans",
        &[
            "environment",
            "service_name",
            "session_id",
            "thread_id",
            "user_id",
            "resource_id",
            "run_id",
            "request_id",
            "experiment_id",
            "release",
            "trace_id",
        ],
    ),
    (
        "traces",
        &[
            "environment",
            "service_name",
            "session_id",
            "thread_id",
            "user_id",
            "experiment_id",
            "release",
        ],
    ),
    ("logs", &["trace_id", "span_id", "environment", "service_name", "session_id", "user_id"]),
    ("metrics", &["trace_id", "span_id", "environment"]),
    ("scores", &["trace_id", "span_id", "environment", "experiment_id"]),
    ("feedback", &["trace_id", "span_id", "environment"]),
];

/// Held by every test that reads or changes the SHAPE of the shared schema.
///
/// The tests in this file run in parallel against one database. One of them
/// drops an index and forgets the schema version to prove `migrate` replays
/// the DDL, and another reads `pg_indexes` to prove every correlation column
/// is indexed. Run at the same moment, the second sees the first's gap and
/// reports a missing index that the schema does declare. Data tests are
/// unaffected, because each writes under its own project; only the two that
/// look at the catalogue itself have to take turns.
static SCHEMA_SHAPE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// A second connection to the same server, for reading the catalogue.
///
/// The store deliberately exposes no way to run arbitrary SQL, and it should
/// not: this file is the only thing that wants one, and it wants it to check
/// what the store built rather than to reach around it.
async fn catalogue(url: &str) -> tokio_postgres::Client {
    let (client, conn) =
        tokio_postgres::connect(url, tokio_postgres::NoTls).await.expect("a second connection");
    tokio::spawn(async move {
        let _ = conn.await;
    });
    client
}

#[tokio::test]
async fn every_correlation_identifier_is_indexed_on_every_signal_table() {
    let Some(url) = endpoint() else {
        eprintln!("MCPD_TEST_POSTGRES_URL not set, skipping");
        return;
    };
    let _shape = SCHEMA_SHAPE.lock().await;
    let Some(_s) = store().await else { return };
    let c = catalogue(&url).await;

    let mut missing: Vec<String> = Vec::new();
    for (table, columns) in REQUIRED_INDEXES {
        let rows = c
            .query("SELECT indexdef FROM pg_indexes WHERE tablename = $1", &[table])
            .await
            .expect("read the index list");
        // `Option`, not `String`. `pg_indexes.indexdef` is NULL for an index
        // that is being built or dropped as the scan runs, and reading it as
        // a `String` failed this test outright before the two catalogue tests
        // took turns. The lock above makes that window unlikely; this makes
        // it a skipped row rather than a panic if it ever opens again.
        let defs: Vec<String> = rows.iter().filter_map(|r| r.get::<_, Option<String>>(0)).collect();
        for column in *columns {
            // The column list runs from the first `(` to the next `)`. Stopping
            // there matters: a partial index carries `WHERE col IS NOT NULL`
            // after it, and reading the whole tail would count the predicate as
            // a column and call a missing index present.
            let found = defs.iter().any(|d| {
                d.split_once('(').and_then(|(_, rest)| rest.split_once(')')).is_some_and(
                    |(cols, _)| {
                        cols.split(',')
                            .any(|x| x.trim().trim_end_matches(" DESC").trim() == *column)
                    },
                )
            });
            if !found {
                missing.push(format!("{table}.{column}"));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "{} correlation identifier(s) have no index, so filtering on one reads the whole \
         retention window: {}. Add them to crates/mcpd-store/src/postgres_schema.sql.",
        missing.len(),
        missing.join(", ")
    );
}

#[tokio::test]
async fn a_start_with_nothing_to_apply_issues_no_ddl() {
    let Some(url) = endpoint() else {
        eprintln!("MCPD_TEST_POSTGRES_URL not set, skipping");
        return;
    };
    let _shape = SCHEMA_SHAPE.lock().await;
    let Some(s) = store().await else { return };
    let c = catalogue(&url).await;
    async fn present(c: &tokio_postgres::Client) -> bool {
        c.query_one("SELECT to_regclass('spans_session') IS NOT NULL", &[])
            .await
            .expect("look for the index")
            .get::<_, bool>(0)
    }
    assert!(present(&c).await, "the schema declares this index, so a migrated database has it");

    // Drop an index the schema creates. A `migrate` that replays the DDL puts
    // it straight back, and one that skips the batch does not, so this is the
    // difference between the two paths seen from outside the store. It matters
    // more here than on the embedded backend: `ALTER TABLE` takes an
    // `AccessExclusiveLock`, so a replica coming up would stall the ones
    // already serving.
    c.execute("DROP INDEX spans_session", &[]).await.expect("drop the index");
    assert!(!present(&c).await);

    s.migrate().await.expect("a second migrate");
    assert!(
        !present(&c).await,
        "migrate replayed the schema on a database already at this version, so every replica \
         coming up takes the exclusive locks again for nothing"
    );

    // And the slow path still runs when the version is behind, or a database
    // written by an older build would never gain the new tables.
    c.execute("DELETE FROM schema_version", &[]).await.expect("forget the version");
    s.migrate().await.expect("a migrate with the version gone");
    assert!(
        present(&c).await,
        "migrate skipped the schema on a database that records no version, so an upgrade from \
         an older build would never gain anything this release added"
    );
}

/// A database written by schema version 4, where `traces` was keyed by the
/// trace id alone, gains one projection per project when a newer daemon opens
/// it.
///
/// In a database of its own. The step drops and rebuilds `traces`, and every
/// other test in this file shares one database and runs at the same time, so
/// doing this there would pull the table out from under them.
#[tokio::test]
async fn a_version_4_database_splits_a_shared_trace_id_by_project() {
    let Some(url) = endpoint() else {
        eprintln!("MCPD_TEST_POSTGRES_URL not set, skipping");
        return;
    };
    let name = format!(
        "mcpd_v4_{}_{}",
        std::process::id(),
        OffsetDateTime::now_utc().unix_timestamp_nanos()
    );
    let admin = catalogue(&url).await;
    admin.batch_execute(&format!("CREATE DATABASE {name}")).await.expect("a scratch database");
    let (base, _) = url.rsplit_once('/').expect("the URL names a database");
    let scratch = format!("{base}/{name}");
    let t = "dddd0000000000000000000000000004";

    // Spans from two projects under one trace id, written by this build.
    {
        let s = PostgresStore::new(PostgresConfig { url: scratch.clone(), ..Default::default() })
            .unwrap();
        s.migrate().await.unwrap();
        let mut downstream =
            span(t, "bbbbbbbbbbbbbbb1", Some("aaaaaaaaaaaaaaa1"), SpanKind::ToolCall, 5);
        downstream.correlation = corr("downstream");
        s.ingest(Batch {
            spans: vec![span(t, "aaaaaaaaaaaaaaa1", None, SpanKind::AgentRun, 20), downstream],
            ..Batch::default()
        })
        .await
        .unwrap();
    }

    // Put the table back the way version 4 had it, from the released schema,
    // holding the one merged row version 4 would have written.
    {
        let c = catalogue(&scratch).await;
        c.batch_execute("DROP TABLE traces").await.unwrap();
        c.batch_execute(include_str!("fixtures/postgres-schema-v4.sql")).await.unwrap();
        c.execute(
            "INSERT INTO traces (trace_id, name, kind, status, start_time, span_count, project,
                                 received_at)
             VALUES ($1, 'merged', 'agent_run', 'ok', 0, 99, 'pg-test', 0)",
            &[&t],
        )
        .await
        .unwrap();
        c.batch_execute("DELETE FROM schema_version; INSERT INTO schema_version VALUES (4, 0);")
            .await
            .unwrap();
        let key: i64 = c
            .query_one(
                "SELECT COUNT(*) FROM information_schema.key_column_usage
                 WHERE table_name = 'traces' AND constraint_name = 'traces_pkey'",
                &[],
            )
            .await
            .unwrap()
            .get(0);
        assert_eq!(key, 1, "the fixture must reproduce the version 4 key");
    }

    let s =
        PostgresStore::new(PostgresConfig { url: scratch.clone(), ..Default::default() }).unwrap();
    s.migrate().await.expect("the version 5 step must apply");
    let upstream = s.get_trace("pg-test", t, &Include::default()).await.unwrap();
    let downstream = s.get_trace("downstream", t, &Include::default()).await.unwrap();
    assert_eq!(
        (upstream.trace.span_count, downstream.trace.span_count, upstream.spans.len()),
        (1, 1, 1),
        "each project's run must be rebuilt from its own spans, not kept from the merged row"
    );
    assert_ne!(upstream.trace.name, "merged", "the merged row must not survive the rebuild");
    s.migrate().await.expect("a second start must be a no-op");
    drop(s);

    let _ = admin.batch_execute(&format!("DROP DATABASE {name} WITH (FORCE)")).await;
}

/// One span of [`varied_runs`], at a fixed instant so a tie is a real tie.
fn step(
    trace: &str,
    id: &str,
    parent: Option<&str>,
    kind: SpanKind,
    offset_ms: i64,
    project: &str,
) -> Span {
    let mut s = span(trace, id, parent, kind, 120);
    s.start_time = OffsetDateTime::UNIX_EPOCH
        + Duration::seconds(1_700_000_000)
        + Duration::milliseconds(offset_ms);
    s.end_time = Some(s.start_time + Duration::milliseconds(120));
    s.duration_ns = None;
    s.received_at = s.start_time;
    s.correlation = corr(project);
    s
}

/// Every shape a projection row can take, written through the ordinary ingest
/// path: two projects under one trace id, a run whose root has not arrived, a
/// run still open, a run of nothing but an event, a caught error under a root
/// that succeeded, two spans that could be the root and started in the same
/// instant, and scores and feedback on some runs and not on others. The costs
/// and scores are exact in binary, so a sum in another order is the same sum.
fn varied_runs() -> Batch {
    let costed = |mut s: Span, cost: f64| {
        s.cost = Some(Cost {
            currency: "USD".into(),
            input_cost: None,
            output_cost: None,
            total_cost: Some(cost),
            price_book: None,
            is_estimated: true,
        });
        s
    };
    let (shared, open, running, event, caught, tied, unspanned) = (
        "eeee0000000000000000000000000001",
        "eeee0000000000000000000000000002",
        "eeee0000000000000000000000000003",
        "eeee0000000000000000000000000004",
        "eeee0000000000000000000000000005",
        "eeee0000000000000000000000000006",
        "eeee0000000000000000000000000007",
    );
    let root = Some("a000000000000001");
    let mut spans = vec![
        step(shared, "a000000000000001", None, SpanKind::AgentRun, 0, "alpha"),
        costed(step(shared, "a000000000000002", root, SpanKind::ModelGeneration, 10, "alpha"), 0.5),
        costed(step(shared, "a000000000000003", root, SpanKind::ToolCall, 20, "alpha"), 0.25),
        // The other project's half of the same trace has no root of its own.
        step(shared, "b000000000000001", root, SpanKind::ToolCall, 15, "beta"),
        step(
            shared,
            "b000000000000002",
            Some("b000000000000001"),
            SpanKind::McpToolCall,
            16,
            "beta",
        ),
        step(open, "a000000000000011", None, SpanKind::AgentRun, 100, "alpha"),
        step(open, "a000000000000012", Some("a000000000000011"), SpanKind::Retrieval, 110, "alpha"),
        step(running, "b000000000000021", None, SpanKind::AgentRun, 200, "beta"),
        step(event, "a000000000000031", None, SpanKind::Generic, 300, "alpha"),
        // The failed child started before its root.
        step(caught, "a000000000000041", None, SpanKind::AgentRun, 405, "alpha"),
        step(
            caught,
            "a000000000000042",
            Some("a000000000000041"),
            SpanKind::ToolCall,
            400,
            "alpha",
        ),
        step(tied, "b000000000000052", Some("b000000000000050"), SpanKind::ToolCall, 500, "beta"),
        step(tied, "b000000000000051", Some("b000000000000050"), SpanKind::Retrieval, 500, "beta"),
    ];
    spans[4].status = SpanStatus::Error;
    spans[6].end_time = None;
    spans[7].end_time = None;
    spans[8].is_event = true;
    spans[8].end_time = None;
    spans[9].tags = Some(vec!["caught".into()]);
    spans[10].status = SpanStatus::Error;

    let now = OffsetDateTime::UNIX_EPOCH + Duration::seconds(1_700_000_000);
    let score = |id: &str, trace: Option<&str>, scorer: &str, value: f64, project: &str| Score {
        score_id: id.into(),
        trace_id: trace.map(Into::into),
        span_id: None,
        scorer_id: scorer.into(),
        scorer_version: None,
        source: ScoreSource::Live,
        value,
        min: Some(0.0),
        max: Some(1.0),
        label: None,
        reason: None,
        score_trace_id: None,
        cost: None,
        metadata: None,
        timestamp: now,
        correlation: corr(project),
    };
    let feedback =
        |id: &str, trace: &str, kind: FeedbackKind, value: serde_json::Value, project: &str| {
            Feedback {
                feedback_id: id.into(),
                trace_id: Some(trace.into()),
                span_id: None,
                kind,
                value,
                comment: None,
                author_id: None,
                author_kind: None,
                timestamp: now,
                metadata: None,
                correlation: corr(project),
            }
        };
    Batch {
        spans,
        scores: vec![
            score("sc-1", Some(shared), "relevance", 0.5, "alpha"),
            score("sc-2", Some(shared), "relevance", 1.0, "alpha"),
            // An integral average, which Postgres and serde_json write
            // differently.
            score("sc-3", Some(shared), "faithfulness", 1.0, "alpha"),
            score("sc-4", Some(shared), "Accuracy", 0.25, "alpha"),
            score("sc-5", Some(shared), "relevance", 0.0, "beta"),
            score("sc-6", Some(caught), "relevance", 0.125, "alpha"),
            score("sc-7", Some(unspanned), "relevance", 0.75, "alpha"),
            score("sc-8", None, "relevance", 0.5, "alpha"),
        ],
        feedback: vec![
            feedback("fb-1", shared, FeedbackKind::Thumbs, json!("up"), "alpha"),
            feedback("fb-2", shared, FeedbackKind::Thumbs, json!("up"), "alpha"),
            feedback("fb-3", shared, FeedbackKind::Thumbs, json!("down"), "alpha"),
            feedback("fb-4", shared, FeedbackKind::Rating, json!(4), "alpha"),
            feedback("fb-5", tied, FeedbackKind::Tag, json!("billing"), "beta"),
            feedback("fb-6", open, FeedbackKind::Comment, json!("say \"slow\"\n"), "alpha"),
        ],
        ..Batch::default()
    }
}

/// Every column of every projection row, in key order.
///
/// The two summaries are read as the JSON they hold, which is what the store's
/// own reader does with them, so a difference in spacing or key order is not a
/// difference here and a number that reads back as another type is.
async fn every_trace_row(c: &tokio_postgres::Client) -> Vec<Vec<serde_json::Value>> {
    use tokio_postgres::types::Type;
    let rows = c.query("SELECT * FROM traces ORDER BY project, trace_id", &[]).await.unwrap();
    rows.iter()
        .map(|r| {
            r.columns()
                .iter()
                .enumerate()
                .map(|(i, col)| match *col.type_() {
                    Type::TEXT if col.name().ends_with("_summary") => r
                        .get::<_, Option<String>>(i)
                        .map_or(serde_json::Value::Null, |s| serde_json::from_str(&s).unwrap()),
                    Type::TEXT => json!(r.get::<_, Option<String>>(i)),
                    Type::INT8 => json!(r.get::<_, Option<i64>>(i)),
                    Type::FLOAT8 => json!(r.get::<_, Option<f64>>(i)),
                    Type::BOOL => json!(r.get::<_, Option<bool>>(i)),
                    ref other => {
                        panic!("traces.{} is {other}, which this test does not read", col.name())
                    }
                })
                .collect()
        })
        .collect()
}

/// The version 5 rebuild writes, for every run, the row the write path wrote.
///
/// The rebuild is one statement over the whole table and the write path is a
/// recompute per run, so they are two programs that must agree. This is the
/// check that they do, column by column, on every shape of run there is. In a
/// database of its own, because the rebuild drops `traces`.
#[tokio::test]
async fn the_version_5_rebuild_writes_the_rows_the_write_path_wrote() {
    let Some(url) = endpoint() else {
        eprintln!("MCPD_TEST_POSTGRES_URL not set, skipping");
        return;
    };
    let name = format!(
        "mcpd_rebuild_{}_{}",
        std::process::id(),
        OffsetDateTime::now_utc().unix_timestamp_nanos()
    );
    let admin = catalogue(&url).await;
    admin.batch_execute(&format!("CREATE DATABASE {name}")).await.expect("a scratch database");
    let (base, _) = url.rsplit_once('/').expect("the URL names a database");
    let scratch = format!("{base}/{name}");

    let s =
        PostgresStore::new(PostgresConfig { url: scratch.clone(), ..Default::default() }).unwrap();
    s.migrate().await.unwrap();
    s.ingest(varied_runs()).await.unwrap();
    let c = catalogue(&scratch).await;
    let written = every_trace_row(&c).await;
    assert_eq!(written.len(), 7, "one row per project and trace id that has spans");

    c.batch_execute("DELETE FROM schema_version; INSERT INTO schema_version VALUES (4, 0);")
        .await
        .unwrap();
    s.migrate().await.expect("the version 5 step must apply");
    let rebuilt = every_trace_row(&c).await;
    drop((s, c));
    let _ = admin.batch_execute(&format!("DROP DATABASE {name} WITH (FORCE)")).await;
    assert_eq!(rebuilt, written, "the rebuild wrote a row the write path would not have");
}
