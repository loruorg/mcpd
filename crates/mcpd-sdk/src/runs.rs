//! The run surface of the daemon, as a client: definitions, runs, events, and
//! the worker protocol [`Worker`](crate::worker::Worker) drives.
//!
//! These calls return errors, like the read API, because a run you asked for
//! and did not get is news. A worker call refused with a 409 returns
//! [`Error::Conflict`] carrying the daemon's closed reason, which is what lets
//! a worker tell a fence from its own landed call.

use std::time::Duration;

use mcpd_core::{
    ClaimRequest, ClaimResponse, Correlation, Definition, DefinitionDocument, DefinitionVersion,
    EventReceipt, LeaseState, PublishEventRequest, Run, RunDetail, RunEvent, RunEventKind,
    RunEventPage, RunListResponse, RunStatus, StartRunRequest, StepAck, StepError, StepFailure,
    StepResult, WorkerRegistered, WorkerRegistration,
};
use serde_json::Value;

use crate::client::Mcpd;
use crate::Error;

/// What [`Mcpd::start_run`] takes.
#[derive(Debug, Clone, Default)]
pub struct StartRun {
    /// Which definition to run.
    pub definition: String,
    /// Which version. `None` pins the highest stored version at start time.
    pub version: Option<u32>,
    /// The run's input, validated against the definition's `input_schema` when it has one.
    pub input: Option<Value>,
    /// Session, user and the other correlation fields the run's spans carry.
    pub correlation: Option<Correlation>,
    /// Unique per project while held: a retry of this call returns the same run.
    pub idempotency_key: Option<String>,
    /// Which project. Needed only when the key reaches several.
    pub project: Option<String>,
}

impl StartRun {
    /// A run of `definition` with the daemon's defaults.
    #[must_use]
    pub fn new(definition: impl Into<String>) -> Self {
        Self { definition: definition.into(), ..Self::default() }
    }

    /// The run's input.
    #[must_use]
    pub fn input(mut self, input: Value) -> Self {
        self.input = Some(input);
        self
    }

    /// Pin a version.
    #[must_use]
    pub fn version(mut self, version: u32) -> Self {
        self.version = Some(version);
        self
    }

    /// Make a retry of this call return the same run.
    #[must_use]
    pub fn idempotency_key(mut self, key: impl Into<String>) -> Self {
        self.idempotency_key = Some(key.into());
        self
    }

    /// The correlation the run's spans carry.
    #[must_use]
    pub fn correlation(mut self, correlation: Correlation) -> Self {
        self.correlation = Some(correlation);
        self
    }
}

/// What [`Mcpd::list_runs`] takes.
#[derive(Debug, Clone, Default)]
pub struct RunFilter {
    /// Which project. `None` means the client's own.
    pub project: Option<String>,
    /// Only runs in this status.
    pub status: Option<RunStatus>,
    /// Only runs of this definition.
    pub definition_id: Option<String>,
    /// Page size.
    pub limit: Option<u32>,
    /// The cursor from the previous page.
    pub cursor: Option<String>,
}

/// What [`Mcpd::publish_event`] takes.
#[derive(Debug, Clone, Default)]
pub struct PublishEvent {
    /// The event name a wait names.
    pub event: String,
    /// The key the wait rendered, such as an order id.
    pub key: String,
    /// What the waiting step reads as its output.
    pub payload: Option<Value>,
    /// Yours to supply from any handler that can be retried, such as a
    /// webhook: a repeat of a supplied id is answered `duplicate` and writes
    /// nothing.
    pub event_id: Option<String>,
    /// Which project. `None` means the client's own.
    pub project: Option<String>,
}

impl PublishEvent {
    /// An event routed by name and key.
    #[must_use]
    pub fn new(event: impl Into<String>, key: impl Into<String>) -> Self {
        Self { event: event.into(), key: key.into(), ..Self::default() }
    }

    /// The payload the waiting step reads as its output.
    #[must_use]
    pub fn payload(mut self, payload: Value) -> Self {
        self.payload = Some(payload);
        self
    }

    /// An id of your own, so a retry is a duplicate and not a second event.
    #[must_use]
    pub fn event_id(mut self, id: impl Into<String>) -> Self {
        self.event_id = Some(id.into());
        self
    }
}

/// What [`Mcpd::wait_run`] answers: the run as it ended, with its output to
/// hand.
#[derive(Debug, Clone)]
pub struct RunResult {
    /// Always `Succeeded`; any other ending is [`Error::RunFailed`].
    pub status: RunStatus,
    /// What the definition's `output` template rendered.
    pub output: Option<Value>,
    /// The priced usage the steps reported, in USD.
    pub cost_usd: f64,
    /// How many attempts the run spent.
    pub steps_used: u32,
    /// The trace every step of the run landed in.
    pub trace_id: String,
    /// The run in full.
    pub run: Run,
}

