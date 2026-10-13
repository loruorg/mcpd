//! The SQL behind each [`Store`](crate::Store) method.

use super::*;

// ---------------------------------------------------------------- ingest ---

/// Write a whole batch in one transaction.
///
/// Everything lands or nothing does. A half-written batch would show up as a
/// trace with holes in it, which reads as a bug in the traced system rather
/// than in mcpd, and is the worst failure mode an observability tool has.
pub(super) fn write_batch(conn: &mut Connection, batch: Batch) -> Result<IngestResult> {
    let mut result = IngestResult { accepted: 0, rejected: 0, duplicate: 0, errors: None };

    // Idempotency: a client that retried after a timeout gets an honest 200
    // and no second copy of its data.
    if let Some(key) = &batch.ingest_key {
        let seen: Option<i64> = conn
            .query_row("SELECT seen_at FROM ingest_keys WHERE key = ?1", [key], |r| r.get(0))
            .optional()
            .map_err(be)?;
        if seen.is_some() {
            result.duplicate = batch.len() as u32;
            return Ok(result);
        }
    }

    // Collected before the batch is consumed below, and written inside the same
    // transaction as the signals. In the same transaction on purpose: a value
    // offered by the filter box that no row carries, or a row whose value the
    // box does not offer, are both states a reader cannot make sense of.
    let facets = crate::facet_values(&batch);

    let now = now_nanos();
    let tx = conn.transaction().map_err(be)?;
    let mut touched_traces: Vec<(String, String)> = Vec::new();

    {
        let mut ins_span = tx
            .prepare_cached(
                "INSERT OR REPLACE INTO spans (
                    trace_id, span_id, parent_span_id, external_parent_span_id, name, kind,
                    status, start_time, end_time, duration_ns, is_root, is_event,
                    entity_kind, entity_id, entity_name, input, output, attributes, metadata,
                    usage_json, input_tokens, output_tokens, cost_json, total_cost,
                    error_json, links_json, tags, project, environment, service_name,
                    service_version, session_id, thread_id, user_id, resource_id, run_id,
                    request_id, experiment_id, release, model, provider, received_at
                 ) VALUES (
                    ?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,
                    ?21,?22,?23,?24,?25,?26,?27,?28,?29,?30,?31,?32,?33,?34,?35,?36,?37,?38,
                    ?39,?40,?41,?42)",
            )
            .map_err(be)?;
        let mut del_fts = tx
            .prepare_cached(
                "DELETE FROM spans_fts WHERE span_id = ?1 AND trace_id = ?2 AND project = ?3",
            )
            .map_err(be)?;
        let mut ins_fts = tx
            .prepare_cached(
                "INSERT INTO spans_fts (trace_id, span_id, project, name, input, output)
                 VALUES (?1,?2,?3,?4,?5,?6)",
            )
            .map_err(be)?;
        let mut span_owner = tx
            .prepare_cached("SELECT project FROM spans WHERE trace_id = ?1 AND span_id = ?2")
            .map_err(be)?;

        for (index, mut span) in batch.spans.into_iter().enumerate() {
            span.normalise();
            if span.received_at == OffsetDateTime::UNIX_EPOCH {
                span.received_at = from_nanos(now);
            }
            let c = &span.correlation;
            let owner = owner_of(&mut span_owner, rusqlite::params![span.trace_id, span.span_id])?;
            if crate::foreign(owner.as_deref(), &c.project) {
                crate::refuse_foreign(
                    &mut result,
                    mcpd_core::SignalType::Span,
                    index,
                    "span",
                    &span.span_id,
                );
                continue;
            }
            ins_span
                .execute(rusqlite::params![
                    span.trace_id,
                    span.span_id,
                    span.parent_span_id,
                    span.external_parent_span_id,
                    span.name,
                    span.kind.as_str(),
                    span.status.as_str(),
                    to_nanos(span.start_time),
                    span.end_time.map(to_nanos),
                    span.duration_ns.map(|v| v as i64),
                    i64::from(span.is_root),
                    i64::from(span.is_event),
                    span.entity_kind.map(|k| k.as_str().to_owned()),
                    span.entity_id,
                    span.entity_name,
                    json_out(&span.input),
                    json_out(&span.output),
                    json_out(&span.attributes),
                    json_out(&span.metadata),
                    enc(&span.usage),
                    span.usage.as_ref().and_then(|u| u.input_tokens).map(|v| v as i64),
                    span.usage.as_ref().and_then(|u| u.output_tokens).map(|v| v as i64),
                    enc(&span.cost),
                    span.cost.as_ref().and_then(|c| c.total_cost),
                    enc(&span.error),
                    enc(&span.links),
                    tags_out(&span.tags),
                    c.project,
                    c.environment,
                    c.service_name,
                    c.service_version,
                    c.session_id,
                    c.thread_id,
                    c.user_id,
                    c.resource_id,
                    c.run_id,
                    c.request_id,
                    c.experiment_id,
                    c.release,
                    span.model,
                    span.provider,
                    to_nanos(span.received_at),
                ])
                .map_err(be)?;

            // Keep the search index in step. A span can be written more than
            // once as it is updated, so the old row goes first.
            del_fts
                .execute(rusqlite::params![span.span_id, span.trace_id, c.project])
                .map_err(be)?;
            ins_fts
                .execute(rusqlite::params![
                    span.trace_id,
                    span.span_id,
                    c.project,
                    span.name,
                    span.input.as_ref().map(searchable_text),
                    span.output.as_ref().map(searchable_text),
                ])
                .map_err(be)?;

            touch(&mut touched_traces, &c.project, &span.trace_id);
            result.accepted += 1;
        }

        let mut ins_log = tx
            .prepare_cached(
                "INSERT OR REPLACE INTO logs (
                    log_id, trace_id, span_id, timestamp, level, severity, message, fields,
                    logger, project, environment, service_name, session_id, user_id, received_at,
                    service_version, thread_id, resource_id, run_id, request_id, experiment_id,
                    release
                 ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,
                           ?16,?17,?18,?19,?20,?21,?22)",
            )
            .map_err(be)?;
        let mut ins_log_fts = tx
            .prepare_cached(
                "INSERT INTO logs_fts (log_id, project, message, fields) VALUES (?1,?2,?3,?4)",
            )
            .map_err(be)?;
        // A replayed log replaces its own row, so its old index entry goes
        // first. Without this every resend added a second hit for the same line.
        let mut del_log_fts = tx
            .prepare_cached("DELETE FROM logs_fts WHERE log_id = ?1 AND project = ?2")
            .map_err(be)?;
        let mut log_owner =
            tx.prepare_cached("SELECT project FROM logs WHERE log_id = ?1").map_err(be)?;
        for (index, mut log) in batch.logs.into_iter().enumerate() {
            if log.received_at == OffsetDateTime::UNIX_EPOCH {
                log.received_at = from_nanos(now);
            }
            let c = &log.correlation;
            let owner = owner_of(&mut log_owner, rusqlite::params![log.log_id])?;
            if crate::foreign(owner.as_deref(), &c.project) {
                crate::refuse_foreign(
                    &mut result,
                    mcpd_core::SignalType::Log,
                    index,
                    "log",
                    &log.log_id,
                );
                continue;
            }
            ins_log
                .execute(rusqlite::params![
                    log.log_id,
                    log.trace_id,
                    log.span_id,
                    to_nanos(log.timestamp),
                    log.level.as_str(),
                    i64::from(log.level.severity()),
                    log.message,
                    json_out(&log.fields),
                    log.logger,
                    c.project,
                    c.environment,
                    c.service_name,
                    c.session_id,
                    c.user_id,
                    to_nanos(log.received_at),
                    c.service_version,
                    c.thread_id,
                    c.resource_id,
                    c.run_id,
                    c.request_id,
                    c.experiment_id,
                    c.release,
                ])
                .map_err(be)?;
            del_log_fts.execute(rusqlite::params![log.log_id, c.project]).map_err(be)?;
            ins_log_fts
                .execute(rusqlite::params![
                    log.log_id,
                    c.project,
                    log.message,
                    log.fields.as_ref().map(searchable_text)
                ])
                .map_err(be)?;
            result.accepted += 1;
        }

        let mut ins_metric = tx
            .prepare_cached(
                "INSERT OR REPLACE INTO metrics (
                    metric_id, name, kind, value, unit, timestamp, labels, buckets,
                    trace_id, span_id, project, environment, received_at
                 ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
            )
            .map_err(be)?;
        let mut metric_owner =
            tx.prepare_cached("SELECT project FROM metrics WHERE metric_id = ?1").map_err(be)?;
        for (index, mut m) in batch.metrics.into_iter().enumerate() {
            if m.received_at == OffsetDateTime::UNIX_EPOCH {
                m.received_at = from_nanos(now);
            }
            let owner = owner_of(&mut metric_owner, rusqlite::params![m.metric_id])?;
            if crate::foreign(owner.as_deref(), &m.correlation.project) {
                crate::refuse_foreign(
                    &mut result,
                    mcpd_core::SignalType::Metric,
                    index,
                    "metric point",
                    &m.metric_id,
                );
                continue;
            }
            ins_metric
                .execute(rusqlite::params![
                    m.metric_id,
                    m.name,
                    m.kind.as_str(),
                    m.value,
                    m.unit,
                    to_nanos(m.timestamp),
                    json_out(&m.labels),
                    json_out(&m.buckets),
                    m.trace_id,
                    m.span_id,
                    m.correlation.project,
                    m.correlation.environment,
                    to_nanos(m.received_at),
                ])
                .map_err(be)?;
            result.accepted += 1;
        }

        let mut ins_score = tx
            .prepare_cached(
                "INSERT OR REPLACE INTO scores (
                    score_id, trace_id, span_id, scorer_id, scorer_version, source, value,
                    min_value, max_value, label, reason, score_trace_id, cost_json, metadata,
                    timestamp, project, environment, experiment_id
                 ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",
            )
            .map_err(be)?;
        let mut score_owner =
            tx.prepare_cached("SELECT project FROM scores WHERE score_id = ?1").map_err(be)?;
        for (index, s) in batch.scores.into_iter().enumerate() {
            let owner = owner_of(&mut score_owner, rusqlite::params![s.score_id])?;
            if crate::foreign(owner.as_deref(), &s.correlation.project) {
                crate::refuse_foreign(
                    &mut result,
                    mcpd_core::SignalType::Score,
                    index,
                    "score",
                    &s.score_id,
                );
                continue;
            }
            ins_score
                .execute(rusqlite::params![
                    s.score_id,
                    s.trace_id,
                    s.span_id,
                    s.scorer_id,
                    s.scorer_version,
                    s.source.as_str(),
                    s.value,
                    s.min,
                    s.max,
                    s.label,
                    s.reason,
                    s.score_trace_id,
                    enc(&s.cost),
                    json_out(&s.metadata),
                    to_nanos(s.timestamp),
                    s.correlation.project,
                    s.correlation.environment,
                    s.correlation.experiment_id,
                ])
                .map_err(be)?;
            if let Some(t) = &s.trace_id {
                touch(&mut touched_traces, &s.correlation.project, t);
            }
            result.accepted += 1;
        }

        let mut ins_fb = tx
            .prepare_cached(
                "INSERT OR REPLACE INTO feedback (
                    feedback_id, trace_id, span_id, kind, value, comment, author_id,
                    author_kind, timestamp, metadata, project, environment
                 ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            )
            .map_err(be)?;
        let mut feedback_owner =
            tx.prepare_cached("SELECT project FROM feedback WHERE feedback_id = ?1").map_err(be)?;
        for (index, f) in batch.feedback.into_iter().enumerate() {
            let owner = owner_of(&mut feedback_owner, rusqlite::params![f.feedback_id])?;
            if crate::foreign(owner.as_deref(), &f.correlation.project) {
                crate::refuse_foreign(
                    &mut result,
                    mcpd_core::SignalType::Feedback,
                    index,
                    "feedback",
                    &f.feedback_id,
                );
                continue;
            }
            ins_fb
                .execute(rusqlite::params![
                    f.feedback_id,
                    f.trace_id,
                    f.span_id,
                    f.kind.as_str(),
                    serde_json::to_string(&f.value).unwrap_or_else(|_| "null".into()),
                    f.comment,
                    f.author_id,
                    f.author_kind,
                    to_nanos(f.timestamp),
                    json_out(&f.metadata),
                    f.correlation.project,
                    f.correlation.environment,
                ])
                .map_err(be)?;
            if let Some(t) = &f.trace_id {
                touch(&mut touched_traces, &f.correlation.project, t);
            }
            result.accepted += 1;
        }

        // Notes last of the six signals, and inside the same transaction. A
        // note resolves the entity it is filed under from its trace's root
        // span, so writing it after the spans of the same batch means a note
        // that travelled beside its own trace is filed correctly rather than
        // waiting for the next pass to notice.
        let written = super::aspects::write_aspect_notes(&tx, &batch.aspect_notes)?;
        for (index, note) in batch.aspect_notes.iter().enumerate() {
            if written.refused.contains(&index) {
                crate::refuse_foreign(
                    &mut result,
                    mcpd_core::SignalType::AspectNote,
                    index,
                    "aspect note",
                    &note.note_id,
                );
            } else {
                touch(&mut touched_traces, &note.correlation.project, &note.trace_id);
            }
        }
        result.accepted += written.stored;

        // `max` of the two rather than the new value alone. Signals arrive out
        // of order, and a batch exported late carries an older arrival time
        // than one already written: taking the incoming value unconditionally
        // would walk `last_seen` backwards and make a live value look stale.
        let mut ins_facet = tx
            .prepare_cached(
                "INSERT INTO discovery (project, facet, key, value, last_seen)
                 VALUES (?1,?2,?3,?4,?5)
                 ON CONFLICT (project, facet, key, value)
                 DO UPDATE SET last_seen = max(last_seen, excluded.last_seen)",
            )
            .map_err(be)?;
        for f in &facets {
            ins_facet
                .execute(rusqlite::params![f.project, f.facet.as_str(), f.key, f.value, now])
                .map_err(be)?;
        }

        for (project, trace_id) in &touched_traces {
            recompute_trace(&tx, project, trace_id)?;
        }

        if let Some(key) = &batch.ingest_key {
            tx.execute(
                "INSERT OR IGNORE INTO ingest_keys (key, seen_at) VALUES (?1, ?2)",
                rusqlite::params![key, now],
            )
            .map_err(be)?;
        }
    }

    tx.commit().map_err(be)?;
    Ok(result)
}

