-- mcpd embedded schema.
--
-- Timestamps are INTEGER nanoseconds since the Unix epoch. Storing them as
-- text would make every range scan a string comparison, and storing them as
-- REAL would lose precision above 2^53 — which nanosecond timestamps pass in
-- 1970. Integers sort correctly, index well, and stay exact.
--
-- Payload columns (input, output, attributes, metadata) hold JSON text. They
-- are written already redacted; nothing in this file is responsible for that.

PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;

CREATE TABLE IF NOT EXISTS schema_version (
    version     INTEGER PRIMARY KEY,
    applied_at  INTEGER NOT NULL
);

-- One unit of work.
CREATE TABLE IF NOT EXISTS spans (
    trace_id                TEXT    NOT NULL,
    span_id                 TEXT    NOT NULL,
    parent_span_id          TEXT,
    external_parent_span_id TEXT,
    name                    TEXT    NOT NULL,
    kind                    TEXT    NOT NULL,
    status                  TEXT    NOT NULL,
    start_time              INTEGER NOT NULL,
    end_time                INTEGER,
    duration_ns             INTEGER,
    is_root                 INTEGER NOT NULL DEFAULT 0,
    is_event                INTEGER NOT NULL DEFAULT 0,
    entity_kind             TEXT,
    entity_id               TEXT,
    entity_name             TEXT,
    input                   TEXT,
    output                  TEXT,
    attributes              TEXT,
    metadata                TEXT,
    usage_json              TEXT,
    input_tokens            INTEGER,
    output_tokens           INTEGER,
    cost_json               TEXT,
    total_cost              REAL,
    error_json              TEXT,
    links_json              TEXT,
    tags                    TEXT,
    project                 TEXT    NOT NULL,
    environment             TEXT,
    service_name            TEXT,
    service_version         TEXT,
    session_id              TEXT,
    thread_id               TEXT,
    user_id                 TEXT,
    resource_id             TEXT,
    run_id                  TEXT,
    request_id              TEXT,
    experiment_id           TEXT,
    release                 TEXT,
    model                   TEXT,
    provider                TEXT,
    received_at             INTEGER NOT NULL,
    PRIMARY KEY (trace_id, span_id)
) WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS spans_project_start   ON spans (project, start_time DESC);
CREATE INDEX IF NOT EXISTS spans_trace           ON spans (trace_id, start_time);
CREATE INDEX IF NOT EXISTS spans_project_kind    ON spans (project, kind, start_time DESC);
CREATE INDEX IF NOT EXISTS spans_project_status  ON spans (project, status, start_time DESC);
CREATE INDEX IF NOT EXISTS spans_project_model   ON spans (project, model, start_time DESC);
CREATE INDEX IF NOT EXISTS spans_project_dur     ON spans (project, duration_ns DESC);
CREATE INDEX IF NOT EXISTS spans_received        ON spans (project, received_at);

-- Every correlation identifier a span carries, so "find the steps for this
-- request" is a lookup rather than a scan of the retention window.
--
-- Partial, and that is the whole reason there can be ten of them. Most of these
-- columns are NULL on most rows: a deployment that sets `request_id` usually
-- sets nothing else, and one that sets none of them pays for none of these
-- indexes at all. A full index over a column that is NULL on every row still
-- stores a row per span, which on the ingest path is ten writes bought for
-- nothing.
CREATE INDEX IF NOT EXISTS spans_environment ON spans (project, environment, start_time DESC)
    WHERE environment IS NOT NULL;
CREATE INDEX IF NOT EXISTS spans_service     ON spans (project, service_name, start_time DESC)
    WHERE service_name IS NOT NULL;
CREATE INDEX IF NOT EXISTS spans_session     ON spans (project, session_id, start_time DESC)
    WHERE session_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS spans_thread      ON spans (project, thread_id, start_time DESC)
    WHERE thread_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS spans_user        ON spans (project, user_id, start_time DESC)
    WHERE user_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS spans_resource    ON spans (project, resource_id, start_time DESC)
    WHERE resource_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS spans_run         ON spans (project, run_id, start_time DESC)
    WHERE run_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS spans_request     ON spans (project, request_id, start_time DESC)
    WHERE request_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS spans_experiment  ON spans (project, experiment_id, start_time DESC)
    WHERE experiment_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS spans_release     ON spans (project, release, start_time DESC)
    WHERE release IS NOT NULL;

