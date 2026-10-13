# Working on mcpd

This file is the contract for anyone changing this repository, human or agent.
`CLAUDE.md` points here; so does the contributing link in the footer.

Read the section that matches what you are about to change, do what it says, and
run `make check` before you finish. `make check` is the same thing CI runs.

---

## The one rule

> **Nothing that describes the data model or the HTTP API is written by hand
> more than once.**

`spec/mcpd.spec.json` is the single source of truth. The Rust model, all six
SDK model types, the OpenAPI document and the reference documentation are
**generated** from it. `make check` fails if any generated file differs from
what the spec currently produces, so drift cannot reach `main`.

Generated files carry a `GENERATED FILE. DO NOT EDIT.` banner. If you find
yourself editing one, stop: the change belongs in the spec or in the generator.

---

## Change → what else must change

Find your change in the left column and do everything in the right column.
The last step of every row is `make check`.

### You changed the data model or the HTTP API

*(added a field, added a span kind, added an endpoint, changed a default)*

1. Edit `spec/mcpd.spec.json`. Every field needs a `doc` string written for a
   reader; the validator rejects one without.
2. Run `make gen`. This rewrites:
   - `crates/mcpd-core/src/generated.rs`
   - `sdks/typescript/src/generated.ts` (camelCase, mapped at the SDK transport)
   - `sdks/python/mcpd/_generated.py`
   - `sdks/go/generated.go`
   - `sdks/kotlin/src/main/kotlin/dev/mcpd/Generated.kt`
   - `sdks/elixir/lib/mcpd/generated.ex`
   - `web/src/generated/types.ts` (wire names, used with raw `fetch`)
   - `web/src/generated/spec-index.json`
   - `spec/openapi.json`
   - `web/src/content/reference/*.md` (the reference docs)

   Never edit any of them. The drift gate in `make check` regenerates and
   compares, so an edit by hand fails CI rather than surviving.
3. Implement the behaviour behind it:
   - storage columns and queries in **every** backend, not just the one you
     tested: `crates/mcpd-store/src/sqlite/`,
     `crates/mcpd-store/src/postgres.rs` and
     `crates/mcpd-store/src/clickhouse.rs`, plus all three schema files. The
     `Store` trait will not compile until every backend implements a new
     method, which is the point of it being a trait. Compiling is not the same
     as working: Postgres and ClickHouse are Cargo features, both in
     `default`, so `cargo check` builds their code and runs none of it. Their
     tests need a real server, so run the gated suites against one. Use
     `--all-features` as well, so a feature outside `default` is built too.
   - (add a SQLite migration; never edit `schema.sql` in place once released)
   - the handler in `crates/mcpd-server/src/`, and the route in
     `crates/mcpd-server/src/lib.rs`. A handler with no route is dead code that
     tests will still pass.
4. Teach each SDK the new surface if it is user-facing. Follow
   [`sdks/SDK-CONTRACT.md`](sdks/SDK-CONTRACT.md). Behaviours **B1-B10** are
   binding in every language.
5. If the change is user-visible, update the example programs under
   `examples/<language>/`, **not** the documentation. Docs read the examples.
6. If the change is user-visible, add a line to `CHANGELOG.md` under
   `Unreleased`. Say what it means for someone using mcpd, not what you edited.
   Label it as a wire-contract change if the spec moved, because that one lands
   in six languages at once.
7. `make check`.

### You changed how an SDK behaves

1. Change `sdks/SDK-CONTRACT.md` first. It is the contract; the code follows it,
   not the other way round.
2. Apply the change in **all six** SDKs. A behaviour that holds in TypeScript
   and not in Go is worse than one that holds nowhere, because it teaches the
   reader something untrue.
3. Update `examples/<language>/quickstart.*` if the surface changed.
4. `make check`.

### You changed a code example

The documentation does **not** contain copies of code. It names a file and a
region, and `web/src/lib/snippets.ts` reads the current contents at build time.

- Regions are marked with the language's own comment syntax:
  `#region init` … `#endregion init`.
