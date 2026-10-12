//! The rules a definition document has to satisfy that a parse cannot
//! express. Every error names the field.
//!
//! `spec/definition.schema.json` checks the same rules for a caller with no
//! Rust, minus the ones a JSON Schema cannot state: uniqueness of ids across
//! array items, a regular expression the engine accepts, and which template
//! roots exist at a position. Those are the ones this file exists for; the
//! rest are here too so a document is judged the same way with or without the
//! schema in front of it.

use crate::graph::{Graph, Node};
use crate::predicate::Predicate;
use crate::template;
use crate::{Error, Result, MAX_DURATION_MS};
use serde_json::Value;
use std::collections::HashSet;
use std::sync::OnceLock;

fn id_pattern() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new("^[a-z][a-z0-9_]*$").expect("a literal pattern"))
}

/// Whether an id is one the contract admits.
#[must_use]
pub fn is_valid_id(id: &str) -> bool {
    id_pattern().is_match(id)
}

fn invalid(message: String) -> Error {
    Error::Invalid(message)
}

/// Refuse a duration longer than [`MAX_DURATION_MS`], naming the field, the
/// bound and the value. A lower bound alone let a sleep of four hundred
/// trillion milliseconds through, and adding it to the clock stopped the
/// daemon.
fn check_upper(field: &str, ms: i64) -> Result<()> {
    if ms > MAX_DURATION_MS {
        return Err(invalid(format!(
            "{field}: {ms} is longer than the {MAX_DURATION_MS} ms (3650 days) a duration may \
             be; a deadline that far out cannot be stored, so name a shorter one"
        )));
    }
    Ok(())
}

/// Which iteration roots a path may name where it sits. Section 3.3 of the
/// contract: `item` and `index` exist inside a `foreach` body and `iter`
/// inside a `loop` body, and nowhere else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RootScope {
    Top,
    ForeachBody,
    LoopBody,
}

fn root_allowed(root: &str, scope: RootScope) -> bool {
    match root {
        "input" | "steps" | "run" => true,
        "item" | "index" => scope == RootScope::ForeachBody,
        "iter" => scope == RootScope::LoopBody,
        _ => false,
    }
}

/// Every leaf path a predicate reads. Exhaustive over the predicate shapes,
/// so a new combinator has to say what it reads.
///
/// Shared with [`crate::cause`], which needs the same list for the opposite
/// reason: the validator refuses a guard that reads a root that does not
/// exist, and the cause walk reports the step a guard read. Two copies of this
/// match would be two chances for a combinator to be forgotten in one of them.
pub(crate) fn predicate_paths<'a>(p: &'a Predicate, out: &mut Vec<&'a str>) {
    match p {
        Predicate::Leaf { path, .. } => out.push(path),
        Predicate::All { all } => all.iter().for_each(|q| predicate_paths(q, out)),
        Predicate::Any { any } => any.iter().for_each(|q| predicate_paths(q, out)),
        Predicate::Not { not } => predicate_paths(not, out),
    }
}

fn check_root(path: &str, at: &str, scope: RootScope) -> Result<()> {
    match template::root(path) {
        Some(root) if root_allowed(root, scope) => Ok(()),
        Some(root) => Err(invalid(format!(
            "{at}: \"{path}\" reads the root \"{root}\", which does not exist here; the roots \
             are input, steps and run, plus item and index inside a foreach body and iter \
             inside a loop body"
        ))),
        None => Err(invalid(format!("{at}: \"{path}\" is not a path"))),
    }
}

fn check_predicate(p: &Predicate, at: &str, scope: RootScope) -> Result<()> {
    p.validate().map_err(|e| invalid(format!("{at}: {e}")))?;
    let mut paths = Vec::new();
    predicate_paths(p, &mut paths);
    for path in paths {
        check_root(path, at, scope)?;
    }
    Ok(())
}

