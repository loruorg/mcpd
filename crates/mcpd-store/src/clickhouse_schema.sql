CREATE DATABASE IF NOT EXISTS mcpd;

-- Two things about this file are worth reading before changing it.
--
-- Partitions are one per day, on the signal's own anchor time. A month was the
-- unit before, and a month is the wrong unit for retention: expiring 90 days of
-- data by month either keeps up to 30 days too much or drops a partition that
-- still holds live rows, so the pass fell back to deleting rows one by one.
-- With a day partition an expiry is a metadata operation on one part. A part
-- per day per table is a size ClickHouse is comfortable with at these
-- retentions, and inserts are per batch rather than per day, so nothing here
-- multiplies the number of parts a merge has to walk.
--
-- The `INDEX` lines are data-skipping indexes on every correlation identifier,
-- not btrees. A bloom filter is the right shape for a high-cardinality id: it
-- says which granules cannot hold the value, which is what makes "find the
-- steps for this request" read a few granules rather than the window. The two
-- LowCardinality columns take a `set` index instead, because a bloom filter
-- over four distinct environments skips nothing.
--
-- Neither reaches a table that already exists: `CREATE TABLE IF NOT EXISTS` is
-- a no-op against one, and ClickHouse cannot change a partition key in place.
-- A deployment created before this file changed keeps its month partitions and
-- its old index set, and `capabilities()` reports what the table actually has
-- rather than what this file says.

CREATE TABLE IF NOT EXISTS mcpd.spans (
    trace_id                String,
    span_id                 String,
    parent_span_id          String,
    external_parent_span_id String,
    name                    LowCardinality(String),
    kind                    LowCardinality(String),
    status                  LowCardinality(String),
    start_time              DateTime64(9, 'UTC'),
    end_time                Nullable(DateTime64(9, 'UTC')),
    duration_ns             Nullable(UInt64),
    is_root                 UInt8,
    is_event                UInt8,
    entity_kind             LowCardinality(String),
    entity_id               String,
    entity_name             String,
    input                   String,
    output                  String,
    attributes              String,
    metadata                String,
    usage_json              String,
    input_tokens            Nullable(UInt64),
    output_tokens           Nullable(UInt64),
    cost_json               String,
    total_cost              Nullable(Float64),
    error_json              String,
    links_json              String,
    tags                    Array(String),
    project                 LowCardinality(String),
    environment             LowCardinality(String),
    service_name            LowCardinality(String),
    service_version         String,
    session_id              String,
    thread_id               String,
    user_id                 String,
    resource_id             String,
    run_id                  String,
    request_id              String,
    experiment_id           String,
    release                 String,
    model                   LowCardinality(String),
    provider                LowCardinality(String),
    received_at             DateTime64(9, 'UTC'),
    INDEX idx_name  name  TYPE bloom_filter GRANULARITY 4,
    INDEX idx_text (input, output) TYPE tokenbf_v1(32768, 3, 0) GRANULARITY 4,
    INDEX idx_trace   trace_id      TYPE bloom_filter GRANULARITY 4,
    INDEX idx_session session_id    TYPE bloom_filter GRANULARITY 4,
    INDEX idx_thread  thread_id     TYPE bloom_filter GRANULARITY 4,
    INDEX idx_user    user_id       TYPE bloom_filter GRANULARITY 4,
    INDEX idx_resource resource_id  TYPE bloom_filter GRANULARITY 4,
    INDEX idx_run     run_id        TYPE bloom_filter GRANULARITY 4,
    INDEX idx_request request_id    TYPE bloom_filter GRANULARITY 4,
    INDEX idx_experiment experiment_id TYPE bloom_filter GRANULARITY 4,
    INDEX idx_release release       TYPE bloom_filter GRANULARITY 4,
    INDEX idx_env     environment   TYPE set(1000) GRANULARITY 4,
    INDEX idx_service service_name  TYPE set(1000) GRANULARITY 4
) ENGINE = ReplacingMergeTree(received_at)
PARTITION BY toYYYYMMDD(start_time)
ORDER BY (project, start_time, trace_id, span_id)
TTL toDateTime(received_at) + INTERVAL 90 DAY;

