//! The section 3.3 context: the one place that knows the path roots.
//!
//! Every predicate and every template in this crate is evaluated against a
//! value built here. The roots are `input`, `steps`, `item`, `index`, `iter`
//! and `run`, and nothing else in the crate spells them, so a root added or
//! renamed is one edit.

use crate::model::{Attempt, AttemptStatus, Run, RunStatus, StepErrorClass};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

/// A child run as the parent's context sees it: the `subworkflow` step's
/// status, output and error come from the child's run row, because the step
/// has no attempt row of its own.
#[derive(Debug, Clone, PartialEq)]
pub struct ChildRun {
    /// The parent step the child hangs off.
    pub step_key: String,
    /// Where the child is.
    pub status: RunStatus,
    /// What it produced.
    pub output: Option<Value>,
    /// Why it ended, when it did not succeed.
    pub error: Option<String>,
    /// The class of that failure.
    pub error_class: Option<StepErrorClass>,
}

/// Everything a context is built from besides the run header.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Journal {
    /// Every attempt of the run.
    pub attempts: Vec<Attempt>,
    /// Every child run of the run.
    pub children: Vec<ChildRun>,
}

/// Which iteration roots are in scope.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Scope {
    /// Inside a `foreach` body: the element and its position.
    pub item: Option<(Value, usize)>,
    /// Inside a `loop` body: the iteration number.
    pub iteration: Option<u32>,
}

impl Scope {
    /// No iteration root in scope.
    #[must_use]
    pub fn top() -> Self {
        Self::default()
    }

    /// Inside a `foreach` body.
    #[must_use]
    pub fn item(value: Value, index: usize) -> Self {
        Self { item: Some((value, index)), iteration: None }
    }

    /// Inside a `loop` body.
    #[must_use]
    pub fn iteration(index: u32) -> Self {
        Self { item: None, iteration: Some(index) }
    }
}

/// The parts of a step key: `id`, `id/sub`, `id[n]` or `id[n]/sub`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyParts<'a> {
    /// The lexical id of the top-level node.
    pub root: &'a str,
    /// The item or iteration index, when the key has one.
    pub index: Option<usize>,
    /// The branch or body id, when the key has one.
    pub sub: Option<&'a str>,
}

/// Split a step key. Ids match `^[a-z][a-z0-9_]*$`, so `[`, `]` and `/` are
/// unambiguous separators.
#[must_use]
pub fn parse_key(key: &str) -> KeyParts<'_> {
    let (head, sub) = match key.split_once('/') {
        Some((head, sub)) => (head, Some(sub)),
        None => (key, None),
    };
    let (root, index) = match head.split_once('[') {
        Some((root, rest)) => (root, rest.strip_suffix(']').and_then(|n| n.parse().ok())),
        None => (head, None),
    };
    KeyParts { root, index, sub }
}

fn entry(
    status: AttemptStatus,
    output: Option<&Value>,
    error: Option<(&str, Option<StepErrorClass>)>,
) -> Value {
    let mut map = Map::new();
    map.insert("status".into(), Value::String(status.as_str().into()));
    match status {
        AttemptStatus::Succeeded => {
            if let Some(out) = output {
                map.insert("output".into(), out.clone());
            }
        }
        AttemptStatus::Failed | AttemptStatus::TimedOut => {
            if let Some((message, class)) = error {
                map.insert(
                    "error".into(),
                    json!({
                        "class": class.map(StepErrorClass::as_str),
                        "message": message,
                    }),
                );
            }
        }
        AttemptStatus::Skipped
        | AttemptStatus::Pending
        | AttemptStatus::Leased
        | AttemptStatus::Started
        | AttemptStatus::Orphaned => {}
    }
    Value::Object(map)
}

fn run_status_as_attempt(status: RunStatus) -> Option<AttemptStatus> {
    match status {
        RunStatus::Running => None,
        RunStatus::Succeeded => Some(AttemptStatus::Succeeded),
        RunStatus::Failed | RunStatus::Cancelled => Some(AttemptStatus::Failed),
        RunStatus::TimedOut => Some(AttemptStatus::TimedOut),
    }
}

/// The class a parent step reports for an ended child.
#[must_use]
pub fn child_error_class(child: &ChildRun) -> StepErrorClass {
    child.error_class.unwrap_or(match child.status {
        RunStatus::Cancelled => StepErrorClass::Cancelled,
        RunStatus::Running | RunStatus::Succeeded | RunStatus::Failed | RunStatus::TimedOut => {
            StepErrorClass::Fatal
        }
    })
}

