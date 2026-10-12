//! End-to-end tests against a real SQLite store.
//!
//! These run against the same code path the server uses: a batch goes in
//! through `ingest`, and everything else is read back through the public trait.

use mcpd_core::*;
use mcpd_store::{
    sqlite::SqliteStore, Aggregation, Batch, Include, LogQuery, MetricQuery, Order, ScoreQuery,
    SpanQuery, Store, TraceQuery, TraceSort,
};
use serde_json::json;
use time::{Duration, OffsetDateTime};

fn corr(project: &str) -> Correlation {
    Correlation {
        project: project.into(),
        environment: Some("production".into()),
        service_name: Some("checkout".into()),
        service_version: None,
        session_id: Some("sess-1".into()),
        thread_id: None,
        user_id: Some("user-7".into()),
        resource_id: None,
        run_id: None,
        request_id: None,
        experiment_id: None,
        release: None,
    }
}

fn span(trace: &str, id: &str, parent: Option<&str>, kind: SpanKind, offset_ms: i64) -> Span {
    let start = OffsetDateTime::UNIX_EPOCH
        + Duration::seconds(1_700_000_000)
        + Duration::milliseconds(offset_ms);
    Span {
        trace_id: trace.into(),
        span_id: id.into(),
        parent_span_id: parent.map(Into::into),
        external_parent_span_id: None,
        name: format!("{}-{id}", kind.as_str()),
        kind,
        status: SpanStatus::Ok,
        start_time: start,
        end_time: Some(start + Duration::milliseconds(120)),
        duration_ns: None,
        is_root: parent.is_none(),
        is_event: false,
        entity_kind: None,
        entity_id: None,
        entity_name: Some("researcher".into()),
        input: Some(json!({ "messages": [{ "role": "user", "content": "where is my order" }] })),
        output: Some(json!({ "text": "It shipped on Tuesday" })),
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
            input_cost: Some(0.003),
            output_cost: Some(0.003),
            total_cost: Some(0.006),
            price_book: Some("builtin".into()),
            is_estimated: true,
        }),
        model_parameters: None,
        error: None,
        links: None,
        tags: parent.is_none().then(|| vec!["checkout".to_string(), "v2".to_string()]),
        correlation: corr("default"),
        model: kind.bills_tokens().then(|| "gpt-4o".to_string()),
        provider: kind.bills_tokens().then(|| "openai".to_string()),
        received_at: start,
        ingest_key: None,
    }
}

