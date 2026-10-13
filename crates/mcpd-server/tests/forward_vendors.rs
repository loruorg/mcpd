//! Every vendor preset, end to end, against a recorded expected payload.
//!
//! One fixed run goes in through the real ingest door: an agent, a model call
//! with a chat prompt, a link and a streamed event, and a tool call that
//! failed. Each vendor's destination points at a local server, and what that
//! server receives (the path, the content type, the headers and every span as
//! the vendor's decoder would read it) is compared with
//! `tests/fixtures/forward/<vendor>.json`. No vendor is contacted.
//!
//! A change to a mapping shows up as a diff in a fixture, which is the point:
//! the attribute names are what each vendor reads, and one renamed by accident
//! is a field that silently goes blank in someone's dashboard. To accept a
//! deliberate change, run with `MCPD_RECORD_FORWARD_FIXTURES=1` and read the
//! diff before committing it.

mod support;

use axum::http::StatusCode;
use mcpd_server::forward::{Destination, Settings, Vendor};
use mcpd_server::{router, AppState, ServerConfig};
use mcpd_store::sqlite::SqliteStore;
use mcpd_store::Store;
use serde_json::{json, Map, Value};
use std::sync::Arc;
use std::time::Duration;
use support::{post, temp_dir, Mock};

const TRACE: &str = "4bf92f3577b34da6a3ce929d0e0e4736";

fn run() -> Value {
    let common = |id: &str, parent: Option<&str>, name: &str, kind: &str| {
        let mut s = json!({
            "trace_id": TRACE,
            "span_id": id,
            "name": name,
            "kind": kind,
            "status": "ok",
            "start_time": "2026-03-01T12:00:00Z",
            "end_time": "2026-03-01T12:00:02Z",
            "is_root": parent.is_none(),
            "is_event": false,
            "correlation": {
                "project": "checkout",
                "environment": "production",
                "service_name": "support-agent",
                "service_version": "1.4.0",
                "session_id": "sess-7",
                "user_id": "user-42"
            },
            "received_at": "2026-03-01T12:00:03Z"
        });
        if let Some(p) = parent {
            s["parent_span_id"] = json!(p);
        }
        s
    };
    let mut agent = common("00000000000000a1", None, "answer ticket", "agent_run");
    agent["entity_name"] = json!("triage");
    agent["input"] = json!({ "ticket": "Where is my order?" });
    agent["output"] = json!("It ships tomorrow.");
    agent["tags"] = json!(["prod", "tier-1"]);
    agent["metadata"] = json!({ "plan": "pro" });

    let mut chat =
        common("00000000000000a2", Some("00000000000000a1"), "chat gpt-4o", "model_generation");
    chat["model"] = json!("gpt-4o");
    chat["provider"] = json!("openai");
    chat["usage"] = json!({ "input_tokens": 1200, "output_tokens": 64 });
    chat["model_parameters"] =
        json!({ "temperature": 0.2, "max_tokens": 256, "has_response_format": false });
    chat["input"] = json!([
        { "role": "system", "content": "Be brief." },
        { "role": "user", "content": "Where is my order?" }
    ]);
    chat["output"] = json!({ "role": "assistant", "content": "It ships tomorrow." });
    chat["links"] = json!([{
        "trace_id": "5bf92f3577b34da6a3ce929d0e0e4736",
        "span_id": "10f067aa0ba902b7",
        "kind": "retry_of"
    }]);

    let mut event =
        common("00000000000000e1", Some("00000000000000a2"), "first_token", "model_chunk");
    event["is_event"] = json!(true);
    event["status"] = json!("unset");
    event["start_time"] = json!("2026-03-01T12:00:00.4Z");
    event.as_object_mut().expect("an object").remove("end_time");

    let mut tool =
        common("00000000000000a3", Some("00000000000000a1"), "lookup order", "tool_call");
    tool["entity_name"] = json!("orders.lookup");
    tool["status"] = json!("error");
    tool["input"] = json!({ "order": "A-17" });
    tool["error"] = json!({ "message": "upstream timed out", "kind": "TimeoutError" });

    json!({ "spans": [agent, chat, event, tool] })
}