-- Sorted on the run and on nothing that can change. `start_time` used to be
-- in the key, and it is the run's earliest span, which a later batch can lower:
-- the rebuilt row then carried a different key, no merge ever collapsed the
-- two, and every read of this table saw the run twice. FINAL only folds rows
-- that share a key. The minmax index on `start_time` is what the time-window
-- reads use instead of the key. A run whose earliest span moves it into an
-- earlier day lands in another partition and is not folded either, and that is
-- the boundary this layout still has. Day partitions make that boundary
-- narrower than month partitions did, since a run has to cross midnight rather
-- than a month end. No semicolon may appear in a comment here: `migrate`
-- splits this file on them.
CREATE TABLE IF NOT EXISTS mcpd.traces (
    trace_id        String,
    root_span_id    String,
    name            LowCardinality(String),
    kind            LowCardinality(String),
    status          LowCardinality(String),
    start_time      DateTime64(9, 'UTC'),
    end_time        Nullable(DateTime64(9, 'UTC')),
    duration_ns     Nullable(UInt64),
    span_count      UInt32,
    error_count     UInt32,
    input_tokens    UInt64,
    output_tokens   UInt64,
    total_cost      Float64,
    currency        LowCardinality(String),
    model_count     UInt32,
    tool_count      UInt32,
    tags            Array(String),
    project         LowCardinality(String),
    environment     LowCardinality(String),
    service_name    LowCardinality(String),
    session_id      String,
    thread_id       String,
    user_id         String,
    experiment_id   String,
    release         String,
    score_summary   String,
    feedback_summary String,
    min_score       Nullable(Float64),
    is_complete     UInt8,
    received_at     DateTime64(9, 'UTC'),
    version         UInt64,
    INDEX idx_start start_time TYPE minmax GRANULARITY 4,
    INDEX idx_session session_id TYPE bloom_filter GRANULARITY 4,
    INDEX idx_thread  thread_id  TYPE bloom_filter GRANULARITY 4,
    INDEX idx_user    user_id    TYPE bloom_filter GRANULARITY 4,
    INDEX idx_experiment experiment_id TYPE bloom_filter GRANULARITY 4,
    INDEX idx_release release    TYPE bloom_filter GRANULARITY 4,
    INDEX idx_env     environment  TYPE set(1000) GRANULARITY 4,
    INDEX idx_service service_name TYPE set(1000) GRANULARITY 4
) ENGINE = ReplacingMergeTree(version)
PARTITION BY toYYYYMMDD(start_time)
ORDER BY (project, trace_id);

CREATE TABLE IF NOT EXISTS mcpd.logs (
    log_id String, trace_id String, span_id String,
    timestamp DateTime64(9,'UTC'), level LowCardinality(String), severity UInt8,
    message String, fields String, logger String,
    project LowCardinality(String), environment LowCardinality(String),
    service_name LowCardinality(String), session_id String, user_id String,
    received_at DateTime64(9,'UTC'),
    INDEX idx_msg message TYPE tokenbf_v1(32768, 3, 0) GRANULARITY 4,
    INDEX idx_trace   trace_id     TYPE bloom_filter GRANULARITY 4,
    INDEX idx_span    span_id      TYPE bloom_filter GRANULARITY 4,
    INDEX idx_session session_id   TYPE bloom_filter GRANULARITY 4,
    INDEX idx_user    user_id      TYPE bloom_filter GRANULARITY 4,
    INDEX idx_env     environment  TYPE set(1000) GRANULARITY 4,
    INDEX idx_service service_name TYPE set(1000) GRANULARITY 4
) ENGINE = MergeTree PARTITION BY toYYYYMMDD(timestamp)
ORDER BY (project, timestamp, log_id);