-- The materialised run projection. Maintained on every span write so the list
-- view is one indexed read instead of an aggregate over every span.
--
-- Keyed by project and trace id together. A trace id crosses service
-- boundaries by design, so two projects that call each other hold spans of the
-- same trace, and each gets its own row built from its own spans. Before
-- schema version 5 the key was the trace id alone; `migrate` rebuilds a table
-- of that shape, which it can do because every column here is derived.
CREATE TABLE IF NOT EXISTS traces (
    trace_id        TEXT    NOT NULL,
    root_span_id    TEXT,
    name            TEXT    NOT NULL,
    kind            TEXT    NOT NULL,
    status          TEXT    NOT NULL,
    start_time      INTEGER NOT NULL,
    end_time        INTEGER,
    duration_ns     INTEGER,
    span_count      INTEGER NOT NULL DEFAULT 0,
    error_count     INTEGER NOT NULL DEFAULT 0,
    input_tokens    INTEGER NOT NULL DEFAULT 0,
    output_tokens   INTEGER NOT NULL DEFAULT 0,
    total_cost      REAL    NOT NULL DEFAULT 0,
    currency        TEXT    NOT NULL DEFAULT 'USD',
    model_count     INTEGER NOT NULL DEFAULT 0,
    tool_count      INTEGER NOT NULL DEFAULT 0,
    tags            TEXT,
    project         TEXT    NOT NULL,
    environment     TEXT,
    service_name    TEXT,
    session_id      TEXT,
    thread_id       TEXT,
    user_id         TEXT,
    experiment_id   TEXT,
    release         TEXT,
    score_summary   TEXT,
    feedback_summary TEXT,
    min_score       REAL,
    is_complete     INTEGER NOT NULL DEFAULT 0,
    received_at     INTEGER NOT NULL,
    PRIMARY KEY (project, trace_id)
) WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS traces_project_start  ON traces (project, start_time DESC);
CREATE INDEX IF NOT EXISTS traces_project_status ON traces (project, status, start_time DESC);
CREATE INDEX IF NOT EXISTS traces_project_cost   ON traces (project, total_cost DESC);
CREATE INDEX IF NOT EXISTS traces_project_dur    ON traces (project, duration_ns DESC);
CREATE INDEX IF NOT EXISTS traces_session        ON traces (project, session_id, start_time DESC);
CREATE INDEX IF NOT EXISTS traces_user           ON traces (project, user_id, start_time DESC);
CREATE INDEX IF NOT EXISTS traces_received       ON traces (project, received_at);
CREATE INDEX IF NOT EXISTS traces_environment    ON traces (project, environment, start_time DESC)
    WHERE environment IS NOT NULL;
CREATE INDEX IF NOT EXISTS traces_service        ON traces (project, service_name, start_time DESC)
    WHERE service_name IS NOT NULL;
CREATE INDEX IF NOT EXISTS traces_thread         ON traces (project, thread_id, start_time DESC)
    WHERE thread_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS traces_experiment     ON traces (project, experiment_id, start_time DESC)
    WHERE experiment_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS traces_release        ON traces (project, release, start_time DESC)
    WHERE release IS NOT NULL;

