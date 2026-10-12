//! The SQL behind the aspect note, snapshot and theme methods.
//!
//! Kept apart from `queries.rs` because it is one family with one shape: every
//! statement here is a single-table scan with a project bound on it, and none
//! of them joins. That is not a stylistic preference. The same six methods have
//! to answer identically on ClickHouse, which joins nowhere, so the entity a
//! note belongs to and the times a theme's snapshot covers are denormalised
//! onto the rows and every backend reads them the same way.

use super::*;
use crate::{
    AspectNoteQuery, AspectNoteRollup, AspectRollupQuery, SnapshotQuery, SnapshotWrite, StoredNote,
    StoredTheme, ThemeLineage, ThemeMember, ThemeMemberQuery, ThemeQuery,
};
use mcpd_core::{AspectNote, Correlation, EntityKind, Theme, ThemeSnapshot, ThemeState};

/// What a trace's root span says about the entity a note belongs to: its kind,
/// its name, and when the run began. All three are absent when the trace is not
/// stored yet, which is the ordinary state of a note that arrived first.
type RootIdentity = (Option<String>, Option<String>, Option<i64>);

/// The columns a stored note is read back from, in one place so the `SELECT`
/// and the row reader cannot drift apart.
const NOTE_COLUMNS: &str = "note_id, trace_id, aspect, text, definition_version, model, \
     created_at, entity_kind, entity_name, trace_started_at, project, environment, service_name, \
     service_version, session_id, thread_id, user_id, resource_id, run_id, request_id, \
     experiment_id, release, sketch";

/// The columns a theme is read back from.
const THEME_COLUMNS: &str = "snapshot_id, aspect, theme_id, project, entity_kind, entity_name, \
     label, label_note_id, label_trace_id, state, note_count, trace_count, coverage, cutoff_at, \
     window_from, window_to, split_from, split_into, merged_from, merged_into";

/// The columns a snapshot is read back from.
const SNAPSHOT_COLUMNS: &str = "snapshot_id, project, entity_kind, entity_name, aspects, ordinal, \
     cutoff_at, window_from, window_to, trace_count, note_count, capped";

/// What happened to one batch of notes: how many were stored, and the
/// positions of those refused because their id belongs to another project.
pub(super) struct NotesWritten {
    pub stored: u32,
    pub refused: Vec<usize>,
}

