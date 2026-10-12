//! The authoring surface for durable workflows.
//!
//! A workflow is data the daemon holds and bodies the daemon never sees. The
//! builder produces both from one file: [`Workflow::document`] is the
//! definition the daemon walks, in the shape `spec/definition.schema.json`
//! describes, and the handlers stay in this process for a [`Worker`] to serve.
//! There is no YAML to keep in step with the code, because the code is where
//! the graph is written.
//!
//! Every construct of the run contract has a method here. A branch, a foreach
//! body and a loop body are one claimable step each; a multi-step body is a
//! [`Workflow::subworkflow`].
//!
//! [`Worker`]: crate::worker::Worker

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use mcpd_core::{Backoff, DefinitionDocument, DefinitionKind, Effect, OnError, WaitTimeout};
use serde::Serialize;
use serde_json::{json, Map, Value};

use crate::worker::{StepContext, StepFailure};
use crate::Error;

// ------------------------------------------------------------ predicates ---

/// A guard the daemon evaluates: a leaf from [`when`] or [`exists`], or
/// [`all`], [`any`] and [`not`] over leaves. A path reads `input`,
/// `steps.<id>`, `run`, and `item`, `index` or `iter.index` where they apply.
pub type Predicate = Value;

/// A leaf predicate: `when("steps.order.output.amount_usd", "gt", 100)`.
/// `op` is one of eq, ne, lt, lte, gt, gte, in, matches.
pub fn when(path: &str, op: &str, value: impl Serialize) -> Predicate {
    json!({ "path": path, "op": op, "value": value })
}

/// A leaf predicate that holds when the path has a value.
#[must_use]
pub fn exists(path: &str) -> Predicate {
    json!({ "path": path, "op": "exists" })
}

/// Holds when every predicate holds.
pub fn all(predicates: impl IntoIterator<Item = Predicate>) -> Predicate {
    json!({ "all": predicates.into_iter().collect::<Vec<_>>() })
}

/// Holds when at least one predicate holds.
pub fn any(predicates: impl IntoIterator<Item = Predicate>) -> Predicate {
    json!({ "any": predicates.into_iter().collect::<Vec<_>>() })
}

/// Holds when the predicate does not.
#[must_use]
pub fn not(predicate: Predicate) -> Predicate {
    json!({ "not": predicate })
}

/// How failures classed `retryable` are retried. `max_attempts` counts every
/// attempt including the first, so one means no retry.
#[derive(Debug, Clone, Default)]
pub struct RetryPolicy {
    /// Every attempt including the first. The daemon's default is one.
    pub max_attempts: Option<u32>,
    /// Fixed or exponential.
    pub backoff: Option<Backoff>,
    /// The first wait.
    pub initial: Option<Duration>,
    /// The longest wait, for exponential backoff.
    pub max: Option<Duration>,
}

impl RetryPolicy {
    /// A policy with `max_attempts` attempts in all.
    #[must_use]
    pub fn attempts(max_attempts: u32) -> Self {
        Self { max_attempts: Some(max_attempts), ..Self::default() }
    }

    /// Exponential backoff from `initial`.
    #[must_use]
    pub fn exponential(mut self, initial: Duration) -> Self {
        self.backoff = Some(Backoff::Exponential);
        self.initial = Some(initial);
        self
    }

    /// Fixed backoff of `initial` every time.
    #[must_use]
    pub fn fixed(mut self, initial: Duration) -> Self {
        self.backoff = Some(Backoff::Fixed);
        self.initial = Some(initial);
        self
    }

    /// The longest wait between attempts.
    #[must_use]
    pub fn max(mut self, max: Duration) -> Self {
        self.max = Some(max);
        self
    }

    fn wire(&self) -> Value {
        let mut out = Map::new();
        if let Some(n) = self.max_attempts {
            out.insert("max_attempts".into(), n.into());
        }
        if let Some(b) = &self.backoff {
            out.insert("backoff".into(), json!(b));
        }
        if let Some(d) = self.initial {
            out.insert("initial_ms".into(), ms(d).into());
        }
        if let Some(d) = self.max {
            out.insert("max_ms".into(), ms(d).into());
        }
        Value::Object(out)
    }
}

/// The ceilings a workflow declares.
#[derive(Debug, Clone, Default)]
pub struct Limits {
    /// How many step attempts a run may spend.
    pub max_steps: Option<u32>,
    /// How long a run may live.
    pub run_timeout: Option<Duration>,
    /// How much a run may cost, from the usage its steps report.
    pub max_cost_usd: Option<f64>,
}