CREATE TABLE IF NOT EXISTS logs (
    log_id      TEXT    PRIMARY KEY,
    trace_id    TEXT,
    span_id     TEXT,
    timestamp   INTEGER NOT NULL,
    level       TEXT    NOT NULL,
    severity    INTEGER NOT NULL,
    message     TEXT    NOT NULL,
    fields      TEXT,
    logger      TEXT,
    project     TEXT    NOT NULL,
    environment TEXT,
    service_name TEXT,
    session_id  TEXT,
    user_id     TEXT,
    received_at INTEGER NOT NULL
) WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS logs_trace          ON logs (trace_id, timestamp);
CREATE INDEX IF NOT EXISTS logs_project_time   ON logs (project, timestamp DESC);
CREATE INDEX IF NOT EXISTS logs_project_sev    ON logs (project, severity, timestamp DESC);
CREATE INDEX IF NOT EXISTS logs_span           ON logs (span_id, timestamp)
    WHERE span_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS logs_environment    ON logs (project, environment, timestamp DESC)
    WHERE environment IS NOT NULL;
CREATE INDEX IF NOT EXISTS logs_service        ON logs (project, service_name, timestamp DESC)
    WHERE service_name IS NOT NULL;
CREATE INDEX IF NOT EXISTS logs_session        ON logs (project, session_id, timestamp DESC)
    WHERE session_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS logs_user           ON logs (project, user_id, timestamp DESC)
    WHERE user_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS metrics (
    metric_id   TEXT    PRIMARY KEY,
    name        TEXT    NOT NULL,
    kind        TEXT    NOT NULL,
    value       REAL    NOT NULL,
    unit        TEXT,
    timestamp   INTEGER NOT NULL,
    labels      TEXT,
    buckets     TEXT,
    trace_id    TEXT,
    span_id     TEXT,
    project     TEXT    NOT NULL,
    environment TEXT,
    received_at INTEGER NOT NULL
) WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS metrics_project_name_time ON metrics (project, name, timestamp);

-- A metric point names the run and the step it came from, which is what turns a
-- spike on a chart into a run you can open. Without these that link is a scan.
CREATE INDEX IF NOT EXISTS metrics_trace       ON metrics (trace_id, timestamp)
    WHERE trace_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS metrics_span        ON metrics (span_id, timestamp)
    WHERE span_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS metrics_environment ON metrics (project, environment, timestamp DESC)
    WHERE environment IS NOT NULL;

CREATE TABLE IF NOT EXISTS scores (
    score_id        TEXT    PRIMARY KEY,
    trace_id        TEXT,
    span_id         TEXT,
    scorer_id       TEXT    NOT NULL,
    scorer_version  TEXT,
    source          TEXT    NOT NULL,
    value           REAL    NOT NULL,
    min_value       REAL,
    max_value       REAL,
    label           TEXT,
    reason          TEXT,
    score_trace_id  TEXT,
    cost_json       TEXT,
    metadata        TEXT,
    timestamp       INTEGER NOT NULL,
    project         TEXT    NOT NULL,
    environment     TEXT,
    experiment_id   TEXT
) WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS scores_trace           ON scores (trace_id);
CREATE INDEX IF NOT EXISTS scores_project_scorer  ON scores (project, scorer_id, timestamp DESC);
CREATE INDEX IF NOT EXISTS scores_span            ON scores (span_id, timestamp)
    WHERE span_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS scores_environment     ON scores (project, environment, timestamp DESC)
    WHERE environment IS NOT NULL;
CREATE INDEX IF NOT EXISTS scores_experiment      ON scores (project, experiment_id, timestamp DESC)
    WHERE experiment_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS feedback (
    feedback_id TEXT    PRIMARY KEY,
    trace_id    TEXT,
    span_id     TEXT,
    kind        TEXT    NOT NULL,
    value       TEXT    NOT NULL,
    comment     TEXT,
    author_id   TEXT,
    author_kind TEXT,
    timestamp   INTEGER NOT NULL,
    metadata    TEXT,
    project     TEXT    NOT NULL,
    environment TEXT
) WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS feedback_trace          ON feedback (trace_id);
CREATE INDEX IF NOT EXISTS feedback_project_time   ON feedback (project, timestamp DESC);
CREATE INDEX IF NOT EXISTS feedback_span           ON feedback (span_id, timestamp)
    WHERE span_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS feedback_environment    ON feedback (project, environment, timestamp DESC)
    WHERE environment IS NOT NULL;

