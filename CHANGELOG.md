# Changelog

Notable changes to mcpd, newest first. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Two things are versioned separately and both appear here, labelled, because a
change can break one without touching the other:

- **The daemon**, `mcpd` itself.
- **The wire contract**, meaning `spec/mcpd.spec.json` and therefore the HTTP
  API and every generated SDK type. A change here is a change in six languages
  at once.

Each GitHub release also carries generated notes listing the merged pull
requests. This file is the curated view: what changed for someone using mcpd,
rather than every commit.

## [Unreleased]

### Added

- **`--skip-email-verification`.** Sign-up opens the account at once, with no
  confirmation mail, for a daemon without a mail server.
- **Prompt management.** Store prompts as numbered versions, text or chat
  messages with `{{variable}}` placeholders and model settings, and point
  labels such as `production` and `staging` at a version. Releasing or rolling
  back a prompt is moving a label; no deploy is needed. An identical body is
  not stored twice. The span list filters on the prompt a step used
  (`prompt`, `prompt_version`), and the new Prompts screen shows each
  version, what changed from the one before, its labels, and the steps that
  ran it. Prompts are stored on SQLite, Postgres and ClickHouse. Wire-contract
  change: the `Prompt` entity and six `/v1/api/prompts` endpoints.
- **More trace filters and list previews.** `GET /v1/api/traces` also filters
  on `max_duration_ms`, `max_cost`, `min_tokens`, `score_above` (with
  `has_score`, and between two bounds with `score_below`) and `has_feedback`.
  With `preview=true` each listed trace carries `input_preview` and
  `output_preview`, the start of the root step's input and output as plain
  text, and the run list shows them as Input and Output columns. A list read
  with previews is recorded in the audit log as `list_traces`. Wire-contract
  change: two `Trace` fields and six query parameters.
- **Live scoring, scorer definitions and judge runs.** Evaluation rules
  (`/v1/api/evaluation-rules`) score runs as they finish, in the background,
  with the scorers they name, on a sampled share of the runs they match, with
  a cap on model calls a minute; their scores have source `live`. Projects
  store their own scorers (`/v1/api/scorer-definitions`): judge prompts and
  code scorers with fixed settings, versioned, named as `id` or `id@3`,
  with the version on every score. Every judged score now names the run of
  its own model call in `score_trace_id`, a `scorer_run` span with the
  prompt, the reply, the tokens and the cost. The Evaluations screen manages
  rules and definitions. Wire-contract change: two entities, six endpoints,
  the `custom` scorer kind and a `project` parameter on
  `GET /v1/api/scorers`.
- **Trajectories and experiments case by case.** `GET /v1/api/traces/{id}/trajectory`
  reads a run as the tool calls, model generations and handoffs its agent
  took, and the `trajectory` scorer marks that path against
  `expected_tools` (`strict`, `ordered` or `unordered`) and
  `max_steps`. `POST /v1/api/experiments/{id}/score` scores every run of an
  experiment against the expected answer of the dataset case it names in
  `mcpd.dataset_item_id`, and `GET /v1/api/experiments/{id}/results` reads
  it back case by case with a summary per scorer. The trace list filters on
  `experiment_id`. Wire-contract change: four types, three endpoints, two
  `ScorerConfig` fields.
- **A review queue, frozen dataset versions and tail sampling.** Queue a run
  for a person with `POST /v1/api/review-queue`, assign it and mark it
  reviewed or dismissed; every change is kept. The Review screen works
  through the queue, and the run panel queues a run. `POST
  /v1/api/datasets/{id}/versions` freezes a dataset's cases into a numbered
  version that `GET /v1/api/datasets/{id}/items?version=` reads back. A
  project's `tail_sampling` block in the config file keeps finished runs
  that failed, were slow, cost too much, scored low or got feedback, keeps
  the rest at a rate, and deletes what it drops. Wire-contract change: two
  entities, four types and five endpoints.
- **Experiments from code in every SDK.** `run_experiment` runs your own
  function over every case of a dataset, a few at a time, each case in a run
  that names the experiment and the case; a case that raises is recorded as a
  failed run and the rest carry on. It then scores the answers against each
  case's expected answer, marks the experiment `succeeded` with a mean per
  scorer, and returns the results case by case. The SDKs also read
  trajectories, dataset cases and experiment results, write experiments, and
  filter the trace list on `experiment_id` with optional previews.
- **Streamed model calls and a test client in every SDK.** Wrap a provider's
  OpenAI- or Anthropic-shaped stream with `record_stream` and read the
  wrapper: every chunk passes through unchanged, and the span gets the time to
  first token, the text, tool calls, usage, response id and finish reason when
  the stream ends, fails or is left early. A test client (`TestClient`,
  `mcpdtest`, `Mcpd.Testing`) records into memory through the same pipeline as
  a real client, so your unit tests read back spans, trees, logs, scores and
  feedback with no daemon.
- **Durable runs in Postgres.** `--runs-data postgres://...` keeps the
  definitions, the step journal, deadlines and waits in Postgres instead of an
  embedded file, so several replicas serve runs from one store. It can be the
  same database as `--postgres-url`, and reads TLS settings the same way. Every
  run store transaction is serializable, so replicas claiming steps or firing
  deadlines at once behave as one daemon would. The schema is created on start.
  `mcpd config` prints the line as `postgres at` the server, without the
  password. In the Helm chart, set `runs.store=postgres` and
  `runs.postgresUrl`, and `replicaCount` above 1 is accepted with runs on.

- **Organisations, roles, single sign-on and SCIM.** An account can create
  organisations, which own projects, invite members by email and give each a
  role (owner, admin, member or viewer, optionally replaced on one project)
  that maps onto the six key scopes. API keys can belong to any project of an
  organisation, and stop working when their maker's role stops allowing them.
  Organisations log in with any OpenID Connect provider or GitHub, can claim
  email domains and enforce single sign-on for them, and can provision users
  and groups over SCIM 2.0, with groups mapped onto roles. Membership, role,
  project, single sign-on and SCIM changes are written to the audit log. See
  the new accounts page.
- **`--accounts`** turns all of that on without public sign-up, so a
  self-hosted deployment gets single sign-on and roles on its own cluster.
  Organisations, their owners, projects, single sign-on and SCIM token can be
  declared in the `accounts` block of the config file. Configured keys keep
  working.
- **The account lifecycle.** Address confirmation, password reset, password and
  address changes, session list and revoke, two-step log-in with recovery
  codes, and account deletion, which erases the projects of organisations the
  account was the only member of. Mail goes through `--smtp-host`,
  `--smtp-port`, `--smtp-user`, `--smtp-password`, `--smtp-from` and
  `--smtp-tls`; links start at `--public-url`.
- **Per-organisation quotas.** An organisation's projects share one ceiling on
  signals per minute, `--org-quota-per-minute` (60000 with `--signup`), and a
  retention default, `--org-retention-days`. Usage is shown in the interface.
- **A themes screen in the interface.** Pick an agent, workflow or tool and
  read how far each aspect has got towards a snapshot, the themes the latest
  pass found with their share and how each moved over recent snapshots, and the
  flow of runs across two to four aspects. A theme opens its history and the
  notes behind it, each linked to its run, and the run panel lists the aspect
  notes written about that run. `mcpd demo` now seeds notes and four snapshots
  so the screen has something to show.

### Changed

- **Wire contract: sign-up answers the same way whether or not the address is
  in use.** `POST /v1/api/account/signup` returns `AccountNotice` and mails a
  link; `POST /v1/api/account/verify` confirms the address and returns the
  session. It no longer answers 409. `Account.project` and
  `AccountSession.project` are optional, `AccountKey` carries `project`, and
  `HealthStatus` carries `accounts` and `sso`. The new endpoints are tagged
  Accounts, Organisations, SSO and SCIM.

### Security

- **A trace id two projects share is a run in each.** A trace id crosses
  service boundaries by design, so a service in one project that calls a
  service in another exports spans under the caller's id. Every backend folded
  those spans into one run, owned by whichever project wrote the earliest span.
  That project's key could then read the other project's logs, scores and
  feedback through the run, on Postgres its span inputs and outputs as well,
  and delete them. A key that learned another project's trace id could cause
  the same thing with one span. Each project now holds its own run under the
  id, built from its own spans, and every read and delete of a run names the
  project.

  SQLite and Postgres databases are rebuilt on the first start of this version
  (schema version 5). Only the run list is rebuilt, from the spans, which are
  not touched, and it is one statement, so a large store does not hold the start
  past its probe. On ClickHouse a run that two projects share is corrected when
  it next receives a span.

  **Upgrading Postgres:** stop every replica of the previous build before the
  first replica of this one starts. A replica of the previous build cannot
  write runs once the new key is in place, and going back to it needs the
  database restored from before the upgrade.