/// The `steps` root: the latest ended attempt of every step, keyed by step
/// id, with `parallel` branches nested under their node, `foreach` and `loop`
/// outputs collected into `outputs`, and a `subworkflow` read from its child.
/// This is also the lease's `prior`.
#[must_use]
pub fn steps_root(journal: &Journal, scope: &Scope) -> Value {
    // The highest-numbered ended attempt per step key. A live attempt above
    // it is not in the map: the contract says prior carries ended attempts
    // only, and a guard reads what is settled.
    let mut latest: BTreeMap<&str, &Attempt> = BTreeMap::new();
    for a in &journal.attempts {
        if !a.status.is_terminal() {
            continue;
        }
        let keep = latest.get(a.step_key.as_str()).is_none_or(|have| a.attempt > have.attempt);
        if keep {
            latest.insert(&a.step_key, a);
        }
    }

    let mut steps = Map::new();
    // (root id, index, output) for foreach items and loop iterations, so the
    // outputs array can be laid out in item order with holes held.
    let mut collected: BTreeMap<&str, BTreeMap<usize, Value>> = BTreeMap::new();
    let mut aggregate: BTreeMap<&str, Vec<AttemptStatus>> = BTreeMap::new();

    for (key, a) in &latest {
        let parts = parse_key(key);
        let value =
            entry(a.status, a.output.as_ref(), a.error.as_deref().map(|m| (m, a.error_class)));
        match (parts.index, parts.sub) {
            (None, None) => {
                steps.insert(parts.root.to_string(), value);
            }
            (None, Some(branch)) => {
                let node = steps
                    .entry(parts.root.to_string())
                    .or_insert_with(|| Value::Object(Map::new()));
                if let Value::Object(map) = node {
                    map.insert(branch.to_string(), value);
                }
                aggregate.entry(parts.root).or_default().push(a.status);
            }
            (Some(index), sub) => {
                let output = match a.status {
                    AttemptStatus::Succeeded => a.output.clone().unwrap_or(Value::Null),
                    AttemptStatus::Failed
                    | AttemptStatus::TimedOut
                    | AttemptStatus::Skipped
                    | AttemptStatus::Pending
                    | AttemptStatus::Leased
                    | AttemptStatus::Started
                    | AttemptStatus::Orphaned => Value::Null,
                };
                collected.entry(parts.root).or_default().insert(index, output);
                aggregate.entry(parts.root).or_default().push(a.status);
                // Inside a loop body, `steps.<body id>` is this iteration's
                // own attempt.
                if let (Some(body), Some(current)) = (sub, scope.iteration) {
                    if index == current as usize {
                        steps.insert(body.to_string(), value);
                    }
                }
            }
        }
    }

    for (root, statuses) in aggregate {
        let status = if statuses
            .iter()
            .any(|s| matches!(s, AttemptStatus::Failed | AttemptStatus::TimedOut))
        {
            AttemptStatus::Failed
        } else {
            AttemptStatus::Succeeded
        };
        let node = steps.entry(root.to_string()).or_insert_with(|| Value::Object(Map::new()));
        if let Value::Object(map) = node {
            map.insert("status".into(), Value::String(status.as_str().into()));
            if let Some(items) = collected.get(root) {
                let len = items.keys().max().map_or(0, |m| m + 1);
                let mut outputs = vec![Value::Null; len];
                for (i, v) in items {
                    outputs[*i] = v.clone();
                }
                map.insert("outputs".into(), Value::Array(outputs));
            }
        }
    }

    for child in &journal.children {
        if let Some(status) = run_status_as_attempt(child.status) {
            let class = child_error_class(child);
            steps.insert(
                child.step_key.clone(),
                entry(
                    status,
                    child.output.as_ref(),
                    child.error.as_deref().map(|m| (m, Some(class))),
                ),
            );
        }
    }

    Value::Object(steps)
}

/// The whole section 3.3 context for one position.
#[must_use]
pub fn context_for(run: &Run, journal: &Journal, scope: &Scope) -> Value {
    let mut ctx = Map::new();
    ctx.insert("input".into(), run.input.clone().unwrap_or(Value::Null));
    ctx.insert("steps".into(), steps_root(journal, scope));
    if let Some((item, index)) = &scope.item {
        ctx.insert("item".into(), item.clone());
        ctx.insert("index".into(), json!(index));
    }
    if let Some(iteration) = scope.iteration {
        ctx.insert("iter".into(), json!({ "index": iteration }));
    }
    ctx.insert(
        "run".into(),
        json!({
            "run_id": run.run_id,
            "project": run.project,
            "definition_id": run.definition_id,
            "definition_version": run.definition_version,
        }),
    );
    Value::Object(ctx)
}
