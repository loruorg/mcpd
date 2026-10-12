//! The definition document of section 3, and the walk over it.
//!
//! `steps` is an ordered sequence and the forward walk visits it in array
//! order. That is the whole of the ordering rule. Guards are evaluated when
//! the cursor reaches a node, templates are rendered per attempt, and both
//! read the section 3.3 context that [`crate::context`] builds from the
//! journal the transaction already holds. Nothing here calls out to anything.
//!
//! Unknown fields are refused on the document and on every node, so a guard
//! that is spelled wrong is an error and not a run that skips its guard.
//!
//! `on_error` is accepted on every node because the contract says so. It is
//! consulted where a node can fail after it has started: a claimable step
//! after its retries, and a `subworkflow` whose child ended badly. A
//! `wait_event` has `on_timeout` instead. A `sleep`, a `parallel`, a `foreach`
//! or a `loop` has no failure of its own after it starts; a branch or body
//! that fails follows the branch's or body's own `on_error`, and a document
//! that cannot be walked at a position (an `over` that is not an array, an
//! `at` that is not a timestamp) fails the run whatever the node's `on_error`
//! says.

use crate::context::{context_for, parse_key, Journal, Scope};
use crate::model::{
    Backoff, Construct, Cursor, Effect, OnError, Run, RunStatus, Slot, StepKind, WaitTimeout,
};
use crate::predicate::Predicate;
use crate::template::{render_string, resolve};
use crate::{after_ms, new_span_id, Error, Result};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use time::OffsetDateTime;

fn accept_parked_default() -> bool {
    true
}

fn concurrency_default() -> usize {
    8
}

fn max_attempts_default() -> u32 {
    1
}

fn initial_ms_default() -> i64 {
    1000
}

fn max_ms_default() -> i64 {
    60_000
}

/// A retry policy. `max_attempts` counts every attempt including the first,
/// so the default of one means no retry.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Retry {
    /// How many attempts in all.
    #[serde(default = "max_attempts_default")]
    pub max_attempts: u32,
    /// How retries are spaced.
    #[serde(default)]
    pub backoff: Backoff,
    /// The first delay.
    #[serde(default = "initial_ms_default")]
    pub initial_ms: i64,
    /// The cap on any delay.
    #[serde(default = "max_ms_default")]
    pub max_ms: i64,
}

impl Default for Retry {
    fn default() -> Self {
        Self {
            max_attempts: max_attempts_default(),
            backoff: Backoff::default(),
            initial_ms: initial_ms_default(),
            max_ms: max_ms_default(),
        }
    }
}

impl Retry {
    /// The delay before retry number `n`, counted from one, capped at
    /// `max_ms`.
    #[must_use]
    pub fn delay_ms(&self, n: u32) -> i64 {
        let raw = match self.backoff {
            Backoff::Fixed => self.initial_ms,
            Backoff::Exponential => {
                let shift = n.saturating_sub(1).min(62);
                self.initial_ms.saturating_mul(1i64 << shift)
            }
        };
        raw.min(self.max_ms).max(0)
    }
}

/// The ceilings a definition declares. All optional.
#[derive(Debug, Clone, PartialEq, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    /// How many steps the run may complete.
    #[serde(default)]
    pub max_steps: Option<i64>,
    /// How long the whole run may take.
    #[serde(default)]
    pub run_timeout_ms: Option<i64>,
    /// How much the run may spend, in USD.
    #[serde(default)]
    pub max_cost_usd: Option<f64>,
}

