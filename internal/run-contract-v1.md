# The run contract, version 1

Status: binding for the first served release of durable runs. Written
2026-09-01 from `internal/framework-design.md` (five review rounds) and from
what `crates/mcpd-runs` actually does. Where the design and this file differ,
this file wins, because it is the one the code, the spec and six SDKs are built
against in one pass. Where this file and `spec/mcpd.spec.json` differ after that
pass, the spec wins, because the spec is generated into every language.

What is deliberately out of version 1, with the reason, is in section 10. A
doc page that describes a mechanism from that section as present is a defect.

---

## 1. The idea, in three sentences

The daemon owns the run between steps: the versioned definition, the journal of
step attempts, every deadline, every wait, the cancel, and the trace. The user's
own process owns everything inside a step: the model call, the prompt, the tool
body, the provider key, the framework, the language. The seam is seven HTTP
calls a worker makes, and a step body is any code in any of the six SDK
languages, or anything with an HTTP client.

The daemon never executes user code and never holds a provider key.

## 2. Enums

Wire spelling is exact. Every value matches `^[a-z][a-z0-9_]*$` so the
generators can turn it into an identifier in six languages.

| Enum | Values | Notes |
| --- | --- | --- |
| `RunStatus` | `running`, `succeeded`, `failed`, `timed_out`, `cancelled` | Only `running` can move. The other four are terminal and hold no deadline, no wait and no live attempt. |
| `AttemptStatus` | `pending`, `leased`, `started`, `orphaned`, `succeeded`, `failed`, `timed_out`, `skipped` | `skipped` is written by a false `when` guard. |
| `StepKind` | `task`, `agent`, `wait` | `task` and `agent` are claimable. A `wait` row is written by the event that resumes it or the deadline that ends it. |
| `ConstructKind` | `task`, `agent`, `sleep`, `sleep_until`, `wait_event`, `parallel`, `foreach`, `loop`, `subworkflow`, `end` | What a cursor names. |
| `Effect` | `at_least_once`, `at_most_once` | Default `at_least_once`. |
| `TimerKind` | `sleep`, `lease_expiry`, `reclaim`, `run_timeout`, `step_timeout`, `wait_timeout`, `retry` | `retry` is new in v1: the backoff before a requeued attempt becomes claimable. |
| `RunEventKind` | `run_started`, `run_cursor`, `step_leased`, `step_started`, `step_finished`, `step_failed`, `step_skipped`, `run_waiting`, `run_resumed`, `run_ended` | Section 8. |
| `DefinitionKind` | `workflow` | `agent` definitions arrive with the in-process agent loop, which is not v1. |
| `EventOutcome` | `delivered`, `parked`, `duplicate` | What `POST /v1/api/events` did. |
| `ConflictReason` | `fenced`, `already_applied`, `not_started`, `run_terminal` | Section 7.4. |
| `StepErrorClass` | `retryable`, `fatal`, `invalid_input`, `unservable`, `cancelled` | Section 7.6. |
| `OnError` | `fail_run`, `continue` | Default `fail_run`. `compensate` is not v1. |
| `Backoff` | `fixed`, `exponential` | Retry spacing. |
| `WaitTimeout` | `fail`, `continue` | What a `wait_event` does when its deadline passes. Default `fail`. |

Scopes gain two values: `execute` (start, cancel, signal, publish an event) and
`worker` (register, claim, started, heartbeat, complete, fail). Both are closed
by default: a scope with no key configured refuses every call, unlike the four
existing scopes, because a daemon that lets any caller start work or claim it
is a daemon anybody can spend. Flags: `--execute-key`, `--worker-key`, and the
matching per-project entries. `admin` implies every scope.

## 3. The definition document

Posted as `body` (type `json`) on `DefinitionDocument`, validated at write
time against `spec/definition.schema.json` and by the Rust validator. A body
the walker cannot run is refused with a 400 naming the field. Unknown fields
are refused, so a guard that is spelled wrong is an error and not a run that
skips its guard.