- **An id another project holds is refused.** A sender chooses the id of every
  span, log line, metric point, score, piece of feedback and aspect note, and a
  span id travels to every downstream service in `traceparent`. On SQLite a
  record that reused another project's id replaced that project's row, and on
  Postgres it changed it. Both now refuse the record, count it as rejected and
  name the id in the response. On ClickHouse the record is stored apart in the
  sender's project, and the other project's row is not touched. An experiment
  id or a dataset item id that another project or dataset holds is refused with
  409 `id_taken`.

- **A note took its agent from another project's run on ClickHouse**, when the
  two projects shared a trace id, which put one project's agent name on the
  other's entity list.

- **An empty key no longer opens a scope.** An empty environment variable,
  such as `MCPD_EXECUTE_KEYS=` in a compose file, made a key list hold one
  blank entry, and a request with an empty `x-mcpd-key` header then matched it.
  A key list that is set but holds no key, in a flag, a variable or a project,
  now stops the daemon at start and names the flag and variable; a blank entry
  beside a real key, such as a trailing comma, is dropped; and an empty header
  counts as no key.
- **mcpd refuses to start with a placeholder key** from the example files, in
  any scope, in any project, or as `--redact-key`. The raw Kubernetes Secret
  is now `deploy/k8s/secret.yaml.example`; create the Secret with generated
  keys, as the deploy page shows.
- **mcpd warns when the ingest keys also start runs and claim steps**, which
  they do when no write, execute or worker key is set. `mcpd doctor` reports it
  too. Set `--execute-key` and `--worker-key` to separate them.
- **The audit log records every read of a stored payload, and only reads that
  were served.** A request answered with 400 no longer leaves a row saying it
  was allowed. Reading, listing, following, explaining or cancelling a durable
  run is recorded under the run's project, and so are the aspect notes of a
  trace (every answer, refusals included), a theme, the theme flow and a
  theme's history, because a theme's label is the text of a note. Eighteen
  actions are recorded. A refused key on a run id that does not exist is
  recorded too, and on the embedded store the row is written even when ingest
  has filled the write queue.
- **A monitor or objective returns only the host of its webhook**, because a
  webhook URL is often its own credential. `mcpd config`, the start-up log,
  `mcpd doctor` and the forwarding log lines no longer print a password, a
  token in a query, or a Postgres `key=value` password, including a password
  that holds `/`, `?`, `#` or `@`. A failed outbound request, to a webhook, a
  judge or a forwarding destination, is logged without its URL.
- **The OTLP doors check the ingest key before they read the request body**, so
  a client with no key can no longer make the daemon buffer a full body.
- **The durable run store is now inside erasure, retention and the audit
  log.** `DELETE /v1/api/subjects/{user_id}` erases ended runs started for that
  `user_id`, with everything stored for them, and reports `runs_erased` and
  `runs_in_flight`; a run still going is counted and named, never cut short, so
  you can cancel it and send the request again. Retention deletes run trees
  that ended before each project's cutoff. The run store keeps run input,
  output and state as the workers sent them, without redaction, because a later
  step may need a value; the copy of each run in the trace store is redacted.
  The documentation now says so. Erasing a person waits for a span drain in
  progress, so a span queued before the erasure cannot land after it, and it
  works through run trees a batch at a time, so leases and readiness do not
  stall behind a large one.

### Fixed

- **The derived `outcome` aspect reaches a snapshot.** Its notes were stamped
  with the time of the pass that wrote them, which put them outside the window
  that pass reads, and each later pass moved them again. They are now stamped
  with the time the run ended.
- **A page of aspect notes or snapshots on ClickHouse could repeat the last
  row of the page before.** The cursor's timestamp went through a float and
  rounded by a few hundred nanoseconds. It is now exact.
- **Postgres counts runs for a run dimension.** A breakdown by name,
  environment, status, session, user or service counted steps on Postgres and
  runs on the other two backends, so the same data reported three times as many
  production runs there. A breakdown by model, provider or entity no longer
  carries a `(none)` group holding every step that names no model. A run still
  going reports the time it has taken so far, as it does on the other two.
- **A search on ClickHouse folds case across Unicode**, so `CAFÉ` finds `Café`
  there as it does on SQLite and Postgres. `GET /v1/api/capabilities` reports
  `unicode_case_folding: true` on every backend.
- **Postgres over TLS.** `sslmode=require` in `--postgres-url` now connects over
  TLS and verifies the server certificate, against the public roots or against
  the authority in `sslrootcert=/path/to/ca.pem`. `verify-ca` and `verify-full`
  are read as `require`. Before, the daemon could not connect to a server that
  accepts only TLS. Every other option in the connection string now reaches the
  connection, where five were copied and the rest dropped, and a failed
  connection names what the server said and the setting that fixes the two
  refusals met most often. The connection string no longer appears in a
  start-up error, because it carries the password, and an escaped character in
  another query value, such as `%26` in a password, is passed on as written.
- **Readiness on SQLite reads the data file**, where it answered `SELECT 1`
  without reading anything, and fails when one write has held the database for
  more than 30 seconds, naming how long. A data file that cannot use
  write-ahead logging, on a network mount for instance, is named in a warning
  at start, because reads and writes then wait for each other.
- **Readiness also reads the run store**, within two seconds, so a replica that
  cannot serve runs and workers leaves service instead of failing every call.
- **SQLite retention in one project no longer removes another project's search
  rows** for a span with the same id.
- **SQLite refuses a write with 503 and `Retry-After: 1` when 64 are already
  waiting**, rather than holding every waiting batch in memory behind a slow
  writer. The SDKs already retry a 503.

- **A panic no longer stops the daemon.** Release builds unwind instead of
  aborting. A panic in a request handler answers 500 in the usual JSON shape,
  and a panic in one pass of the monitor, aspect, retention, forwarding, run
  deadline or span drain work is logged with the loop's name and the next pass
  runs.
- **Inputs that stopped the daemon are refused instead:** a model id with a
  multi-byte character near its end; a run whose output trips the
  `no_leaked_secret` scorer with a multi-byte character near the credential; a relative time such as `from=-9999999d`
  (a 400 naming the parameter and the range, at most 36500 days); a run
  definition, or a worker's `retry_after_ms`, with a duration over 3650 days (a
  400 naming the field).
- **Settings that deleted everything or stopped a loop are refused at start:**
  `--retention-days 0` or a project `retention_days` of 0, which deleted every
  signal (omit the setting to keep everything; the maximum is 36500); an SLO
  `objective` that is not at least 0 and below 1; an SLO `window_days` outside 1
  to 365; `monitor_interval_seconds` over 86400; `aspects.window_seconds`
  outside 1 to 31536000 and `aspects.interval_seconds` over 604800; a `runs`
  duration in the config file over 3650 days; and a `--forward-header` with no
  colon, which was dropped without a word.
- **Forwarding keeps at most 64 requests' copies waiting** on a destination
  that does not answer. Past that, a copy is dropped and counted in
  `mcpd_forward_dropped_spans_total`.
- **Judged scoring keeps what it paid for.** Each judged score is stored as soon
  as the model answers, so a request cut short keeps the scores it finished. A
  request that names more runs than fit in `--request-timeout-seconds` is
  refused with the cap before any model is called; one that names no runs and
  sets no `limit` grades the newest runs that fit. The new `judge.timeout_seconds`
  (default 30, from 1 to 300) bounds each call, and each call is also held to
  the time left in the request, so a slow judge produces declines inside the
  deadline rather than a 503. A reply over 1 MiB is declined.
- **An idle worker no longer spins.** A claim that leased nothing woke its own
  long-poll, so an idle worker ran claim transactions without stopping. A claim
  now answers within four fifths of the request timeout rather than with a 503.
- **SIGTERM ends open long-polls at once**, and shutdown waits, within its 5
  second bound, for the monitor and aspect loops and flushes queued run spans.
