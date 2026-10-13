//! Every wire protocol mcpd takes, end to end.
//!
//! Each test sends what a stock sender sends: the OTLP services through the
//! generated tonic client, the Zipkin and Jaeger protobuf messages through
//! prost, and Thrift through a writer of the binary protocol. Each reads the
//! result back from the store, because the response is what the handler
//! believed and the row is what landed.
//!
//! The gRPC tests run the router on a real socket. A gRPC client speaks
//! cleartext HTTP/2, and only the listener can show that the port takes it.

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use mcpd_server::accounts::AccountStore;
use mcpd_server::{router, AppState, ServerConfig};
use mcpd_store::sqlite::SqliteStore;
use mcpd_store::{Include, LogQuery, Store};
use opentelemetry_proto::tonic::collector::logs::v1::logs_service_client::LogsServiceClient;
use opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceRequest;
use opentelemetry_proto::tonic::collector::metrics::v1::metrics_service_client::MetricsServiceClient;
use opentelemetry_proto::tonic::collector::metrics::v1::ExportMetricsServiceRequest;
use opentelemetry_proto::tonic::collector::trace::v1::trace_service_client::TraceServiceClient;
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use opentelemetry_proto::tonic::common::v1::{any_value, AnyValue, KeyValue};
use opentelemetry_proto::tonic::logs::v1::{LogRecord, ResourceLogs, ScopeLogs};
use opentelemetry_proto::tonic::metrics::v1::{
    metric, number_data_point, Metric, NumberDataPoint, ResourceMetrics, ScopeMetrics, Sum,
};
use opentelemetry_proto::tonic::resource::v1::Resource;
use opentelemetry_proto::tonic::trace::v1::{span, ResourceSpans, ScopeSpans, Span};
use prost::Message;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Arc;
use tonic::codec::CompressionEncoding;
use tonic::transport::Channel;
use tonic::Code;
use tower::ServiceExt;

const TRACE: [u8; 16] = [0x4b; 16];
const TRACE_HEX: &str = "4b4b4b4b4b4b4b4b4b4b4b4b4b4b4b4b";
const SPAN: [u8; 8] = [0x01; 8];
const SPAN_HEX: &str = "0101010101010101";
const LINKED_TRACE: [u8; 16] = [0x9c; 16];
const LINKED_TRACE_HEX: &str = "9c9c9c9c9c9c9c9c9c9c9c9c9c9c9c9c";
const INGEST_KEY: &str = "wire-protocols-ingest-key";

// ---------------------------------------------------------------- setup ---

/// A private directory per test, named from the pid and the instant the
/// process started, for the reason `http.rs` gives.
fn temp_dir() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::OnceLock;
    static N: AtomicU64 = AtomicU64::new(0);
    static ROOT: OnceLock<std::path::PathBuf> = OnceLock::new();
    let root = ROOT.get_or_init(|| {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        std::env::temp_dir().join(format!("mcpd-wire-test-{}-{unique}", std::process::id()))
    });
    let dir = root.join(N.fetch_add(1, Ordering::Relaxed).to_string());
    std::fs::create_dir_all(&dir).expect("create the test directory");
    dir
}

struct Daemon {
    app: Router,
    store: Arc<SqliteStore>,
}

async fn daemon(config: ServerConfig) -> Daemon {
    let dir = temp_dir();
    let store = Arc::new(SqliteStore::open(dir.join("signals.db"), 2).expect("open the store"));
    store.migrate().await.expect("migrate the store");
    let mut state = AppState::new(Arc::clone(&store) as Arc<dyn Store>, config.clone());
    if config.signup {
        let accounts = AccountStore::open(dir.join("accounts.db").to_str().expect("utf-8 path"))
            .expect("open the account store");
        state = state.with_account_store(Arc::new(accounts));
    }
    Daemon { app: router(Arc::new(state)), store }
}

/// Serve the router on a port the operating system assigns, and connect a
/// gRPC channel to it.
async fn channel(app: &Router) -> Channel {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local address");
    let app = app.clone();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
            .await
            .expect("serve");
    });
    Channel::from_shared(format!("http://{addr}"))
        .expect("a valid endpoint")
        .connect()
        .await
        .expect("connect over HTTP/2")
}

struct Reply {
    status: StatusCode,
    content_type: String,
    body: Vec<u8>,
}

impl Reply {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or_else(|_| {
            panic!("not JSON ({}): {}", self.status, String::from_utf8_lossy(&self.body))
        })
    }
}

async fn post(
    app: &Router,
    path: &str,
    content_type: &str,
    body: Vec<u8>,
    headers: &[(&str, &str)],
) -> Reply {
    let mut req = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", content_type)
        .header("content-length", body.len().to_string());
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let mut req = req.body(Body::from(body)).expect("build the request");
    req.extensions_mut().insert(ConnectInfo("192.0.2.10:4000".parse::<SocketAddr>().unwrap()));
    let res = app.clone().oneshot(req).await.expect("route the request");
    let status = res.status();
    let content_type =
        res.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").to_owned();
    let body = res.into_body().collect().await.expect("read the body").to_bytes().to_vec();
    Reply { status, content_type, body }
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(bytes).expect("compress");
    e.finish().expect("finish compressing")
}

