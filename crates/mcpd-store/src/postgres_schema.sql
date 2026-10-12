-- mcpd Postgres schema.
--
-- The shape mirrors the embedded schema deliberately: same columns, same
-- names, same nanosecond BIGINT timestamps. A backend that renames things is a
-- backend whose queries have to be written twice and whose bugs only appear on
-- one of them.
--
-- Payload columns hold JSON as TEXT rather than JSONB, which is the one place
-- this schema declines a Postgres-native type. JSONB normalises the document
-- (it reorders keys and drops duplicates) and, more importantly, rejects a
-- string containing a NUL byte. Model output is arbitrary text and does
-- occasionally carry one, and an ingest that fails on the payload it exists to
-- record is a worse outcome than storing the document verbatim. Nothing queries
-- inside these columns; full-text search is handled below.

CREATE TABLE IF NOT EXISTS schema_version (
    version     INTEGER PRIMARY KEY,
    applied_at  BIGINT NOT NULL
);

-- One unit of work.
CREATE TABLE IF NOT EXISTS spans (
    trace_id                TEXT   NOT NULL,
    span_id                 TEXT   NOT NULL,
    parent_span_id          TEXT,
    external_parent_span_id TEXT,
    name                    TEXT   NOT NULL,
    kind                    TEXT   NOT NULL,
    status                  TEXT   NOT NULL,
    start_time              BIGINT NOT NULL,
    end_time                BIGINT,
    duration_ns             BIGINT,
    is_root                 BOOLEAN NOT NULL DEFAULT FALSE,
    is_event                BOOLEAN NOT NULL DEFAULT FALSE,
    entity_kind             TEXT,
    entity_id               TEXT,
    entity_name             TEXT,
    input                   TEXT,
    output                  TEXT,
    attributes              TEXT,
    metadata                TEXT,
    usage_json              TEXT,
    input_tokens            BIGINT,
    output_tokens           BIGINT,
    cost_json               TEXT,
    total_cost              DOUBLE PRECISION,
    error_json              TEXT,
    links_json              TEXT,
    tags                    TEXT,
    project                 TEXT   NOT NULL,
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
    received_at             BIGINT NOT NULL,
    PRIMARY KEY (trace_id, span_id)
);

CREATE INDEX IF NOT EXISTS spans_project_start  ON spans (project, start_time DESC);
CREATE INDEX IF NOT EXISTS spans_trace          ON spans (trace_id, start_time);
CREATE INDEX IF NOT EXISTS spans_project_kind   ON spans (project, kind, start_time DESC);
CREATE INDEX IF NOT EXISTS spans_project_status ON spans (project, status, start_time DESC);
CREATE INDEX IF NOT EXISTS spans_project_model  ON spans (project, model, start_time DESC);
CREATE INDEX IF NOT EXISTS spans_project_dur    ON spans (project, duration_ns DESC);
CREATE INDEX IF NOT EXISTS spans_received       ON spans (project, received_at);

-- Every correlation identifier a span carries, so "find the steps for this
-- request" is a lookup rather than a scan of the retention window.
--
-- Partial, and that is the whole reason there can be ten of them. Most of these
-- columns are NULL on most rows: a deployment that sets `request_id` usually
-- sets nothing else, and one that sets none of them pays for none of these
-- indexes at all. A full index over a column that is NULL on every row still
-- stores a heap pointer per span, which on the ingest path is ten writes bought
-- for nothing.
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

-- Full-text search over the payloads, which is the query that answers "find the
-- run where the model said that". A stored generated column rather than a
-- trigger, so it cannot fall out of step with the row it describes.
--
-- The 'simple' configuration is deliberate: it lower-cases and splits on word
-- boundaries but does not stem, and does not drop stop words. An English
-- stemmer would make searching a prompt for the literal token a user typed
-- unreliable, and these documents are not prose in one known language.
ALTER TABLE spans ADD COLUMN IF NOT EXISTS search tsvector
    GENERATED ALWAYS AS (
        to_tsvector('simple',
            coalesce(name, '') || ' ' || coalesce(input, '') || ' ' || coalesce(output, ''))
    ) STORED;

CREATE INDEX IF NOT EXISTS spans_search ON spans USING GIN (search);