-- Idempotency. A client that retries a batch after a timeout must not double
-- count; we remember accepted keys and skip replays.
CREATE TABLE IF NOT EXISTS ingest_keys (
    key      TEXT    PRIMARY KEY,
    seen_at  INTEGER NOT NULL
) WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS ingest_keys_seen ON ingest_keys (seen_at);

-- Datasets: a fixed set of cases an experiment runs against. Capturing a
-- production trace into one is how a bug report becomes a permanent test.
CREATE TABLE IF NOT EXISTS datasets (
    dataset_id  TEXT    PRIMARY KEY,
    name        TEXT    NOT NULL,
    description TEXT,
    item_count  INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL,
    project     TEXT    NOT NULL,
    environment TEXT
) WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS datasets_project ON datasets (project, created_at DESC);

CREATE TABLE IF NOT EXISTS dataset_items (
    item_id         TEXT PRIMARY KEY,
    dataset_id      TEXT NOT NULL,
    input           TEXT NOT NULL,
    expected        TEXT,
    metadata        TEXT,
    source_trace_id TEXT
) WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS dataset_items_set ON dataset_items (dataset_id);

CREATE TABLE IF NOT EXISTS experiments (
    experiment_id TEXT    PRIMARY KEY,
    name          TEXT    NOT NULL,
    dataset_id    TEXT    NOT NULL,
    status        TEXT    NOT NULL,
    started_at    INTEGER,
    finished_at   INTEGER,
    item_count    INTEGER NOT NULL DEFAULT 0,
    summary       TEXT,
    baseline_id   TEXT,
    cost_json     TEXT,
    error         TEXT,
    metadata      TEXT,
    project       TEXT    NOT NULL,
    environment   TEXT
) WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS experiments_project ON experiments (project, started_at DESC);

-- What a filter box can offer, kept as data arrives.
--
-- The alternative is `SELECT DISTINCT environment FROM spans`, which reads
-- every span in the retention window to produce a list of four strings, and is
-- asked again on every keystroke. This table holds one row per distinct value
-- instead, so the same question is a prefix scan of its primary key.
--
-- `key` is the empty string for every facet but `metric_label_value`, where it
-- is the label key the value belongs to. A nullable column would put NULL in
-- the primary key, and NULLs do not compare equal in SQLite, so every upsert
-- would insert a second row rather than updating the first.
--
-- `last_seen` is when a signal carrying this value was last written. It ranks a
-- value that is still live above one nothing has reported for a month; it is
-- not what the list is ordered by, because a list somebody is reading must not
-- reshuffle under them as data lands.
CREATE TABLE IF NOT EXISTS discovery (
    project     TEXT    NOT NULL,
    facet       TEXT    NOT NULL,
    key         TEXT    NOT NULL DEFAULT '',
    value       TEXT    NOT NULL,
    last_seen   INTEGER NOT NULL,
    PRIMARY KEY (project, facet, key, value)
) WITHOUT ROWID;

-- Retention reaches this table by `last_seen`, so a value nothing has carried
-- since before the window leaves with the rows that named it.
CREATE INDEX IF NOT EXISTS discovery_seen ON discovery (project, last_seen);

-- Full-text search over the payloads. This is the query that answers "find the
-- run where the model said that", and it is why prompts are worth storing.
CREATE VIRTUAL TABLE IF NOT EXISTS spans_fts USING fts5(
    trace_id UNINDEXED,
    span_id  UNINDEXED,
    project  UNINDEXED,
    name,
    input,
    output,
    tokenize = 'unicode61 remove_diacritics 2'
);

CREATE VIRTUAL TABLE IF NOT EXISTS logs_fts USING fts5(
    log_id  UNINDEXED,
    project UNINDEXED,
    message,
    fields,
    tokenize = 'unicode61 remove_diacritics 2'
);

