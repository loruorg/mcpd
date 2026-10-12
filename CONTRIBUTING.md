# Contributing to mcpd

mcpd is a daemon that records what AI agents did. It is free and open-source
software under [Apache-2.0](LICENSE), and contributions are welcome.

Read [`AGENTS.md`](AGENTS.md) first. It is the contract for this repository, and
it is short. This file covers the rest: how to set the repository up, how to run
what CI runs, and what a reviewer will look for.

## Before you start

- Search the open issues and pull requests. Someone may be on it already.
- For anything larger than a fix, open an issue and say what you plan to do. A
  disagreement about design costs less before the code than after it.
- Keep a pull request to one subject. Two unrelated changes in one branch take
  longer to review than the same two changes apart.

## The one rule

> Nothing that describes the data model or the HTTP API is written by hand more
> than once.

`spec/mcpd.spec.json` is the single source of truth. The Rust model, every SDK's
model types, the OpenAPI document and the reference documentation are generated
from it by `tools/codegen/`.

Generated files carry a `GENERATED FILE` banner, and `make check` fails when one
differs from what the spec currently produces. If you find yourself editing a
file with that banner, stop. The change belongs in `spec/mcpd.spec.json` or in
the generator, and then in the output of `make gen`.

Every field you add to the spec needs a `doc` string written for a reader. The
validator rejects one without.

## What you need installed

You do not need all of these. `make check` needs Rust and Node. A toolchain you
do not have is reported as skipped by `make test` and `make conformance`, never
passed over quietly.

| Toolchain | Version | Needed for |
| --- | --- | --- |
| Rust | 1.88 or newer, stable | The daemon, the store, the Rust SDK, `make check` |
| Node | 22.12 or newer | Code generation, the consistency checks, the site, the conformance harness |
| Python | 3.9 or newer | The Python SDK and its example |
| Go | 1.21 or newer | The Go SDK and its example |
| JDK | 17 | The Kotlin SDK |
| Elixir | 1.17 or newer, on OTP 27 or newer | The Elixir SDK, which calls OTP 27's `:json` module. CI runs Elixir 1.18 on OTP 27 |

## The loop

Run `make ui` once after you clone. The interface is compiled into the daemon
from `crates/mcpd-server/ui`, which is generated and not tracked, so the Rust
crates do not build until it exists, and `make check` does not build it for
you. Run it again when you change the interface.

```shell
make ui       # once after cloning: build the interface into the server crate
make gen      # regenerate everything derived from the spec
make check    # everything CI runs
```

`make check` is the gate. It runs the drift check, the `THIRD_PARTY.md` check,
`cargo fmt --check`, Clippy with warnings denied, the Rust test suite, the
cross-cutting consistency checks and the site build. If it passes locally it
passes in CI, so run it before you open a pull request.

Two more that help while you work:

```shell
make dev      # the daemon with demo data and the site, both watching
make fmt      # format the Rust code
```

Never weaken a check to make it pass. If a check is wrong, say so in the pull
request and fix the check on its own terms.

## Change, and what else must change

`AGENTS.md` has the full table, one row per kind of change, and the row tells you
every file that has to move with yours. This is the index into it.

| You changed | Start at |
| --- | --- |
| The data model or the HTTP API | `spec/mcpd.spec.json`, then `make gen`, then every storage backend |
| How an SDK behaves | `sdks/SDK-CONTRACT.md` first, then all of the SDKs |
| A code sample in the documentation | `examples/<language>/`, never the documentation page |
| The daemon's flags or configuration | `crates/mcpd/src/main.rs` and `web/src/pages/docs/configuration.astro` |
| A dependency, in `Cargo.toml` or `web/package.json` | `make third-party`, and commit the regenerated `THIRD_PARTY.md` |
| A storage backend | The `Store` trait, all three schema files, a CI job against a real server |
| A scorer, a watch or an SLO window | `crates/mcpd-core/src/scoring.rs` or `crates/mcpd-server/src/monitor.rs`, and the table on the matching documentation page |

Two of those deserve repeating, because they are the ones that get missed.

**A new field is not done when it compiles.** Storage columns and queries go in
every backend, not the one you tested. The `Store` trait will not compile until
each backend implements a new method, which is the point of it being a trait.

**A handler with no route is dead code that tests still pass.** The route goes in
`crates/mcpd-server/src/lib.rs`.

## The SDK contract

[`sdks/SDK-CONTRACT.md`](sdks/SDK-CONTRACT.md) is the surface every SDK
implements. The names follow each language's conventions. The behaviour does not
change between them, and behaviours **B1 to B10** are binding in all of them.

Three of those are the reason the project exists in this shape, and a pull
request that breaks one will not be merged:

- **B1, never throw from instrumentation.** A failed export, an unreachable
  server, a serialisation error: logged at most once per interval, then
  swallowed.
- **B2, non-blocking by default.** Recording a span appends to an in-memory
  queue and returns. The network happens on a background worker.
- **B3, bounded memory.** The queue has a maximum. When it is full the oldest
  spans are dropped and a counter increments.

An observability library that takes down production has done more harm than no
observability at all.