/// One node of a graph. Section 3.1.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Node {
    /// Claimable work.
    Task {
        /// The lexical step id.
        id: String,
        /// The guard.
        #[serde(default)]
        when: Option<Predicate>,
        /// What a failure after the retries does.
        #[serde(default)]
        on_error: OnError,
        /// The input template. Default: the run's input.
        #[serde(default)]
        input: Option<Value>,
        /// What a duplicated body would cost.
        #[serde(default)]
        effect: Effect,
        /// One attempt's own ceiling.
        #[serde(default)]
        step_timeout_ms: Option<i64>,
        /// The retry policy.
        #[serde(default)]
        retry: Retry,
    },
    /// Claimable work whose body is the agent loop. Identical on the wire; the
    /// kind is recorded so the trace can tell an agent turn from a step.
    Agent {
        /// The lexical step id.
        id: String,
        /// The guard.
        #[serde(default)]
        when: Option<Predicate>,
        /// What a failure after the retries does.
        #[serde(default)]
        on_error: OnError,
        /// The input template. Default: the run's input.
        #[serde(default)]
        input: Option<Value>,
        /// What a duplicated body would cost.
        #[serde(default)]
        effect: Effect,
        /// One attempt's own ceiling.
        #[serde(default)]
        step_timeout_ms: Option<i64>,
        /// The retry policy.
        #[serde(default)]
        retry: Retry,
    },
    /// A relative delay.
    Sleep {
        /// The lexical step id.
        id: String,
        /// The guard.
        #[serde(default)]
        when: Option<Predicate>,
        /// Accepted on every node; see the module comment.
        #[serde(default)]
        on_error: OnError,
        /// How long.
        ms: i64,
    },
    /// An absolute delay.
    SleepUntil {
        /// The lexical step id.
        id: String,
        /// The guard.
        #[serde(default)]
        when: Option<Predicate>,
        /// Accepted on every node; see the module comment.
        #[serde(default)]
        on_error: OnError,
        /// A template rendering to an RFC 3339 timestamp.
        at: String,
    },
    /// A suspension an inbound event resumes.
    WaitEvent {
        /// The lexical step id.
        id: String,
        /// The guard.
        #[serde(default)]
        when: Option<Predicate>,
        /// Accepted on every node; a wait's own failure path is `on_timeout`.
        #[serde(default)]
        on_error: OnError,
        /// The event name.
        event: String,
        /// The correlation key template, rendered per run.
        key: String,
        /// How long to wait.
        timeout_ms: i64,
        /// Whether an event that arrived early counts.
        #[serde(default = "accept_parked_default")]
        accept_parked: bool,
        /// What the deadline passing does.
        #[serde(default)]
        on_timeout: WaitTimeout,
    },
    /// A fixed fan-out.
    Parallel {
        /// The lexical step id.
        id: String,
        /// The guard.
        #[serde(default)]
        when: Option<Predicate>,
        /// Accepted on every node; see the module comment.
        #[serde(default)]
        on_error: OnError,
        /// One claimable node per branch.
        branches: Vec<Node>,
    },
    /// A fan-out over items.
    Foreach {
        /// The lexical step id.
        id: String,
        /// The guard.
        #[serde(default)]
        when: Option<Predicate>,
        /// Accepted on every node; see the module comment.
        #[serde(default)]
        on_error: OnError,
        /// A path that must resolve to an array.
        over: String,
        /// The one claimable node run per item.
        body: Box<Node>,
        /// How many items may be claimable at once.
        #[serde(default = "concurrency_default")]
        concurrency: usize,
        /// The most items the array may hold. Required: a fan-out with no
        /// bound is a run that no ceiling above it can price.
        max_items: usize,
    },
    /// A bounded loop.
    Loop {
        /// The lexical step id.
        id: String,
        /// The guard.
        #[serde(default)]
        when: Option<Predicate>,
        /// Accepted on every node; see the module comment.
        #[serde(default)]
        on_error: OnError,
        /// The one claimable node run per iteration.
        body: Box<Node>,
        /// The bound. Required, because an unbounded loop is a run that never
        /// ends and no ceiling above it is cheaper than this one.
        max_iterations: u32,
        /// Evaluated after each iteration; absent means run the bound.
        #[serde(default)]
        until: Option<Predicate>,
    },
    /// A child run.
    Subworkflow {
        /// The lexical step id.
        id: String,
        /// The guard.
        #[serde(default)]
        when: Option<Predicate>,
        /// What a child that ends badly does to this run.
        #[serde(default)]
        on_error: OnError,
        /// Which definition the child runs.
        definition: String,
        /// Which version. Absent pins the highest stored version at the
        /// parent's start.
        #[serde(default)]
        version: Option<u32>,
        /// The child's input template.
        #[serde(default)]
        input: Option<Value>,
    },
}

