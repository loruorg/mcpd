---
name: mcpd-investigate
description: Use when an agent run failed, stalled, looped, timed out, got slower or cost more than expected, and mcpd is recording its traces. Covers finding the run, reading its waterfall, naming the step that failed, reading the prompt and completion behind it, and deciding whether one run is broken or the whole deployment is. Needs the mcpd MCP server (`mcpd mcp`) connected, which serves find_runs, get_run and run_stats. Not for changing configuration or writing data: every tool here reads.
user-invocable: true
argument-hint: "[trace id | run name | what went wrong]"
license: Apache 2.0
---

You have three read-only tools over one mcpd daemon.

| Tool | Answers |
| --- | --- |
| `find_runs` | Which run are we talking about. |
| `get_run` | What happened inside it, step by step. |
| `run_stats` | Whether this is one run or all of them. |

Work in that order. Do not open the store, read a database file, or reach for
the HTTP API: the tools carry the same key scopes the daemon enforces, and going
around them reads data the key is not entitled to.

## Procedure

1. **Get a trace id.** If the user gave you one, use it.
   Otherwise `find_runs({ status: "error" })`. Narrow with `name`,
   `session_id`, `environment` or `q`. Nothing back? The header line names the
   window it searched; widen it with `since: "-7d"` or drop a filter.

2. **Read the run.** `get_run({ trace_id })`. Read it in this order:
   - the `N steps failed` block, which names the step and the reason;
   - the waterfall, for the shape of the run;
   - the failing step's detail: `error`, `kind`, `retryable`, and its input.

3. **Find the decision, not only the failure.** A failing tool call is usually
   the consequence. Read the model call above it: its prompt is what the agent
   was told, its completion is what it decided. Most agent bugs are visible
   there and nowhere else.

4. **Ask how wide it is.** `run_stats({ since: "-24h" })`, then again with
   `group_by: "model"` or `"name"`. A 40% failure rate means the run you read is
   a symptom. A single failure in 2,000 runs means it is not a regression.

5. **Report.** Name the step number, quote the error text exactly, say what the
   model decided, and state whether other runs fail the same way.

## Reading a waterfall

Each row is one step, indented under its parent, with a bar drawn to scale
across the whole run.

- **One long bar and nothing under it.** The run never got past its first step.
  Read the root step's input and error.
- **The same tool three or four times.** A loop. Read the completions: the model
  is calling the tool again instead of using its result.
- **A long bar, then a short failing one at the end.** A fast refusal, usually a
  rate limit, a 503 or a timeout. Check `retryable`.
- **A gap between bars.** Time nothing accounts for. Either a step is not
  instrumented, or the agent was waiting on something outside the trace.

## Rules

- Quote the step number and the exact error string. Do not paraphrase a stack.
- Never describe a payload you did not read. If you called `get_run` with
  `payloads: "none"`, say the prompts were not read.
- One failing run is not a regression until `run_stats` says so.
- A tool that reports a key does not reach a project is telling you the answer,
  not asking you to retry. Say which project needs to be reachable.
- Cost is context. `get_run` returns every payload by default; use
  `payloads: "errors"` when the failure is all you need, and `"none"` when only
  the shape matters. `max_payload_chars` caps each one.
- `find_runs` prints `next_cursor` when more match. Pass it as `cursor` rather
  than raising `limit` past what you will read.

## When the tools do not answer

- *Cannot reach the mcpd daemon*: the daemon is down, or `mcpd mcp` was started
  with the wrong `--endpoint`. Report it; do not fall back to another source.
- *The daemon refused the credential*: `mcpd mcp` needs a read key, passed with
  `--api-key` or `MCPD_API_KEY`.
- *That key is not allowed to read this*: the key is scoped to other projects.
  Name the project you need and stop.
