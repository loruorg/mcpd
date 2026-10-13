//! Experiments (SDK contract 5.2): the reads and writes, and the runner.
//!
//! The daemon never calls the agent under test. The runner does, in your
//! process, once per dataset case inside a root span that names the experiment
//! and the case, and then asks the daemon to score what was recorded.

use crate::context::SpanContext;
use crate::prompts::segment;
use crate::span::Span;
use crate::{Error, Mcpd};
use mcpd_core::{
    DatasetItem, Experiment, ExperimentResults, ExperimentStatus, ScoreRunResult, ScorerConfig,
    SpanKind, SpanStatus, Trajectory,
};
use serde::Serialize;
use serde_json::Value;
use std::future::Future;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::Poll;
use time::OffsetDateTime;

/// The attribute the daemon reads to find the case a run answered.
pub const DATASET_ITEM_ATTRIBUTE: &str = "mcpd.dataset_item_id";

/// How many cases [`Mcpd::run_experiment`] runs at once unless told otherwise.
pub const DEFAULT_EXPERIMENT_CONCURRENCY: usize = 4;

/// The most cases the daemon answers in one page.
const MAX_CASES: u32 = 5000;

/// How [`Mcpd::run_experiment`] names, scores and paces an experiment.
///
/// ```
/// use mcpd_sdk::RunExperiment;
/// use serde_json::json;
///
/// let options = RunExperiment::new()
///     .name("answers-v2")
///     .scorers(["exact_match", "similarity"])
///     .metadata(json!({ "prompt": "answer@4" }));
/// assert_eq!(options.concurrency, 4);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct RunExperiment {
    /// The experiment's name, and the name of each run's root span. The
    /// dataset id when not given.
    pub name: Option<String>,
    /// The scorers, as `POST /v1/api/score` takes them. Empty runs the code
    /// scorers that need no settings.
    pub scorers: Vec<String>,
    /// Settings for the scorers. Each case's expected answer is filled in by
    /// the daemon.
    pub config: Option<ScorerConfig>,
    /// How many cases run at once. At least 1.
    pub concurrency: usize,
    /// Anything you want to keep with the experiment, such as the prompt
    /// version under test.
    pub metadata: Option<Value>,
}

impl Default for RunExperiment {
    fn default() -> Self {
        Self {
            name: None,
            scorers: Vec::new(),
            config: None,
            concurrency: DEFAULT_EXPERIMENT_CONCURRENCY,
            metadata: None,
        }
    }
}

impl RunExperiment {
    /// Named after the dataset, scored by the code scorers that need no
    /// settings, four cases at a time.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Name the experiment and each run's root span.
    #[must_use]
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// The scorers to run once every case has an answer.
    #[must_use]
    pub fn scorers<I, S>(mut self, scorers: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.scorers = scorers.into_iter().map(Into::into).collect();
        self
    }

    /// Settings for the scorers.
    #[must_use]
    pub fn config(mut self, config: ScorerConfig) -> Self {
        self.config = Some(config);
        self
    }

    /// Run this many cases at once.
    #[must_use]
    pub const fn concurrency(mut self, concurrency: usize) -> Self {
        self.concurrency = concurrency;
        self
    }

    /// Keep this with the experiment.
    #[must_use]
    pub fn metadata(mut self, metadata: Value) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

impl Mcpd {
    /// A run as the tool calls, model generations and handoffs its agent took.
    ///
    /// # Errors
    /// As [`Mcpd::list_traces`].
    pub async fn get_trajectory(&self, trace_id: &str) -> Result<Trajectory, Error> {
        self.get(
            &format!("/v1/api/traces/{}/trajectory", segment(trace_id)),
            vec![("project", self.project().to_owned())],
        )
        .await
    }

    /// The cases of a dataset. Sends `limit` only: the daemon finds the
    /// project from the dataset.
    ///
    /// # Errors
    /// As [`Mcpd::list_traces`].
    pub async fn list_dataset_items(
        &self,
        dataset_id: &str,
        limit: Option<u32>,
    ) -> Result<Vec<DatasetItem>, Error> {
        let params = limit.map(|l| vec![("limit", l.to_string())]).unwrap_or_default();
        self.get(&format!("/v1/api/datasets/{}/items", segment(dataset_id)), params).await
    }