- **`mcpd config` prints every flag and config file key**, with the value the
  daemon actually runs: the config file, the log format, per-project keys
  (counted), retention and sampling, run settings, derived metrics, aspects,
  forwarding headers by name, and the judge timeout. `make check` now compares
  every `mcpd config` cast on the deploy and install pages, in every language,
  with the binary, and fails in CI when there is no binary to compare.


- **The SDKs send what the daemon reads.** `list_feedback` takes a required
  trace id, an optional limit and the project in every SDK; before, five of
  them sent parameters the daemon refuses with a 400. List filters such as
  `tags` go out as one value joined by commas in TypeScript and Python, where
  only the last tag applied. A metric filter works in Go, Python and Elixir,
  and TypeScript, Kotlin and Rust gain one. TypeScript metric and analytics
  queries no longer carry `limit` or `cursor`.
- **An SDK never loses a batch to one value.** A NaN or an infinity, a cycle, a
  `BigInt`, a set, a `Decimal`, bytes or an Elixir tuple used to make the whole
  batch fail to encode. A payload value that JSON cannot carry now becomes a
  string that names it, and a metric or score that carries one is dropped on
  its own.
- **`flush` and `shutdown` return within 10 seconds by default in every SDK**,
  even when the daemon never answers; one deadline covers a shutdown. In
  TypeScript `flush()` resolves to whether everything was sent, Rust adds
  `flush_within` and `shutdown_within`, Kotlin's `flush(timeout)` returns a
  Boolean, and Elixir answers `{:error, :timeout}`.
- **A step that passes its deadline, or is fenced, frees its worker slot** in
  every SDK, even when its body ignores cancellation. A worker with no
  `on_error` logs each kind of failure once per interval rather than once per
  step.
- **Bounded memory and no stalls in the SDKs.** The TypeScript export queue
  keeps at most one pump waiting behind a slow export, and its heartbeats no
  longer overlap. The Elixir worker bounds its mailbox and its flush waiters,
  and sends through an HTTP profile of its own, so a read no longer waits
  behind a worker's long poll and times out. Go no longer panics on an error
  interface that holds a nil pointer. Rust refuses a zero flush interval, batch
  size, queue size or timeout, which spun its export loop.

### Changed

- **Wire contract.** The metric `filter` parameter is a string of `key:value`
  pairs joined by commas, which is what the daemon has always read; the spec
  typed it as JSON, so Go and Python sent JSON and matched nothing. Array query
  parameters in `openapi.json` are `style: form, explode: false`, one value
  joined by commas, so a generated client sends what the daemon reads.
- **Wire contract.** `ErasureResult` now describes every field the erasure
  reply carries: `runs_erased` and `runs_in_flight` are new, and
  `aspect_notes_erased`, `dataset_items_erased`, `datasets`, `deletes_pending`,
  `unmatchable_identifiers` and `note` were sent before and are now in the spec
  and every SDK type. `ScoreRunRequest.limit` has no default in the spec any
  more, so a judged pass can tell "unset" from 100.
  `GET /v1/api/traces/{trace_id}/aspects` takes an optional `project`. A run is
  keyed by project and trace id in the data model. The run definition schema
  states the 3650 day maximum on every duration.
- **The Helm chart starts with an external store.** With `runs.enabled`, the
  default, the run store stays on a volume and the chart runs one replica; set
  `runs.enabled=false` to run more. `externalStore.type` picks the backend, so
  a URL kept only in your own Secret works. `replicaCount` defaults to 1, and
  the chart refuses a count it cannot serve instead of pinning it in silence.
- **The chart and the raw manifests pass Pod Security `restricted`.** They run
  as uid 10001, the image's own user, with a `RuntimeDefault` seccomp profile
  and a 45 second grace period. The chart derives its ports from
  `config.listen` and leaves out a retention of 0, which means keep everything.
- **Compose no longer passes empty execute and worker keys.**
- **The Elixir SDK needs Elixir 1.17 or newer on OTP 27**, and the build says
  so on an older OTP, because it uses the `:json` module.

### Added

- **OTLP over gRPC, on the same port** (wire contract). The trace, logs and
  metrics collector services are served beside the HTTP doors, so an
  OpenTelemetry exporter set to `grpc` needs only the daemon's address. The
  port takes cleartext HTTP/2 beside HTTP/1.1. The key goes in the call's
  metadata, gzip is read, and every call takes the same road as the matching
  HTTP door: key check, an account key's project, ceilings, sampling,
  redaction and resend recognition. Refusals end with the gRPC status an
  exporter acts on.
- **Protobuf on `/v1/logs` and `/v1/metrics`.** All three OTLP/HTTP doors now
  take protobuf as well as JSON, so an unmodified OpenTelemetry SDK sends every
  signal with no configuration. A protobuf export is answered in protobuf, and
  `partialSuccess` carries the reason when something was refused.
- **Zipkin and Jaeger ingest** (wire contract). `POST /api/v2/spans` takes
  Zipkin v2 spans in JSON or proto3; `POST /api/traces` takes a Jaeger batch in
  Thrift binary; `jaeger.api_v2.CollectorService/PostSpans` takes the same over
  gRPC. Services, tags, annotations and logs, span kinds, errors and
  references map onto mcpd's model, and the semantic conventions in tags are
  read as on OTLP. A Jaeger reference that is not the parent becomes a span
  link, so links are kept from every door. A Zipkin or Jaeger reporter needs
  only its endpoint pointed at the daemon.
- **Forward to several destinations at once, by vendor.** The config file's
  `forward` block now takes a list. Each destination names a vendor (Langfuse,
  LangSmith, Braintrust, Arize AX, Phoenix, Datadog LLM Observability,
  Honeycomb, New Relic, Grafana Cloud, Sentry, Weave, Laminar, Helicone, Opik,
  Elastic, Dynatrace, or any OTLP receiver over HTTP or gRPC) and the key,
  region and project that vendor needs; mcpd fills in the endpoint, headers and
  attribute conventions. Each has its own filter (projects, environments, span
  kinds, sample), its own redaction and its own signals. Credentials can be read
  from the environment as `${NAME}`. The old single-object form still works.
  New flags: `--forward-vendor` and `--forward-protocol`.
- **Links, events, logs and metrics are forwarded.** Span links and events used
  to be dropped by the forwarder, and only spans were sent.
- **Forwarding survives outages and restarts.** Each destination has a bounded
  spool on disk (`--spool-dir`, `--spool-max-bytes`) and retries with backoff,
  jitter and `Retry-After` until a batch is accepted, refused for good or a day
  old; refused batches are kept in `dead/`. `/metrics` reports deliveries,
  retries, dead letters, spool depth and health per destination, and
  `mcpd doctor` checks each one and reads its spool.
- **`mcpd relay`, an edge collector.** It accepts every HTTP ingest door,
  spools each request to disk before answering, and ships to the daemons named
  by `--upstream` in order, failing over to the next and back. `/readyz`
  follows the spool and the upstreams, and a full spool answers 503 with
  `Retry-After`. The Helm chart gains `mode: relay` and multi-region examples.
- **`Idempotency-Key` on every ingest door (wire-contract change).** A request
  repeated under the same key is stored once and reported as a duplicate. The
  relay sends one with every request, so a resend is never counted twice.

- **A live tail over server-sent events.** `GET /v1/api/deltas` with
  `Accept: text/event-stream` stays open and sends each batch of new runs,
  steps or log lines as a `delta` event whose id is the cursor to reconnect
  with.
- **Credentials found by their shape, not only their key.** Every string
  value is searched for private keys, JSON Web Tokens, AWS access keys,
  provider API keys, GitHub, Slack and Google tokens, bearer tokens and
  passwords in URLs, and each is replaced in place before storage. Email
  addresses are left alone.
- **Tokens and cost by model on `/metrics`.** `mcpd_model_input_tokens_total`,
  `mcpd_model_output_tokens_total` and `mcpd_model_cost_total`, labelled by
  provider and model, so a Prometheus alert can watch spend per model. At
  most 200 series; the rest are counted under `other`.
- **Log lines keep their whole correlation, and the log list filters on it**
  (wire contract). Service version, thread, resource, run, request,
  experiment and release are stored with every log line on all three
  backends, and `GET /v1/api/logs` filters on environment, service, service
  version, session, thread, user, run, request, experiment and release.
  SQLite and Postgres databases gain the columns on the first start of this
  version (schema version 6) with no row rewritten; lines stored before it
  carry none of the new fields.