fn with_key<T>(message: T, key: &str) -> tonic::Request<T> {
    let mut req = tonic::Request::new(message);
    req.metadata_mut()
        .insert("authorization", format!("Bearer {key}").parse().expect("a metadata value"));
    req
}

async fn spans_of(store: &SqliteStore, project: &str, trace: &str) -> Vec<mcpd_core::Span> {
    match store.get_trace(project, trace, &Include::default()).await {
        Ok(t) => t.spans,
        Err(_) => Vec::new(),
    }
}

async fn logs_of(store: &SqliteStore, project: &str) -> Vec<mcpd_core::LogRecord> {
    store
        .list_logs(&LogQuery { project: project.to_owned(), limit: 100, ..LogQuery::default() })
        .await
        .expect("list logs")
        .items
}

// -------------------------------------------------------- OTLP messages ---

fn kv(key: &str, value: &str) -> KeyValue {
    KeyValue {
        key: key.to_owned(),
        value: Some(AnyValue { value: Some(any_value::Value::StringValue(value.to_owned())) }),
        ..KeyValue::default()
    }
}

fn resource(attributes: Vec<KeyValue>) -> Option<Resource> {
    Some(Resource { attributes, ..Resource::default() })
}

/// One span with a link and an event, under a service.
fn trace_request(extra_resource: Vec<KeyValue>) -> ExportTraceServiceRequest {
    let mut attrs = vec![kv("service.name", "checkout-agent")];
    attrs.extend(extra_resource);
    ExportTraceServiceRequest {
        resource_spans: vec![ResourceSpans {
            resource: resource(attrs),
            scope_spans: vec![ScopeSpans {
                spans: vec![Span {
                    trace_id: TRACE.to_vec(),
                    span_id: SPAN.to_vec(),
                    name: "chat gpt-4o".to_owned(),
                    start_time_unix_nano: 1_767_225_600_000_000_000,
                    end_time_unix_nano: 1_767_225_601_000_000_000,
                    attributes: vec![kv("gen_ai.system", "openai")],
                    links: vec![span::Link {
                        trace_id: LINKED_TRACE.to_vec(),
                        span_id: [0x07; 8].to_vec(),
                        attributes: vec![kv("mcpd.link.kind", "retry_of")],
                        ..span::Link::default()
                    }],
                    events: vec![span::Event {
                        time_unix_nano: 1_767_225_600_500_000_000,
                        name: "retry".to_owned(),
                        ..span::Event::default()
                    }],
                    ..Span::default()
                }],
                ..ScopeSpans::default()
            }],
            ..ResourceSpans::default()
        }],
    }
}

fn logs_request() -> ExportLogsServiceRequest {
    ExportLogsServiceRequest {
        resource_logs: vec![ResourceLogs {
            resource: resource(vec![kv("service.name", "checkout-agent")]),
            scope_logs: vec![ScopeLogs {
                log_records: vec![LogRecord {
                    time_unix_nano: 1_767_225_600_000_000_000,
                    severity_number: 17,
                    body: Some(AnyValue {
                        value: Some(any_value::Value::StringValue("provider refused".to_owned())),
                    }),
                    attributes: vec![kv("attempt", "2")],
                    trace_id: TRACE.to_vec(),
                    span_id: SPAN.to_vec(),
                    ..LogRecord::default()
                }],
                ..ScopeLogs::default()
            }],
            ..ResourceLogs::default()
        }],
    }
}

fn metrics_request() -> ExportMetricsServiceRequest {
    ExportMetricsServiceRequest {
        resource_metrics: vec![ResourceMetrics {
            scope_metrics: vec![ScopeMetrics {
                metrics: vec![Metric {
                    name: "tokens".to_owned(),
                    unit: "1".to_owned(),
                    data: Some(metric::Data::Sum(Sum {
                        data_points: vec![NumberDataPoint {
                            time_unix_nano: 1_767_225_600_000_000_000,
                            value: Some(number_data_point::Value::AsInt(1284)),
                            attributes: vec![kv("model", "gpt-4o")],
                            ..NumberDataPoint::default()
                        }],
                        is_monotonic: true,
                        ..Sum::default()
                    })),
                    ..Metric::default()
                }],
                ..ScopeMetrics::default()
            }],
            ..ResourceMetrics::default()
        }],
    }
}

/// The span the trace request carries, and the event span beside it.
fn assert_stored_span(spans: &[mcpd_core::Span]) {
    assert_eq!(spans.len(), 2, "the span and its event: {spans:#?}");
    let s = spans.iter().find(|s| s.span_id == SPAN_HEX).expect("the span itself");
    assert_eq!(s.correlation.service_name.as_deref(), Some("checkout-agent"));
    assert_eq!(s.kind, mcpd_core::SpanKind::ModelGeneration);
    let links = s.links.as_ref().expect("the link survived");
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].trace_id, LINKED_TRACE_HEX);
    assert_eq!(links[0].span_id.as_deref(), Some("0707070707070707"));
    assert_eq!(links[0].kind.as_deref(), Some("retry_of"));
    let event = spans.iter().find(|s| s.is_event).expect("the event span");
    assert_eq!(event.name, "retry");
    assert_eq!(event.parent_span_id.as_deref(), Some(SPAN_HEX));
}