impl RunResult {
    /// The output, decoded.
    ///
    /// # Errors
    /// When the output is absent or is not a `T`.
    pub fn output_as<T: serde::de::DeserializeOwned>(&self) -> Result<T, Error> {
        let value = self.output.clone().unwrap_or(Value::Null);
        serde_json::from_value(value).map_err(Error::Encode)
    }
}

/// What [`Mcpd::claim_steps`] takes.
#[derive(Debug, Clone)]
pub struct ClaimSteps {
    /// The name recorded as the owner of every lease taken.
    pub worker_id: String,
    /// How many leases to take at most, 1 to 100.
    pub max: u32,
    /// How long the daemon holds the request open for work.
    pub wait: Duration,
    /// Limit the claim to one run.
    pub run_id: Option<String>,
    /// Which project. `None` means the client's own.
    pub project: Option<String>,
}

impl Mcpd {
    fn project_param(&self, project: Option<&str>) -> Vec<(&'static str, String)> {
        vec![("project", project.unwrap_or(self.project()).to_owned())]
    }

    fn project_or_own(&self, project: Option<String>) -> Option<String> {
        Some(project.unwrap_or_else(|| self.project().to_owned()))
    }

    /// Store one version of a definition and read the version back. `None`
    /// for the project means the client's own.
    ///
    /// # Errors
    /// [`Error::Api`] with status 409 when the version exists with a different
    /// body, 400 when the body is not a definition the walker can run.
    pub async fn put_definition(
        &self,
        doc: &DefinitionDocument,
        project: Option<&str>,
    ) -> Result<DefinitionVersion, Error> {
        self.post("/v1/api/definitions", self.project_param(project), doc).await
    }

    /// One definition: the latest version, or the one named.
    ///
    /// # Errors
    /// As [`Mcpd::list_traces`].
    pub async fn get_definition(
        &self,
        id: &str,
        version: Option<u32>,
        project: Option<&str>,
    ) -> Result<Definition, Error> {
        let mut params = self.project_param(project);
        if let Some(v) = version {
            params.push(("version", v.to_string()));
        }
        self.get(&format!("/v1/api/definitions/{id}"), params).await
    }

    /// Every stored version in the project, or every version of one id.
    ///
    /// # Errors
    /// As [`Mcpd::list_traces`].
    pub async fn list_definitions(
        &self,
        id: Option<&str>,
        project: Option<&str>,
    ) -> Result<Vec<Definition>, Error> {
        let mut params = self.project_param(project);
        if let Some(id) = id {
            params.push(("id", id.to_owned()));
        }
        self.get("/v1/api/definitions", params).await
    }

    /// Start a run. The response is the run as stored, with its trace id.
    ///
    /// # Errors
    /// [`Error::Api`] when the definition is unknown or the input fails its
    /// schema; [`Error::Http`] when mcpd cannot be reached.
    pub async fn start_run(&self, req: StartRun) -> Result<Run, Error> {
        let project = self.project_or_own(req.project);
        let correlation = req.correlation.map(|mut c| {
            if c.project.is_empty() {
                c.project = project.clone().unwrap_or_default();
            }
            c
        });
        let body = StartRunRequest {
            project,
            definition_id: req.definition,
            version: req.version,
            input: req.input,
            correlation,
            idempotency_key: req.idempotency_key,
        };
        self.post("/v1/api/runs", Vec::new(), &body).await
    }

    /// One run and everything durable about it: its journal, deadlines and
    /// waits.
    ///
    /// # Errors
    /// As [`Mcpd::list_traces`].
    pub async fn get_run(&self, run_id: &str) -> Result<RunDetail, Error> {
        self.get(&format!("/v1/api/runs/{run_id}"), Vec::new()).await
    }

    /// Runs in the project, most recently updated first.
    ///
    /// # Errors
    /// As [`Mcpd::list_traces`].
    pub async fn list_runs(&self, filter: &RunFilter) -> Result<RunListResponse, Error> {
        let mut params = self.project_param(filter.project.as_deref());
        if let Some(s) = &filter.status {
            params.push(("status", serde_plain(s)));
        }
        if let Some(d) = &filter.definition_id {
            params.push(("definition_id", d.clone()));
        }
        if let Some(l) = filter.limit {
            params.push(("limit", l.to_string()));
        }
        if let Some(c) = &filter.cursor {
            params.push(("cursor", c.clone()));
        }
        self.get("/v1/api/runs", params).await
    }