/// The project that already holds the row with this id, if any.
pub(super) fn owner_of(
    stmt: &mut rusqlite::CachedStatement<'_>,
    id: impl rusqlite::Params,
) -> Result<Option<String>> {
    stmt.query_row(id, |r| r.get(0)).optional().map_err(be)
}

/// The aggregates of one run's projection over its spans, in the order
/// [`Totals`] reads them.
///
/// One text for the two places that build a projection, the per-run recompute
/// on every write and the rebuild below, so they cannot count differently.
///
/// Every SUM is wrapped, because SUM over no rows is NULL, not zero. The
/// recompute runs for any trace a score or a piece of feedback names, and
/// attaching feedback before its spans have landed is an ordinary ordering in
/// an async system, not a mistake. It used to fail the whole request with a 500.
const TRACE_TOTALS: &str = "
    COUNT(*),
    COALESCE(SUM(CASE WHEN status = 'error' THEN 1 ELSE 0 END), 0),
    COALESCE(MIN(start_time), 0),
    MAX(end_time),
    COALESCE(SUM(COALESCE(input_tokens, 0)), 0),
    COALESCE(SUM(COALESCE(output_tokens, 0)), 0),
    COALESCE(SUM(CASE WHEN end_time IS NULL AND is_event = 0 THEN 1 ELSE 0 END), 0),
    COALESCE(SUM(COALESCE(total_cost, 0)), 0.0),
    COALESCE(SUM(CASE WHEN kind IN ('model_generation','model_inference') THEN 1 ELSE 0 END), 0),
    COALESCE(SUM(CASE WHEN kind IN
        ('tool_call','mcp_tool_call','client_tool_call','provider_tool_call') THEN 1 ELSE 0 END), 0)";

/// Which span a run takes its name and identity from: the real root first,
/// then the earliest span. The span id settles a tie, which two spans that
/// started in the same nanosecond produce. Without it the choice was whatever
/// order the engine read the rows in, and a rebuild could name a run
/// differently from the write that first built it.
const ROOT_ORDER: &str = "is_root DESC, start_time ASC, span_id ASC";

/// The score rollup, from rows of `(scorer_id, avg_value)`. Ordered by the key
/// so the stored text does not depend on the order the engine groups in.
const SCORE_SUMMARY: &str = "json_group_object(scorer_id, avg_value ORDER BY scorer_id)";

/// The feedback rollup, from rows of `(kind, n)`.
const FEEDBACK_SUMMARY: &str = "json_group_object(kind, n ORDER BY kind)";

/// Build the projection of every trace from its spans, one row per project and
/// trace id. Used once, by the migration that changed the key.
///
/// One statement over the whole table, not [`recompute_trace`] once per run.
/// The loop ran six queries a run, so a store of 500,000 runs took over a
/// minute before the daemon could answer `/healthz`. A startup probe restarted
/// it before it finished, and every restart began the rebuild again. This
/// sorts the spans once and computes every column the way the recompute does:
/// the aggregates and the root order are the same text, and the two summaries
/// use the same JSON functions. A test compares the rows each one writes.
pub(super) fn rebuild_traces(conn: &mut Connection) -> Result<u64> {
    let tx = conn.transaction().map_err(be)?;
    let sql = format!(
        "WITH totals (project, trace_id, span_count, error_count, start_time, end_time,
                      input_tokens, output_tokens, open, total_cost, model_count, tool_count)
         AS (SELECT project, trace_id, {TRACE_TOTALS} FROM spans GROUP BY project, trace_id),
         roots AS (
            SELECT project, trace_id, span_id, name, kind, status, tags, environment,
                   service_name, session_id, thread_id, user_id, experiment_id, release,
                   received_at,
                   ROW_NUMBER() OVER (PARTITION BY project, trace_id ORDER BY {ROOT_ORDER}) AS rn
            FROM spans),
         score AS (
            SELECT project, trace_id, {SCORE_SUMMARY} AS summary, MIN(min_value) AS min_score
            FROM (SELECT project, trace_id, scorer_id, AVG(value) AS avg_value,
                         MIN(value) AS min_value
                  FROM scores WHERE trace_id IS NOT NULL
                  GROUP BY project, trace_id, scorer_id)
            GROUP BY project, trace_id),
         fb AS (
            SELECT project, trace_id, {FEEDBACK_SUMMARY} AS summary
            FROM (SELECT project, trace_id, kind, COUNT(*) AS n
                  FROM feedback WHERE trace_id IS NOT NULL
                  GROUP BY project, trace_id, kind)
            GROUP BY project, trace_id)
         INSERT INTO traces (
            trace_id, root_span_id, name, kind, status, start_time, end_time, duration_ns,
            span_count, error_count, input_tokens, output_tokens, total_cost, currency,
            model_count, tool_count, tags, project, environment, service_name, session_id,
            thread_id, user_id, experiment_id, release, score_summary, feedback_summary,
            min_score, is_complete, received_at)
         SELECT t.trace_id, r.span_id, r.name, r.kind,
                CASE WHEN t.error_count > 0 THEN 'error' ELSE r.status END,
                t.start_time, t.end_time,
                CASE WHEN t.end_time IS NULL THEN NULL
                     ELSE max(t.end_time - t.start_time, 0) END,
                t.span_count, t.error_count, t.input_tokens, t.output_tokens, t.total_cost,
                'USD', t.model_count, t.tool_count, r.tags, t.project, r.environment,
                r.service_name, r.session_id, r.thread_id, r.user_id, r.experiment_id,
                r.release,
                -- The recompute aggregates over no rows when a run has no
                -- scores, and json_group_object answers '{{}}' for that, not NULL.
                COALESCE(s.summary, '{{}}'), COALESCE(f.summary, '{{}}'), s.min_score,
                t.open = 0 AND t.end_time IS NOT NULL, r.received_at
         FROM totals t
         JOIN roots r ON r.project = t.project AND r.trace_id = t.trace_id AND r.rn = 1
         LEFT JOIN score s ON s.project = t.project AND s.trace_id = t.trace_id
         LEFT JOIN fb f ON f.project = t.project AND f.trace_id = t.trace_id"
    );
    let rebuilt = tx.execute(&sql, []).map_err(be)?;
    tx.commit().map_err(be)?;
    Ok(rebuilt as u64)
}

/// Note a trace whose projection must be rebuilt, once per project.
fn touch(touched: &mut Vec<(String, String)>, project: &str, trace_id: &str) {
    if !touched.iter().any(|(p, t)| p == project && t == trace_id) {
        touched.push((project.to_owned(), trace_id.to_owned()));
    }
}