/// A template is a JSON value whose strings may carry `${path}` references.
fn check_template(value: &Value, at: &str, scope: RootScope) -> Result<()> {
    template::check(value).map_err(|e| invalid(format!("{at}: {e}")))?;
    for path in template::find_paths(value) {
        check_root(&path, at, scope)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn check_node(
    node: &Node,
    at: &str,
    ids: &mut HashSet<String>,
    keys: &mut HashSet<String>,
    key_prefix: &str,
    own_definition: &str,
    scope: RootScope,
) -> Result<()> {
    let id = node.id();
    if !is_valid_id(id) {
        return Err(invalid(format!("{at}.id: \"{id}\" does not match ^[a-z][a-z0-9_]*$")));
    }
    if !ids.insert(id.to_string()) {
        return Err(invalid(format!("{at}.id: \"{id}\" is used more than once in the document")));
    }
    let key = if key_prefix.is_empty() { id.to_string() } else { format!("{key_prefix}{id}") };
    if !keys.insert(key.clone()) {
        return Err(invalid(format!("{at}.id: \"{id}\" produces the step key \"{key}\" twice")));
    }
    if let Some(guard) = node.when() {
        check_predicate(guard, &format!("{at}.when"), scope)?;
    }

    match node {
        Node::Task { retry, step_timeout_ms, input, .. }
        | Node::Agent { retry, step_timeout_ms, input, .. } => {
            if let Some(input) = input {
                check_template(input, &format!("{at}.input"), scope)?;
            }
            if retry.max_attempts == 0 {
                return Err(invalid(format!("{at}.retry.max_attempts: must be at least 1")));
            }
            if retry.initial_ms < 0 {
                return Err(invalid(format!("{at}.retry.initial_ms: must not be negative")));
            }
            if retry.max_ms < retry.initial_ms {
                return Err(invalid(format!(
                    "{at}.retry.max_ms: {} is less than initial_ms {}",
                    retry.max_ms, retry.initial_ms
                )));
            }
            check_upper(&format!("{at}.retry.initial_ms"), retry.initial_ms)?;
            check_upper(&format!("{at}.retry.max_ms"), retry.max_ms)?;
            if step_timeout_ms.is_some_and(|ms| ms <= 0) {
                return Err(invalid(format!("{at}.step_timeout_ms: must be positive")));
            }
            if let Some(ms) = step_timeout_ms {
                check_upper(&format!("{at}.step_timeout_ms"), *ms)?;
            }
        }
        Node::Sleep { ms, .. } => {
            if *ms < 0 {
                return Err(invalid(format!("{at}.ms: must not be negative")));
            }
            check_upper(&format!("{at}.ms"), *ms)?;
        }
        Node::SleepUntil { at: template, .. } => {
            if template.is_empty() {
                return Err(invalid(format!("{at}.at: must not be empty")));
            }
            check_template(&Value::String(template.clone()), &format!("{at}.at"), scope)?;
        }
        Node::WaitEvent { event, key, timeout_ms, .. } => {
            if event.is_empty() {
                return Err(invalid(format!("{at}.event: must not be empty")));
            }
            if key.is_empty() {
                return Err(invalid(format!("{at}.key: must not be empty")));
            }
            check_template(&Value::String(key.clone()), &format!("{at}.key"), scope)?;
            if *timeout_ms <= 0 {
                return Err(invalid(format!("{at}.timeout_ms: must be positive")));
            }
            check_upper(&format!("{at}.timeout_ms"), *timeout_ms)?;
        }
        Node::Parallel { branches, .. } => {
            if branches.is_empty() {
                return Err(invalid(format!(
                    "{at}.branches: a parallel needs at least one branch"
                )));
            }
            for (i, branch) in branches.iter().enumerate() {
                let branch_at = format!("{at}.branches[{i}]");
                if branch.claimable().is_none() {
                    return Err(invalid(format!(
                        "{branch_at}.kind: a parallel branch must be a task or an agent, got {}",
                        branch.kind_name()
                    )));
                }
                check_node(
                    branch,
                    &branch_at,
                    ids,
                    keys,
                    &format!("{id}/"),
                    own_definition,
                    scope,
                )?;
            }
        }
        Node::Foreach { body, over, concurrency, max_items, .. } => {
            if over.is_empty() {
                return Err(invalid(format!("{at}.over: must not be empty")));
            }
            if !template::is_valid_path(over) {
                return Err(invalid(format!(
                    "{at}.over: \"{over}\" is not a path; write input.items, not a template"
                )));
            }
            check_root(over, &format!("{at}.over"), scope)?;
            if *concurrency == 0 {
                return Err(invalid(format!("{at}.concurrency: must be at least 1")));
            }
            if *max_items == 0 {
                return Err(invalid(format!("{at}.max_items: must be at least 1")));
            }
            if body.claimable().is_none() {
                return Err(invalid(format!(
                    "{at}.body.kind: a foreach body must be a task or an agent, got {}",
                    body.kind_name()
                )));
            }
            // The item keys are `id[n]`; the body's own id is what the
            // uniqueness rule sees, and `[` cannot appear in an id.
            check_node(
                body,
                &format!("{at}.body"),
                ids,
                keys,
                &format!("{id}[*]/"),
                own_definition,
                RootScope::ForeachBody,
            )?;
        }
        Node::Loop { body, max_iterations, until, .. } => {
            if *max_iterations == 0 {
                return Err(invalid(format!("{at}.max_iterations: must be at least 1")));
            }
            if let Some(until) = until {
                check_predicate(until, &format!("{at}.until"), RootScope::LoopBody)?;
            }
            if body.claimable().is_none() {
                return Err(invalid(format!(
                    "{at}.body.kind: a loop body must be a task or an agent, got {}",
                    body.kind_name()
                )));
            }
            check_node(
                body,
                &format!("{at}.body"),
                ids,
                keys,
                &format!("{id}[*]/"),
                own_definition,
                RootScope::LoopBody,
            )?;
        }
        Node::Subworkflow { definition, version, input, .. } => {
            if definition == own_definition {
                return Err(invalid(format!(
                    "{at}.definition: \"{definition}\" names the definition it is part of, which \
                     is a run tree no ceiling bounds"
                )));
            }
            if !is_valid_id(definition) {
                return Err(invalid(format!(
                    "{at}.definition: \"{definition}\" does not match ^[a-z][a-z0-9_]*$"
                )));
            }
            if version.is_some_and(|v| v == 0) {
                return Err(invalid(format!("{at}.version: must be at least 1")));
            }
            if let Some(input) = input {
                check_template(input, &format!("{at}.input"), scope)?;
            }
        }
    }
    Ok(())
}

/// Check a parsed document against the rules of section 3.
///
/// # Errors
/// [`Error::Invalid`] naming the field.
pub fn validate(graph: &Graph, definition_id: &str, materialise_budget: usize) -> Result<()> {
    if !is_valid_id(definition_id) {
        return Err(invalid(format!("id: \"{definition_id}\" does not match ^[a-z][a-z0-9_]*$")));
    }
    if let Some(schema) = &graph.input_schema {
        jsonschema::validator_for(schema)
            .map_err(|e| invalid(format!("input_schema: not a valid JSON Schema: {e}")))?;
    }
    if graph.limits.max_steps.is_some_and(|n| n < 0) {
        return Err(invalid("limits.max_steps: must not be negative".into()));
    }
    if graph.limits.run_timeout_ms.is_some_and(|n| n <= 0) {
        return Err(invalid("limits.run_timeout_ms: must be positive".into()));
    }
    if let Some(ms) = graph.limits.run_timeout_ms {
        check_upper("limits.run_timeout_ms", ms)?;
    }
    if graph.limits.max_cost_usd.is_some_and(|n| n < 0.0 || !n.is_finite()) {
        return Err(invalid("limits.max_cost_usd: must be a non-negative number".into()));
    }
    if let Some(output) = &graph.output {
        check_template(output, "output", RootScope::Top)?;
    }

    let mut ids = HashSet::new();
    let mut keys = HashSet::new();
    // A run of positions that settle on arrival is walked inside one
    // transaction, so a document longer than the budget in that shape can
    // never start. With the empty fan-out and the zero-bound loop refused
    // above, nothing static settles on arrival any more, but the check stays
    // so a construct added later has to answer it.
    let mut consecutive = 0usize;
    for (i, node) in graph.steps.iter().enumerate() {
        check_node(
            node,
            &format!("steps[{i}]"),
            &mut ids,
            &mut keys,
            "",
            definition_id,
            RootScope::Top,
        )?;
        if statically_settles(node) {
            consecutive += 1;
            if consecutive > materialise_budget {
                return Err(invalid(format!(
                    "steps[{i}]: more than {materialise_budget} consecutive positions settle on \
                     arrival, which is more than one transaction may walk"
                )));
            }
        } else {
            consecutive = 0;
        }
    }
    Ok(())
}

/// Whether a position is over the moment it is reached, as far as the
/// document alone can tell. Guards and item counts are decided per run.
fn statically_settles(node: &Node) -> bool {
    match node {
        Node::Parallel { branches, .. } => branches.is_empty(),
        Node::Loop { max_iterations, .. } => *max_iterations == 0,
        Node::Task { .. }
        | Node::Agent { .. }
        | Node::Sleep { .. }
        | Node::SleepUntil { .. }
        | Node::WaitEvent { .. }
        | Node::Foreach { .. }
        | Node::Subworkflow { .. } => false,
    }
}