- Required regions in every example: `init`, `trace`, `score`, `query`.
- Keep them tight. A region is an excerpt for a reader, not the whole file: aim
  for **8 to 46 lines**, and keep lines under 100 characters so they do not
  overflow a documentation column.
- Deleting a region breaks the site build **on purpose**. A page that silently
  renders an empty code block teaches nothing and hides the breakage.

### You added a language SDK

1. Add it to `LANGUAGES` in `web/src/lib/site.ts`.
2. Add an emitter in `tools/codegen/`, register it in `tools/codegen/index.mjs`.
3. Create `examples/<language>/` with all four regions.
4. Add `web/src/pages/docs/sdks/<language>.astro` and an entry in
   `web/src/lib/docs.ts`.
5. Add it to the CI matrix in `.github/workflows/ci.yml`.
6. `make check`.

### You added or renamed a documentation page

1. Add it to `DOCS` in `web/src/lib/docs.ts`. That list drives the sidebar, the
   previous/next links and the index; a page missing from it is unreachable.
2. Create `web/src/pages/<path>.astro`.
3. Every page needs a `title`, a `description` and at least one motion graphic.
4. `make check` verifies that every entry in `DOCS` resolves to a real page.

### You changed the run protocol

The durable run surface is bound by `internal/run-contract-v1.md`, and every
SDK's worker speaks it. A change to a construct, a lease field, a 409 reason or
an endpoint touches all of these:

1. `spec/mcpd.spec.json` (tags Runs and Workers) and `spec/definition.schema.json`,
   then `make gen`.
2. `internal/run-contract-v1.md`: the contract is the authority, so it changes
   first or the change is a bug.
3. `crates/mcpd-runs` and its tests (`tests/contract.rs` covers every enum
   value and every 409 reason), then `crates/mcpd-server/src/runs.rs` and
   `tests/runs.rs`. The rules live once, in `src/engine.rs`; a table or a
   column goes in both `src/schema.sql` and `src/postgres_schema.sql`, and the
   second needs `SCHEMA_VERSION` in `src/postgres.rs` raised. Write each new
   test inside `backend_tests!`, so it runs on SQLite and on Postgres, and run
   it once with `MCPD_TEST_POSTGRES_URL` set.
4. All six workers: `sdks/typescript/src/{workflow,runs,worker}.ts`,
   `sdks/python/mcpd/{workflow,runs,worker}.py`, `crates/mcpd-sdk/src/{workflow,runs,worker}.rs`,
   `sdks/go/{workflow,runs,worker}.go`, `sdks/kotlin/.../{Workflow,Runs,Worker}.kt`,
   `sdks/elixir/lib/mcpd/{workflow,runs,step_worker,step_context}.ex`, each
   with its unit tests and its two live tests.
5. `sdks/SDK-CONTRACT.md` section 7, the `examples/<language>/workflow.*`
   programs the docs read, and the pages `web/src/pages/docs/{workflows,workers,runs}.astro`.
6. `tests/e2e/probes/runs/` and `tests/e2e/runs.mjs`, which serve the same run
   from every language and compare the result. `make conformance` runs it.

### You changed the daemon's flags or configuration

1. `crates/mcpd/src/main.rs` (the clap `Cli`) and
   `crates/mcpd-server/src/state.rs` (`ServerConfig`). Settings too structured
   for a flag go in the `FileConfig` struct in `main.rs` and are read from
   `--config-file`.
2. `web/src/pages/docs/configuration.astro` documents every flag; update it.
3. Add it to the `mcpd config` output in `main.rs`, so an operator can see what
   an invocation resolved to without reading the source, and add the line to the
   pre-flight block in `web/src/pages/deploy.astro`. `make check` compares the
   two: a documented pre-flight that shows output the binary no longer produces
   is worse than none, because the lines a reader is told to look for may not be
   there.
4. Update `deploy/` if the deployment surface changed.
5. `make check` compares the flags **both ways**: it fails on a documented flag
   the binary does not have, and on a flag the binary has that the page never
   mentions.

### You added a method to the `Store` trait

1. Implement it in all three backends: `crates/mcpd-store/src/sqlite/`,
   `postgres.rs` and `clickhouse.rs`. The trait will not compile until you do,
   which is the point of it being a trait.
