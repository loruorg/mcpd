//! Background-job leases on SQLite.
//!
//! Several daemons can share one database file, so the claim is one upsert
//! whose `WHERE` is the compare-and-set: the row changes only when the claimant
//! already holds it or its lease has expired. SQLite runs one writer at a time,
//! so two claims never interleave.

use super::be;
use super::convert::{from_nanos, to_nanos};
use crate::{Lease, LeaseClaim, Result};
use rusqlite::Connection;

pub(super) fn claim_lease_sync(conn: &Connection, c: &LeaseClaim<'_>) -> Result<Lease> {
    let now = to_nanos(c.now);
    let expires = to_nanos(c.now + c.ttl);
    // Every column on the right of `SET` reads the row as it was, so `since`
    // is decided against the old holder and the old expiry.
    conn.execute(
        "INSERT INTO leases (job, holder, since, expires_at, progress)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (job) DO UPDATE SET
             since = CASE WHEN leases.holder = excluded.holder AND leases.expires_at > ?3
                          THEN leases.since ELSE excluded.since END,
             holder = excluded.holder,
             expires_at = excluded.expires_at,
             progress = COALESCE(excluded.progress, leases.progress)
         WHERE leases.holder = excluded.holder OR leases.expires_at <= ?3",
        rusqlite::params![c.job, c.holder, now, expires, c.progress],
    )
    .map_err(be)?;
    conn.query_row(
        "SELECT job, holder, since, expires_at, progress FROM leases WHERE job = ?1",
        [c.job],
        |r| {
            Ok(Lease {
                job: r.get(0)?,
                holder: r.get(1)?,
                since: from_nanos(r.get(2)?),
                expires_at: from_nanos(r.get(3)?),
                progress: r.get(4)?,
            })
        },
    )
    .map_err(be)
}

pub(super) fn release_lease_sync(conn: &Connection, job: &str, holder: &str, now: i64) -> Result<()> {
    conn.execute(
        "UPDATE leases SET expires_at = ?3 WHERE job = ?1 AND holder = ?2 AND expires_at > ?3",
        rusqlite::params![job, holder, now],
    )
    .map_err(be)?;
    Ok(())
}