```json
{
  "input_schema": { "...": "optional JSON Schema; a run start that fails it is a 400" },
  "limits": { "max_steps": 200, "run_timeout_ms": 900000, "max_cost_usd": 2.5 },
  "steps": [ "...Node, in order..." ],
  "output": { "text": "${steps.reply.output.text}" }
}
```

`steps` is an ordered sequence walked in array order. `limits` are all
optional. `output` is an optional template rendered when the run reaches the
end of the graph, stored on the run.

### 3.1 Node

Every node has `"id"` (unique within the document, `^[a-z][a-z0-9_]*$`) and
`"kind"`. Every node except `end` (which is never written) accepts:

| Field | Type | Meaning |
| --- | --- | --- |
| `when` | Predicate | Evaluated when the cursor reaches the node. False writes an attempt row with status `skipped` and the cursor moves on. Absent means true. |
| `on_error` | OnError | What happens when the node fails after its retries. Default `fail_run`. |

Claimable nodes (`task`, `agent`) also accept:

| Field | Type | Meaning |
| --- | --- | --- |
| `input` | template JSON | Rendered per attempt and handed to the worker as the lease's `input`. Default: the run's input. |
| `effect` | Effect | Default `at_least_once`. |
| `step_timeout_ms` | integer | Bounds one attempt, heartbeats or not. |
| `retry` | `{ "max_attempts": 3, "backoff": "exponential", "initial_ms": 500, "max_ms": 30000 }` | Default `max_attempts: 1`, meaning no retry. `max_attempts` counts every attempt including the first. `initial_ms` default 1000, `max_ms` default 60000. |

The kinds:

- `task`: claimable. The worker runs whatever code it registered under this id.
- `agent`: claimable, identical on the wire. The kind is recorded so the trace
  and the UI can tell an agent turn from a plain step. The worker resolves the
  body; the daemon does not care.
- `sleep`: `{ "ms": integer }`. No process is alive during it.
- `sleep_until`: `{ "at": template string }` rendering to an RFC 3339 timestamp.
- `wait_event`: `{ "event": string, "key": template string, "timeout_ms": integer, "accept_parked": true, "on_timeout": "fail" }`.
  `key` is rendered per run, so `"${input.order_id}"` addresses this run and no
  other. The event payload becomes the step's output.
- `parallel`: `{ "branches": [ Node(task|agent)... ] }`. Each branch is one
  claimable step keyed `<id>/<branch id>`, all on disk before any is offered.
  The node is done when every branch has ended. A branch that fails follows its
  own `on_error`; `continue` lets the join finish.
- `foreach`: `{ "over": path, "body": Node(task|agent), "concurrency": 8, "max_items": 500 }`.
  `over` must resolve to a JSON array; each element becomes one attempt keyed
  `<id>[<index>]` whose `input` template can read `item` and `index`. An array
  longer than `max_items` fails the run. `concurrency` bounds how many items are
  claimable at once. Afterwards `steps.<id>.outputs` is the array of item
  outputs in item order.
- `loop`: `{ "body": Node(task|agent), "max_iterations": 8, "until": Predicate }`.
  Iteration `i` is keyed `<id>[<i>]/<body id>`. `until` is evaluated after each
  iteration; absent means run `max_iterations` times. Inside the body, `iter.index`
  is the iteration number and `steps.<body id>` is this iteration's attempt.
  Afterwards `steps.<id>.outputs` is the array of iteration outputs.
- `subworkflow`: `{ "definition": string, "version": integer?, "input": template JSON }`.
  A child run. `version` absent pins the highest stored version at the parent's
  start. The child's output becomes `steps.<id>.output`; a child that ends
  `failed`, `timed_out` or `cancelled` fails the parent step, which then follows
  its `on_error`. Cancel walks down. `max_steps` and `max_cost_usd` on the root
  bound the whole tree; a child's own limits also apply to it.