Change the contract file first, then the code. Six SDKs implement it, in
TypeScript, Python, Rust, Go, Kotlin and Elixir, and a seventh conformance probe
drives the same run over raw HTTP with cURL. A behaviour that holds in one
language and not another is worse than one that holds nowhere, because it teaches
the reader something untrue.

## Running the tests

### Everything CI runs

```shell
make check
make test     # Rust tests plus every SDK's tests, skipping absent toolchains
```

### Cross-language conformance

Each probe emits the same canonical run through its own SDK against one live
daemon, and the harness reads all of them back through the HTTP API and asserts
they are structurally identical. This is the only test that can catch a
difference between two languages.

```shell
make conformance
```

It builds the daemon, starts one on a port the operating system assigns, or on
`MCPD_E2E_PORT` when you set it, and runs whichever languages have a toolchain
present. It reports the ones it skipped. To make a skip a failure, which is
what CI does:

```shell
MCPD_REQUIRE_ALL_LANGUAGES=1 node tests/e2e/run.mjs
```

The same target runs `tests/e2e/runs.mjs`, the same idea for durable runs:
every SDK's worker serves the canonical workflow in
`tests/e2e/probes/runs/canonical.json` against one daemon, and the definition,
the run, its journal and its trace are compared across the six languages and
against what the canonical file pins.

CI installs every toolchain the probes need in one place,
`.github/actions/conformance-setup/action.yml`, so that variable can hold there.
The Kotlin probe wants the compiler at `/tmp/kotlinc`, the compiled SDK at
`/tmp/ktbuild` and its jars at `/tmp/kdeps`, or the `MCPD_E2E_KOTLIN_*`
variables named in `tests/e2e/languages.mjs`; that action shows the exact
commands.

### The other end-to-end suites

```shell
make e2e
```

Three more suites, each against a daemon it starts itself: every SDK against a
daemon that is broken in eight ways, the MCP server over stdio, and a run that
corrupts batches on the wire to prove the conformance checks can go red. CI
runs them in the `e2e` job. `tests/e2e/README.md` describes each one.

### The judge, against a real model

`crates/mcpd-server/src/judge.rs` has unit tests, and every one of them is about
parsing. None of them calls a model, so none of them can tell you whether a real
model separates a run that answered its question from one that did not.
`make judge-e2e` does that, and it is not part of `make check`, because every run
it grades is a paid request.

```shell
make judge-e2e
```

Credentials come from `LLM_API_ENDPOINT` and `LLM_API_TOKEN`, in the environment
or in `.env`, which is gitignored. With neither set the suite reports a skip and
exits zero, so it is safe to run anywhere. Set `MCPD_REQUIRE_LLM=1` to turn that
skip into a failure, which is what a pipeline meant to run it should do, so it
cannot go green by doing nothing.

Run it when you change the judge, the prompt it sends, or the criteria contract.

### One SDK at a time

The unit tests need nothing running. Set `MCPD_TEST_ENDPOINT` to a live daemon
and the same suites also run their integration tests against it. Without the
variable those are skipped, so an offline run stays green.

```shell
cargo run -p mcpd -- demo --data /tmp/mcpd-test.db --listen 127.0.0.1:4317
export MCPD_TEST_ENDPOINT=http://127.0.0.1:4317
```

| SDK | Command |
| --- | --- |
| TypeScript | `cd sdks/typescript && npm install && npx tsc --noEmit && npm test` |
| Python | `cd sdks/python && python3 -m pytest -q` |
| Rust | `cargo test -p mcpd-sdk` |
| Go | `cd sdks/go && go vet ./... && go test ./...` |
| Kotlin | `cd sdks/kotlin && gradle build` (no wrapper is tracked; CI installs Gradle 9.7.0) |
| Elixir | `cd sdks/elixir && mix test` |

### Postgres and ClickHouse

Both backends are tested against a real server, never a mock. Every bug either
one had while it was written appeared only once the query reached the engine:
an aggregate nested inside an `if`, an alias shadowing the column it read, two
replicas deadlocking on a concurrent `CREATE TABLE IF NOT EXISTS`, a JSON value
sent to a text column. A mock passes all four.

Both suites are gated on an environment variable, so an ordinary `cargo test`
needs no services. Start a server, then point the suite at it.

```shell
docker run --rm -p 5432:5432 \
  -e POSTGRES_USER=mcpd -e POSTGRES_PASSWORD=mcpd -e POSTGRES_DB=mcpd \
  postgres:17-alpine

MCPD_TEST_POSTGRES_URL=postgres://mcpd:mcpd@localhost:5432/mcpd \
  cargo test -p mcpd-store --features postgres --test postgres
```

```shell
docker run --rm -p 8123:8123 \
  -e CLICKHOUSE_SKIP_USER_SETUP=1 \
  clickhouse/clickhouse-server:24.8-alpine

MCPD_TEST_CLICKHOUSE_URL=http://localhost:8123 \
  cargo test -p mcpd-store --features clickhouse -- --include-ignored clickhouse
```