- **Sort runs by tokens.** `sort=tokens` on `GET /v1/api/traces` orders runs
  by input and output tokens together, on all three backends, and the token
  columns in the run list sort by it.
- **A project's feedback, listed and filtered** (wire contract).
  `GET /v1/api/feedback` no longer needs a `trace_id`: it lists the whole
  project newest first, narrows by run, step, kind, author kind and window,
  and pages with a cursor, on all three backends. The SDKs' feedback filters
  now reach it.
- **Model calls recorded from the provider's response, in all six SDKs.**
  `record_response` reads an OpenAI or Anthropic response and records the
  model, provider, token usage with cache and reasoning meters, response id,
  finish reasons and service tier, so a batch or flex call is priced at its
  discount. `usage_from_openai` and `usage_from_anthropic` do the mapping
  alone. `mark_first_token` records time to first token and `set_guardrail`
  a guardrail's outcome. Elixir offers them as `Mcpd.ModelCall`.
- **Redaction in the process, in all six SDKs.** A `redact` client option is
  applied to span payloads, attributes, metadata and log fields before they
  are queued. A redactor that fails drops the field and is counted in
  `redaction_failures`.
- **A `handoff` span kind** (wire contract), for one agent passing control to
  another.
- **Four more screens in the interface.** Evaluations lists every scorer the
  daemon offers, the recent scores with their reasons, and scores the newest
  runs on demand, as a dry run or stored. Datasets shows each dataset's cases
  and compares two experiments with intervals and a regression reading per
  scorer. Metrics draws a histogram's merged buckets with p50 to p99 per label,
  and the stored points behind it. Monitors shows every monitor and objective,
  its latched condition and last reading, with acknowledge beside a tripped
  one.
- **Judge a run and keep it as a case from its header.** The run panel's
  judge control records a reviewer's thumbs up or down with a comment, and
  save to dataset captures the run into a dataset, optionally with its answer
  as the expected one. A prompt or completion in the step inspector expands
  to the height of the window.
- **Span links in the step inspector.** A step's links are listed with their
  kind; one inside the run opens the linked step, one into another run opens
  that run.
- **Histograms read back as distributions** (wire contract).
  `GET /v1/api/metrics/histogram` adds up the buckets of every point in a
  window, per label combination, and reads p50, p90, p95 and p99 from them.
  Cumulative streams count only their growth. `GET /v1/api/metrics/points`
  lists the stored points as they were sent. Both work on all three backends.
- **OTLP metric attributes are labels you can group and filter on.** They were
  stored as OpenTelemetry's key and value list, which `group_by` and `filter`
  could not reach. Points sent before this version keep the old shape.
- **Nine shipped judge rubrics and nine more code scorers** (wire contract).
  With a `judge` block configured, `faithfulness`, `hallucination`,
  `answer_relevance`, `answer_correctness`, `context_precision`,
  `context_recall`, `context_relevance`, `toxicity` and `bias` run with no
  criteria written. Each runs in steps: the run's question, answer, expected
  answer and retrieved context are gathered in code, the model gives itemised
  yes, no or unsure verdicts, and the score is counted from them. The verdicts,
  the token counts and the priced cost of the call are kept on the score, and a
  busy judge endpoint is retried. The new code scorers are `excludes`,
  `keyword_coverage`, `similarity`, `matches_pattern`, `tool_call_accuracy`,
  `tool_order`, `tool_not_called`, `answer_length` and `json_schema`, set
  through new `ScorerConfig` fields. `GET /v1/api/scorers` now lists each
  scorer's kind, direction, settings and availability, configured criteria
  included.
- **`mcpd eval` gates a deploy on an experiment comparison.** It asks a
  running daemon to compare two experiments, prints every scorer with its
  interval, and exits 1 on a regression and 2 when it cannot compare.
- **Experiment comparison with confidence intervals** (wire contract). Each
  scorer's delta carries a 95% interval and a p-value from Welch's t-test, and
  `regressed` follows the scorer's direction, so a fall in `toxicity` reads as
  an improvement. A shift inside the noise is no longer a regression.
- **Prices for every major provider, and your own price book.** The built-in
  book now holds list prices for OpenAI, Anthropic, Google, Mistral, xAI,
  DeepSeek, Cohere, Perplexity, Moonshot, Z.AI, Alibaba and MiniMax, with
  tables for Bedrock, Vertex AI, Azure, Groq, Together, Fireworks, DeepInfra,
  Cerebras, OpenRouter and Vercel. Long prompts are priced at the provider's
  long-context rate, and batch and flex calls at their discount, read from
  `gen_ai.response.service_tier` and its neighbours. `--price-book` (or
  `price_book` in the config file) loads a book of your own, alone or over the
  built-in one with `"extends": "builtin"`. `mcpd prices refresh` writes a book
  from a public model catalogue, and `mcpd prices show` prints what a model is
  priced at. `mcpd config` prints a `prices` line.
- **Span ingest prices with the provider.** A span that names its provider is
  priced from that provider's table, so a Bedrock or Azure call is billed at
  the cloud's rate rather than the vendor's. Vertex `@version` tags, Bedrock
  `-v1:0` suffixes and dotted versions such as `claude-sonnet-4.5` now resolve.
- **Open sign-up, with `--signup`** (wire contract). Anyone may create an
  account with an email and a password. Each account owns one new project and
  mints its own API keys for it, one scope each; nothing it holds reaches
  another project, and a signal an account key sends is stored in the
  account's project, and a read that names no project reads the account's
  own. Seven endpoints under `/v1/api/account`, a `session`
  value for an endpoint's `auth`, the `KeyScope` enum, and `signup` on
  `HealthStatus`, present only when the mode is on. With `--signup`, a request
  with no key is refused even where no key is configured. Accounts live in
  their own SQLite file, `--accounts-data`. `--client-ip-header` names the
  header a trusted proxy puts the client's address in, so the sign-up and
  log-in limits count clients. The interface shows sign-up and log-in screens
  and an Account panel when the daemon offers them, and its default endpoint
  can be set at build time with `PUBLIC_MCPD_ENDPOINT`. Without the flag
  nothing changes.
- **The chart can mount the daemon's config file** from a ConfigMap or a
  Secret, and takes `extraEnv`, `extraVolumes` and `extraVolumeMounts`. It
  refuses a ConfigMap that would hold a key or a credential.
- **Each release builds the image for linux/amd64 and linux/arm64** with an
  SBOM and provenance, publishes the Helm chart at
  `oci://ghcr.io/mcpd/charts/mcpd` and tags the Go module `sdks/go/vX.Y.Z`. The
  release fails until the image and the chart pull without a login. No SDK is
  published to a package registry yet.
- **The TypeScript package builds when it is packed** and carries CommonJS
  types, and the Kotlin build makes the sources jar, javadoc jar and POM that
  Maven Central asks for, signed when a key is given.

## [0.1.0] - 2026-09-13

The first tagged release. The wire contract should be treated as unstable
until 1.0: a field can still be renamed between minor versions, and the
changelog will say so under a **wire contract** label when one is.

### Added

- **Aspect notes, and the themes the daemon builds from them.** An **aspect** is
  an axis a run is described along, an **aspect note** is one sentence recorded
  against one run along that axis, and a **theme** is a cluster of notes that
  share their wording. Four aspects ship: `goal`, `sentiment`, `behaviour` and
  `outcome`. A note arrives through the ordinary ingest door beside the run's
  other signals, so it passes through the same redactor, retention sweeps it,
  and erasing a subject erases it and empties any theme label taken from one of
  their runs.

  There is no model in the daemon and no embedding anywhere. A theme groups
  notes by their wording: the daemon keeps a 64-integer sketch of each sentence
  and compares them with integer arithmetic alone, so two replicas of one
  deployment cannot disagree about where a cluster boundary falls. The cost of
  that is worth knowing, because it is the reason it needs nothing configured:
  two notes that mean the same thing in different words land in separate themes,
  and a theme's label is the note most like the others rather than a summary
  written for the screen.

  A pass runs on a timer and writes a snapshot; every read is a lookup of what
  it wrote, so refreshing a screen shows the same numbers rather than a fresh
  clustering. It is off until you add an `aspects` block to the config file:
  until then the daemon spawns no timer, takes no connection for it and writes
  none of its four tables, and `pass_armed` on the catalogue says so, because an
  operator should not have to infer it from a readiness that never moves. Switching it on needs no model and no credential,
  because `outcome` is derived: the daemon writes those notes itself from the
  run's status, the error class of the step that failed, the silent producer
  flag on the cause report and the structural divergence watch.