/// Write the aspect notes of one batch.
///
/// The entity is resolved from each trace's root span, once per distinct trace
/// rather than once per note, and inside the caller's transaction so a note
/// cannot be filed under an entity a concurrent write has since changed.
///
/// A note whose trace is not stored yet keeps a NULL entity. Writing a note
/// lags ingestion, sometimes by a long way, so this is an ordinary state and
/// not a failure: the note is readable by trace id and is absent from the
/// entity roll-ups until nothing more can be done about it. Filing it under a
/// guessed entity would put a sentence on a screen about an agent nobody named.
pub(super) fn write_aspect_notes(
    tx: &rusqlite::Transaction<'_>,
    notes: &[AspectNote],
) -> Result<NotesWritten> {
    let mut written = NotesWritten { stored: 0, refused: Vec::new() };
    if notes.is_empty() {
        return Ok(written);
    }
    let mut ins = tx
        .prepare_cached(
            "INSERT OR REPLACE INTO aspect_notes (
                note_id, trace_id, aspect, text, definition_version, model, created_at,
                entity_kind, entity_name, trace_started_at, sketch, project, environment,
                service_name, service_version, session_id, thread_id, user_id, resource_id,
                run_id, request_id, experiment_id, release
             ) VALUES (
                ?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,
                ?22,?23)",
        )
        .map_err(be)?;
    let mut owner =
        tx.prepare_cached("SELECT project FROM aspect_notes WHERE note_id = ?1").map_err(be)?;
    let mut root = tx
        .prepare_cached(
            "SELECT s.entity_kind, s.entity_name, t.start_time
               FROM traces t
               LEFT JOIN spans s ON s.project = t.project AND s.trace_id = t.trace_id
                                 AND s.span_id = t.root_span_id
              WHERE t.trace_id = ?1 AND t.project = ?2",
        )
        .map_err(be)?;

    // Resolved once per distinct trace. A batch of notes about one run would
    // otherwise ask the same question once per aspect.
    let mut resolved: std::collections::HashMap<String, RootIdentity> =
        std::collections::HashMap::new();

    for (index, note) in notes.iter().enumerate() {
        let project = &note.correlation.project;
        // `INSERT OR REPLACE` deletes the row it conflicts with, so without
        // this a note id another project already used would delete that
        // project's note and empty any theme label taken from it.
        let held = super::queries::owner_of(&mut owner, rusqlite::params![note.note_id])?;
        if crate::foreign(held.as_deref(), project) {
            written.refused.push(index);
            continue;
        }
        let key = format!("{project}\u{0}{}", note.trace_id);
        let entity = match resolved.get(&key) {
            Some(v) => v.clone(),
            None => {
                let found = root
                    .query_row(rusqlite::params![&note.trace_id, project], |r| {
                        Ok((
                            r.get::<_, Option<String>>(0)?,
                            r.get::<_, Option<String>>(1)?,
                            r.get::<_, Option<i64>>(2)?,
                        ))
                    })
                    .optional()
                    .map_err(be)?
                    .unwrap_or((None, None, None));
                resolved.insert(key, found.clone());
                found
            }
        };
        // Computed here rather than handed in, because it must be a sketch of
        // the text that was actually stored: the redactor runs before this, and
        // sketching what the sender wrote would cluster on words the daemon
        // deliberately did not keep.
        let sketch = mcpd_core::sketch::Sketch::of(&note.text).to_bytes();
        let c = &note.correlation;
        ins.execute(rusqlite::params![
            note.note_id,
            note.trace_id,
            note.aspect,
            note.text,
            note.definition_version,
            note.model,
            to_nanos(note.created_at),
            entity.0,
            entity.1,
            entity.2,
            sketch,
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
        ])
        .map_err(be)?;
        written.stored += 1;
    }
    Ok(written)
}

/// One row of `NOTE_COLUMNS`.
fn row_to_note(r: &rusqlite::Row<'_>) -> rusqlite::Result<StoredNote> {
    Ok(StoredNote {
        note: AspectNote {
            note_id: r.get(0)?,
            trace_id: r.get(1)?,
            aspect: r.get(2)?,
            text: r.get(3)?,
            definition_version: r.get(4)?,
            model: r.get(5)?,
            created_at: from_nanos(r.get(6)?),
            correlation: Correlation {
                project: r.get(10)?,
                environment: r.get(11)?,
                service_name: r.get(12)?,
                service_version: r.get(13)?,
                session_id: r.get(14)?,
                thread_id: r.get(15)?,
                user_id: r.get(16)?,
                resource_id: r.get(17)?,
                run_id: r.get(18)?,
                request_id: r.get(19)?,
                experiment_id: r.get(20)?,
                release: r.get(21)?,
            },
        },
        entity_kind: r.get::<_, Option<String>>(7)?.and_then(|s| s.parse::<EntityKind>().ok()),
        entity_name: r.get(8)?,
        trace_started_at: r.get::<_, Option<i64>>(9)?.map(from_nanos),
        sketch: r
            .get::<_, Option<Vec<u8>>>(22)?
            .as_deref()
            .and_then(mcpd_core::sketch::Sketch::from_bytes),
    })
}

