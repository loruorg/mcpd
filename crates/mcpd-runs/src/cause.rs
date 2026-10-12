//! Why one step of a run failed: the ancestors whose output it consumed.
//!
//! # The defect this module is for
//!
//! When a step fails, the step that caused it has usually **succeeded**. A
//! classifier returns `{"kinds": []}` rather than an error, the step after it
//! passes the empty list along without noticing, and the step after that fails
//! because there is nothing to charge. Every status on that path is
//! `succeeded` except the last one, so a search over statuses finds the step
//! that broke and never the step that did it. That is the case this module
//! answers, and [`CauseLink::silent_producer`] is the field that says so.
//!
//! # Why the answer can be trusted
//!
//! A definition's templates are declared edges. `${steps.classify.output.kind}`
//! in one node's input names another node's output, in the document version the
//! run pinned at start, and the daemon has to resolve exactly those references
//! to render the step's input at all. So the chain is **read** out of the
//! document rather than guessed from timing or from what happened to run first,
//! and the same walk works with span payload capture off: nothing here reads a
//! prompt, a completion or a model.
//!
//! Every path this module follows is a path [`crate::template`] parses and
//! [`crate::context`] would have resolved, so an edge reported here is an edge
//! the renderer actually followed. Where the two could disagree, this module
//! follows the context: a `parallel` branch is addressable only as
//! `steps.<parent>.<branch>` because that is where [`crate::context::steps_root`]
//! puts it, and a `loop` body is addressable by its own id only from inside its
//! own loop for the same reason.
//!
//! # What it cannot say
//!
//! [`crate::validate`] checks that a template names a root that exists where it
//! sits. It does not check that `${steps.classify...}` names a step the
//! document declares, so an unknown id passes validation and resolves to null
//! at run time. A path naming nothing the document declares yields no link
//! here, rather than a link to a step that does not exist.
//!
//! A `subworkflow` is one hop. Its status and output come from the child run
//! row, which is what the parent's context reads, and the walk stops at the
//! boundary: why a step of the child failed is the child's own answer, asked
//! for by the child's run id.

use crate::context::{child_error_class, context_for, parse_key, ChildRun, Journal};
use crate::graph::{Graph, Node, Placed, Site};
use crate::model::{Attempt, AttemptStatus, Run, RunStatus, StepErrorClass};
use crate::store::StepError;
use crate::template::{self, PathStep};
use crate::validate::predicate_paths;
use crate::{Error, Result};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

/// How many links one answer may carry.
///
/// `max_depth` bounds how far back the walk goes, and this bounds how wide it
/// gets on the way. A `foreach` of five hundred items read by name, or one
/// ancestor referenced from thirty places, would otherwise produce an answer
/// nobody can read out of a walk nobody asked to be that long. Reaching it
/// sets [`StepCause::truncated`], which already means "this chain is a prefix
/// of the answer".
const MAX_LINKS: usize = 512;

/// The JSON type of a value.
///
/// The cause walk reports the shape of what an ancestor handed over without
/// returning the value itself. The value is caller-owned content and may be
/// large; the shape is what tells a reader that a green step handed the step
/// below it an empty list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueKind {
    /// The value was JSON null.
    Null,
    /// The value was `true` or `false`.
    Boolean,
    /// The value was a JSON number.
    Number,
    /// The value was a JSON string.
    String,
    /// The value was a JSON array.
    Array,
    /// The value was a JSON object.
    Object,
}

impl ValueKind {
    /// The wire spelling.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Boolean => "boolean",
            Self::Number => "number",
            Self::String => "string",
            Self::Array => "array",
            Self::Object => "object",
        }
    }

    /// The kind of a value.
    #[must_use]
    pub const fn of(value: &Value) -> Self {
        match value {
            Value::Null => Self::Null,
            Value::Bool(_) => Self::Boolean,
            Value::Number(_) => Self::Number,
            Value::String(_) => Self::String,
            Value::Array(_) => Self::Array,
            Value::Object(_) => Self::Object,
        }
    }
}

/// Whether a value is the empty one of its kind.
///
/// Null, the empty string, the empty array and the empty object all count. A
/// green step that handed the step below it one of these is the failure no
/// status code catches, and it is the whole reason the shape is reported.
#[must_use]
pub fn is_empty_value(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(s) => s.is_empty(),
        Value::Array(items) => items.is_empty(),
        Value::Object(map) => map.is_empty(),
        Value::Bool(_) | Value::Number(_) => false,
    }
}

