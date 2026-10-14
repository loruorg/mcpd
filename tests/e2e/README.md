# End-to-end suites

Five programs, each one against a daemon it starts itself, on a port the
operating system says is free.

| Suite | What it holds | What runs it |
| --- | --- | --- |
| `run.mjs` | Seven languages produce the same trace | `make conformance`, and the `conformance` job in CI |
| `runs.mjs` | Six SDK workers serve the same durable run | `make conformance`, and the `conformance` job in CI |
| `mutations.mjs` | Those checks fail when the thing they guard is broken | `make e2e`, and the `e2e` job in CI |
| `errors.mjs` | Every SDK survives a broken daemon, and the daemon survives a broken client | `make e2e`, and the `e2e` job in CI |
| `mcp.mjs` | `mcpd mcp` answers over stdio and obeys the key scopes | `make e2e`, and the `e2e` job in CI |
| `judge.mjs` | A real model separates a good run from a bad one | `make judge-e2e`, by hand, because it costs money |

None of them is in `make check`, which is `gen-check lint-rust test-rust
consistency site`, because each needs every SDK toolchain to say anything. They
run in CI instead: `conformance` and `e2e` are jobs in `.github/workflows/ci.yml`,
and `make conformance` and `make e2e` run the same suites locally.

`languages.mjs` holds the probe table the suites share: how to run each
language, and how to know whether it can run at all. One copy, because two would
drift and the copy that drifts reports the language as skipped, which is the one
result nobody reads.

Every suite reports a language it could not run rather than passing over it
quietly. `MCPD_REQUIRE_ALL_LANGUAGES=1` turns a skip into a failure.
`MCPD_E2E_LANGUAGES=typescript,python` narrows a run while working on a suite;
the languages it leaves out are reported as skips, so the same flag cannot be
used to make a suite green by doing less.

The suites run the daemon `cargo build -p mcpd` wrote, under `CARGO_TARGET_DIR`
when it is set and `target/` when not. `MCPD_BIN` names another binary.

## Cross-language conformance

Every SDK is unit-tested on its own. That proves each one does what its authors
meant, and proves nothing about the property that actually matters: that all
seven produce the **same trace** for the same agent.

This suite is the only test that can. Each probe emits one canonical run through
its own SDK against one live daemon, and the harness reads all seven back
through the HTTP API and asserts they are structurally identical:

- the span tree, the kinds and how status propagates,
- all five signals: spans, logs, metrics, scores and feedback,
- correlation: session, thread, user, request and release, on every step, plus
  the three fields that come from the environment variables the contract fixes,
- the error: the same status, the same message, in the same place,
- token counts, and the cost mcpd computed from them,
- score and feedback filtering, which is how either is found again,
- idempotency: the batch each SDK actually sent, replayed byte for byte.

Two of those are read off the wire rather than out of storage. A recording proxy
sits between the probes and the daemon, so the harness can see whether a batch
carried an `ingest_key` at all and can replay that exact batch. Nothing stored
answers either question.

The run ends by erasing the one person every language said the run was for. One
`DELETE /v1/api/subjects/{user_id}` reaches all seven runs only if every SDK
produced the same digest from the same identifier, so it is a cross-language
check and not a daemon one.

    node tests/e2e/run.mjs

Set `MCPD_E2E_DEBUG=1` to see every request the probes made.

### When a language disagrees

A difference between two languages shows up here and nowhere else, and the point
of the suite is to say so rather than to go green. Differences already found are
listed in `KNOWN_GAPS` in `run.mjs`, each one naming the file and function that
causes it. They are printed on every run under "known difference(s) between
languages" and they do not fail the suite, so one open gap cannot hide the next
one. A difference that is not in that list fails the run.

`SHAPE_GAPS` is the same set of differences named by the field they show up in.
A language listed there is left out of the cross-language comparison for that
ONE field and compared on every other, and the field is still compared between
every language not listed. The reference a field is compared against is the
first language that ran which is not listed for it, so the order languages run
in decides nothing.