/// Flatten a JSON payload into text the FTS index can tokenise.
///
/// Indexing the raw JSON would make every brace and quote a token and bury the
/// words a person actually searches for.
fn searchable_text(v: &Value) -> String {
    fn walk(v: &Value, out: &mut String) {
        match v {
            Value::String(s) => {
                out.push_str(s);
                out.push(' ');
            }
            Value::Number(n) => {
                out.push_str(&n.to_string());
                out.push(' ');
            }
            Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
            Value::Object(m) => m.values().for_each(|x| walk(x, out)),
            _ => {}
        }
    }
    let mut s = String::new();
    walk(v, &mut s);
    // Bound what goes into the index: a megabyte prompt would otherwise make
    // the index larger than the data.
    //
    // Walked back to a character boundary first. `String::truncate` asserts the
    // new length is one, so a bare call panics on any payload whose 65536th
    // byte lands mid-codepoint. That panic happened inside the writer mutex,
    // which poisoned it, and every later write in the process failed: one
    // oversized non-ASCII prompt stopped all ingest until restart.
    let mut end = (64 * 1024).min(s.len());
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s.truncate(end);
    s
}

/// Rebuild one trace's projection from its spans.
///
/// The aggregate row `recompute_trace` reads: counts, the time bounds, token
/// totals and cost, in the order `TRACE_TOTALS` lists them.
type Totals = (i64, i64, i64, Option<i64>, i64, i64, i64, f64, i64, i64);

/// The identity a trace inherits from its root span: ids, name, kind, status,
/// and every correlation field, in `SELECT` order.
type RootRow = (
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    i64,
);

/// Recomputing rather than incrementing is deliberate. Spans arrive out of
/// order, get retried, and get updated in place; an incremental counter drifts
/// under all three, and a wrong span count in the list view destroys trust in
/// every other number on the page.
///
/// One projection per project and trace id, never one per trace id. A trace id
/// is shared across a service boundary by design, so two projects that call
/// each other hold spans of the same trace. Folding them into one row once let
/// the project that wrote the earliest span own the whole trace, and let it
/// read the other project's logs, scores and feedback through it.
fn recompute_trace(tx: &rusqlite::Transaction<'_>, project: &str, trace_id: &str) -> Result<()> {
    let row: Option<Totals> = tx
        .query_row(
            &format!("SELECT {TRACE_TOTALS} FROM spans WHERE project = ?1 AND trace_id = ?2"),
            [project, trace_id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                    r.get(8)?,
                    r.get(9)?,
                ))
            },
        )
        .optional()
        .map_err(be)?;

    let Some((count, errors, start, end, in_tok, out_tok, open, cost, models, tools)) = row else {
        return Ok(());
    };
    if count == 0 {
        // Every span for this trace has been deleted; drop the projection too
        // rather than leaving a row pointing at nothing.
        tx.execute("DELETE FROM traces WHERE project = ?1 AND trace_id = ?2", [project, trace_id])
            .map_err(be)?;
        return Ok(());
    }

    // Prefer the real root. Falling back to the earliest span means a trace
    // whose root has not arrived yet is still listed, with the best name we
    // have, instead of disappearing until it does.
    let root: Option<RootRow> = tx
        .query_row(
            &format!(
                "SELECT span_id, name, kind, status, tags, environment, service_name,
                        session_id, thread_id, user_id, experiment_id, release, received_at
                 FROM spans WHERE project = ?1 AND trace_id = ?2
                 ORDER BY {ROOT_ORDER} LIMIT 1"
            ),
            [project, trace_id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                    r.get(8)?,
                    r.get(9)?,
                    r.get(10)?,
                    r.get(11)?,
                    r.get(12)?,
                ))
            },
        )
        .optional()
        .map_err(be)?;

    let Some((
        span_id,
        name,
        kind,
        root_status,
        tags,
        environment,
        service_name,
        session_id,
        thread_id,
        user_id,
        experiment_id,
        release,
        received_at,
    )) = root
    else {
        return Ok(());
    };

    // A trace is failed if any span in it failed, even when the root reports
    // success — an error that was caught and swallowed is still what you came
    // to the trace list to find.
    let status = if errors > 0 { "error".to_owned() } else { root_status };
    let is_complete = open == 0 && end.is_some();
    let duration = match (end, start) {
        (Some(e), s) => Some((e - s).max(0)),
        _ => None,
    };

    // Score and feedback rollups, so the list can sort by quality with no join.
    let score_summary: Option<String> = tx
        .query_row(
            &format!(
                "SELECT {SCORE_SUMMARY} FROM (
                    SELECT scorer_id, AVG(value) AS avg_value FROM scores
                    WHERE project = ?1 AND trace_id = ?2 GROUP BY scorer_id)"
            ),
            [project, trace_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(be)?
        .flatten();
    let min_score: Option<f64> = tx
        .query_row(
            "SELECT MIN(value) FROM scores WHERE project = ?1 AND trace_id = ?2",
            [project, trace_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(be)?
        .flatten();
    let feedback_summary: Option<String> = tx
        .query_row(
            &format!(
                "SELECT {FEEDBACK_SUMMARY} FROM (
                    SELECT kind, COUNT(*) AS n FROM feedback
                    WHERE project = ?1 AND trace_id = ?2 GROUP BY kind)"
            ),
            [project, trace_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(be)?
        .flatten();

    tx.execute(
        "INSERT OR REPLACE INTO traces (
            trace_id, root_span_id, name, kind, status, start_time, end_time, duration_ns,
            span_count, error_count, input_tokens, output_tokens, total_cost, currency,
            model_count, tool_count, tags, project, environment, service_name, session_id,
            thread_id, user_id, experiment_id, release, score_summary, feedback_summary,
            min_score, is_complete, received_at
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,'USD',?14,?15,?16,?17,?18,?19,
                   ?20,?21,?22,?23,?24,?25,?26,?27,?28,?29)",
        rusqlite::params![
            trace_id,
            span_id,
            name,
            kind,
            status,
            start,
            end,
            duration,
            count,
            errors,
            in_tok,
            out_tok,
            cost,
            models,
            tools,
            tags,
            project,
            environment,
            service_name,
            session_id,
            thread_id,
            user_id,
            experiment_id,
            release,
            score_summary,
            feedback_summary,
            min_score,
            i64::from(is_complete),
            received_at,
        ],
    )
    .map_err(be)?;
    Ok(())
}

// ----------------------------------------------------------------- reads ---

pub(super) fn list_traces_sync(conn: &Connection, q: &TraceQuery) -> Result<Paged<Trace>> {
    let mut w = Where::default();
    w.push("t.project = ?", q.project.clone());
    if let Some(s) = q.status {
        w.push("t.status = ?", s.as_str().to_owned());
    }
    if let Some(k) = q.kind {
        w.push("t.kind = ?", k.as_str().to_owned());
    }
    if let Some(n) = &q.name {
        w.push("t.name = ?", n.clone());
    }
    if let Some(s) = &q.session_id {
        w.push("t.session_id = ?", s.clone());
    }
    if let Some(s) = &q.thread_id {
        w.push("t.thread_id = ?", s.clone());
    }
    if let Some(s) = &q.user_id {
        w.push("t.user_id = ?", s.clone());
    }
    if let Some(s) = &q.environment {
        w.push("t.environment = ?", s.clone());
    }
    if let Some(f) = q.from {
        w.push("t.start_time >= ?", to_nanos(f));
    }
    if let Some(t) = q.to {
        w.push("t.start_time < ?", to_nanos(t));
    }
    if let Some(d) = q.min_duration_ms {
        w.push("t.duration_ns >= ?", (d as i64).saturating_mul(1_000_000));
    }
    if let Some(c) = q.min_cost {
        w.push("t.total_cost >= ?", c);
    }
    if let Some(m) = &q.model {
        w.push(
            "EXISTS (SELECT 1 FROM spans s WHERE s.project = t.project AND s.trace_id = t.trace_id \
             AND s.model = ?)",
            m.clone(),
        );
    }
    for tag in &q.tags {
        w.push("EXISTS (SELECT 1 FROM json_each(t.tags) je WHERE je.value = ?)", tag.clone());
    }
    if let Some(scorer) = &q.has_score {
        match q.score_below {
            Some(below) => {
                w.clauses.push(
                    "EXISTS (SELECT 1 FROM scores sc WHERE sc.project = t.project \
                     AND sc.trace_id = t.trace_id AND sc.scorer_id = ? AND sc.value < ?)"
                        .to_owned(),
                );
                w.binds.push(Box::new(scorer.clone()));
                w.binds.push(Box::new(below));
            }
            None => w.push(
                "EXISTS (SELECT 1 FROM scores sc WHERE sc.project = t.project \
                 AND sc.trace_id = t.trace_id AND sc.scorer_id = ?)",
                scorer.clone(),
            ),
        }
    }
    if let Some(raw) = &q.q {
        if let Some(fts) = fts_query(raw) {
            w.clauses.push(
                "t.trace_id IN (SELECT trace_id FROM spans_fts WHERE spans_fts MATCH ? \
                 AND project = ?)"
                    .to_owned(),
            );
            w.binds.push(Box::new(fts));
            w.binds.push(Box::new(q.project.clone()));
        }
    }

    // The score sort reads `min_score`, which the projection maintains as
    // scores arrive, so ordering by quality costs no join. COALESCE is what
    // makes an unscored run rank as a zero rather than sorting to whichever
    // end this particular database puts a NULL.
    let sort_col = match q.sort {
        TraceSort::StartTime => "t.start_time",
        TraceSort::Duration => "COALESCE(t.duration_ns, 0)",
        TraceSort::Cost => "t.total_cost",
        TraceSort::SpanCount => "t.span_count",
        TraceSort::Tokens => "(t.input_tokens + t.output_tokens)",
        TraceSort::Score => "COALESCE(t.min_score, 0)",
    };
    let dir = match q.order {
        Order::Asc => "ASC",
        Order::Desc => "DESC",
    };

    // Keyset pagination. OFFSET would drift as new traces arrive during
    // paging — the reader would see the same row twice or miss one entirely.
    if let Some(c) = &q.cursor {
        let cmp = if q.order == Order::Asc { '>' } else { '<' };
        w.clauses.push(format!("({sort_col}, t.trace_id) {cmp} (?, ?)"));
        // Cost and score are compared as the floats they are stored as. The
        // cursor used to carry cost scaled to an integer, which dropped every
        // digit under a millionth: the value it resumed from was below the row
        // it came from, so that row satisfied the predicate again and the walk
        // returned page one for ever.
        match q.sort {
            TraceSort::Cost | TraceSort::Score => {
                let (v, id) = decode_cursor_f64(c)?;
                w.binds.push(Box::new(v));
                w.binds.push(Box::new(id));
            }
            _ => {
                let (v, id) = decode_cursor(c)?;
                w.binds.push(Box::new(v));
                w.binds.push(Box::new(id));
            }
        }
    }

    let limit = q.limit.clamp(1, 500);
    // `min_score` is a column of the projection, not a field of the trace the
    // caller gets back, so the score cursor is selected beside the row. Without
    // it the next page would have to guess the value it must resume from.
    let scoring = q.sort == TraceSort::Score;
    let extra = if scoring { ", COALESCE(t.min_score, 0) AS min_score" } else { "" };
    let sql = format!(
        "SELECT {TRACE_COLUMNS}{extra} FROM traces t{} \
         ORDER BY {sort_col} {dir}, t.trace_id {dir} LIMIT {}",
        w.sql(),
        limit + 1
    );

    let mut stmt = conn.prepare(&sql).map_err(be)?;
    let mut rows: Vec<(Trace, f64)> = stmt
        .query_map(params_from_iter(w.refs()), |r| {
            let min_score = if scoring { r.get("min_score")? } else { 0.0 };
            Ok((row_to_trace(r)?, min_score))
        })
        .map_err(be)?
        .collect::<std::result::Result<_, _>>()
        .map_err(be)?;

    let next_cursor = if rows.len() > limit as usize {
        rows.truncate(limit as usize);
        rows.last()
            .map(|(t, min_score)| encode_cursor(q.sort.cursor_value(t, *min_score), &t.trace_id))
    } else {
        None
    };

    Ok(Paged { items: rows.into_iter().map(|(t, _)| t).collect(), next_cursor, total: None })
}

pub(super) fn get_trace_sync(
    conn: &Connection,
    project: &str,
    trace_id: &str,
    include: &Include,
) -> Result<TraceDetail> {
    let trace: Trace = conn
        .query_row(
            &format!("SELECT {TRACE_COLUMNS} FROM traces WHERE trace_id = ?1 AND project = ?2"),
            rusqlite::params![trace_id, project],
            row_to_trace,
        )
        .optional()
        .map_err(be)?
        .ok_or_else(|| Error::NotFound { kind: "trace", id: trace_id.to_owned() })?;

    // Ordered so a waterfall renders straight from the array: parents before
    // children, siblings in the order they started.
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {SPAN_COLUMNS} FROM spans WHERE trace_id = ?1 AND project = ?2 \
             ORDER BY start_time ASC, span_id ASC"
        ))
        .map_err(be)?;
    let spans: Vec<Span> = stmt
        .query_map(rusqlite::params![trace_id, project], row_to_span)
        .map_err(be)?
        .collect::<std::result::Result<_, _>>()
        .map_err(be)?;

    let logs = if include.logs {
        let mut s = conn
            .prepare(&format!(
                "SELECT {LOG_COLUMNS} FROM logs WHERE trace_id = ?1 AND project = ?2 \
                 ORDER BY timestamp ASC LIMIT 5000"
            ))
            .map_err(be)?;
        let rows = s
            .query_map(rusqlite::params![trace_id, project], row_to_log)
            .map_err(be)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(be)?;
        Some(rows)
    } else {
        None
    };

    let scores = if include.scores {
        let mut s = conn
            .prepare(&format!(
                "SELECT {SCORE_COLUMNS} FROM scores WHERE trace_id = ?1 AND project = ?2 \
                 ORDER BY timestamp ASC"
            ))
            .map_err(be)?;
        let rows = s
            .query_map(rusqlite::params![trace_id, project], row_to_score)
            .map_err(be)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(be)?;
        Some(rows)
    } else {
        None
    };

    let feedback = if include.feedback {
        let mut s = conn
            .prepare(&format!(
                "SELECT {FEEDBACK_COLUMNS} FROM feedback WHERE trace_id = ?1 AND project = ?2 \
                 ORDER BY timestamp ASC"
            ))
            .map_err(be)?;
        let rows = s
            .query_map(rusqlite::params![trace_id, project], row_to_feedback)
            .map_err(be)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(be)?;
        Some(rows)
    } else {
        None
    };

    Ok(TraceDetail { trace, spans, logs, scores, feedback })
}