// ------------------------------------------------------------ OTLP gRPC ---

#[tokio::test]
async fn otlp_grpc_traces_store_the_span_its_link_and_its_event() {
    let d = daemon(ServerConfig::default()).await;
    let mut client = TraceServiceClient::new(channel(&d.app).await);
    let res = client.export(trace_request(Vec::new())).await.expect("export over gRPC");
    assert!(res.get_ref().partial_success.is_none(), "a full success leaves partial_success unset");
    assert_stored_span(&spans_of(&d.store, "default", TRACE_HEX).await);
}

#[tokio::test]
async fn otlp_grpc_takes_gzip_and_a_resend_stores_nothing_twice() {
    let d = daemon(ServerConfig::default()).await;
    let ch = channel(&d.app).await;
    let mut traces = TraceServiceClient::new(ch.clone())
        .send_compressed(CompressionEncoding::Gzip)
        .accept_compressed(CompressionEncoding::Gzip);
    let mut logs = LogsServiceClient::new(ch).send_compressed(CompressionEncoding::Gzip);
    for _ in 0..2 {
        traces.export(trace_request(Vec::new())).await.expect("gzip trace export");
        logs.export(logs_request()).await.expect("gzip log export");
    }
    assert_stored_span(&spans_of(&d.store, "default", TRACE_HEX).await);
    assert_eq!(logs_of(&d.store, "default").await.len(), 1, "the resent log is one row");
}

#[tokio::test]
async fn otlp_grpc_logs_and_metrics_are_stored() {
    let d = daemon(ServerConfig::default()).await;
    let ch = channel(&d.app).await;
    LogsServiceClient::new(ch.clone()).export(logs_request()).await.expect("log export");
    MetricsServiceClient::new(ch).export(metrics_request()).await.expect("metric export");

    let logs = logs_of(&d.store, "default").await;
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].message, "provider refused");
    assert_eq!(logs[0].level, mcpd_core::LogLevel::Error);
    assert_eq!(logs[0].trace_id.as_deref(), Some(TRACE_HEX));
    assert_eq!(logs[0].correlation.service_name.as_deref(), Some("checkout-agent"));

    // Metrics read back through the query API, which is where they are used.
    let req = Request::builder()
        .uri("/v1/api/metrics?project=default&name=tokens&from=2025-12-31T00:00:00Z&to=2026-01-02T00:00:00Z&agg=sum")
        .body(Body::empty())
        .unwrap();
    let res = d.app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert!(body.to_string().contains("1284"), "the point is stored: {body}");
}

#[tokio::test]
async fn otlp_grpc_refuses_a_missing_or_unknown_key() {
    let d = daemon(ServerConfig {
        ingest_keys: vec![INGEST_KEY.to_owned()],
        ..ServerConfig::default()
    })
    .await;
    let mut client = TraceServiceClient::new(channel(&d.app).await);

    let none = client.export(trace_request(Vec::new())).await.expect_err("no key");
    assert_eq!(none.code(), Code::Unauthenticated, "{none:?}");
    assert!(none.message().contains("API key"), "{none:?}");
    let wrong = client.export(with_key(trace_request(Vec::new()), "nope")).await.expect_err("bad");
    assert_eq!(wrong.code(), Code::Unauthenticated);
    assert!(spans_of(&d.store, "default", TRACE_HEX).await.is_empty(), "nothing was stored");

    client.export(with_key(trace_request(Vec::new()), INGEST_KEY)).await.expect("the right key");
    assert_stored_span(&spans_of(&d.store, "default", TRACE_HEX).await);
}