async fn store() -> SqliteStore {
    let dir = std::env::temp_dir().join(format!("mcpd-test-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(format!("{}.db", uniq()));
    let s = SqliteStore::open(&path, 2).expect("open");
    s.migrate().await.expect("migrate");
    s
}

/// A name no other test in this binary can produce.
///
/// The parts are joined rather than added. Adding them collided: two tests
/// starting a few nanoseconds apart could reach the same sum from different
/// (timestamp, counter) pairs, land on one database file, and then see each
/// other's rows. That made `find_bad_answers_by_score_threshold` fail about one
/// run in twelve, which is the worst frequency available: often enough to
/// redden CI, rare enough to be dismissed as noise.
///
/// The counter alone is unique within a process, and the timestamp keeps a
/// recycled process id from opening a previous run's leftover file. This is the
/// same construction `datasets.rs` already used.
fn uniq() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    format!(
        "{}-{}",
        OffsetDateTime::now_utc().unix_timestamp_nanos(),
        N.fetch_add(1, Ordering::Relaxed)
    )
}

fn full_trace(trace: &str) -> Batch {
    Batch {
        spans: vec![
            span(trace, "aaaaaaaaaaaaaaa1", None, SpanKind::AgentRun, 0),
            span(
                trace,
                "aaaaaaaaaaaaaaa2",
                Some("aaaaaaaaaaaaaaa1"),
                SpanKind::ModelGeneration,
                10,
            ),
            span(trace, "aaaaaaaaaaaaaaa3", Some("aaaaaaaaaaaaaaa1"), SpanKind::ToolCall, 40),
        ],
        ..Batch::default()
    }
}

#[tokio::test]
async fn ingest_then_read_back_a_whole_trace() {
    let s = store().await;
    let t = "4bf92f3577b34da6a3ce929d0e0e4736";
    let r = s.ingest(full_trace(t)).await.unwrap();
    assert_eq!(r.accepted, 3);
    assert_eq!(r.rejected, 0);

    let detail = s.get_trace("default", t, &Include::default()).await.unwrap();
    assert_eq!(detail.spans.len(), 3);
    assert_eq!(detail.trace.span_count, 3);
    assert_eq!(detail.trace.error_count, 0);
    assert_eq!(detail.trace.model_count, 1);
    assert_eq!(detail.trace.tool_count, 1);
    assert!(detail.trace.is_complete);
    // Root is chosen correctly, not just the first row.
    assert_eq!(detail.trace.root_span_id.as_deref(), Some("aaaaaaaaaaaaaaa1"));
    assert_eq!(detail.trace.kind, SpanKind::AgentRun);
    // Spans come back ready to render: parents before children.
    assert!(detail.spans[0].is_root);
}

#[tokio::test]
async fn trace_rollups_sum_tokens_and_cost() {
    let s = store().await;
    let t = "1111111111111111a3ce929d0e0e4736";
    s.ingest(full_trace(t)).await.unwrap();
    let d = s.get_trace("default", t, &Include::default()).await.unwrap();
    let usage = d.trace.usage.expect("usage should roll up");
    assert_eq!(usage.input_tokens, Some(1000));
    assert_eq!(usage.output_tokens, Some(200));
    let cost = d.trace.cost.expect("cost should roll up");
    assert!((cost.total_cost.unwrap() - 0.006).abs() < 1e-9);
}

#[tokio::test]
async fn a_failed_child_marks_the_whole_trace_failed() {
    // The root reported success; a swallowed error still has to surface,
    // because that is exactly the trace someone is hunting for.
    let s = store().await;
    let t = "2222222222222222a3ce929d0e0e4736";
    let mut batch = full_trace(t);
    batch.spans[2].status = SpanStatus::Error;
    batch.spans[2].error = Some(SpanError {
        message: "connection reset".into(),
        kind: Some("ECONNRESET".into()),
        stack: None,
        domain: None,
        retryable: Some(true),
        details: None,
    });
    s.ingest(batch).await.unwrap();

    let d = s.get_trace("default", t, &Include::default()).await.unwrap();
    assert_eq!(d.trace.status, SpanStatus::Error);
    assert_eq!(d.trace.error_count, 1);
}

#[tokio::test]
async fn spans_arriving_out_of_order_still_project_correctly() {
    let s = store().await;
    let t = "3333333333333333a3ce929d0e0e4736";
    let b = full_trace(t);
    // Children first, root last — the order a real collector often sees.
    let (root, children) = (b.spans[0].clone(), b.spans[1..].to_vec());
    s.ingest(Batch { spans: children, ..Batch::default() }).await.unwrap();
    let mid = s.get_trace("default", t, &Include::default()).await.unwrap();
    assert_eq!(mid.trace.span_count, 2, "trace must be listable before its root arrives");

    s.ingest(Batch { spans: vec![root], ..Batch::default() }).await.unwrap();
    let after = s.get_trace("default", t, &Include::default()).await.unwrap();
    assert_eq!(after.trace.span_count, 3);
    assert_eq!(after.trace.root_span_id.as_deref(), Some("aaaaaaaaaaaaaaa1"));
    assert_eq!(after.trace.kind, SpanKind::AgentRun);
}

#[tokio::test]
async fn re_ingesting_a_span_updates_rather_than_duplicates() {
    let s = store().await;
    let t = "4444444444444444a3ce929d0e0e4736";
    s.ingest(full_trace(t)).await.unwrap();
    let mut again = full_trace(t);
    again.spans[0].name = "renamed".into();
    s.ingest(again).await.unwrap();

    let d = s.get_trace("default", t, &Include::default()).await.unwrap();
    assert_eq!(d.trace.span_count, 3, "a retry must not inflate the span count");
    assert_eq!(d.trace.name, "renamed");
}

#[tokio::test]
async fn ingest_key_makes_a_batch_idempotent() {
    let s = store().await;
    let t = "5555555555555555a3ce929d0e0e4736";
    let mut b = full_trace(t);
    b.ingest_key = Some("batch-1".into());
    let first = s.ingest(b.clone()).await.unwrap();
    assert_eq!(first.accepted, 3);
    assert_eq!(first.duplicate, 0);

    let second = s.ingest(b).await.unwrap();
    assert_eq!(second.accepted, 0, "a replayed batch must not be stored again");
    assert_eq!(second.duplicate, 3);
}

#[tokio::test]
async fn full_text_search_finds_a_run_by_what_the_model_said() {
    let s = store().await;
    let t = "6666666666666666a3ce929d0e0e4736";
    s.ingest(full_trace(t)).await.unwrap();

    let hit = s
        .list_traces(&TraceQuery {
            project: "default".into(),
            q: Some("shipped Tuesday".into()),
            limit: 10,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(hit.items.len(), 1, "should find the run by its completion text");

    let miss = s
        .list_traces(&TraceQuery {
            project: "default".into(),
            q: Some("refund".into()),
            limit: 10,
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(miss.items.is_empty());
}

#[tokio::test]
async fn search_input_with_fts_syntax_is_treated_as_text() {
    // A stray quote or operator must be a search, not a syntax error.
    let s = store().await;
    s.ingest(full_trace("7777777777777777a3ce929d0e0e4736")).await.unwrap();
    for weird in ["\"", "order OR", "NEAR(a b)", "*", "a\"b"] {
        let r = s
            .list_traces(&TraceQuery {
                project: "default".into(),
                q: Some(weird.into()),
                limit: 5,
                ..Default::default()
            })
            .await;
        assert!(r.is_ok(), "query {weird:?} should not error: {r:?}");
    }
}

#[tokio::test]
async fn filters_combine_and_narrow() {
    let s = store().await;
    for (i, t) in
        ["8888888888888888a3ce929d0e0e4736", "9999999999999999a3ce929d0e0e4736"].iter().enumerate()
    {
        let mut b = full_trace(t);
        if i == 1 {
            for sp in &mut b.spans {
                sp.correlation.environment = Some("staging".into());
            }
        }
        s.ingest(b).await.unwrap();
    }
    let prod = s
        .list_traces(&TraceQuery {
            project: "default".into(),
            environment: Some("production".into()),
            limit: 10,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(prod.items.len(), 1);

    let tagged = s
        .list_traces(&TraceQuery {
            project: "default".into(),
            tags: vec!["checkout".into()],
            limit: 10,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(tagged.items.len(), 2);

    let by_model = s
        .list_traces(&TraceQuery {
            project: "default".into(),
            model: Some("gpt-4o".into()),
            limit: 10,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(by_model.items.len(), 2);
}

#[tokio::test]
async fn pagination_is_stable_and_covers_every_row() {
    let s = store().await;
    for i in 0..25u32 {
        let t = format!("{:032x}", 0xa000_0000_u64 + u64::from(i));
        s.ingest(Batch {
            spans: vec![span(
                &t,
                "bbbbbbbbbbbbbbb1",
                None,
                SpanKind::AgentRun,
                i64::from(i) * 1000,
            )],
            ..Batch::default()
        })
        .await
        .unwrap();
    }
    let mut seen = std::collections::HashSet::new();
    let mut cursor = None;
    let mut pages = 0;
    loop {
        let page = s
            .list_traces(&TraceQuery {
                project: "default".into(),
                limit: 7,
                cursor: cursor.clone(),
                ..Default::default()
            })
            .await
            .unwrap();
        for t in &page.items {
            assert!(seen.insert(t.trace_id.clone()), "row {} appeared twice", t.trace_id);
        }
        pages += 1;
        cursor = page.next_cursor;
        if cursor.is_none() || pages > 10 {
            break;
        }
    }
    assert_eq!(seen.len(), 25, "pagination must return every row exactly once");
}

#[tokio::test]
async fn sorting_by_cost_and_duration_works() {
    let s = store().await;
    for (i, cost) in [(0u32, 0.01f64), (1, 0.50), (2, 0.05)] {
        let t = format!("{:032x}", 0xb000_0000_u64 + u64::from(i));
        let mut sp = span(&t, "ccccccccccccccc1", None, SpanKind::ModelGeneration, 0);
        sp.cost = Some(Cost {
            currency: "USD".into(),
            input_cost: None,
            output_cost: None,
            total_cost: Some(cost),
            price_book: None,
            is_estimated: true,
        });
        s.ingest(Batch { spans: vec![sp], ..Batch::default() }).await.unwrap();
    }
    let page = s
        .list_traces(&TraceQuery {
            project: "default".into(),
            sort: TraceSort::Cost,
            order: Order::Desc,
            limit: 10,
            ..Default::default()
        })
        .await
        .unwrap();
    let costs: Vec<f64> = page
        .items
        .iter()
        .map(|t| t.cost.as_ref().and_then(|c| c.total_cost).unwrap_or(0.0))
        .collect();
    assert_eq!(costs, vec![0.50, 0.05, 0.01], "most expensive first");
}

#[tokio::test]
async fn logs_scores_and_feedback_attach_to_their_trace() {
    let s = store().await;
    let t = "cccccccccccccccca3ce929d0e0e4736";
    s.ingest(full_trace(t)).await.unwrap();

    let now = OffsetDateTime::UNIX_EPOCH + Duration::seconds(1_700_000_000);
    s.ingest(Batch {
        logs: vec![LogRecord {
            log_id: "log-1".into(),
            trace_id: Some(t.into()),
            span_id: Some("aaaaaaaaaaaaaaa2".into()),
            timestamp: now,
            level: LogLevel::Warn,
            message: "retrying provider call".into(),
            fields: Some(json!({ "attempt": 2 })),
            logger: Some("agent".into()),
            correlation: corr("default"),
            received_at: now,
        }],
        scores: vec![Score {
            score_id: "score-1".into(),
            trace_id: Some(t.into()),
            span_id: None,
            scorer_id: "answer_relevance".into(),
            scorer_version: Some("1".into()),
            source: ScoreSource::Live,
            value: 0.42,
            min: Some(0.0),
            max: Some(1.0),
            label: None,
            reason: Some("did not address the shipping question".into()),
            score_trace_id: None,
            cost: None,
            metadata: None,
            timestamp: now,
            correlation: corr("default"),
        }],
        feedback: vec![Feedback {
            feedback_id: "fb-1".into(),
            trace_id: Some(t.into()),
            span_id: None,
            kind: FeedbackKind::Thumbs,
            value: json!("down"),
            comment: Some("wrong date".into()),
            author_id: Some("reviewer-3".into()),
            author_kind: Some("reviewer".into()),
            timestamp: now,
            metadata: None,
            correlation: corr("default"),
        }],
        ..Batch::default()
    })
    .await
    .unwrap();

    let d = s
        .get_trace("default", t, &Include { logs: true, scores: true, feedback: true })
        .await
        .unwrap();
    assert_eq!(d.logs.as_ref().unwrap().len(), 1);
    assert_eq!(d.scores.as_ref().unwrap().len(), 1);
    assert_eq!(d.feedback.as_ref().unwrap().len(), 1);
    // The rollup is maintained so the list can sort by quality without a join.
    let summary = d.trace.score_summary.expect("score summary");
    assert!((summary["answer_relevance"].as_f64().unwrap() - 0.42).abs() < 1e-9);
}

#[tokio::test]
async fn find_bad_answers_by_score_threshold() {
    let s = store().await;
    let now = OffsetDateTime::UNIX_EPOCH + Duration::seconds(1_700_000_000);
    for (i, value) in [(0u32, 0.9f64), (1, 0.2)] {
        let t = format!("{:032x}", 0xd000_0000_u64 + u64::from(i));
        s.ingest(full_trace(&t)).await.unwrap();
        s.ingest(Batch {
            scores: vec![Score {
                score_id: format!("sc-{i}"),
                trace_id: Some(t.clone()),
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
                timestamp: now,
                correlation: corr("default"),
            }],
            ..Batch::default()
        })
        .await
        .unwrap();
    }
    let bad = s
        .list_traces(&TraceQuery {
            project: "default".into(),
            has_score: Some("answer_relevance".into()),
            score_below: Some(0.5),
            limit: 10,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(bad.items.len(), 1, "only the low-scoring run should match");
}

#[tokio::test]
async fn log_search_filters_by_severity() {
    let s = store().await;
    let now = OffsetDateTime::UNIX_EPOCH + Duration::seconds(1_700_000_000);
    let mk = |id: &str, level: LogLevel, msg: &str| LogRecord {
        log_id: id.into(),
        trace_id: None,
        span_id: None,
        timestamp: now,
        level,
        message: msg.into(),
        fields: None,
        logger: None,
        correlation: corr("default"),
        received_at: now,
    };
    s.ingest(Batch {
        logs: vec![
            mk("l1", LogLevel::Debug, "verbose detail"),
            mk("l2", LogLevel::Error, "provider refused the request"),
        ],
        ..Batch::default()
    })
    .await
    .unwrap();

    let errors = s
        .list_logs(&LogQuery {
            project: "default".into(),
            level: Some(LogLevel::Error),
            limit: 50,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(errors.items.len(), 1);
    assert_eq!(errors.items[0].level, LogLevel::Error);

    let found = s
        .list_logs(&LogQuery {
            project: "default".into(),
            q: Some("refused".into()),
            limit: 50,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(found.items.len(), 1);
}

#[tokio::test]
async fn metrics_aggregate_into_buckets_with_exemplars() {
    let s = store().await;
    let base = OffsetDateTime::UNIX_EPOCH + Duration::seconds(1_700_000_000);
    let mut points = Vec::new();
    for i in 0..10i64 {
        points.push(MetricPoint {
            metric_id: format!("m{i}"),
            name: "mcpd.model.duration".into(),
            kind: MetricKind::Histogram,
            value: 100.0 + i as f64,
            unit: Some("ms".into()),
            timestamp: base + Duration::seconds(i * 10),
            labels: Some(json!({ "model": if i % 2 == 0 { "gpt-4o" } else { "claude-sonnet-4" } })),
            buckets: None,
            trace_id: Some("4bf92f3577b34da6a3ce929d0e0e4736".into()),
            span_id: None,
            correlation: corr("default"),
            received_at: base,
        });
    }
    s.ingest(Batch { metrics: points, ..Batch::default() }).await.unwrap();

    let r = s
        .query_metrics(&MetricQuery {
            project: "default".into(),
            name: "mcpd.model.duration".into(),
            from: base - Duration::seconds(60),
            to: base + Duration::seconds(600),
            bucket_seconds: 60,
            agg: Aggregation::Mean,
            group_by: vec!["model".into()],
            filter: vec![],
        })
        .await
        .unwrap();
    assert_eq!(r.unit.as_deref(), Some("ms"));
    assert_eq!(r.bucket, "60s");
    assert_eq!(r.series.len(), 2, "one series per model");
    let total: usize = r.series.iter().map(|s| s.points.len()).sum();
    assert!(total > 0);
    // An exemplar is what turns a spike into a run you can open.
    assert!(r.series.iter().any(|s| s.points.iter().any(|p| p.exemplar_trace_id.is_some())));
}

#[tokio::test]
async fn metric_label_filters_narrow_the_series() {
    let s = store().await;
    let base = OffsetDateTime::UNIX_EPOCH + Duration::seconds(1_700_000_000);
    let points: Vec<MetricPoint> = (0..10i64)
        .map(|i| MetricPoint {
            metric_id: format!("f{i}"),
            name: "mcpd.model.duration".into(),
            kind: MetricKind::Gauge,
            value: 100.0 + i as f64,
            unit: Some("ms".into()),
            timestamp: base + Duration::seconds(i),
            labels: Some(json!({ "model": if i % 2 == 0 { "gpt-4o" } else { "claude-sonnet-4" } })),
            buckets: None,
            trace_id: None,
            span_id: None,
            correlation: corr("default"),
            received_at: base,
        })
        .collect();
    s.ingest(Batch { metrics: points, ..Batch::default() }).await.unwrap();

    let q = |filter: Vec<(String, String)>| MetricQuery {
        project: "default".into(),
        name: "mcpd.model.duration".into(),
        from: base - Duration::minutes(1),
        to: base + Duration::minutes(5),
        bucket_seconds: 60,
        agg: Aggregation::Mean,
        group_by: vec!["model".into()],
        filter,
    };

    let all = s.query_metrics(&q(vec![])).await.unwrap();
    assert_eq!(all.series.len(), 2);

    let one = s.query_metrics(&q(vec![("model".into(), "gpt-4o".into())])).await.unwrap();
    assert_eq!(one.series.len(), 1, "the filter should leave one series");

    // A label value nobody used matches nothing, rather than everything.
    let none = s.query_metrics(&q(vec![("model".into(), "not-a-model".into())])).await.unwrap();
    assert!(none.series.is_empty());
}

#[tokio::test]
async fn scores_filter_by_source() {
    let s = store().await;
    let now = OffsetDateTime::UNIX_EPOCH + Duration::seconds(1_700_000_000);
    let mk = |id: &str, source: ScoreSource| Score {
        score_id: id.into(),
        trace_id: None,
        span_id: None,
        scorer_id: "relevance".into(),
        scorer_version: None,
        source,
        value: 0.5,
        min: Some(0.0),
        max: Some(1.0),
        label: None,
        reason: None,
        score_trace_id: None,
        cost: None,
        metadata: None,
        timestamp: now,
        correlation: corr("default"),
    };
    s.ingest(Batch {
        scores: vec![
            mk("a", ScoreSource::Live),
            mk("b", ScoreSource::Manual),
            mk("c", ScoreSource::Live),
        ],
        ..Batch::default()
    })
    .await
    .unwrap();

    let live = s
        .list_scores(&ScoreQuery {
            project: "default".into(),
            source: Some(ScoreSource::Live),
            limit: 50,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(live.items.len(), 2);
    assert!(live.items.iter().all(|x| x.source == ScoreSource::Live));
}

#[tokio::test]
async fn analytics_summary_reports_real_quantiles() {
    let s = store().await;
    let base = OffsetDateTime::UNIX_EPOCH + Duration::seconds(1_700_000_000);
    for i in 0..20u32 {
        let t = format!("{:032x}", 0xe000_0000_u64 + u64::from(i));
        let mut sp = span(&t, "eeeeeeeeeeeeeee1", None, SpanKind::AgentRun, 0);
        sp.end_time = Some(sp.start_time + Duration::milliseconds(i64::from(i + 1) * 10));
        if i < 3 {
            sp.status = SpanStatus::Error;
        }
        s.ingest(Batch { spans: vec![sp], ..Batch::default() }).await.unwrap();
    }
    let sum = s
        .analytics_summary("default", base - Duration::hours(1), base + Duration::hours(1), None)
        .await
        .unwrap();
    assert_eq!(sum.trace_count, 20);
    assert_eq!(sum.error_count, 3);
    assert!((sum.error_rate - 0.15).abs() < 1e-9);
    assert!(sum.latency.p50.unwrap() > 0.0);
    assert!(sum.latency.p99.unwrap() >= sum.latency.p50.unwrap());
    assert!(sum.latency.max.unwrap() >= sum.latency.p99.unwrap());
}

#[tokio::test]
async fn grouping_rejects_an_unknown_dimension() {
    // group_by comes off the query string; anything not on the allowlist must
    // be refused rather than interpolated into SQL.
    let s = store().await;
    let base = OffsetDateTime::UNIX_EPOCH;
    let r =
        s.analytics_summary("default", base, base + Duration::hours(1), Some("1) OR 1=1--")).await;
    assert!(r.is_err(), "unknown group_by must be rejected");
}

#[tokio::test]
async fn cost_breaks_down_by_model() {
    let s = store().await;
    let base = OffsetDateTime::UNIX_EPOCH + Duration::seconds(1_700_000_000);
    s.ingest(full_trace("f000000000000000a3ce929d0e0e4736")).await.unwrap();
    let c = s
        .analytics_cost(
            "default",
            base - Duration::hours(1),
            base + Duration::hours(1),
            "model",
            Some(3600),
        )
        .await
        .unwrap();
    assert!(c.total_cost > 0.0);
    assert_eq!(c.groups[0].key, "gpt-4o");
    assert!(c.series.is_some());
}

/// Every dimension the endpoint advertises must survive a `bucket`.
///
/// `cost_breaks_down_by_model` covered `model`, and `model` is one of the three
/// dimensions that resolve to a span column. The other six resolve to a `t.`
/// column, and the bucketed series read `FROM spans s` with no `traces` in
/// scope, so all six answered "no such column: t.environment" as an HTTP 500.
/// A test naming one dimension out of nine is how that reached a released
/// endpoint and a screen built on top of it.
#[tokio::test]
async fn every_advertised_cost_dimension_survives_a_bucket() {
    let s = store().await;
    let base = OffsetDateTime::UNIX_EPOCH + Duration::seconds(1_700_000_000);
    s.ingest(full_trace("f000000000000000a3ce929d0e0e4737")).await.unwrap();

    // The daemon's own allowlist, in `group_column`. If a dimension is added
    // there and not here, this test is the one that should have caught it.
    for dim in [
        "model",
        "provider",
        "entity_name",
        "environment",
        "status",
        "session_id",
        "user_id",
        "service_name",
        "name",
    ] {
        let c = s
            .analytics_cost(
                "default",
                base - Duration::hours(1),
                base + Duration::hours(1),
                dim,
                Some(3600),
            )
            .await
            .unwrap_or_else(|e| panic!("group_by={dim} with a bucket failed: {e}"));
        assert!(c.series.is_some(), "group_by={dim} returned no series");
    }
}

/// The bars have to add up to the table beside them.
///
/// The totals come from `grouped`, which reads `traces` for a trace dimension.
/// A series built over `spans` for the same dimension would sum a different set
/// of rows, so the chart and the ranking under it would disagree and neither
/// would be wrong on its own terms.
#[tokio::test]
async fn the_cost_series_sums_to_the_same_total_as_its_groups() {
    let s = store().await;
    let base = OffsetDateTime::UNIX_EPOCH + Duration::seconds(1_700_000_000);
    s.ingest(full_trace("f000000000000000a3ce929d0e0e4738")).await.unwrap();

    let c = s
        .analytics_cost(
            "default",
            base - Duration::hours(1),
            base + Duration::hours(1),
            "environment",
            Some(3600),
        )
        .await
        .unwrap();

    let series: f64 = c.series.as_ref().expect("a series").iter().map(|b| b.total_cost).sum();
    let groups: f64 = c.groups.iter().filter_map(|g| g.total_cost).sum();
    assert!(
        (series - groups).abs() < 1e-9,
        "the series sums to {series} and its groups to {groups}"
    );
}

#[tokio::test]
async fn deleting_a_trace_removes_every_attached_signal() {
    let s = store().await;
    let t = "aaaa0000000000000000000000000001";
    s.ingest(full_trace(t)).await.unwrap();
    let removed = s.delete_trace("default", t).await.unwrap();
    assert!(removed > 0);
    assert!(s.get_trace("default", t, &Include::default()).await.is_err());
    // The search index must go too, or a deleted prompt is still findable.
    let after = s
        .list_traces(&TraceQuery {
            project: "default".into(),
            q: Some("shipped".into()),
            limit: 10,
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(after.items.is_empty(), "deleted content must leave the search index");
}

#[tokio::test]
async fn retention_deletes_only_what_is_old_enough() {
    let s = store().await;
    let now = OffsetDateTime::now_utc();
    let mut old =
        span("bbbb0000000000000000000000000001", "fffffffffffffff1", None, SpanKind::AgentRun, 0);
    old.received_at = now - Duration::days(40);
    let mut fresh =
        span("bbbb0000000000000000000000000002", "fffffffffffffff2", None, SpanKind::AgentRun, 0);
    fresh.received_at = now;
    s.ingest(Batch { spans: vec![old, fresh], ..Batch::default() }).await.unwrap();

    let n = s.enforce_retention("default", now - Duration::days(30)).await.unwrap();
    assert!(n > 0);
    let left = s
        .list_traces(&TraceQuery { project: "default".into(), limit: 50, ..Default::default() })
        .await
        .unwrap();
    assert_eq!(left.items.len(), 1, "only the fresh trace should remain");
    assert_eq!(left.items[0].trace_id, "bbbb0000000000000000000000000002");
}

/// A span id is unique within its trace, not across projects, so retention in
/// one project must leave another project's search rows alone even where the
/// two share a span id.
#[tokio::test]
async fn retention_in_one_project_keeps_another_projects_search_rows() {
    let s = store().await;
    let now = OffsetDateTime::now_utc();
    let mut old =
        span("bbbb0000000000000000000000000011", "fffffffffffffff9", None, SpanKind::AgentRun, 0);
    old.received_at = now - Duration::days(40);
    let mut theirs =
        span("bbbb0000000000000000000000000012", "fffffffffffffff9", None, SpanKind::AgentRun, 0);
    theirs.received_at = now;
    theirs.correlation = corr("other-team");
    theirs.input = Some(json!({ "text": "a porcupine in the warehouse" }));
    s.ingest(Batch { spans: vec![old, theirs], ..Batch::default() }).await.unwrap();

    s.enforce_retention("default", now - Duration::days(30)).await.unwrap();
    let found = s
        .list_spans(&SpanQuery {
            project: "other-team".into(),
            q: Some("porcupine".into()),
            limit: 10,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(found.items.len(), 1, "another project's span can no longer be found by its text");
}

#[tokio::test]
async fn projects_are_isolated_from_each_other() {
    let s = store().await;
    let t = "cccc0000000000000000000000000001";
    let mut b = full_trace(t);
    for sp in &mut b.spans {
        sp.correlation.project = "team-a".into();
    }
    s.ingest(b).await.unwrap();
    s.ingest(full_trace("cccc0000000000000000000000000002")).await.unwrap();

    let a = s
        .list_traces(&TraceQuery { project: "team-a".into(), limit: 50, ..Default::default() })
        .await
        .unwrap();
    assert_eq!(a.items.len(), 1);
    // Reading another project's trace by id must not work.
    assert!(s.get_trace("default", t, &Include::default()).await.is_err());

    let mut projects = s.list_projects().await.unwrap();
    projects.sort();
    assert_eq!(projects, vec!["default".to_string(), "team-a".to_string()]);
}

#[tokio::test]
async fn span_search_finds_slow_steps_across_traces() {
    let s = store().await;
    for i in 0..5u32 {
        let t = format!("{:032x}", 0xf100_0000_u64 + u64::from(i));
        let mut sp = span(&t, "ddddddddddddddd1", None, SpanKind::ToolCall, 0);
        sp.end_time = Some(sp.start_time + Duration::milliseconds(i64::from(i) * 500));
        s.ingest(Batch { spans: vec![sp], ..Batch::default() }).await.unwrap();
    }
    let slow = s
        .list_spans(&SpanQuery {
            project: "default".into(),
            kind: Some(SpanKind::ToolCall),
            min_duration_ms: Some(1000),
            limit: 50,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(slow.items.len(), 3, "spans at 1000, 1500 and 2000 ms");
}

#[tokio::test]
async fn an_unfinished_trace_is_visible_and_marked_incomplete() {
    // Live tail depends on this: a run in flight must be listable.
    let s = store().await;
    let t = "dddd0000000000000000000000000001";
    let mut sp = span(t, "aaaaaaaaaaaaaaa1", None, SpanKind::AgentRun, 0);
    sp.end_time = None;
    s.ingest(Batch { spans: vec![sp], ..Batch::default() }).await.unwrap();

    let page = s
        .list_traces(&TraceQuery { project: "default".into(), limit: 10, ..Default::default() })
        .await
        .unwrap();
    assert_eq!(page.items.len(), 1);
    assert!(!page.items[0].is_complete);
    assert!(page.items[0].duration_ns.is_none());
}

#[tokio::test]
async fn a_missing_trace_is_not_found_not_an_error() {
    let s = store().await;
    let r = s.get_trace("default", "0000000000000000000000000000dead", &Include::default()).await;
    assert!(matches!(r, Err(mcpd_store::Error::NotFound { .. })));
}

#[tokio::test]
async fn a_malformed_cursor_is_rejected_clearly() {
    let s = store().await;
    let r = s
        .list_traces(&TraceQuery {
            project: "default".into(),
            cursor: Some("not-a-cursor".into()),
            limit: 10,
            ..Default::default()
        })
        .await;
    assert!(matches!(r, Err(mcpd_store::Error::InvalidQuery(_))));
}

#[tokio::test]
async fn scores_list_filters_by_value_range() {
    let s = store().await;
    let now = OffsetDateTime::UNIX_EPOCH + Duration::seconds(1_700_000_000);
    let scores: Vec<Score> = [0.1f64, 0.5, 0.9]
        .iter()
        .enumerate()
        .map(|(i, v)| Score {
            score_id: format!("s{i}"),
            trace_id: None,
            span_id: None,
            scorer_id: "faithfulness".into(),
            scorer_version: None,
            source: ScoreSource::Offline,
            value: *v,
            min: Some(0.0),
            max: Some(1.0),
            label: None,
            reason: None,
            score_trace_id: None,
            cost: None,
            metadata: None,
            timestamp: now,
            correlation: corr("default"),
        })
        .collect();
    s.ingest(Batch { scores, ..Batch::default() }).await.unwrap();

    let low = s
        .list_scores(&ScoreQuery {
            project: "default".into(),
            max_value: Some(0.5),
            limit: 50,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(low.items.len(), 2);
}

#[tokio::test]
async fn metric_quantiles_are_exact_not_the_mean() {
    // 100 points: 1..100 in one bucket. The mean is 50.5; p50 is 50, p90 is 90,
    // p99 is 99. A fallback that returns the mean is wrong for all three, and
    // wrong in a way that flatters a latency chart.
    let s = store().await;
    let base = OffsetDateTime::UNIX_EPOCH + Duration::seconds(1_700_000_000);
    let points: Vec<MetricPoint> = (1..=100)
        .map(|i| MetricPoint {
            metric_id: format!("q{i}"),
            name: "latency".into(),
            kind: MetricKind::Histogram,
            value: f64::from(i),
            unit: Some("ms".into()),
            timestamp: base + Duration::milliseconds(i64::from(i)),
            labels: None,
            buckets: None,
            trace_id: None,
            span_id: None,
            correlation: corr("default"),
            received_at: base,
        })
        .collect();
    s.ingest(Batch { metrics: points, ..Batch::default() }).await.unwrap();

    let ask = |agg| MetricQuery {
        project: "default".into(),
        name: "latency".into(),
        from: base - Duration::seconds(10),
        to: base + Duration::seconds(10),
        bucket_seconds: 3600,
        agg,
        group_by: vec![],
        filter: vec![],
    };

    async fn value_of(s: &SqliteStore, q: MetricQuery) -> f64 {
        s.query_metrics(&q).await.unwrap().series[0].points[0].value
    }

    assert_eq!(value_of(&s, ask(Aggregation::P50)).await, 50.0, "p50 of 1..100 is 50");
    assert_eq!(value_of(&s, ask(Aggregation::P90)).await, 90.0, "p90 of 1..100 is 90");
    assert_eq!(value_of(&s, ask(Aggregation::P95)).await, 95.0);
    assert_eq!(value_of(&s, ask(Aggregation::P99)).await, 99.0);
    assert_eq!(value_of(&s, ask(Aggregation::Max)).await, 100.0);
    assert_eq!(value_of(&s, ask(Aggregation::Min)).await, 1.0);
    let mean = value_of(&s, ask(Aggregation::Mean)).await;
    assert!((mean - 50.5).abs() < 1e-9, "the mean is distinct from p50: {mean}");
}

/// One oversized non-ASCII prompt used to panic inside the writer mutex, poison
/// it, and make every later write in the process fail. That turned a single bad
/// span into an outage that lasted until somebody restarted the daemon.
#[tokio::test]
async fn an_oversized_non_ascii_prompt_does_not_stop_all_later_ingest() {
    let s = store().await;

    let mut ok =
        span("aa00000000000000000000000000aa01", "1111111111111111", None, SpanKind::AgentRun, 10);
    ok.input = Some(json!("ascii is fine"));
    s.ingest(Batch { spans: vec![ok], ..Batch::default() }).await.expect("first write");

    // 90 000 bytes of three-byte characters: byte 65536 lands mid-codepoint,
    // which is exactly where the search-index truncation used to cut.
    let mut huge =
        span("aa00000000000000000000000000aa02", "2222222222222222", None, SpanKind::AgentRun, 10);
    huge.input = Some(json!("\u{20ac}".repeat(30_000)));
    let second = s.ingest(Batch { spans: vec![huge], ..Batch::default() }).await;
    assert!(second.is_ok(), "an oversized non-ascii payload must be stored, not panic: {second:?}");

    // The write that proves the mutex survived.
    let mut after =
        span("aa00000000000000000000000000aa03", "3333333333333333", None, SpanKind::AgentRun, 10);
    after.input = Some(json!("ascii again"));
    let third = s.ingest(Batch { spans: vec![after], ..Batch::default() }).await;
    assert!(third.is_ok(), "ingest must keep working after a hostile payload: {third:?}");

    // And the big one is readable back whole, as the searching page promises.
    let d = s
        .get_trace("default", "aa00000000000000000000000000aa02", &Include::default())
        .await
        .expect("read the large span back");
    let stored = d.spans[0].input.as_ref().and_then(|v| v.as_str()).unwrap_or_default();
    assert_eq!(stored.chars().count(), 30_000, "the payload is stored whole, not truncated");
}

/// Every correlation identifier that has to be indexed, per signal table.
///
/// The documented answer for this backend, written here rather than read from
/// the schema file: comparing the schema to itself proves nothing. A filter on
/// any of these is a lookup by an identifier a caller already holds, and without
/// an index it reads the whole retention window.
///
/// `traces` carries the run-level subset because that is the set of columns it
/// has; `metrics`, `scores` and `feedback` carry the exemplar links, which are
/// what turn a spike on a chart into a run you can open.
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

/// Which columns an index leads with, for every index on one table.
fn indexed_columns(conn: &rusqlite::Connection, table: &str) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT sql FROM sqlite_master WHERE type = 'index' AND tbl_name = ?1")
        .expect("read the index list");
    stmt.query_map([table], |r| r.get::<_, Option<String>>(0))
        .expect("index rows")
        .filter_map(|r| r.expect("an index row"))
        .collect()
}

#[tokio::test]
async fn every_correlation_identifier_is_indexed_on_every_signal_table() {
    let s = store().await;
    let conn = rusqlite::Connection::open(s.path()).expect("reopen the database");

    let mut missing: Vec<String> = Vec::new();
    for (table, columns) in REQUIRED_INDEXES {
        let sql = indexed_columns(&conn, table);
        for column in *columns {
            // The column has to appear inside an index definition for this
            // table. Matching on the definition rather than the index name so a
            // renamed index still counts, and a differently-shaped one that
            // happens to lead with the column counts too.
            let found = sql.iter().any(|s| {
                // The column list runs from the first `(` to the next `)`. Stop
                // at that `)` rather than at the end of the string: a partial
                // index carries `WHERE col IS NOT NULL` after it, and reading
                // the whole tail counted the predicate as a column, which made
                // a single-column index look like it named nothing at all.
                s.split_once('(').and_then(|(_, rest)| rest.split_once(')')).is_some_and(
                    |(cols, _)| {
                        cols.split(',')
                            .any(|c| c.trim().trim_end_matches(" DESC").trim() == *column)
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
         retention window: {}. Add them to crates/mcpd-store/src/sqlite/schema.sql.",
        missing.len(),
        missing.join(", ")
    );
}

#[tokio::test]
async fn a_start_with_nothing_to_apply_issues_no_ddl() {
    let s = store().await;
    let conn = rusqlite::Connection::open(s.path()).expect("reopen the database");
    let present = |conn: &rusqlite::Connection| -> bool {
        conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'index' AND name = 'spans_session')",
            [],
            |r| r.get::<_, bool>(0),
        )
        .expect("look for the index")
    };
    assert!(present(&conn), "the schema declares this index, so a migrated database has it");

    // Drop an index the schema creates. A `migrate` that replays the DDL puts
    // it straight back, and one that skips the batch does not, so this is the
    // difference between the two paths seen from outside the store.
    conn.execute("DROP INDEX spans_session", []).expect("drop the index");
    assert!(!present(&conn));

    s.migrate().await.expect("a second migrate");
    assert!(
        !present(&conn),
        "migrate replayed the schema on a database already at this version. Every statement in \
         it is IF NOT EXISTS, so the replay is correct and it is still sixty statements of \
         parsing and catalogue lookup on a path with nothing to do, and CREATE INDEX IF NOT \
         EXISTS takes the write lock even when it creates nothing."
    );

    // And the slow path still runs when the version is behind, or a database
    // written by an older build would never gain the new tables.
    conn.execute("DELETE FROM schema_version", []).expect("forget the version");
    s.migrate().await.expect("a migrate with the version gone");
    assert!(
        present(&conn),
        "migrate skipped the schema on a database that records no version, so an upgrade from \
         an older build would never gain anything this release added"
    );
}

/// A database written by schema version 4, where `traces` was keyed by the
/// trace id alone, gains one projection per project when a newer daemon opens
/// it.
///
/// The old table is rebuilt from the released schema, kept verbatim in
/// `fixtures/`, rather than described here. A test that wrote its own idea of
/// the old table would prove the migration against something no deployment
/// ever had.
#[tokio::test]
async fn a_version_4_database_splits_a_shared_trace_id_by_project() {
    let dir = std::env::temp_dir().join(format!("mcpd-test-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(format!("{}.db", uniq()));
    let t = "dddd0000000000000000000000000004";

    // Spans from two projects under one trace id, written by this build.
    {
        let s = SqliteStore::open(&path, 1).expect("open");
        s.migrate().await.expect("migrate");
        let mut spans = full_trace(t).spans;
        let mut downstream =
            span(t, "bbbbbbbbbbbbbbb1", Some("aaaaaaaaaaaaaaa1"), SpanKind::ToolCall, 5);
        downstream.correlation = corr("downstream");
        spans.push(downstream);
        s.ingest(Batch { spans, ..Batch::default() }).await.unwrap();
    }

    // Put the table back the way version 4 had it, holding the one merged row
    // version 4 would have written, and record version 4.
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute("DROP TABLE traces", []).unwrap();
        conn.execute_batch(include_str!("fixtures/sqlite-schema-v4.sql")).unwrap();
        conn.execute(
            "INSERT INTO traces (trace_id, name, kind, status, start_time, span_count, project,
                                 received_at)
             VALUES (?1, 'merged', 'agent_run', 'ok', 0, 99, 'default', 0)",
            [t],
        )
        .unwrap();
        conn.execute("DELETE FROM schema_version", []).unwrap();
        conn.execute("INSERT INTO schema_version (version, applied_at) VALUES (4, 0)", []).unwrap();
        let key: i64 = conn
            .query_row("SELECT COUNT(*) FROM pragma_table_info('traces') WHERE pk > 0", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(key, 1, "the fixture must reproduce the version 4 key");
    }

    let s = SqliteStore::open(&path, 1).expect("reopen");
    s.migrate().await.expect("the version 5 step must apply");
    let upstream = s.get_trace("default", t, &Include::default()).await.unwrap();
    let downstream = s.get_trace("downstream", t, &Include::default()).await.unwrap();
    assert_eq!(
        (upstream.trace.span_count, downstream.trace.span_count),
        (upstream.spans.len() as u32, 1),
        "each project's run must be rebuilt from its own spans, not kept from the merged row"
    );
    assert_ne!(upstream.trace.name, "merged", "the merged row must not survive the rebuild");

    // A second start finds version 5 recorded and changes nothing.
    s.migrate().await.expect("a second start must be a no-op");
    let again = s.get_trace("downstream", t, &Include::default()).await.unwrap();
    assert_eq!(again.trace.span_count, 1);
}

/// Every shape a projection row can take, written through the ordinary ingest
/// path: two projects under one trace id, a run whose root has not arrived, a
/// run still open, a run of nothing but an event, a caught error under a root
/// that succeeded, two spans that could be the root and started in the same
/// instant, and scores and feedback on some runs and not on others.
fn varied_runs() -> Batch {
    let at = |mut s: Span, project: &str| {
        s.correlation = corr(project);
        s
    };
    let costed = |mut s: Span, cost: f64| {
        s.cost.get_or_insert_with(|| Cost {
            currency: "USD".into(),
            input_cost: None,
            output_cost: None,
            total_cost: None,
            price_book: None,
            is_estimated: true,
        });
        s.cost.as_mut().unwrap().total_cost = Some(cost);
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
    let mut spans = vec![
        at(span(shared, "a000000000000001", None, SpanKind::AgentRun, 0), "alpha"),
        at(
            costed(
                span(
                    shared,
                    "a000000000000002",
                    Some("a000000000000001"),
                    SpanKind::ModelGeneration,
                    10,
                ),
                0.5,
            ),
            "alpha",
        ),
        at(
            costed(
                span(shared, "a000000000000003", Some("a000000000000001"), SpanKind::ToolCall, 20),
                0.25,
            ),
            "alpha",
        ),
        // The other project's half of the same trace has no root of its own.
        at(
            span(shared, "b000000000000001", Some("a000000000000001"), SpanKind::ToolCall, 15),
            "beta",
        ),
        at(
            span(shared, "b000000000000002", Some("b000000000000001"), SpanKind::McpToolCall, 16),
            "beta",
        ),
        at(span(open, "a000000000000011", None, SpanKind::AgentRun, 100), "alpha"),
        at(
            span(open, "a000000000000012", Some("a000000000000011"), SpanKind::Retrieval, 110),
            "alpha",
        ),
        at(span(running, "b000000000000021", None, SpanKind::AgentRun, 200), "beta"),
        at(span(event, "a000000000000031", None, SpanKind::Generic, 300), "alpha"),
        // The failed child started before its root.
        at(span(caught, "a000000000000041", None, SpanKind::AgentRun, 405), "alpha"),
        at(
            span(caught, "a000000000000042", Some("a000000000000041"), SpanKind::ToolCall, 400),
            "alpha",
        ),
        at(
            span(tied, "b000000000000052", Some("b000000000000050"), SpanKind::ToolCall, 500),
            "beta",
        ),
        at(
            span(tied, "b000000000000051", Some("b000000000000050"), SpanKind::Retrieval, 500),
            "beta",
        ),
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
            feedback("fb-6", open, FeedbackKind::Comment, json!("slow"), "alpha"),
        ],
        ..Batch::default()
    }
}

/// Every column of every projection row, in key order.
fn every_trace_row(conn: &rusqlite::Connection) -> Vec<Vec<rusqlite::types::Value>> {
    let mut stmt = conn.prepare("SELECT * FROM traces ORDER BY project, trace_id").unwrap();
    let width = stmt.column_count();
    stmt.query_map([], |r| (0..width).map(|i| r.get(i)).collect())
        .unwrap()
        .collect::<std::result::Result<_, _>>()
        .unwrap()
}

/// The version 5 rebuild writes, for every run, the row the write path wrote.
///
/// The rebuild is one statement over the whole table and the write path is a
/// recompute per run, so they are two programs that must agree. This is the
/// check that they do, column by column, on every shape of run there is.
#[tokio::test]
async fn the_version_5_rebuild_writes_the_rows_the_write_path_wrote() {
    let s = store().await;
    s.ingest(varied_runs()).await.unwrap();
    let conn = rusqlite::Connection::open(s.path()).unwrap();
    let written = every_trace_row(&conn);
    assert_eq!(written.len(), 7, "one row per project and trace id that has spans");

    conn.execute("DELETE FROM schema_version", []).unwrap();
    conn.execute("INSERT INTO schema_version (version, applied_at) VALUES (4, 0)", []).unwrap();
    s.migrate().await.expect("the version 5 step must apply");
    let rebuilt = every_trace_row(&conn);
    assert_eq!(rebuilt, written, "the rebuild wrote a row the write path would not have");
}

/// How long the version 5 rebuild takes on a store large enough to matter.
///
/// Ignored, because it writes 200,000 runs. Run it on its own, in release, when
/// you change the rebuild:
/// `cargo test --release -p mcpd-store --test sqlite -- --ignored --nocapture rebuild_time`.
/// The daemon runs `migrate` before it binds its port, and the Kubernetes
/// startup probe gives it 90 s, so a rebuild that grows with the store is a
/// pod that restarts for ever. The bound below is far above what the rebuild
/// needs and far below that window.
#[tokio::test]
#[ignore = "writes 200,000 runs; run on its own with --ignored --release"]
async fn rebuild_time_on_a_large_store() {
    const TRACES: i64 = 200_000;
    const SPANS_PER_TRACE: i64 = 4;
    let s = store().await;
    {
        let conn = rusqlite::Connection::open(s.path()).unwrap();
        // Written straight into the table: the ingest path rebuilds each run's
        // projection as it goes, which is the cost this test measures.
        conn.execute(
            "WITH RECURSIVE n(i) AS (SELECT 0 UNION ALL SELECT i + 1 FROM n WHERE i < ?1)
             INSERT INTO spans (trace_id, span_id, parent_span_id, name, kind, status,
                                start_time, end_time, is_root, is_event, input_tokens,
                                output_tokens, total_cost, tags, project, environment,
                                service_name, session_id, user_id, received_at)
             SELECT printf('%032x', i / ?2), printf('%016x', i),
                    CASE WHEN i % ?2 = 0 THEN NULL ELSE printf('%016x', i - i % ?2) END,
                    'step-' || (i % ?2),
                    CASE i % ?2 WHEN 0 THEN 'agent_run' WHEN 1 THEN 'model_generation'
                                WHEN 2 THEN 'tool_call' ELSE 'retrieval' END,
                    CASE WHEN i % 97 = 0 THEN 'error' ELSE 'ok' END,
                    1700000000000000000 + i * 1000,
                    CASE WHEN i % 13 = 0 THEN NULL ELSE 1700000000000000000 + i * 1000 + 500 END,
                    i % ?2 = 0, 0,
                    CASE WHEN i % ?2 = 1 THEN 1000 END, CASE WHEN i % ?2 = 1 THEN 200 END,
                    CASE WHEN i % ?2 = 1 THEN 0.25 END,
                    CASE WHEN i % ?2 = 0 THEN '[\"bench\"]' END,
                    'p' || ((i / ?2) % 3), 'production', 'agent', 's' || (i / 40),
                    'u' || (i / 400), 1700000000000000000 + i * 1000
             FROM n",
            rusqlite::params![TRACES * SPANS_PER_TRACE - 1, SPANS_PER_TRACE],
        )
        .unwrap();
        conn.execute(
            "WITH RECURSIVE n(i) AS (SELECT 0 UNION ALL SELECT i + 1 FROM n WHERE i < ?1)
             INSERT INTO scores (score_id, trace_id, scorer_id, source, value, timestamp, project)
             SELECT 'sc' || i, printf('%032x', i / 2), 'scorer-' || (i % 2), 'live',
                    (i % 4) * 0.25, 0, 'p' || ((i / 2) % 3)
             FROM n",
            [TRACES - 1],
        )
        .unwrap();
        conn.execute(
            "WITH RECURSIVE n(i) AS (SELECT 0 UNION ALL SELECT i + 1 FROM n WHERE i < ?1)
             INSERT INTO feedback (feedback_id, trace_id, kind, value, timestamp, project)
             SELECT 'fb' || i, printf('%032x', i * 3), 'thumbs', '\"up\"', 0, 'p' || (i % 3)
             FROM n",
            [TRACES / 3 - 1],
        )
        .unwrap();
        conn.execute("DELETE FROM schema_version", []).unwrap();
        conn.execute("INSERT INTO schema_version (version, applied_at) VALUES (4, 0)", []).unwrap();
    }

    let started = std::time::Instant::now();
    s.migrate().await.expect("the version 5 step must apply");
    let took = started.elapsed();
    let conn = rusqlite::Connection::open(s.path()).unwrap();
    let rows: i64 = conn.query_row("SELECT COUNT(*) FROM traces", [], |r| r.get(0)).unwrap();
    eprintln!(
        "rebuilt {rows} runs from {} spans in {:.2} s",
        TRACES * SPANS_PER_TRACE,
        took.as_secs_f64()
    );
    assert_eq!(rows, TRACES);
    assert!(took < std::time::Duration::from_secs(30), "the rebuild took {took:?}");
}