pub(super) fn list_spans_sync(conn: &Connection, q: &SpanQuery) -> Result<Paged<Span>> {
    let mut w = Where::default();
    w.push("project = ?", q.project.clone());
    if let Some(k) = q.kind {
        w.push("kind = ?", k.as_str().to_owned());
    }
    if let Some(s) = q.status {
        w.push("status = ?", s.as_str().to_owned());
    }
    if let Some(n) = &q.name {
        w.push("name = ?", n.clone());
    }
    if let Some(m) = &q.model {
        w.push("model = ?", m.clone());
    }
    if let Some(t) = &q.trace_id {
        w.push("trace_id = ?", t.clone());
    }
    if let Some(f) = q.from {
        w.push("start_time >= ?", to_nanos(f));
    }
    if let Some(t) = q.to {
        w.push("start_time < ?", to_nanos(t));
    }
    if let Some(d) = q.min_duration_ms {
        w.push("duration_ns >= ?", (d as i64).saturating_mul(1_000_000));
    }
    if let Some(raw) = &q.q {
        if let Some(fts) = fts_query(raw) {
            w.clauses.push(
                "span_id IN (SELECT span_id FROM spans_fts WHERE spans_fts MATCH ? AND project = ?)"
                    .to_owned(),
            );
            w.binds.push(Box::new(fts));
            w.binds.push(Box::new(q.project.clone()));
        }
    }
    // Both alternatives to `start_time` are nullable, and SQLite and Postgres
    // put a NULL at opposite ends of the same ORDER BY. COALESCE decides it
    // here instead, so the two backends answer the same question the same way.
    let sort_col = match q.sort {
        SpanSort::StartTime => "start_time",
        SpanSort::Duration => "COALESCE(duration_ns, 0)",
        SpanSort::Cost => "COALESCE(total_cost, 0)",
    };

    if let Some(c) = &q.cursor {
        let cmp = if q.order == Order::Asc { '>' } else { '<' };
        w.clauses.push(format!("({sort_col}, span_id) {cmp} (?, ?)"));
        // Cost is compared as the float it is stored as, for the reason set out
        // over the trace listing: a rounded cursor either repeats a row or
        // skips one.
        if q.sort == SpanSort::Cost {
            let (v, id) = decode_cursor_f64(c)?;
            w.binds.push(Box::new(v));
            w.binds.push(Box::new(id));
        } else {
            let (v, id) = decode_cursor(c)?;
            w.binds.push(Box::new(v));
            w.binds.push(Box::new(id));
        }
    }

    let dir = if q.order == Order::Asc { "ASC" } else { "DESC" };
    let limit = q.limit.clamp(1, 500);
    let sql = format!(
        "SELECT {SPAN_COLUMNS} FROM spans{} ORDER BY {sort_col} {dir}, span_id {dir} LIMIT {}",
        w.sql(),
        limit + 1
    );
    let mut stmt = conn.prepare(&sql).map_err(be)?;
    let mut rows: Vec<Span> = stmt
        .query_map(params_from_iter(w.refs()), row_to_span)
        .map_err(be)?
        .collect::<std::result::Result<_, _>>()
        .map_err(be)?;

    let next_cursor = if rows.len() > limit as usize {
        rows.truncate(limit as usize);
        rows.last().map(|s| encode_cursor(q.sort.cursor_value(s), &s.span_id))
    } else {
        None
    };
    Ok(Paged { items: rows, next_cursor, total: None })
}

pub(super) fn list_logs_sync(conn: &Connection, q: &LogQuery) -> Result<Paged<LogRecord>> {
    let mut w = Where::default();
    w.push("project = ?", q.project.clone());
    if let Some(l) = q.level {
        w.push("severity >= ?", i64::from(l.severity()));
    }
    if let Some(t) = &q.trace_id {
        w.push("trace_id = ?", t.clone());
    }
    if let Some(s) = &q.span_id {
        w.push("span_id = ?", s.clone());
    }
    for (field, value) in &q.correlation {
        // Only a name from the fixed list reaches the SQL, so the column is
        // never caller text.
        if let Some(col) = crate::LOG_CORRELATION_FILTERS.iter().find(|c| **c == field.as_str()) {
            w.push(&format!("{col} = ?"), value.clone());
        }
    }
    if let Some(f) = q.from {
        w.push("timestamp >= ?", to_nanos(f));
    }
    if let Some(t) = q.to {
        w.push("timestamp < ?", to_nanos(t));
    }
    if let Some(raw) = &q.q {
        if let Some(fts) = fts_query(raw) {
            w.clauses.push(
                "log_id IN (SELECT log_id FROM logs_fts WHERE logs_fts MATCH ? AND project = ?)"
                    .to_owned(),
            );
            w.binds.push(Box::new(fts));
            w.binds.push(Box::new(q.project.clone()));
        }
    }
    if let Some(c) = &q.cursor {
        let (v, id) = decode_cursor(c)?;
        let cmp = if q.order == Order::Asc { '>' } else { '<' };
        w.clauses.push(format!("(timestamp, log_id) {cmp} (?, ?)"));
        w.binds.push(Box::new(v));
        w.binds.push(Box::new(id));
    }

    let dir = if q.order == Order::Asc { "ASC" } else { "DESC" };
    let limit = q.limit.clamp(1, 1000);
    let sql = format!(
        "SELECT {LOG_COLUMNS} FROM logs{} ORDER BY timestamp {dir}, log_id {dir} LIMIT {}",
        w.sql(),
        limit + 1
    );
    let mut stmt = conn.prepare(&sql).map_err(be)?;
    let mut rows: Vec<LogRecord> = stmt
        .query_map(params_from_iter(w.refs()), row_to_log)
        .map_err(be)?
        .collect::<std::result::Result<_, _>>()
        .map_err(be)?;
    let next_cursor = if rows.len() > limit as usize {
        rows.truncate(limit as usize);
        rows.last().map(|l| encode_cursor(to_nanos(l.timestamp), &l.log_id))
    } else {
        None
    };
    Ok(Paged { items: rows, next_cursor, total: None })
}