/// The policy of a claimable node, viewed uniformly whatever it is nested in.
#[derive(Debug, Clone, Copy)]
pub struct Claimable<'a> {
    /// The lexical id.
    pub id: &'a str,
    /// Task or agent.
    pub kind: StepKind,
    /// The guard.
    pub when: Option<&'a Predicate>,
    /// What a failure after the retries does.
    pub on_error: OnError,
    /// The input template.
    pub input: Option<&'a Value>,
    /// What a duplicated body would cost.
    pub effect: Effect,
    /// One attempt's own ceiling.
    pub step_timeout_ms: Option<i64>,
    /// The retry policy.
    pub retry: &'a Retry,
}

/// Where a node sits in a document, which is what decides the shape of the
/// step keys it produces.
///
/// A step key folds the enclosing branch, item or iteration into the lexical
/// id, so the same node produces `issue`, `refund/issue`, `issue[3]` or
/// `poll[7]/issue` depending only on this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Site<'a> {
    /// A top-level node. Its step key is its id.
    Top,
    /// A branch of a `parallel`, named by the parallel's id. Its step key is
    /// `parent/id`.
    Branch(&'a str),
    /// The body of a `foreach`, named by the foreach's id. Its step key is
    /// `parent[item]`.
    ForeachBody(&'a str),
    /// The body of a `loop`, named by the loop's id. Its step key is
    /// `parent[iteration]/id`.
    LoopBody(&'a str),
}

/// A node together with where it sits and what encloses it.
#[derive(Debug, Clone, Copy)]
pub struct Placed<'a> {
    /// The node itself.
    pub node: &'a Node,
    /// The top-level node it hangs off, which is the node itself when it is
    /// top level. A branch or a body is reached only through its container, so
    /// the container's guard and fan-out decide it too.
    pub container: &'a Node,
    /// Where it sits.
    pub site: Site<'a>,
}

impl Node {
    /// The lexical step id.
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Node::Task { id, .. }
            | Node::Agent { id, .. }
            | Node::Sleep { id, .. }
            | Node::SleepUntil { id, .. }
            | Node::WaitEvent { id, .. }
            | Node::Parallel { id, .. }
            | Node::Foreach { id, .. }
            | Node::Loop { id, .. }
            | Node::Subworkflow { id, .. } => id,
        }
    }

    /// The wire spelling of the kind.
    #[must_use]
    pub fn kind_name(&self) -> &'static str {
        match self {
            Node::Task { .. } => "task",
            Node::Agent { .. } => "agent",
            Node::Sleep { .. } => "sleep",
            Node::SleepUntil { .. } => "sleep_until",
            Node::WaitEvent { .. } => "wait_event",
            Node::Parallel { .. } => "parallel",
            Node::Foreach { .. } => "foreach",
            Node::Loop { .. } => "loop",
            Node::Subworkflow { .. } => "subworkflow",
        }
    }

    /// The guard.
    #[must_use]
    pub fn when(&self) -> Option<&Predicate> {
        match self {
            Node::Task { when, .. }
            | Node::Agent { when, .. }
            | Node::Sleep { when, .. }
            | Node::SleepUntil { when, .. }
            | Node::WaitEvent { when, .. }
            | Node::Parallel { when, .. }
            | Node::Foreach { when, .. }
            | Node::Loop { when, .. }
            | Node::Subworkflow { when, .. } => when.as_ref(),
        }
    }

    /// The error policy.
    #[must_use]
    pub fn on_error(&self) -> OnError {
        match self {
            Node::Task { on_error, .. }
            | Node::Agent { on_error, .. }
            | Node::Sleep { on_error, .. }
            | Node::SleepUntil { on_error, .. }
            | Node::WaitEvent { on_error, .. }
            | Node::Parallel { on_error, .. }
            | Node::Foreach { on_error, .. }
            | Node::Loop { on_error, .. }
            | Node::Subworkflow { on_error, .. } => *on_error,
        }
    }

    /// The claimable view, when this node is a task or an agent.
    #[must_use]
    pub fn claimable(&self) -> Option<Claimable<'_>> {
        match self {
            Node::Task { id, when, on_error, input, effect, step_timeout_ms, retry } => {
                Some(Claimable {
                    id,
                    kind: StepKind::Task,
                    when: when.as_ref(),
                    on_error: *on_error,
                    input: input.as_ref(),
                    effect: *effect,
                    step_timeout_ms: *step_timeout_ms,
                    retry,
                })
            }
            Node::Agent { id, when, on_error, input, effect, step_timeout_ms, retry } => {
                Some(Claimable {
                    id,
                    kind: StepKind::Agent,
                    when: when.as_ref(),
                    on_error: *on_error,
                    input: input.as_ref(),
                    effect: *effect,
                    step_timeout_ms: *step_timeout_ms,
                    retry,
                })
            }
            Node::Sleep { .. }
            | Node::SleepUntil { .. }
            | Node::WaitEvent { .. }
            | Node::Parallel { .. }
            | Node::Foreach { .. }
            | Node::Loop { .. }
            | Node::Subworkflow { .. } => None,
        }
    }

    /// The nested nodes: branches, or the one body.
    #[must_use]
    pub fn children(&self) -> Vec<&Node> {
        match self {
            Node::Parallel { branches, .. } => branches.iter().collect(),
            Node::Foreach { body, .. } | Node::Loop { body, .. } => vec![body.as_ref()],
            Node::Task { .. }
            | Node::Agent { .. }
            | Node::Sleep { .. }
            | Node::SleepUntil { .. }
            | Node::WaitEvent { .. }
            | Node::Subworkflow { .. } => Vec::new(),
        }
    }
}