/// Read aspect notes.
pub(super) fn list_notes_sync(conn: &Connection, q: &AspectNoteQuery) -> Result<Paged<StoredNote>> {
    let mut w = Where::default();
    w.push("project = ?", q.project.clone());
    if let Some(t) = &q.trace_id {
        w.push("trace_id = ?", t.clone());
    }
    if let Some(a) = &q.aspect {
        w.push("aspect = ?", a.clone());
    }
    if let Some((kind, name)) = &q.entity {
        w.push("entity_kind = ?", kind.as_str().to_owned());
        w.push("entity_name = ?", name.clone());
    }
    if let Some(from) = q.from {
        w.push("created_at >= ?", to_nanos(from));
    }
    if let Some(to) = q.to {
        w.push("created_at < ?", to_nanos(to));
    }
    if !q.note_ids.is_empty() {
        // A list has no parameter shape in SQLite, so the placeholders are
        // generated and every value is still bound. Nothing here is
        // interpolated into the statement.
        let holes = vec!["?"; q.note_ids.len()].join(",");
        w.clauses.push(format!("note_id IN ({holes})"));
        for id in &q.note_ids {
            w.binds.push(Box::new(id.clone()));
        }
    }

    let desc = matches!(q.order, Order::Desc);
    let (cmp, dir) = if desc { ('<', "DESC") } else { ('>', "ASC") };
    if let Some(c) = &q.cursor {
        let (v, id) = decode_cursor(c)?;
        w.clauses.push(format!("(created_at, note_id) {cmp} (?, ?)"));
        w.binds.push(Box::new(v));
        w.binds.push(Box::new(id));
    }

    let limit = q.limit.clamp(1, 1000);
    let sql = format!(
        "SELECT {NOTE_COLUMNS} FROM aspect_notes{} \
         ORDER BY created_at {dir}, note_id {dir} LIMIT {}",
        w.sql(),
        limit + 1
    );
    let mut stmt = conn.prepare(&sql).map_err(be)?;
    let mut rows: Vec<StoredNote> = stmt
        .query_map(params_from_iter(w.refs()), row_to_note)
        .map_err(be)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(be)?;

    let next_cursor = if rows.len() > limit as usize {
        rows.truncate(limit as usize);
        rows.last().map(|n| encode_cursor(to_nanos(n.note.created_at), &n.note.note_id))
    } else {
        None
    };
    Ok(Paged { items: rows, next_cursor, total: None })
}

/// Count notes per entity and aspect.
pub(super) fn rollup_sync(
    conn: &Connection,
    q: &AspectRollupQuery,
) -> Result<Vec<AspectNoteRollup>> {
    let mut w = Where::default();
    w.push("project = ?", q.project.clone());
    // A note whose trace was not stored when it arrived has no entity, and a
    // roll-up is by entity, so those rows sit outside every group here.
    w.clauses.push("entity_name IS NOT NULL".to_owned());
    w.clauses.push("entity_kind IS NOT NULL".to_owned());
    if let Some((kind, name)) = &q.entity {
        w.push("entity_kind = ?", kind.as_str().to_owned());
        w.push("entity_name = ?", name.clone());
    } else if let Some(kind) = q.entity_kind {
        w.push("entity_kind = ?", kind.as_str().to_owned());
    }

    let sql = format!(
        "SELECT entity_kind, entity_name, aspect, COUNT(*), COUNT(DISTINCT trace_id), \
                MAX(created_at) \
           FROM aspect_notes{} \
          GROUP BY entity_kind, entity_name, aspect \
          ORDER BY MAX(created_at) DESC, entity_name ASC, aspect ASC LIMIT {}",
        w.sql(),
        q.max_groups.clamp(1, 10_000),
    );
    let mut stmt = conn.prepare(&sql).map_err(be)?;
    let rows = stmt
        .query_map(params_from_iter(w.refs()), |r| {
            Ok(AspectNoteRollup {
                entity_kind: r
                    .get::<_, String>(0)?
                    .parse::<EntityKind>()
                    .unwrap_or(EntityKind::Service),
                entity_name: r.get(1)?,
                aspect: r.get(2)?,
                note_count: r.get::<_, i64>(3)?.max(0) as u64,
                trace_count: r.get::<_, i64>(4)?.max(0) as u64,
                last_note_at: from_nanos(r.get(5)?),
            })
        })
        .map_err(be)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(be)?;
    Ok(rows)
}