pub(super) fn list_feedback_sync(
    conn: &Connection,
    q: &crate::FeedbackQuery,
) -> Result<Paged<Feedback>> {
    let mut w = Where::default();
    w.push("project = ?", q.project.clone());
    if let Some(t) = &q.trace_id {
        w.push("trace_id = ?", t.clone());
    }
    if let Some(s) = &q.span_id {
        w.push("span_id = ?", s.clone());
    }
    if let Some(k) = &q.kind {
        w.push("kind = ?", k.clone());
    }
    if let Some(a) = &q.author_kind {
        w.push("author_kind = ?", a.clone());
    }
    if let Some(f) = q.from {
        w.push("timestamp >= ?", to_nanos(f));
    }
    if let Some(t) = q.to {
        w.push("timestamp < ?", to_nanos(t));
    }
    if let Some(c) = &q.cursor {
        let (v, id) = decode_cursor(c)?;
        w.clauses.push("(timestamp, feedback_id) < (?, ?)".to_owned());
        w.binds.push(Box::new(v));
        w.binds.push(Box::new(id));
    }
    let limit = q.limit.clamp(1, 500);
    let sql = format!(
        "SELECT {FEEDBACK_COLUMNS} FROM feedback{} ORDER BY timestamp DESC, feedback_id DESC \
         LIMIT {}",
        w.sql(),
        limit + 1
    );
    let mut stmt = conn.prepare(&sql).map_err(be)?;
    let mut rows: Vec<Feedback> = stmt
        .query_map(params_from_iter(w.refs()), row_to_feedback)
        .map_err(be)?
        .collect::<std::result::Result<_, _>>()
        .map_err(be)?;
    let next_cursor = if rows.len() > limit as usize {
        rows.truncate(limit as usize);
        rows.last().map(|f| encode_cursor(to_nanos(f.timestamp), &f.feedback_id))
    } else {
        None
    };
    Ok(Paged { items: rows, next_cursor, total: None })
}

pub(super) fn list_metric_points_sync(
    conn: &Connection,
    q: &crate::MetricPointQuery,
) -> Result<Paged<mcpd_core::MetricPoint>> {
    let mut w = Where::default();
    w.push("project = ?", q.project.clone());
    if let Some(n) = &q.name {
        w.push("name = ?", n.clone());
    }
    if let Some(t) = &q.trace_id {
        w.push("trace_id = ?", t.clone());
    }
    if let Some(f) = q.from {
        w.push("timestamp >= ?", to_nanos(f));
    }
    if let Some(t) = q.to {
        w.push("timestamp < ?", to_nanos(t));
    }
    for (k, v) in &q.filter {
        // The label key is a bound JSON path, never interpolated.
        w.clauses.push("json_extract(labels, ?) = ?".to_owned());
        w.binds.push(Box::new(format!("$.{k}")));
        w.binds.push(Box::new(v.clone()));
    }
    if let Some(c) = &q.cursor {
        let (v, id) = decode_cursor(c)?;
        let cmp = if q.order == Order::Asc { '>' } else { '<' };
        w.clauses.push(format!("(timestamp, metric_id) {cmp} (?, ?)"));
        w.binds.push(Box::new(v));
        w.binds.push(Box::new(id));
    }
    let dir = if q.order == Order::Asc { "ASC" } else { "DESC" };
    let limit = q.limit.clamp(1, 10_000);
    let sql = format!(
        "SELECT metric_id, name, kind, value, unit, timestamp, labels, buckets, trace_id, \
                span_id, project, environment, received_at
         FROM metrics{} ORDER BY timestamp {dir}, metric_id {dir} LIMIT {}",
        w.sql(),
        limit + 1
    );
    let mut stmt = conn.prepare(&sql).map_err(be)?;
    let mut rows: Vec<mcpd_core::MetricPoint> = stmt
        .query_map(params_from_iter(w.refs()), |r| {
            Ok(mcpd_core::MetricPoint {
                metric_id: r.get(0)?,
                name: r.get(1)?,
                kind: r.get::<_, String>(2)?.parse().unwrap_or(mcpd_core::MetricKind::Gauge),
                value: r.get(3)?,
                unit: r.get(4)?,
                timestamp: from_nanos(r.get(5)?),
                labels: json_in(r.get(6)?),
                buckets: json_in(r.get(7)?),
                trace_id: r.get(8)?,
                span_id: r.get(9)?,
                correlation: mcpd_core::Correlation::for_project(r.get(10)?, r.get(11)?),
                received_at: from_nanos(r.get(12)?),
            })
        })
        .map_err(be)?
        .collect::<std::result::Result<_, _>>()
        .map_err(be)?;
    let next_cursor = if rows.len() > limit as usize {
        rows.truncate(limit as usize);
        rows.last().map(|m| encode_cursor(to_nanos(m.timestamp), &m.metric_id))
    } else {
        None
    };
    Ok(Paged { items: rows, next_cursor, total: None })
}

pub(super) fn list_scores_sync(conn: &Connection, q: &ScoreQuery) -> Result<Paged<Score>> {
    let mut w = Where::default();
    w.push("project = ?", q.project.clone());
    if let Some(s) = &q.scorer_id {
        w.push("scorer_id = ?", s.clone());
    }
    if let Some(src) = q.source {
        w.push("source = ?", src.as_str().to_owned());
    }
    if let Some(t) = &q.trace_id {
        w.push("trace_id = ?", t.clone());
    }
    if let Some(e) = &q.experiment_id {
        w.push("experiment_id = ?", e.clone());
    }
    if let Some(v) = q.min_value {
        w.push("value >= ?", v);
    }
    if let Some(v) = q.max_value {
        w.push("value <= ?", v);
    }
    if let Some(f) = q.from {
        w.push("timestamp >= ?", to_nanos(f));
    }
    if let Some(t) = q.to {
        w.push("timestamp < ?", to_nanos(t));
    }
    if let Some(c) = &q.cursor {
        let (v, id) = decode_cursor(c)?;
        w.clauses.push("(timestamp, score_id) < (?, ?)".to_owned());
        w.binds.push(Box::new(v));
        w.binds.push(Box::new(id));
    }
    let limit = q.limit.clamp(1, 500);
    let sql = format!(
        "SELECT {SCORE_COLUMNS} FROM scores{} ORDER BY timestamp DESC, score_id DESC LIMIT {}",
        w.sql(),
        limit + 1
    );
    let mut stmt = conn.prepare(&sql).map_err(be)?;
    let mut rows: Vec<Score> = stmt
        .query_map(params_from_iter(w.refs()), row_to_score)
        .map_err(be)?
        .collect::<std::result::Result<_, _>>()
        .map_err(be)?;
    let next_cursor = if rows.len() > limit as usize {
        rows.truncate(limit as usize);
        rows.last().map(|s| encode_cursor(to_nanos(s.timestamp), &s.score_id))
    } else {
        None
    };
    Ok(Paged { items: rows, next_cursor, total: None })
}

// ------------------------------------------------------------- analytics ---

/// One exact quantile, read straight off the index.
///
/// SQLite has no percentile function. Sorting and taking the nth row is exact
/// and, with an index on the column, costs one seek — far better than pulling
/// every duration into memory to sort it here.
fn quantile(
    conn: &Connection,
    column: &str,
    where_sql: &str,
    binds: &[&dyn ToSql],
    count: i64,
    p: f64,
) -> Result<Option<f64>> {
    if count == 0 {
        return Ok(None);
    }
    // Nearest rank: the ceil(p * n)-th value, 1-indexed. The alternative,
    // (n-1)*p rounded, returns the maximum for p50 when n is 2, which reads as
    // a broken chart on a low-traffic project.
    let rank = ((p * count as f64).ceil() as i64).clamp(1, count);
    let offset = rank - 1;
    let sql = format!(
        "SELECT {column} FROM traces{where_sql} AND {column} IS NOT NULL \
         ORDER BY {column} ASC LIMIT 1 OFFSET {offset}"
    );
    let v: Option<i64> = conn
        .query_row(&sql, params_from_iter(binds.iter().copied()), |r| r.get(0))
        .optional()
        .map_err(be)?;
    // Stored in nanoseconds, reported in milliseconds.
    Ok(v.map(|n| n as f64 / 1_000_000.0))
}

pub(super) fn summary_sync(
    conn: &Connection,
    project: &str,
    from: OffsetDateTime,
    to: OffsetDateTime,
    group_by: Option<&str>,
) -> Result<AnalyticsSummary> {
    let (f, t) = (to_nanos(from), to_nanos(to));
    let where_sql = " WHERE project = ?1 AND start_time >= ?2 AND start_time < ?3";
    let binds: Vec<&dyn ToSql> = vec![&project, &f, &t];

    let (
        trace_count,
        error_count,
        span_count,
        in_tok,
        out_tok,
        cost,
        mean_ns,
        max_ns,
        completed_count,
    ): (i64, i64, i64, i64, i64, f64, Option<f64>, Option<i64>, i64) = conn
        .query_row(
            &format!(
                "SELECT COUNT(*),
                        SUM(CASE WHEN status = 'error' THEN 1 ELSE 0 END),
                        SUM(span_count),
                        SUM(input_tokens), SUM(output_tokens), SUM(total_cost),
                        AVG(duration_ns), MAX(duration_ns),
                        COUNT(duration_ns)
                 FROM traces{where_sql}"
            ),
            params_from_iter(binds.iter().copied()),
            |r| {
                Ok((
                    r.get(0)?,
                    r.get::<_, Option<i64>>(1)?.unwrap_or(0),
                    r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                    r.get::<_, Option<i64>>(3)?.unwrap_or(0),
                    r.get::<_, Option<i64>>(4)?.unwrap_or(0),
                    r.get::<_, Option<f64>>(5)?.unwrap_or(0.0),
                    r.get(6)?,
                    r.get(7)?,
                    r.get(8)?,
                ))
            },
        )
        .map_err(be)?;

    // The quantile offsets into the rows that HAVE a duration, so it has to be
    // told how many those are. Passing `trace_count` overshot the row set
    // whenever a run was still in flight: p50 came back as the maximum of the
    // finished runs and the higher percentiles came back null.
    let latency = LatencyStats {
        p50: quantile(conn, "duration_ns", where_sql, &binds, completed_count, 0.50)?,
        p90: quantile(conn, "duration_ns", where_sql, &binds, completed_count, 0.90)?,
        p95: quantile(conn, "duration_ns", where_sql, &binds, completed_count, 0.95)?,
        p99: quantile(conn, "duration_ns", where_sql, &binds, completed_count, 0.99)?,
        mean: mean_ns.map(|n| n / 1_000_000.0),
        max: max_ns.map(|n| n as f64 / 1_000_000.0),
    };

    let groups = match group_by {
        Some(dim) => Some(grouped(conn, project, f, t, dim)?),
        None => None,
    };

    Ok(AnalyticsSummary {
        from,
        to,
        trace_count: trace_count.max(0) as u64,
        span_count: span_count.max(0) as u64,
        error_count: error_count.max(0) as u64,
        error_rate: if trace_count > 0 { error_count as f64 / trace_count as f64 } else { 0.0 },
        latency,
        input_tokens: in_tok.max(0) as u64,
        output_tokens: out_tok.max(0) as u64,
        total_cost: cost,
        currency: "USD".to_owned(),
        groups,
    })
}