-- The materialised run projection. Maintained on every span write so the list
-- view is one indexed read instead of an aggregate over every span.
--
-- Keyed by project and trace id together. A trace id crosses service
-- boundaries by design, so two projects that call each other hold spans of the
-- same trace, and each gets its own row built from its own spans. Before
-- schema version 5 the key was the trace id alone; `migrate` rebuilds a table
-- of that shape, which it can do because every column here is derived.
CREATE TABLE IF NOT EXISTS traces (
    trace_id         TEXT   NOT NULL,
    root_span_id     TEXT,
    name             TEXT   NOT NULL,
    kind             TEXT   NOT NULL,
    status           TEXT   NOT NULL,
    start_time       BIGINT NOT NULL,
    end_time         BIGINT,
    duration_ns      BIGINT,
    span_count       BIGINT NOT NULL DEFAULT 0,
    error_count      BIGINT NOT NULL DEFAULT 0,
    input_tokens     BIGINT NOT NULL DEFAULT 0,
    output_tokens    BIGINT NOT NULL DEFAULT 0,
    total_cost       DOUBLE PRECISION NOT NULL DEFAULT 0,
    currency         TEXT   NOT NULL DEFAULT 'USD',
    model_count      BIGINT NOT NULL DEFAULT 0,
    tool_count       BIGINT NOT NULL DEFAULT 0,
    tags             TEXT,
    project          TEXT   NOT NULL,
    environment      TEXT,
    service_name     TEXT,
    session_id       TEXT,
    thread_id        TEXT,
    user_id          TEXT,
    experiment_id    TEXT,
    release          TEXT,
    score_summary    TEXT,
    feedback_summary TEXT,
    min_score        DOUBLE PRECISION,
    is_complete      BOOLEAN NOT NULL DEFAULT FALSE,
    received_at      BIGINT NOT NULL,
    PRIMARY KEY (project, trace_id)
);

CREATE INDEX IF NOT EXISTS traces_project_start  ON traces (project, start_time DESC);
CREATE INDEX IF NOT EXISTS traces_project_status ON traces (project, status, start_time DESC);
CREATE INDEX IF NOT EXISTS traces_project_cost   ON traces (project, total_cost DESC);
CREATE INDEX IF NOT EXISTS traces_project_dur    ON traces (project, duration_ns DESC);
CREATE INDEX IF NOT EXISTS traces_session        ON traces (project, session_id, start_time DESC);
CREATE INDEX IF NOT EXISTS traces_user           ON traces (project, user_id, start_time DESC);
CREATE INDEX IF NOT EXISTS traces_received       ON traces (project, received_at);
CREATE INDEX IF NOT EXISTS traces_environment   ON traces (project, environment, start_time DESC)
    WHERE environment IS NOT NULL;
CREATE INDEX IF NOT EXISTS traces_service       ON traces (project, service_name, start_time DESC)
    WHERE service_name IS NOT NULL;
CREATE INDEX IF NOT EXISTS traces_thread        ON traces (project, thread_id, start_time DESC)
    WHERE thread_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS traces_experiment    ON traces (project, experiment_id, start_time DESC)
    WHERE experiment_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS traces_release       ON traces (project, release, start_time DESC)
    WHERE release IS NOT NULL;

CREATE TABLE IF NOT EXISTS logs (
    log_id       TEXT   PRIMARY KEY,
    trace_id     TEXT,
    span_id      TEXT,
    timestamp    BIGINT NOT NULL,
    level        TEXT   NOT NULL,
    severity     BIGINT NOT NULL,
    message      TEXT   NOT NULL,
    fields       TEXT,
    logger       TEXT,
    project      TEXT   NOT NULL,
    environment  TEXT,
    service_name TEXT,
    session_id   TEXT,
    user_id      TEXT,
    received_at  BIGINT NOT NULL
);

ALTER TABLE logs ADD COLUMN IF NOT EXISTS search tsvector
    GENERATED ALWAYS AS (
        to_tsvector('simple', coalesce(message, '') || ' ' || coalesce(fields, ''))
    ) STORED;

CREATE INDEX IF NOT EXISTS logs_trace        ON logs (trace_id, timestamp);
CREATE INDEX IF NOT EXISTS logs_project_time ON logs (project, timestamp DESC);
CREATE INDEX IF NOT EXISTS logs_project_sev  ON logs (project, severity, timestamp DESC);
CREATE INDEX IF NOT EXISTS logs_search       ON logs USING GIN (search);
CREATE INDEX IF NOT EXISTS logs_span         ON logs (span_id, timestamp)
    WHERE span_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS logs_environment  ON logs (project, environment, timestamp DESC)
    WHERE environment IS NOT NULL;
