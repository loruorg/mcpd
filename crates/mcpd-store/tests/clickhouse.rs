//! The ClickHouse backend, against a real server.
//!
//! Skipped unless `MCPD_TEST_CLICKHOUSE_URL` is set, so the ordinary test run
//! needs no services. CI sets it. Every bug this backend had while it was being
//! written was one that only appeared once the query reached the engine, so a
//! mock here would be worse than no test at all.
//!
//! A skip is silent, though. Every test here returns on its first line with
//! no server, and `cargo test` reports the whole file green, so a variable
//! that stops reaching the job looks exactly like a passing suite.
//! `MCPD_REQUIRE_LIVE` is the guard: with it set, a missing URL is a failure
//! rather than a skip. CI sets it in the job that starts a ClickHouse service container,
//! the one job where a skip can only mean something is wrong. The SDK live
//! suites carry the same guard for the same reason.

use mcpd_core::*;
use mcpd_store::{Batch, ClickHouseConfig, ClickHouseStore, Include, Store, TraceQuery};
use serde_json::json;
use time::{Duration, OffsetDateTime};

fn endpoint() -> Option<String> {
    match std::env::var("MCPD_TEST_CLICKHOUSE_URL") {
        Ok(url) => Some(url),
        Err(_) => {
            assert!(
                std::env::var("MCPD_REQUIRE_LIVE").is_err(),
                "MCPD_REQUIRE_LIVE is set but MCPD_TEST_CLICKHOUSE_URL is not, so this suite \
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
        correlation: corr("ch-test"),
        model: kind.bills_tokens().then(|| "gpt-4o".to_string()),
        provider: kind.bills_tokens().then(|| "openai".to_string()),
        received_at: start,
        ingest_key: None,
    };
    s.normalise();
    s
}

async fn store() -> Option<ClickHouseStore> {
    let url = endpoint()?;
    let s = ClickHouseStore::new(ClickHouseConfig {
        url,
        database: format!("mcpd_test_{}", std::process::id()),
        ..Default::default()
    })
    .expect("build store");
    s.migrate().await.expect("migrate");
    Some(s)
}

#[tokio::test]
async fn a_round_trip_through_clickhouse_preserves_the_run() {
    let Some(s) = store().await else {
        eprintln!("MCPD_TEST_CLICKHOUSE_URL not set, skipping");
        return;
    };
    let t = "5c0ffee0d15ea5e5b0a7f00dd15ea5e5";
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

    let d = s.get_trace("ch-test", t, &Include::default()).await.expect("read back");
    assert_eq!(d.spans.len(), 3);
    assert_eq!(d.trace.span_count, 3);
    assert_eq!(d.trace.model_count, 1);
    assert_eq!(d.trace.tool_count, 1);
    assert_eq!(d.trace.root_span_id.as_deref(), Some("1111111111111111"));
    assert_eq!(d.trace.kind, SpanKind::AgentRun);
    assert!(d.trace.cost.is_some(), "cost should roll up");
}

#[tokio::test]
async fn full_text_search_works_against_clickhouse() {
    let Some(s) = store().await else { return };
    let t = "6c0ffee0d15ea5e5b0a7f00dd15ea5e6";
    s.ingest(Batch {
        spans: vec![span(t, "4444444444444444", None, SpanKind::AgentRun, 100)],
        ..Batch::default()
    })
    .await
    .expect("ingest");

    let hit = s
        .list_traces(&TraceQuery {
            project: "ch-test".into(),
            q: Some("Leeds".into()),
            limit: 10,
            ..Default::default()
        })
        .await
        .expect("search");
    assert!(hit.items.iter().any(|x| x.trace_id == t));

    let miss = s
        .list_traces(&TraceQuery {
            project: "ch-test".into(),
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
    // ClickHouse's HTTP interface has no bind parameters, so this is the check
    // that the quoting actually holds against the engine.
    let r = s
        .list_traces(&TraceQuery {
            project: "ch-test".into(),
            q: Some("'; DROP TABLE mcpd.spans; --".into()),
            name: Some("' OR 1=1 --".into()),
            limit: 5,
            ..Default::default()
        })
        .await;
    assert!(r.is_ok(), "the query should run and match nothing: {r:?}");
    assert!(r.unwrap().items.is_empty());
}

/// The `traces` table used to be sorted on `(project, start_time, trace_id)`,
/// and a run whose earliest span arrived in a later batch was written under a
/// second key that no merge collapsed. The schema now sorts on
/// `(project, trace_id)`, and `CREATE TABLE IF NOT EXISTS` leaves an existing
/// table as it is, so `migrate` has to move a table it finds under the old key.
/// This builds one the old way, fills it with the duplicate the old key
/// produced, and checks that migrating leaves one table, one key, one run.
#[tokio::test]
async fn migrate_moves_a_traces_table_from_the_old_sort_key_to_the_current_one() {
    let Some(url) = endpoint() else { return };
    let db = format!(
        "mcpd_resort_{}_{}",
        std::process::id(),
        OffsetDateTime::now_utc().unix_timestamp_nanos()
    );
    let http = reqwest::Client::new();
    let run = |sql: String| {
        let http = http.clone();
        let url = url.clone();
        async move {
            let res = http
                .post(&url)
                .query(&[("query", sql.as_str())])
                .header(reqwest::header::CONTENT_LENGTH, 0)
                .body("")
                .send()
                .await
                .expect("clickhouse answered");
            let status = res.status();
            let text = res.text().await.unwrap_or_default();
            assert!(status.is_success(), "{sql}\n{status}: {text}");
            text
        }
    };

    run(format!("CREATE DATABASE {db}")).await;
    // The table as the schema wrote it before the key changed.
    run(format!(
        "CREATE TABLE {db}.traces (
            trace_id String, root_span_id String, name LowCardinality(String),
            kind LowCardinality(String), status LowCardinality(String),
            start_time DateTime64(9, 'UTC'), end_time Nullable(DateTime64(9, 'UTC')),
            duration_ns Nullable(UInt64), span_count UInt32, error_count UInt32,
            input_tokens UInt64, output_tokens UInt64, total_cost Float64,
            currency LowCardinality(String), model_count UInt32, tool_count UInt32,
            tags Array(String), project LowCardinality(String),
            environment LowCardinality(String), service_name LowCardinality(String),
            session_id String, thread_id String, user_id String, experiment_id String,
            release String, score_summary String, feedback_summary String,
            min_score Nullable(Float64), is_complete UInt8,
            received_at DateTime64(9, 'UTC'), version UInt64
        ) ENGINE = ReplacingMergeTree(version)
        PARTITION BY toYYYYMM(start_time)
        ORDER BY (project, start_time, trace_id)"
    ))
    .await;
    // One run, twice: the second batch lowered its start time, so under the
    // old key these are two rows that never fold.
    let trace = "feedfacefeedfacefeedfacefeedface";
    for (start, version) in
        [("2026-03-01 00:00:01.000000000", 1), ("2026-03-01 00:00:00.000000000", 2)]
    {
        run(format!(
            "INSERT INTO {db}.traces (trace_id, root_span_id, name, kind, status, start_time, \
             span_count, error_count, input_tokens, output_tokens, total_cost, currency, \
             model_count, tool_count, project, is_complete, received_at, version) VALUES \
             ('{trace}', 'a1', 'checkout', 'agent_run', 'ok', '{start}', {version}, 0, 0, 0, 0, \
             'USD', 0, 0, 'p', 1, '{start}', {version})"
        ))
        .await;
    }
    let before = run(format!("SELECT count() FROM {db}.traces FINAL")).await;
    assert_eq!(before.trim(), "2", "the old key does not fold the two rows, which is the defect");

    let store = ClickHouseStore::new(ClickHouseConfig {
        url: url.clone(),
        database: db.clone(),
        ..Default::default()
    })
    .expect("build the store");
    store.migrate().await.expect("migrate");

    let key = run(format!(
        "SELECT sorting_key FROM system.tables WHERE database = '{db}' AND name = 'traces'"
    ))
    .await;
    assert_eq!(key.trim(), "project, trace_id", "the table was not moved to the current key");
    let leftover = run(format!(
        "SELECT count() FROM system.tables WHERE database = '{db}' AND name = 'traces_resorted'"
    ))
    .await;
    assert_eq!(leftover.trim(), "0", "the scratch table was left behind");
    let after = run(format!("SELECT count() FROM {db}.traces FINAL")).await;
    assert_eq!(after.trim(), "1", "the two copies of the run were not folded under the new key");
    let latest = run(format!("SELECT span_count FROM {db}.traces FINAL")).await;
    assert_eq!(latest.trim(), "2", "the newer version of the run did not win");

    // And through the store, which is what the daemon reads.
    let listed = store
        .list_traces(&TraceQuery { project: "p".into(), limit: 10, ..Default::default() })
        .await
        .expect("list the runs");
    assert_eq!(listed.items.len(), 1);

    // A second migrate finds the current key and does nothing.
    store.migrate().await.expect("migrate again");
    let again = run(format!("SELECT count() FROM {db}.traces FINAL")).await;
    assert_eq!(again.trim(), "1");

    run(format!("DROP DATABASE {db}")).await;
}

/// Run one statement against the server, for reading the catalogue.
///
/// The store deliberately exposes no way to run arbitrary SQL, and it should
/// not: this file is the only thing that wants one, and it wants it to check
/// what the store built rather than to reach around it.
async fn query(url: &str, sql: &str) -> String {
    let res = reqwest::Client::new()
        .post(url)
        .query(&[("query", sql)])
        .header(reqwest::header::CONTENT_LENGTH, 0)
        .body("")
        .send()
        .await
        .expect("clickhouse answered");
    let status = res.status();
    let text = res.text().await.unwrap_or_default();
    assert!(status.is_success(), "{sql}\n{status}: {text}");
    text
}

/// Every correlation identifier that has to carry a data-skipping index, per
/// signal table.
///
/// The documented answer for this backend, written here rather than read from
/// `clickhouse_schema.sql`: comparing the schema to itself proves nothing.
/// These are skipping indexes rather than btrees, so the promise is narrower
/// than on the other two: a filter on one of these reads a few granules instead
/// of the retention window, and it is still a scan of what it reads.
const REQUIRED_SKIP_INDEXES: &[(&str, &[&str])] = &[
    (
        "spans",
        &[
            "trace_id",
            "session_id",
            "thread_id",
            "user_id",
            "resource_id",
            "run_id",
            "request_id",
            "experiment_id",
            "release",
            "environment",
            "service_name",
        ],
    ),
    (
        "traces",
        &[
            "session_id",
            "thread_id",
            "user_id",
            "experiment_id",
            "release",
            "environment",
            "service_name",
        ],
    ),
    ("logs", &["trace_id", "span_id", "session_id", "user_id", "environment", "service_name"]),
    ("metrics", &["trace_id", "span_id", "environment"]),
    ("scores", &["trace_id", "span_id", "experiment_id", "environment"]),
    ("feedback", &["trace_id", "span_id", "environment"]),
];

#[tokio::test]
async fn every_correlation_identifier_carries_a_skipping_index() {
    let Some(url) = endpoint() else {
        eprintln!("MCPD_TEST_CLICKHOUSE_URL not set, skipping");
        return;
    };
    let db = format!("mcpd_idx_{}", std::process::id());
    let store = ClickHouseStore::new(ClickHouseConfig {
        url: url.clone(),
        database: db.clone(),
        ..Default::default()
    })
    .expect("build the store");
    store.migrate().await.expect("migrate");

    let mut missing: Vec<String> = Vec::new();
    for (table, columns) in REQUIRED_SKIP_INDEXES {
        let exprs = query(
            &url,
            &format!(
                "SELECT expr FROM system.data_skipping_indices \
                 WHERE database = '{db}' AND table = '{table}' FORMAT TSV"
            ),
        )
        .await;
        for column in *columns {
            if !exprs.lines().any(|e| e.trim() == *column) {
                missing.push(format!("{table}.{column}"));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "{} correlation identifier(s) carry no skipping index, so filtering on one reads every \
         granule in the window: {}. Add them to crates/mcpd-store/src/clickhouse_schema.sql.",
        missing.len(),
        missing.join(", ")
    );

    // And the partitions are cut by day, which is what lets a retention pass
    // drop one rather than read and rewrite every row in it.
    for (table, _) in REQUIRED_SKIP_INDEXES {
        let key = query(
            &url,
            &format!(
                "SELECT partition_key FROM system.tables \
                 WHERE database = '{db}' AND name = '{table}' FORMAT TSV"
            ),
        )
        .await;
        assert!(
            key.contains("toYYYYMMDD("),
            "{table} is partitioned by {key:?}, not by day. A month partition cannot be dropped \
             for a window shorter than a month, which is every window anybody sets."
        );
    }
    assert!(
        store.capabilities().partition_drop_retention,
        "the tables are cut by day and the backend says it cannot drop a partition"
    );

    query(&url, &format!("DROP DATABASE {db}")).await;
}

#[tokio::test]
async fn a_start_with_nothing_to_apply_issues_no_ddl() {
    let Some(url) = endpoint() else {
        eprintln!("MCPD_TEST_CLICKHOUSE_URL not set, skipping");
        return;
    };
    let db = format!("mcpd_ddl_{}", std::process::id());
    let store = ClickHouseStore::new(ClickHouseConfig {
        url: url.clone(),
        database: db.clone(),
        ..Default::default()
    })
    .expect("build the store");
    store.migrate().await.expect("migrate");

    // Drop a table the schema creates. A `migrate` that replays the DDL puts it
    // straight back, and one that skips does not, so this is the difference
    // between the two paths seen from outside the store. It matters here more
    // than anywhere: every replica calls this on start, and each statement is
    // metadata the whole cluster has to agree on.
    query(&url, &format!("DROP TABLE {db}.feedback")).await;
    store.migrate().await.expect("a second migrate");
    let back = query(
        &url,
        &format!(
            "SELECT count() FROM system.tables WHERE database = '{db}' AND name = 'feedback' \
             FORMAT TSV"
        ),
    )
    .await;
    assert_eq!(
        back.trim(),
        "0",
        "migrate replayed the whole schema on a database already at this version"
    );

    // And the slow path still runs when the recorded fingerprint is gone, or a
    // database written by an older build would never gain the new tables.
    query(&url, &format!("TRUNCATE TABLE {db}.schema_version")).await;
    store.migrate().await.expect("a migrate with the fingerprint gone");
    let restored = query(
        &url,
        &format!(
            "SELECT count() FROM system.tables WHERE database = '{db}' AND name = 'feedback' \
             FORMAT TSV"
        ),
    )
    .await;
    assert_eq!(
        restored.trim(),
        "1",
        "migrate skipped the schema on a database recording no fingerprint, so an upgrade from \
         an older build would never gain anything this release added"
    );

    query(&url, &format!("DROP DATABASE {db}")).await;
}