- **Wire contract.** Nine `Intelligence` endpoints, all taking the `read` scope.
  `GET /v1/api/aspects` is the catalogue, the bounds a pass runs under and
  whether one is armed at all;
  `/v1/api/aspects/entities` and `/v1/api/aspects/progress` are the honest empty
  state, which separates "nothing is wired" from "notes are arriving and the
  floor of fifty has not been reached"; `/v1/api/traces/{trace_id}/aspects` is
  what was written about one run; and `/v1/api/themes/snapshots`, `/flow`,
  `/{theme_id}`, `/{theme_id}/examples` and `/{theme_id}/history` read the
  clustering. Eight of the nine refuse with 501 on a storage backend that does
  not hold the tables, naming `theme_analysis`, which
  `GET /v1/api/capabilities` now publishes; the catalogue answers everywhere,
  because what the four aspects are is a fact about the daemon and not about its
  storage. Windows overlap, so two snapshots' counts must never be added
  together.

- **The daemon checks its own configuration before it binds.** `mcpd doctor`
  found these mistakes and nothing ran it, so the deployment carrying one was
  usually the deployment nobody had pointed it at. A `judge` block with no
  criteria is the clearest case: a request that names a criterion is refused
  with a 400 as an unknown scorer, so no judged score is ever produced, and the
  daemon serving it answers its health check and reads as well from every
  angle. The four groups that need no store
  and no network now run on every start, which is `keys`, `identity`, `limits`
  and `judge`; the other eight stay in the command, because each reads the store
  or contacts the forward endpoint. A fault stops the daemon and names the flag
  or the config key that fixes it, a warning is logged and it carries on, and
  the line between the two is whether the configuration asks for something that
  cannot work. The open scopes are the exception: they are the shipped
  default, and the daemon warns about them at startup rather than refusing. `mcpd
  config` and `mcpd doctor` still run against a configuration a start refuses,
  because they are the two commands that exist to explain one.

- **Wire contract.** `GET /v1/api/runs/{run_id}/cause?step=<key>` explains why one
  step of a run failed. It walks backwards along the data dependencies the run's
  pinned definition declares, because a template such as
  `${steps.classify.output.kind}` is an explicit edge from one step to another,
  and returns the ancestors whose output that step consumed with the template
  path each value flowed through. The field worth reading is `silent_producer`:
  true when that ancestor ended `succeeded` and every step between it and the
  failure did too. That is the case a status code cannot report, and it is the
  common one, because a step that returns valid output that is semantically
  wrong is green. `value_kind` and `value_empty` say what shape flowed through
  the path without returning the value, so a green step that handed over an
  empty list is visible. The answer is built from the journal and the
  definition, so it is the same with span payload capture off, and no model,
  prompt or completion is involved. The step key is a query parameter because a
  key may hold a slash.
- **Daemon.** A new monitor watch, `step_count_divergence`, reports runs that
  took an unusual number of steps for their own definition version, with no
  threshold in steps for anyone to choose. The hardest runaway to catch is the
  one that stays under every ceiling: a run that loops eight times instead of
  twice, finishes, costs four times what it should and reports no error. For
  each definition version the watch takes the step counts of the runs completed
  in its window and flags a run whose modified z-score,
  `0.6745 x (steps - median) / MAD`, passes `z_bound`, which defaults to the
  conventional 3.5. The median absolute deviation is used rather than a standard
  deviation because it survives the thing being looked for: one run forty times
  the usual length moves a mean and inflates a standard deviation enough to hide
  itself. A version needs twelve completed runs before anything is compared
  against it, and one whose runs have never varied at all reports the departure
  in steps rather than dividing by zero. It reads the durable run store, so a
  daemon serving no runs reports it as insufficient and says so. The measured
  value is the number of runs that diverged, so `threshold` counts runs and not
  steps.
- **Daemon.** The `run_repetition` monitor watch is implemented. It was in the
  wire contract, in all six SDK types and in the published reference already,
  and the daemon had no variant for it, so a monitor written from the reference
  stopped the daemon reading its configuration at all. The watch reads, for each
  run in its window, the most attempts that run made at any one node of its
  definition, and reports the mean of that number. A retry writes another
  attempt at the node it retried and a loop iteration writes a new position at
  the node it re-entered, so both count, and 1 means no node ran twice. A loop
  that re-enters one node and a retry storm that gets there in the end are both
  successes, so neither moves the error rate, and neither is visible in a step
  count that cannot say whether forty steps were forty nodes or one node forty
  times. Unlike `step_count_divergence` it carries an ordinary threshold,
  because the number reads the same in every project. It reads the durable run
  store, so a daemon serving no runs reports it as insufficient and says so. A
  `foreach` over nine items counts as nine attempts at one node, the same as a
  loop that went round nine times, so set the threshold for the definitions you
  have.
- **Daemon.** A run that repeats itself is ended rather than left retrying.
  `runs.max_step_repeats` in the config file, default 16, is how many times one
  step key may fail with an identical rendered input before the run ends
  `failed` naming the step and the count, the way a spent `max_steps` ends one.
  `retry.max_attempts` does not bound this: a failure classified `unservable`
  comes back after one lease length for ever, and a reclaimed lease is requeued
  free, so both retry the same work without spending any retry budget. Zero
  turns the breaker off. It counts identical inputs and not failures, so a step
  whose input moves because a parallel branch beside it finished is left alone.
- **Wire contract.** Three read endpoints. `GET /v1/api/discovery` lists the values a filter dimension has actually
  taken in a project, for the eight dimensions a run list filters on: entity
  kind and name, service name, environment, tag, metric name, metric label key
  and metric label value, each prefix-matched for typeahead. Before this the
  only discovery route was `GET /v1/api/projects`, so an interface offering an
  environment picker had to build it from whatever happened to be on the current
  page. `GET /v1/api/deltas` returns what has arrived since an opaque cursor, for
  traces, spans or logs, so a list refreshes in place instead of running its
  whole query again. `GET /v1/api/capabilities` says what the storage backend
  behind the daemon can do, because one storage trait has three backends and
  they differ on case folding outside ASCII, on whether a delete is on disk when
  the call returns, and on how retention is enforced. An endpoint whose
  capability is false now answers `501` with the capability named in the body,
  instead of a difference between two backends showing up as an empty list.
- Durable runs. A workflow is a definition the daemon stores, validates and
  versions, and a run is a row the daemon walks: nine constructs (`task`,
  `agent`, `sleep`, `sleep_until`, `wait_event`, `parallel`, `foreach`, `loop`
  and `subworkflow`), templates and guards rendered per attempt, retries with a
  backoff per step, `at_least_once` and `at_most_once` effects, events that
  answer a wait by name and key with parking and duplicate detection, cancel
  down a tree of child runs, and `max_steps`, `run_timeout_ms` and
  `max_cost_usd` ceilings. The step bodies never reach the daemon: a worker in
  the user's own process claims one lease at a time over HTTP, posts `started`,
  heartbeats, and `complete` or `fail`, and every refusal is one of four closed
  reasons. Every run is also a trace, with one span per attempt and the body's
  own spans nested under it. The core is `crates/mcpd-runs`, on its own SQLite
  file beside the trace store (`--runs-data`, `off` to disable), and the
  binding contract is `internal/run-contract-v1.md`.
- Every SDK carries a workflow builder, the run API and a worker, tested
  against a fake daemon and a real one. Each worker enforces a lease's
  `deadline_ms` locally, cancelling the body and failing the attempt
  `retryable`; each step context decodes the input, the prior steps and the
  state on request and fails a shape the body did not expect `invalid_input`;
  and `runs.wait` and `runs.start_and_wait` follow a run to its end and hand
  back its output, or a typed failure carrying the run. A second
  cross-language conformance suite (`tests/e2e/runs.mjs`) serves the same run
  from all six workers and compares the definition, the run, its journal and
  its trace.
- A payload capture switch in every SDK: `capture_payloads` and
  `MCPD_CAPTURE_PAYLOADS`. Off, a span still arrives with its timing, status,
  usage, model, correlation and errors, and `input` and `output` are dropped
  before they are queued, so they never leave the process; a span can
  override it for itself. Redaction on the daemon is the other end of the
  same wire.
