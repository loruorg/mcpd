//! Versioned documents on SQLite: prompts, scorer definitions, evaluation rules.

use super::be;
use super::convert::{from_nanos, now_nanos, to_nanos};
use crate::{DocKind, DocumentLabel, NewDocument, Result, StoredDocument};
use rusqlite::{Connection, OptionalExtension};

const COLUMNS: &str = "project, kind, id, version, body, note, created_by, created_at";

fn row_to_doc(kind: DocKind, r: &rusqlite::Row<'_>) -> rusqlite::Result<StoredDocument> {
    let body: String = r.get(4)?;
    Ok(StoredDocument {
        project: r.get(0)?,
        kind,
        id: r.get(2)?,
        version: r.get::<_, i64>(3)?.clamp(0, i64::from(u32::MAX)) as u32,
        body: serde_json::from_str(&body).unwrap_or(serde_json::Value::Null),
        note: r.get(5)?,
        created_by: r.get(6)?,
        created_at: from_nanos(r.get(7)?),
    })
}

fn latest(
    conn: &Connection,
    project: &str,
    kind: DocKind,
    id: &str,
) -> Result<Option<StoredDocument>> {
    conn.query_row(
        &format!(
            "SELECT {COLUMNS} FROM documents WHERE project = ?1 AND kind = ?2 AND id = ?3
             ORDER BY version DESC LIMIT 1"
        ),
        rusqlite::params![project, kind.as_str(), id],
        |r| row_to_doc(kind, r),
    )
    .optional()
    .map_err(be)
}

/// One transaction on the single writer, so two posts cannot take one number.
pub(super) fn put_document_sync(
    conn: &mut Connection,
    doc: &NewDocument,
) -> Result<(StoredDocument, bool)> {
    let tx = conn.transaction().map_err(be)?;
    let held = latest(&tx, &doc.project, doc.kind, &doc.id)?;
    if let Some(held) = held.as_ref().filter(|h| h.body == doc.body) {
        return Ok((held.clone(), false));
    }
    let version = held.map_or(1, |h| h.version.saturating_add(1));
    tx.execute(
        &format!("INSERT INTO documents ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"),
        rusqlite::params![
            doc.project,
            doc.kind.as_str(),
            doc.id,
            i64::from(version),
            doc.body.to_string(),
            doc.note,
            doc.created_by,
            to_nanos(doc.created_at),
        ],
    )
    .map_err(be)?;
    tx.commit().map_err(be)?;
    Ok((
        StoredDocument {
            project: doc.project.clone(),
            kind: doc.kind,
            id: doc.id.clone(),
            version,
            body: doc.body.clone(),
            note: doc.note.clone(),
            created_by: doc.created_by.clone(),
            created_at: doc.created_at,
        },
        true,
    ))
}

pub(super) fn list_documents_sync(
    conn: &Connection,
    project: &str,
    kind: DocKind,
    id: Option<&str>,
) -> Result<Vec<StoredDocument>> {
    let rows = match id {
        Some(id) => {
            let mut stmt = conn
                .prepare_cached(&format!(
                    "SELECT {COLUMNS} FROM documents WHERE project = ?1 AND kind = ?2 AND id = ?3
                     ORDER BY version DESC"
                ))
                .map_err(be)?;
            let rows = stmt
                .query_map(rusqlite::params![project, kind.as_str(), id], |r| row_to_doc(kind, r))
                .map_err(be)?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(be)?;
            return Ok(rows);
        }
        None => {
            let mut stmt = conn
                .prepare_cached(&format!(
                    "SELECT {COLUMNS} FROM documents d WHERE project = ?1 AND kind = ?2
                       AND version = (SELECT MAX(version) FROM documents
                                      WHERE project = d.project AND kind = d.kind AND id = d.id)"
                ))
                .map_err(be)?;
            let rows = stmt
                .query_map(rusqlite::params![project, kind.as_str()], |r| row_to_doc(kind, r))
                .map_err(be)?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(be)?;
            rows
        }
    };
    Ok(crate::latest_per_id(rows))
}

pub(super) fn get_document_sync(
    conn: &Connection,
    project: &str,
    kind: DocKind,
    id: &str,
    version: Option<u32>,
) -> Result<Option<StoredDocument>> {
    let Some(v) = version else { return latest(conn, project, kind, id) };
    conn.query_row(
        &format!(
            "SELECT {COLUMNS} FROM documents
             WHERE project = ?1 AND kind = ?2 AND id = ?3 AND version = ?4"
        ),
        rusqlite::params![project, kind.as_str(), id, i64::from(v)],
        |r| row_to_doc(kind, r),
    )
    .optional()
    .map_err(be)
}

pub(super) fn set_document_label_sync(
    conn: &mut Connection,
    project: &str,
    kind: DocKind,
    id: &str,
    label: &str,
    version: Option<u32>,
) -> Result<bool> {
    let tx = conn.transaction().map_err(be)?;
    let changed = match version {
        None => tx
            .execute(
                "DELETE FROM document_labels
                 WHERE project = ?1 AND kind = ?2 AND id = ?3 AND label = ?4",
                rusqlite::params![project, kind.as_str(), id, label],
            )
            .map_err(be)?,
        Some(v) => {
            if get_document_sync(&tx, project, kind, id, Some(v))?.is_none() {
                return Ok(false);
            }
            tx.execute(
                "INSERT OR REPLACE INTO document_labels
                 (project, kind, id, label, version, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![project, kind.as_str(), id, label, i64::from(v), now_nanos()],
            )
            .map_err(be)?
        }
    };
    tx.commit().map_err(be)?;
    Ok(changed > 0)
}

pub(super) fn list_document_labels_sync(
    conn: &Connection,
    project: &str,
    kind: DocKind,
    id: Option<&str>,
) -> Result<Vec<DocumentLabel>> {
    let mut stmt = conn
        .prepare_cached(
            "SELECT id, label, version FROM document_labels
             WHERE project = ?1 AND kind = ?2 AND (?3 IS NULL OR id = ?3)
             ORDER BY id, label",
        )
        .map_err(be)?;
    let rows = stmt
        .query_map(rusqlite::params![project, kind.as_str(), id], |r| {
            Ok(DocumentLabel {
                id: r.get(0)?,
                label: r.get(1)?,
                version: r.get::<_, i64>(2)?.clamp(0, i64::from(u32::MAX)) as u32,
            })
        })
        .map_err(be)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(be)?;
    Ok(rows)
}