/// A parsed definition body. Section 3.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Graph {
    /// A JSON Schema a run's input must satisfy.
    #[serde(default)]
    pub input_schema: Option<Value>,
    /// The ceilings.
    #[serde(default)]
    pub limits: Limits,
    /// The steps, in order.
    pub steps: Vec<Node>,
    /// A template rendered when the run reaches the end of the graph.
    #[serde(default)]
    pub output: Option<Value>,
}

/// Why a position cannot be walked. The run that reaches it ends `failed`
/// with this message, which names the field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal(pub String);

impl Graph {
    /// Parse a definition body. Structure only; [`crate::validate::validate`]
    /// holds the rules a parse cannot express.
    ///
    /// # Errors
    /// Fails when the body is not a document this core can walk, and says
    /// which field was wrong.
    pub fn parse(body: &Value) -> Result<Self> {
        let mut body = body.clone();
        // `version` is a field of the document envelope, not of the graph;
        // a body that still carries it is a document posted whole.
        if let Value::Object(map) = &mut body {
            map.remove("version");
        }
        serde_json::from_value(body)
            .map_err(|e| Error::Invalid(format!("definition body is not a graph: {e}")))
    }

    /// The node at a position.
    #[must_use]
    pub fn node_at(&self, index: usize) -> Option<&Node> {
        self.steps.get(index)
    }