impl Limits {
    fn wire(&self) -> Value {
        let mut out = Map::new();
        if let Some(n) = self.max_steps {
            out.insert("max_steps".into(), n.into());
        }
        if let Some(d) = self.run_timeout {
            out.insert("run_timeout_ms".into(), ms(d).into());
        }
        if let Some(c) = self.max_cost_usd {
            out.insert("max_cost_usd".into(), json!(c));
        }
        Value::Object(out)
    }
}

/// What a node accepts. Each field says which constructs read it; the others
/// ignore it.
#[derive(Debug, Clone, Default)]
pub struct StepOptions {
    when: Option<Predicate>,
    input: Option<Value>,
    effect: Option<Effect>,
    timeout: Option<Duration>,
    retry: Option<RetryPolicy>,
    on_error: Option<OnError>,
    accept_parked: Option<bool>,
    on_timeout: Option<WaitTimeout>,
    concurrency: Option<u32>,
    until: Option<Predicate>,
    version: Option<u32>,
}

impl StepOptions {
    /// No options: the daemon's defaults throughout.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A guard. False writes the node as `skipped` and the run moves on.
    #[must_use]
    pub fn when(mut self, predicate: Predicate) -> Self {
        self.when = Some(predicate);
        self
    }

    /// The input template. Default: the run's input. `${path}` references are
    /// rendered per attempt. Read by step, agent and subworkflow.
    #[must_use]
    pub fn input(mut self, template: Value) -> Self {
        self.input = Some(template);
        self
    }

    /// What a duplicated body would cost. Default `at_least_once`.
    #[must_use]
    pub fn effect(mut self, effect: Effect) -> Self {
        self.effect = Some(effect);
        self
    }

    /// `effect(Effect::AtMostOnce)`: `started` must be posted before the body,
    /// and a started attempt whose lease is lost fails rather than runs again.
    #[must_use]
    pub fn at_most_once(self) -> Self {
        self.effect(Effect::AtMostOnce)
    }

    /// One attempt's own ceiling, heartbeats or not.
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// How failures classed `retryable` are retried.
    #[must_use]
    pub fn retry(mut self, policy: RetryPolicy) -> Self {
        self.retry = Some(policy);
        self
    }

    /// What happens after the retries. Default `fail_run`.
    #[must_use]
    pub fn on_error(mut self, on_error: OnError) -> Self {
        self.on_error = Some(on_error);
        self
    }

    /// `on_error(OnError::Continue)`: the node is marked failed and the run
    /// moves on.
    #[must_use]
    pub fn continue_on_error(self) -> Self {
        self.on_error(OnError::Continue)
    }

    /// Let a wait take an event that arrived before it opened.
    #[must_use]
    pub fn accept_parked(mut self) -> Self {
        self.accept_parked = Some(true);
        self
    }

    /// What a wait does when its deadline passes. Default `fail`.
    #[must_use]
    pub fn on_timeout(mut self, on_timeout: WaitTimeout) -> Self {
        self.on_timeout = Some(on_timeout);
        self
    }

    /// How many foreach items run at once.
    #[must_use]
    pub fn concurrency(mut self, concurrency: u32) -> Self {
        self.concurrency = Some(concurrency);
        self
    }

    /// Ends a loop early. Evaluated after each iteration; reads
    /// `steps.<body id>` for that iteration.
    #[must_use]
    pub fn until(mut self, predicate: Predicate) -> Self {
        self.until = Some(predicate);
        self
    }

    /// Pins a subworkflow's definition version. Absent, the highest stored
    /// version at start time is used.
    #[must_use]
    pub fn version(mut self, version: u32) -> Self {
        self.version = Some(version);
        self
    }
}

// -------------------------------------------------------------- handlers ---

/// A boxed future, as the handlers are stored.
pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

/// A step body as stored: takes the context, returns the output as JSON.
pub type StepHandler =
    Arc<dyn Fn(StepContext) -> BoxFuture<Result<Value, StepFailure>> + Send + Sync>;

fn wrap<F, Fut, T, E>(f: F) -> StepHandler
where
    F: Fn(StepContext) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<T, E>> + Send + 'static,
    T: Serialize,
    E: Into<StepFailure>,
{
    Arc::new(move |ctx| {
        let fut = f(ctx);
        Box::pin(async move {
            match fut.await {
                Ok(value) => serde_json::to_value(value).map_err(|e| {
                    StepFailure::fatal(format!("the step's output could not be encoded: {e}"))
                }),
                Err(e) => Err(e.into()),
            }
        })
    })
}