/// The minimum each vendor needs, pointed at the local server.
fn destination(v: Vendor, url: &str) -> Destination {
    let mut d = Destination::new(v, Some(url.to_owned()));
    d.api_key = Some("test-api-key".into());
    d.project = Some("support".into());
    match v {
        Vendor::Langfuse => {
            d.public_key = Some("pk-lf-test".into());
            d.secret_key = Some("sk-lf-test".into());
        }
        Vendor::Arize => d.space_id = Some("space-1".into()),
        Vendor::Grafana => d.instance_id = Some("123456".into()),
        Vendor::Opik => d.workspace = Some("team".into()),
        Vendor::Honeycomb => d.dataset = Some("agents".into()),
        Vendor::Sentry => {
            d.endpoint = None;
            d.dsn = Some(url.replacen("http://", "http://publickey@", 1) + "/42");
        }
        Vendor::Dynatrace | Vendor::Elastic => {
            d.token = Some("test-token".into());
            d.api_key = None;
        }
        _ => {}
    }
    d
}

/// Headers that depend on the client rather than on the mapping.
const INCIDENTAL: &[&str] = &[
    "host",
    "content-length",
    "accept",
    "accept-encoding",
    "user-agent",
    "te",
    "content-encoding",
];

fn spans_json(decoded: &[mcpd_core::otlp::OtlpSpan]) -> Vec<Value> {
    let mut out: Vec<Value> = decoded
        .iter()
        .map(|s| {
            let mut attrs = s.attributes.clone();
            // The cost comes from the built-in price book, which changes with
            // prices rather than with the mapping.
            attrs.retain(|k, _| !k.contains("cost"));
            json!({
                "name": s.name,
                "span_id": s.span_id,
                "parent_span_id": s.parent_span_id,
                "start": s.start_unix_nano.to_string(),
                "end": s.end_unix_nano.to_string(),
                "status": s.status_code,
                "status_message": s.status_message,
                "resource": s.resource,
                "attributes": attrs,
                "events": s.events.iter().map(|e| json!({ "name": e.name, "time": e.time_unix_nano.to_string() })).collect::<Vec<_>>(),
                "links": s.links.iter().map(|l| json!({ "trace_id": l.trace_id, "span_id": l.span_id, "attributes": l.attributes })).collect::<Vec<_>>(),
            })
        })
        .collect();
    out.sort_by(|a, b| a["span_id"].as_str().cmp(&b["span_id"].as_str()));
    out
}