/// One edge of a cause chain: an ancestor step, and the template path through
/// which the step below it read that ancestor's output.
///
/// An ancestor read through two paths appears twice, once per path, because
/// the two paths are two different claims about what went wrong.
#[derive(Debug, Clone, PartialEq)]
pub struct CauseLink {
    /// The ancestor step, keyed the way the journal keys it.
    pub step_key: String,
    /// Where the ancestor's latest attempt stands. A live attempt reports the
    /// state it is live in, so a chain read while a run is moving does not
    /// have to lie about it.
    pub status: AttemptStatus,
    /// How many attempts were made at the ancestor.
    pub attempts: u32,
    /// The path as the definition writes it inside `${...}`.
    pub template_path: String,
    /// The step key that read this ancestor through `template_path`.
    pub consumed_by: String,
    /// How far this ancestor sits from the step the answer is about. `1` is
    /// read directly.
    pub depth: u32,
    /// True when this ancestor ended `succeeded` and every step between it and
    /// the failed step ended `succeeded` too.
    ///
    /// Not the same as `status` being `succeeded`. An ancestor sitting behind a
    /// step that did fail is not a silent producer, because that failure
    /// already explains the run and pointing at the ancestor would send a
    /// reader past the answer.
    pub silent_producer: bool,
    /// The JSON type `template_path` resolved to on the ancestor's output.
    /// Absent when there is no output to read: a step that never ran, one that
    /// failed, one still running.
    pub value_kind: Option<ValueKind>,
    /// Whether that value was empty. Absent for the same reason `value_kind`
    /// is.
    pub value_empty: Option<bool>,
}

/// Why one step of a run failed.
#[derive(Debug, Clone, PartialEq)]
pub struct StepCause {
    /// The run this answer is about.
    pub run_id: String,
    /// The step this answer is about, as it was asked for.
    pub step_key: String,
    /// Where that step's latest attempt stands.
    pub status: AttemptStatus,
    /// How many attempts were made at it.
    pub attempts: u32,
    /// Why the latest attempt failed, when it did.
    pub error: Option<StepError>,
    /// The ancestors whose output this step consumed, by `depth` ascending and
    /// then by the order the paths appear in the definition.
    pub chain: Vec<CauseLink>,
    /// True when the walk stopped with ancestors left unvisited, so `chain` is
    /// a prefix of the answer rather than the whole of it.
    pub truncated: bool,
}

/// What the journal says about one step key.
struct Evidence<'a> {
    status: AttemptStatus,
    attempts: u32,
    /// The attempt the status came from. Absent for a `subworkflow`, whose
    /// status comes from the child run row and which has no attempt of its
    /// own.
    latest: Option<&'a Attempt>,
    /// The child run a `subworkflow` step is answered by.
    child: Option<&'a ChildRun>,
}

/// Everything the journal says about every step key of a run, and every
/// concrete key each top-level node produced, in one pass.
///
/// Built once and looked up, rather than scanned per edge. A wide chain over a
/// long journal would otherwise walk every attempt row once per link, and a
/// `foreach` of hundreds of items with retries is a long journal.
struct Index<'a> {
    /// What the journal says about one step key.
    ///
    /// A step with no attempt row and no child run has produced nothing and
    /// consumed nothing, so it is absent here rather than carrying an invented
    /// status. That covers a step the run never reached, and a `sleep` or a
    /// `wait_event`, which are constructs of the cursor and write no attempt.
    evidence: HashMap<&'a str, Evidence<'a>>,
    /// Every concrete step key under one top-level node id, in a stable order.
    /// This is what `steps.<fanout>.status` and `steps.<fanout>.outputs` name:
    /// the node as a whole, which is every item or branch of it.
    under: HashMap<&'a str, Vec<&'a str>>,
}