A branch, a foreach body and a loop body are ONE claimable step each in v1. A
multi-step body is a `subworkflow`. This is a limit the docs state.

Every duration has an upper bound as well as a lower one: `limits.run_timeout_ms`,
`step_timeout_ms`, `retry.initial_ms`, `retry.max_ms`, a sleep's `ms` and a
wait's `timeout_ms` are each at most 315360000000 (3650 days). A larger value
is a 400 that names the field, the bound and the value. A deadline is stored as
nanoseconds since the Unix epoch in a signed 64-bit integer, which ends in
April 2262, and a duration added to the clock past that range stopped the
daemon. A `sleep_until` is rendered per run and has no bound of its own: an
instant past April 2262 is stored as the latest instant the store holds, which
means it does not come due.

### 3.2 Predicate

```json
{ "path": "steps.order.output.amount_usd", "op": "lte", "value": 100 }
{ "all": [ P, P ] }   { "any": [ P, P ] }   { "not": P }
```

`op` is one of `eq`, `ne`, `lt`, `lte`, `gt`, `gte`, `in` (value is an array),
`exists` (no value), `matches` (value is a regular expression, applied to a
string). Comparison of unlike JSON types is false, never an error. A path that
resolves to nothing is treated as JSON null.

### 3.3 Template and path

A path is dot-separated with `[n]` for arrays: `input.order.lines[0].sku`.
The roots available at each position:

| Root | Holds |
| --- | --- |
| `input` | The run's input. |
| `steps.<id>` | `{ "status": AttemptStatus, "output": json?, "error": { "class", "message" }? }` of the latest attempt of that step, or `{ "status": "skipped" }`. For `parallel`, `steps.<id>.<branch>` works too. For `foreach` and `loop`, `steps.<id>.outputs` is the array. |
| `item`, `index` | Inside a `foreach` body only. |
| `iter.index` | Inside a `loop` body only. |
| `run` | `{ "run_id", "project", "definition_id", "definition_version" }`. |

A template is any JSON value. In a string, `${path}` is replaced by the value
rendered as text (a string verbatim, a number or bool as JSON, an object or
array as compact JSON, null as an empty string). A string that is exactly one
`${path}` and nothing else yields the JSON value itself, so `"amount": "${steps.order.output.amount_usd}"`
is a number. Objects and arrays are rendered recursively. There is no escape
sequence in v1; a literal `${` is not expressible.

## 4. Definition versioning

`POST /v1/api/definitions` and each document in `POST /v1/api/workers/register`
follow one rule. The canonical body is the document serialised with sorted keys
and no insignificant whitespace, minus `version`.

| The document's `version` | An existing version holds the same canonical body | Result |
| --- | --- | --- |
| absent | yes | that version, `assigned: false` |
| absent | no | `max(existing) + 1`, `assigned: true` |
| named and free | any | stored at that number, `assigned: false` |
| named, taken by an identical body | | that version, `assigned: false` |
| named, taken by a different body | | 409. A stored version never changes. |

A run started without a version pins the highest stored version at that
moment and records it. A subworkflow without a version is resolved the same way
at the parent's start and recorded in the parent run's `bindings`.

## 5. Runs

`POST /v1/api/runs` with `{ project?, definition_id, version?, input?, correlation?, idempotency_key? }`.

- `project` is a selector: the daemon hands it to `authorize_project`, answers
  403 on a mismatch, fills it in for a single-project grant, and answers 400
  when a multi-project grant names none. The same rule applies to every request
  body in this contract that carries `project`.
- `input` is validated against `input_schema` when the definition has one.
- `correlation` is the existing `Correlation` type minus `project` and `run_id`;
  it is stored on the run and copied onto every span the daemon writes for it,
  with `run_id` set to the run id.
- `idempotency_key` is unique per `(project, idempotency_key)` and written in
  the transaction that creates the run. A repost returns the same run and starts
  nothing, and two simultaneous posts of one key produce one run. The key is
  released after 24 hours by the tick, and the run keeps existing.