- A sink under every SDK's exporter. The HTTP transport is the default; a
  `RecordingSink` keeps batches in memory so a test can assert what was
  recorded without a server, and a `ConsoleSink` prints one JSON line per
  signal. A sink that throws is a failed export, reported and never raised.
- `record(span)` in every SDK, for a span another system finished: queued as
  given, never re-parented, re-timed or re-sampled, refused and counted when
  its ids are malformed. `external_parent_span_id` is settable in all six.
  `stats()` answers the same six counters in all six.
- Step usage that adds itself up. A span the body records through the step
  context, and every span under it, adds its usage to the step's when it
  ends; `set_usage` stays an explicit override. A body that records its model
  calls as spans no longer sums tokens by hand, and `max_cost_usd` fires on
  what the trace shows.
- Worker hooks (`on_lease`, `on_step_start`, `on_step_end`, `on_conflict`)
  and `worker.stats()` with claimed, inflight, completed, failed by class,
  conflicts by reason and unservable, so a lease abandoned as `fenced` no
  longer looks like no work from outside.
- The consistency gate checks that every SDK reads every client environment
  variable in the contract, not only that no SDK reads a foreign one. The
  Python wheel ships `py.typed`.
- Two key scopes for the run surface, `execute` (start, signal, cancel,
  publish) and `worker` (register, claim, started, heartbeat, complete, fail),
  each falling back to the write keys when unset; `mcpd doctor` grades the run
  store as a twelfth group; `mcpd demo` seeds a workflow and a finished run.
- Six native SDKs (TypeScript, Python, Rust, Go, Kotlin, Elixir) generated from
  one spec, plus a cross-language conformance suite that writes the same run
  through all of them and asserts the traces come back structurally identical.
  The suite now also runs a raw `curl` channel, checks each language against the
  fixed values of a canonical run rather than only against its neighbours, and
  fails when a difference it has been told about stops reproducing, so the list
  of known gaps cannot go stale.
- Three storage backends behind one trait: SQLite, Postgres and ClickHouse,
  each tested against a real server rather than a mock.
- OTLP ingest over HTTP in both JSON and protobuf, mapping the `gen_ai.*` and
  OpenInference `llm.*` conventions, and OTLP forwarding out after redaction.
- Evaluation: thirteen deterministic scorers, an LLM judge against any
  OpenAI-shaped endpoint, datasets, experiments and capture-from-production.
- Alerting: latching monitors with webhooks, and service level objectives with
  multi-window burn-rate alerting.
- Projects as a tenancy boundary, with API keys, retention, sampling and an
  ingest ceiling scoped per project. Signals over a project's ceiling are
  refused and reported with the count and the flag that raises it, never
  dropped in silence, and the rest of the batch is still stored. Spans are cut
  by whole trace so a run is never half stored, and an allowance the trim could
  not spend is handed back rather than charged.
- `--redact-key-contains` adds a masking rule of your own on top of the
  built-in ones, for the field your tools put an account number in that no
  default list knows about, and `--redact-max-string` lowers the 128 KiB cap on
  any single stored string. Both print on `mcpd config`, so the rule running
  is the rule you can read back.
- `--cors-origin`, for an interface served from another origin, and
  `--request-timeout-seconds`, the deadline every request is held to. Both
  print on `mcpd config`.
- `--quota-per-minute` also bounds every project the configuration does not
  list, through one counter they all share. The sender picks the project name in
  the body of its own request, so a ceiling that only covered the projects
  somebody wrote down was optional from the sender's side. A project worth its
  own allowance should be named in the `projects` block, where it gets one.
- Six key scopes, `ingest`, `read`, `write`, `admin`, `execute` and `worker`,
  because emitting telemetry, reading prompts, changing stored data, destroying
  it, starting runs and claiming steps are different privileges. An unset write
  list accepts the ingest keys, and an unset admin list accepts the read keys.
  Unset execute and worker lists accept the write keys. Upgrading does not lock
  you out.
- An audit log covering the eight actions that hand a stored payload out or
  destroy one, recording refusals as well as successes. The principal is a
  truncated digest of the key and never the key. Retention does not trim it.
- A subject on that audit row, taken from a header your ingress sets and read
  only from address ranges you name. Off unless both halves are configured.
- Erasure by subject: `DELETE /v1/api/subjects/{user_id}` removes every signal
  for a person within the authorised project and returns a count per signal.
- That erasure now answers with more than a count, because a count alone cannot
  be trusted. The reply names how many stored identifiers are in a form the
  request could not have matched, how many captured dataset cases were emptied
  and which datasets they sit in, and whether the backend applies the deletes
  after answering. A zero it cannot stand behind carries a note saying so, in
  words you can pass on to the person who asked. Stored identifiers are hashed
  with the redaction key and nothing records which key wrote which row, so a
  rotation puts older rows beyond reach and mcpd cannot tell you that is what
  happened.
- Dataset cases captured from an erased run are emptied in place rather than
  deleted, so a regression test somebody captured is not destroyed without
  anyone being told. A case captured into another project's dataset is out of
  reach: the call is authorised for one project and touches one project.
- `mcpd doctor`, which grades a deployment and names the flag or config key
  behind every warning and fault. A check that was not run reports `skipped`
  rather than `ok`, and the summary counts the skipped checks separately so a
  run that verified almost nothing cannot read as a clean bill of health. It
  exits non-zero on a fault so it can gate a deploy.
- Two more groups for `mcpd doctor`. `limits` grades the ingest ceilings and the
  sampling fractions, including a project-level `sample` outside 0.0 to 1.0,
  which `--sample` refuses at startup and a config file did not. `identity`
  grades the audit subject header and faults when the range permitted to set it
  contains the whole internet.
- A load harness in `benches/`: a plain Rust generator that measures a real
  `mcpd serve` over a socket, with the whole path included. Its README says how
  to reproduce a run and what each column means. The figures are not published
  on the site.
- `mcpd mcp`, a Model Context Protocol server over stdio serving `find_runs`,
  `get_run` and `run_stats`, so an agent can read its own traces. It is a
  client of a running daemon, so it obeys the same key scopes as everything
  else rather than reading around them.
- Redaction before storage, on by default, across all six signals.
- A Helm chart with optional Ingress, NetworkPolicy, PodDisruptionBudget,
  ServiceAccount and ServiceMonitor, Kubernetes manifests and a Compose file.

### Changed

- **Toolchain.** The documentation site builds on Astro 7, React 19.3 and
  TypeScript 6, and the TypeScript SDK's tests run on vitest 5 and TypeScript
  6. Building the site now needs Node 22.12 or newer, which is what Astro 7
  requires; the published SDK still runs on Node 20. TypeScript 7 was not
  taken: it ships no programmatic API, which Astro's tooling needs, and it
  removes the `node10` module resolution, so both packages pin `^6.0.3`. The
  SDK's CommonJS build stopped using `node10` resolution, which TypeScript 6
  deprecates, and its output is unchanged.

- **Dependencies, for the release.** The daemon builds on axum 0.8, reqwest
  0.13, tower-http 0.7, rusqlite 0.40 (SQLite 3.53), jsonschema 0.56 and rand
  0.10, with every transitive crate at its current version, and the floor is
  now Rust 1.88, which is the oldest compiler the crate graph declares it
  needs; 1.82 had been a number on a page rather than a version that built.
  Two consequences are worth knowing. TLS peers are verified against the
  platform's certificate store rather than a root bundle compiled into the
  binary, so a forward endpoint or a judge behind a private CA works by
  installing that CA where the operating system looks, and the container image
  now ships `ca-certificates` on Alpine 3.24 for the same reason. And the
  Kotlin SDK is built with Kotlin 2.4 and kotlinx-serialization 1.11, and its
  timestamps are `kotlin.time.Instant` from the standard library, so
  `kotlinx-datetime` is no longer a dependency: a caller that imported
  `kotlinx.datetime.Instant` to read a `Trace` imports `kotlin.time.Instant`
  instead. `cargo deny` now checks licences in CI as well as advisories.