/// Map a caller-supplied dimension to a column.
///
/// An allowlist, not string interpolation: `group_by` comes straight off the
/// query string, and anything that reaches SQL unchecked is an injection.
fn group_column(dim: &str) -> Result<(&'static str, &'static str)> {
    Ok(match dim {
        "model" => ("s.model", "spans"),
        "provider" => ("s.provider", "spans"),
        "entity_name" => ("s.entity_name", "spans"),
        "environment" => ("t.environment", "traces"),
        "status" => ("t.status", "traces"),
        "session_id" => ("t.session_id", "traces"),
        "user_id" => ("t.user_id", "traces"),
        "service_name" => ("t.service_name", "traces"),
        "name" => ("t.name", "traces"),
        other => {
            return Err(Error::InvalidQuery(format!(
                "cannot group by {other:?}; supported: model, provider, entity_name, \
                 environment, status, session_id, user_id, service_name, name"
            )))
        }
    })
}

fn grouped(
    conn: &Connection,
    project: &str,
    from: i64,
    to: i64,
    dim: &str,
) -> Result<Vec<GroupedStat>> {
    let (col, table) = group_column(dim)?;
    let sql = if table == "traces" {
        format!(
            "SELECT COALESCE({col}, '(none)'), COUNT(*),
                    SUM(CASE WHEN t.status = 'error' THEN 1 ELSE 0 END),
                    SUM(t.total_cost), SUM(t.input_tokens), SUM(t.output_tokens),
                    AVG(t.duration_ns), MAX(t.duration_ns)
             FROM traces t
             WHERE t.project = ?1 AND t.start_time >= ?2 AND t.start_time < ?3
             GROUP BY 1 ORDER BY 2 DESC LIMIT 100"
        )
    } else {
        format!(
            "SELECT COALESCE({col}, '(none)'), COUNT(*),
                    SUM(CASE WHEN s.status = 'error' THEN 1 ELSE 0 END),
                    SUM(COALESCE(s.total_cost, 0)),
                    SUM(COALESCE(s.input_tokens, 0)), SUM(COALESCE(s.output_tokens, 0)),
                    AVG(s.duration_ns), MAX(s.duration_ns)
             FROM spans s
             WHERE s.project = ?1 AND s.start_time >= ?2 AND s.start_time < ?3
               AND {col} IS NOT NULL
             GROUP BY 1 ORDER BY 2 DESC LIMIT 100"
        )
    };
    let mut stmt = conn.prepare(&sql).map_err(be)?;
    let rows = stmt
        .query_map(rusqlite::params![project, from, to], |r| {
            let mean: Option<f64> = r.get(6)?;
            let max: Option<i64> = r.get(7)?;
            Ok(GroupedStat {
                key: r.get(0)?,
                count: r.get::<_, i64>(1)?.max(0) as u64,
                error_count: r.get::<_, Option<i64>>(2)?.unwrap_or(0).max(0) as u64,
                total_cost: r.get::<_, Option<f64>>(3)?,
                input_tokens: r.get::<_, Option<i64>>(4)?.map(|v| v.max(0) as u64),
                output_tokens: r.get::<_, Option<i64>>(5)?.map(|v| v.max(0) as u64),
                latency: Some(LatencyStats {
                    p50: None,
                    p90: None,
                    p95: None,
                    p99: None,
                    mean: mean.map(|n| n / 1_000_000.0),
                    max: max.map(|n| n as f64 / 1_000_000.0),
                }),
            })
        })
        .map_err(be)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(be)?;
    Ok(rows)
}

pub(super) fn cost_sync(
    conn: &Connection,
    project: &str,
    from: OffsetDateTime,
    to: OffsetDateTime,
    group_by: &str,
    bucket_seconds: Option<u32>,
) -> Result<CostBreakdown> {
    let (f, t) = (to_nanos(from), to_nanos(to));
    let mut groups = grouped(conn, project, f, t, group_by)?;
    groups.sort_by(|a, b| {
        b.total_cost
            .unwrap_or(0.0)
            .partial_cmp(&a.total_cost.unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let total: f64 = groups.iter().filter_map(|g| g.total_cost).sum();

    let series = match bucket_seconds.filter(|b| *b > 0) {
        Some(bucket) => {
            // Branch on the table the dimension lives in, exactly as `grouped`
            // does ten lines up.
            //
            // This did not branch. It interpolated the column from
            // `group_column` into one query that always read `FROM spans s`,
            // and six of the nine dimensions resolve to a `t.` column, so
            // `/v1/api/analytics/cost` with a `bucket` answered HTTP 500 with
            // "no such column: t.environment" for environment, status,
            // session_id, user_id, service_name and name. The three span
            // dimensions worked, which is why it read as a working endpoint.
            //
            // Branching also keeps the series and the totals counting the same
            // thing. `grouped` counts runs for a trace dimension and steps for
            // a span one, so a series built over spans for a trace dimension
            // would have drawn bars that did not add up to the table beside
            // them.
            let (col, table) = group_column(group_by)?;
            let width = i64::from(bucket) * 1_000_000_000;
            let sql = if table == "traces" {
                format!(
                    "SELECT (t.start_time / {width}) * {width} AS bucket,
                            COALESCE({col}, '(none)') AS k,
                            SUM(COALESCE(t.total_cost, 0)),
                            SUM(COALESCE(t.input_tokens, 0)),
                            SUM(COALESCE(t.output_tokens, 0))
                     FROM traces t
                     WHERE t.project = ?1 AND t.start_time >= ?2 AND t.start_time < ?3
                     GROUP BY bucket, k ORDER BY bucket ASC LIMIT 5000"
                )
            } else {
                format!(
                    "SELECT (s.start_time / {width}) * {width} AS bucket,
                            COALESCE({col}, '(none)') AS k,
                            SUM(COALESCE(s.total_cost, 0)),
                            SUM(COALESCE(s.input_tokens, 0)),
                            SUM(COALESCE(s.output_tokens, 0))
                     FROM spans s
                     WHERE s.project = ?1 AND s.start_time >= ?2 AND s.start_time < ?3
                       AND {col} IS NOT NULL
                     GROUP BY bucket, k ORDER BY bucket ASC LIMIT 5000"
                )
            };
            let mut stmt = conn.prepare(&sql).map_err(be)?;
            let rows = stmt
                .query_map(rusqlite::params![project, f, t], |r| {
                    Ok(CostBucket {
                        timestamp: from_nanos(r.get(0)?),
                        key: r.get(1)?,
                        total_cost: r.get::<_, Option<f64>>(2)?.unwrap_or(0.0),
                        input_tokens: r.get::<_, Option<i64>>(3)?.map(|v| v.max(0) as u64),
                        output_tokens: r.get::<_, Option<i64>>(4)?.map(|v| v.max(0) as u64),
                    })
                })
                .map_err(be)?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(be)?;
            Some(rows)
        }
        None => None,
    };

    Ok(CostBreakdown { from, to, currency: "USD".to_owned(), total_cost: total, groups, series })
}

pub(super) fn query_metrics_sync(
    conn: &Connection,
    q: &MetricQuery,
) -> Result<MetricSeriesResponse> {
    let (f, t) = (to_nanos(q.from), to_nanos(q.to));

    // Pick a bucket that yields a readable number of points when the caller
    // did not choose one.
    let bucket = if q.bucket_seconds > 0 {
        q.bucket_seconds
    } else {
        let span_seconds = ((t - f) / 1_000_000_000).max(1);
        let target = span_seconds / 120;
        [1u32, 5, 15, 30, 60, 300, 900, 3600, 21600, 86400]
            .into_iter()
            .find(|b| i64::from(*b) >= target)
            .unwrap_or(86400)
    };
    let width = i64::from(bucket) * 1_000_000_000;

    // Series key: the requested label values, joined. Building it in SQL keeps
    // grouping in the engine rather than folding every raw point in Rust.
    let key_expr = if q.group_by.is_empty() {
        "''".to_owned()
    } else {
        q.group_by
            .iter()
            .map(|k| {
                // json_extract's path is a bound parameter, never interpolated.
                let _ = k;
                "COALESCE(json_extract(labels, ?), '(none)')".to_owned()
            })
            .collect::<Vec<_>>()
            .join(" || char(31) || ")
    };

    // Quantiles are exact.
    //
    // SQLite has no percentile aggregate, but it does have window functions.
    // Numbering each row within its bucket by value and picking the
    // ceil(p * n)-th is the nearest-rank definition, computed in the engine over
    // the same buckets the other aggregates use. This replaces a fallback that
    // returned the mean and called it a quantile, which is the kind of number
    // that quietly makes a latency dashboard lie.
    let quantile = match q.agg {
        Aggregation::P50 => Some(0.50),
        Aggregation::P90 => Some(0.90),
        Aggregation::P95 => Some(0.95),
        Aggregation::P99 => Some(0.99),
        _ => None,
    };

    let agg_expr = match q.agg {
        Aggregation::Sum => "SUM(value)",
        Aggregation::Count => "CAST(COUNT(*) AS REAL)",
        Aggregation::Mean => "AVG(value)",
        Aggregation::Min => "MIN(value)",
        Aggregation::Max => "MAX(value)",
        // Handled by the windowed query below, never by this expression.
        Aggregation::P50 | Aggregation::P90 | Aggregation::P95 | Aggregation::P99 => "AVG(value)",
    };

    // Label filters are matched with json_extract against a bound path, never
    // by interpolating the key into SQL.
    let filter_sql: String =
        q.filter.iter().map(|_| " AND json_extract(labels, ?) = ?".to_owned()).collect();

    let sql = match quantile {
        None => format!(
            "SELECT (timestamp / {width}) * {width} AS bucket,
                    {key_expr} AS series_key,
                    {agg_expr}, COUNT(*), MAX(trace_id)
             FROM metrics
             WHERE project = ? AND name = ? AND timestamp >= ? AND timestamp < ?{filter_sql}
             GROUP BY bucket, series_key
             ORDER BY bucket ASC
             LIMIT 20000"
        ),
        // Rank every point inside its own bucket, then take the nearest rank.
        // `ranked` carries the bucket size along so the rank can be computed
        // without a second pass over the data.
        // `series_key` is computed once, in `base`, and the window functions
        // partition by the alias. Repeating `{key_expr}` inside both OVER
        // clauses put three copies of its bound parameters in the statement
        // while the caller pushed them once, so every percentile query with a
        // `group_by` failed outright with a parameter-count error.
        Some(p) => format!(
            "WITH base AS (
               SELECT (timestamp / {width}) * {width} AS bucket,
                      {key_expr} AS series_key,
                      value,
                      trace_id
               FROM metrics
               WHERE project = ? AND name = ? AND timestamp >= ? AND timestamp < ?{filter_sql}
             ),
             ranked AS (
               SELECT bucket, series_key, value, trace_id,
                      ROW_NUMBER() OVER (
                        PARTITION BY bucket, series_key ORDER BY value
                      ) AS rn,
                      COUNT(*) OVER (PARTITION BY bucket, series_key) AS n
               FROM base
             )
             SELECT bucket, series_key, value, n, trace_id
             FROM ranked
             WHERE rn = MAX(1, CAST(({p} * n) + 0.9999999 AS INTEGER))
             ORDER BY bucket ASC
             LIMIT 20000"
        ),
    };

    let mut binds: Vec<Box<dyn ToSql>> = Vec::new();
    for k in &q.group_by {
        binds.push(Box::new(format!("$.{k}")));
    }
    binds.push(Box::new(q.project.clone()));
    binds.push(Box::new(q.name.clone()));
    binds.push(Box::new(f));
    binds.push(Box::new(t));
    for (k, v) in &q.filter {
        binds.push(Box::new(format!("$.{k}")));
        binds.push(Box::new(v.clone()));
    }

    let mut stmt = conn.prepare(&sql).map_err(be)?;
    let rows = stmt
        .query_map(params_from_iter(binds.iter().map(|b| b.as_ref() as &dyn ToSql)), |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<f64>>(2)?.unwrap_or(0.0),
                r.get::<_, i64>(3)?,
                r.get::<_, Option<String>>(4)?,
            ))
        })
        .map_err(be)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(be)?;

    let unit: Option<String> = conn
        .query_row(
            "SELECT unit FROM metrics WHERE project = ?1 AND name = ?2 AND unit IS NOT NULL LIMIT 1",
            rusqlite::params![q.project, q.name],
            |r| r.get(0),
        )
        .optional()
        .map_err(be)?
        .flatten();

    // Fold the flat rows into one series per label combination, preserving the
    // order the labels were requested in.
    let mut series: Vec<MetricSeries> = Vec::new();
    let mut index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for (bucket_ts, key, value, count, exemplar) in rows {
        let idx = *index.entry(key.clone()).or_insert_with(|| {
            let labels = if q.group_by.is_empty() {
                None
            } else {
                let parts: Vec<&str> = key.split('\u{1f}').collect();
                Some(Value::Object(
                    q.group_by
                        .iter()
                        .zip(parts)
                        .map(|(k, v)| (k.clone(), Value::String(v.to_owned())))
                        .collect(),
                ))
            };
            series.push(MetricSeries { labels, points: Vec::new() });
            series.len() - 1
        });
        series[idx].points.push(MetricSeriesPoint {
            timestamp: from_nanos(bucket_ts),
            value,
            count: Some(count.max(0) as u64),
            exemplar_trace_id: exemplar,
        });
    }

    Ok(MetricSeriesResponse { name: q.name.clone(), unit, bucket: format!("{bucket}s"), series })
}