/// Write one whole clustering pass.
///
/// Everything lands or nothing does, for the same reason a signal batch is
/// written whole: a snapshot whose themes arrived and whose memberships did not
/// is a screen that shows a theme and then refuses to say what is in it.
pub(super) fn write_snapshot_sync(conn: &mut Connection, w: &SnapshotWrite) -> Result<()> {
    let tx = conn.transaction().map_err(be)?;
    // A pass is a pure function of the notes it read, so rewriting an id that
    // already exists writes what is already there. The old rows go first
    // because a rerun can produce fewer themes than the run before it, and
    // `INSERT OR REPLACE` alone would leave the extras behind.
    for sql in [
        "DELETE FROM themes WHERE snapshot_id = ?1",
        "DELETE FROM theme_members WHERE snapshot_id = ?1",
        "DELETE FROM theme_snapshots WHERE snapshot_id = ?1",
    ] {
        tx.execute(sql, rusqlite::params![&w.snapshot.snapshot_id]).map_err(be)?;
    }

    let s = &w.snapshot;
    tx.execute(
        "INSERT INTO theme_snapshots (
            snapshot_id, project, entity_kind, entity_name, aspects, ordinal, cutoff_at,
            window_from, window_to, trace_count, note_count, capped
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
        rusqlite::params![
            s.snapshot_id,
            s.project,
            s.entity_kind.as_str(),
            s.entity_name,
            serde_json::to_string(&s.aspects).unwrap_or_else(|_| "[]".into()),
            s.ordinal as i64,
            to_nanos(s.cutoff_at),
            to_nanos(s.window_from),
            to_nanos(s.window_to),
            s.trace_count as i64,
            s.note_count as i64,
            i64::from(s.capped),
        ],
    )
    .map_err(be)?;

    {
        let mut ins = tx
            .prepare_cached(
                "INSERT INTO themes (
                    snapshot_id, aspect, theme_id, project, entity_kind, entity_name, label,
                    label_note_id, label_trace_id, state, note_count, trace_count, coverage,
                    cutoff_at, window_from, window_to, split_from, split_into, merged_from,
                    merged_into
                 ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20)",
            )
            .map_err(be)?;
        for t in &w.themes {
            let th = &t.theme;
            ins.execute(rusqlite::params![
                th.snapshot_id,
                th.aspect,
                th.theme_id,
                t.project,
                th.entity_kind.as_str(),
                th.entity_name,
                th.label,
                th.label_note_id,
                th.label_trace_id,
                th.state.as_str(),
                th.note_count as i64,
                th.trace_count as i64,
                th.coverage,
                to_nanos(t.cutoff_at),
                to_nanos(t.window_from),
                to_nanos(t.window_to),
                list_out(&t.lineage.split_from),
                list_out(&t.lineage.split_into),
                list_out(&t.lineage.merged_from),
                list_out(&t.lineage.merged_into),
            ])
            .map_err(be)?;
        }

        let mut ins_m = tx
            .prepare_cached(
                "INSERT OR REPLACE INTO theme_members (
                    snapshot_id, aspect, theme_id, note_id, trace_id, project, similarity,
                    trace_started_at
                 ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            )
            .map_err(be)?;
        for m in &w.members {
            ins_m
                .execute(rusqlite::params![
                    m.snapshot_id,
                    m.aspect,
                    m.theme_id,
                    m.note_id,
                    m.trace_id,
                    m.project,
                    i64::from(m.similarity),
                    to_nanos(m.trace_started_at),
                ])
                .map_err(be)?;
        }
    }

    tx.commit().map_err(be)?;
    Ok(())
}

