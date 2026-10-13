//! The read surface: what goes out as a query string, and what comes back as a
//! generated type.
//!
//! Responses are built from the generated model and serialised, so these tests
//! break if the SDK and the server ever stop agreeing about a field name.

mod common;

use common::{FakeMcpd, Reply};
use mcpd_sdk::model::{
    Correlation, HealthStatus, Page, SpanKind, SpanStatus, Trace, TraceListResponse,
};
use mcpd_sdk::{Config, Include, LogLevel, Mcpd, TraceFilter};
use time::OffsetDateTime;

fn client(endpoint: String) -> Mcpd {
    Mcpd::with_config(Config { endpoint, project: "shop".to_owned(), ..Config::default() })
}

fn a_trace() -> Trace {
    Trace {
        trace_id: "4bf92f3577b34da6a3ce929d0e0e4736".to_owned(),
        root_span_id: Some("00f067aa0ba902b7".to_owned()),
        name: "support agent".to_owned(),
        kind: SpanKind::AgentRun,
        status: SpanStatus::Ok,
        start_time: OffsetDateTime::UNIX_EPOCH,
        end_time: Some(OffsetDateTime::UNIX_EPOCH + time::Duration::milliseconds(1200)),
        duration_ns: Some(1_200_000_000),
        span_count: 4,
        error_count: 0,
        usage: None,
        cost: None,
        model_count: 1,
        tool_count: 2,
        input_preview: None,
        output_preview: None,
        tags: Some(vec!["billing".to_owned()]),
        correlation: Correlation {
            project: "shop".to_owned(),
            environment: Some("production".to_owned()),
            service_name: None,
            service_version: None,
            session_id: None,
            thread_id: None,
            user_id: None,
            resource_id: None,
            run_id: None,
            request_id: None,
            experiment_id: None,
            release: None,
        },
        score_summary: None,
        feedback_summary: None,
        is_complete: true,
        received_at: OffsetDateTime::UNIX_EPOCH,
    }
}

#[tokio::test]
async fn list_traces_sends_the_filter_and_decodes_the_reply() {
    let body = serde_json::to_string(&TraceListResponse {
        traces: vec![a_trace()],
        page: Page { next_cursor: Some("c-2".to_owned()), has_more: true, total: Some(1) },
    })
    .unwrap();
    let server = FakeMcpd::scripted(vec![Reply::Body(200, body)]).await;
    let mcpd = client(server.endpoint());

    let response = mcpd
        .list_traces(&TraceFilter {
            status: Some(SpanStatus::Error),
            kind: Some(SpanKind::AgentRun),
            tags: vec!["billing".to_owned()],
            limit: Some(25),
            ..TraceFilter::default()
        })
        .await
        .expect("the read should have succeeded");

    assert_eq!(response.traces.len(), 1);
    assert_eq!(response.traces[0].name, "support agent");
    assert_eq!(response.traces[0].span_count, 4);
    assert_eq!(response.page.next_cursor.as_deref(), Some("c-2"));

    let request = &server.requests()[0];
    assert_eq!(request.method, "GET");
    assert_eq!(request.path(), "/v1/api/traces");
    let query = request.query();
    for expected in ["project=shop", "status=error", "kind=agent_run", "tags=billing", "limit=25"] {
        assert!(query.contains(expected), "{query} is missing {expected}");
    }
}

#[tokio::test]
async fn a_filter_that_was_not_set_is_not_sent() {
    let body = serde_json::to_string(&TraceListResponse {
        traces: Vec::new(),
        page: Page { next_cursor: None, has_more: false, total: Some(0) },
    })
    .unwrap();
    let server = FakeMcpd::scripted(vec![Reply::Body(200, body)]).await;
    let mcpd = client(server.endpoint());

    mcpd.list_traces(&TraceFilter::default()).await.expect("the read should have succeeded");
    assert_eq!(server.requests()[0].query(), "project=shop");
}

#[tokio::test]
async fn get_trace_asks_for_the_extras_it_was_told_to() {
    let server = FakeMcpd::scripted(vec![Reply::Body(200, "{}".to_owned())]).await;
    let mcpd = client(server.endpoint());

    // The body is deliberately not a TraceDetail: this test is about the request.
    let _ = mcpd.get_trace("4bf92f3577b34da6a3ce929d0e0e4736", Include::all()).await;

    let request = &server.requests()[0];
    assert_eq!(request.path(), "/v1/api/traces/4bf92f3577b34da6a3ce929d0e0e4736");
    assert!(
        request.query().contains("include=logs%2Cscores%2Cfeedback%2Cmetrics"),
        "{}",
        request.query()
    );
}

