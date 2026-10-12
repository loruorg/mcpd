//! The bounded in-memory queue every signal passes through.
//!
//! Recording a signal appends here and returns; the worker drains it. That is
//! the whole of why instrumentation costs an agent microseconds rather than a
//! network round trip.
//!
//! The bound is the important part. An unbounded buffer turns a network outage
//! into an out-of-memory kill of the process being observed, which is the worst
//! thing an observability tool can do. When the queue is full the **oldest**
//! signal is discarded: during an outage the recent past is the part worth
//! keeping, and the head of the queue is the part already being retried.

use crate::Throttle;
use mcpd_core::{Feedback, IngestBatch, LogRecord, MetricPoint, Score, Span};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tokio::sync::Notify;

/// One queued item. Boxed because a [`Span`] is an order of magnitude larger
/// than the other four, and an enum is the size of its largest variant.
#[derive(Debug)]
pub(crate) enum Signal {
    Span(Box<Span>),
    Log(Box<LogRecord>),
    Metric(Box<MetricPoint>),
    Score(Box<Score>),
    Feedback(Box<Feedback>),
}

/// Fold a drained run of signals into the batch shape the ingest endpoint takes.
pub(crate) fn to_batch(signals: Vec<Signal>) -> IngestBatch {
    let mut spans = Vec::new();
    let mut logs = Vec::new();
    let mut metrics = Vec::new();
    let mut scores = Vec::new();
    let mut feedback = Vec::new();
    for s in signals {
        match s {
            Signal::Span(v) => spans.push(*v),
            Signal::Log(v) => logs.push(*v),
            Signal::Metric(v) => metrics.push(*v),
            Signal::Score(v) => scores.push(*v),
            Signal::Feedback(v) => feedback.push(*v),
        }
    }
    IngestBatch {
        spans: (!spans.is_empty()).then_some(spans),
        logs: (!logs.is_empty()).then_some(logs),
        metrics: (!metrics.is_empty()).then_some(metrics),
        scores: (!scores.is_empty()).then_some(scores),
        feedback: (!feedback.is_empty()).then_some(feedback),
        // The queue carries no aspect notes yet: a note is written about a
        // trace after the fact, by a worker or by the daemon, not by the
        // instrumented process on its way past.
        aspect_notes: None,
        // Stable for the life of this batch, including across retries, so a
        // reply lost on the way back cannot become a second copy of the data.
        ingest_key: Some(mcpd_core::ids::new_trace_id()),
    }
}

/// A bounded FIFO with a drop-oldest policy.
#[derive(Debug)]
pub(crate) struct Queue {
    items: Mutex<VecDeque<Signal>>,
    /// Signalled when a full batch is waiting, so the worker exports it without
    /// waiting out the flush interval.
    ready: Notify,
    max: usize,
    batch_size: usize,
    dropped: AtomicU64,
    warn: Throttle,
}

impl Queue {
    pub(crate) fn new(max: usize, batch_size: usize, warn_interval: Duration) -> Self {
        Self {
            items: Mutex::new(VecDeque::with_capacity(batch_size.min(max))),
            ready: Notify::new(),
            // A queue that cannot hold a single signal would silently discard
            // everything, which reads as "the SDK is broken".
            max: max.max(1),
            batch_size: batch_size.clamp(1, max.max(1)),
            dropped: AtomicU64::new(0),
            warn: Throttle::new(warn_interval),
        }
    }

    /// Append a signal, discarding the oldest if that would exceed the bound.
    pub(crate) fn push(&self, signal: Signal) {
        // A poisoned lock means a producer panicked while holding it. Recovering
        // the queue is strictly better than propagating that panic into the
        // caller's code path, which B1 forbids.
        let mut q = self.items.lock().unwrap_or_else(|e| e.into_inner());
        while q.len() >= self.max {
            q.pop_front();
            let total = self.dropped.fetch_add(1, Ordering::Relaxed) + 1;
            if self.warn.allow() {
                tracing::warn!(
                    dropped_total = total,
                    max_queue = self.max,
                    "mcpd queue is full; dropping the oldest signals. \
                     Raise max_queue, or check that the endpoint is reachable."
                );
            }
        }
        q.push_back(signal);
        let len = q.len();
        drop(q);
        if len >= self.batch_size {
            self.ready.notify_one();
        }
    }

    /// Take up to `batch_size` signals, oldest first.
    pub(crate) fn drain(&self) -> Vec<Signal> {
        let mut q = self.items.lock().unwrap_or_else(|e| e.into_inner());
        let take = q.len().min(self.batch_size);
        q.drain(..take).collect()
    }

    /// Wait until a full batch is queued.
    pub(crate) async fn ready(&self) {
        self.ready.notified().await;
    }

    pub(crate) fn len(&self) -> usize {
        self.items.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    /// The bound.
    pub(crate) fn capacity(&self) -> usize {
        self.max
    }

    /// How many signals the bound has cost so far. Exposed so a user can tell a
    /// quiet SDK from a losing one.
    pub(crate) fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcpd_core::{Correlation, LogLevel};
    use time::OffsetDateTime;

    fn log(message: &str) -> Signal {
        Signal::Log(Box::new(LogRecord {
            log_id: message.to_owned(),
            trace_id: None,
            span_id: None,
            timestamp: OffsetDateTime::UNIX_EPOCH,
            level: LogLevel::Info,
            message: message.to_owned(),
            fields: None,
            logger: None,
            correlation: Correlation {
                project: "default".into(),
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
            received_at: OffsetDateTime::UNIX_EPOCH,
        }))
    }

    fn message(s: &Signal) -> String {
        match s {
            Signal::Log(l) => l.message.clone(),
            _ => panic!("expected a log"),
        }
    }

    #[test]
    fn the_bound_holds_however_much_is_pushed() {
        let q = Queue::new(8, 4, Duration::from_secs(60));
        for i in 0..1_000 {
            q.push(log(&i.to_string()));
        }
        assert_eq!(q.len(), 8, "the queue grew past max_queue");
        assert_eq!(q.dropped(), 992);
    }

    #[test]
    fn the_oldest_signal_is_the_one_discarded() {
        let q = Queue::new(3, 3, Duration::from_secs(60));
        for name in ["a", "b", "c", "d"] {
            q.push(log(name));
        }
        let drained: Vec<String> = q.drain().iter().map(message).collect();
        assert_eq!(drained, vec!["b", "c", "d"], "kept the wrong end of the queue");
    }

    #[test]
    fn draining_takes_at_most_one_batch_oldest_first() {
        let q = Queue::new(100, 2, Duration::from_secs(60));
        for name in ["a", "b", "c"] {
            q.push(log(name));
        }
        let first: Vec<String> = q.drain().iter().map(message).collect();
        assert_eq!(first, vec!["a", "b"]);
        assert_eq!(q.len(), 1);
    }

    #[test]
    fn nothing_is_dropped_while_there_is_room() {
        let q = Queue::new(4, 4, Duration::from_secs(60));
        q.push(log("a"));
        q.push(log("b"));
        assert_eq!(q.dropped(), 0);
    }

    #[test]
    fn a_batch_carries_every_signal_kind_it_was_given() {
        let batch = to_batch(vec![log("a"), log("b")]);
        assert_eq!(batch.logs.as_ref().map(Vec::len), Some(2));
        assert!(batch.spans.is_none(), "an empty array is not the same as no array");
        assert!(batch.ingest_key.is_some(), "batches must be idempotent");
    }
}