- The response is the `Run` as stored. The run row, its `trace_id`, its first
  cursor and everything that cursor requires commit in one transaction.

A run carries `trace_id` (32 hex, minted at start) and `state` (a JSON object
merged shallowly on every `complete` that sends one). Every step attempt
carries `span_id` (16 hex, minted when the attempt row is written).

### 5.1 What the run store keeps, for how long, and who read it

The run store keeps a run's `input`, `output`, `state` and `correlation`, every
attempt's rendered input, output, usage and error, every event payload and every
span payload still waiting in the outbox, exactly as they were sent. None of it
passes through the redactor: these are the workflow's own operational state,
and a later step reads them back, so a redacted value would change what the run
does. `correlation.user_id` is kept unhashed for the same reason. The spans the
drain writes into the trace store do pass through the redactor (section 9), so
the trace store holds the redacted, hashed copy.

- Retention. The daemon's retention pass also runs over the run store, on the
  same per-project window as the trace store. It deletes every run tree whose
  root ended before the cutoff, with every row keyed by those runs: attempts,
  events, deadlines, waits, outbox rows and the parked events they consumed. A
  tree with any run that has not ended is never deleted, however old. A project
  with no retention window is never trimmed. Definitions are not trimmed.
- Erasure. `DELETE /v1/api/subjects/{user_id}` also erases from the run store
  every run tree in the project with a run whose `correlation.user_id` equals
  the identifier as given (the run store holds it unhashed), with every row
  keyed by those runs. A tree with a run that has not ended is left whole and
  counted in `runs_in_flight`; `note` names the root runs to cancel before the
  request is sent again. The run store is erased before the trace store, so a
  span still queued for an erased run cannot land after the erasure. A parked
  event no run has consumed is keyed by its correlation key, not by a person,
  and is not reached; it is deleted when it expires, after 24 hours.
- Audit. `GET /v1/api/runs`, `GET /v1/api/runs/{run_id}`, `GET
  /v1/api/runs/{run_id}/events` (a stream counts once), `GET
  /v1/api/runs/{run_id}/cause` and `POST /v1/api/runs/{run_id}/cancel` are
  recorded in the audit log as `list_runs`, `get_run`, `list_run_events`,
  `get_step_cause` and `cancel_run`, allowed or refused, under the project that
  owns the run. The worker calls of section 7 are not recorded: they are a
  machine loop that claims many times a minute. `POST /v1/api/runs` is not
  recorded either; a repost of a held `idempotency_key` answers with the run
  that key started, so an execute key can read the runs it starts.

## 6. Events and waits

`POST /v1/api/events` with `{ project?, event, key, payload?, event_id? }`.
The daemon matches `(project, event, key)` against live waits inside their
window, delivers to the one whose wait expires soonest, writes the payload as
that step's output and moves the run in the same transaction. No waiting run
parks the event for 24 hours, and the next run reaching a matching wait with
`accept_parked` consumes it, oldest first.

Deduplication depends on who supplied the id. A supplied `event_id` seen
before answers `duplicate` and writes nothing, whether or not a run is waiting.
A derived id (`sha256(project:event:key:payload)`, used when the caller sends
none) answers `duplicate` only when no run is waiting; when a run is waiting the
event is delivered, because two identical approvals for two runs on one key are
two events and the hash cannot tell them apart. Send your own `event_id` from
any handler that can be retried. The receipt carries the id used.

`POST /v1/api/runs/{run_id}/signal` with `{ event, payload?, event_id? }`
delivers to one run by id: the daemon reads that run's open wait on `event`,
takes its key, and routes as above. 404 when the run holds no such wait.

Parked events past their expiry are deleted by the tick.

## 7. The worker protocol

Every worker call needs a key with the `worker` scope. All durations on the
wire are integer milliseconds; all timestamps RFC 3339 with `Z`.