`crates/mcpd-store/tests/datasets.rs` reads `MCPD_TEST_POSTGRES_URL` too. It runs
its assertions against SQLite always, and against Postgres as well when the
variable is set.

## Documentation and examples

The documentation contains no copies of code. It names a file and a region, and
`web/src/lib/snippets.ts` reads the current contents at build time. So a change
to a code sample is a change to `examples/<language>/`, not to a page.

- Regions use the language's own comment syntax: `#region init` to
  `#endregion init`.
- Every example needs all four regions: `init`, `trace`, `score`, `query`.
- Keep a region between 8 and 46 lines, with lines under 100 characters, so it
  does not overflow a documentation column. `make check` enforces both limits.
- Deleting a region breaks the site build on purpose. A page that renders an
  empty code block teaches nothing and hides the breakage.

## The web surfaces

`web/` holds the site, the documentation and the operator interface. It follows
`DESIGN.md` and the direction contract in `web/src/layouts/Base.astro`. Use the
design tokens, never a raw hex value. Content is visible by default: nothing is
ever gated on an animation completing, because an animation that does not run
leaves an empty section.

A graphic must be computed from the thing it depicts. A hand-written timeline
that merely looks right drifts from the behaviour it claims to show, and then the
documentation is lying in a way no test can catch.

## House style

The voice rules apply to code comments, documentation and interface copy alike,
and `tools/check-consistency.mjs` enforces part of them:

- Plain and precise. Short sentences. Address the reader as "you".
- No em-dashes. Use a comma, a colon, or split the sentence.
- British spelling: behaviour, colour, recognise.
- Banned words, among others: simply, easily, powerful, seamless, blazing,
  effortless, leverage.
- Never invent a fact. No performance numbers, no customers, no benchmarks, no
  adoption figures. None exist. If something is not implemented, say so.

Comments explain why, not what. The useful comment says what would go wrong
otherwise, or why the obvious approach was rejected.

Errors name the problem and the recovery. `invalid input` is a bug report waiting
to happen. `--sample must be between 0.0 and 1.0, got 4.2` is an answer.

## Branches

`main` is the branch the project uses. Pull requests target it, `make check`
is what has to pass on it, and a release is a tag on it. The workflows under
`.github/workflows` also run on a branch called `master`, so a clone or a mirror
that still uses that name is checked rather than quietly unchecked; it is the
same code path, not a second branch.

## Commits and pull requests

Commit messages follow `type: subject`, where the type is one of `feat`, `fix`,
`chore`, `docs`, `refactor`, `test` or `perf`. The subject is lower case and says
what changed. The body says why, and is worth writing for anything that is not
obvious from the diff.

### Sign your commits off

Every commit needs a `Signed-off-by:` trailer with your name and email, which
`git commit -s` adds for you:

```
Signed-off-by: Your Name <you@example.com>
```

The trailer is your statement, per commit, that you wrote the change or have
the right to submit it under the project's licence: the
[Developer Certificate of Origin](https://developercertificate.org/), which is
short and worth reading once. It is the record that answers the question
"who says this code may be here?" for every line, and it is checked by the
`dco` workflow on every pull request. A commit without one fails that check,
and the failure names the commit and the two commands that fix it:
`git commit --amend -s` for one commit, `git rebase --signoff <base>` for a
branch. The trailer has to name the commit's author or committer; a sign-off
copied from somebody else's commit does not count.

This sits alongside the contributor agreement below rather than replacing it.
The agreement is the licence grant; the sign-off is the per-commit record.

The pull request template asks which row of the `AGENTS.md` table your change
falls under. Answer it. If no row fits, say so, because a gap in that table is
worth knowing about.

A good pull request also:

- names the issue it closes, as `Fixes #123` or `Refs #123`,
- says what you ran, and pastes the tail of `make check`,
- includes a screenshot or a recording for a change to the web surfaces,
- leaves "Allow edits from maintainers" ticked.

### AI-assisted contributions

A model may write your code. It cannot speak for you.

- Say in the pull request description whether AI was used and what it did.
  Disclosing it is never held against you.
- No AI co-authors. Strip any `Co-Authored-By:` trailer naming a tool, and any
  "Generated with" line, before you push. Most coding agents add them by default.
- Write your own words in the description and in review. Imperfect English is
  welcome. Model English costs a reviewer time in proportion to how confident it
  sounds.

If a model wrote the change, you still have to have read it. You are the author
of anything you open.

## Licence and the contributor agreement

Contributions are licensed under [Apache-2.0](LICENSE), the same licence the
project ships under. Before your first pull request is merged, agree to the
[Contributor License Agreement](CONTRIBUTOR_LICENSE_AGREEMENT.md). It grants
copyright and patent licences to Internet Technology Services LLC and to
everyone who receives the software. You keep every right to your own work, and
there is no relicensing clause.

## Getting help

- Questions about using mcpd: [`SUPPORT.md`](SUPPORT.md).
- A security issue: [`SECURITY.md`](SECURITY.md). Never a public issue.
- Behaviour in the community: [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md).
- Who reviews what: [`MAINTAINERS.md`](MAINTAINERS.md).