// ----------------------------------------------- datasets and experiments ---

pub(super) fn put_dataset_sync(conn: &Connection, d: &Dataset) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO datasets
         (dataset_id, name, description, item_count, created_at, project, environment)
         VALUES (?1,?2,?3,?4,?5,?6,?7)",
        rusqlite::params![
            d.dataset_id,
            d.name,
            d.description,
            d.item_count,
            to_nanos(d.created_at),
            d.correlation.project,
            d.correlation.environment,
        ],
    )
    .map_err(be)?;
    Ok(())
}

pub(super) fn list_datasets_sync(conn: &Connection, project: &str) -> Result<Vec<Dataset>> {
    let mut stmt = conn
        .prepare(
            "SELECT dataset_id, name, description, item_count, created_at, project, environment
             FROM datasets WHERE project = ?1 ORDER BY created_at DESC",
        )
        .map_err(be)?;
    let rows = stmt
        .query_map([project], |r| {
            Ok(Dataset {
                dataset_id: r.get(0)?,
                name: r.get(1)?,
                description: r.get(2)?,
                item_count: r.get::<_, i64>(3)?.max(0) as u32,
                created_at: from_nanos(r.get(4)?),
                correlation: Correlation {
                    project: r.get(5)?,
                    environment: r.get(6)?,
                    service_name: None,
                    service_version: None,
                    session_id: None,
                    thread_id: None,
                    user_id: None,
                    resource_id: None,
                    run_id: None,
                    request_id: None,
                    experiment_id: None,
                    release: None,
                },
            })
        })
        .map_err(be)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(be)?;
    Ok(rows)
}

pub(super) fn add_items_sync(
    conn: &mut Connection,
    dataset_id: &str,
    items: &[DatasetItem],
) -> Result<u32> {
    let tx = conn.transaction().map_err(be)?;
    {
        let mut held = tx
            .prepare_cached("SELECT dataset_id FROM dataset_items WHERE item_id = ?1")
            .map_err(be)?;
        for it in items {
            let owner = owner_of(&mut held, [&it.item_id])?;
            if crate::foreign(owner.as_deref(), dataset_id) {
                return Err(crate::item_taken(&it.item_id));
            }
        }
        let mut stmt = tx
            .prepare_cached(
                "INSERT OR REPLACE INTO dataset_items
                 (item_id, dataset_id, input, expected, metadata, source_trace_id)
                 VALUES (?1,?2,?3,?4,?5,?6)",
            )
            .map_err(be)?;
        for it in items {
            stmt.execute(rusqlite::params![
                it.item_id,
                dataset_id,
                serde_json::to_string(&it.input).unwrap_or_else(|_| "null".into()),
                json_out(&it.expected),
                json_out(&it.metadata),
                it.source_trace_id,
            ])
            .map_err(be)?;
        }
        // Keep the count honest rather than incrementing it, so a replaced item
        // does not inflate it.
        tx.execute(
            "UPDATE datasets SET item_count =
               (SELECT COUNT(*) FROM dataset_items WHERE dataset_id = ?1)
             WHERE dataset_id = ?1",
            [dataset_id],
        )
        .map_err(be)?;
    }
    tx.commit().map_err(be)?;
    Ok(items.len() as u32)
}

pub(super) fn list_items_sync(
    conn: &Connection,
    dataset_id: &str,
    limit: u32,
) -> Result<Vec<DatasetItem>> {
    let mut stmt = conn
        .prepare(
            "SELECT item_id, input, expected, metadata, source_trace_id
             FROM dataset_items WHERE dataset_id = ?1 LIMIT ?2",
        )
        .map_err(be)?;
    let rows = stmt
        .query_map(rusqlite::params![dataset_id, limit.clamp(1, 5000)], |r| {
            Ok(DatasetItem {
                item_id: r.get(0)?,
                dataset_id: dataset_id.to_owned(),
                input: json_in(r.get(1)?).unwrap_or(serde_json::Value::Null),
                expected: json_in(r.get(2)?),
                metadata: json_in(r.get(3)?),
                source_trace_id: r.get(4)?,
            })
        })
        .map_err(be)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(be)?;
    Ok(rows)
}

pub(super) fn put_experiment_sync(conn: &Connection, e: &Experiment) -> Result<()> {
    let owner: Option<String> = conn
        .query_row(
            "SELECT project FROM experiments WHERE experiment_id = ?1",
            [&e.experiment_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(be)?;
    if crate::foreign(owner.as_deref(), &e.correlation.project) {
        return Err(crate::experiment_taken(&e.experiment_id));
    }
    conn.execute(
        "INSERT OR REPLACE INTO experiments
         (experiment_id, name, dataset_id, status, started_at, finished_at, item_count,
          summary, baseline_id, cost_json, error, metadata, project, environment)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
        rusqlite::params![
            e.experiment_id,
            e.name,
            e.dataset_id,
            e.status.as_str(),
            e.started_at.map(to_nanos),
            e.finished_at.map(to_nanos),
            e.item_count,
            json_out(&e.summary),
            e.baseline_id,
            enc(&e.cost),
            e.error,
            json_out(&e.metadata),
            e.correlation.project,
            e.correlation.environment,
        ],
    )
    .map_err(be)?;
    Ok(())
}

pub(super) fn list_experiments_sync(conn: &Connection, project: &str) -> Result<Vec<Experiment>> {
    let mut stmt = conn
        .prepare(
            "SELECT experiment_id, name, dataset_id, status, started_at, finished_at,
                    item_count, summary, baseline_id, cost_json, error, metadata,
                    project, environment
             FROM experiments WHERE project = ?1 ORDER BY COALESCE(started_at, 0) DESC",
        )
        .map_err(be)?;
    let rows = stmt
        .query_map([project], |r| {
            Ok(Experiment {
                experiment_id: r.get(0)?,
                name: r.get(1)?,
                dataset_id: r.get(2)?,
                status: mcpd_core::ExperimentStatus::from_str(&r.get::<_, String>(3)?)
                    .unwrap_or(mcpd_core::ExperimentStatus::Pending),
                started_at: r.get::<_, Option<i64>>(4)?.map(from_nanos),
                finished_at: r.get::<_, Option<i64>>(5)?.map(from_nanos),
                item_count: r.get::<_, i64>(6)?.max(0) as u32,
                summary: json_in(r.get(7)?),
                baseline_id: r.get(8)?,
                cost: dec(r.get(9)?),
                error: r.get(10)?,
                metadata: json_in(r.get(11)?),
                correlation: Correlation {
                    project: r.get(12)?,
                    environment: r.get(13)?,
                    service_name: None,
                    service_version: None,
                    session_id: None,
                    thread_id: None,
                    user_id: None,
                    resource_id: None,
                    run_id: None,
                    request_id: None,
                    experiment_id: None,
                    release: None,
                },
            })
        })
        .map_err(be)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(be)?;
    Ok(rows)
}

// ------------------------------------------------------------------ audit ---

pub(super) fn record_audit_sync(conn: &Connection, e: &AuditEvent) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO audit_log
         (audit_id, timestamp, principal, subject, action, project, resource, result)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
        rusqlite::params![
            e.audit_id,
            to_nanos(e.timestamp),
            e.principal,
            e.subject,
            e.action,
            e.project,
            e.resource,
            e.result.as_str(),
        ],
    )
    .map_err(be)?;
    Ok(())
}

