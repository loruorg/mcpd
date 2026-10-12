//! Payload capture, a finished span handed in whole, the sink seam and the
//! client's stats (contract sections 2.1 to 2.3 and 3).

mod common;

use common::FakeMcpd;
use mcpd_sdk::model::{Correlation, ModelParameters, Span as SpanRecord, SpanStatus};
use mcpd_sdk::sink::SinkFuture;
use mcpd_sdk::{Config, Error, LogLevel, Mcpd, RecordingSink, Scale, Sink, SpanKind, Target};
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use time::OffsetDateTime;

const TRACE: &str = "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1";
const SPAN: &str = "b2b2b2b2b2b2b2b2";
const PARENT: &str = "c3c3c3c3c3c3c3c3";

/// A configuration whose export happens only when a test asks for it.
fn config() -> Config {
    Config {
        endpoint: "http://127.0.0.1:9".to_owned(),
        project: "shop".to_owned(),
        environment: Some("test".to_owned()),
        batch_size: 64,
        flush_interval: Duration::from_secs(3600),
        max_queue: 1024,
        timeout: Duration::from_secs(5),
        ..Config::default()
    }
}

/// A client whose batches land in memory.
fn recorded(config: Config) -> (Arc<RecordingSink>, Mcpd) {
    let sink = Arc::new(RecordingSink::new());
    let mcpd = Mcpd::with_config(config.with_sink(sink.clone()));
    (sink, mcpd)
}

fn correlation(project: &str) -> Correlation {
    Correlation {
        project: project.to_owned(),
        environment: None,
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
    }
}

/// A finished span an adapter might hold: ids, parent, start and end all set.
fn finished(name: &str) -> SpanRecord {
    let start = OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(1_700_000_000);
    SpanRecord {
        trace_id: TRACE.to_owned(),
        span_id: SPAN.to_owned(),
        parent_span_id: Some(PARENT.to_owned()),
        external_parent_span_id: None,
        name: name.to_owned(),
        kind: SpanKind::ToolCall,
        status: SpanStatus::Ok,
        start_time: start,
        end_time: Some(start + time::Duration::milliseconds(250)),
        duration_ns: Some(250_000_000),
        is_root: false,
        is_event: false,
        entity_kind: None,
        entity_id: None,
        entity_name: None,
        input: Some(json!("the prompt")),
        output: Some(json!("the completion")),
        attributes: None,
        metadata: None,
        usage: None,
        cost: None,
        model_parameters: None,
        error: None,
        links: None,
        tags: None,
        correlation: correlation(""),
        model: None,
        provider: None,
        received_at: OffsetDateTime::UNIX_EPOCH,
        ingest_key: None,
    }
}

// ------------------------------------------------------------ capture (2.1) ---

#[tokio::test]
async fn capture_off_in_code_keeps_input_and_output_in_the_process() {
    let (sink, mcpd) = recorded(config().with_capture_payloads(false));

    let run = mcpd.trace("support agent");
    run.set_input("where is my order?");
    run.set_output("It shipped on Tuesday.");
    run.set_model("claude-sonnet-4", "anthropic");
    run.set_tokens(12, 4);
    run.set_model_parameters(ModelParameters {
        temperature: Some(0.0),
        seed: Some(0),
        stop_sequences: Some(vec!["STOP".to_owned()]),
        ..ModelParameters::default()
    });
    run.set_session("s-1");
    run.event_with("chunk", json!({ "input": "secret", "output": "secret", "index": 3 }));
    run.set_error("the tool timed out");
    run.end();
    mcpd.flush().await.expect("flush");

    let spans = sink.spans();
    let ended = spans.iter().find(|s| !s.is_event && s.end_time.is_some()).expect("the end");
    assert!(ended.input.is_none(), "input left the process: {:?}", ended.input);
    assert!(ended.output.is_none(), "output left the process: {:?}", ended.output);
    // Everything else on the record survives.
    assert_eq!(ended.model.as_deref(), Some("claude-sonnet-4"));
    assert_eq!(ended.usage.as_ref().and_then(|u| u.input_tokens), Some(12));
    // Sampling settings are metadata, not payload: no prompt is in them, and
    // they are what the run has to be reproduced from. Capture off must not
    // take them, and a zero seed must survive being read as absent.
    let params = ended.model_parameters.as_ref().expect("the sampling settings");
    assert_eq!(params.seed, Some(0), "a zero seed was dropped");
    assert_eq!(params.temperature, Some(0.0), "a zero temperature was dropped");
    assert_eq!(params.stop_sequences.as_deref(), Some(&["STOP".to_owned()][..]));
    assert!(!params.has_response_format);
    assert_eq!(ended.correlation.session_id.as_deref(), Some("s-1"));
    assert_eq!(ended.status, SpanStatus::Error);
    assert_eq!(ended.error.as_ref().map(|e| e.message.as_str()), Some("the tool timed out"));

    let event = spans.iter().find(|s| s.is_event).expect("the event");
    let attributes = event.attributes.clone().expect("attributes");
    assert!(attributes.get("input").is_none(), "an event's payload left: {attributes}");
    assert!(attributes.get("output").is_none(), "an event's payload left: {attributes}");
    assert_eq!(attributes["index"], 3, "the rest of the event survives");
}