    /// The top-level node a step key belongs to, and the claimable node the
    /// key addresses.
    #[must_use]
    pub fn policy(&self, step_key: &str) -> Option<Claimable<'_>> {
        let parts = crate::context::parse_key(step_key);
        let node = self.steps.iter().find(|n| n.id() == parts.root)?;
        match node {
            Node::Task { .. } | Node::Agent { .. } => match (parts.index, parts.sub) {
                (None, None) => node.claimable(),
                (Some(_), _) | (None, Some(_)) => None,
            },
            Node::Parallel { branches, .. } => match (parts.index, parts.sub) {
                (None, Some(branch)) => branches.iter().find(|b| b.id() == branch)?.claimable(),
                (Some(_), _) | (None, None) => None,
            },
            Node::Foreach { body, .. } => match (parts.index, parts.sub) {
                (Some(_), None) => body.claimable(),
                (None, _) | (Some(_), Some(_)) => None,
            },
            Node::Loop { body, .. } => match (parts.index, parts.sub) {
                (Some(_), Some(sub)) if body.id() == sub => body.claimable(),
                (Some(_), _) | (None, _) => None,
            },
            Node::Sleep { .. }
            | Node::SleepUntil { .. }
            | Node::WaitEvent { .. }
            | Node::Subworkflow { .. } => None,
        }
    }

    /// The `on_error` of the top-level node a step key belongs to. A branch or
    /// a body answers with its own policy; this is the container's.
    #[must_use]
    pub fn on_error_of(&self, step_key: &str) -> Option<OnError> {
        let parts = crate::context::parse_key(step_key);
        self.steps.iter().find(|n| n.id() == parts.root).map(Node::on_error)
    }

    /// Every node the document declares, by its lexical id.
    ///
    /// Ids are unique across a document, which [`crate::validate`] enforces,
    /// so one flat map addresses every node whatever it is nested in.
    #[must_use]
    pub fn nodes(&self) -> BTreeMap<&str, Placed<'_>> {
        let mut out = BTreeMap::new();
        for node in &self.steps {
            out.insert(node.id(), Placed { node, container: node, site: Site::Top });
            // Exhaustive with no catch-all: a construct that grows a nested
            // body has to say what step key that body produces, or the cause
            // walk would silently stop being able to name it.
            match node {
                Node::Parallel { id, branches, .. } => {
                    for branch in branches {
                        out.insert(
                            branch.id(),
                            Placed { node: branch, container: node, site: Site::Branch(id) },
                        );
                    }
                }
                Node::Foreach { id, body, .. } => {
                    out.insert(
                        body.id(),
                        Placed {
                            node: body.as_ref(),
                            container: node,
                            site: Site::ForeachBody(id),
                        },
                    );
                }
                Node::Loop { id, body, .. } => {
                    out.insert(
                        body.id(),
                        Placed { node: body.as_ref(), container: node, site: Site::LoopBody(id) },
                    );
                }
                Node::Task { .. }
                | Node::Agent { .. }
                | Node::Sleep { .. }
                | Node::SleepUntil { .. }
                | Node::WaitEvent { .. }
                | Node::Subworkflow { .. } => {}
            }
        }
        out
    }

    /// The node a concrete step key addresses, or `None` when the document
    /// declares no such key.
    ///
    /// This is [`Graph::policy`] widened past the claimable nodes: a
    /// `subworkflow`, a `sleep` and a `wait_event` are steps of a run and are
    /// answered for here, because a reader asking why a step failed does not
    /// know in advance which kind it was.
    #[must_use]
    pub fn at_key(&self, step_key: &str) -> Option<Placed<'_>> {
        let parts = parse_key(step_key);
        let node = self.steps.iter().find(|n| n.id() == parts.root)?;
        match node {
            Node::Task { .. }
            | Node::Agent { .. }
            | Node::Sleep { .. }
            | Node::SleepUntil { .. }
            | Node::WaitEvent { .. }
            | Node::Subworkflow { .. } => match (parts.index, parts.sub) {
                (None, None) => Some(Placed { node, container: node, site: Site::Top }),
                (Some(_), _) | (None, Some(_)) => None,
            },
            Node::Parallel { id, branches, .. } => {
                match (parts.index, parts.sub) {
                    (None, Some(sub)) => branches.iter().find(|b| b.id() == sub).map(|branch| {
                        Placed { node: branch, container: node, site: Site::Branch(id) }
                    }),
                    (Some(_), _) | (None, None) => None,
                }
            }
            Node::Foreach { id, body, .. } => match (parts.index, parts.sub) {
                (Some(_), None) => Some(Placed {
                    node: body.as_ref(),
                    container: node,
                    site: Site::ForeachBody(id),
                }),
                (None, _) | (Some(_), Some(_)) => None,
            },
            Node::Loop { id, body, .. } => match (parts.index, parts.sub) {
                (Some(_), Some(sub)) if body.id() == sub => {
                    Some(Placed { node: body.as_ref(), container: node, site: Site::LoopBody(id) })
                }
                (Some(_), _) | (None, _) => None,
            },
        }
    }

    /// The iteration roots a step key implies: the `loop` iteration, and the
    /// `foreach` item, which is re-read from the same path the fan-out was
    /// resolved from.
    ///
    /// Every template of a step is rendered against this, so anything that
    /// wants to reproduce what a step actually read has to build the same
    /// scope. Both the attempt writer and the cause walk do.
    #[must_use]
    pub fn scope_for(&self, run: &Run, journal: &Journal, step_key: &str) -> Scope {
        let parts = parse_key(step_key);
        match (parts.index, parts.sub) {
            (Some(i), None) => {
                let over = self.steps.iter().find_map(|n| match n {
                    Node::Foreach { id, over, .. } if id == parts.root => Some(over.as_str()),
                    Node::Foreach { .. }
                    | Node::Task { .. }
                    | Node::Agent { .. }
                    | Node::Sleep { .. }
                    | Node::SleepUntil { .. }
                    | Node::WaitEvent { .. }
                    | Node::Parallel { .. }
                    | Node::Loop { .. }
                    | Node::Subworkflow { .. } => None,
                });
                let item = over
                    .map(|path| resolve(path, &context_for(run, journal, &Scope::top())))
                    .and_then(|v| v.get(i).cloned())
                    .unwrap_or(Value::Null);
                Scope::item(item, i)
            }
            (Some(i), Some(_)) => Scope::iteration(i as u32),
            (None, _) => Scope::top(),
        }
    }

    fn slot_for(
        body: &Node,
        step_key: String,
        run: &Run,
        journal: &Journal,
        scope: &Scope,
        now: OffsetDateTime,
    ) -> Result<Slot, Refusal> {
        let policy = body
            .claimable()
            .ok_or_else(|| Refusal(format!("step \"{step_key}\" is not a task or an agent")))?;
        let skipped = match policy.when {
            Some(guard) => !guard.eval(&context_for(run, journal, scope)),
            None => false,
        };
        Ok(Slot {
            step_key,
            kind: policy.kind,
            effect: policy.effect,
            step_deadline: policy.step_timeout_ms.map(|ms| after_ms(now, ms)),
            item_index: scope.item.as_ref().map(|(_, i)| *i),
            iteration: scope.iteration,
            skipped,
        })
    }

    /// Resolve the construct at `index`, with every value the materialising
    /// transaction needs already computed, against the journal as it stands.
    ///
    /// # Errors
    /// A [`Refusal`] names a field the document cannot be walked at, and the
    /// caller ends the run `failed` with it.
    pub fn resolve(
        &self,
        index: usize,
        iteration: u32,
        run: &Run,
        journal: &Journal,
        now: OffsetDateTime,
    ) -> Result<Cursor, Refusal> {
        let Some(node) = self.steps.get(index) else {
            return Ok(Cursor {
                index,
                at: Construct::End { status: RunStatus::Succeeded },
                skipped: false,
            });
        };
        let ctx = context_for(run, journal, &Scope::top());
        let skipped = node.when().is_some_and(|guard| !guard.eval(&ctx));

        let at = match node {
            Node::Task { id, .. } => Construct::Task {
                slot: Self::slot_for(node, id.clone(), run, journal, &Scope::top(), now)?,
            },
            Node::Agent { id, .. } => Construct::Agent {
                slot: Self::slot_for(node, id.clone(), run, journal, &Scope::top(), now)?,
            },
            Node::Sleep { id, ms, .. } => {
                Construct::Sleep { step: id.clone(), due_at: after_ms(now, *ms) }
            }
            Node::SleepUntil { id, at, .. } => {
                let text = render_string(at, &ctx);
                let due_at =
                    OffsetDateTime::parse(&text, &time::format_description::well_known::Rfc3339)
                        .map_err(|e| {
                            Refusal(format!(
                                "steps[{index}].at: \"{text}\" is not an RFC 3339 timestamp: {e}"
                            ))
                        })?;
                Construct::SleepUntil { step: id.clone(), due_at }
            }
            Node::WaitEvent { id, event, key, timeout_ms, accept_parked, on_timeout, .. } => {
                Construct::WaitEvent {
                    step: id.clone(),
                    event: event.clone(),
                    correlation_key: render_string(key, &ctx),
                    expires_at: after_ms(now, *timeout_ms),
                    accept_parked: *accept_parked,
                    on_timeout: *on_timeout,
                }
            }
            Node::Parallel { id, branches, .. } => {
                let mut slots = Vec::with_capacity(branches.len());
                for branch in branches {
                    slots.push(Self::slot_for(
                        branch,
                        format!("{id}/{}", branch.id()),
                        run,
                        journal,
                        &Scope::top(),
                        now,
                    )?);
                }
                Construct::Parallel { step: id.clone(), slots }
            }
            Node::Foreach { id, over, body, concurrency, max_items, .. } => {
                let items = match resolve(over, &ctx) {
                    Value::Array(items) => items,
                    other => {
                        return Err(Refusal(format!(
                            "steps[{index}].over: \"{over}\" resolved to {} rather than an array",
                            json_type(&other)
                        )))
                    }
                };
                if items.len() > *max_items {
                    return Err(Refusal(format!(
                        "steps[{index}].over: \"{over}\" holds {} items and max_items is {max_items}",
                        items.len()
                    )));
                }
                let mut slots = Vec::with_capacity(items.len());
                for (i, item) in items.into_iter().enumerate() {
                    slots.push(Self::slot_for(
                        body,
                        format!("{id}[{i}]"),
                        run,
                        journal,
                        &Scope::item(item, i),
                        now,
                    )?);
                }
                Construct::Foreach { step: id.clone(), slots, concurrency: (*concurrency).max(1) }
            }
            Node::Loop { id, body, max_iterations, .. } => {
                // The bound is a bound, so zero is zero bodies.
                if *max_iterations == 0 {
                    return self.resolve(index + 1, 0, run, journal, now);
                }
                Construct::Loop {
                    step: id.clone(),
                    slot: Self::slot_for(
                        body,
                        format!("{id}[{iteration}]/{}", body.id()),
                        run,
                        journal,
                        &Scope::iteration(iteration),
                        now,
                    )?,
                    iteration,
                    max_iterations: *max_iterations,
                }
            }
            Node::Subworkflow { id, definition, version, .. } => {
                let pinned = run
                    .bindings
                    .get(id)
                    .and_then(|b| b.get("version"))
                    .and_then(Value::as_u64)
                    .map(|v| v as u32)
                    .or(*version)
                    .ok_or_else(|| {
                        Refusal(format!(
                            "steps[{index}].version: no version of \"{definition}\" was pinned for step \"{id}\" at start"
                        ))
                    })?;
                Construct::Subworkflow {
                    step: id.clone(),
                    // Minted here rather than at the child's own start, so the
                    // parent's cursor and the child row it names commit
                    // together.
                    child_run_id: crate::new_id("run"),
                    definition_id: definition.clone(),
                    definition_version: pinned,
                    span_id: new_span_id(),
                }
            }
        };

        Ok(Cursor { index, at, skipped })
    }

    /// Where the run goes after `cursor`, against the journal as it stands.
    ///
    /// # Errors
    /// As [`Graph::resolve`].
    pub fn advance(
        &self,
        cursor: &Cursor,
        run: &Run,
        journal: &Journal,
        now: OffsetDateTime,
    ) -> Result<Cursor, Refusal> {
        // Exhaustive, no catch-all: a construct that moves the cursor in some
        // new way should not inherit "index plus one" by accident.
        match &cursor.at {
            Construct::Loop { iteration, max_iterations, .. } => {
                let done = match self.steps.get(cursor.index) {
                    Some(Node::Loop { until: Some(until), .. }) => {
                        until.eval(&context_for(run, journal, &Scope::iteration(*iteration)))
                    }
                    Some(Node::Loop { until: None, .. }) => false,
                    Some(
                        Node::Task { .. }
                        | Node::Agent { .. }
                        | Node::Sleep { .. }
                        | Node::SleepUntil { .. }
                        | Node::WaitEvent { .. }
                        | Node::Parallel { .. }
                        | Node::Foreach { .. }
                        | Node::Subworkflow { .. },
                    )
                    | None => false,
                };
                if !cursor.skipped && !done && iteration + 1 < *max_iterations {
                    self.resolve(cursor.index, iteration + 1, run, journal, now)
                } else {
                    self.resolve(cursor.index + 1, 0, run, journal, now)
                }
            }
            Construct::Task { .. }
            | Construct::Agent { .. }
            | Construct::Sleep { .. }
            | Construct::SleepUntil { .. }
            | Construct::WaitEvent { .. }
            | Construct::Parallel { .. }
            | Construct::Foreach { .. }
            | Construct::Subworkflow { .. } => self.resolve(cursor.index + 1, 0, run, journal, now),
            Construct::End { .. } => Ok(cursor.clone()),
        }
    }

    /// The first position.
    ///
    /// # Errors
    /// As [`Graph::resolve`].
    pub fn start(
        &self,
        run: &Run,
        journal: &Journal,
        now: OffsetDateTime,
    ) -> Result<Cursor, Refusal> {
        self.resolve(0, 0, run, journal, now)
    }
}

fn json_type(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}
