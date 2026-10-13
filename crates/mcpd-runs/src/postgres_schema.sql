-- The durable run core, in Postgres. Schema version 1.
--
-- The same tables, columns and keys as `schema.sql`, which carries the reasons
-- for each of them. Three things differ, and each keeps the two backends
-- answering identically.
--
-- Timestamps are BIGINT nanoseconds since the Unix epoch rather than
-- `timestamptz`, which holds microseconds and would round every deadline.
--
-- Every text column compares byte for byte (`COLLATE "C"`), as SQLite's does.
-- Under a locale collation a page of runs, a list of step keys and the order a
-- claim offers steps in would all sort differently from SQLite.
--
-- Integers are BIGINT throughout, so every value the engine binds is the type
-- the column already has.
--
-- `migrate` runs this file only when `run_schema_version` does not hold the
-- version this build writes, so a change here, an index included, raises
-- `SCHEMA_VERSION` in `postgres.rs` as well.

CREATE TABLE IF NOT EXISTS run_schema_version (
    version    BIGINT PRIMARY KEY,
    applied_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS definitions (
    project    TEXT COLLATE "C" NOT NULL,
    id         TEXT COLLATE "C" NOT NULL,
    version    BIGINT           NOT NULL,
    kind       TEXT COLLATE "C" NOT NULL,
    body       TEXT COLLATE "C" NOT NULL,
    body_hash  TEXT COLLATE "C" NOT NULL,
    created_at BIGINT           NOT NULL,
    PRIMARY KEY (project, id, version)
);
CREATE INDEX IF NOT EXISTS definitions_hash ON definitions (project, id, body_hash);

CREATE TABLE IF NOT EXISTS runs (
    run_id             TEXT COLLATE "C" PRIMARY KEY,
    project            TEXT COLLATE "C" NOT NULL,
    definition_id      TEXT COLLATE "C" NOT NULL,
    definition_version BIGINT           NOT NULL,
    status             TEXT COLLATE "C" NOT NULL,
    cursor             TEXT COLLATE "C" NOT NULL,
    input              TEXT COLLATE "C",
    output             TEXT COLLATE "C",
    error              TEXT COLLATE "C",
    error_class        TEXT COLLATE "C",
    trace_id           TEXT COLLATE "C" NOT NULL,
    root_span_id       TEXT COLLATE "C" NOT NULL,
    state              TEXT COLLATE "C" NOT NULL DEFAULT '{}',
    correlation        TEXT COLLATE "C" NOT NULL DEFAULT '{}',
    idempotency_key    TEXT COLLATE "C",
    bindings           TEXT COLLATE "C" NOT NULL DEFAULT '{}',
    version            BIGINT           NOT NULL DEFAULT 0,
    next_event_seq     BIGINT           NOT NULL DEFAULT 0,
    steps_used         BIGINT           NOT NULL DEFAULT 0,
    max_steps          BIGINT,
    cost_usd           DOUBLE PRECISION NOT NULL DEFAULT 0,
    max_cost_usd       DOUBLE PRECISION,
    deadline_at        BIGINT,
    root_run_id        TEXT COLLATE "C" NOT NULL,
    parent_run_id      TEXT COLLATE "C",
    parent_step_key    TEXT COLLATE "C",
    depth              BIGINT           NOT NULL DEFAULT 0,
    created_at         BIGINT           NOT NULL,
    updated_at         BIGINT           NOT NULL
);
CREATE INDEX IF NOT EXISTS runs_open   ON runs (status, updated_at);
CREATE INDEX IF NOT EXISTS runs_parent ON runs (parent_run_id);
CREATE INDEX IF NOT EXISTS runs_root   ON runs (root_run_id);
CREATE INDEX IF NOT EXISTS runs_list   ON runs (project, updated_at, run_id);
CREATE UNIQUE INDEX IF NOT EXISTS runs_idempotency
    ON runs (project, idempotency_key) WHERE idempotency_key IS NOT NULL;

CREATE TABLE IF NOT EXISTS step_attempts (
    run_id            TEXT COLLATE "C" NOT NULL,
    step_key          TEXT COLLATE "C" NOT NULL,
    attempt           BIGINT           NOT NULL,
    kind              TEXT COLLATE "C" NOT NULL,
    effect            TEXT COLLATE "C" NOT NULL,
    status            TEXT COLLATE "C" NOT NULL,
    input             TEXT COLLATE "C",
    output            TEXT COLLATE "C",
    error             TEXT COLLATE "C",
    error_class       TEXT COLLATE "C",
    error_details     TEXT COLLATE "C",
    usage             TEXT COLLATE "C",
    cost_usd          DOUBLE PRECISION,
    span_id           TEXT COLLATE "C" NOT NULL,
    deadline_ms       BIGINT,
    budget_spent      BIGINT           NOT NULL DEFAULT 0,
    lease_id          TEXT COLLATE "C",
    lease_owner       TEXT COLLATE "C",
    lease_until       BIGINT,
    leased_at         BIGINT,
    started_at        BIGINT,
    orphaned_at       BIGINT,
    reclaim_until     BIGINT,
    ended_at          BIGINT,
    ended_by_lease_id TEXT COLLATE "C",
    created_at        BIGINT           NOT NULL DEFAULT 0,
    PRIMARY KEY (run_id, step_key, attempt)
);
CREATE UNIQUE INDEX IF NOT EXISTS step_attempts_lease
    ON step_attempts (lease_id) WHERE lease_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS step_attempts_open ON step_attempts (status);

CREATE TABLE IF NOT EXISTS timers (
    run_id     TEXT COLLATE "C" NOT NULL,
    step_key   TEXT COLLATE "C" NOT NULL,
    kind       TEXT COLLATE "C" NOT NULL,
    due_at     BIGINT           NOT NULL,
    created_at BIGINT           NOT NULL,
    PRIMARY KEY (run_id, step_key, kind)
);
CREATE INDEX IF NOT EXISTS timers_due ON timers (due_at);

CREATE TABLE IF NOT EXISTS waits (
    run_id          TEXT COLLATE "C" NOT NULL,
    step_key        TEXT COLLATE "C" NOT NULL,
    project         TEXT COLLATE "C" NOT NULL,
    event           TEXT COLLATE "C" NOT NULL,
    correlation_key TEXT COLLATE "C" NOT NULL,
    expires_at      BIGINT           NOT NULL,
    span_id         TEXT COLLATE "C" NOT NULL,
    created_at      BIGINT           NOT NULL,
    PRIMARY KEY (run_id, step_key)
);
CREATE INDEX IF NOT EXISTS waits_lookup ON waits (project, event, correlation_key);

CREATE TABLE IF NOT EXISTS run_events (
    run_id  TEXT COLLATE "C" NOT NULL,
    seq     BIGINT           NOT NULL,
    type    TEXT COLLATE "C" NOT NULL,
    payload TEXT COLLATE "C" NOT NULL,
    at      BIGINT           NOT NULL,
    PRIMARY KEY (run_id, seq)
);

CREATE TABLE IF NOT EXISTS parked_events (
    project            TEXT COLLATE "C" NOT NULL,
    event              TEXT COLLATE "C" NOT NULL,
    correlation_key    TEXT COLLATE "C" NOT NULL,
    event_id           TEXT COLLATE "C" NOT NULL,
    id_is_derived      BIGINT           NOT NULL DEFAULT 0,
    payload            TEXT COLLATE "C",
    received_at        BIGINT           NOT NULL,
    expires_at         BIGINT           NOT NULL,
    consumed_by_run_id TEXT COLLATE "C",
    PRIMARY KEY (project, event_id)
);
CREATE INDEX IF NOT EXISTS parked_lookup
    ON parked_events (project, event, correlation_key, consumed_by_run_id);
CREATE INDEX IF NOT EXISTS parked_expiry ON parked_events (expires_at);
CREATE INDEX IF NOT EXISTS parked_consumer ON parked_events (consumed_by_run_id);

-- `seq` comes from a sequence, which hands out numbers in the order rows are
-- inserted rather than committed. That is enough for what it orders: the
-- writes for one span, whose open row commits before its close row is
-- written.
CREATE TABLE IF NOT EXISTS span_outbox (
    seq           BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    outbox_id     TEXT COLLATE "C" NOT NULL UNIQUE,
    run_id        TEXT COLLATE "C" NOT NULL,
    span_id       TEXT COLLATE "C" NOT NULL,
    ingest_key    TEXT COLLATE "C" NOT NULL,
    phase         TEXT COLLATE "C" NOT NULL,
    payload       TEXT COLLATE "C" NOT NULL,
    claim_id      TEXT COLLATE "C",
    claimed_until BIGINT,
    created_at    BIGINT           NOT NULL
);
CREATE INDEX IF NOT EXISTS span_outbox_span ON span_outbox (run_id, span_id, seq);

-- The `user_id` a run's correlation carries, for erasure. NULL for a column
-- that does not parse, so a bad row reads as no subject rather than failing
-- the statement, because a statement that fails is an erasure that does not
-- happen. Only a string matches, as only a string can in SQLite when it is
-- compared with the text the caller sent.
CREATE OR REPLACE FUNCTION mcpd_runs_user_id(correlation TEXT) RETURNS TEXT
LANGUAGE plpgsql IMMUTABLE AS $$
DECLARE
    doc JSONB;
BEGIN
    doc := correlation::jsonb;
    IF jsonb_typeof(doc) = 'object' AND jsonb_typeof(doc -> 'user_id') = 'string' THEN
        RETURN doc ->> 'user_id';
    END IF;
    RETURN NULL;
EXCEPTION WHEN others THEN
    RETURN NULL;
END
$$;