#[tokio::test]
async fn analytics_windows_go_out_as_rfc3339() {
    let server = FakeMcpd::scripted(vec![Reply::Body(200, "{}".to_owned())]).await;
    let mcpd = client(server.endpoint());

    let from = OffsetDateTime::UNIX_EPOCH;
    let to = OffsetDateTime::UNIX_EPOCH + time::Duration::hours(1);
    let _ = mcpd.analytics_cost(from, to, Some("model"), Some("1h")).await;

    let query = server.requests()[0].query().to_owned();
    assert!(query.contains("from=1970-01-01T00%3A00%3A00Z"), "{query}");
    assert!(query.contains("to=1970-01-01T01%3A00%3A00Z"), "{query}");
    assert!(query.contains("group_by=model"), "{query}");
    assert!(query.contains("bucket=1h"), "{query}");
}

#[tokio::test]
async fn log_and_score_filters_reach_their_endpoints() {
    let server = FakeMcpd::scripted(vec![
        Reply::Body(200, r#"{"logs":[],"page":{"has_more":false}}"#.to_owned()),
        Reply::Body(200, r#"{"scores":[],"page":{"has_more":false}}"#.to_owned()),
    ])
    .await;
    let mcpd = client(server.endpoint());

    mcpd.list_logs(&mcpd_sdk::LogFilter {
        level: Some(LogLevel::Warn),
        trace_id: Some("t-1".to_owned()),
        ..mcpd_sdk::LogFilter::default()
    })
    .await
    .expect("the read should have succeeded");
    mcpd.list_scores(&mcpd_sdk::ScoreFilter {
        scorer_id: Some("helpfulness".to_owned()),
        min_value: Some(0.5),
        ..mcpd_sdk::ScoreFilter::default()
    })
    .await
    .expect("the read should have succeeded");

    let requests = server.requests();
    assert_eq!(requests[0].path(), "/v1/api/logs");
    assert!(requests[0].query().contains("level=warn"));
    assert_eq!(requests[1].path(), "/v1/api/scores");
    assert!(requests[1].query().contains("scorer_id=helpfulness"));
    assert!(requests[1].query().contains("min_value=0.5"));
}

#[tokio::test]
async fn health_decodes_the_servers_own_shape() {
    let body = serde_json::to_string(&HealthStatus {
        status: "ok".to_owned(),
        version: "0.1.0".to_owned(),
        spec_version: "1.0.0".to_owned(),
        uptime_seconds: 42,
        storage: Some("sqlite".to_owned()),
        detail: None,
        signup: None,
        accounts: None,
        sso: None,
    })
    .unwrap();
    let server = FakeMcpd::scripted(vec![Reply::Body(200, body)]).await;
    let mcpd = client(server.endpoint());

    let health = mcpd.health().await.expect("the read should have succeeded");
    assert_eq!(health.status, "ok");
    assert_eq!(health.spec_version, "1.0.0");
    assert_eq!(server.requests()[0].path(), "/healthz");
}

#[tokio::test]
async fn a_refused_read_reports_what_mcpd_said() {
    let server = FakeMcpd::scripted(vec![Reply::Body(
        400,
        r#"{"error":"bad_parameter","message":"limit=\"x\" could not be read as a u32","field":"limit"}"#
            .to_owned(),
    )])
    .await;
    let mcpd = client(server.endpoint());

    let error = mcpd.list_traces(&TraceFilter::default()).await.expect_err("should have failed");
    let text = error.to_string();
    assert!(text.contains("400"), "{text}");
    assert!(text.contains("could not be read as a u32"), "{text}");
}

#[tokio::test]
async fn a_disabled_client_says_so_rather_than_pretending_to_read() {
    let mcpd = Mcpd::with_config(Config { enabled: false, ..Config::default() });
    let error = mcpd.health().await.expect_err("a disabled client cannot read");
    assert!(error.to_string().contains("disabled"), "{error}");
}