### 7.1 Register

`POST /v1/api/workers/register` with `{ project?, worker_id, definitions: [DefinitionDocument...] }`.
Stores each document under section 4, answers `{ worker_id, registered: [{ id, version, assigned }...], lease_ttl_ms, heartbeat_ms }`.
Nothing about the worker itself is stored. Dispatch reads the runnable set,
never a worker table. `heartbeat_ms` is a third of `lease_ttl_ms`.

### 7.2 Claim

`POST /v1/api/steps/claim` with `{ project?, worker_id, max, wait_ms, run_id? }`.
`max` is 1 to 100; `wait_ms` is 0 to 60000. Long-polls up to `wait_ms` and
answers `200 { "leases": [...] }`, empty when nothing became runnable; the
worker reissues. `run_id` scopes the claim to one run, which is how a script
drives its own run inline.

The daemon holds a claim for at most four fifths of its request timeout
(`--request-timeout-seconds`, so 24000 ms at the default 30 s), whatever
`wait_ms` says, and answers empty when that passes, so a long `wait_ms` is never
cut off with a 503 by the timeout. A claim in flight when the daemon is told to
stop answers empty at once. An empty answer always means reissue.

A step is runnable when it has a `pending` attempt whose retry timer, if any,
has fired, its run is `running`, its root is `running`, no other attempt of the
same step is live or inside its reclaim window, and neither the run nor its
root has spent `max_steps` or `max_cost_usd`. A run refused for a spent
ceiling is ended `failed` with an error naming the ceiling.

The lease:

```json
{ "lease_id": "lse_...", "run_id": "run_...", "project": "acme",
  "definition_id": "refund_review", "definition_version": 3,
  "step_key": "large_refund", "kind": "task", "attempt": 1,
  "effect": "at_most_once", "expires_at": "...", "deadline_ms": 60000,
  "idempotency_key": "sha256 of run_id:step_key",
  "input": { "...": "the rendered input template" },
  "prior": { "order": { "status": "succeeded", "output": { "amount_usd": 240 } },
             "small_refund": { "status": "failed", "error": { "class": "retryable", "message": "429" } },
             "approval": { "status": "skipped" } },
  "state": {},
  "correlation": { "project": "acme", "run_id": "run_...", "session_id": "sess_41" },
  "trace_id": "4bf9...", "span_id": "a3ce...", "parent_span_id": "9d10..." }
```

`prior` is keyed by step id, latest attempt per step, in the shape section 3.3
gives; it carries only ended attempts. `deadline_ms` is the step timeout or
absent. `span_id` is the span the daemon has already opened for this attempt;
a worker that records its own spans (a model call, a tool call) sets
`trace_id` to the lease's and `parent_span_id` to the lease's `span_id`, so
they nest under the step in the trace. `parent_span_id` on the lease is the run's
root span.

A worker that receives a lease for a `definition_id@definition_version` it
does not serve, or a `step_key` it does not recognise, fails it with class
`unservable` rather than guessing.

### 7.3 Started

`POST /v1/api/steps/{lease_id}/started`, empty body. Moves the attempt from
`leased` to `started`, extends the lease, closes an open reclaim window.
Answers `{ expires_at, reclaimed }`. Post it, wait for the 200, then run the
body. Required under `at_most_once`: a `complete` or `fail` on a `leased`
`at_most_once` attempt answers 409 `not_started`. Optional under
`at_least_once`; what a worker gives up by skipping it is only that a crash
mid-body is recorded in the journal as a requeue and not in the trace as an
error on that attempt.

### 7.4 Heartbeat, and what a 409 means

`POST /v1/api/steps/{lease_id}/heartbeat`, empty body, every `heartbeat_ms`
while the body runs, whether or not `started` was posted. Answers
`{ expires_at, reclaimed }`. `reclaimed` is true when this call found the
attempt `orphaned` and restored it to the state it came from.