#[tokio::test]
async fn otlp_grpc_pins_an_account_key_to_its_own_project() {
    let outbox = mcpd_server::mail::Outbox::default();
    let mut config = ServerConfig { signup: true, ..ServerConfig::default() };
    config.account_settings.mail = mcpd_server::mail::MailConfig::Memory(outbox.clone());
    let d = daemon(config).await;
    let signup = post(
        &d.app,
        "/v1/api/account/signup",
        "application/json",
        json!({ "email": "ana@example.com", "password": "a long enough password" })
            .to_string()
            .into_bytes(),
        &[],
    )
    .await;
    assert_eq!(signup.status, StatusCode::OK, "{}", String::from_utf8_lossy(&signup.body));
    // Sign-up mails a confirmation link; confirming it is what opens a session.
    let mail = outbox.last_to("ana@example.com").expect("a confirmation mail");
    let at = mail.body.find("#verify=").expect("a verify link");
    let link_token = mail.body[at + "#verify=".len()..].split_whitespace().next().unwrap();
    let verified = post(
        &d.app,
        "/v1/api/account/verify",
        "application/json",
        json!({ "token": link_token }).to_string().into_bytes(),
        &[],
    )
    .await;
    assert_eq!(verified.status, StatusCode::OK, "{}", String::from_utf8_lossy(&verified.body));
    let who = verified.json();
    let token = who["token"].as_str().unwrap().to_owned();
    let project = who["project"].as_str().unwrap().to_owned();
    let minted = post(
        &d.app,
        "/v1/api/account/keys",
        "application/json",
        json!({ "scope": "ingest" }).to_string().into_bytes(),
        &[("authorization", &format!("Bearer {token}"))],
    )
    .await;
    assert_eq!(minted.status, StatusCode::OK);
    let key = minted.json()["secret"].as_str().unwrap().to_owned();

    let mut client = TraceServiceClient::new(channel(&d.app).await);
    // With sign-up on, every ingest call needs a key, configured or not.
    let refused = client.export(trace_request(Vec::new())).await.expect_err("no key");
    assert_eq!(refused.code(), Code::Unauthenticated);

    // The export names another project. The account's key puts it in its own.
    let elsewhere = vec![kv("mcpd.project", "someone-else")];
    client.export(with_key(trace_request(elsewhere), &key)).await.expect("export");
    assert_stored_span(&spans_of(&d.store, &project, TRACE_HEX).await);
    assert!(spans_of(&d.store, "someone-else", TRACE_HEX).await.is_empty());
}

#[tokio::test]
async fn an_unknown_grpc_method_is_unimplemented() {
    let d = daemon(ServerConfig::default()).await;
    let mut grpc = tonic::client::Grpc::new(channel(&d.app).await);
    grpc.ready().await.expect("ready");
    let codec =
        tonic_prost::ProstCodec::<ExportTraceServiceRequest, ExportTraceServiceRequest>::default();
    let path = tonic::codegen::http::uri::PathAndQuery::from_static("/some.Service/Method");
    let err = grpc
        .unary(tonic::Request::new(ExportTraceServiceRequest::default()), path, codec)
        .await
        .expect_err("not served");
    assert_eq!(err.code(), Code::Unimplemented, "{err:?}");
}

// ------------------------------------------------------ OTLP/HTTP proto ---

#[tokio::test]
async fn otlp_http_takes_protobuf_on_every_signal_and_answers_in_kind() {
    let d = daemon(ServerConfig::default()).await;
    for (path, body) in [
        ("/v1/traces", trace_request(Vec::new()).encode_to_vec()),
        ("/v1/logs", logs_request().encode_to_vec()),
        ("/v1/metrics", metrics_request().encode_to_vec()),
    ] {
        let plain = post(&d.app, path, "application/x-protobuf", body.clone(), &[]).await;
        assert_eq!(
            plain.status,
            StatusCode::OK,
            "{path}: {}",
            String::from_utf8_lossy(&plain.body)
        );
        assert_eq!(plain.content_type, "application/x-protobuf", "{path}");
        assert!(plain.body.is_empty(), "{path}: a full success is an empty response message");

        let zipped = post(
            &d.app,
            path,
            "application/x-protobuf",
            gzip(&body),
            &[("content-encoding", "gzip")],
        )
        .await;
        assert_eq!(zipped.status, StatusCode::OK, "{path} gzip");
    }
    assert_stored_span(&spans_of(&d.store, "default", TRACE_HEX).await);
    let logs = logs_of(&d.store, "default").await;
    assert_eq!(logs.len(), 1, "the gzip resend is the same record");
    assert_eq!(logs[0].message, "provider refused");
}

#[tokio::test]
async fn otlp_http_protobuf_and_json_logs_store_the_same_record() {
    let pb = daemon(ServerConfig::default()).await;
    post(&pb.app, "/v1/logs", "application/x-protobuf", logs_request().encode_to_vec(), &[]).await;
    let js = daemon(ServerConfig::default()).await;
    let json_body = json!({ "resourceLogs": [{
        "resource": { "attributes": [
            { "key": "service.name", "value": { "stringValue": "checkout-agent" } }
        ]},
        "scopeLogs": [{ "logRecords": [{
            "timeUnixNano": "1767225600000000000",
            "severityNumber": 17,
            "body": { "stringValue": "provider refused" },
            "attributes": [ { "key": "attempt", "value": { "stringValue": "2" } } ],
            "traceId": TRACE_HEX,
            "spanId": SPAN_HEX
        }]}]
    }]});
    post(&js.app, "/v1/logs", "application/json", json_body.to_string().into_bytes(), &[]).await;
    let (a, b) = (logs_of(&pb.store, "default").await, logs_of(&js.store, "default").await);
    assert_eq!(a.len(), 1);
    assert_eq!(a[0].log_id, b[0].log_id, "the same export derives the same id either way");
    assert_eq!(a[0].fields, b[0].fields);
}

// --------------------------------------------------------------- Zipkin ---