**Both lists must be true, and the suite checks that they are.** An entry for a
behaviour somebody has since fixed fails the run, and the failure names the line
to delete. That is what stops the list becoming a place a real regression can
land unnoticed: a suppression with nothing under it still suppresses. So
deleting an entry is how a fix is proved, and it is not optional. Ten of the
twelve entries this arrangement inherited were already stale on the first run
that audited them.

The audit says nothing about a language that did not run, so narrowing a run
with `MCPD_E2E_LANGUAGES` cannot turn a gap into a failure.

### The canonical run

`probes/canonical.json` is the run itself, written once and read by the harness.
Each probe writes the same run in its own language, and reads like the code a
user would write rather than like a fixture. A probe prints its trace id on its
last line and nothing else.

`SDK-CONTRACT.md` says behaviours B1 to B10 are binding in every language; this
is what makes that sentence checkable rather than aspirational.

## Runs conformance

`runs.mjs` is the same argument for durable runs. Every SDK carries a workflow
builder and a worker, each unit-tested against a fake daemon; nothing but this
suite proves that all six serve the **same run** the same way.

Each probe under `probes/runs/` builds the canonical workflow through its own
builder, registers it, starts a run, publishes the approval event before the
wait exists, so the event is parked and taken when the wait opens, and serves
the run to its end with its own worker: `started`, a span nested under the
daemon's step span, usage the daemon prices, a retryable failure the daemon
retries, an `at_most_once` step, and the run's output template. The harness
reads the definition, the run, its journal and its trace back and asserts:

- the stored definition body is exactly `probes/runs/canonical.json`'s,
- the run succeeded with the pinned output and state, on version 1,
- every attempt has the pinned status, input, output, error class and usage,
  and names the probe as its lease owner,
- the journal starts with `run_started`, ends with `run_ended`, records the
  retryable failure, and has no gaps in its sequence numbers,
- the body's span sits under the draft step's span in the run's trace and
  carries the run's session and user,
- the first publish was `parked` and the second a `duplicate`,
- and every one of those shapes is identical across the languages that ran,
  down to the correlation each body's span carries.

    node tests/e2e/runs.mjs

There is no curl probe: the worker protocol is a loop with state, and a shell
that drove it would be an SDK by another name.

## The error paths

`run.mjs` only ever meets a daemon that works. `errors.mjs` is the other half:
what every SDK does when the daemon does not.

B1 says instrumentation never throws, B2 that it never blocks the agent, and B5
that `shutdown()` returns. Each SDK checks those against its own fake transport,
which is the one place they cannot fail. This suite stands up eight ways a
deployment actually breaks and runs the same canonical probes against them:
nothing listening, an endpoint that does not resolve, a 500 on every batch, a
connection dropped mid-response, a 200 whose body is not JSON, a 413, a keyed
daemon that refuses the batch, and a daemon that accepts the batch and never
answers.

The unresolvable endpoint is not the same failure as a refused connection. It
happens inside the client's own name lookup, which in several HTTP libraries is
the one call that throws synchronously, straight into the agent's call stack.

The probe has to exit zero, print its trace id, and take no longer than it took
against a healthy daemon. The healthy figure is measured in the same run, so the
bound is a fact about the SDK rather than about the machine.

`curl` is the control. `probes/probe.sh` is raw HTTP with no SDK, so it fails
every scenario an SDK is expected to survive. If it passes one, the scenario was
never hostile and every other language passed it for nothing. The one scenario
it survives is named in the code, with the reason.

The second half is the same question from the other end: an oversized body,
malformed JSON, an unstorable item, no key and the wrong key. What is checked is
not only the status but that the daemon is still serving afterwards and still
accepts a good batch. A hostile client must not be able to take mcpd down.

    node tests/e2e/errors.mjs

## The MCP server

`mcpd mcp` is what lets an agent read its own traces. What has to be true is not
that a function returns a string: it is that a real child process, spoken to in
JSON-RPC over stdin and stdout, answers a real daemon's data.

    node tests/e2e/mcp.mjs

Three properties it holds. That stdout carries JSON-RPC and nothing else, since
one stray log line drops the session in every MCP client there is. That a run
comes back as a readable waterfall with the prompts rendered as prose, so a
regression to a JSON dump fails here. And that it obeys the key scopes: the
daemon is started with two projects and two keys, and the crossing is attempted.