CREATE TABLE IF NOT EXISTS mcpd.metrics (
    metric_id String, name LowCardinality(String), kind LowCardinality(String),
    value Float64, unit LowCardinality(String), timestamp DateTime64(9,'UTC'),
    labels String, buckets String, trace_id String, span_id String,
    project LowCardinality(String), environment LowCardinality(String),
    received_at DateTime64(9,'UTC'),
    INDEX idx_trace trace_id    TYPE bloom_filter GRANULARITY 4,
    INDEX idx_span  span_id     TYPE bloom_filter GRANULARITY 4,
    INDEX idx_env   environment TYPE set(1000) GRANULARITY 4
) ENGINE = MergeTree PARTITION BY toYYYYMMDD(timestamp)
ORDER BY (project, name, timestamp);

CREATE TABLE IF NOT EXISTS mcpd.scores (
    score_id String, trace_id String, span_id String,
    scorer_id LowCardinality(String), scorer_version String,
    source LowCardinality(String), value Float64,
    min_value Nullable(Float64), max_value Nullable(Float64),
    label String, reason String, score_trace_id String, cost_json String,
    metadata String, timestamp DateTime64(9,'UTC'),
    project LowCardinality(String), environment LowCardinality(String),
    experiment_id String,
    INDEX idx_trace trace_id      TYPE bloom_filter GRANULARITY 4,
    INDEX idx_span  span_id       TYPE bloom_filter GRANULARITY 4,
    INDEX idx_experiment experiment_id TYPE bloom_filter GRANULARITY 4,
    INDEX idx_env   environment   TYPE set(1000) GRANULARITY 4
) ENGINE = ReplacingMergeTree PARTITION BY toYYYYMMDD(timestamp)
ORDER BY (project, timestamp, score_id);

CREATE TABLE IF NOT EXISTS mcpd.feedback (
    feedback_id String, trace_id String, span_id String,
    kind LowCardinality(String), value String, comment String,
    author_id String, author_kind LowCardinality(String),
    timestamp DateTime64(9,'UTC'), metadata String,
    project LowCardinality(String), environment LowCardinality(String),
    INDEX idx_trace trace_id    TYPE bloom_filter GRANULARITY 4,
    INDEX idx_span  span_id     TYPE bloom_filter GRANULARITY 4,
    INDEX idx_env   environment TYPE set(1000) GRANULARITY 4
) ENGINE = ReplacingMergeTree PARTITION BY toYYYYMMDD(timestamp)
ORDER BY (project, timestamp, feedback_id);

CREATE TABLE IF NOT EXISTS mcpd.datasets (
    dataset_id String, name String, description String, item_count UInt32,
    created_at DateTime64(9,'UTC'), project LowCardinality(String), environment LowCardinality(String)
) ENGINE = ReplacingMergeTree ORDER BY (project, dataset_id);

CREATE TABLE IF NOT EXISTS mcpd.dataset_items (
    item_id String, dataset_id String, input String, expected String,
    metadata String, source_trace_id String
) ENGINE = ReplacingMergeTree ORDER BY (dataset_id, item_id);

CREATE TABLE IF NOT EXISTS mcpd.experiments (
    experiment_id String, name String, dataset_id String, status LowCardinality(String),
    started_at Nullable(DateTime64(9,'UTC')), finished_at Nullable(DateTime64(9,'UTC')),
    item_count UInt32, summary String, baseline_id String, cost_json String,
    error String, metadata String,
    project LowCardinality(String), environment LowCardinality(String)
) ENGINE = ReplacingMergeTree ORDER BY (project, experiment_id);