/// Read snapshot headers.
pub(super) fn list_snapshots_sync(
    conn: &Connection,
    q: &SnapshotQuery,
) -> Result<Paged<ThemeSnapshot>> {
    let mut w = Where::default();
    w.push("project = ?", q.project.clone());
    if let Some(id) = &q.snapshot_id {
        w.push("snapshot_id = ?", id.clone());
    }
    if let Some((kind, name)) = &q.entity {
        w.push("entity_kind = ?", kind.as_str().to_owned());
        w.push("entity_name = ?", name.clone());
    }
    if let Some(from) = q.from {
        w.push("cutoff_at >= ?", to_nanos(from));
    }
    if let Some(to) = q.to {
        w.push("cutoff_at < ?", to_nanos(to));
    }
    for aspect in &q.aspects {
        // The column holds a JSON array of names, so membership is a search for
        // the quoted name. Bound, never interpolated, and the quotes are what
        // stop `goal` matching an aspect called `goal_reached`.
        w.push("instr(aspects, ?) > 0", format!("\"{aspect}\""));
    }

    // Counted before the cursor is added, so it is how many the filter matched
    // and not how many are left. A timeline showing two dozen points out of
    // nine hundred has to be able to say what it stands in for.
    let count_sql = format!("SELECT COUNT(*) FROM theme_snapshots{}", w.sql());
    let total: i64 =
        conn.query_row(&count_sql, params_from_iter(w.refs()), |r| r.get(0)).map_err(be)?;

    let desc = matches!(q.order, Order::Desc);
    let (cmp, dir) = if desc { ('<', "DESC") } else { ('>', "ASC") };
    if let Some(c) = &q.cursor {
        let (v, id) = decode_cursor(c)?;
        w.clauses.push(format!("(cutoff_at, snapshot_id) {cmp} (?, ?)"));
        w.binds.push(Box::new(v));
        w.binds.push(Box::new(id));
    }

    let limit = q.limit.clamp(1, 1000);
    let sql = format!(
        "SELECT {SNAPSHOT_COLUMNS} FROM theme_snapshots{} \
         ORDER BY cutoff_at {dir}, snapshot_id {dir} LIMIT {}",
        w.sql(),
        limit + 1
    );
    let mut stmt = conn.prepare(&sql).map_err(be)?;
    let mut rows: Vec<ThemeSnapshot> = stmt
        .query_map(params_from_iter(w.refs()), row_to_snapshot)
        .map_err(be)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(be)?;

    let next_cursor = if rows.len() > limit as usize {
        rows.truncate(limit as usize);
        rows.last().map(|s| encode_cursor(to_nanos(s.cutoff_at), &s.snapshot_id))
    } else {
        None
    };
    Ok(Paged { items: rows, next_cursor, total: Some(total.max(0) as u64) })
}

fn row_to_snapshot(r: &rusqlite::Row<'_>) -> rusqlite::Result<ThemeSnapshot> {
    Ok(ThemeSnapshot {
        snapshot_id: r.get(0)?,
        project: r.get(1)?,
        entity_kind: r.get::<_, String>(2)?.parse().unwrap_or(EntityKind::Service),
        entity_name: r.get(3)?,
        aspects: serde_json::from_str(&r.get::<_, String>(4)?).unwrap_or_default(),
        ordinal: r.get::<_, i64>(5)?.max(0) as u64,
        cutoff_at: from_nanos(r.get(6)?),
        window_from: from_nanos(r.get(7)?),
        window_to: from_nanos(r.get(8)?),
        trace_count: r.get::<_, i64>(9)?.max(0) as u64,
        note_count: r.get::<_, i64>(10)?.max(0) as u64,
        capped: r.get::<_, i64>(11)? != 0,
    })
}

/// Read themes.
pub(super) fn list_themes_sync(conn: &Connection, q: &ThemeQuery) -> Result<Paged<StoredTheme>> {
    let mut w = Where::default();
    w.push("project = ?", q.project.clone());
    if let Some(id) = &q.snapshot_id {
        w.push("snapshot_id = ?", id.clone());
    }
    if let Some((kind, name)) = &q.entity {
        w.push("entity_kind = ?", kind.as_str().to_owned());
        w.push("entity_name = ?", name.clone());
    }
    if let Some(a) = &q.aspect {
        w.push("aspect = ?", a.clone());
    }
    if let Some(t) = &q.theme_id {
        w.push("theme_id = ?", t.clone());
    }

    let desc = matches!(q.order, Order::Desc);
    let (cmp, dir) = if desc { ('<', "DESC") } else { ('>', "ASC") };
    if let Some(c) = &q.cursor {
        let (v, id) = decode_cursor(c)?;
        w.clauses.push(format!("(cutoff_at, theme_id) {cmp} (?, ?)"));
        w.binds.push(Box::new(v));
        w.binds.push(Box::new(id));
    }

    let limit = q.limit.clamp(1, 1000);
    let sql = format!(
        "SELECT {THEME_COLUMNS} FROM themes{} \
         ORDER BY cutoff_at {dir}, theme_id {dir} LIMIT {}",
        w.sql(),
        limit + 1
    );
    let mut stmt = conn.prepare(&sql).map_err(be)?;
    let mut rows: Vec<StoredTheme> = stmt
        .query_map(params_from_iter(w.refs()), row_to_theme)
        .map_err(be)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(be)?;

    let next_cursor = if rows.len() > limit as usize {
        rows.truncate(limit as usize);
        rows.last().map(|t| encode_cursor(to_nanos(t.cutoff_at), &t.theme.theme_id))
    } else {
        None
    };
    Ok(Paged { items: rows, next_cursor, total: None })
}