pub(super) fn list_audit_sync(conn: &Connection, q: &AuditQuery) -> Result<Vec<AuditEvent>> {
    let mut w = Where::default();
    w.push("project = ?", q.project.clone());
    if let Some(a) = &q.action {
        w.push("action = ?", a.clone());
    }
    if let Some(f) = q.from {
        w.push("timestamp >= ?", to_nanos(f));
    }
    if let Some(t) = q.to {
        w.push("timestamp < ?", to_nanos(t));
    }
    let sql = format!(
        "SELECT audit_id, timestamp, principal, subject, action, project, resource, result
         FROM audit_log{} ORDER BY timestamp DESC LIMIT {}",
        w.sql(),
        q.limit.clamp(1, 1000)
    );
    let mut stmt = conn.prepare(&sql).map_err(be)?;
    let rows = stmt
        .query_map(params_from_iter(w.refs()), |r| {
            Ok(AuditEvent {
                audit_id: r.get(0)?,
                timestamp: from_nanos(r.get(1)?),
                principal: r.get(2)?,
                subject: r.get(3)?,
                action: r.get(4)?,
                project: r.get(5)?,
                resource: r.get(6)?,
                // An unreadable result column is a row written by a build that
                // knew a variant this one does not. Reporting it as refused is
                // the safe reading: it never claims an access was served.
                result: mcpd_core::AuditResult::from_str(&r.get::<_, String>(7)?)
                    .unwrap_or(mcpd_core::AuditResult::Refused),
            })
        })
        .map_err(be)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(be)?;
    Ok(rows)
}

/* --------------------------------------------------------------- discovery -- */

/// The values one facet takes, for a filter box.
pub(super) fn discover_sync(conn: &Connection, q: &DiscoveryQuery) -> Result<Vec<Discovered>> {
    if q.facet.needs_key() && q.key.as_deref().unwrap_or("").is_empty() {
        return Err(Error::InvalidQuery(
            "metric_label_value needs a key: ask for the values of one label, not for every \
             label's values mixed together"
                .to_owned(),
        ));
    }
    let key = if q.facet.needs_key() { q.key.clone().unwrap_or_default() } else { String::new() };
    let mut sql = String::from(
        "SELECT value, last_seen FROM discovery WHERE project = ?1 AND facet = ?2 AND key = ?3",
    );
    let mut args: Vec<Box<dyn ToSql>> =
        vec![Box::new(q.project.clone()), Box::new(q.facet.as_str()), Box::new(key)];
    if let Some(prefix) = q.prefix.as_ref().filter(|p| !p.is_empty()) {
        // A range over the primary key rather than a LIKE, so typeahead reads
        // the answer and not the list. See `crate::prefix_upper_bound`.
        sql.push_str(" AND value >= ?4");
        args.push(Box::new(prefix.clone()));
        if let Some(upper) = crate::prefix_upper_bound(prefix) {
            sql.push_str(" AND value < ?5");
            args.push(Box::new(upper));
        }
    }
    sql.push_str(" ORDER BY value ASC LIMIT ");
    sql.push_str(&q.effective_limit().to_string());

    let mut stmt = conn.prepare(&sql).map_err(be)?;
    let rows = stmt
        .query_map(params_from_iter(args.iter().map(|a| a.as_ref())), |r| {
            Ok(Discovered { value: r.get(0)?, last_seen: from_nanos(r.get(1)?) })
        })
        .map_err(be)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(be)?;
    Ok(rows)
}

/* ----------------------------------------------------------------- polling -- */

/// What has arrived since the watermark, oldest first.
pub(super) fn poll_sync(conn: &Connection, q: &DeltaQuery) -> Result<Delta<PolledSignal>> {
    let (table, id_col, columns) = match q.signal {
        DeltaSignal::Traces => ("traces", "trace_id", TRACE_COLUMNS),
        DeltaSignal::Spans => ("spans", "span_id", SPAN_COLUMNS),
        DeltaSignal::Logs => ("logs", "log_id", LOG_COLUMNS),
    };
    // No cursor yet: start just before `since`, or at the beginning of time.
    // The id is empty, which sorts before every real id, so the first row at
    // exactly that nanosecond is included rather than skipped.
    let (watermark, after_id) = match &q.cursor {
        Some(c) => decode_delta_cursor(c)?,
        None => (q.since.map_or(i64::MIN, to_nanos), String::new()),
    };
    let limit = match q.limit {
        0 => 100,
        n => n.min(1000),
    };

    let sql = format!(
        "SELECT {columns} FROM {table} \
         WHERE project = ?1 AND (received_at > ?2 OR (received_at = ?2 AND {id_col} > ?3)) \
         ORDER BY received_at ASC, {id_col} ASC LIMIT {}",
        limit + 1
    );
    let mut stmt = conn.prepare(&sql).map_err(be)?;
    let mut rows: Vec<(i64, String, PolledSignal)> = stmt
        .query_map(rusqlite::params![&q.project, watermark, &after_id], |r| {
            // `received_at` and the id are read from the row rather than from
            // the decoded signal, because a `Trace` does not carry its arrival
            // time and a cursor built from `start_time` would resume in the
            // wrong order the first time a run arrived late.
            let received: i64 = r.get("received_at")?;
            let id: String = r.get(id_col)?;
            let item = match q.signal {
                DeltaSignal::Traces => PolledSignal::Trace(Box::new(row_to_trace(r)?)),
                DeltaSignal::Spans => PolledSignal::Span(Box::new(row_to_span(r)?)),
                DeltaSignal::Logs => PolledSignal::Log(Box::new(row_to_log(r)?)),
            };
            Ok((received, id, item))
        })
        .map_err(be)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(be)?;

    let has_more = rows.len() > limit as usize;
    rows.truncate(limit as usize);
    // The cursor is the last row returned, or the one that came in when nothing
    // did. Advancing it past an empty page to "now" is what would drop a row
    // that lands a moment later carrying an earlier arrival time.
    let cursor = rows.last().map_or_else(
        || q.cursor.clone().unwrap_or_else(|| encode_delta_cursor(watermark, &after_id)),
        |(received, id, _)| encode_delta_cursor(*received, id),
    );
    Ok(Delta { items: rows.into_iter().map(|(_, _, item)| item).collect(), cursor, has_more })
}

/* --------------------------------------------------------------- retention -- */

/// Apply a window per signal, in one transaction.
///
/// One transaction so a crash between two signals cannot leave a run whose
/// steps are gone. The FTS rows go first, while the spans naming them still
/// exist: deleting them afterwards would leave them orphaned and searchable.
///
/// The window is measured from when the thing happened, not from when mcpd
/// heard about it: `start_time` for a step and a run, `timestamp` for
/// everything else. A run backfilled an hour after it ran is thirty days old
/// when the run is, which is what somebody setting "keep thirty days of runs"
/// means. `enforce_retention` above measures from arrival instead, and the two
/// are deliberately different methods rather than one with a flag.
///
/// This is also what lets ClickHouse expire a whole day at once: its partitions
/// are cut on the same column, and a window measured on arrival would not line
/// up with them.
pub(super) fn apply_retention_sync(
    conn: &mut Connection,
    project: &str,
    policy: &RetentionPolicy,
    now: OffsetDateTime,
) -> Result<RetentionOutcome> {
    let mut out = RetentionOutcome::default();
    if policy.keeps_everything() {
        return Ok(out);
    }
    let tx = conn.transaction().map_err(be)?;

    if let Some(cut) = RetentionPolicy::cutoff(policy.traces, now).map(to_nanos) {
        tx.execute(
            "DELETE FROM spans_fts WHERE project = ?1 AND span_id IN \
             (SELECT span_id FROM spans WHERE project = ?1 AND start_time < ?2)",
            rusqlite::params![project, cut],
        )
        .map_err(be)?;
        for table in ["spans", "traces"] {
            out.removed += tx
                .execute(
                    &format!("DELETE FROM {table} WHERE project = ?1 AND start_time < ?2"),
                    rusqlite::params![project, cut],
                )
                .map_err(be)? as u64;
        }
        // The idempotency keys expire with the runs they were keying. They
        // exist to catch a retry seconds after the fact and nothing reads one
        // older than that.
        out.removed += tx
            .execute("DELETE FROM ingest_keys WHERE seen_at < ?1", rusqlite::params![cut])
            .map_err(be)? as u64;

        // Aspect notes go with the runs, not on a window of their own, and no
        // flag adds one. A note is a sentence about a trace: outliving its
        // trace it is prose about a run nobody can open, which is a worse thing
        // to keep than nothing. Cut on when the note was written rather than on
        // when its run started, because writing a note lags the run and a note
        // recorded today about a run from last month is still a sentence
        // somebody wrote today.
        //
        // The whole analysis chain goes on the same window and in dependency
        // order: memberships, then themes, then the snapshots that hold them.
        // A theme carries a label that is the text of one of its own notes, so
        // leaving the clustering behind would keep that sentence readable after
        // the note it came from had gone.
        for sql in [
            "DELETE FROM aspect_notes WHERE project = ?1 AND created_at < ?2",
            "DELETE FROM theme_members WHERE project = ?1 AND snapshot_id IN \
             (SELECT snapshot_id FROM theme_snapshots WHERE project = ?1 AND cutoff_at < ?2)",
            "DELETE FROM themes WHERE project = ?1 AND cutoff_at < ?2",
            "DELETE FROM theme_snapshots WHERE project = ?1 AND cutoff_at < ?2",
        ] {
            out.removed += tx.execute(sql, rusqlite::params![project, cut]).map_err(be)? as u64;
        }
    }
    if let Some(cut) = RetentionPolicy::cutoff(policy.logs, now).map(to_nanos) {
        tx.execute(
            "DELETE FROM logs_fts WHERE project = ?1 AND log_id IN \
             (SELECT log_id FROM logs WHERE project = ?1 AND timestamp < ?2)",
            rusqlite::params![project, cut],
        )
        .map_err(be)?;
        out.removed += tx
            .execute(
                "DELETE FROM logs WHERE project = ?1 AND timestamp < ?2",
                rusqlite::params![project, cut],
            )
            .map_err(be)? as u64;
    }
    for (days, table) in
        [(policy.metrics, "metrics"), (policy.scores, "scores"), (policy.feedback, "feedback")]
    {
        if let Some(cut) = RetentionPolicy::cutoff(days, now).map(to_nanos) {
            out.removed += tx
                .execute(
                    &format!("DELETE FROM {table} WHERE project = ?1 AND timestamp < ?2"),
                    rusqlite::params![project, cut],
                )
                .map_err(be)? as u64;
        }
    }

    // The filter box last. A value nothing has carried since before the widest
    // window has no row left to name it, so offering it would filter to an
    // empty list. The widest window is the right cutoff: a value carried by a
    // score that is still stored is still a value the box should offer.
    let widest =
        policy.windows().iter().map(|(_, d)| *d).max_by_key(|d| d.unwrap_or(u32::MAX)).flatten();
    if let Some(cut) = RetentionPolicy::cutoff(widest, now).map(to_nanos) {
        out.removed += tx
            .execute(
                "DELETE FROM discovery WHERE project = ?1 AND last_seen < ?2",
                rusqlite::params![project, cut],
            )
            .map_err(be)? as u64;
    }

    tx.commit().map_err(be)?;
    Ok(out)
}