2. **Also implement it on the `Null` fake in
   `crates/mcpd-server/src/auth.rs`.** It is a test-only `impl Store` inside a
   test module, so `cargo build` succeeds and `cargo test` fails, a long way
   from the change you made. This has cost two debugging rounds; it is the first
   place to look when tests stop compiling and the library still does.
3. Return the narrowest thing the caller needs. A method that hands back a whole
   row where the caller only needs one field becomes a way to read that row
   without a grant for it.
4. ClickHouse holds more than one row per key and merges them later, so a lookup
   that must be unique needs `FINAL` **and** a rule for what a multi-row result
   means. `FINAL` alone does not collapse rows that differ in the sort key, and
   assuming it does once meant a lookup could return the wrong tenant.

### You added an audited action

1. Emit it through `audited()` in `crates/mcpd-server/src/query.rs`, never by
   calling `record_audit` directly, so the recorded result cannot disagree with
   the response actually returned. Audit the LAST authorisation decision, not
   the first: an endpoint with two checks recorded `allowed` for a request it
   then refused.
2. The count appears in prose in two places that `make check` does not compare:
   `web/src/pages/docs/security.astro` names every action, and
   `web/src/components/AuditLedger.astro` prints the total. Update both.

### You added a storage backend

1. Implement every method of `Store` in `crates/mcpd-store/src/`. The trait is
   deliberately narrow so this is one file; keep it that way.
2. Add a schema file beside `sqlite/schema.sql` and `clickhouse_schema.sql`, and
   create it from `migrate()` so there is nothing to apply by hand.
3. Put it behind a Cargo feature in `crates/mcpd-store/Cargo.toml`, and add the
   feature to `default` only if it carries no heavy dependency.