impl<'a> Index<'a> {
    fn build(journal: &'a Journal) -> Self {
        let mut evidence: HashMap<&str, Evidence<'_>> = HashMap::new();
        for a in &journal.attempts {
            let key = a.step_key.as_str();
            match evidence.get_mut(key) {
                Some(have) => {
                    have.attempts += 1;
                    if have.latest.is_none_or(|l| a.attempt > l.attempt) {
                        have.latest = Some(a);
                        have.status = a.status;
                    }
                }
                None => {
                    evidence.insert(
                        key,
                        Evidence { status: a.status, attempts: 1, latest: Some(a), child: None },
                    );
                }
            }
        }
        for child in &journal.children {
            // A `subworkflow` step has no attempt row of its own, so its
            // status comes from the child run row, which is what the parent's
            // own context reads for it. A child still running has produced
            // nothing, and the parent's context does not carry it either.
            let status = match child.status {
                RunStatus::Running => continue,
                RunStatus::Succeeded => AttemptStatus::Succeeded,
                RunStatus::Failed | RunStatus::Cancelled => AttemptStatus::Failed,
                RunStatus::TimedOut => AttemptStatus::TimedOut,
            };
            evidence.entry(child.step_key.as_str()).or_insert(Evidence {
                status,
                attempts: 1,
                latest: None,
                child: Some(child),
            });
        }

        let mut under: HashMap<&str, Vec<&str>> = HashMap::new();
        for key in evidence.keys() {
            under.entry(parse_key(key).root).or_default().push(key);
        }
        for keys in under.values_mut() {
            // By item or iteration index first, so an answer does not depend
            // on the order rows came back in, and `handle[2]` never sorts
            // before `handle[10]` the way a plain string compare would.
            keys.sort_by(|a, b| {
                let (x, y) = (parse_key(a), parse_key(b));
                x.index.cmp(&y.index).then_with(|| x.sub.cmp(&y.sub)).then_with(|| a.cmp(b))
            });
        }
        Self { evidence, under }
    }

    fn get(&self, key: &str) -> Option<&Evidence<'a>> {
        self.evidence.get(key)
    }

    fn keys_under(&self, root: &str) -> Vec<String> {
        self.under
            .get(root)
            .map(|keys| keys.iter().map(|k| (*k).to_owned()).collect())
            .unwrap_or_default()
    }
}

/// The error the answer reports for one step.
fn error_of(evidence: &Evidence<'_>) -> Option<StepError> {
    match (evidence.latest, evidence.child) {
        (Some(a), _) => a.error.as_ref().map(|message| StepError {
            class: a.error_class.unwrap_or(StepErrorClass::Fatal),
            message: message.clone(),
            details: a.error_details.clone(),
            // The floor a worker asked for bounds one retry; it is not stored
            // on the attempt and is not part of why the step failed.
            retry_after_ms: None,
        }),
        (None, Some(child)) => child.error.as_ref().map(|message| StepError {
            class: child_error_class(child),
            message: message.clone(),
            details: None,
            retry_after_ms: None,
        }),
        (None, None) => None,
    }
}

/// Add a path once, keeping the order it was first seen in.
fn push_unique(out: &mut Vec<String>, path: String) {
    if !out.contains(&path) {
        out.push(path);
    }
}

/// Every `${path}` one node's own guard and templates read, in document order.
///
/// The match is exhaustive with no catch-all. A construct that grows a
/// template has to be listed here, or the edges it declares would quietly stop
/// being reported and the chain would be short without saying so.
fn node_paths(node: &Node) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(guard) = node.when() {
        let mut paths = Vec::new();
        predicate_paths(guard, &mut paths);
        for path in paths {
            push_unique(&mut out, path.to_string());
        }
    }
    let template_paths = |value: &Value, out: &mut Vec<String>| {
        for path in template::find_paths(value) {
            push_unique(out, path);
        }
    };
    match node {
        Node::Task { input, .. } | Node::Agent { input, .. } | Node::Subworkflow { input, .. } => {
            if let Some(input) = input {
                template_paths(input, &mut out);
            }
        }
        // `at` and `key` are template strings, not paths.
        Node::SleepUntil { at, .. } => {
            template_paths(&Value::String(at.clone()), &mut out);
        }
        Node::WaitEvent { key, .. } => {
            template_paths(&Value::String(key.clone()), &mut out);
        }
        // `over` is a bare path: the walker resolves it rather than rendering
        // it, so it is not run through the template scanner.
        Node::Foreach { over, .. } => push_unique(&mut out, over.clone()),
        Node::Loop { until: Some(until), .. } => {
            let mut paths = Vec::new();
            predicate_paths(until, &mut paths);
            for path in paths {
                push_unique(&mut out, path.to_string());
            }
        }
        Node::Loop { until: None, .. } | Node::Sleep { .. } | Node::Parallel { .. } => {}
    }
    out
}

