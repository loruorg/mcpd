//! A bounded, durable queue of requests waiting to be sent.
//!
//! Forwarding and the relay both hold work that another system has not
//! accepted yet. Held in memory, a restart loses it and a long outage grows the
//! process until it is killed. The spool is a directory with one file per
//! entry, written to a temporary name, synced and then renamed, so an entry is
//! either wholly on disk or absent: a crash mid-write leaves a temporary file
//! that the next open removes, never half an entry that is sent as garbage.
//!
//! It is bounded in bytes. What happens at the bound is the caller's choice,
//! because the two callers want opposite things: forwarding drops the new copy
//! and counts it, since the local write already succeeded, while the relay
//! refuses the request with a 503 so the sender keeps it and retries.
//!
//! With no directory the spool is held in memory under the same bound, which
//! is what a daemon storing to `:memory:` gets: nothing it holds survives a
//! restart, and a spool that did would be the one thing that outlived the data.

use std::collections::{BTreeMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The first bytes of every entry file, so a stray file in the directory is
/// recognised and left alone rather than sent.
const MAGIC: &[u8] = b"MCPDSPOOL1\n";

/// How many refused entries are kept in `dead/` for an operator to read.
/// Older ones are deleted, so a destination that refuses everything cannot
/// fill the disk through the dead-letter directory either.
const DEAD_KEPT: usize = 100;

/// Why an entry could not be added.
#[derive(Debug)]
pub enum PushError {
    /// The spool is at its bound.
    Full,
    /// The entry could not be written.
    Io(std::io::Error),
}

impl std::fmt::Display for PushError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Full => f.write_str("the spool is full"),
            Self::Io(e) => write!(f, "the spool could not be written: {e}"),
        }
    }
}

struct Entry {
    size: u64,
    /// Seconds since the epoch when it was written.
    created: i64,
    /// The bytes, for a spool held in memory.
    mem: Option<Vec<u8>>,
}

#[derive(Default)]
struct Inner {
    pending: BTreeMap<u64, Entry>,
    in_flight: HashSet<u64>,
    /// Bytes held, plus bytes reserved by writes still in progress.
    bytes: u64,
    next: u64,
}

/// An entry handed to a sender. Acknowledge, release or bury it by `id`.
pub struct Claimed {
    /// The entry's sequence number.
    pub id: u64,
    /// What was pushed.
    pub bytes: Vec<u8>,
    /// Seconds since the epoch when it was written.
    pub created: i64,
}

/// What a spool directory holds, read without opening it for writing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Contents {
    /// Entries waiting.
    pub entries: u64,
    /// Their size on disk.
    pub bytes: u64,
    /// Refused entries kept in `dead/`.
    pub dead: u64,
    /// Seconds since the oldest waiting entry was written.
    pub oldest_age_seconds: Option<u64>,
}

/// A bounded queue of byte payloads, on disk or in memory.
pub struct Spool {
    dir: Option<PathBuf>,
    max_bytes: u64,
    inner: Mutex<Inner>,
    ready: tokio::sync::Notify,
}

fn now() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

fn entry_name(id: u64) -> String {
    format!("{id:016x}.entry")
}

/// Create a file only this user can read: an entry can hold a prompt, and with
/// no upstream key a relay entry holds the sender's key.
fn create_private(path: &Path) -> std::io::Result<std::fs::File> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o.open(path)
}