Every 409 on started, heartbeat, complete and fail carries
`{ "error": "conflict", "reason": ConflictReason, "message", "attempt", "attempt_status"?, "run_status" }`:

| `reason` | What is true | What the worker does |
| --- | --- | --- |
| `fenced` | a later attempt exists, or another lease terminated this attempt | abandon the work; post nothing more for this lease |
| `already_applied` | this lease's own earlier `complete` or `fail` landed and its reply was lost | treat the step as done; `attempt_status` says how; post nothing more |
| `not_started` | `complete` or `fail` on a `leased` `at_most_once` attempt | a bug in this worker: post `started` first; log it loudly |
| `run_terminal` | the run has ended | stop, and stop claiming for this run |

`already_applied` is decidable: the attempt row records the lease that ended
it, and an ended attempt whose ending lease is the caller's own is the caller's
own committed work. Any other ended attempt is `fenced`.

### 7.5 Complete

`POST /v1/api/steps/{lease_id}/complete` with `{ output?, state?, usage?, error_span? }`.
One transaction: the attempt becomes `succeeded` with `output`; `state` is
merged shallowly into the run's state; `usage` (`TokenUsage` plus `model` and
`provider`) is priced by the daemon's price book and charged to the attempt, the
run and the root; the step's deadlines are deleted; the closing step span is
enqueued; once every slot the cursor requires has ended, the cursor advances
and everything the next position requires is written. Answers
`{ run_status, next }` where `next` is `claim` when more work is runnable now
and `idle` otherwise.

### 7.6 Fail

`POST /v1/api/steps/{lease_id}/fail` with `{ error: { class, message, details?, retry_after_ms? } }`.

| class | daemon behaviour |
| --- | --- |
| `retryable` | if attempts remain, write a `retry` timer at the backoff (honouring `retry_after_ms` when larger) whose firing writes attempt `n+1` as `pending`; else apply `on_error`. |
| `fatal` | no retry; apply `on_error`. |
| `invalid_input` | no retry; apply `on_error`. |
| `unservable` | the attempt returns to `pending` after one lease length, without spending the retry budget; the run's own deadline bounds it. |
| `cancelled` | no retry; apply `on_error`. |

`on_error: fail_run` ends the run `failed` with the message. `on_error: continue`
marks the step `failed` and advances, so a later `when` can read
`steps.<id>.status` and `steps.<id>.error`. Answers `{ run_status, next, attempts_left }`.

`retry_after_ms` is at most 315360000000, the duration bound of section 3. A
larger value is a 400 naming `error.retry_after_ms`, and nothing is written, so
the worker still holds the lease and can report again.

### 7.7 Reclaim

A lease with no heartbeat past `expires_at` moves to `orphaned` and opens a
reclaim window of one lease length. A heartbeat inside it restores the attempt.
A window that closes with no contact: a never-started attempt is requeued under
either effect; a started `at_least_once` attempt is requeued; a started
`at_most_once` attempt fails the run rather than running twice. A requeue after
a reclaim never spends the retry budget: the worker reported no failure, the
daemon lost contact, and charging a lost lease to a budget whose default is one
attempt would let a daemon deploy end every in-flight run. The step and run
deadlines bound a step that keeps losing its worker.

### 7.8 A cancel

`POST /v1/api/runs/{run_id}/cancel`. The run and every child under it move to
`cancelled`; their deadlines and waits are deleted; every live attempt is
marked `failed` with `run cancelled`; a parent of a cancelled child sees the
child's step fail and follows its `on_error`, unless the parent itself was the
run cancelled. A worker holding a lease learns on its next call: 409
`run_terminal`. Cancelling an ended run changes nothing and answers 200.

Every run that ends by any route fails its live attempts the same way, so the
journal never shows a live attempt on a dead run.

## 8. The event log