/// Every path that decided one concrete step, in document order.
///
/// A nested step is reached only through its container, so the container's
/// guard and fan-out decided it as much as its own input did: a `foreach` item
/// exists at all because the array in `over` had an element at that index, and
/// that array came from somewhere. The container is written earlier in the
/// document, so its paths come first.
fn paths_for(placed: Placed<'_>) -> Vec<String> {
    let mut out = Vec::new();
    // Ids are unique across a document, so this is "the node is nested" and
    // not a pointer-identity trick.
    if placed.container.id() != placed.node.id() {
        for path in node_paths(placed.container) {
            push_unique(&mut out, path);
        }
    }
    for path in node_paths(placed.node) {
        push_unique(&mut out, path);
    }
    out
}

/// The concrete ancestor step keys one `${steps...}` path names.
///
/// Empty when the path reads something other than `steps`, when it names an id
/// the document does not declare, or when it names a nested node from a place
/// the context does not offer it. Every one of those resolves to null at run
/// time, and reporting an edge that the renderer would not have followed would
/// make the chain a guess.
fn producers(
    nodes: &BTreeMap<&str, Placed<'_>>,
    index: &Index<'_>,
    reader_key: &str,
    path: &str,
) -> Vec<String> {
    let Some(steps) = template::path_steps(path) else {
        return Vec::new();
    };
    match steps.first() {
        Some(PathStep::Key(root)) if root == "steps" => {}
        _ => return Vec::new(),
    }
    let Some(PathStep::Key(name)) = steps.get(1) else {
        return Vec::new();
    };
    let Some(placed) = nodes.get(name.as_str()) else {
        return Vec::new();
    };

    match placed.site {
        Site::Top => match placed.node {
            Node::Parallel { branches, .. } => match steps.get(2) {
                // `steps.<parallel>.<branch>` names one branch. Anything else
                // (`steps.<parallel>.status`) names the node as a whole.
                Some(PathStep::Key(branch)) if branches.iter().any(|node| node.id() == branch) => {
                    vec![format!("{name}/{branch}")]
                }
                _ => index.keys_under(name),
            },
            // `steps.<fanout>.outputs[i]` names one item or iteration.
            Node::Foreach { .. } => match (steps.get(2), steps.get(3)) {
                (Some(PathStep::Key(field)), Some(PathStep::Index(i))) if field == "outputs" => {
                    vec![format!("{name}[{i}]")]
                }
                _ => index.keys_under(name),
            },
            Node::Loop { body, .. } => match (steps.get(2), steps.get(3)) {
                (Some(PathStep::Key(field)), Some(PathStep::Index(i))) if field == "outputs" => {
                    vec![format!("{name}[{i}]/{}", body.id())]
                }
                _ => index.keys_under(name),
            },
            // A top-level step's key is its id. A `sleep` or a `wait_event`
            // writes no attempt, so the key yields no evidence and no link;
            // that is decided by the journal rather than here.
            Node::Task { .. }
            | Node::Agent { .. }
            | Node::Sleep { .. }
            | Node::SleepUntil { .. }
            | Node::WaitEvent { .. }
            | Node::Subworkflow { .. } => vec![name.clone()],
        },
        // A loop body is addressable by its own id, and only from inside its
        // own loop: that is the one place `steps_root` puts it, and it puts
        // this iteration's attempt there.
        Site::LoopBody(parent) => {
            let parts = parse_key(reader_key);
            match (parts.root == parent, parts.index) {
                (true, Some(i)) => vec![format!("{parent}[{i}]/{name}")],
                _ => Vec::new(),
            }
        }
        // A parallel branch and a foreach body are not addressable by their
        // own id at all: the context nests them under their container, so
        // `steps.<branch>` reads nothing.
        Site::Branch(_) | Site::ForeachBody(_) => Vec::new(),
    }
}