impl Spool {
    /// Open a spool in `dir`, or in memory when `dir` is `None`, picking up
    /// every entry a previous process left.
    ///
    /// # Errors
    /// The directory cannot be created or read.
    pub fn open(dir: Option<&Path>, max_bytes: u64) -> std::io::Result<Self> {
        let mut inner = Inner::default();
        if let Some(dir) = dir {
            std::fs::create_dir_all(dir)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
            }
            for e in std::fs::read_dir(dir)? {
                let e = e?;
                let name = e.file_name().to_string_lossy().into_owned();
                if name.ends_with(".tmp") {
                    // A write that a crash interrupted. It was never
                    // acknowledged to anyone, so removing it loses nothing
                    // that was promised.
                    let _ = std::fs::remove_file(e.path());
                    continue;
                }
                let Some(id) =
                    name.strip_suffix(".entry").and_then(|h| u64::from_str_radix(h, 16).ok())
                else {
                    continue;
                };
                let meta = e.metadata()?;
                let created = meta
                    .modified()
                    .ok()
                    .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or_else(now, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
                inner.bytes += meta.len();
                inner.next = inner.next.max(id + 1);
                inner.pending.insert(id, Entry { size: meta.len(), created, mem: None });
            }
        }
        Ok(Self {
            dir: dir.map(Path::to_path_buf),
            max_bytes,
            inner: Mutex::new(inner),
            ready: tokio::sync::Notify::new(),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        // A panic while the lock was held leaves counters that are at worst a
        // little off, never an entry that is lost, so the poison is cleared
        // rather than spread to every later push.
        self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Where the spool lives, when it is on disk.
    #[must_use]
    pub fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    /// The bound, in bytes.
    #[must_use]
    pub const fn max_bytes(&self) -> u64 {
        self.max_bytes
    }

    /// Add an entry. Durable on disk before this returns.
    ///
    /// # Errors
    /// [`PushError::Full`] at the bound, [`PushError::Io`] when the write
    /// fails. Either way nothing was added.
    pub fn push(&self, payload: &[u8]) -> Result<u64, PushError> {
        let size = (MAGIC.len() + payload.len()) as u64;
        let id = {
            let mut g = self.lock();
            if g.bytes + size > self.max_bytes {
                return Err(PushError::Full);
            }
            g.bytes += size;
            let id = g.next;
            g.next += 1;
            id
        };
        let written = match &self.dir {
            None => Ok(Some(payload.to_vec())),
            Some(dir) => self.write(dir, id, payload).map(|()| None),
        };
        let mut g = self.lock();
        match written {
            Ok(mem) => {
                g.pending.insert(id, Entry { size, created: now(), mem });
                drop(g);
                self.ready.notify_one();
                Ok(id)
            }
            Err(e) => {
                g.bytes = g.bytes.saturating_sub(size);
                Err(PushError::Io(e))
            }
        }
    }

    fn write(&self, dir: &Path, id: u64, payload: &[u8]) -> std::io::Result<()> {
        let tmp = dir.join(format!("{id:016x}.tmp"));
        let result = (|| {
            let mut f = create_private(&tmp)?;
            f.write_all(MAGIC)?;
            f.write_all(payload)?;
            f.sync_all()?;
            std::fs::rename(&tmp, dir.join(entry_name(id)))
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        result
    }

    /// The oldest entry no sender holds, marked as held.
    #[must_use]
    pub fn claim(&self) -> Option<Claimed> {
        loop {
            let (id, created, mem) = {
                let mut g = self.lock();
                let (id, e) = {
                    let (id, e) = g.pending.iter().find(|(id, _)| !g.in_flight.contains(id))?;
                    (*id, (e.created, e.mem.clone()))
                };
                g.in_flight.insert(id);
                (id, e.0, e.1)
            };
            let bytes = match (mem, &self.dir) {
                (Some(b), _) => Some(b),
                (None, Some(dir)) => std::fs::read(dir.join(entry_name(id)))
                    .ok()
                    .and_then(|b| b.strip_prefix(MAGIC).map(<[u8]>::to_vec)),
                (None, None) => None,
            };
            match bytes {
                Some(bytes) => return Some(Claimed { id, bytes, created }),
                // A file removed or corrupted under the spool. Nothing can
                // be sent from it, so it is forgotten and the next one tried.
                None => self.forget(id),
            }
        }
    }

    fn forget(&self, id: u64) {
        let mut g = self.lock();
        g.in_flight.remove(&id);
        if let Some(e) = g.pending.remove(&id) {
            g.bytes = g.bytes.saturating_sub(e.size);
        }
    }

    /// The entry was delivered: remove it.
    pub fn ack(&self, id: u64) {
        if let Some(dir) = &self.dir {
            let _ = std::fs::remove_file(dir.join(entry_name(id)));
        }
        self.forget(id);
    }

    /// The entry was not delivered and may be tried again.
    pub fn release(&self, id: u64) {
        self.lock().in_flight.remove(&id);
        self.ready.notify_one();
    }

    /// The entry was refused for good: move it to `dead/`, where the most
    /// recent are kept for an operator to read, and out of the queue.
    pub fn bury(&self, id: u64) {
        if let Some(dir) = &self.dir {
            let dead = dir.join("dead");
            let moved = std::fs::create_dir_all(&dead).and_then(|()| {
                std::fs::rename(dir.join(entry_name(id)), dead.join(entry_name(id)))
            });
            if moved.is_err() {
                let _ = std::fs::remove_file(dir.join(entry_name(id)));
            }
            prune(&dead);
        }
        self.forget(id);
    }

    /// Entries waiting, held ones included.
    #[must_use]
    pub fn len(&self) -> usize {
        self.lock().pending.len()
    }

    /// Whether nothing is waiting.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Bytes held.
    #[must_use]
    pub fn bytes(&self) -> u64 {
        self.lock().bytes
    }

    /// Seconds since the oldest waiting entry was written.
    #[must_use]
    pub fn oldest_age_seconds(&self) -> Option<u64> {
        let g = self.lock();
        let oldest = g.pending.values().map(|e| e.created).min()?;
        Some(u64::try_from(now() - oldest).unwrap_or(0))
    }

    /// Wait until an entry may be ready to claim. Wakes on a push and on a
    /// release, and may wake when nothing is ready, so the caller claims and
    /// waits again.
    pub async fn ready(&self) {
        self.ready.notified().await;
    }

    /// Read what a spool directory holds without opening it, for `mcpd
    /// doctor`, which runs beside a daemon that owns the directory.
    ///
    /// # Errors
    /// The directory exists and cannot be read.
    pub fn inspect(dir: &Path) -> std::io::Result<Contents> {
        let mut c = Contents::default();
        if !dir.exists() {
            return Ok(c);
        }
        let mut oldest: Option<std::time::SystemTime> = None;
        for e in std::fs::read_dir(dir)? {
            let e = e?;
            if !e.file_name().to_string_lossy().ends_with(".entry") {
                continue;
            }
            let m = e.metadata()?;
            c.entries += 1;
            c.bytes += m.len();
            if let Ok(t) = m.modified() {
                oldest = Some(oldest.map_or(t, |o| o.min(t)));
            }
        }
        if let Ok(rd) = std::fs::read_dir(dir.join("dead")) {
            c.dead = rd.filter_map(Result::ok).count() as u64;
        }
        c.oldest_age_seconds = oldest.and_then(|t| t.elapsed().ok()).map(|d| d.as_secs());
        Ok(c)
    }
}

/// Keep the newest [`DEAD_KEPT`] files in a dead-letter directory.
fn prune(dead: &Path) {
    let Ok(rd) = std::fs::read_dir(dead) else { return };
    let mut names: Vec<String> =
        rd.filter_map(Result::ok).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    if names.len() <= DEAD_KEPT {
        return;
    }
    // The names are zero-padded sequence numbers, so they sort by age.
    names.sort();
    for n in &names[..names.len() - DEAD_KEPT] {
        let _ = std::fs::remove_file(dead.join(n));
    }
}

/// An entry with a small JSON header in front of its body: what to send and
/// where, then the bytes.
#[must_use]
pub fn frame(meta: &serde_json::Value, body: &[u8]) -> Vec<u8> {
    let head = meta.to_string();
    let mut out = Vec::with_capacity(head.len() + 1 + body.len());
    out.extend_from_slice(head.as_bytes());
    out.push(b'\n');
    out.extend_from_slice(body);
    out
}

/// The header and body of a framed entry.
#[must_use]
pub fn unframe(bytes: &[u8]) -> Option<(serde_json::Value, &[u8])> {
    let at = bytes.iter().position(|b| *b == b'\n')?;
    let meta = serde_json::from_slice(&bytes[..at]).ok()?;
    Some((meta, &bytes[at + 1..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        static STAMP: std::sync::OnceLock<u128> = std::sync::OnceLock::new();
        let stamp = STAMP.get_or_init(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        });
        std::env::temp_dir().join(format!(
            "mcpd-spool-{tag}-{}-{stamp}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn entries_survive_a_reopen_in_order() {
        let d = dir("reopen");
        {
            let s = Spool::open(Some(&d), 1 << 20).expect("open");
            s.push(b"one").expect("push");
            s.push(b"two").expect("push");
        }
        let s = Spool::open(Some(&d), 1 << 20).expect("reopen");
        assert_eq!(s.len(), 2);
        let first = s.claim().expect("an entry");
        assert_eq!(first.bytes, b"one");
        let second = s.claim().expect("another");
        assert_eq!(second.bytes, b"two");
        assert!(s.claim().is_none(), "both are held");
        s.ack(first.id);
        s.release(second.id);
        let again = s.claim().expect("released one comes back");
        assert_eq!(again.bytes, b"two");
        let id3 = s.push(b"three").expect("push after reopen");
        assert!(id3 > again.id, "sequence numbers continue past the reopened ones");
    }

    #[test]
    fn the_bound_refuses_rather_than_grows() {
        let s = Spool::open(None, 64).expect("open");
        s.push(&[0u8; 40]).expect("fits");
        assert!(matches!(s.push(&[0u8; 40]), Err(PushError::Full)));
        let c = s.claim().expect("an entry");
        s.ack(c.id);
        assert_eq!(s.bytes(), 0);
        s.push(&[0u8; 40]).expect("room again after the ack");
    }

    #[test]
    fn a_half_written_entry_is_removed_on_open_and_a_stray_file_left_alone() {
        let d = dir("tmp");
        std::fs::create_dir_all(&d).expect("mkdir");
        std::fs::write(d.join("0000000000000009.tmp"), b"partial").expect("write");
        std::fs::write(d.join("notes.txt"), b"mine").expect("write");
        let s = Spool::open(Some(&d), 1 << 20).expect("open");
        assert_eq!(s.len(), 0);
        assert!(!d.join("0000000000000009.tmp").exists());
        assert!(d.join("notes.txt").exists());
    }

    #[test]
    fn a_buried_entry_is_kept_for_reading_and_counted_by_inspect() {
        let d = dir("dead");
        let s = Spool::open(Some(&d), 1 << 20).expect("open");
        s.push(b"bad").expect("push");
        s.push(b"waiting").expect("push");
        let c = s.claim().expect("an entry");
        s.bury(c.id);
        let seen = Spool::inspect(&d).expect("inspect");
        assert_eq!(seen.entries, 1);
        assert_eq!(seen.dead, 1);
        assert!(seen.oldest_age_seconds.is_some());
    }

    #[cfg(unix)]
    #[test]
    fn entries_are_readable_by_this_user_only() {
        use std::os::unix::fs::PermissionsExt;
        let d = dir("perm");
        let s = Spool::open(Some(&d), 1 << 20).expect("open");
        let id = s.push(b"secret").expect("push");
        let mode = std::fs::metadata(d.join(entry_name(id))).expect("stat").permissions().mode();
        assert_eq!(mode & 0o077, 0, "mode {mode:o}");
    }

    #[test]
    fn a_frame_reads_back() {
        let f = frame(&serde_json::json!({ "a": 1 }), b"body\nwith newline");
        let (meta, body) = unframe(&f).expect("unframes");
        assert_eq!(meta["a"], 1);
        assert_eq!(body, b"body\nwith newline");
    }
}