#[tokio::test]
async fn capture_off_from_the_environment_reaches_the_wire() {
    // This test binary is the only one that reads the process environment,
    // and this is the only test in it that sets anything.
    std::env::set_var("MCPD_CAPTURE_PAYLOADS", "false");
    let from_env = Config::from_env();
    std::env::remove_var("MCPD_CAPTURE_PAYLOADS");
    assert!(!from_env.capture_payloads);

    let (sink, mcpd) = recorded(Config { capture_payloads: from_env.capture_payloads, ..config() });
    let run = mcpd.trace("support agent");
    run.set_input("kept here");
    run.end();
    mcpd.flush().await.expect("flush");
    assert!(sink.spans().iter().all(|s| s.input.is_none()));
}

#[tokio::test]
async fn a_span_overrides_the_client_in_either_direction() {
    // Client on, one span off.
    let (sink, mcpd) = recorded(config());
    let run = mcpd.trace("support agent");
    run.set_input("kept: the client captures");
    let private = run.child_of("model_call", SpanKind::ModelGeneration);
    private.set_input("set before the switch");
    private.capture_payloads(false);
    private.set_output("set after the switch");
    private.end();
    run.end();
    mcpd.flush().await.expect("flush");
    let spans = sink.spans();
    let root = spans.iter().find(|s| s.is_root && s.end_time.is_some()).unwrap();
    assert_eq!(root.input, Some(json!("kept: the client captures")));
    let call = spans.iter().find(|s| s.name == "model_call" && s.end_time.is_some()).unwrap();
    assert!(call.input.is_none(), "a payload set before the switch still left");
    assert!(call.output.is_none(), "a payload set after the switch left");
    assert!(!private.capturing());

    // Client off, one span on.
    let (sink, mcpd) = recorded(config().with_capture_payloads(false));
    let run = mcpd.trace("support agent");
    run.set_input("dropped: the client does not capture");
    let public = run.child_of("lookup_order", SpanKind::ToolCall);
    public.capture_payloads(true);
    public.set_output("order 5512 shipped");
    public.end();
    run.end();
    mcpd.flush().await.expect("flush");
    let spans = sink.spans();
    let root = spans.iter().find(|s| s.is_root && s.end_time.is_some()).unwrap();
    assert!(root.input.is_none());
    let lookup = spans.iter().find(|s| s.name == "lookup_order" && s.end_time.is_some()).unwrap();
    assert_eq!(lookup.output, Some(json!("order 5512 shipped")));
}

// ---------------------------------------------------- external parent (3) ---

#[tokio::test]
async fn an_external_parent_reaches_the_wire_under_its_own_name() {
    let server = FakeMcpd::start().await;
    let mcpd = Mcpd::with_config(Config { endpoint: server.endpoint(), ..config() });
    let run = mcpd.trace("support agent");
    run.set_external_parent("d4d4d4d4d4d4d4d4");
    run.end();
    mcpd.flush().await.expect("flush");
    let spans = server.all_spans();
    let ended = spans.last().unwrap();
    assert_eq!(ended["external_parent_span_id"], "d4d4d4d4d4d4d4d4");
    // Carried for correlation, never used for parenting inside mcpd's tree.
    assert!(ended["parent_span_id"].is_null());
    assert_eq!(ended["is_root"], true);
}

// ------------------------------------------------------------- record (3) ---