CREATE INDEX IF NOT EXISTS logs_service      ON logs (project, service_name, timestamp DESC)
    WHERE service_name IS NOT NULL;
CREATE INDEX IF NOT EXISTS logs_session      ON logs (project, session_id, timestamp DESC)
    WHERE session_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS logs_user         ON logs (project, user_id, timestamp DESC)
    WHERE user_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS metrics (
    metric_id   TEXT   PRIMARY KEY,
    name        TEXT   NOT NULL,
    kind        TEXT   NOT NULL,
    value       DOUBLE PRECISION NOT NULL,
    unit        TEXT,
    timestamp   BIGINT NOT NULL,
    labels      TEXT,
    buckets     TEXT,
    trace_id    TEXT,
    span_id     TEXT,
    project     TEXT   NOT NULL,
    environment TEXT,
    received_at BIGINT NOT NULL
);

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
    score_id       TEXT   PRIMARY KEY,
    trace_id       TEXT,
    span_id        TEXT,
    scorer_id      TEXT   NOT NULL,
    scorer_version TEXT,
    source         TEXT   NOT NULL,
    value          DOUBLE PRECISION NOT NULL,
    min_value      DOUBLE PRECISION,
    max_value      DOUBLE PRECISION,
    label          TEXT,
    reason         TEXT,
    score_trace_id TEXT,
    cost_json      TEXT,
    metadata       TEXT,
    timestamp      BIGINT NOT NULL,
    project        TEXT   NOT NULL,
    environment    TEXT,
    experiment_id  TEXT
);

CREATE INDEX IF NOT EXISTS scores_trace          ON scores (trace_id);
CREATE INDEX IF NOT EXISTS scores_project_scorer ON scores (project, scorer_id, timestamp DESC);
CREATE INDEX IF NOT EXISTS scores_span          ON scores (span_id, timestamp)
    WHERE span_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS scores_environment   ON scores (project, environment, timestamp DESC)
    WHERE environment IS NOT NULL;
CREATE INDEX IF NOT EXISTS scores_experiment    ON scores (project, experiment_id, timestamp DESC)
    WHERE experiment_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS feedback (
    feedback_id TEXT   PRIMARY KEY,
    trace_id    TEXT,
    span_id     TEXT,
    kind        TEXT   NOT NULL,
    value       TEXT   NOT NULL,
    comment     TEXT,
    author_id   TEXT,
    author_kind TEXT,
    timestamp   BIGINT NOT NULL,
    metadata    TEXT,
    project     TEXT   NOT NULL,
    environment TEXT
);

CREATE INDEX IF NOT EXISTS feedback_trace        ON feedback (trace_id);
CREATE INDEX IF NOT EXISTS feedback_project_time ON feedback (project, timestamp DESC);
CREATE INDEX IF NOT EXISTS feedback_span         ON feedback (span_id, timestamp)
    WHERE span_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS feedback_environment  ON feedback (project, environment, timestamp DESC)
    WHERE environment IS NOT NULL;

-- What a filter box can offer, kept as data arrives.
--
-- The alternative is `SELECT DISTINCT environment FROM spans`, which reads every
-- span in the retention window to produce a list of four strings, and is asked
-- again on every keystroke. This table holds one row per distinct value instead,
-- so the same question is an index scan over a table the size of the answer.
--
-- `key` is the empty string for every facet but `metric_label_value`, where it
-- is the label key the value belongs to. A nullable column would put NULL in the
-- primary key, which Postgres does not allow.
--
-- `last_seen` is when a signal carrying this value was last written. It ranks a
-- value that is still live above one nothing has reported for a month; it is not
-- what the list is ordered by, because a list somebody is reading must not
-- reshuffle under them as data lands.
CREATE TABLE IF NOT EXISTS discovery (
    project   TEXT   NOT NULL,
    facet     TEXT   NOT NULL,
    key       TEXT   NOT NULL DEFAULT '',
    value     TEXT   NOT NULL,
    last_seen BIGINT NOT NULL,
    PRIMARY KEY (project, facet, key, value)
);

-- Retention reaches this table by `last_seen`, so a value nothing has carried
-- since before the window leaves with the rows that named it.
CREATE INDEX IF NOT EXISTS discovery_seen ON discovery (project, last_seen);