fn ms(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

fn check_id(id: &str) -> Result<(), Error> {
    let mut chars = id.chars();
    let ok = matches!(chars.next(), Some('a'..='z'))
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if ok {
        Ok(())
    } else {
        Err(Error::Config(format!("\"{id}\" is not a step id; an id matches ^[a-z][a-z0-9_]*$")))
    }
}

fn common(mut node: Map<String, Value>, options: &StepOptions) -> Map<String, Value> {
    if let Some(p) = &options.when {
        node.insert("when".into(), p.clone());
    }
    if let Some(o) = &options.on_error {
        node.insert("on_error".into(), json!(o));
    }
    node
}

fn claimable(kind: &str, id: &str, options: &StepOptions) -> Value {
    let mut node = Map::new();
    node.insert("kind".into(), kind.into());
    node.insert("id".into(), id.into());
    let mut node = common(node, options);
    if let Some(i) = &options.input {
        node.insert("input".into(), i.clone());
    }
    if let Some(e) = &options.effect {
        node.insert("effect".into(), json!(e));
    }
    if let Some(t) = options.timeout {
        node.insert("step_timeout_ms".into(), ms(t).into());
    }
    if let Some(r) = &options.retry {
        node.insert("retry".into(), r.wire());
    }
    Value::Object(node)
}

fn node(
    kind: &str,
    id: &str,
    fields: &[(&str, Value)],
    options: &StepOptions,
) -> Map<String, Value> {
    let mut node = Map::new();
    node.insert("kind".into(), kind.into());
    node.insert("id".into(), id.into());
    for (k, v) in fields {
        node.insert((*k).into(), v.clone());
    }
    common(node, options)
}

/// What a parallel branch, a foreach body or a loop body may hold: one
/// claimable step each.
#[derive(Default)]
pub struct Branch {
    nodes: Vec<Value>,
    handlers: HashMap<String, StepHandler>,
    err: Option<Error>,
}

impl Branch {
    /// A branch or body that a worker serves.
    pub fn step<F, Fut, T, E>(&mut self, id: &str, body: F, options: StepOptions) -> &mut Self
    where
        F: Fn(StepContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<T, E>> + Send + 'static,
        T: Serialize,
        E: Into<StepFailure>,
    {
        self.add("task", id, wrap(body), &options)
    }

    /// The same, recorded as an agent turn.
    pub fn agent<F, Fut, T, E>(&mut self, id: &str, body: F, options: StepOptions) -> &mut Self
    where
        F: Fn(StepContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<T, E>> + Send + 'static,
        T: Serialize,
        E: Into<StepFailure>,
    {
        self.add("agent", id, wrap(body), &options)
    }

    fn add(
        &mut self,
        kind: &str,
        id: &str,
        handler: StepHandler,
        options: &StepOptions,
    ) -> &mut Self {
        if let Err(e) = check_id(id) {
            self.err.get_or_insert(e);
            return self;
        }
        self.nodes.push(claimable(kind, id, options));
        self.handlers.insert(id.to_owned(), handler);
        self
    }
}

/// A workflow under construction. Steps run in the order they are added.
///
/// A mistake in the graph, such as an id the daemon would refuse, is kept and
/// returned by [`Workflow::document`], so a builder chain reads without a
/// `?` at every step.
pub struct Workflow {
    id: String,
    version: Option<u32>,
    input_schema: Option<Value>,
    limits: Option<Limits>,
    nodes: Vec<Value>,
    handlers: HashMap<String, StepHandler>,
    /// The body id a foreach or loop root maps to, so a lease's key finds its handler.
    body_of: HashMap<String, String>,
    output: Option<Value>,
    err: Option<Error>,
}

impl std::fmt::Debug for Workflow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Workflow")
            .field("id", &self.id)
            .field("version", &self.version)
            .field("steps", &self.nodes.len())
            .finish_non_exhaustive()
    }
}

impl Workflow {
    /// Start building a workflow. `id` is unique within the project.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        let id = id.into();
        let err = check_id(&id).err();
        Self {
            id,
            version: None,
            input_schema: None,
            limits: None,
            nodes: Vec::new(),
            handlers: HashMap::new(),
            body_of: HashMap::new(),
            output: None,
            err,
        }
    }

    /// Name the version this document owns. Without it the daemon assigns the
    /// next number.
    #[must_use]
    pub fn with_version(mut self, version: u32) -> Self {
        self.version = Some(version);
        self
    }

    /// A JSON Schema the run's input must satisfy.
    #[must_use]
    pub fn with_input_schema(mut self, schema: Value) -> Self {
        self.input_schema = Some(schema);
        self
    }

    /// The ceilings.
    #[must_use]
    pub fn with_limits(mut self, limits: Limits) -> Self {
        self.limits = Some(limits);
        self
    }

    /// The definition id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The version this builder owns, when it named one.
    #[must_use]
    pub fn version(&self) -> Option<u32> {
        self.version
    }

    /// The first mistake made while building, if any.
    #[must_use]
    pub fn err(&self) -> Option<&Error> {
        self.err.as_ref()
    }

    fn fail(mut self, err: Error) -> Self {
        self.err.get_or_insert(err);
        self
    }

    /// A claimable step a worker serves. The body takes a [`StepContext`] and
    /// returns its output, or a [`StepFailure`].
    #[must_use]
    pub fn step<F, Fut, T, E>(self, id: &str, body: F, options: StepOptions) -> Self
    where
        F: Fn(StepContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<T, E>> + Send + 'static,
        T: Serialize,
        E: Into<StepFailure>,
    {
        self.add("task", id, wrap(body), &options)
    }

    /// A claimable step recorded as an agent turn: your own loop, however
    /// many turns, as one durable step.
    #[must_use]
    pub fn agent<F, Fut, T, E>(self, id: &str, body: F, options: StepOptions) -> Self
    where
        F: Fn(StepContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<T, E>> + Send + 'static,
        T: Serialize,
        E: Into<StepFailure>,
    {
        self.add("agent", id, wrap(body), &options)
    }

    fn add(mut self, kind: &str, id: &str, handler: StepHandler, options: &StepOptions) -> Self {
        if let Err(e) = check_id(id) {
            return self.fail(e);
        }
        self.nodes.push(claimable(kind, id, options));
        self.handlers.insert(id.to_owned(), handler);
        self
    }

    /// A delay. No process is alive during it.
    #[must_use]
    pub fn sleep(mut self, id: &str, duration: Duration, options: StepOptions) -> Self {
        if let Err(e) = check_id(id) {
            return self.fail(e);
        }
        self.nodes.push(Value::Object(node("sleep", id, &[("ms", ms(duration).into())], &options)));
        self
    }

    /// A delay until an instant rendered from a template such as
    /// `${input.billing_date}`.
    #[must_use]
    pub fn sleep_until(mut self, id: &str, at: &str, options: StepOptions) -> Self {
        if let Err(e) = check_id(id) {
            return self.fail(e);
        }
        self.nodes.push(Value::Object(node("sleep_until", id, &[("at", at.into())], &options)));
        self
    }

    /// A pause until an event arrives. The daemon holds the deadline and the
    /// mailbox; `key` is rendered per run, so `${input.order_id}` addresses
    /// this run and no other.
    #[must_use]
    pub fn wait_for(
        mut self,
        id: &str,
        event: &str,
        key: &str,
        timeout: Duration,
        options: StepOptions,
    ) -> Self {
        if let Err(e) = check_id(id) {
            return self.fail(e);
        }
        let mut n = node(
            "wait_event",
            id,
            &[("event", event.into()), ("key", key.into()), ("timeout_ms", ms(timeout).into())],
            &options,
        );
        if let Some(a) = options.accept_parked {
            n.insert("accept_parked".into(), a.into());
        }
        if let Some(t) = &options.on_timeout {
            n.insert("on_timeout".into(), json!(t));
        }
        self.nodes.push(Value::Object(n));
        self
    }

    /// Named branches, each one claimable step, all on disk before any is
    /// offered.
    #[must_use]
    pub fn parallel(
        mut self,
        id: &str,
        build: impl FnOnce(&mut Branch),
        options: StepOptions,
    ) -> Self {
        if let Err(e) = check_id(id) {
            return self.fail(e);
        }
        let mut b = Branch::default();
        build(&mut b);
        if let Some(e) = b.err {
            return self.fail(e);
        }
        self.nodes.push(Value::Object(node(
            "parallel",
            id,
            &[("branches", Value::Array(b.nodes))],
            &options,
        )));
        self.handlers.extend(b.handlers);
        self
    }

    /// One claimable step per element of the array at `over`. The body's
    /// input template can read `item` and `index`; afterwards
    /// `steps.<id>.outputs` is the array of outputs in item order.
    #[must_use]
    pub fn foreach(
        mut self,
        id: &str,
        over: &str,
        max_items: u32,
        build: impl FnOnce(&mut Branch),
        options: StepOptions,
    ) -> Self {
        if let Err(e) = check_id(id) {
            return self.fail(e);
        }
        let (body, handlers) = match one_body(id, "foreach", build) {
            Ok(v) => v,
            Err(e) => return self.fail(e),
        };
        let mut n = node(
            "foreach",
            id,
            &[("over", over.into()), ("body", body), ("max_items", max_items.into())],
            &options,
        );
        if let Some(c) = options.concurrency {
            n.insert("concurrency".into(), c.into());
        }
        self.nodes.push(Value::Object(n));
        self.adopt(id, handlers);
        self
    }

    /// The loop construct: a bounded loop. [`StepOptions::until`] ends it
    /// early; `iter.index` is the iteration number.
    #[must_use]
    pub fn repeat(
        mut self,
        id: &str,
        max_iterations: u32,
        build: impl FnOnce(&mut Branch),
        options: StepOptions,
    ) -> Self {
        if let Err(e) = check_id(id) {
            return self.fail(e);
        }
        let (body, handlers) = match one_body(id, "loop", build) {
            Ok(v) => v,
            Err(e) => return self.fail(e),
        };
        let mut n = node(
            "loop",
            id,
            &[("body", body), ("max_iterations", max_iterations.into())],
            &options,
        );
        if let Some(u) = &options.until {
            n.insert("until".into(), u.clone());
        }
        self.nodes.push(Value::Object(n));
        self.adopt(id, handlers);
        self
    }

    /// A child run of another definition. Its output becomes
    /// `steps.<id>.output`.
    #[must_use]
    pub fn subworkflow(mut self, id: &str, definition: &str, options: StepOptions) -> Self {
        if let Err(e) = check_id(id) {
            return self.fail(e);
        }
        let mut n = node("subworkflow", id, &[("definition", definition.into())], &options);
        if let Some(v) = options.version {
            n.insert("version".into(), v.into());
        }
        if let Some(i) = &options.input {
            n.insert("input".into(), i.clone());
        }
        self.nodes.push(Value::Object(n));
        self
    }

    /// The template rendered when the run reaches the end of the graph.
    #[must_use]
    pub fn output(mut self, template: Value) -> Self {
        self.output = Some(template);
        self
    }

    /// The definition document the daemon stores and walks.
    ///
    /// # Errors
    /// The first mistake made while building: an id the daemon would refuse,
    /// or a foreach or loop with more or fewer than one body step.
    pub fn document(&self) -> Result<DefinitionDocument, Error> {
        if let Some(e) = &self.err {
            return Err(Error::Config(e.to_string()));
        }
        let mut body = Map::new();
        body.insert("steps".into(), Value::Array(self.nodes.clone()));
        if let Some(s) = &self.input_schema {
            body.insert("input_schema".into(), s.clone());
        }
        if let Some(l) = &self.limits {
            body.insert("limits".into(), l.wire());
        }
        if let Some(o) = &self.output {
            body.insert("output".into(), o.clone());
        }
        Ok(DefinitionDocument {
            id: self.id.clone(),
            kind: Some(DefinitionKind::Workflow),
            version: self.version,
            body: Value::Object(body),
        })
    }

    /// The handler a lease's step key addresses. `order`, `checks/fraud`,
    /// `notify[3]` and `refine[2]/rewrite` each resolve to the lexical id
    /// that registered the body.
    #[must_use]
    pub fn handler_for(&self, step_key: &str) -> Option<StepHandler> {
        let (head, sub) = match step_key.split_once('/') {
            Some((h, s)) => (h, Some(s)),
            None => (step_key, None),
        };
        let (root, indexed) = match head.split_once('[') {
            Some((r, _)) => (r, true),
            None => (head, false),
        };
        if let Some(sub) = sub {
            return self.handlers.get(sub).cloned();
        }
        if indexed {
            let body = self.body_of.get(root).map_or(root, String::as_str);
            return self.handlers.get(body).cloned();
        }
        self.handlers.get(root).cloned()
    }

    fn adopt(&mut self, id: &str, handlers: HashMap<String, StepHandler>) {
        for (body, handler) in handlers {
            self.body_of.insert(id.to_owned(), body.clone());
            self.handlers.insert(body, handler);
        }
    }
}

fn one_body(
    id: &str,
    what: &str,
    build: impl FnOnce(&mut Branch),
) -> Result<(Value, HashMap<String, StepHandler>), Error> {
    let mut b = Branch::default();
    build(&mut b);
    if let Some(e) = b.err {
        return Err(e);
    }
    if b.nodes.len() != 1 {
        return Err(Error::Config(format!(
            "{what} \"{id}\" needs exactly one body step; use a subworkflow for more"
        )));
    }
    Ok((b.nodes.remove(0), b.handlers))
}