    /// Record an experiment, or replace one: the daemon upserts on
    /// `experiment_id`, and an empty id gets a new one. The client's project
    /// is used when the correlation names none.
    ///
    /// # Errors
    /// As [`Mcpd::list_traces`].
    pub async fn create_experiment(&self, experiment: &Experiment) -> Result<Experiment, Error> {
        let mut experiment = experiment.clone();
        if experiment.correlation.project.trim().is_empty() {
            experiment.correlation.project = self.project().to_owned();
        }
        self.post("/v1/api/experiments", Vec::new(), &experiment).await
    }

    /// Score every run of an experiment, each against the expected answer of
    /// the dataset case it names. No scorers runs the code scorers that need
    /// no settings.
    ///
    /// # Errors
    /// As [`Mcpd::list_traces`].
    pub async fn score_experiment(
        &self,
        experiment_id: &str,
        scorers: &[String],
        config: Option<&ScorerConfig>,
    ) -> Result<ScoreRunResult, Error> {
        let mut body = serde_json::json!({ "scorers": scorers });
        if let Some(config) = config {
            body["config"] = serde_json::to_value(config)?;
        }
        self.post(
            &format!("/v1/api/experiments/{}/score", segment(experiment_id)),
            vec![("project", self.project().to_owned())],
            &body,
        )
        .await
    }

    /// An experiment case by case, with a summary per scorer.
    ///
    /// # Errors
    /// As [`Mcpd::list_traces`].
    pub async fn experiment_results(
        &self,
        experiment_id: &str,
    ) -> Result<ExperimentResults, Error> {
        self.get(
            &format!("/v1/api/experiments/{}/results", segment(experiment_id)),
            vec![("project", self.project().to_owned())],
        )
        .await
    }

    /// Run `target` over every case of a dataset and score the answers.
    ///
    /// Each case runs inside a root span named after the experiment, carrying
    /// the experiment id and the case id, so a span the target opens with
    /// [`Mcpd::span`] is a child of its case. At most `concurrency` cases are
    /// in flight, on the calling task, so `target` may borrow from its
    /// surroundings. A target that returns an error or panics fails its own
    /// run and the next case still runs. Then the runs are flushed and scored,
    /// the experiment is marked `succeeded` with each scorer's mean, and the
    /// results are returned.
    ///
    /// # Errors
    /// [`Error::Config`] before any request when the client is disabled or
    /// `concurrency` is 0. Otherwise the first read, write or flush that
    /// failed, after the experiment has been marked `failed` with its message.
    pub async fn run_experiment<F, Fut, T, E>(
        &self,
        dataset_id: &str,
        target: F,
        options: &RunExperiment,
    ) -> Result<ExperimentResults, Error>
    where
        F: Fn(Value) -> Fut,
        Fut: Future<Output = Result<T, E>>,
        T: Serialize,
        E: std::fmt::Display,
    {
        if !self.is_enabled() {
            return Err(Error::Config(
                "run_experiment needs an enabled client, because no run would reach the daemon \
                 to be scored; turn the client on with Config::with_enabled(true) or MCPD_ENABLED"
                    .to_owned(),
            ));
        }
        if options.concurrency < 1 {
            return Err(Error::Config(format!(
                "run_experiment concurrency must be at least 1, got {}; leave it at the default \
                 of {DEFAULT_EXPERIMENT_CONCURRENCY} or pass a positive number",
                options.concurrency
            )));
        }
        let draft = Experiment {
            experiment_id: String::new(),
            name: options.name.clone().unwrap_or_else(|| dataset_id.to_owned()),
            dataset_id: dataset_id.to_owned(),
            status: ExperimentStatus::Running,
            started_at: Some(OffsetDateTime::now_utc()),
            finished_at: None,
            item_count: 0,
            summary: None,
            baseline_id: None,
            cost: None,
            error: None,
            metadata: options.metadata.clone(),
            correlation: self.inner.correlation(),
        };

        let mut created = None;
        let outcome = self.drive_experiment(&draft, &target, options, &mut created).await;
        let Err(error) = outcome else { return outcome };
        if let Some(experiment) = created {
            let failed = Experiment {
                status: ExperimentStatus::Failed,
                finished_at: Some(OffsetDateTime::now_utc()),
                error: Some(error.to_string()),
                ..experiment
            };
            // Reported and swallowed: the caller needs the error that stopped
            // the run, not the one that stopped us saying so.
            if let Err(recording) = self.create_experiment(&failed).await {
                tracing::warn!(
                    error = %recording,
                    experiment = %failed.experiment_id,
                    "run_experiment could not record the failed status"
                );
            }
        }
        Err(error)
    }