fn row_to_theme(r: &rusqlite::Row<'_>) -> rusqlite::Result<StoredTheme> {
    Ok(StoredTheme {
        theme: Theme {
            theme_id: r.get(2)?,
            snapshot_id: r.get(0)?,
            aspect: r.get(1)?,
            entity_kind: r.get::<_, String>(4)?.parse().unwrap_or(EntityKind::Service),
            entity_name: r.get(5)?,
            label: r.get(6)?,
            label_note_id: r.get(7)?,
            label_trace_id: r.get(8)?,
            state: r.get::<_, String>(9)?.parse().unwrap_or(ThemeState::Continue),
            note_count: r.get::<_, i64>(10)?.max(0) as u64,
            trace_count: r.get::<_, i64>(11)?.max(0) as u64,
            coverage: r.get(12)?,
        },
        project: r.get(3)?,
        cutoff_at: from_nanos(r.get(13)?),
        window_from: from_nanos(r.get(14)?),
        window_to: from_nanos(r.get(15)?),
        lineage: ThemeLineage {
            split_from: list_in(r.get(16)?),
            split_into: list_in(r.get(17)?),
            merged_from: list_in(r.get(18)?),
            merged_into: list_in(r.get(19)?),
        },
    })
}

/// Read theme memberships.
pub(super) fn list_members_sync(
    conn: &Connection,
    q: &ThemeMemberQuery,
) -> Result<Paged<ThemeMember>> {
    let mut w = Where::default();
    w.push("project = ?", q.project.clone());
    w.push("snapshot_id = ?", q.snapshot_id.clone());
    if let Some(a) = &q.aspect {
        w.push("aspect = ?", a.clone());
    }
    if let Some(t) = &q.theme_id {
        w.push("theme_id = ?", t.clone());
    }
    if let Some(c) = &q.cursor {
        // The sort runs one way on similarity and the other on the note id, so
        // the resume condition is written out rather than borrowed from a walk
        // whose two keys point the same way. Getting this wrong repeats a row
        // on every page, which is the failure this shape exists to avoid.
        let (v, id) = decode_cursor(c)?;
        w.clauses.push("(similarity < ? OR (similarity = ? AND note_id > ?))".to_owned());
        w.binds.push(Box::new(v));
        w.binds.push(Box::new(v));
        w.binds.push(Box::new(id));
    }

    let limit = q.limit.clamp(1, 20_000);
    let sql = format!(
        "SELECT snapshot_id, aspect, theme_id, note_id, trace_id, project, similarity, \
                trace_started_at \
           FROM theme_members{} \
          ORDER BY similarity DESC, note_id ASC LIMIT {}",
        w.sql(),
        limit + 1
    );
    let mut stmt = conn.prepare(&sql).map_err(be)?;
    let mut rows: Vec<ThemeMember> = stmt
        .query_map(params_from_iter(w.refs()), |r| {
            Ok(ThemeMember {
                snapshot_id: r.get(0)?,
                aspect: r.get(1)?,
                theme_id: r.get(2)?,
                note_id: r.get(3)?,
                trace_id: r.get(4)?,
                project: r.get(5)?,
                similarity: r.get::<_, i64>(6)?.clamp(0, i64::from(u32::MAX)) as u32,
                trace_started_at: from_nanos(r.get(7)?),
            })
        })
        .map_err(be)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(be)?;

    let next_cursor = if rows.len() > limit as usize {
        rows.truncate(limit as usize);
        rows.last().map(|m| encode_cursor(i64::from(m.similarity), &m.note_id))
    } else {
        None
    };
    Ok(Paged { items: rows, next_cursor, total: None })
}

/// A list of theme ids as stored: JSON, or NULL when there is nothing in it.
///
/// NULL rather than `[]` so the common case, a theme that neither split nor
/// merged, costs one byte on every row rather than two.
fn list_out(ids: &[String]) -> Option<String> {
    (!ids.is_empty()).then(|| serde_json::to_string(ids).unwrap_or_else(|_| "[]".into()))
}

fn list_in(raw: Option<String>) -> Vec<String> {
    raw.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}