#[tokio::test]
async fn record_queues_a_finished_span_as_given() {
    // Sample 0.0 would drop any span the client minted; a recorded span is
    // never re-sampled, so it goes through.
    let (sink, mcpd) = recorded(Config { sample: 0.0, ..config() });
    let span = finished("import");
    mcpd.record(span.clone());
    mcpd.flush().await.expect("flush");

    let spans = sink.spans();
    assert_eq!(spans.len(), 1, "a finished span is queued once, not on start and end");
    let got = &spans[0];
    assert_eq!(got.trace_id, TRACE);
    assert_eq!(got.span_id, SPAN);
    assert_eq!(got.parent_span_id.as_deref(), Some(PARENT), "re-parented");
    assert_eq!(got.start_time, span.start_time, "re-timed");
    assert_eq!(got.end_time, span.end_time, "re-timed");
    assert_eq!(got.input, span.input);
    // Empty correlation fields are filled from the client's; set ones are kept.
    assert_eq!(got.correlation.project, "shop");
    assert_eq!(got.correlation.environment.as_deref(), Some("test"));
    assert_eq!(mcpd.stats().rejected, 0);

    let mut own = finished("own project");
    own.correlation = correlation("billing");
    own.correlation.environment = Some("staging".to_owned());
    mcpd.record(own);
    mcpd.flush().await.expect("flush");
    let got = sink.spans().into_iter().last().unwrap();
    assert_eq!(got.correlation.project, "billing");
    assert_eq!(got.correlation.environment.as_deref(), Some("staging"));
}

#[tokio::test]
async fn record_refuses_each_malformed_shape_by_counting_rather_than_raising() {
    let (sink, mcpd) = recorded(config());
    let mut bad_trace = finished("bad trace");
    bad_trace.trace_id = "not-hex".to_owned();
    let mut short_span = finished("short span");
    short_span.span_id = "b2b2".to_owned();
    let mut zero_trace = finished("zero trace");
    zero_trace.trace_id = "0".repeat(32);
    let mut zero_span = finished("zero span");
    zero_span.span_id = "0".repeat(16);
    let mut bad_parent = finished("bad parent");
    bad_parent.parent_span_id = Some("PARENT".to_owned());
    let mut nameless = finished("   ");
    nameless.name = "   ".to_owned();
    let mut unstarted = finished("unstarted");
    unstarted.start_time = OffsetDateTime::UNIX_EPOCH;

    for span in [bad_trace, short_span, zero_trace, zero_span, bad_parent, nameless, unstarted] {
        mcpd.record(span);
    }
    mcpd.flush().await.expect("flush");
    assert!(sink.spans().is_empty(), "a refused span reached the sink");
    assert_eq!(mcpd.stats().rejected, 7);

    // A well-formed one still goes through afterwards.
    mcpd.record(finished("fine"));
    mcpd.flush().await.expect("flush");
    assert_eq!(sink.spans().len(), 1);
}

#[tokio::test]
async fn record_is_stripped_by_capture_and_dropped_by_enabled_like_any_span() {
    let (sink, mcpd) = recorded(config().with_capture_payloads(false));
    mcpd.record(finished("stripped"));
    mcpd.flush().await.expect("flush");
    let got = sink.spans().into_iter().next().expect("recorded");
    assert!(got.input.is_none() && got.output.is_none());
    assert_eq!(got.name, "stripped");

    let (sink, mcpd) = recorded(config().with_enabled(false));
    mcpd.record(finished("dropped"));
    mcpd.flush().await.expect("flush");
    assert_eq!(sink.batches(), 0);
    assert_eq!(mcpd.stats().rejected, 0, "a disabled client refuses nothing; it records nothing");
}

// -------------------------------------------------------------- sinks (2.2) ---

#[tokio::test]
async fn a_recording_sink_sees_exactly_the_signals_recorded() {
    let (sink, mcpd) = recorded(config());
    let run = mcpd.trace("support agent");
    let trace_id = run.trace_id().to_owned();
    run.sync_scope(|| {
        mcpd.log(LogLevel::Info, "answering");
        mcpd.gauge("queue_depth", 3.0, None);
    });
    run.end();
    mcpd.score(Target::trace(&trace_id), "helpfulness", 0.9, Scale::UNIT, None);
    mcpd.feedback(Target::trace(&trace_id), mcpd_sdk::FeedbackKind::Thumbs, "up", None);
    mcpd.flush().await.expect("flush");

    assert_eq!(sink.batches(), 1);
    let spans = sink.spans();
    assert_eq!(spans.len(), 2, "a span arrives on start and on end, as on the wire");
    assert!(spans.iter().all(|s| s.name == "support agent"));
    assert_eq!(sink.logs().len(), 1);
    assert_eq!(sink.logs()[0].message, "answering");
    assert_eq!(sink.logs()[0].trace_id.as_deref(), Some(trace_id.as_str()));
    assert_eq!(sink.metrics().len(), 1);
    assert_eq!(sink.scores().len(), 1);
    assert_eq!(sink.feedback().len(), 1);
    assert_eq!(mcpd.stats().exported, 6);

    sink.clear();
    assert_eq!(sink.batches(), 0);
    assert!(sink.spans().is_empty() && sink.logs().is_empty());
}

