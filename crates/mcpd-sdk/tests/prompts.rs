//! Prompts (contract section 5.1): which version is fetched, what the cache
//! serves when the daemon fails, the fallback, the two write calls, and the
//! span link. The list filters of section 5 are checked at the end.

mod common;

use common::{FakeMcpd, Reply};
use mcpd_sdk::{
    Config, CreatePromptRequest, Error, GetPrompt, Mcpd, Prompt, PromptFallback, PromptKind,
    RecordingSink, SpanFilter, TraceFilter,
};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

fn client(endpoint: String) -> Mcpd {
    Mcpd::with_config(Config { endpoint, project: "shop".to_owned(), ..Config::default() })
}

fn stored(name: &str, version: u32, template: &str) -> String {
    json!({
        "project": "shop",
        "name": name,
        "version": version,
        "kind": "text",
        "template": template,
        "variables": [],
        "labels": [],
        "created_at": "2026-10-01T00:00:00Z",
    })
    .to_string()
}

const NAME: &str = "support/triage";
const PATH: &str = "/v1/api/prompts/support%2Ftriage";

#[tokio::test]
async fn the_default_label_falls_back_to_the_latest_version_on_a_404() {
    let server = FakeMcpd::scripted(vec![
        Reply::Status(404, None),
        Reply::Body(200, stored(NAME, 3, "latest")),
    ])
    .await;
    let mcpd = client(server.endpoint());

    let prompt = mcpd.get_prompt(NAME, &GetPrompt::new()).await.expect("the latest is served");
    assert_eq!(prompt.version, 3);

    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].path(), PATH);
    assert_eq!(requests[0].query(), "project=shop&label=production");
    assert_eq!(requests[1].path(), PATH);
    assert_eq!(requests[1].query(), "project=shop");
}

#[tokio::test]
async fn a_label_or_a_version_is_sent_as_asked() {
    let server = FakeMcpd::scripted(vec![
        Reply::Body(200, stored(NAME, 2, "a")),
        Reply::Body(200, stored(NAME, 5, "b")),
    ])
    .await;
    let mcpd = client(server.endpoint());

    mcpd.get_prompt(NAME, &GetPrompt::new().label("staging")).await.unwrap();
    mcpd.get_prompt(NAME, &GetPrompt::new().version(5)).await.unwrap();

    let requests = server.requests();
    assert_eq!(requests[0].query(), "project=shop&label=staging");
    assert_eq!(requests[1].query(), "project=shop&version=5");
}

#[tokio::test]
async fn both_selectors_are_refused_before_the_wire() {
    let server = FakeMcpd::start().await;
    let mcpd = client(server.endpoint());

    let err = mcpd
        .get_prompt(NAME, &GetPrompt::new().label("production").version(2))
        .await
        .expect_err("a label and a version together are ambiguous");
    assert!(matches!(err, Error::Config(_)), "{err}");
    assert_eq!(server.count(), 0, "nothing should have reached the wire");
}

#[tokio::test]
async fn a_fresh_copy_is_served_with_no_request() {
    let server = FakeMcpd::scripted(vec![Reply::Body(200, stored(NAME, 1, "one"))]).await;
    let mcpd = client(server.endpoint());

    let first = mcpd.get_prompt(NAME, &GetPrompt::new().label("production")).await.unwrap();
    let second = mcpd.get_prompt(NAME, &GetPrompt::new().label("production")).await.unwrap();
    assert_eq!(first, second);
    assert_eq!(server.count(), 1, "the second call should have been answered from memory");

    // Another selector is another entry, so it is fetched.
    let _ = mcpd.get_prompt(NAME, &GetPrompt::new().version(1)).await;
    assert_eq!(server.count(), 2);
}

#[tokio::test]
async fn a_stale_copy_is_served_when_the_refetch_fails() {
    let server = FakeMcpd::scripted(vec![
        Reply::Body(200, stored(NAME, 4, "kept")),
        Reply::Status(503, None),
        Reply::Status(404, None),
    ])
    .await;
    let mcpd = client(server.endpoint());
    let options = GetPrompt::new().label("production").cache_ttl(Duration::ZERO);

    let first = mcpd.get_prompt(NAME, &options).await.unwrap();
    let after_5xx = mcpd.get_prompt(NAME, &options).await.expect("the copy survives a 503");
    let after_404 = mcpd.get_prompt(NAME, &options).await.expect("the copy survives a 404");
    assert_eq!(after_5xx, first);
    assert_eq!(after_404, first);
    assert_eq!(server.count(), 3, "a zero ttl fetches every time");
}

