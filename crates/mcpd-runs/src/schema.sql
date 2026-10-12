-- The durable run core. Schema version 2.
--
-- Timestamps are INTEGER nanoseconds since the Unix epoch, the same convention
-- the observability schema uses, so a run row and a span row sort against each
-- other without a conversion.
--
-- Every deadline in this design is a row in `timers`. A deadline that lives as
-- a column on some other table is a promise: it needs a scan written for it,
-- and twice now the scan was never written. One tick reads this one table.

PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;

CREATE TABLE IF NOT EXISTS run_schema_version (
    version    INTEGER PRIMARY KEY,
    applied_at INTEGER NOT NULL
);

-- Immutable per version. `body_hash` is the sha256 of the canonical body, which
-- is what decides whether a document posted without a version is a version
-- that already exists.
CREATE TABLE IF NOT EXISTS definitions (
    project    TEXT    NOT NULL,
    id         TEXT    NOT NULL,
    version    INTEGER NOT NULL,
    kind       TEXT    NOT NULL,
    body       TEXT    NOT NULL,
    body_hash  TEXT    NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (project, id, version)
);
CREATE INDEX IF NOT EXISTS definitions_hash ON definitions (project, id, body_hash);

-- The run header. `version` is the compare-and-set column, `cursor` is the
-- frontier, and `next_event_seq` allocates `run_events.seq` under that same
-- compare-and-set so two writers cannot leave a gap.
CREATE TABLE IF NOT EXISTS runs (
    run_id             TEXT    PRIMARY KEY,
    project            TEXT    NOT NULL,
    definition_id      TEXT    NOT NULL,
    definition_version INTEGER NOT NULL,
    status             TEXT    NOT NULL,
    cursor             TEXT    NOT NULL,
    input              TEXT,
    output             TEXT,
    error              TEXT,
    error_class        TEXT,
    trace_id           TEXT    NOT NULL,
    root_span_id       TEXT    NOT NULL,
    state              TEXT    NOT NULL DEFAULT '{}',
    correlation        TEXT    NOT NULL DEFAULT '{}',
    idempotency_key    TEXT,
    bindings           TEXT    NOT NULL DEFAULT '{}',
    version            INTEGER NOT NULL DEFAULT 0,
    next_event_seq     INTEGER NOT NULL DEFAULT 0,
    steps_used         INTEGER NOT NULL DEFAULT 0,
    max_steps          INTEGER,
    cost_usd           REAL    NOT NULL DEFAULT 0,
    max_cost_usd       REAL,
    deadline_at        INTEGER,
    root_run_id        TEXT    NOT NULL,
    parent_run_id      TEXT,
    parent_step_key    TEXT,
    depth              INTEGER NOT NULL DEFAULT 0,
    created_at         INTEGER NOT NULL,
    updated_at         INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS runs_open   ON runs (status, updated_at);
CREATE INDEX IF NOT EXISTS runs_parent ON runs (parent_run_id);
-- Erasure and retention delete a whole tree by its root. Added after version
-- 2 shipped; `IF NOT EXISTS` makes it land on an existing file at the next
-- start, and no row changes shape, so the version stays where it is.
CREATE INDEX IF NOT EXISTS runs_root   ON runs (root_run_id);
CREATE INDEX IF NOT EXISTS runs_list   ON runs (project, updated_at, run_id);
-- Two simultaneous posts of one key produce one run: the second insert loses
-- here, and the conflict path reads the winning row.
CREATE UNIQUE INDEX IF NOT EXISTS runs_idempotency
    ON runs (project, idempotency_key) WHERE idempotency_key IS NOT NULL;

-- The journal, and the authority. One row per attempt, append only.
--
-- `started_at` is the discriminator the reclaim branch reads: it is set by
-- `started`, never cleared, and null on an attempt that never declared a body,
-- so one `orphaned` state serves both live states. `ended_by_lease_id` is what
-- makes `already_applied` decidable: an ended attempt whose ending lease is
-- the caller's own is the caller's own committed work.
CREATE TABLE IF NOT EXISTS step_attempts (
    run_id            TEXT    NOT NULL,
    step_key          TEXT    NOT NULL,
    attempt           INTEGER NOT NULL,
    kind              TEXT    NOT NULL,
    effect            TEXT    NOT NULL,
    status            TEXT    NOT NULL,
    input             TEXT,
    output            TEXT,
    error             TEXT,
    error_class       TEXT,
    error_details     TEXT,
    usage             TEXT,
    cost_usd          REAL,
    span_id           TEXT    NOT NULL,
    deadline_ms       INTEGER,
    budget_spent      INTEGER NOT NULL DEFAULT 0,
    lease_id          TEXT,
    lease_owner       TEXT,
    lease_until       INTEGER,
    leased_at         INTEGER,
    started_at        INTEGER,
    orphaned_at       INTEGER,
    reclaim_until     INTEGER,
    ended_at          INTEGER,
    ended_by_lease_id TEXT,
    created_at        INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (run_id, step_key, attempt)
);
-- A lease id addresses a worker call, so it has to resolve to exactly one row.
CREATE UNIQUE INDEX IF NOT EXISTS step_attempts_lease
    ON step_attempts (lease_id) WHERE lease_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS step_attempts_open ON step_attempts (status);

-- Every deadline. `step_key` is empty for a run-level deadline. `created_at`
-- is when the row was written, which is the start of the span a sleep leaves
-- behind.
CREATE TABLE IF NOT EXISTS timers (
    run_id     TEXT    NOT NULL,
    step_key   TEXT    NOT NULL,
    kind       TEXT    NOT NULL,
    due_at     INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (run_id, step_key, kind)
);
CREATE INDEX IF NOT EXISTS timers_due ON timers (due_at);

-- One index lets an inbound event find the waiting run by key. The lookup
-- joins `runs` and matches only a non-terminal one, so a row a cancelled run
-- left behind cannot eat a live run's approval. `span_id` is the open
-- `workflow_wait_event` span the delivery or the timeout closes.
CREATE TABLE IF NOT EXISTS waits (
    run_id          TEXT    NOT NULL,
    step_key        TEXT    NOT NULL,
    project         TEXT    NOT NULL,
    event           TEXT    NOT NULL,
    correlation_key TEXT    NOT NULL,
    expires_at      INTEGER NOT NULL,
    span_id         TEXT    NOT NULL,
    created_at      INTEGER NOT NULL,
    PRIMARY KEY (run_id, step_key)
);
CREATE INDEX IF NOT EXISTS waits_lookup ON waits (project, event, correlation_key);

CREATE TABLE IF NOT EXISTS run_events (
    run_id  TEXT    NOT NULL,
    seq     INTEGER NOT NULL,
    type    TEXT    NOT NULL,
    payload TEXT    NOT NULL,
    at      INTEGER NOT NULL,
    PRIMARY KEY (run_id, seq)
);

-- Every event the daemon accepts, whether it was parked or delivered straight
-- to a waiting run. A row with `consumed_by_run_id` set at insert time is a
-- delivery record: it carries no payload and exists so the provider's fifth
-- retry is a no-op rather than a spare event that jumps the next run's
-- approval. `id_is_derived` records which kind of id the row holds, because a
-- derived id deduplicates parking only.
CREATE TABLE IF NOT EXISTS parked_events (
    project            TEXT    NOT NULL,
    event              TEXT    NOT NULL,
    correlation_key    TEXT    NOT NULL,
    event_id           TEXT    NOT NULL,
    id_is_derived      INTEGER NOT NULL DEFAULT 0,
    payload            TEXT,
    received_at        INTEGER NOT NULL,
    expires_at         INTEGER NOT NULL,
    consumed_by_run_id TEXT,
    PRIMARY KEY (project, event_id)
);
CREATE INDEX IF NOT EXISTS parked_lookup
    ON parked_events (project, event, correlation_key, consumed_by_run_id);
CREATE INDEX IF NOT EXISTS parked_expiry ON parked_events (expires_at);
-- Retention and erasure find the events a run tree consumed by the run, and
-- they do it while holding the store's only lock. `parked_lookup` names the
-- run last, so without this each tree read the whole table: one retention
-- batch of 200 trees over 200,000 parked rows held the lock for seconds, and
-- the readiness probe gave up behind it. `migrate` runs this file on every
-- start, so an existing database gains the index on its next one.
CREATE INDEX IF NOT EXISTS parked_consumer ON parked_events (consumed_by_run_id);

-- The span outbox. One row per span write, enqueued in the transaction that
-- made the change it describes and drained by a loop in the server. `seq`
-- orders the writes for one span, so a close row never lands ahead of its own
-- open row. `claim_id` and `claimed_until` are what a drain takes before it
-- ingests; a claim that is not acknowledged expires and the row is offered
-- again.
CREATE TABLE IF NOT EXISTS span_outbox (
    seq           INTEGER PRIMARY KEY AUTOINCREMENT,
    outbox_id     TEXT    NOT NULL UNIQUE,
    run_id        TEXT    NOT NULL,
    span_id       TEXT    NOT NULL,
    ingest_key    TEXT    NOT NULL,
    phase         TEXT    NOT NULL,
    payload       TEXT    NOT NULL,
    claim_id      TEXT,
    claimed_until INTEGER,
    created_at    INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS span_outbox_span ON span_outbox (run_id, span_id, seq);