-- What a filter box can offer, kept as data arrives. The alternative is
-- `SELECT DISTINCT environment FROM spans`, which reads every span in the
-- retention window to produce a list of four strings and is asked again on
-- every keystroke.
--
-- `key` is the empty string for every facet but `metric_label_value`, where it
-- is the label key the value belongs to. Replacing on `last_seen` means the
-- newest write for a value wins, which is exactly the semantics wanted: the
-- row records when the value was last carried.
CREATE TABLE IF NOT EXISTS mcpd.discovery (
    project LowCardinality(String), facet LowCardinality(String),
    key String, value String, last_seen DateTime64(9,'UTC')
) ENGINE = ReplacingMergeTree(last_seen) ORDER BY (project, facet, key, value);

-- What `migrate` has already applied, so a start with nothing to do issues no
-- DDL. `fingerprint` is a digest of the statements this build would run,
-- retention included, so a changed schema or a changed retention window is
-- noticed and an unchanged one is not replayed. Replicated deployments are the
-- reason: every replica calls `migrate` on start, and each `CREATE TABLE IF NOT
-- EXISTS` and `MODIFY TTL` is metadata the whole cluster has to agree on.
CREATE TABLE IF NOT EXISTS mcpd.schema_version (
    version UInt32, fingerprint String, applied_at DateTime64(9,'UTC')
) ENGINE = ReplacingMergeTree(applied_at) ORDER BY version;

CREATE TABLE IF NOT EXISTS mcpd.ingest_keys (
    key String, seen_at DateTime64(9,'UTC')
) ENGINE = ReplacingMergeTree ORDER BY key;

-- The audit log: who read a payload, and who deleted one. Carries no TTL, on
-- purpose: a log that expired on the same schedule as the data it describes
-- could not answer who read the data that has since gone. `principal` is a
-- truncated digest of the key that acted, never the key.
CREATE TABLE IF NOT EXISTS mcpd.audit_log (
    audit_id String, timestamp DateTime64(9,'UTC'),
    principal String, subject String,
    action LowCardinality(String), project LowCardinality(String),
    resource String, result LowCardinality(String)
) ENGINE = ReplacingMergeTree
PARTITION BY toYYYYMMDD(timestamp)
ORDER BY (project, timestamp, audit_id);

-- One sentence recorded against one trace along one aspect.
--
-- `text` is payload: it arrives through the same redactor as every other
-- signal, retention sweeps it, and erasing a subject erases it.
--
-- `entity_kind` and `entity_name` are copied from the trace's root span when
-- the note is written. Nothing in this backend joins, so a roll-up by entity
-- has to be answerable from this one table, and that is what the copy is for.
-- Both are the empty string when the note arrived before its trace did.
--
-- `sketch` is the 64 integers the clustering compares, written as 1024
-- lowercase hex characters rather than as 512 raw bytes. The insert path here
-- is `JSONEachRow`, and JSON carries text: a byte above 0x7f would be re-encoded
-- as UTF-8 on the way in and the sketch would read back as something else. Hex
-- costs twice the width and is exact, which is the right way round for a column
-- whose whole job is to be compared for equality.
--
-- It is a `String` and not a `FixedString(1024)`: ClickHouse refuses to create
-- a fixed string wider than 256 bytes unless `allow_suspicious_fixed_string_types`
-- is set, and a table that only exists under a server setting is a table an
-- operator's ClickHouse may not have. The width is fixed by the writer, not by
-- the column, and equality reads the same either way.
CREATE TABLE IF NOT EXISTS mcpd.aspect_notes (
    note_id String, trace_id String, aspect LowCardinality(String), text String,
    definition_version UInt32, model LowCardinality(String),
    created_at DateTime64(9,'UTC'),
    entity_kind LowCardinality(String), entity_name String,
    trace_started_at DateTime64(9,'UTC'),
    sketch String,
    project LowCardinality(String), environment LowCardinality(String),
    service_name LowCardinality(String), service_version String,
    session_id String, thread_id String, user_id String, resource_id String,
    run_id String, request_id String, experiment_id String, release String,
    INDEX idx_trace  trace_id    TYPE bloom_filter GRANULARITY 4,
    INDEX idx_entity entity_name TYPE bloom_filter GRANULARITY 4,
    INDEX idx_user   user_id     TYPE bloom_filter GRANULARITY 4,
    INDEX idx_env    environment TYPE set(1000) GRANULARITY 4
) ENGINE = ReplacingMergeTree PARTITION BY toYYYYMMDD(created_at)
ORDER BY (project, aspect, created_at, note_id);