## The judge

`judge.rs` has unit tests and every one of them is about parsing. None calls a
model, so none can say whether the thing works.

    node tests/e2e/judge.mjs

It grades a run that answers its question and a run that does not, and asserts
the **gap** between them rather than a number: the same model behind the same
gateway returns 0.9 one week and 0.95 the next, and a test that pins 0.9 fails
for a reason that has nothing to do with mcpd. It also counts the calls that
reach the model, because a decline that still spent money is a bug you cannot
see in the response body.

This costs money, which is why it is not in `make check`. With no credentials it
reports a skip and exits zero, unless `MCPD_REQUIRE_LLM=1`, which turns the skip
into a failure so a pipeline meant to run it cannot go green by doing nothing.

## Proving a check can fail

Seven tests in this repository have reported success without testing anything.
An assertion nobody has watched go red is not evidence, so break the thing a
check guards, once, and watch it fail before you trust it.

`mutations.mjs` is that habit, made repeatable:

    node tests/e2e/mutations.mjs

Each case names a fault and the checks it must break. `run.mjs` injects the
fault into every batch on its way to the daemon, through the proxy it already
runs, so one corruption reaches every language at once. A check that stays green
under the fault that breaks it is reported as what it is: not a check. It takes
about ten seconds and drives two languages, because the fault is applied to the
wire and one language proves the check as well as seven do.

    MCPD_E2E_FAULT=flat-trace node tests/e2e/run.mjs

runs a single fault by hand. `MCPD_E2E_FAULT_LANG=curl` corrupts one language
and leaves the rest alone, which is the only way to put the cross-language
comparison itself to the test: six agree and the seventh does not. A run with a
fault set can never pass. If nothing failed, that is itself the failure, because
the corruption reached the daemon and no check noticed. That is how the missing
check on a model call's `provider` was found.

The two timing bounds have no fault, because the environment is the input:

    MCPD_E2E_SLACK_MS=0 node tests/e2e/errors.mjs        # the B2 timing checks
    MCPD_E2E_BLACK_HOLE_MS=1 node tests/e2e/errors.mjs   # the B5 ceiling

For a check about what one SDK sent, edit its probe under `probes/` and run the
conformance suite: changing `gpt-4o` in `probe.py` fails that language's model
and cost checks and prints the disagreement with both values. For a check about
what the daemon did, change the daemon and rebuild; `cargo build -p mcpd` is a
second or two once the workspace is warm.

### Two checks that could not fail apart

A pair of checks where one cannot fail unless the other does is one invariant
reported as two, and it makes the suite's own count say more than the suite
knows. Two pairs were folded back into one check each: the number of spans is
already part of comparing their names and kinds, and counting the roots is
already part of asking whether every other span hangs off the root. Before
adding a check, ask what state makes it fail while the one beside it passes. If
there is none, it is a sentence, not a check.

Four defects found this way, all now fixed, are worth knowing about because they
are the shape the next one will take:

- The check on the span tree passed a completely flat trace. mcpd derives
  `is_root` from the absence of a parent, so a flattened run comes back as four
  roots, the list of non-root spans is empty, and `every` over an empty list is
  true. It counts the roots as well, which is why `flat-trace` in `mutations.mjs`
  exists and why the run against it is kept.
- The provider a model call was made against was compared between languages and
  asserted nowhere, so seven SDKs that all dropped it would have agreed
  perfectly and passed. `provider-renamed` found it, because a fault that breaks
  no check fails the run on its own.
- The cross-language comparison ran against whichever language happened to run
  first, so every field that language was already excused from was excused for
  every other language too. Two fields were unguarded across all seven at once.
  Each field is now compared against the first language not excused from it.
- Ten of the twelve entries in `KNOWN_GAPS` described behaviour somebody had
  already fixed. Nothing said so, and the suppression each one left behind was a
  place a new disagreement could land and be printed as an old one.
- `mcp.mjs` bound a fixed port. A daemon left on that port from an earlier run
  answered instead, its own daemon never started, its database stayed zero
  bytes, and 76 checks went green having tested a different process entirely.
  Every suite now takes a port from the operating system and refuses to run if
  something else is answering on it.