    /// End a run and every child run under it. Cancelling an ended run
    /// changes nothing.
    ///
    /// # Errors
    /// As [`Mcpd::list_traces`].
    pub async fn cancel_run(&self, run_id: &str) -> Result<Run, Error> {
        self.post(
            &format!("/v1/api/runs/{run_id}/cancel"),
            Vec::new(),
            &Value::Object(Default::default()),
        )
        .await
    }

    /// A page of the run's journal, from `after` exclusive. Pass -1 to start
    /// at the beginning.
    ///
    /// # Errors
    /// As [`Mcpd::list_traces`].
    pub async fn run_events(
        &self,
        run_id: &str,
        after: i64,
        limit: Option<u32>,
    ) -> Result<RunEventPage, Error> {
        let mut params = vec![("after", after.to_string())];
        if let Some(l) = limit {
            params.push(("limit", l.to_string()));
        }
        self.get(&format!("/v1/api/runs/{run_id}/events"), params).await
    }

    /// Hand every event of a run to `f` as it happens, from `after`
    /// exclusive, until the run ends. A client that stops and starts again
    /// passes the last `seq` it saw and misses nothing that was committed.
    ///
    /// # Errors
    /// As [`Mcpd::list_traces`].
    pub async fn watch_run(
        &self,
        run_id: &str,
        after: i64,
        f: impl FnMut(RunEvent),
    ) -> Result<(), Error> {
        self.watch_run_every(run_id, after, Duration::from_millis(500), f).await
    }

    /// [`Mcpd::watch_run`] with the polling interval chosen by the caller.
    ///
    /// # Errors
    /// As [`Mcpd::list_traces`].
    pub async fn watch_run_every(
        &self,
        run_id: &str,
        mut after: i64,
        poll: Duration,
        mut f: impl FnMut(RunEvent),
    ) -> Result<(), Error> {
        loop {
            let page = self.run_events(run_id, after, Some(500)).await?;
            let mut ended = false;
            for event in page.events {
                after = after.max(i64::try_from(event.seq).unwrap_or(i64::MAX));
                if event.kind == RunEventKind::RunEnded {
                    ended = true;
                }
                f(event);
            }
            if ended {
                return Ok(());
            }
            if page.has_more {
                continue;
            }
            tokio::time::sleep(poll).await;
        }
    }

    /// Wait for a run to end and return it with its output. A run that ended
    /// failed, timed out or cancelled is [`Error::RunFailed`] carrying the
    /// run; one still going after `timeout` is [`Error::Config`] naming the
    /// wait.
    ///
    /// # Errors
    /// As above, and as [`Mcpd::list_traces`] for the reads.
    pub async fn wait_run(
        &self,
        run_id: &str,
        after: i64,
        timeout: Option<Duration>,
    ) -> Result<RunResult, Error> {
        self.wait_run_every(run_id, after, timeout, Duration::from_millis(500)).await
    }

    /// [`Mcpd::wait_run`] with the polling interval chosen by the caller.
    ///
    /// # Errors
    /// As [`Mcpd::wait_run`].
    pub async fn wait_run_every(
        &self,
        run_id: &str,
        mut after: i64,
        timeout: Option<Duration>,
        poll: Duration,
    ) -> Result<RunResult, Error> {
        let deadline = timeout.map(|t| tokio::time::Instant::now() + t);
        loop {
            let page = self.run_events(run_id, after, Some(500)).await?;
            let mut ended = false;
            for event in page.events {
                after = after.max(i64::try_from(event.seq).unwrap_or(i64::MAX));
                if event.kind == RunEventKind::RunEnded {
                    ended = true;
                }
            }
            if ended {
                break;
            }
            if page.has_more {
                continue;
            }
            match deadline {
                Some(d) if tokio::time::Instant::now() >= d => {
                    return Err(Error::Config(format!(
                        "run {run_id} is still running after {:?}",
                        timeout.unwrap_or_default()
                    )));
                }
                Some(d) => {
                    tokio::time::sleep(
                        poll.min(d.saturating_duration_since(tokio::time::Instant::now())),
                    )
                    .await
                }
                None => tokio::time::sleep(poll).await,
            }
        }
        let run = self.get_run(run_id).await?.run;
        if run.status != RunStatus::Succeeded {
            return Err(Error::RunFailed(Box::new(run)));
        }
        Ok(RunResult {
            status: run.status,
            output: run.output.clone(),
            cost_usd: run.cost_usd,
            steps_used: run.steps_used,
            trace_id: run.trace_id.clone(),
            run,
        })
    }