async fn captured(v: Vendor) -> Value {
    let dest = Mock::start(&[]).await;
    let store = SqliteStore::open(temp_dir("vendor-db").join("mcpd.db"), 2).expect("open");
    store.migrate().await.expect("migrate");
    let config = ServerConfig {
        forward: Settings {
            destinations: vec![destination(v, &dest.url)],
            spool_dir: None,
            spool_max_bytes: 1 << 20,
        },
        ..ServerConfig::default()
    };
    let state = Arc::new(AppState::new(Arc::new(store) as Arc<dyn Store>, config));
    let (status, body) = post(
        &router(Arc::clone(&state)),
        "/v1/ingest",
        "application/json",
        &[],
        run().to_string().into_bytes(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let got = dest.wait_accepted(1, Duration::from_secs(10)).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let got = if got.len() < dest.accepted().len() { dest.accepted() } else { got };

    let requests: Vec<Value> = got
        .iter()
        .map(|seen| {
            let mut headers = Map::new();
            for (k, val) in &seen.headers {
                if !INCIDENTAL.contains(&k.as_str()) {
                    headers.insert(k.as_str().to_owned(), json!(val.to_str().unwrap_or("")));
                }
            }
            let ct = seen.header("content-type").unwrap_or("").to_owned();
            let payload = if ct == "application/json" && v == Vendor::Helicone {
                serde_json::from_slice::<Value>(&seen.body).expect("json")
            } else if ct == "application/json" {
                let v: Value = serde_json::from_slice(&seen.body).expect("json");
                json!(spans_json(&mcpd_core::otlp::decode_json(&v).expect("OTLP JSON")))
            } else {
                json!(spans_json(
                    &mcpd_core::otlp::protobuf::decode_traces(seen.payload()).expect("protobuf")
                ))
            };
            json!({ "method": seen.method.as_str(), "path": seen.path, "headers": headers, "payload": payload })
        })
        .collect();
    json!(requests)
}

fn fixture_path(v: Vendor) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/forward")
        .join(format!("{}.json", v.as_str()))
}

#[tokio::test]
async fn every_vendor_is_sent_its_recorded_payload() {
    let record = std::env::var("MCPD_RECORD_FORWARD_FIXTURES").is_ok();
    let mut wrong = Vec::new();
    for v in Vendor::ALL {
        let got = captured(*v).await;
        let path = fixture_path(*v);
        if record {
            std::fs::create_dir_all(path.parent().expect("a parent")).expect("mkdir");
            std::fs::write(&path, serde_json::to_string_pretty(&got).expect("json") + "\n")
                .expect("write the fixture");
            continue;
        }
        let want: Value = match std::fs::read_to_string(&path) {
            Ok(t) => serde_json::from_str(&t).expect("a fixture is JSON"),
            Err(_) => panic!(
                "no fixture at {}; run with MCPD_RECORD_FORWARD_FIXTURES=1 and review it",
                path.display()
            ),
        };
        if got != want {
            wrong.push(format!(
                "{}: got\n{}",
                v.as_str(),
                serde_json::to_string_pretty(&got).expect("json")
            ));
        }
    }
    assert!(wrong.is_empty(), "payloads differ from the fixtures:\n{}", wrong.join("\n\n"));
}

/// Spot checks that read as the vendor documents them, so a fixture recorded
/// wrong cannot pass unnoticed.
#[tokio::test]
async fn the_fields_each_vendor_reads_are_where_it_reads_them() {
    let first = |v: &Value| v[0].clone();
    let span = |req: &Value, id: &str| -> Value {
        req["payload"]
            .as_array()
            .expect("spans")
            .iter()
            .find(|s| s["span_id"] == id)
            .cloned()
            .expect("the span")
    };

    let lf = first(&captured(Vendor::Langfuse).await);
    assert_eq!(lf["path"], "/api/public/otel/v1/traces");
    assert!(lf["headers"]["authorization"].as_str().unwrap().starts_with("Basic "));
    let chat = span(&lf, "00000000000000a2");
    assert_eq!(chat["attributes"]["langfuse.observation.type"], "generation");
    assert_eq!(chat["attributes"]["langfuse.observation.model.name"], "gpt-4o");
    assert_eq!(span(&lf, "00000000000000a1")["attributes"]["langfuse.trace.name"], "answer ticket");

    let ax = first(&captured(Vendor::Arize).await);
    assert_eq!(ax["headers"]["space_id"], "space-1");
    let chat = span(&ax, "00000000000000a2");
    assert_eq!(chat["attributes"]["openinference.span.kind"], "LLM");
    assert_eq!(chat["attributes"]["llm.token_count.prompt"], 1200);
    assert_eq!(chat["attributes"]["llm.input_messages.1.message.content"], "Where is my order?");
    assert_eq!(chat["resource"]["openinference.project.name"], "support");

    let dd = first(&captured(Vendor::Datadog).await);
    assert_eq!(dd["headers"]["dd-otlp-source"], "llmobs");
    let msgs = span(&dd, "00000000000000a2")["attributes"]["gen_ai.input.messages"].clone();
    assert!(msgs.as_str().unwrap().contains("\"parts\""), "{msgs}");

    let hc = captured(Vendor::Helicone).await;
    assert_eq!(hc.as_array().unwrap().len(), 1, "one model call, one log");
    assert_eq!(hc[0]["path"], "/custom/v1/log");
    assert_eq!(hc[0]["payload"]["providerResponse"]["json"]["usage"]["prompt_tokens"], 1200);

    let sentry = first(&captured(Vendor::Sentry).await);
    assert_eq!(sentry["path"], "/api/42/integration/otlp/v1/traces");
    assert_eq!(sentry["headers"]["x-sentry-auth"], "sentry sentry_key=publickey");

    let dt = first(&captured(Vendor::Dynatrace).await);
    assert_eq!(dt["path"], "/api/v2/otlp/v1/traces");
    assert_eq!(dt["headers"]["content-type"], "application/x-protobuf");

    let generic = first(&captured(Vendor::Otlp).await);
    let chat = span(&generic, "00000000000000a2");
    assert_eq!(chat["events"][0]["name"], "first_token");
    assert_eq!(chat["links"][0]["attributes"]["mcpd.link.kind"], "retry_of");
    assert_eq!(chat["resource"]["mcpd.project"], "checkout");
}