    /// The runner's steps after its checks. `created` is set as soon as the
    /// daemon has the experiment, so a failure after that can be recorded on
    /// it.
    async fn drive_experiment<F, Fut, T, E>(
        &self,
        draft: &Experiment,
        target: &F,
        options: &RunExperiment,
        created: &mut Option<Experiment>,
    ) -> Result<ExperimentResults, Error>
    where
        F: Fn(Value) -> Fut,
        Fut: Future<Output = Result<T, E>>,
        T: Serialize,
        E: std::fmt::Display,
    {
        let experiment = created.insert(self.create_experiment(draft).await?).clone();
        let items = self.list_dataset_items(&draft.dataset_id, Some(MAX_CASES)).await?;

        // A fixed set of workers pulls the next case, so at most `concurrency`
        // targets are in flight however long the dataset is.
        let next = AtomicUsize::new(0);
        let workers = (0..options.concurrency.min(items.len()))
            .map(|_| async {
                while let Some(item) = items.get(next.fetch_add(1, Ordering::Relaxed)) {
                    self.run_case(&experiment, item, target).await;
                }
            })
            .collect();
        join_all(workers).await;

        self.flush().await.map_err(|e| {
            Error::Export(format!("the runs did not all reach the daemon, so none was scored: {e}"))
        })?;
        self.score_experiment(&experiment.experiment_id, &options.scorers, options.config.as_ref())
            .await?;
        let results = self.experiment_results(&experiment.experiment_id).await?;
        let summary = results
            .scorers
            .iter()
            .map(|s| (s.scorer_id.clone(), Value::from(s.mean)))
            .collect::<serde_json::Map<_, _>>();
        let finished = Experiment {
            status: ExperimentStatus::Succeeded,
            finished_at: Some(OffsetDateTime::now_utc()),
            item_count: u32::try_from(items.len()).unwrap_or(u32::MAX),
            summary: Some(Value::Object(summary)),
            ..experiment
        };
        self.create_experiment(&finished).await?;
        Ok(results)
    }

    /// One case: a root span around the target. A target that fails or panics
    /// fails its own span only.
    async fn run_case<F, Fut, T, E>(&self, experiment: &Experiment, item: &DatasetItem, target: &F)
    where
        F: Fn(Value) -> Fut,
        Fut: Future<Output = Result<T, E>>,
        T: Serialize,
        E: std::fmt::Display,
    {
        let mut correlation = self.inner.correlation();
        correlation.experiment_id = Some(experiment.experiment_id.clone());
        // Sampled whatever the rate: a sampled-out case would be a case with
        // no answer.
        let span = Span::start(
            &self.inner,
            SpanContext::root(true),
            None,
            experiment.name.clone(),
            SpanKind::AgentRun,
            correlation,
        );
        span.set_input(&item.input);
        span.set_attributes(serde_json::json!({ DATASET_ITEM_ATTRIBUTE: item.item_id }));
        // The call itself is inside the guard, so a target that panics before
        // its first await is caught too.
        let input = item.input.clone();
        match span.scope(catch_panic(async move { target(input).await })).await {
            Ok(Ok(output)) => {
                span.set_output(output);
                span.end_with(SpanStatus::Ok);
            }
            Ok(Err(error)) => {
                span.set_error(error.to_string());
                span.end_with(SpanStatus::Error);
            }
            Err(panic) => {
                span.set_error(format!("the target panicked: {panic}"));
                span.end_with(SpanStatus::Error);
            }
        }
    }
}

/// Drive every future to completion on the calling task. Each wake polls all
/// of them, which is cheap for the handful of workers the runner starts.
async fn join_all<F: Future<Output = ()>>(futures: Vec<F>) {
    let mut pending: Vec<Pin<Box<F>>> = futures.into_iter().map(Box::pin).collect();
    std::future::poll_fn(|cx| {
        pending.retain_mut(|f| f.as_mut().poll(cx).is_pending());
        if pending.is_empty() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    })
    .await;
}

/// Run a future, turning a panic inside it into the panic's message.
async fn catch_panic<F: Future>(future: F) -> Result<F::Output, String> {
    let mut future = std::pin::pin!(future);
    std::future::poll_fn(|cx| match catch_unwind(AssertUnwindSafe(|| future.as_mut().poll(cx))) {
        Ok(Poll::Ready(value)) => Poll::Ready(Ok(value)),
        Ok(Poll::Pending) => Poll::Pending,
        Err(payload) => Poll::Ready(Err(payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned()))
            .unwrap_or_else(|| "no message".to_owned()))),
    })
    .await
}