-- Typeahead is a prefix match, and a prefix match is `LIKE 'x%'`. The primary
-- key above cannot serve one: its `value` column is indexed under the database
-- collation, and `LIKE` is defined on bytes, so the planner will not use it
-- unless the cluster happens to be initialised with the C locale.
-- `text_pattern_ops` indexes the column by byte order instead, which is exactly
-- what `LIKE 'x%'` needs, whatever collation the cluster was created with.
--
-- The range form the other two backends use (`value >= 'g' AND value < 'h'`) is
-- not correct here for the same reason: under a linguistic collation that upper
-- bound can sort before a value that really does start with the prefix, and the
-- match is then silently missing from the list.
CREATE INDEX IF NOT EXISTS discovery_prefix
    ON discovery (project, facet, key, value text_pattern_ops);

-- Idempotency. A client that retries a batch after a timeout must not double
-- count; we remember accepted keys and skip replays.
CREATE TABLE IF NOT EXISTS ingest_keys (
    key     TEXT   PRIMARY KEY,
    seen_at BIGINT NOT NULL
);

CREATE INDEX IF NOT EXISTS ingest_keys_seen ON ingest_keys (seen_at);

CREATE TABLE IF NOT EXISTS datasets (
    dataset_id  TEXT   PRIMARY KEY,
    name        TEXT   NOT NULL,
    description TEXT,
    item_count  BIGINT NOT NULL DEFAULT 0,
    created_at  BIGINT NOT NULL,
    project     TEXT   NOT NULL,
    environment TEXT
);

CREATE INDEX IF NOT EXISTS datasets_project ON datasets (project, created_at DESC);

CREATE TABLE IF NOT EXISTS dataset_items (
    item_id         TEXT PRIMARY KEY,
    dataset_id      TEXT NOT NULL,
    input           TEXT NOT NULL,
    expected        TEXT,
    metadata        TEXT,
    source_trace_id TEXT
);

CREATE INDEX IF NOT EXISTS dataset_items_set ON dataset_items (dataset_id);

CREATE TABLE IF NOT EXISTS experiments (
    experiment_id TEXT   PRIMARY KEY,
    name          TEXT   NOT NULL,
    dataset_id    TEXT   NOT NULL,
    status        TEXT   NOT NULL,
    started_at    BIGINT,
    finished_at   BIGINT,
    item_count    BIGINT NOT NULL DEFAULT 0,
    summary       TEXT,
    baseline_id   TEXT,
    cost_json     TEXT,
    error         TEXT,
    metadata      TEXT,
    project       TEXT   NOT NULL,
    environment   TEXT
);

CREATE INDEX IF NOT EXISTS experiments_project ON experiments (project, started_at DESC);

-- The audit log: who read a payload, and who deleted one.
--
-- Not covered by the retention pass, on purpose. A log that deleted itself on
-- the same schedule as the data it describes could not answer who read the data
-- that has since gone, which is the question it exists to answer.
--
-- `principal` is a truncated digest of the key that acted, never the key.
CREATE TABLE IF NOT EXISTS audit_log (
    audit_id  TEXT   PRIMARY KEY,
    timestamp BIGINT NOT NULL,
    principal TEXT   NOT NULL,
    subject   TEXT   NOT NULL DEFAULT '',
    action    TEXT   NOT NULL,
    project   TEXT   NOT NULL,
    resource  TEXT,
    result    TEXT   NOT NULL
);

CREATE INDEX IF NOT EXISTS audit_project_time ON audit_log (project, timestamp DESC);
CREATE INDEX IF NOT EXISTS audit_action       ON audit_log (project, action, timestamp DESC);

-- One sentence recorded against one trace along one aspect.
--
-- `text` is payload: it arrives through the same redactor as every other
-- signal, retention sweeps it, and erasing a subject erases it. Nothing else in
-- the analysis tables holds prose, which is what makes that one deletion
-- enough.
--
-- `entity_kind` and `entity_name` are copied from the trace's root span when
-- the note is written, because every read of the notes is grouped by entity and
-- because the ClickHouse backend joins nowhere and must answer the same
-- question. Both are NULL when the note arrived before its trace did, and such
-- a note is readable by trace id and absent from the entity roll-ups.
--
-- `sketch` is 512 bytes: 64 big-endian integers, ascending, which is what the
-- clustering compares.
CREATE TABLE IF NOT EXISTS aspect_notes (
    note_id            TEXT   PRIMARY KEY,
    trace_id           TEXT   NOT NULL,
    aspect             TEXT   NOT NULL,
    text               TEXT   NOT NULL,
    definition_version BIGINT NOT NULL DEFAULT 1,
    model              TEXT,
    created_at         BIGINT NOT NULL,
    entity_kind        TEXT,
    entity_name        TEXT,
    trace_started_at   BIGINT,
    sketch             BYTEA,
    project            TEXT   NOT NULL,
    environment        TEXT,
    service_name       TEXT,
    service_version    TEXT,
    session_id         TEXT,
    thread_id          TEXT,
    user_id            TEXT,
    resource_id        TEXT,
    run_id             TEXT,
    request_id         TEXT,
    experiment_id      TEXT,
    release            TEXT
);

