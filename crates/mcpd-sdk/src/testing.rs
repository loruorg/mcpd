//! A client for your own unit tests (SDK contract 2.5).
//!
//! ```no_run
//! # async fn answer_question(_: &mcpd_sdk::Mcpd) {}
//! # async fn f() {
//! use mcpd_sdk::testing::TestClient;
//!
//! let test = TestClient::new();
//! answer_question(test.client()).await;
//! let call = test.span("model_call").await.unwrap();
//! assert_eq!(call.usage.and_then(|u| u.output_tokens), Some(37));
//! # }
//! ```
//!
//! It is a real [`Mcpd`] whose sink keeps every batch in memory, so what you
//! read back went through sampling, payload capture, redaction and the queue
//! exactly as it would on the way to a daemon. Nothing leaves the process.

use crate::{Config, Mcpd, RecordingSink};
use mcpd_core::{Feedback, LogRecord, MetricPoint, Score, Span as SpanRecord, SpanKind};
use std::collections::HashMap;
use std::sync::Arc;

/// One finished span and the finished spans under it, by start time.
#[derive(Debug, Clone)]
pub struct SpanTree {
    /// The span.
    pub span: SpanRecord,
    /// Its children, earliest start first.
    pub children: Vec<SpanTree>,
}

/// Why [`TestClient::span`] found no single span by that name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpanLookupError {
    /// The name asked for.
    pub name: String,
    /// How many finished spans carry it.
    pub found: usize,
    /// The names of every finished span recorded, each once.
    pub seen: Vec<String>,
}

impl std::fmt::Display for SpanLookupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let seen = if self.seen.is_empty() { "none".to_owned() } else { self.seen.join(", ") };
        write!(
            f,
            "expected one finished span named \"{}\", found {}; spans recorded: {seen}",
            self.name, self.found
        )
    }
}

impl std::error::Error for SpanLookupError {}

/// An [`Mcpd`] that records into memory. Pass [`TestClient::client`] wherever
/// your code takes a client, then read back what it recorded. Every accessor
/// flushes first, so a test never calls `flush` itself.
#[derive(Debug, Clone)]
pub struct TestClient {
    client: Mcpd,
    recorder: Arc<RecordingSink>,
}

impl Default for TestClient {
    fn default() -> Self {
        Self::new()
    }
}

impl TestClient {
    /// A test client configured as [`Mcpd::new`] would be, except that it is
    /// on and records every trace whatever the environment says: a CI job
    /// that turns mcpd off must not turn the test client off with it.
    #[must_use]
    pub fn new() -> Self {
        Self::with_config(Config::from_env().with_enabled(true).with_sample(1.0))
    }

    /// A test client from your own configuration, used as given apart from
    /// the sink. Start from [`Config::default`] to leave the environment out
    /// of it entirely.
    #[must_use]
    pub fn with_config(config: Config) -> Self {
        let recorder = Arc::new(RecordingSink::new());
        let client = Mcpd::with_config(config.with_sink(recorder.clone()));
        Self { client, recorder }
    }

    /// The client to hand to the code under test.
    #[must_use]
    pub const fn client(&self) -> &Mcpd {
        &self.client
    }

    /// The sink underneath, for a test that wants the raw batches.
    #[must_use]
    pub const fn recorder(&self) -> &Arc<RecordingSink> {
        &self.recorder
    }

    async fn flush(&self) {
        // A recording sink does not fail, and a test reading back what it
        // recorded has nothing useful to do with a flush error anyway.
        let _ = self.client.flush().await;
    }

    /// The finished spans, one per span id, in the order each id first
    /// arrived, filtered by name and kind when given.
    pub async fn spans(&self, name: Option<&str>, kind: Option<SpanKind>) -> Vec<SpanRecord> {
        self.flush().await;
        finished(self.recorder.spans())
            .into_iter()
            .filter(|s| name.is_none_or(|n| s.name == n) && kind.is_none_or(|k| s.kind == k))
            .collect()
    }

    /// The one finished span called `name`.
    ///
    /// # Errors
    /// When there is none, or more than one. The error gives the count and
    /// the names that were recorded, so `.unwrap()` in a test says what went
    /// wrong.
    pub async fn span(&self, name: &str) -> Result<SpanRecord, SpanLookupError> {
        let all = self.spans(None, None).await;
        let mut found: Vec<SpanRecord> = all.iter().filter(|s| s.name == name).cloned().collect();
        if found.len() == 1 {
            return Ok(found.remove(0));
        }
        let mut seen: Vec<String> = Vec::new();
        for s in &all {
            if !seen.contains(&s.name) {
                seen.push(s.name.clone());
            }
        }
        Err(SpanLookupError { name: name.to_owned(), found: found.len(), seen })
    }

    /// Every log record, in arrival order.
    pub async fn logs(&self) -> Vec<LogRecord> {
        self.flush().await;
        self.recorder.logs()
    }

    /// Every metric point, in arrival order.
    pub async fn metrics(&self) -> Vec<MetricPoint> {
        self.flush().await;
        self.recorder.metrics()
    }

    /// Every score, in arrival order.
    pub async fn scores(&self) -> Vec<Score> {
        self.flush().await;
        self.recorder.scores()
    }

    /// Every piece of feedback, in arrival order. Not `feedback`, because the
    /// client already has a `feedback` that records one.
    pub async fn recorded_feedback(&self) -> Vec<Feedback> {
        self.flush().await;
        self.recorder.feedback()
    }

    /// Every trace as a tree of finished spans. A span whose parent was not
    /// recorded is a root.
    pub async fn trees(&self) -> Vec<SpanTree> {
        let spans = self.spans(None, None).await;
        let position: HashMap<&str, usize> =
            spans.iter().enumerate().map(|(i, s)| (s.span_id.as_str(), i)).collect();
        let mut children: Vec<Vec<usize>> = vec![Vec::new(); spans.len()];
        let mut roots = Vec::new();
        for (i, s) in spans.iter().enumerate() {
            match s.parent_span_id.as_deref().and_then(|p| position.get(p)) {
                Some(&parent) if parent != i => children[parent].push(i),
                _ => roots.push(i),
            }
        }
        let mut trees = build(&roots, &spans, &children);
        order(&mut trees);
        trees
    }

    /// Flush, then forget everything recorded so far.
    pub async fn reset(&self) {
        self.flush().await;
        self.recorder.clear();
    }
}

/// The nodes at `at`, with their children. Every node here descends from a
/// root, so following children never comes back round.
fn build(at: &[usize], spans: &[SpanRecord], children: &[Vec<usize>]) -> Vec<SpanTree> {
    at.iter()
        .map(|&i| SpanTree {
            span: spans[i].clone(),
            children: build(&children[i], spans, children),
        })
        .collect()
}

/// Sort every level by start time. The sort is stable, so spans that started
/// together keep their arrival order.
fn order(list: &mut [SpanTree]) {
    list.sort_by_key(|n| n.span.start_time);
    for node in list {
        order(&mut node.children);
    }
}

/// The last record per span id, in first-arrival order, ended spans only.
fn finished(records: Vec<SpanRecord>) -> Vec<SpanRecord> {
    let mut slot: HashMap<String, usize> = HashMap::new();
    let mut last: Vec<SpanRecord> = Vec::new();
    for s in records {
        match slot.get(&s.span_id) {
            Some(&i) => last[i] = s,
            None => {
                slot.insert(s.span_id.clone(), last.len());
                last.push(s);
            }
        }
    }
    last.retain(|s| s.end_time.is_some());
    last
}