#[tokio::test]
async fn a_stale_copy_is_served_when_the_refetch_times_out() {
    let server = FakeMcpd::scripted(vec![
        Reply::Body(200, stored(NAME, 4, "kept")),
        Reply::After(2_000, 200, stored(NAME, 9, "late")),
    ])
    .await;
    let mcpd = Mcpd::with_config(Config {
        endpoint: server.endpoint(),
        project: "shop".to_owned(),
        timeout: Duration::from_millis(200),
        ..Config::default()
    });
    let options = GetPrompt::new().label("production").cache_ttl(Duration::ZERO);

    mcpd.get_prompt(NAME, &options).await.unwrap();
    let stale = mcpd.get_prompt(NAME, &options).await.expect("the copy survives a timeout");
    assert_eq!(stale.version, 4);
}

#[tokio::test]
async fn a_fallback_is_served_when_nothing_was_fetched_and_never_cached() {
    let server = FakeMcpd::scripted(vec![
        Reply::Status(500, None),
        Reply::Body(200, stored(NAME, 7, "real")),
    ])
    .await;
    let mcpd = client(server.endpoint());
    let options = GetPrompt::new()
        .label("production")
        .fallback(PromptFallback::Template("Answer {{ question }} for {{user}}".into()));

    let fallback = mcpd.get_prompt(NAME, &options).await.expect("the fallback is served");
    assert_eq!(fallback.version, 0);
    assert_eq!(fallback.name, NAME);
    assert_eq!(fallback.project, "shop");
    assert_eq!(fallback.kind, PromptKind::Text);
    assert!(fallback.labels.is_empty());
    assert_eq!(fallback.variables, ["question", "user"]);

    let real = mcpd.get_prompt(NAME, &options).await.unwrap();
    assert_eq!(real.version, 7, "a fallback must not be cached");
    assert_eq!(server.count(), 2);
}

#[tokio::test]
async fn a_chat_fallback_is_a_chat_prompt() {
    let server = FakeMcpd::scripted(vec![Reply::Status(503, None)]).await;
    let mcpd = client(server.endpoint());
    let messages = vec![json!({ "role": "user", "content": "{{q}}" })];
    let options = GetPrompt::new().fallback(PromptFallback::Messages(messages.clone()));

    let prompt = mcpd.get_prompt(NAME, &options).await.unwrap();
    assert_eq!(prompt.kind, PromptKind::Chat);
    assert_eq!(prompt.messages, Some(json!(messages)));
    assert_eq!(prompt.variables, ["q"]);
}

#[tokio::test]
async fn with_no_copy_and_no_fallback_the_fetch_error_is_returned() {
    let server = FakeMcpd::scripted(vec![Reply::Status(503, None)]).await;
    let mcpd = client(server.endpoint());

    let err = mcpd.get_prompt(NAME, &GetPrompt::new().label("production")).await.unwrap_err();
    assert!(matches!(err, Error::Api { status: 503, .. }), "{err}");
}

#[tokio::test]
async fn a_disabled_client_still_fetches_its_prompts() {
    let server = FakeMcpd::scripted(vec![Reply::Body(200, stored(NAME, 1, "x"))]).await;
    let mcpd = Mcpd::with_config(Config {
        endpoint: server.endpoint(),
        project: "shop".to_owned(),
        enabled: false,
        ..Config::default()
    });

    let prompt = mcpd.get_prompt(NAME, &GetPrompt::new().label("production")).await;
    assert_eq!(prompt.expect("a prompt is input, not telemetry").version, 1);
}

#[tokio::test]
async fn create_prompt_posts_the_request_to_the_clients_project() {
    let server = FakeMcpd::scripted(vec![Reply::Body(200, stored(NAME, 1, "Hi {{name}}"))]).await;
    let mcpd = client(server.endpoint());

    let prompt = mcpd
        .create_prompt(&CreatePromptRequest {
            name: NAME.to_owned(),
            kind: None,
            template: Some("Hi {{name}}".to_owned()),
            messages: None,
            config: None,
            note: Some("first".to_owned()),
            labels: Some(vec!["production".to_owned()]),
        })
        .await
        .unwrap();
    assert_eq!(prompt.version, 1);

    let request = &server.requests()[0];
    assert_eq!(request.method, "POST");
    assert_eq!(request.path(), "/v1/api/prompts");
    assert_eq!(request.query(), "project=shop");
    assert_eq!(
        request.json(),
        json!({
            "name": NAME,
            "template": "Hi {{name}}",
            "note": "first",
            "labels": ["production"],
        })
    );
}