CREATE INDEX IF NOT EXISTS aspect_notes_trace  ON aspect_notes (trace_id);
CREATE INDEX IF NOT EXISTS aspect_notes_pass   ON aspect_notes
    (project, aspect, created_at DESC);
CREATE INDEX IF NOT EXISTS aspect_notes_entity ON aspect_notes
    (project, entity_kind, entity_name, created_at DESC)
    WHERE entity_name IS NOT NULL;
CREATE INDEX IF NOT EXISTS aspect_notes_user   ON aspect_notes (project, user_id)
    WHERE user_id IS NOT NULL;

-- One clustering pass over one entity, covering a moving window of notes.
CREATE TABLE IF NOT EXISTS theme_snapshots (
    snapshot_id  TEXT   PRIMARY KEY,
    project      TEXT   NOT NULL,
    entity_kind  TEXT   NOT NULL,
    entity_name  TEXT   NOT NULL,
    aspects      TEXT   NOT NULL,
    ordinal      BIGINT NOT NULL,
    cutoff_at    BIGINT NOT NULL,
    window_from  BIGINT NOT NULL,
    window_to    BIGINT NOT NULL,
    trace_count  BIGINT NOT NULL,
    note_count   BIGINT NOT NULL,
    capped       BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE INDEX IF NOT EXISTS theme_snapshots_series ON theme_snapshots
    (project, entity_kind, entity_name, cutoff_at DESC);

-- A cluster of aspect notes, in one snapshot along one aspect.
--
-- `cutoff_at` and the window bounds are copied from the snapshot, so a history
-- is one indexed scan of this table rather than a join. The four lineage
-- columns are JSON arrays, both directions stored, so learning what became of a
-- theme never means reading every other theme in the same snapshot.
CREATE TABLE IF NOT EXISTS themes (
    snapshot_id    TEXT   NOT NULL,
    aspect         TEXT   NOT NULL,
    theme_id       TEXT   NOT NULL,
    project        TEXT   NOT NULL,
    entity_kind    TEXT   NOT NULL,
    entity_name    TEXT   NOT NULL,
    label          TEXT   NOT NULL,
    label_note_id  TEXT   NOT NULL,
    label_trace_id TEXT   NOT NULL,
    state          TEXT   NOT NULL,
    note_count     BIGINT NOT NULL,
    trace_count    BIGINT NOT NULL,
    coverage       DOUBLE PRECISION NOT NULL,
    cutoff_at      BIGINT NOT NULL,
    window_from    BIGINT NOT NULL,
    window_to      BIGINT NOT NULL,
    split_from     TEXT,
    split_into     TEXT,
    merged_from    TEXT,
    merged_into    TEXT,
    PRIMARY KEY (snapshot_id, aspect, theme_id)
);

CREATE INDEX IF NOT EXISTS themes_history ON themes
    (project, entity_kind, entity_name, aspect, theme_id, cutoff_at);
CREATE INDEX IF NOT EXISTS themes_label_trace ON themes (label_trace_id);

-- Which note joined which theme, in one snapshot. Ids and one integer, and no
-- prose: the sentence stays in `aspect_notes`, so erasing a subject removes the
-- text from every theme's examples by removing it once.
CREATE TABLE IF NOT EXISTS theme_members (
    snapshot_id      TEXT   NOT NULL,
    aspect           TEXT   NOT NULL,
    theme_id         TEXT   NOT NULL,
    note_id          TEXT   NOT NULL,
    trace_id         TEXT   NOT NULL,
    project          TEXT   NOT NULL,
    similarity       BIGINT NOT NULL,
    trace_started_at BIGINT NOT NULL,
    PRIMARY KEY (snapshot_id, aspect, theme_id, note_id)
);

CREATE INDEX IF NOT EXISTS theme_members_examples ON theme_members
    (snapshot_id, aspect, theme_id, similarity DESC, note_id);
CREATE INDEX IF NOT EXISTS theme_members_flow ON theme_members
    (snapshot_id, aspect, trace_id);
CREATE INDEX IF NOT EXISTS theme_members_trace ON theme_members (trace_id);