/// Walk backwards from `step_key` along the data dependencies `graph`
/// declares, and answer what produced what it read.
///
/// # Errors
/// [`Error::NotFound`] when the definition declares no such step key, and when
/// it declares one this run never reached, because there is then nothing in
/// the journal to answer about.
pub fn explain(
    run: &Run,
    graph: &Graph,
    journal: &Journal,
    step_key: &str,
    max_depth: u32,
) -> Result<StepCause> {
    if graph.at_key(step_key).is_none() {
        return Err(Error::NotFound { kind: "step", id: step_key.to_owned() });
    }
    let index = Index::build(journal);
    let Some(root) = index.get(step_key) else {
        return Err(Error::NotFound { kind: "attempt of step", id: step_key.to_owned() });
    };

    let nodes = graph.nodes();
    let mut chain: Vec<CauseLink> = Vec::new();
    let mut truncated = false;
    // Expanded once each, at the shallowest depth that reaches them, which is
    // what bounds the walk over a definition whose steps read each other.
    let mut expanded: HashSet<String> = HashSet::new();
    // Emitted once per (reader, path, ancestor): the same ancestor read
    // through two paths is two links, and the same path twice is one.
    let mut emitted: HashSet<(String, String, String)> = HashSet::new();
    // Whether everything between a step and the failed step ended
    // `succeeded`. The failed step itself is the endpoint, so nothing is
    // between it and itself.
    let mut silent: HashMap<String, bool> = HashMap::new();
    // One context per reader, because building one walks the whole journal and
    // a wide chain would otherwise build the same one many times.
    let mut contexts: HashMap<String, Value> = HashMap::new();

    silent.insert(step_key.to_owned(), true);
    expanded.insert(step_key.to_owned());
    let mut queue: VecDeque<(String, u32)> = VecDeque::new();
    queue.push_back((step_key.to_owned(), 0));

    while let Some((reader, depth)) = queue.pop_front() {
        let Some(placed) = graph.at_key(&reader) else {
            continue;
        };
        let paths = paths_for(placed);
        if depth >= max_depth {
            // Only a reader that would have produced a link makes the answer a
            // prefix. A leaf that reads nothing but `input` does not.
            if paths.iter().any(|path| {
                producers(&nodes, &index, &reader, path)
                    .iter()
                    .any(|key| *key != reader && key != step_key && index.get(key).is_some())
            }) {
                truncated = true;
            }
            continue;
        }
        let reader_silent = silent.get(&reader).copied().unwrap_or(false);

        for path in paths {
            for key in producers(&nodes, &index, &reader, &path) {
                // A step is not its own ancestor, and neither is the step the
                // whole answer is about. A loop body naming itself reads this
                // iteration's own attempt, and a document whose steps name each
                // other in a circle would otherwise report the failed step as
                // its own cause, which is noise a reader has to discard.
                if key == reader || key == step_key {
                    continue;
                }
                let Some(found) = index.get(&key) else {
                    continue;
                };
                if !emitted.insert((reader.clone(), path.clone(), key.clone())) {
                    continue;
                }
                if chain.len() >= MAX_LINKS {
                    truncated = true;
                    return Ok(StepCause {
                        run_id: run.run_id.clone(),
                        step_key: step_key.to_owned(),
                        status: root.status,
                        attempts: root.attempts,
                        error: error_of(root),
                        chain,
                        truncated,
                    });
                }

                let succeeded = found.status == AttemptStatus::Succeeded;
                let silent_producer = succeeded && reader_silent;
                // The shape is read from the context the reading step's own
                // template was rendered against, so what is reported is what
                // the renderer saw and not a second opinion about it.
                let (value_kind, value_empty) = if succeeded {
                    let ctx = contexts.entry(reader.clone()).or_insert_with(|| {
                        let scope = graph.scope_for(run, journal, &reader);
                        context_for(run, journal, &scope)
                    });
                    let value = template::resolve(&path, ctx);
                    (Some(ValueKind::of(&value)), Some(is_empty_value(&value)))
                } else {
                    (None, None)
                };

                chain.push(CauseLink {
                    step_key: key.clone(),
                    status: found.status,
                    attempts: found.attempts,
                    template_path: path.clone(),
                    consumed_by: reader.clone(),
                    depth: depth + 1,
                    silent_producer,
                    value_kind,
                    value_empty,
                });

                if expanded.insert(key.clone()) {
                    silent.insert(key.clone(), silent_producer);
                    queue.push_back((key, depth + 1));
                }
            }
        }
    }

    Ok(StepCause {
        run_id: run.run_id.clone(),
        step_key: step_key.to_owned(),
        status: root.status,
        attempts: root.attempts,
        error: error_of(root),
        chain,
        truncated,
    })
}
