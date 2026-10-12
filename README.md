<div align="center">

# mcpd

**The unopinionated agent observatory.**

[Hosted](https://mcpd.sh/app) ·
[Documentation](https://mcpd.sh/docs) ·
[Quickstart](https://mcpd.sh/docs/quickstart) ·
[Deploy](https://mcpd.sh/deploy) ·
[Coming from Mastra](https://mcpd.sh/docs/from-mastra) ·
[Data model](https://mcpd.sh/docs/reference/data-model)

</div>

---

mcpd watches any agent, in any framework, in any language, against any model
provider. Send it OpenTelemetry, or use one of six SDKs: TypeScript, Python,
Rust, Go, Kotlin and Elixir. It records every model call, every tool call and
every retrieval, with the exact prompt, the exact completion, the latency and
the cost attached to each.

It is a daemon, not a framework: one static Rust binary that holds a durable
workflow between its steps, the cursor, the retries, the timers and the waits,
while your own code, in any language, does the work inside each step. It records
every step as a trace with its cost, and agents that already emit OpenTelemetry
are recorded without a rewrite. It has no opinion about how you built the agent,
which model or provider it calls, or whether a framework was involved at all.

## Start

**Hosted.** The fastest start is the free hosted version at
<https://mcpd.sh/app>. Sign-up is open and every feature is free.

**Self-hosted.** Build from source and run the demo:

```shell
make build && ./target/release/mcpd demo
```

Then open <http://localhost:4317>. No account, no database to stand up,
nothing to sign.

Use `make build`, not `cargo build`. The interface is compiled into the binary
with `include_dir!` from `crates/mcpd-server/ui`, which `make ui` generates and
the repository does not track, so a plain `cargo build` on a fresh clone has no
interface to embed. The build needs Node as well as Rust.

The container image is `ghcr.io/mcpd/mcpd`:

```shell
docker run -p 4317:4317 ghcr.io/mcpd/mcpd
```

## Why this exists

Observability for agents usually arrives attached to a framework: to be seen,
your agent has to be built with it, in the language it prefers.

mcpd works the other way round. Point any OpenTelemetry SDK at it, or use one of the six native
SDKs, and the run shows up. The native SDKs are a convenience over an HTTP
surface anyone can call, not a condition of being recorded. The same binary runs
on a laptop with no configuration and as several replicas on a cluster.

It is not a gateway and not a proxy. Nothing travels through mcpd to reach a
model, so mcpd being slow or down cannot slow your agent or break it. What it
stores it also forwards, as OTLP, to any endpoint you name, which makes it one
hop in a pipeline you already have rather than the place the data has to stay.

## What it records

Six signals, all correlated by the run that produced them, so a spike on a
chart opens the trace that caused it and a trace shows the log lines it emitted.

| Signal | What it is |
| --- | --- |
| **Spans** | What ran, nested, with timings, tokens, cost, input and output |
| **Logs** | Lines the run emitted, correlated to the exact span |
| **Metrics** | Numbers, with exemplars that link back to a run |
| **Scores** | Machine evaluation, with the scale it was measured against |
| **Feedback** | What a person thought, kept separate from machine scores |
| **Aspect notes** | One sentence about a run along one aspect: the goal, the sentiment, the behaviour or the outcome |

## Instrument it

Six SDKs with one behaviour: recording a span never blocks, never throws, and
never grows without bound. If mcpd is unreachable, your agent does not notice.

Each SDK lives in this repository under `sdks/`, with these package
coordinates:

| Language | Package |
| --- | --- |
| TypeScript | `npm install @mcpd/sdk` |
| Python | `pip install mcpd` |
| Rust | `cargo add mcpd-sdk` |
| Go | `go get github.com/mcpd/mcpd/sdks/go` |
| Kotlin | `implementation("dev.mcpd:mcpd-kotlin:0.1.0")` |
| Elixir | `{:mcpd, "~> 0.1"}` |

Each one writes the
same canonical run against one live daemon, and a conformance suite reads all of
them back and asserts the traces are structurally identical, seven channels
including plain `curl`. A behaviour that holds in one language and not another
fails there and nowhere else.

```shell
make conformance
```

Or let an agent read its own traces. `mcpd mcp` speaks the Model Context
Protocol over stdio and answers three tools: which run, what happened inside it,
and whether one run is broken or the whole deployment is. It is a client of a
running daemon, so it obeys the same key scopes as everything else.

```shell
MCPD_ENDPOINT=https://mcpd.internal MCPD_API_KEY=$READ mcpd mcp
```

Already using OpenTelemetry? Skip all of that:

```shell
export OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317
export OTEL_EXPORTER_OTLP_PROTOCOL=http/json
```

mcpd maps both semantic conventions the ecosystem actually uses: OpenTelemetry's
`gen_ai.*` and Arize's OpenInference `llm.*`.

## Evaluate it

Thirteen deterministic scorers run over stored runs, never inline, so an
evaluation can never add latency to the thing it is judging. A model can grade
as well, against any OpenAI-shaped endpoint including one on your own machine.
Capture a production run that went wrong into a dataset and it becomes a
regression case every later version has to pass.

Monitors watch a threshold and latch, so a transition is reported once rather
than every cycle. Service level objectives watch the error budget and trip only
when a fast and a slow burn window agree, which is what stops a two minute blip
paging anyone.

## Running it for other people

The enterprise features are configuration on the same binary, never a separate
build and never a licence key:

- **Redaction before storage**, on by default, across all six signals. A value
  a rule matches never reaches the disk.
- **Projects as a real tenancy boundary.** A key listed under a project reaches
  that project and no other. Retention, sampling and an ingest ceiling are per
  project too, so one daemon serves several teams without any of them reading
  the others' prompts, and without one of them spending the whole daemon.
- **Six key scopes**, because emitting telemetry, reading prompts, changing
  stored data, destroying it, starting runs and claiming steps are different
  privileges. An unset write list accepts the ingest keys, and an unset admin
  list accepts the read keys. Unset execute and worker lists accept the write
  keys. Upgrading does not lock you out.
- **An audit log** of every action that hands a stored payload out or destroys
  one, recording refusals as well as successes. It stores a digest of the key
  that acted, never the key, and retention does not trim it. Give mcpd the
  header your ingress already sets, and the addresses allowed to set it, and it
  records the person too.
- **Erasure by subject**, so a request about a person is one call rather than a
  procedure, returning a count per signal for the reply you have to write.
- **Three storage backends**: embedded SQLite for a laptop, Postgres for a team
  that already runs one, ClickHouse when retention is measured in billions of
  spans. All three are tested in CI against a real server, not a mock.
- **Deterministic sampling**, so a fleet of instances agrees without
  coordinating.
- **Helm chart**, health and readiness probes, graceful shutdown on SIGTERM, and
  its own Prometheus metrics at `/metrics`.
- **`mcpd doctor`**, which grades a deployment and names the flag that fixes
  each thing it finds. A check it could not run says so rather than saying ok.

Nothing leaves the machine unless you configure it to. Self-hosted mcpd has no
telemetry, no licence check and no account. Every feature is free, whether you
run it yourself or use the hosted version at [mcpd.sh/app](https://mcpd.sh/app).

## Repository layout

| Path | What is in it |
| --- | --- |
| `spec/` | **The source of truth.** The data model and HTTP API. |
| `crates/mcpd-core` | Model, ids, cost, redaction, sampling, OTLP decoding |
| `crates/mcpd-store` | Storage: SQLite, Postgres and ClickHouse |
| `crates/mcpd-runs` | The durable run core: definitions, runs, leases, timers, waits, the journal |
| `crates/mcpd-server` | HTTP: ingest, query, runs and workers, evaluation, alerting, the interface |
| `crates/mcpd` | The daemon binary |
| `crates/mcpd-sdk` | The Rust SDK |
| `sdks/` | TypeScript, Python, Go, Kotlin and Elixir SDKs, each with a workflow builder and a worker |
| `examples/` | Runnable programs the documentation reads its samples from |
| `tests/e2e` | Cross-language conformance for traces and for durable runs, and the judge against a real model |
| `web/` | The site, the documentation and the operator interface |
| `deploy/` | Compose file, Kubernetes manifests, Helm chart |
| `tools/codegen` | Generates everything derived from the spec |

## Contributing

Read [`AGENTS.md`](AGENTS.md) first. It is short, and it has a
"change, what else must change" table that will save you a review round. Then
[`CONTRIBUTING.md`](CONTRIBUTING.md) for the toolchains and the test suites.

The short version: `spec/mcpd.spec.json` is the single source of truth, the Rust
model and all six SDK model types are generated from it, and `make check` fails
if any of them has drifted.

```shell
make dev      # daemon with demo data, plus the site, both watching
make check    # everything CI runs
```

Contributions are accepted under the
[Contributor License Agreement](CONTRIBUTOR_LICENSE_AGREEMENT.md).

| | |
| --- | --- |
| Security | [`SECURITY.md`](SECURITY.md), or hi@mcpd.sh |
| Conduct | [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md) |
| Help | [`SUPPORT.md`](SUPPORT.md) |
| Maintainers | [`MAINTAINERS.md`](MAINTAINERS.md) |

## Licence

[Apache-2.0](LICENSE). Copyright Internet Technology Services LLC.