`GET /v1/api/runs/{run_id}/events?after=<seq>&limit=<n>` answers
`{ events: [RunEvent...], has_more }`. With `Accept: text/event-stream` the same
route answers SSE, one frame per row with `id` set to `seq`, and stays open until
the run ends or the client leaves; `after` is the reattach point. Every
lifecycle row is written in the transaction that made the change it describes,
with `seq` allocated from the run header, so a reader that has seen `n` has seen
every row below it.

| kind | payload |
| --- | --- |
| `run_started` | `{ definition_id, definition_version }` |
| `run_cursor` | `{ index, construct, step_key? }` |
| `step_leased` | `{ step_key, attempt, worker_id }` |
| `step_started` | `{ step_key, attempt }` |
| `step_finished` | `{ step_key, attempt }` |
| `step_failed` | `{ step_key, attempt, class, message, attempts_left }` |
| `step_skipped` | `{ step_key }` |
| `run_waiting` | `{ step_key, event, key, expires_at }` |
| `run_resumed` | `{ step_key, event_id }` |
| `run_ended` | `{ status, error? }` |

## 9. The trace

The daemon writes spans for every run through the ordinary ingest pipeline
(sampling, redaction, pricing, storage, forwarding), keyed so a retry never
duplicates. Rows are enqueued in the transaction that made the change and
drained by a loop; a crash between the two loses nothing.

| Span | kind | open | closed |
| --- | --- | --- | --- |
| the run | `workflow_run` | at start, `is_root`, name = definition id | at end, status from `RunStatus` |
| a claimable attempt | `workflow_step`; `agent_run` for an `agent` node | at `started`, or at lease when `started` is never posted | at complete, fail, timeout or reclaim close, with `attributes.attempt`, `attributes.step_key`, `attributes.usage`, `attributes.cost_usd` |
| a sleep | `workflow_sleep` | written closed when it fires | |
| a wait | `workflow_wait_event` | at the wait | at resume or timeout, `attributes.event`, `attributes.key` |
| a skipped node | `workflow_conditional_eval` | written closed, `attributes.result: false` | |
| a subworkflow | the child's own `workflow_run`, whose `parent_span_id` is the parent step's span | | |

The attempt's priced usage and cost ride in the step span's attributes, never
in the span's own `usage` and `cost`. A step span is a container: the model
spans a body records under it already carry the tokens they spent, and a
trace total that summed both would count every token twice. The run and the
attempt carry the priced numbers; the trace shows where they were spent.

`ingest_key` is `run:<run_id>:<step_key>:<attempt>:<open|close>` and
`run:<run_id>:root:<open|close>`. Correlation on every span is the run's
correlation with `run_id` set. A closing span carries the identical `trace_id`,
`span_id` and `start_time` as its open row, so it replaces rather than
duplicates on every backend.

## 10. Not in version 1

Named so a reader can tell a decision from neglect.

- `Mcpd-Request-Id` and the `step_requests` table. The 409 `already_applied`
  reason covers the case that matters: a landed `complete` or `fail` whose reply
  was lost. A lost `claim` reply costs one lease length plus one reclaim window
  of delay on up to `max` steps, and nothing else.
- `after` constraints. Order is the array; a guard reads `steps.<id>.status`.
- `compensate` and the compensator role.
- `POST /v1/api/runs/{run_id}/retry` (a new attempt on a settled step, time
  travel) and effect epochs.
- `POST /v1/api/steps/{lease_id}/deltas` and token streaming out of the daemon.
  In-process streaming is the worker's own business.
- A Postgres or ClickHouse run store. The run store is an embedded SQLite file
  beside the signal store, whatever the signal store is, so the run surface is
  one daemon replica in v1. The docs say so on the deploy page.
- Definitions of kind `agent`, the in-process agent loop, tool contracts, model
  routing, memory. An `agent` node in v1 is a claimable step whose body is the
  user's own loop; the trace shows the model calls it records.
- Multi-step bodies for a branch, a foreach item or a loop iteration. Use a
  subworkflow.