fn zipkin_json() -> Value {
    json!([
        {
            "traceId": TRACE_HEX,
            "id": SPAN_HEX,
            "kind": "CLIENT",
            "name": "chat gpt-4o",
            "timestamp": 1_767_225_600_000_000u64,
            "duration": 1_000_000,
            "localEndpoint": { "serviceName": "checkout-agent", "ipv4": "10.0.0.1" },
            "remoteEndpoint": { "serviceName": "openai", "port": 443 },
            "annotations": [{ "timestamp": 1_767_225_600_500_000u64, "value": "retry" }],
            "tags": { "gen_ai.system": "openai", "error": "the provider refused" }
        },
        {
            "traceId": TRACE_HEX,
            "id": SPAN_HEX,
            "kind": "SERVER",
            "shared": true,
            "name": "chat",
            "timestamp": 1_767_225_600_100_000u64,
            "duration": 800_000,
            "localEndpoint": { "serviceName": "model-gateway" }
        }
    ])
}

#[tokio::test]
async fn zipkin_json_spans_are_stored_faithfully() {
    let d = daemon(ServerConfig::default()).await;
    let res = post(
        &d.app,
        "/api/v2/spans",
        "application/json",
        zipkin_json().to_string().into_bytes(),
        &[],
    )
    .await;
    assert_eq!(res.status, StatusCode::ACCEPTED, "{}", String::from_utf8_lossy(&res.body));
    assert_eq!(res.json()["accepted"], json!(3), "two spans and one annotation");

    let spans = spans_of(&d.store, "default", TRACE_HEX).await;
    assert_eq!(spans.len(), 3, "{spans:#?}");
    let client = spans.iter().find(|s| s.span_id == SPAN_HEX).expect("the client span");
    assert_eq!(client.correlation.service_name.as_deref(), Some("checkout-agent"));
    assert_eq!(client.kind, mcpd_core::SpanKind::ModelGeneration);
    assert_eq!(client.status, mcpd_core::SpanStatus::Error);
    assert_eq!(client.error.as_ref().unwrap().message, "the provider refused");
    let attrs = client.attributes.as_ref().unwrap();
    assert_eq!(attrs["span.kind"], "client");
    assert_eq!(attrs["peer.service"], "openai");
    assert_eq!(client.duration_ns, Some(1_000_000_000));

    let server = spans
        .iter()
        .find(|s| s.correlation.service_name.as_deref() == Some("model-gateway"))
        .expect("the shared server half is kept beside the client half");
    assert_ne!(server.span_id, SPAN_HEX);
    assert_eq!(server.parent_span_id.as_deref(), Some(SPAN_HEX));

    let event = spans.iter().find(|s| s.is_event).expect("the annotation");
    assert_eq!(event.name, "retry");
}

/// Zipkin's proto3 messages, as `zipkin.proto3` declares them.
mod zipkin_pb {
    #[derive(Clone, PartialEq, prost::Message)]
    pub struct ListOfSpans {
        #[prost(message, repeated, tag = "1")]
        pub spans: Vec<Span>,
    }
    #[derive(Clone, PartialEq, prost::Message)]
    pub struct Span {
        #[prost(bytes = "vec", tag = "1")]
        pub trace_id: Vec<u8>,
        #[prost(bytes = "vec", tag = "2")]
        pub parent_id: Vec<u8>,
        #[prost(bytes = "vec", tag = "3")]
        pub id: Vec<u8>,
        #[prost(int32, tag = "4")]
        pub kind: i32,
        #[prost(string, tag = "5")]
        pub name: String,
        #[prost(fixed64, tag = "6")]
        pub timestamp: u64,
        #[prost(uint64, tag = "7")]
        pub duration: u64,
        #[prost(message, optional, tag = "8")]
        pub local_endpoint: Option<Endpoint>,
        #[prost(message, optional, tag = "9")]
        pub remote_endpoint: Option<Endpoint>,
        #[prost(message, repeated, tag = "10")]
        pub annotations: Vec<Annotation>,
        #[prost(map = "string, string", tag = "11")]
        pub tags: std::collections::HashMap<String, String>,
        #[prost(bool, tag = "12")]
        pub debug: bool,
        #[prost(bool, tag = "13")]
        pub shared: bool,
    }
    #[derive(Clone, PartialEq, prost::Message)]
    pub struct Endpoint {
        #[prost(string, tag = "1")]
        pub service_name: String,
        #[prost(bytes = "vec", tag = "2")]
        pub ipv4: Vec<u8>,
        #[prost(bytes = "vec", tag = "3")]
        pub ipv6: Vec<u8>,
        #[prost(int32, tag = "4")]
        pub port: i32,
    }
    #[derive(Clone, PartialEq, prost::Message)]
    pub struct Annotation {
        #[prost(fixed64, tag = "1")]
        pub timestamp: u64,
        #[prost(string, tag = "2")]
        pub value: String,
    }
}

