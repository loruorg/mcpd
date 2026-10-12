//! The logging signal of contract section 4: the level floor, an error under a
//! field key, and the correlation a log line carries.

use mcpd_sdk::{Config, LogLevel, Mcpd, RecordingSink};
use std::sync::Arc;
use std::time::Duration;

fn recorded(config: Config) -> (Arc<RecordingSink>, Mcpd) {
    let sink = Arc::new(RecordingSink::new());
    let mcpd = Mcpd::with_config(
        Config { project: "shop".to_owned(), flush_interval: Duration::from_secs(3600), ..config }
            .with_sink(sink.clone()),
    );
    (sink, mcpd)
}

#[tokio::test]
async fn the_floor_drops_trace_by_default_and_keeps_debug() {
    let (sink, mcpd) = recorded(Config::default());
    assert_eq!(mcpd.config().log_level, LogLevel::Debug);
    mcpd.log(LogLevel::Trace, "finer than debug");
    mcpd.log(LogLevel::Debug, "kept");
    mcpd.flush().await.expect("export");

    let messages: Vec<String> = sink.logs().into_iter().map(|l| l.message).collect();
    assert_eq!(messages, vec!["kept".to_owned()]);
}

#[tokio::test]
async fn the_floor_drops_everything_below_it() {
    let (sink, mcpd) = recorded(Config::default().with_log_level(LogLevel::Warn));
    for level in LogLevel::ALL {
        mcpd.log(*level, level.as_str());
    }
    mcpd.flush().await.expect("export");

    let messages: Vec<String> = sink.logs().into_iter().map(|l| l.message).collect();
    assert_eq!(messages, vec!["warn".to_owned(), "error".to_owned(), "fatal".to_owned()]);
    assert_eq!(mcpd.stats().dropped, 0, "a line below the floor must never be queued");
}

#[tokio::test]
async fn the_floor_never_filters_the_other_four_signals() {
    let (sink, mcpd) = recorded(Config::default().with_log_level(LogLevel::Fatal));
    let run = mcpd.trace("run");
    let trace_id = run.trace_id().to_owned();
    mcpd.metric("latency_ms", 3.0, mcpd_sdk::MetricKind::Gauge, None, None);
    mcpd.score(mcpd_sdk::Target::trace(&trace_id), "judge", 1.0, mcpd_sdk::Scale::UNIT, None);
    mcpd.feedback(mcpd_sdk::Target::trace(&trace_id), mcpd_sdk::FeedbackKind::Thumbs, "up", None);
    run.end();
    mcpd.flush().await.expect("export");

    assert!(!sink.spans().is_empty());
    assert_eq!(sink.metrics().len(), 1);
    assert_eq!(sink.scores().len(), 1);
    assert_eq!(sink.feedback().len(), 1);
    assert!(sink.logs().is_empty());
}

#[tokio::test]
async fn an_error_reaches_a_field_key_as_its_type_and_message() {
    let (sink, mcpd) = recorded(Config::default());
    let io = std::io::Error::new(std::io::ErrorKind::NotFound, "no such file");
    mcpd.log_error(LogLevel::Error, "reading the prompt", &io);
    mcpd.flush().await.expect("export");

    let fields = sink.logs()[0].fields.clone().expect("the line carried fields");
    assert_eq!(fields["error"]["message"], "no such file");
    assert_eq!(fields["error"]["type"], "Error");
    assert_eq!(sink.logs()[0].level, LogLevel::Error);
}

#[tokio::test]
async fn a_log_line_inside_a_run_carries_the_runs_correlation() {
    let (sink, mcpd) = recorded(Config::default());
    let run = mcpd.trace("run");
    run.set_session("session-7");
    run.set_user("u-1");
    run.sync_scope(|| {
        mcpd.log(LogLevel::Info, "inside");
        mcpd.metric("latency_ms", 4.0, mcpd_sdk::MetricKind::Gauge, None, None);
    });
    run.end();
    mcpd.flush().await.expect("export");

    let line = &sink.logs()[0];
    assert_eq!(line.correlation.session_id.as_deref(), Some("session-7"));
    assert_eq!(line.correlation.user_id.as_deref(), Some("u-1"));
    assert_eq!(line.correlation.project, "shop");
    let point = &sink.metrics()[0];
    assert_eq!(point.correlation.session_id.as_deref(), Some("session-7"));
}

#[test]
fn the_environment_sets_the_floor_and_a_bad_name_is_ignored() {
    // Set through the process environment the same way the client reads it.
    // Serialised by running in one test, because the environment is global.
    std::env::set_var("MCPD_LOG_LEVEL", "ERROR");
    assert_eq!(Config::from_env().log_level, LogLevel::Error);
    std::env::set_var("MCPD_LOG_LEVEL", "verbose");
    assert_eq!(Config::from_env().log_level, LogLevel::Debug);
    std::env::remove_var("MCPD_LOG_LEVEL");
    assert_eq!(Config::from_env().log_level, LogLevel::Debug);
}