-- The audit log: who read a payload, and who deleted one.
--
-- Not covered by the retention pass, on purpose. A log that deleted itself on
-- the same schedule as the data it describes could not answer who read the data
-- that has since gone, which is the question it exists to answer. Rows are
-- small and only written by the handlers that hand payloads back, so the table
-- grows with reads of prompts rather than with traffic.
--
-- `principal` is a truncated digest of the key that acted. The key itself is
-- never written here: an audit log that stores credentials is a credential
-- store nobody guards.
CREATE TABLE IF NOT EXISTS audit_log (
    audit_id    TEXT    PRIMARY KEY,
    timestamp   INTEGER NOT NULL,
    principal   TEXT    NOT NULL,
    subject     TEXT    NOT NULL DEFAULT '',
    action      TEXT    NOT NULL,
    project     TEXT    NOT NULL,
    resource    TEXT,
    result      TEXT    NOT NULL
) WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS audit_project_time ON audit_log (project, timestamp DESC);
CREATE INDEX IF NOT EXISTS audit_action       ON audit_log (project, action, timestamp DESC);

-- One sentence recorded against one trace along one aspect.
--
-- `text` is payload. It arrives through the same redactor as every other
-- signal, retention sweeps it, and erasing a subject erases it. Nothing else
-- in the analysis tables holds prose, which is what makes that one deletion
-- enough.
--
-- `entity_kind` and `entity_name` are copied from the trace's root span when
-- the note is written. A note is a sentence about a trace and which agent that
-- trace belongs to is a property of the trace, so this is a denormalisation:
-- it is here because every read of the notes is grouped by entity, and because
-- the ClickHouse backend joins nowhere and must be able to answer the same
-- question. Both are NULL when the note arrived before its trace did, and such
-- a note is readable by trace id and absent from the entity roll-ups, which is
-- the honest answer rather than filing it under an entity nobody named.
--
-- `sketch` is 512 bytes: 64 big-endian integers, ascending. It is what the
-- clustering compares, it is written once here rather than recomputed on every
-- pass, and it is deliberately not a field of the wire entity, because a caller
-- can do nothing with it and publishing it would fix the hashing where it
-- stands today.
CREATE TABLE IF NOT EXISTS aspect_notes (
    note_id             TEXT    PRIMARY KEY,
    trace_id            TEXT    NOT NULL,
    aspect              TEXT    NOT NULL,
    text                TEXT    NOT NULL,
    definition_version  INTEGER NOT NULL DEFAULT 1,
    model               TEXT,
    created_at          INTEGER NOT NULL,
    entity_kind         TEXT,
    entity_name         TEXT,
    trace_started_at    INTEGER,
    sketch              BLOB,
    project             TEXT    NOT NULL,
    environment         TEXT,
    service_name        TEXT,
    service_version     TEXT,
    session_id          TEXT,
    thread_id           TEXT,
    user_id             TEXT,
    resource_id         TEXT,
    run_id              TEXT,
    request_id          TEXT,
    experiment_id       TEXT,
    release             TEXT
) WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS aspect_notes_trace   ON aspect_notes (trace_id);
-- One aspect's notes newest first, which is the order a clustering pass reads
-- them in and the order the cap drops the oldest from.
CREATE INDEX IF NOT EXISTS aspect_notes_pass    ON aspect_notes
    (project, aspect, created_at DESC);
-- The entity roll-up: one scan per entity rather than per note.
CREATE INDEX IF NOT EXISTS aspect_notes_entity  ON aspect_notes
    (project, entity_kind, entity_name, created_at DESC)
    WHERE entity_name IS NOT NULL;
CREATE INDEX IF NOT EXISTS aspect_notes_user    ON aspect_notes (project, user_id)
    WHERE user_id IS NOT NULL;