-- One clustering pass over one entity, covering a moving window of notes.
CREATE TABLE IF NOT EXISTS mcpd.theme_snapshots (
    snapshot_id String, project LowCardinality(String),
    entity_kind LowCardinality(String), entity_name String,
    aspects String, ordinal UInt64,
    cutoff_at DateTime64(9,'UTC'),
    window_from DateTime64(9,'UTC'), window_to DateTime64(9,'UTC'),
    trace_count UInt64, note_count UInt64, capped UInt8
) ENGINE = ReplacingMergeTree PARTITION BY toYYYYMMDD(cutoff_at)
ORDER BY (project, entity_kind, entity_name, cutoff_at, snapshot_id);

-- A cluster of aspect notes, in one snapshot along one aspect.
--
-- `cutoff_at` and the window bounds are copied from the snapshot, because a
-- history is a read of one theme across every snapshot that held it and nothing
-- in this backend joins. The four lineage columns are JSON arrays, both
-- directions stored.
CREATE TABLE IF NOT EXISTS mcpd.themes (
    snapshot_id String, aspect LowCardinality(String), theme_id String,
    project LowCardinality(String),
    entity_kind LowCardinality(String), entity_name String,
    label String, label_note_id String, label_trace_id String,
    state LowCardinality(String),
    note_count UInt64, trace_count UInt64, coverage Float64,
    cutoff_at DateTime64(9,'UTC'),
    window_from DateTime64(9,'UTC'), window_to DateTime64(9,'UTC'),
    split_from String, split_into String, merged_from String, merged_into String,
    INDEX idx_theme theme_id       TYPE bloom_filter GRANULARITY 4,
    INDEX idx_label label_trace_id TYPE bloom_filter GRANULARITY 4
) ENGINE = ReplacingMergeTree PARTITION BY toYYYYMMDD(cutoff_at)
ORDER BY (project, entity_kind, entity_name, aspect, theme_id, cutoff_at, snapshot_id);

-- Which note joined which theme, in one snapshot. Ids and one integer, and no
-- prose: the sentence stays in `aspect_notes`.
CREATE TABLE IF NOT EXISTS mcpd.theme_members (
    snapshot_id String, aspect LowCardinality(String), theme_id String,
    note_id String, trace_id String, project LowCardinality(String),
    similarity UInt32, trace_started_at DateTime64(9,'UTC'),
    INDEX idx_trace trace_id TYPE bloom_filter GRANULARITY 4
) ENGINE = ReplacingMergeTree
ORDER BY (project, snapshot_id, aspect, theme_id, note_id);

-- The rest of the correlation on a log line.
ALTER TABLE mcpd.logs ADD COLUMN IF NOT EXISTS service_version String DEFAULT '';
ALTER TABLE mcpd.logs ADD COLUMN IF NOT EXISTS thread_id String DEFAULT '';
ALTER TABLE mcpd.logs ADD COLUMN IF NOT EXISTS resource_id String DEFAULT '';
ALTER TABLE mcpd.logs ADD COLUMN IF NOT EXISTS run_id String DEFAULT '';
ALTER TABLE mcpd.logs ADD COLUMN IF NOT EXISTS request_id String DEFAULT '';
ALTER TABLE mcpd.logs ADD COLUMN IF NOT EXISTS experiment_id String DEFAULT '';
ALTER TABLE mcpd.logs ADD COLUMN IF NOT EXISTS release String DEFAULT '';