- **Licensing.** One licence, stated where a machine can check it. The Apache
  2.0 text at the repository root now names its copyright holder, Internet
  Technology Services LLC, in place of the stock `[name of copyright owner]`
  placeholder it shipped with, and a `NOTICE` file states that the same licence
  covers the daemon, all six SDKs, the interface, the site and the
  specification. Each SDK is installed on its own from its own registry, so each
  one is now its own redistribution: `sdks/typescript`, `sdks/python`,
  `sdks/go`, `sdks/kotlin`, `sdks/elixir` and `crates/mcpd-sdk` each carry the
  licence text, the npm `files` list and the Python `include` list pack it, and
  the Kotlin build publishes a POM that declares it. The Go SDK is its own
  module and had no way to declare a licence at all, which is why its absence
  was invisible. `make check` now fails on a licence file it has not reviewed,
  on a package whose declaration and text disagree, and on any directory named
  `ee`, `enterprise`, `pro` or `commercial`, because a held-back directory is
  how a project that calls itself open source stops being one.

- **Wire contract.** A list endpoint now refuses a query parameter it does not
  apply, naming it, instead of accepting and dropping it. `?statuss=error` used
  to come back `200` with every run in the project and nothing about the
  response said the filter had not applied. The trade is deliberate and goes the
  other way from before: a client sending a filter a newer daemon understands
  now gets a `400` from an older one rather than a wider result set, which is
  the better of the two failures. `service_name`, `service_version`,
  `resource_id`, `run_id`, `request_id`, `experiment_id` and `release` are
  recorded on every trace and are not yet filters, so they are refused with a
  message that says so rather than one that reads as a spelling mistake.

- **Sampling now decides every signal, not only spans.** A log, metric, score
  or feedback record whose trace the sampler discarded is no longer stored. It
  used to be, which left rows pointing at spans that were never written, so a
  reader saw a log line whose trace did not exist. A signal whose trace arrived
  in an earlier batch keeps its decision, because the deterministic sampler
  answers the same trace id the same way; a signal a ratio policy cannot
  re-decide is kept, because losing real data is worse than an orphan. Visible
  on any deployment running `--sample`.
- `POST /v1/api/scores` and `POST /v1/api/feedback` answer `422 sampled_out`
  when the trace they name was discarded by the sampler, rather than storing a
  record no reader can open.

- The step span and the run's root span the daemon writes carry the priced
  usage and cost as `attributes.usage` and `attributes.cost_usd`, not as the
  span's own `usage` and `cost`. A step span is a container: the model spans a
  body records under it carry the tokens they spent, and a trace total that
  summed both counted every token twice.
- **A trace reading panel, mountable anywhere.** The waterfall, the span
  inspector, the payload viewer and in-trace search are one component rather
  than markup welded into one screen. The waterfall's ruler and every row
  share one grid, so expanding a branch does not move the time axis, and a
  span kept only because its payload matched a search is painted differently
  from one whose name matched, so a reader knows why it survived.
- **A filtered, virtualized run list.** Twelve filter dimensions, each one a
  parameter the endpoint actually declares, with values suggested by the new
  discovery endpoint. Presets and a custom time range. Twenty-four columns,
  three of them fixed, with metadata columns added by key. Preferences persist
  under a versioned key. The whole view is in the address bar, so it is
  shareable. Live refresh runs on the delta endpoint and stops while the tab
  is hidden.
- **Two new screens.** A conversation view reconstructs a run as the agent
  turn its spans already encode, from the span tree alone. A threads screen
  reads one thread's runs beside the panel and steps through them.
- **A storage capability set.** `GET /v1/api/capabilities` publishes what the
  backend can do, every store implements it, and a door whose capability is
  false answers `501 not_implemented` naming the capability. A reader no
  longer discovers a silent difference between SQLite and ClickHouse by
  getting a wrong answer.
- **Discovery for every filter dimension.** `GET /v1/api/discovery` answers
  entity kinds and names, service names, environments, tags, metric names,
  label keys and label values, per project and prefix-matched, from a
  discovery table written in the same transaction as the signals it
  describes. This is what makes a filter box possible.
- **Delta polling.** `GET /v1/api/deltas` returns what has arrived since an
  opaque cursor, oldest first, on all three backends. An empty poll leaves
  the cursor where it was.
- **Metrics the daemon derives.** A finished span of a kind that has a
  duration writes a histogram with a status label, and a model span carrying
  usage writes the thirteen token counters mcpd already models. A total is
  written whenever it is defined, an explicit zero included; a detail line
  only when it is strictly positive.
- Per-signal retention, applied on all three backends and anchored on event
  time; a ClickHouse deployment is cut one partition per day and drops whole
  partitions before deleting rows. Every correlation identifier on every
  signal table is now indexed. A start with nothing to apply issues no schema
  statement at all.
- The trace, span, log, score, metric and feedback lists refuse a parameter
  they do not apply, naming it, rather than accepting it and returning a
  result that ignored it.
- A `log_level` floor in every SDK (`MCPD_LOG_LEVEL`, default `debug`),
  checked before the record is built. It covers logs only.
- An error passed under a structured-data key is serialised to its type,
  message and stack rather than collapsing to an empty object, and a value
  nothing can serialise costs its own key rather than the batch it travelled
  in.
- A correlation key present with the language's absent value no longer erases
  an inherited one on a log line or a metric point, in any SDK.