/// A sink that refuses, and one that panics.
struct Refusing {
    calls: AtomicU64,
    panic: bool,
}

impl Sink for Refusing {
    fn export(&self, _batch: mcpd_sdk::model::IngestBatch, _key: String) -> SinkFuture<'_> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        Box::pin(async move {
            if self.panic {
                panic!("the sink fell over");
            }
            Err(Error::Export("refused".to_owned()))
        })
    }
}

#[tokio::test]
async fn a_sink_that_fails_or_panics_is_a_failed_export_the_caller_never_sees() {
    for panics in [false, true] {
        let sink = Arc::new(Refusing { calls: AtomicU64::new(0), panic: panics });
        let mcpd = Mcpd::with_config(config().with_sink(Arc::clone(&sink) as Arc<dyn Sink>));
        mcpd.trace("support agent").end();
        mcpd.log(LogLevel::Info, "still recording");
        // Recording never raised; flush reports the failure, as it does for
        // the transport, and the process is still here.
        let flushed = mcpd.flush().await;
        assert!(flushed.is_err(), "flush should report the sink's failure");
        if panics {
            assert!(flushed.unwrap_err().to_string().contains("panicked"));
        }
        assert_eq!(sink.calls.load(Ordering::Relaxed), 1);
        let stats = mcpd.stats();
        assert_eq!(stats.failed, 1);
        assert_eq!(stats.exported, 0);
        assert_eq!(stats.queued, 0, "a failed batch is let go, not re-queued");
        // The client keeps working afterwards.
        mcpd.trace("again").end();
        let _ = mcpd.flush().await;
        assert_eq!(sink.calls.load(Ordering::Relaxed), 2);
    }
}

/// A sink that takes `delay` over every batch: a daemon that accepts the
/// connection and answers late, or never within the caller's patience.
struct Slow {
    calls: AtomicU64,
    delay: Duration,
}

impl Sink for Slow {
    fn export(&self, _batch: mcpd_sdk::model::IngestBatch, _key: String) -> SinkFuture<'_> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        let delay = self.delay;
        Box::pin(async move {
            tokio::time::sleep(delay).await;
            Ok(())
        })
    }
}

#[tokio::test]
async fn flush_and_shutdown_return_by_their_deadline_when_the_sink_never_answers() {
    // Against a black-holed daemon every batch costs three attempts of the full
    // request timeout. A bound on batches is no bound on time.
    let sink = Arc::new(Slow { calls: AtomicU64::new(0), delay: Duration::from_millis(400) });
    let mcpd = Mcpd::with_config(
        Config { batch_size: 1, ..config() }.with_sink(Arc::clone(&sink) as Arc<dyn Sink>),
    );
    for i in 0..5 {
        mcpd.log(LogLevel::Info, format!("line {i}"));
    }

    let started = std::time::Instant::now();
    assert!(mcpd.flush_within(Duration::from_millis(50)).await.is_err());
    assert!(mcpd.shutdown_within(Duration::from_millis(50)).await.is_err());
    assert!(started.elapsed() < Duration::from_millis(300), "took {:?}", started.elapsed());

    // The batch in flight finishes and is counted; no further batch starts.
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(sink.calls.load(Ordering::Relaxed), 1);
    let stats = mcpd.stats();
    assert_eq!(stats.exported, 1);
    assert_eq!(stats.queued, 4, "what was given up stays visible as queued");
}

// -------------------------------------------------------------- stats (2.3) ---

#[tokio::test]
async fn stats_count_queued_dropped_exported_failed_and_rejected() {
    let (sink, mcpd) = recorded(Config { max_queue: 2, ..config() });
    let fresh = mcpd.stats();
    assert_eq!((fresh.queued, fresh.dropped, fresh.capacity), (0, 0, 2));
    assert_eq!((fresh.exported, fresh.failed, fresh.rejected), (0, 0, 0));

    for i in 0..5 {
        mcpd.log(LogLevel::Info, format!("line {i}"));
    }
    let full = mcpd.stats();
    assert_eq!(full.queued, 2);
    assert_eq!(full.dropped, 3);

    mcpd.flush().await.expect("flush");
    assert_eq!(mcpd.stats().exported, 2);
    assert_eq!(sink.logs().len(), 2);

    let mut bad = finished("bad");
    bad.span_id = "nope".to_owned();
    mcpd.record(bad);
    let after = mcpd.stats();
    assert_eq!(after.rejected, 1);
    assert_eq!(after.failed, 0);
    assert_eq!(after.queued, 0);
}