#[tokio::test]
async fn zipkin_proto3_spans_are_stored_and_gzip_is_read() {
    let d = daemon(ServerConfig::default()).await;
    let list = zipkin_pb::ListOfSpans {
        spans: vec![zipkin_pb::Span {
            // A 64-bit trace id, which Zipkin allows and mcpd pads.
            trace_id: vec![0x4b; 8],
            parent_id: vec![0x02; 8],
            id: SPAN.to_vec(),
            kind: 2,
            name: "get /chat".to_owned(),
            timestamp: 1_767_225_600_000_000,
            duration: 2_000,
            local_endpoint: Some(zipkin_pb::Endpoint {
                service_name: "frontend".to_owned(),
                ipv4: vec![10, 0, 0, 7],
                ..zipkin_pb::Endpoint::default()
            }),
            tags: [("http.method".to_owned(), "GET".to_owned())].into(),
            ..zipkin_pb::Span::default()
        }],
    };
    let res = post(
        &d.app,
        "/api/v2/spans",
        "application/x-protobuf",
        gzip(&list.encode_to_vec()),
        &[("content-encoding", "gzip")],
    )
    .await;
    assert_eq!(res.status, StatusCode::ACCEPTED, "{}", String::from_utf8_lossy(&res.body));

    let spans = spans_of(&d.store, "default", "00000000000000004b4b4b4b4b4b4b4b").await;
    assert_eq!(spans.len(), 1);
    let s = &spans[0];
    assert_eq!(s.parent_span_id.as_deref(), Some("0202020202020202"));
    assert_eq!(s.kind, mcpd_core::SpanKind::HttpRequest);
    assert_eq!(s.correlation.service_name.as_deref(), Some("frontend"));
    let attrs = s.attributes.as_ref().unwrap();
    assert_eq!(attrs["span.kind"], "server");
    assert_eq!(attrs["network.local.address"], "10.0.0.7");
}

#[tokio::test]
async fn zipkin_and_jaeger_doors_refuse_a_missing_key() {
    let d = daemon(ServerConfig {
        ingest_keys: vec![INGEST_KEY.to_owned()],
        ..ServerConfig::default()
    })
    .await;
    let zipkin = zipkin_json().to_string().into_bytes();
    let res = post(&d.app, "/api/v2/spans", "application/json", zipkin.clone(), &[]).await;
    assert_eq!(res.status, StatusCode::UNAUTHORIZED);
    let res = post(&d.app, "/api/traces", "application/x-thrift", jaeger_thrift(), &[]).await;
    assert_eq!(res.status, StatusCode::UNAUTHORIZED);
    assert!(spans_of(&d.store, "default", TRACE_HEX).await.is_empty());

    let auth = format!("Bearer {INGEST_KEY}");
    let res =
        post(&d.app, "/api/v2/spans", "application/json", zipkin, &[("authorization", &auth)])
            .await;
    assert_eq!(res.status, StatusCode::ACCEPTED);
}

// --------------------------------------------------------------- Jaeger ---

/// A writer of Thrift's binary protocol, enough for `jaeger.thrift`.
#[derive(Default)]
struct Thrift(Vec<u8>);

impl Thrift {
    fn field(&mut self, ty: u8, id: i16) -> &mut Self {
        self.0.push(ty);
        self.0.extend_from_slice(&id.to_be_bytes());
        self
    }
    fn i32(&mut self, id: i16, v: i32) -> &mut Self {
        self.field(8, id);
        self.0.extend_from_slice(&v.to_be_bytes());
        self
    }
    fn i64(&mut self, id: i16, v: i64) -> &mut Self {
        self.field(10, id);
        self.0.extend_from_slice(&v.to_be_bytes());
        self
    }
    fn bool(&mut self, id: i16, v: bool) -> &mut Self {
        self.field(2, id);
        self.0.push(u8::from(v));
        self
    }
    fn string(&mut self, id: i16, v: &str) -> &mut Self {
        self.field(11, id);
        self.0.extend_from_slice(&(v.len() as i32).to_be_bytes());
        self.0.extend_from_slice(v.as_bytes());
        self
    }
    fn structs(&mut self, id: i16, items: &[Vec<u8>]) -> &mut Self {
        self.field(15, id);
        self.0.push(12);
        self.0.extend_from_slice(&(items.len() as i32).to_be_bytes());
        for i in items {
            self.0.extend_from_slice(i);
        }
        self
    }
    fn strukt(&mut self, id: i16, item: &[u8]) -> &mut Self {
        self.field(12, id);
        self.0.extend_from_slice(item);
        self
    }
    fn end(&mut self) -> Vec<u8> {
        self.0.push(0);
        std::mem::take(&mut self.0)
    }
}

fn tag_str(k: &str, v: &str) -> Vec<u8> {
    Thrift::default().string(1, k).i32(2, 0).string(3, v).end()
}

fn tag_bool(k: &str, v: bool) -> Vec<u8> {
    Thrift::default().string(1, k).i32(2, 2).bool(5, v).end()
}

fn tag_long(k: &str, v: i64) -> Vec<u8> {
    Thrift::default().string(1, k).i32(2, 3).i64(6, v).end()
}