-- One clustering pass over one entity, covering a moving window of notes.
--
-- `aspects` is a JSON array of the aspect names the pass covered. An aspect
-- with too few notes when the pass ran is absent from it, which means it is
-- still collecting rather than that anything failed.
CREATE TABLE IF NOT EXISTS theme_snapshots (
    snapshot_id  TEXT    PRIMARY KEY,
    project      TEXT    NOT NULL,
    entity_kind  TEXT    NOT NULL,
    entity_name  TEXT    NOT NULL,
    aspects      TEXT    NOT NULL,
    ordinal      INTEGER NOT NULL,
    cutoff_at    INTEGER NOT NULL,
    window_from  INTEGER NOT NULL,
    window_to    INTEGER NOT NULL,
    trace_count  INTEGER NOT NULL,
    note_count   INTEGER NOT NULL,
    capped       INTEGER NOT NULL DEFAULT 0
) WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS theme_snapshots_series ON theme_snapshots
    (project, entity_kind, entity_name, cutoff_at DESC);

-- A cluster of aspect notes, in one snapshot along one aspect.
--
-- `cutoff_at`, `window_from` and `window_to` are copied from the snapshot onto
-- every theme. A history is a read of one theme across every snapshot that held
-- it, ordered by time, and copying three integers is what keeps that a single
-- indexed scan instead of the one join this crate does not do anywhere.
--
-- The four lineage columns are JSON arrays and both directions are stored.
-- Deriving `split_into` on read would mean scanning every other theme in the
-- same snapshot to answer a question about one of them.
--
-- A theme with `state = 'death'` is a tombstone: it holds no notes here and is
-- excluded from every read but a history, which is the only place a death is
-- reachable from.
CREATE TABLE IF NOT EXISTS themes (
    snapshot_id     TEXT    NOT NULL,
    aspect          TEXT    NOT NULL,
    theme_id        TEXT    NOT NULL,
    project         TEXT    NOT NULL,
    entity_kind     TEXT    NOT NULL,
    entity_name     TEXT    NOT NULL,
    label           TEXT    NOT NULL,
    label_note_id   TEXT    NOT NULL,
    label_trace_id  TEXT    NOT NULL,
    state           TEXT    NOT NULL,
    note_count      INTEGER NOT NULL,
    trace_count     INTEGER NOT NULL,
    coverage        REAL    NOT NULL,
    cutoff_at       INTEGER NOT NULL,
    window_from     INTEGER NOT NULL,
    window_to       INTEGER NOT NULL,
    split_from      TEXT,
    split_into      TEXT,
    merged_from     TEXT,
    merged_into     TEXT,
    PRIMARY KEY (snapshot_id, aspect, theme_id)
) WITHOUT ROWID;

-- One theme across every snapshot that holds it, which is what a history reads.
CREATE INDEX IF NOT EXISTS themes_history ON themes
    (project, entity_kind, entity_name, aspect, theme_id, cutoff_at);
CREATE INDEX IF NOT EXISTS themes_label_trace ON themes (label_trace_id);

-- Which note joined which theme, in one snapshot.
--
-- Ids and one integer, and no prose: the sentence stays in `aspect_notes` and
-- is read from there, so erasing a subject removes the text from every theme's
-- examples by removing it once. `theme_id` is the literal `noise` for a note
-- that joined no theme, which makes reading a stage's noise the same lookup as
-- reading a theme rather than a second code path.
CREATE TABLE IF NOT EXISTS theme_members (
    snapshot_id      TEXT    NOT NULL,
    aspect           TEXT    NOT NULL,
    theme_id         TEXT    NOT NULL,
    note_id          TEXT    NOT NULL,
    trace_id         TEXT    NOT NULL,
    project          TEXT    NOT NULL,
    similarity       INTEGER NOT NULL,
    trace_started_at INTEGER NOT NULL,
    PRIMARY KEY (snapshot_id, aspect, theme_id, note_id)
) WITHOUT ROWID;

-- The examples page: most like the theme's label first, the note id breaking a
-- tie so the order is total and the same on every read and every replica.
CREATE INDEX IF NOT EXISTS theme_members_examples ON theme_members
    (snapshot_id, aspect, theme_id, similarity DESC, note_id);
-- The flow: every membership of one snapshot, to pair one trace's themes across
-- stages.
CREATE INDEX IF NOT EXISTS theme_members_flow ON theme_members
    (snapshot_id, aspect, trace_id);
CREATE INDEX IF NOT EXISTS theme_members_trace ON theme_members (trace_id);