    /// [`Mcpd::start_run`], then [`Mcpd::wait_run`].
    ///
    /// # Errors
    /// As [`Mcpd::start_run`] and [`Mcpd::wait_run`].
    pub async fn start_run_and_wait(
        &self,
        req: StartRun,
        timeout: Option<Duration>,
    ) -> Result<RunResult, Error> {
        let run = self.start_run(req).await?;
        self.wait_run(&run.run_id, -1, timeout).await
    }

    /// Deliver an event to one run through its own open wait.
    ///
    /// # Errors
    /// As [`Mcpd::list_traces`].
    pub async fn signal_run(
        &self,
        run_id: &str,
        event: &str,
        payload: Option<Value>,
        event_id: Option<&str>,
    ) -> Result<EventReceipt, Error> {
        let mut body = serde_json::Map::new();
        body.insert("event".into(), event.into());
        if let Some(p) = payload {
            body.insert("payload".into(), p);
        }
        if let Some(id) = event_id {
            body.insert("event_id".into(), id.into());
        }
        self.post(&format!("/v1/api/runs/{run_id}/signal"), Vec::new(), &Value::Object(body)).await
    }

    /// Route an event by name and key to the run waiting for it.
    ///
    /// # Errors
    /// As [`Mcpd::list_traces`].
    pub async fn publish_event(&self, ev: PublishEvent) -> Result<EventReceipt, Error> {
        let body = PublishEventRequest {
            project: self.project_or_own(ev.project),
            event: ev.event,
            key: ev.key,
            payload: ev.payload,
            event_id: ev.event_id,
        };
        self.post("/v1/api/events", Vec::new(), &body).await
    }

    /// Announce a worker and store what it serves.
    ///
    /// # Errors
    /// As [`Mcpd::put_definition`].
    pub async fn register_worker(
        &self,
        worker_id: &str,
        definitions: Vec<DefinitionDocument>,
        project: Option<&str>,
    ) -> Result<WorkerRegistered, Error> {
        let body = WorkerRegistration {
            project: self.project_or_own(project.map(str::to_owned)),
            worker_id: worker_id.to_owned(),
            definitions,
        };
        self.post("/v1/api/workers/register", Vec::new(), &body).await
    }

    /// Offer up to `max` runnable steps, waiting up to `wait` for one.
    ///
    /// # Errors
    /// As [`Mcpd::list_traces`].
    pub async fn claim_steps(&self, req: ClaimSteps) -> Result<ClaimResponse, Error> {
        let body = ClaimRequest {
            project: self.project_or_own(req.project),
            worker_id: req.worker_id,
            max: req.max,
            wait_ms: u32::try_from(req.wait.as_millis()).unwrap_or(u32::MAX),
            run_id: req.run_id,
        };
        self.transport()?.post_held("/v1/api/steps/claim", &[], &body, req.wait).await
    }

    /// Declare that the body is running. Required under `at_most_once`.
    ///
    /// # Errors
    /// [`Error::Conflict`] with the daemon's reason; otherwise as
    /// [`Mcpd::list_traces`].
    pub async fn step_started(&self, lease_id: &str) -> Result<LeaseState, Error> {
        self.post(&format!("/v1/api/steps/{lease_id}/started"), Vec::new(), &empty()).await
    }

    /// Extend the lease. The only evidence the daemon has that a worker is
    /// alive.
    ///
    /// # Errors
    /// As [`Mcpd::step_started`].
    pub async fn step_heartbeat(&self, lease_id: &str) -> Result<LeaseState, Error> {
        self.post(&format!("/v1/api/steps/{lease_id}/heartbeat"), Vec::new(), &empty()).await
    }

    /// Finish the step.
    ///
    /// # Errors
    /// As [`Mcpd::step_started`].
    pub async fn complete_step(
        &self,
        lease_id: &str,
        result: &StepResult,
    ) -> Result<StepAck, Error> {
        self.post(&format!("/v1/api/steps/{lease_id}/complete"), Vec::new(), result).await
    }

    /// Fail the step, with the class that decides whether it is retried.
    ///
    /// # Errors
    /// As [`Mcpd::step_started`].
    pub async fn fail_step(&self, lease_id: &str, error: StepError) -> Result<StepAck, Error> {
        let body = StepFailure { error };
        self.post(&format!("/v1/api/steps/{lease_id}/fail"), Vec::new(), &body).await
    }
}

fn empty() -> Value {
    Value::Object(serde_json::Map::new())
}

fn serde_plain<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()
}