/// One batch: a process, and a span with a parent, a FOLLOWS_FROM reference
/// into another trace, typed tags and a log.
fn jaeger_thrift() -> Vec<u8> {
    let half = |b: u8| i64::from_be_bytes([b; 8]);
    let follows =
        Thrift::default().i32(1, 1).i64(2, half(0x9c)).i64(3, half(0x9c)).i64(4, half(0x07)).end();
    let log = Thrift::default()
        .i64(1, 1_767_225_600_500_000)
        .structs(2, &[tag_str("event", "retry"), tag_long("attempt", 2)])
        .end();
    let span = Thrift::default()
        .i64(1, half(0x4b))
        .i64(2, half(0x4b))
        .i64(3, half(0x01))
        .i64(4, half(0x02))
        .string(5, "chat gpt-4o")
        .structs(6, &[follows])
        .i32(7, 1)
        .i64(8, 1_767_225_600_000_000)
        .i64(9, 1_000_000)
        .structs(
            10,
            &[
                tag_str("gen_ai.system", "openai"),
                tag_str("span.kind", "client"),
                tag_bool("error", true),
                tag_long("gen_ai.usage.input_tokens", 1284),
            ],
        )
        .structs(11, &[log])
        .end();
    let process = Thrift::default()
        .string(1, "checkout-agent")
        .structs(2, &[tag_str("hostname", "worker-3")])
        .end();
    Thrift::default().strukt(1, &process).structs(2, &[span]).end()
}

fn assert_jaeger_span(spans: &[mcpd_core::Span]) {
    assert_eq!(spans.len(), 2, "the span and its log: {spans:#?}");
    let s = spans.iter().find(|s| s.span_id == SPAN_HEX).expect("the span");
    assert_eq!(s.parent_span_id.as_deref(), Some("0202020202020202"));
    assert_eq!(s.correlation.service_name.as_deref(), Some("checkout-agent"));
    assert_eq!(s.kind, mcpd_core::SpanKind::ModelGeneration);
    assert_eq!(s.status, mcpd_core::SpanStatus::Error);
    assert_eq!(s.usage.as_ref().and_then(|u| u.input_tokens), Some(1284));
    let links = s.links.as_ref().expect("the reference became a link");
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].trace_id, LINKED_TRACE_HEX);
    assert_eq!(links[0].kind.as_deref(), Some("follows_from"));
    let event = spans.iter().find(|s| s.is_event).expect("the log");
    assert_eq!(event.name, "retry");
    assert_eq!(event.attributes.as_ref().unwrap()["attempt"], 2);
}

#[tokio::test]
async fn jaeger_thrift_over_http_is_stored_faithfully() {
    let d = daemon(ServerConfig::default()).await;
    for _ in 0..2 {
        let res = post(&d.app, "/api/traces", "application/x-thrift", jaeger_thrift(), &[]).await;
        assert_eq!(res.status, StatusCode::ACCEPTED, "{}", String::from_utf8_lossy(&res.body));
    }
    // Sent twice, stored once: a span is keyed on its id, and the log's span
    // id is derived from the span, not drawn at random.
    assert_jaeger_span(&spans_of(&d.store, "default", TRACE_HEX).await);

    let broken = post(&d.app, "/api/traces", "application/x-thrift", vec![12, 0], &[]).await;
    assert_eq!(broken.status, StatusCode::BAD_REQUEST);
}