- The OTLP door carries what it used to drop: span links, span events (an
  `exception` event fills the span's error from its OTel attributes), the
  trace-flags byte (kept on the span; the daemon's sampler does not read it in
  this build, while every SDK honours the flag on an inbound `traceparent`), and
  `external_parent_span_id` when the parent is both absent and marked remote.
- The native ingest door also takes a top-level array and NDJSON, chosen by
  content type. The signal and body caps apply across the whole request.
- `max_body_bytes` and `max_batch_signals` are settable by flag and config
  file. The OTLP logs and metrics doors gained a per-request record cap.
- An over-quota refusal carries `Retry-After`, set from when the window resets.
- The TypeScript and Python workers no longer drop a run's correlation field
  when an explicit undefined or None sits over it. Kotlin draws span ids from
  `SecureRandom` rather than `ThreadLocalRandom`, so all six SDKs are
  cryptographic.
- Two SDK renames that go with the sink seam: the Go `Stats.Sent` counter is
  `Stats.Exported`, and the Rust `Error` enum gained a `Hook` variant for a
  worker hook that panicked, which an exhaustive match has to name.

### Fixed

- **An idle worker no longer reports a failed claim every ten seconds.** A
  worker long-polls `POST /v1/api/steps/claim` for `wait_ms`, twenty seconds
  by default, and the daemon holds the request open for that long when nothing
  is runnable. Every SDK applied its ordinary request timeout, ten seconds by
  default, to that request too, so the client cancelled the poll it had asked
  for before the daemon could answer, logged `claim failed; retrying`, paused
  and polled again. The run itself was never affected, since a step that
  exists is claimed at once; the loop was an error on a healthy system. The
  claim request now outlives the ordinary timeout by `wait_ms`, in all six
  SDKs, and nothing else about the transport changes.

- **ClickHouse could not create the aspect note table.** The `sketch` column
  was a `FixedString(1024)`, and ClickHouse refuses a fixed string wider than
  256 bytes unless a server setting allows it, so migration 1 failed on every
  ClickHouse deployment and the daemon could not start against one. It is a
  `String` now. The read that files a note under its run's entity also had its
  `FINAL` and its alias the wrong way round, which the server rejected as a
  syntax error. Both were found by running the shared conformance suite
  against a real server, which is where it runs in CI.
- **The conformance suite was flaky on ClickHouse and wrong on a reused
  Postgres.** Its fixture left every row's arrival time at the epoch, so a run
  written after the poll cursor was invisible whenever its id sorted before
  the cursor's; it declared a thirty day retention window over a forty day old
  run and then counted what an empty policy kept; and it wrote notes under ids
  that were the same in every run, so a second run against a Postgres or
  ClickHouse that kept the first run's rows updated those instead of writing
  its own. Each is fixed in the fixture. The two Postgres tests that read the
  catalogue while another drops an index now take turns.
- **Three CI jobs could not pass as written.** `examples/typescript` had no
  TypeScript to run and no `tsconfig.json`, so the `examples` job's
  `tsc --noEmit` ran a stub that exits 1; it now carries both. The TypeScript
  SDK's live test and the cross-language conformance harness both read
  feedback with a `kind` filter, which `GET /v1/api/feedback` refuses by name;
  both now read by run and check the kind on the record. And the curl probe
  the error-path suite drives asked Python for `datetime.UTC`, which arrived
  in 3.11, so on a machine with the 3.9 the Python SDK itself supports the
  raw HTTP control failed before it sent anything.

- **The cost documentation described a price book you cannot load.** It told
  you to embed mcpd as a library and pass a book to
  `AppState::with_price_book`, which reads as a capability a deployment has.
  That method has no caller anywhere in the tree, no flag or config file key
  reaches it, and `AppState::new` always installs the built-in book, so no
  operator can supply negotiated rates, a self-hosted model or a currency other
  than US dollars. The page now says so plainly, and a test fails if it ever
  names the injection point again while nothing under `crates/mcpd/src` reaches
  it. The gap itself is unchanged and is listed under Known issues.

- **Security: the monitor and objective lists ignored the project a key is
  scoped to.** `GET /v1/api/monitors` and `GET /v1/api/slos` checked the read
  scope and then returned every configured monitor and objective on the daemon,
  whatever projects the key was limited to. A monitor carries the `webhook` it
  posts to, so a read key scoped to one project received every other project's
  alerting destinations along with their names and thresholds. `POST
  /v1/api/monitors/{id}/ack` and the objective equivalent had the same hole on
  the write side: the id is global, so a write key scoped to one project could
  silence another project's alert. All four now check the key's project grant,
  the two lists by filtering and the two acknowledgements by refusing with
  `403` after the lookup, naming the monitor or objective the caller already
  named rather than the project that owns it. The filter is a small function
  with a test rather than an inline closure, because the code that was wrong
  here is the code that was not there. `web/src/pages/docs/security.astro` said
  four endpoints take no project; three do (the project list, the scorer
  catalogue and the storage capability set), and the sentence is corrected in
  all nine languages.

- The request body limit is the configured one. The daemon documented 32 MiB
  and enforced 2 MiB, because the HTTP framework applies a limit of its own
  inside every body extractor and nothing turned it off. A 3 MiB batch was
  refused with an empty 413 from a limit no flag names. There is one limit now,
  `max_body_bytes`, it applies to every route that reads a body, and a body
  over it is refused in the same JSON shape as every other error, naming the
  size sent and the size accepted.
- A gzip-compressed body is held to the same limit after decompression. The
  wire size was bounded and the decompressed size was not, so a 2 MiB request
  of compressed zeros expanded to 2 GiB in memory before a byte of it was
  parsed. The daemon now reads one byte past the limit and stops, answering
  413 with the limit named.
- The query API no longer allows every browser origin. It did, on every data
  route and the delete routes, and the read API is open by default, so any web
  page open in the same browser as a daemon on localhost could read the prompts
  it stores. With nothing configured, only a page served by the daemon itself,
  which is what the bundled interface is, can read the API from a browser. An
  interface served from elsewhere names its origin with `--cors-origin`.
  `/healthz`, `/readyz` and `/metrics` stay readable from any origin.
- Every request has a deadline. A storage backend that accepted connections
  and never answered held a request for as long as the client waited. A
  request that runs past `--request-timeout-seconds`, 30 by default, is
  answered with a 503 whose body says `timeout` and names the flag.
- `mcpd_request_errors_total` moves. It was rendered on `/metrics` and never
  incremented, so an operator alerting on it was alerting on a number that
  could not change. It now counts every response that left as a server error,
  timeouts included.
- Forwarding retries before it gives up, and says so when it does. A batch the
  destination refused or could not reach was dropped on the first failure with
  a log line and nothing else, while the code said it was counted. It now gets
  three attempts with backoff, honouring `Retry-After`, never for a 4xx other
  than 429, and only then counted in `mcpd_forward_dropped_spans_total`,
  by spans. A forward still in flight when the daemon stops is waited for, up
  to five seconds, rather than dropped with the runtime; the retention loop
  stops between passes rather than mid-delete for the same reason.
- ClickHouse no longer counts a run twice when its steps arrive in two batches.
  The `traces` table was sorted on the run's start time, which a later batch
  can lower, so the rebuilt row carried a different sort key and every read
  saw the run twice. It is sorted on `(project, trace_id)` now, `migrate`
  moves an existing table across on the next start, and the four suppressions
  the conformance suite carried for this are gone. A run whose earliest step
  moves it into an earlier month still lands in another partition; that
  boundary remains and is written down in the schema.
- `--config-file`'s help names everything the file holds, not two of the six.

- A dataset id could be seized by re-creating it under another project, which
  made the dataset tenancy boundary bypassable in three requests.
- `add_dataset_items` looked up a dataset id before checking the key, so ids
  could be enumerated without a credential, and it bypassed the intake, so
  captured production payloads were stored unredacted.
- A read key could delete traces, and an ingest key could copy any stored
  prompt into a dataset and read it back.
- Redaction let a weaker rule on an inner key override the mask inherited from
  a secret parent, so a value under a matched key stayed partly readable.
- OTLP read its token total from the opposite semantic convention to the counts
  beside it, so a span instrumented with both reported a total unrelated to its
  own input and output, and every cost aggregate over it was wrong.
- An unrecognised span kind ended the classification search instead of falling
  through, so spans from newer SDK releases stopped being billed.
- Image input tokens were never billed, and a price book key that was not
  lowercase could never be matched.
- Cost, under-billed by 65% on a real usage shape. The plain input share was
  derived as the reported total minus the separately metered tokens, which
  assumes the total includes cache tokens. Anthropic reports `input_tokens` net
  of cache, so the subtraction underflowed, saturated at zero, and the whole
  uncached share billed as nothing. No error and no flag: the cost column
  quietly dropped. The detail a provider reports wins over the subtraction now.
- The same defect on the output half of that function, which was left unfixed
  when the input half was corrected. 400 of 1000 output tokens, 40% of that
  bill, still escaped it. Both halves are written identically now.
- Cost was not deterministic. The case-insensitive fallback in the price book
  took the first match in iteration order, and `HashMap` ordering is seeded per
  process, so a book holding two keys that differ only in case resolved to one
  or the other by hash seed. mcpd prices at ingest and stores the result, so
  two spans for one model would have carried permanently different costs. The
  smallest matching key wins now.
- Redaction ranked `hash` above `partial`, so a `hash` rule inherited from an
  outer key overrode a `partial` rule on an inner one. `partial` masks a value
  of six characters or fewer and `hash` does not, so a five-digit value under a
  `user_id` key was stored as a digest recoverable in under a hundred thousand
  guesses. The two rank equal now and a tie resolves to `mask`, because both
  rules agreed the field is sensitive and disagreed only about treatment.
- Erasure by subject answered a request about a person with `200` and a count of
  zero after the redaction key was rotated, which reads as "no data about this
  person". That is a false answer to a legal request. It reports rather than
  refuses, because a retry after a timeout has to be answerable and the reply
  for "never seen" and "erased an hour ago" must be identical.
- Three ways a signal survived an erasure. A score, a log line or a piece of
  feedback that named a `span_id` and no `trace_id` was not reached. A captured dataset
  case kept the subject's verbatim prompt. Feedback the subject wrote survived
  on somebody else's run, carrying their own prose.
- `mcpd doctor` reported a false fault and exited 2 against any live SQLite
  deployment under write load. The embedded store takes one writer, so a busy
  healthy daemon holds the write lock almost continuously and the probe write
  lost the race for it. The command exists to gate deploys, so it failed
  precisely when the deployment was healthy. Contention is retried and then
  reported as `skipped`. A read-only mount, a full disk, a role without
  `INSERT` and a corrupt lock are each still a fault.
- The ingest ceiling was bypassable by naming any project the configuration did
  not list, which is not a ceiling when the caller picks the project name.

### Known issues

Reported rather than hidden. The conformance suite found twelve disagreements
between the SDKs on its first full run, and records the ones still standing in
`KNOWN_GAPS` in `tests/e2e/run.mjs`, each named with the file and function that
causes it. A run fails when a listed difference stops reproducing, so that list
cannot outlive the bugs it describes and is the authority on what is left.

None stand as this is written. The two that did, the Rust SDK's missing
correlation setters and the Elixir SDK dropping `request_id` and `release`, are
fixed, and the suite's list of known gaps is empty.

### Security

- Redaction runs before anything is written, so a matched value never reaches
  the disk and never reaches a forwarding destination either.
- Every SDK's live test suite fails rather than skips when a daemon is expected
  and the endpoint does not reach it. Several suites had been reporting success
  having contacted no server at all.

[Unreleased]: https://github.com/mcpd/mcpd/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/mcpd/mcpd/releases/tag/v0.1.0