4. Wire the flags into `main.rs`, and document them
   (see *You changed the daemon's flags*).
5. Add a CI job that runs the backend's tests against a real server as a service
   container. A storage backend tested only against a mock is not tested.
6. Update the storage section of `README.md` and the deploy page.

### You changed what mcpd measures or alerts on

*(a new `Watch`, a new built-in scorer, a new SLO burn window, a change to the
latching rule)*

1. The behaviour lives in `crates/mcpd-core/src/scoring.rs` (scorers) or
   `crates/mcpd-server/src/monitor.rs` (monitors).
2. A new scorer must appear in `Builtin::ALL`, which is what
   `GET /v1/api/scorers` enumerates and what the catalogue test counts.
3. Update the scorer table in `web/src/pages/docs/testing.astro` or the watch
   table in `web/src/pages/docs/alerting.astro`. Both name every variant; a
   table that is missing one is a table nobody can trust. If you change a burn
   threshold, change the worked numbers on the alerting page with it: they say
   what error rate those thresholds actually correspond to, which is the part a
   reader needs and the part that silently goes wrong.
4. A judged scorer is different: a new criterion is configuration, not code. It
   goes in the `judge.criteria` map of the config file and needs no release. Add
   a *behaviour* of the judge (a new decline reason, a different request shape)
   in `crates/mcpd-server/src/judge.rs`, and document it in the decline table on
   the testing page, which names every reason a judged score is not produced.
4. `make check`.

---

## Traps in this repository

Each of these cost real debugging time here. They are mechanical, they are not
obvious from the code, and none of them is caught by a compiler.

- **Never put an item after a `mod tests` block** in a Rust file. Clippy rejects
  it with "items after a test module", and it is a confusing failure because the
  item itself is fine. New functions go above the test module.
- **Run `make` from the repository root.** A `cd web` earlier in the same shell
  makes `make ui` fail silently, and you then test a stale bundle and conclude
  your change did nothing.
- **Name a test's temp directory from the pid AND a per-process stamp.** Pids
  are recycled, and a recycled one reopens the previous run's database, so every
  count is off by a whole prior suite. That produced 89 false failures in one
  run. See `crates/mcpd-server/tests/http.rs` for the shape.
- **Bind a test daemon to a port the OS assigns**, never a hardcoded one. A
  hardcoded port collided with an unrelated process and produced three
  convincing but entirely false test failures against someone else's server.
  `python3 -c "import socket;s=socket.socket();s.bind(('127.0.0.1',0));print(s.getsockname()[1]);s.close()"`
- **Do not run prettier.** There is no config, and its defaults explode the
  deliberately tabular docs nav and reformat the generated types, which breaks
  the codegen drift gate.
- **An environment-gated test suite must fail, not skip, when CI expects a
  server.** Every live suite reads `MCPD_TEST_ENDPOINT` and fails if
  `MCPD_REQUIRE_LIVE` is set without it. Six separate suites here were reporting
  success having contacted no server at all; this is the guard against a
  seventh. When you add a gated suite, add the guard in the same commit.
- **Prove an assertion can fail before you trust it.** Break the thing it
  guards, once, and watch it go red. Every silent-success defect found here
  would have been caught by that one habit, and none of them was caught by
  review.

## Commands

| Command | What it does |
| --- | --- |
| `make gen` | Regenerate everything from the spec. |
| `make check` | Everything CI runs. Run this before you finish. |
| `make test` | Rust tests plus every SDK's tests. |
| `make dev` | The daemon with demo data, plus the site, both watching. |
| `make ui` | Build the web application into the binary. |
| `make fmt` | Format the Rust code with `cargo fmt`. It formats no TypeScript and no Python. |

---

## House style

**Comments explain why, not what.** `// increment the counter` is noise. The
useful comment is the one that says what would go wrong otherwise, or why the
obvious approach was rejected. Match the density of `crates/mcpd-core/src/`.

**Prose voice**, in code comments, documentation and UI copy alike:

- Plain and precise. Short sentences. Address the reader as "you".
- No marketing language. No exclamation marks. No em-dashes: use a comma, a
  colon, or split the sentence.
- Banned words: *simply, just, easily, powerful, seamless, blazing, effortless,
  leverage, utilise*.
- British spelling: *behaviour, colour, recognise*.
- **Never invent a fact.** No performance numbers, no customers, no benchmarks,
  no adoption figures. None exist. If something is not implemented, say so.

**Errors name the problem and the recovery.** `invalid input` is a bug report
waiting to happen. `--sample must be between 0.0 and 1.0, got 4.2` is an answer.

**Never break the agent being observed.** An SDK must never throw from
instrumentation, never block on the network, and never grow without bound. This
is contract behaviour B1-B3 and it is not negotiable: an observability library
that takes down production has done more harm than no observability at all.

**A graphic must be computed from the thing it depicts.** The figures on this
site derive their frames from the same data they draw: the waterfall's bars
grow from the spans' real offsets and durations, the monitor's state is folded
out of its own readings by the same rule the daemon uses, and the sampling
figure's decisions come from the hash it prints. Every figure runs on
`animate()` in `web/src/lib/motion.ts`, which pauses it off-screen and skips it
under reduced motion; its markup is its finished state. A hand-written
timeline that merely looks right will drift from the behaviour it claims to
show, and then the documentation is lying in a way no test can catch.

**shadcn is for the interface, not the documents.** `web/components.json` and
`web/src/components/ui/` hold the primitives, built on the Radix packages in
`web/package.json` and styled in the panel system, never in shadcn's default
look. Take them where a control needs behaviour that is genuinely hard to get
right (focus, portalling, dismissal, positioning) and the page is already a
React island, which in practice means `/app`. Do NOT reach for them in the docs:
those pages render at build time, and the language switcher in particular is a
site-wide preference shared by every block on the page, not per-block tab state,
so replacing it with a component library would cost 80 React hydrations a page
and lose the behaviour that matters.

**The design law applies to every pixel.** The web surfaces follow
`DESIGN.md` and the direction contract in
`web/src/layouts/Base.astro`. Use design tokens, never raw hex, and check every
change in both the light and the dark theme. No kicker labels
above headings, no icon-in-a-tinted-tile, no pill badges for metadata, no
gradient text, no glow. Content is visible by default: nothing is ever gated on
an animation completing.