/// Jaeger's `api_v2` messages, as `model.proto` and `collector.proto`
/// declare them.
mod jaeger_pb {
    #[derive(Clone, PartialEq, prost::Message)]
    pub struct PostSpansRequest {
        #[prost(message, optional, tag = "1")]
        pub batch: Option<Batch>,
    }
    #[derive(Clone, PartialEq, prost::Message)]
    pub struct PostSpansResponse {}
    #[derive(Clone, PartialEq, prost::Message)]
    pub struct Batch {
        #[prost(message, repeated, tag = "1")]
        pub spans: Vec<Span>,
        #[prost(message, optional, tag = "2")]
        pub process: Option<Process>,
    }
    #[derive(Clone, PartialEq, prost::Message)]
    pub struct Span {
        #[prost(bytes = "vec", tag = "1")]
        pub trace_id: Vec<u8>,
        #[prost(bytes = "vec", tag = "2")]
        pub span_id: Vec<u8>,
        #[prost(string, tag = "3")]
        pub operation_name: String,
        #[prost(message, repeated, tag = "4")]
        pub references: Vec<SpanRef>,
        #[prost(uint32, tag = "5")]
        pub flags: u32,
        #[prost(message, optional, tag = "6")]
        pub start_time: Option<Stamp>,
        #[prost(message, optional, tag = "7")]
        pub duration: Option<Stamp>,
        #[prost(message, repeated, tag = "8")]
        pub tags: Vec<KeyValue>,
        #[prost(message, repeated, tag = "9")]
        pub logs: Vec<Log>,
    }
    #[derive(Clone, PartialEq, prost::Message)]
    pub struct SpanRef {
        #[prost(bytes = "vec", tag = "1")]
        pub trace_id: Vec<u8>,
        #[prost(bytes = "vec", tag = "2")]
        pub span_id: Vec<u8>,
        #[prost(int32, tag = "3")]
        pub ref_type: i32,
    }
    /// `google.protobuf.Timestamp` and `Duration` share this layout.
    #[derive(Clone, PartialEq, prost::Message)]
    pub struct Stamp {
        #[prost(int64, tag = "1")]
        pub seconds: i64,
        #[prost(int32, tag = "2")]
        pub nanos: i32,
    }
    #[derive(Clone, PartialEq, prost::Message)]
    pub struct KeyValue {
        #[prost(string, tag = "1")]
        pub key: String,
        #[prost(int32, tag = "2")]
        pub v_type: i32,
        #[prost(string, tag = "3")]
        pub v_str: String,
        #[prost(bool, tag = "4")]
        pub v_bool: bool,
        #[prost(int64, tag = "5")]
        pub v_int64: i64,
        #[prost(double, tag = "6")]
        pub v_float64: f64,
        #[prost(bytes = "vec", tag = "7")]
        pub v_binary: Vec<u8>,
    }
    #[derive(Clone, PartialEq, prost::Message)]
    pub struct Log {
        #[prost(message, optional, tag = "1")]
        pub timestamp: Option<Stamp>,
        #[prost(message, repeated, tag = "2")]
        pub fields: Vec<KeyValue>,
    }
    #[derive(Clone, PartialEq, prost::Message)]
    pub struct Process {
        #[prost(string, tag = "1")]
        pub service_name: String,
        #[prost(message, repeated, tag = "2")]
        pub tags: Vec<KeyValue>,
    }
}

fn jaeger_request() -> jaeger_pb::PostSpansRequest {
    use jaeger_pb::{Batch, KeyValue, Log, PostSpansRequest, Process, SpanRef, Stamp};
    let s = |k: &str, v: &str| KeyValue { key: k.into(), v_str: v.into(), ..KeyValue::default() };
    PostSpansRequest {
        batch: Some(Batch {
            process: Some(Process {
                service_name: "checkout-agent".into(),
                tags: vec![s("hostname", "worker-3")],
            }),
            spans: vec![jaeger_pb::Span {
                trace_id: TRACE.to_vec(),
                span_id: SPAN.to_vec(),
                operation_name: "chat gpt-4o".into(),
                references: vec![
                    // CHILD_OF, in the same trace: the parent.
                    SpanRef { trace_id: TRACE.to_vec(), span_id: vec![0x02; 8], ref_type: 0 },
                    SpanRef {
                        trace_id: LINKED_TRACE.to_vec(),
                        span_id: vec![0x07; 8],
                        ref_type: 1,
                    },
                ],
                flags: 1,
                start_time: Some(Stamp { seconds: 1_767_225_600, nanos: 0 }),
                duration: Some(Stamp { seconds: 1, nanos: 0 }),
                tags: vec![
                    s("gen_ai.system", "openai"),
                    KeyValue {
                        key: "error".into(),
                        v_type: 1,
                        v_bool: true,
                        ..KeyValue::default()
                    },
                    KeyValue {
                        key: "gen_ai.usage.input_tokens".into(),
                        v_type: 2,
                        v_int64: 1284,
                        ..KeyValue::default()
                    },
                ],
                logs: vec![Log {
                    timestamp: Some(Stamp { seconds: 1_767_225_600, nanos: 500_000_000 }),
                    fields: vec![
                        s("event", "retry"),
                        KeyValue {
                            key: "attempt".into(),
                            v_type: 2,
                            v_int64: 2,
                            ..KeyValue::default()
                        },
                    ],
                }],
            }],
        }),
    }
}

#[tokio::test]
async fn jaeger_grpc_post_spans_is_stored_faithfully() {
    let d = daemon(ServerConfig {
        ingest_keys: vec![INGEST_KEY.to_owned()],
        ..ServerConfig::default()
    })
    .await;
    let mut grpc =
        tonic::client::Grpc::new(channel(&d.app).await).send_compressed(CompressionEncoding::Gzip);
    let path = || {
        tonic::codegen::http::uri::PathAndQuery::from_static(
            "/jaeger.api_v2.CollectorService/PostSpans",
        )
    };
    let codec = || {
        tonic_prost::ProstCodec::<jaeger_pb::PostSpansRequest, jaeger_pb::PostSpansResponse>::default()
    };

    grpc.ready().await.expect("ready");
    let refused = grpc
        .unary(tonic::Request::new(jaeger_request()), path(), codec())
        .await
        .expect_err("no key");
    assert_eq!(refused.code(), Code::Unauthenticated);

    grpc.ready().await.expect("ready");
    grpc.unary(with_key(jaeger_request(), INGEST_KEY), path(), codec()).await.expect("post spans");
    assert_jaeger_span(&spans_of(&d.store, "default", TRACE_HEX).await);
}
