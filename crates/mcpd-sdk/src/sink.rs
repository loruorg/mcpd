//! Where a drained batch goes.
//!
//! The exporter turns the queue into an [`IngestBatch`] and hands it to the
//! client's sink. The HTTP transport is the default sink and the only one that
//! talks to a daemon; the two here keep the batch in the process. A recording
//! sink is how a test asserts what the instrumentation recorded without a
//! server, and a console sink is a local dump while debugging.
//!
//! A sink runs on the exporter's worker inside the same guard as the transport:
//! one that fails or panics is a failed export, reported once per interval and
//! never raised to the caller (B1).

use crate::Error;
use mcpd_core::IngestBatch;
use serde::Serialize;
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;

/// The future a sink answers with.
pub type SinkFuture<'a> = Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'a>>;

/// A destination for drained batches. See the module comment.
pub trait Sink: Send + Sync + 'static {
    /// Take one batch. `ingest_key` is stable for the batch, including across
    /// the transport's own retries, so a destination can refuse a duplicate.
    fn export(&self, batch: IngestBatch, ingest_key: String) -> SinkFuture<'_>;
}

#[derive(Debug, Default)]
struct Recorded {
    spans: Vec<mcpd_core::Span>,
    logs: Vec<mcpd_core::LogRecord>,
    metrics: Vec<mcpd_core::MetricPoint>,
    scores: Vec<mcpd_core::Score>,
    feedback: Vec<mcpd_core::Feedback>,
    batches: u64,
}

/// A sink that keeps every batch in memory.
///
/// ```no_run
/// # async fn f() {
/// use std::sync::Arc;
/// use mcpd_sdk::{Config, Mcpd, RecordingSink};
///
/// let sink = Arc::new(RecordingSink::new());
/// let mcpd = Mcpd::with_config(Config::default().with_sink(sink.clone()));
/// mcpd.trace("support agent").end();
/// let _ = mcpd.flush().await;
/// assert_eq!(sink.spans().last().map(|s| s.name.as_str()), Some("support agent"));
/// # }
/// ```
#[derive(Debug, Default)]
pub struct RecordingSink {
    inner: Mutex<Recorded>,
}

impl RecordingSink {
    /// An empty recorder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn read<T>(&self, f: impl FnOnce(&Recorded) -> T) -> T {
        f(&self.inner.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Every span seen, in arrival order. A span sent on start and again on
    /// end appears twice, as it did on the wire.
    #[must_use]
    pub fn spans(&self) -> Vec<mcpd_core::Span> {
        self.read(|r| r.spans.clone())
    }

    /// Every log record seen, in arrival order.
    #[must_use]
    pub fn logs(&self) -> Vec<mcpd_core::LogRecord> {
        self.read(|r| r.logs.clone())
    }

    /// Every metric point seen, in arrival order.
    #[must_use]
    pub fn metrics(&self) -> Vec<mcpd_core::MetricPoint> {
        self.read(|r| r.metrics.clone())
    }

    /// Every score seen, in arrival order.
    #[must_use]
    pub fn scores(&self) -> Vec<mcpd_core::Score> {
        self.read(|r| r.scores.clone())
    }

    /// Every piece of feedback seen, in arrival order.
    #[must_use]
    pub fn feedback(&self) -> Vec<mcpd_core::Feedback> {
        self.read(|r| r.feedback.clone())
    }

    /// How many batches have arrived.
    #[must_use]
    pub fn batches(&self) -> u64 {
        self.read(|r| r.batches)
    }

    /// Forget everything seen so far.
    pub fn clear(&self) {
        *self.inner.lock().unwrap_or_else(|e| e.into_inner()) = Recorded::default();
    }
}

impl Sink for RecordingSink {
    fn export(&self, batch: IngestBatch, _ingest_key: String) -> SinkFuture<'_> {
        Box::pin(async move {
            let mut r = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            r.batches += 1;
            r.spans.extend(batch.spans.unwrap_or_default());
            r.logs.extend(batch.logs.unwrap_or_default());
            r.metrics.extend(batch.metrics.unwrap_or_default());
            r.scores.extend(batch.scores.unwrap_or_default());
            r.feedback.extend(batch.feedback.unwrap_or_default());
            Ok(())
        })
    }
}

/// The writer a [`ConsoleSink`] hands each line to.
pub type LineWriter = Box<dyn Fn(String) + Send + Sync>;

/// A sink that writes one JSON line per signal, shaped
/// `{"signal":"span",...record}` with the wire's field names, to standard
/// error by default.
pub struct ConsoleSink {
    write: LineWriter,
}

impl std::fmt::Debug for ConsoleSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConsoleSink").finish_non_exhaustive()
    }
}

impl Default for ConsoleSink {
    fn default() -> Self {
        Self::new()
    }
}

impl ConsoleSink {
    /// Write to standard error.
    #[must_use]
    pub fn new() -> Self {
        Self { write: Box::new(|line| eprintln!("{line}")) }
    }

    /// Write each line somewhere else, such as a buffer in a test.
    #[must_use]
    pub fn with_writer(write: LineWriter) -> Self {
        Self { write }
    }

    fn line<T: Serialize>(&self, signal: &str, record: &T) {
        let mut value = serde_json::to_value(record).unwrap_or(serde_json::Value::Null);
        if let serde_json::Value::Object(map) = &mut value {
            map.insert("signal".to_owned(), serde_json::Value::String(signal.to_owned()));
        }
        (self.write)(value.to_string());
    }
}

impl Sink for ConsoleSink {
    fn export(&self, batch: IngestBatch, _ingest_key: String) -> SinkFuture<'_> {
        Box::pin(async move {
            for s in batch.spans.unwrap_or_default() {
                self.line("span", &s);
            }
            for l in batch.logs.unwrap_or_default() {
                self.line("log", &l);
            }
            for m in batch.metrics.unwrap_or_default() {
                self.line("metric", &m);
            }
            for s in batch.scores.unwrap_or_default() {
                self.line("score", &s);
            }
            for f in batch.feedback.unwrap_or_default() {
                self.line("feedback", &f);
            }
            Ok(())
        })
    }
}

impl Sink for crate::transport::Transport {
    fn export(&self, batch: IngestBatch, _ingest_key: String) -> SinkFuture<'_> {
        Box::pin(async move { self.send(&batch).await.map(|_| ()) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn a_console_sink_writes_one_line_per_signal_with_the_wire_names() {
        let lines = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&lines);
        let sink = ConsoleSink::with_writer(Box::new(move |line| seen.lock().unwrap().push(line)));
        let batch = IngestBatch {
            spans: None,
            logs: Some(vec![mcpd_core::LogRecord {
                log_id: "l1".into(),
                trace_id: None,
                span_id: None,
                timestamp: time::OffsetDateTime::UNIX_EPOCH,
                level: mcpd_core::LogLevel::Info,
                message: "hello".into(),
                fields: None,
                logger: None,
                correlation: mcpd_core::Correlation {
                    project: "shop".into(),
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
                },
                received_at: time::OffsetDateTime::UNIX_EPOCH,
            }]),
            metrics: None,
            scores: None,
            feedback: None,
            aspect_notes: None,
            ingest_key: Some("k".into()),
        };
        sink.export(batch, "k".into()).await.unwrap();
        let lines = lines.lock().unwrap();
        assert_eq!(lines.len(), 1);
        let v: serde_json::Value = serde_json::from_str(&lines[0]).unwrap();
        assert_eq!(v["signal"], "log");
        assert_eq!(v["log_id"], "l1");
        assert_eq!(v["correlation"]["project"], "shop");
    }
}