#[tokio::test]
async fn set_prompt_label_puts_the_version_at_an_escaped_path() {
    let server =
        FakeMcpd::scripted(vec![Reply::Body(200, stored(NAME, 2, "x")), Reply::Status(404, None)])
            .await;
    let mcpd = client(server.endpoint());

    let prompt = mcpd.set_prompt_label(NAME, "production", 2).await.unwrap();
    assert_eq!(prompt.version, 2);
    let err = mcpd.set_prompt_label(NAME, "production", 99).await.unwrap_err();
    assert!(matches!(err, Error::Api { status: 404, .. }), "{err}");

    let request = &server.requests()[0];
    assert_eq!(request.method, "PUT");
    assert_eq!(request.path(), "/v1/api/prompts/support%2Ftriage/labels/production");
    assert_eq!(request.query(), "project=shop");
    assert_eq!(request.json(), json!({ "version": 2 }));
}

fn a_prompt(name: &str, version: u32) -> Prompt {
    serde_json::from_str(&stored(name, version, "t")).unwrap()
}

#[tokio::test]
async fn set_prompt_links_a_span_by_name_and_version() {
    let sink = Arc::new(RecordingSink::new());
    let mcpd = Mcpd::with_config(
        Config {
            endpoint: "http://127.0.0.1:9".to_owned(),
            project: "shop".to_owned(),
            flush_interval: Duration::from_secs(3600),
            ..Config::default()
        }
        .with_sink(sink.clone()),
    );

    let stored_one = mcpd.trace("stored");
    stored_one.set_attributes(json!({ "kept": true }));
    stored_one.set_prompt(&a_prompt(NAME, 3));
    stored_one.end();

    let fallback = mcpd.trace("fallback");
    fallback.set_prompt(&a_prompt(NAME, 0));
    fallback.end();

    // Nothing to link: no name, and a span that has ended. Neither panics.
    let nameless = mcpd.trace("nameless");
    nameless.set_prompt(&a_prompt("", 2));
    nameless.end();
    nameless.set_prompt(&a_prompt(NAME, 2));

    mcpd.flush().await.unwrap();
    let spans = sink.spans();
    let attrs = |name: &str| {
        spans.iter().rev().find(|s| s.name == name).expect("span recorded").attributes.clone()
    };
    assert_eq!(
        attrs("stored"),
        Some(json!({ "kept": true, "mcpd.prompt.name": NAME, "mcpd.prompt.version": 3 }))
    );
    assert_eq!(attrs("fallback"), Some(json!({ "mcpd.prompt.name": NAME })));
    assert_eq!(attrs("nameless"), None);
}

#[tokio::test]
async fn list_traces_sends_the_new_filters_and_nothing_else() {
    let server = FakeMcpd::scripted(vec![Reply::Body(
        200,
        r#"{"traces":[],"page":{"has_more":false}}"#.to_owned(),
    )])
    .await;
    let mcpd = client(server.endpoint());

    mcpd.list_traces(&TraceFilter {
        max_duration_ms: Some(5000),
        max_cost: Some(0.25),
        min_tokens: Some(1000),
        has_score: Some("helpfulness".to_owned()),
        score_above: Some(0.5),
        has_feedback: Some(false),
        ..TraceFilter::default()
    })
    .await
    .unwrap();

    assert_eq!(
        server.requests()[0].query(),
        "project=shop&max_duration_ms=5000&max_cost=0.25&min_tokens=1000&has_score=helpfulness\
         &score_above=0.5&has_feedback=false"
    );
}

#[tokio::test]
async fn list_spans_sends_the_prompt_filters_and_nothing_else() {
    let server = FakeMcpd::scripted(vec![Reply::Body(
        200,
        r#"{"spans":[],"page":{"has_more":false}}"#.to_owned(),
    )])
    .await;
    let mcpd = client(server.endpoint());

    mcpd.list_spans(&SpanFilter {
        prompt: Some(NAME.to_owned()),
        prompt_version: Some(3),
        ..SpanFilter::default()
    })
    .await
    .unwrap();

    let request = &server.requests()[0];
    assert_eq!(request.path(), "/v1/api/spans");
    assert_eq!(request.query(), "project=shop&prompt=support%2Ftriage&prompt_version=3");
}
