//! Server configuration and shared state.

use mcpd_core::cost::PriceBook;
use mcpd_core::redact::Redactor;
use mcpd_core::sampling::Policy;
use mcpd_store::Store;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use time::OffsetDateTime;

/// Everything the server needs to run.
#[derive(Clone)]
pub struct ServerConfig {
    /// Project a signal lands in when the producer did not name one.
    pub default_project: String,
    /// Largest request body accepted, in bytes.
    pub max_body_bytes: usize,
    /// Most signals accepted in one batch.
    pub max_batch_signals: usize,
    /// How long to keep data, in days. `None` keeps it forever.
    pub retention_days: Option<u32>,
    /// Ingest keys, when ingest is protected. Empty means open ingest, which is
    /// correct on a laptop and wrong on the internet.
    pub ingest_keys: Vec<String>,
    /// Read keys, when the query API is protected.
    pub read_keys: Vec<String>,
    /// Keys permitted to change stored data: scores, feedback, datasets,
    /// experiments and acknowledgements.
    ///
    /// Empty falls back to [`ServerConfig::ingest_keys`], so a deployment
    /// configured before the write scope existed keeps working on upgrade with
    /// the reach it already had.
    pub write_keys: Vec<String>,
    /// Keys permitted to destroy data or read the audit log.
    ///
    /// Empty falls back to [`ServerConfig::read_keys`], for the same upgrade
    /// reason: deletion was authorised by the read scope in earlier builds, and
    /// an upgrade that suddenly refused every configured key would be an outage
    /// dressed as a security fix.
    pub admin_keys: Vec<String>,
    /// Keys permitted to start, cancel and signal runs. Empty falls back to
    /// the write keys, and an admin key is accepted here too.
    pub execute_keys: Vec<String>,
    /// Keys permitted to claim steps and report their outcome. Empty falls
    /// back to the write keys, and an admin key is accepted here too.
    pub worker_keys: Vec<String>,
    /// The knobs the run store's ticks read: lease length, reclaim window and
    /// the rest. Defaults are right for one daemon on one machine.
    pub run_config: mcpd_runs::RunConfig,
    /// Sampling policy applied at ingest, for any project without its own.
    pub sampling: Policy,
    /// Most signals one project may store per minute, for any project in
    /// [`ServerConfig::projects`] without a ceiling of its own, for
    /// [`ServerConfig::default_project`], and **once over for every project the
    /// configuration does not name at all**. `None` is unlimited, so a
    /// deployment that never sets it behaves exactly as it did before quotas
    /// existed.
    ///
    /// The last of those three is the part that makes this a ceiling, and this
    /// number is the only thing that creates it. The project name arrives
    /// inside the request body, so a sender that chose a name no configuration
    /// file lists is charged to nothing and can store without bound. A ceiling
    /// the sender can opt out of by inventing a string is not a ceiling.
    ///
    /// **So a ceiling written only under `projects` bounds the projects it
    /// names and nothing else.** That is the natural way to write per-project
    /// limits, and it leaves the bypass open, so the daemon does not keep that
    /// to itself: `mcpd doctor` grades it under `limits`, and
    /// `mcpd_signals_unbounded_total` counts what the ingest doors let through
    /// with no ceiling over them.
    ///
    /// That counter is a floor and not a total, and the difference matters to
    /// anyone alerting on it. It sees the batch doors, which apply the ceiling
    /// in `ingest::enforce_quotas`, and the single-record score and feedback
    /// doors, which apply it in `ingest::charge_one`. `POST /v1/api/score`
    /// hands its scores to `store.ingest` in `eval.rs` without passing either,
    /// so those rows are charged to no ceiling, counted by nothing here, and
    /// land on disk even for a project whose ceiling is already spent.
    /// See [`ProjectQuotas::unnamed_are_unbounded`].
    ///
    /// The gap is reported rather than closed on your behalf, because every way
    /// of closing it costs data. Deriving a shared number from the named
    /// ceilings invents a limit nobody wrote; refusing a project the
    /// configuration does not name refuses traffic that is being stored today.
    /// Either would make an upgrade that changed no configuration start
    /// dropping a working deployment's signals, which is worse than the bypass.
    /// Set this number to close it.
    ///
    /// Every unnamed project shares one counter, rather than being given one
    /// each. A counter per name would be an allocation any sender could drive
    /// by inventing names, which is the same hole in a different shape. Sharing
    /// means a flood spread over a thousand invented names is bounded in total
    /// by this number, and it means a project worth its own allowance should be
    /// named in `projects`, where it gets one.
    pub quota_per_minute: Option<u32>,
    /// Conditions watched in the background. Empty means no alerting.
    pub monitors: Vec<crate::monitor::Monitor>,
    /// Service level objectives, evaluated on the same timer as the monitors.
    pub slos: Vec<crate::monitor::Slo>,
    /// How often the monitors are evaluated, in seconds.
    pub monitor_interval_seconds: u64,
    /// Where to copy spans on to, when mcpd is one hop in a pipeline rather
    /// than the destination.
    pub forward: Option<crate::forward::Forwarder>,
    /// The model that grades runs, when judged scoring is turned on. `None`
    /// means judged scorers decline rather than run, which is the right default
    /// for a feature that spends money per run.
    pub judge: Option<crate::judge::JudgeConfig>,
    /// Per-project settings, keyed by project name.
    ///
    /// This is what makes a project a tenancy boundary rather than a label. A
    /// key listed under a project reaches that project and no other, so one
    /// daemon serves several teams without any of them being able to read the
    /// others' prompts. Retention and sampling are per project for the same
    /// reason: one team's ninety-day requirement should not force everybody
    /// else's data to be kept, and one team's traffic should not decide how the
    /// rest are sampled.
    ///
    /// Empty by default. An operator who never opens this block never meets any
    /// of it, and the top-level keys keep granting everything.
    pub projects: std::collections::BTreeMap<String, ProjectConfig>,

    /// The header an ingress may use to name the person behind a request, and
    /// the addresses permitted to set it. Off unless both halves are set.
    ///
    /// mcpd has no identity provider, so this is the only source of a `subject`
    /// on an audit row. See `crate::identity` for why the address list is not
    /// optional.
    pub trusted_identity: crate::identity::TrustedIdentity,

    /// Open sign-up. Off by default, and off means nothing in `crate::accounts`
    /// runs and the key rules are exactly what they were.
    ///
    /// On, anyone may create an account, which owns one project of its own.
    /// It also ends the rule that a scope with no keys is open: with strangers
    /// allowed in, a request with no credential is refused everywhere except
    /// the health probes, sign-up and log-in.
    pub signup: bool,

    /// Where a request came from, for the per-address limits on sign-up and
    /// log-in. The peer address unless a proxy header is configured and the
    /// peer is one of the trusted proxies.
    pub client_ip: crate::identity::ClientIp,

    /// Browser origins permitted to call the query API from another site.
    ///
    /// Empty means no cross-origin request is answered with the headers a
    /// browser needs, so only a page served by this daemon, which is what the
    /// bundled interface is, can read the API from a browser. The query API used
    /// to allow every origin, and the read API is open by default, so any web
    /// page open in the same browser as a daemon on localhost could read the
    /// stored prompts. A deployment that serves the interface from somewhere
    /// else names that origin here. `*` restores the old behaviour, and has to
    /// be typed.
    ///
    /// The health, readiness and metrics routes are not governed by this list:
    /// they carry no stored data and stay readable from any origin, because the
    /// interface probes `/healthz` before it does anything else.
    pub cors_origins: Vec<String>,

    /// Metrics the daemon derives from the spans it stores: a duration per
    /// span kind that has one, and the token counters a model call implies.
    ///
    /// Both halves are on. They are derivation from data that is already
    /// arriving, so an operator who never opens this setting gets the latency
    /// and token dashboards without configuring anything, which is the point of
    /// deriving them in the daemon rather than asking six SDKs to emit them.
    ///
    /// It is not free, and `crate::derive` states the multiplier: up to
    /// thirteen extra rows per model call with full usage detail, and one per
    /// finished span of five kinds. There is no flag for it: turn either half
    /// off in the config file's `derived_metrics` block, with `durations` and
    /// `tokens` set to `false`. The derived rows are added after
    /// the per-project ceiling, so turning this on can never cost a sender a
    /// signal it sent.
    pub derived_metrics: crate::derive::DerivedMetrics,

    /// What the aspect analysis is armed with.
    ///
    /// Off by default. Nothing here spawns a timer, takes a connection or
    /// writes a table until an operator switches it on, which is the same rule
    /// the monitors follow.
    pub aspects: crate::aspects::AspectSettings,

    /// Longest a request may take before the daemon answers for the handler.
    ///
    /// Bounds the whole request, reading the body included. A handler stuck on
    /// a storage backend that accepts connections and never answers would
    /// otherwise hold the connection for as long as the client waited, and a
    /// client that never gave up would hold it for ever.
    pub request_timeout_seconds: u64,
}

/// What one project may configure for itself.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfig {
    /// Keys that may write to this project, and only this project.
    #[serde(default)]
    pub ingest_keys: Vec<String>,
    /// Keys that may read this project, and only this project.
    #[serde(default)]
    pub read_keys: Vec<String>,
    /// Keys that may change this project's stored data. Falls back to this
    /// project's `ingest_keys`.
    #[serde(default)]
    pub write_keys: Vec<String>,
    /// Keys that may delete from this project, or read its audit log. Falls
    /// back to this project's `read_keys`.
    #[serde(default)]
    pub admin_keys: Vec<String>,
    /// Keys that may start, cancel and signal this project's runs. Falls back
    /// to this project's `write_keys`.
    #[serde(default)]
    pub execute_keys: Vec<String>,
    /// Keys that may claim and finish this project's steps. Falls back to this
    /// project's `write_keys`.
    #[serde(default)]
    pub worker_keys: Vec<String>,
    /// Days to keep this project's signals. Falls back to the global setting.
    #[serde(default)]
    pub retention_days: Option<u32>,
    /// Fraction of this project's traces to keep, 0.0 to 1.0. Falls back to the
    /// global policy.
    #[serde(default)]
    pub sample: Option<f64>,
    /// Most signals this project may store per minute. Falls back to the global
    /// setting. Unset in both means this project has no ceiling.
    ///
    /// A number here bounds this one project. It does not bound a project
    /// nobody wrote down, and the sender picks the project name in the body of
    /// its own request. Only [`ServerConfig::quota_per_minute`] bounds those.
    #[serde(default)]
    pub quota_per_minute: Option<u32>,
}

impl ProjectConfig {
    /// The keys this project accepts for one scope.
    ///
    /// Write falls back to ingest and admin falls back to read when the newer
    /// list is empty. Without that, adding two scopes would silently revoke
    /// access on every project block written before they existed.
    #[must_use]
    pub fn keys_for(&self, scope: crate::auth::Scope) -> &[String] {
        match scope {
            crate::auth::Scope::Ingest => &self.ingest_keys,
            crate::auth::Scope::Read => &self.read_keys,
            crate::auth::Scope::Write => fall_back(&self.write_keys, &self.ingest_keys),
            crate::auth::Scope::Admin => fall_back(&self.admin_keys, &self.read_keys),
            crate::auth::Scope::Execute => {
                fall_back(&self.execute_keys, self.keys_for(crate::auth::Scope::Write))
            }
            crate::auth::Scope::Worker => {
                fall_back(&self.worker_keys, self.keys_for(crate::auth::Scope::Write))
            }
        }
    }
}

/// The narrower list, or the wider one it inherits from when it is unset.
fn fall_back<'a>(narrow: &'a [String], wider: &'a [String]) -> &'a [String] {
    if narrow.is_empty() {
        wider
    } else {
        narrow
    }
}

/// The window a quota is measured over, in milliseconds.
///
/// One minute, and not configurable. The flag names the unit, so a second
/// setting that changed what "per minute" meant would make the first number
/// unreadable. A shorter window refuses the ordinary burst an SDK produces when
/// it flushes a full queue; a longer one lets a flood run for longer before
/// anything stops it.
const QUOTA_WINDOW_MS: u64 = 60_000;

/// Bits each of the two window counts gets inside the packed word.
const QUOTA_COUNT_BITS: u32 = 20;

/// Mask for one packed count.
const QUOTA_COUNT_MASK: u64 = (1 << QUOTA_COUNT_BITS) - 1;

/// Mask for the packed window index. Twenty-four bits of whole minutes is more
/// than thirty years of uptime before it wraps.
const QUOTA_INDEX_MASK: u64 = (1 << 24) - 1;

/// The largest count one window can hold, and so the largest ceiling that means
/// anything. `main.rs` refuses a flag above it rather than saturating quietly.
pub const QUOTA_MAX: u32 = QUOTA_COUNT_MASK as u32;

/// One project's ceiling and its counter.
struct Quota {
    /// Signals this project may store per window.
    limit: u32,
    /// The window index and both window counts, in one word.
    ///
    /// One word rather than three fields so all of it moves under a single
    /// compare-and-exchange. With a field each, two batches arriving together
    /// could both read the old index and roll the window twice, losing a whole
    /// window of counting. The alternative to that race is a lock on the ingest
    /// path, which is the thing this is avoiding.
    packed: AtomicU64,
}

/// Per-project ceilings on how many signals may be stored per minute.
///
/// **Signals per minute** is the unit, and the choice is about what ingest
/// already knows. By the time a batch reaches the ceiling it is decoded, so a
/// count is a `len()` and costs nothing. Bytes would mean serialising every
/// signal a second time to weigh it, on the hottest path in the daemon. A daily
/// cap has to survive a restart, so it is a stored counter with a schema, a
/// migration and a write per batch: a different feature at a different price.
///
/// One slot per project that has a ceiling of its own, built at construction and
/// never added to afterwards, plus one shared slot for every project the
/// configuration does not name, when there is a top-level ceiling for that slot
/// to hold. That is what keeps the counters bounded. The project name arrives inside the
/// request body, so a map that grew on first sight of a name would be an
/// allocation any sender could drive.
///
/// ## Where the ceiling stops
///
/// Without that top-level number there is no shared slot, and then a name no
/// configuration lists is charged to nothing: there is no counter to charge it
/// to, and the signals are stored unbounded. Ceilings set only under
/// `projects` therefore bound the projects they name and nothing else.
/// [`ProjectQuotas::unnamed_are_unbounded`] reports that state, ingest counts
/// what rides through it, and [`ServerConfig::quota_per_minute`] says why it is
/// reported rather than closed by guessing a number.
///
/// ## What the ceiling actually bounds
///
/// The counter is the two-window weighted kind: this window's count, plus the
/// window before it weighted by how much of it the minute ending now still
/// covers. It holds the **sustained** rate to the ceiling, and it permits a
/// burst of up to a whole ceiling on top of that, which is deliberate: an SDK
/// flushing a full queue arrives as one burst, and a counter that refused it
/// would refuse ordinary traffic.
///
/// Those two together have an arithmetic consequence, and it is worth stating
/// rather than hiding. A project may store a shade under **twice** its ceiling
/// inside one 60-second span: a full ceiling at the very end of one window, and
/// a full ceiling again at the very end of the next. That is not an artefact of
/// the approximation, it is what permitting a full-ceiling burst means. Any
/// limiter that lets a project spend `L` at once and `L` per minute afterwards
/// permits `2L` across a boundary; the only way to remove it is to refuse the
/// burst. `a_ceiling_bounds_the_sustained_rate_and_permits_one_burst_on_top`
/// measures the figure, so it cannot get quietly worse.
///
/// Sized for a minute, and not configurable. See [`QUOTA_WINDOW_MS`].
pub struct ProjectQuotas {
    /// The configured projects, fixed for the life of the process.
    slots: std::collections::BTreeMap<String, Quota>,
    /// The ceiling for every project the configuration does not name, and the
    /// one counter all of them share.
    ///
    /// `None` when no top-level ceiling is set, which is the default and means
    /// an unnamed project is unlimited exactly as it always was.
    unnamed: Option<Quota>,
    /// Where the window index is measured from.
    ///
    /// Monotonic rather than wall clock: a clock stepped backwards would make a
    /// window look as though it had not rolled, and the counter would refuse
    /// traffic that is inside its ceiling.
    started: std::time::Instant,
}

impl ProjectQuotas {
    /// Build the counters: one slot per named ceiling, and one shared slot for
    /// every project the configuration does not name.
    #[must_use]
    pub fn new(limits: std::collections::BTreeMap<String, u32>, unnamed: Option<u32>) -> Self {
        Self {
            slots: limits
                .into_iter()
                .map(|(name, limit)| (name, Quota { limit, packed: AtomicU64::new(0) }))
                .collect(),
            unnamed: unnamed.map(|limit| Quota { limit, packed: AtomicU64::new(0) }),
            started: std::time::Instant::now(),
        }
    }

    /// Whether any project has a ceiling at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty() && self.unnamed.is_none()
    }

    /// Every named ceiling and the project it applies to, in name order.
    ///
    /// The shared ceiling is not in here because it belongs to no one project.
    /// Read it from [`ProjectQuotas::unnamed_limit`].
    pub fn limits(&self) -> impl Iterator<Item = (&str, u32)> {
        self.slots.iter().map(|(name, q)| (name.as_str(), q.limit))
    }

    /// The ceiling every project the configuration does not name shares.
    #[must_use]
    pub fn unnamed_limit(&self) -> Option<u32> {
        self.unnamed.as_ref().map(|q| q.limit)
    }

    /// Whether a sender can leave every configured ceiling by naming a project
    /// nobody wrote down.
    ///
    /// True when ceilings exist but none of them is shared, which is what
    /// `quota_per_minute` under `projects` alone produces. It is reported and
    /// not repaired, because the number that would close it is one only an
    /// operator can choose: see [`ServerConfig::quota_per_minute`].
    ///
    /// False when nothing is limited at all. Nothing is being bypassed there,
    /// the deployment has no ceilings, and telling an operator who set none
    /// that theirs can be walked past would be noise.
    #[must_use]
    pub fn unnamed_are_unbounded(&self) -> bool {
        !self.slots.is_empty() && self.unnamed.is_none()
    }

    /// The counter one project is charged to.
    ///
    /// A name the configuration lists has its own; everything else shares the
    /// one built for unnamed projects.
    fn slot_for(&self, project: &str) -> Option<&Quota> {
        self.slots.get(project).or(self.unnamed.as_ref())
    }

    /// This project's ceiling, or `None` when it has none.
    #[must_use]
    pub fn limit_for(&self, project: &str) -> Option<u32> {
        self.slot_for(project).map(|q| q.limit)
    }

    /// Whether this project is charged to the counter every unnamed project
    /// shares, rather than to one of its own.
    ///
    /// The refusal a sender reads says so, because "you are over project
    /// `typo-in-my-config`'s ceiling" is a confusing thing to be told about a
    /// name you invented a second ago.
    #[must_use]
    pub fn is_shared(&self, project: &str) -> bool {
        !self.slots.contains_key(project) && self.unnamed.is_some()
    }

    /// Charge `wanted` signals to `project` and return how many are permitted.
    ///
    /// Only what it permits is charged, so a refused batch does not push the
    /// project further behind. A project with no ceiling is permitted all of
    /// them and touches no counter.
    pub fn admit(&self, project: &str, wanted: u32) -> u32 {
        self.admit_at(project, wanted, self.elapsed_ms())
    }

    /// Give back part of a charge that was not spent.
    ///
    /// The trim keeps whole traces, so it can store fewer signals than it was
    /// permitted. Without a refund the project pays for signals nobody stored,
    /// and one whose traces are larger than its remaining allowance would be
    /// charged down to zero while storing nothing.
    pub fn refund(&self, project: &str, amount: u32) {
        self.refund_at(project, amount, self.elapsed_ms());
    }

    /// Whole seconds until this project's counter would admit one more signal.
    ///
    /// This is the number a refusal puts in `Retry-After`, and it is derived
    /// from the counter's own state rather than being a figure somebody picked.
    /// A constant would be wrong in both directions: too short and every
    /// refused sender comes back while the ceiling is still full, which is the
    /// retry storm the ceiling exists to prevent; too long and a sender that
    /// could have resumed in two seconds waits a minute.
    ///
    /// Never zero, whatever the arithmetic says. A refusal answered with
    /// `Retry-After: 0` is an instruction to retry immediately into the same
    /// refusal.
    #[must_use]
    pub fn retry_after_seconds(&self, project: &str) -> u64 {
        let ms = self.retry_after_ms_at(project, self.elapsed_ms());
        // Rounded up, because rounding down advertises an instant at which the
        // counter is still full.
        ms.div_ceil(1000).max(1)
    }

    /// [`ProjectQuotas::retry_after_seconds`] in milliseconds at a stated
    /// instant, so the derivation can be tested without a test that waits a
    /// minute.
    ///
    /// The counter is the weighted two-window kind, so "when the window resets"
    /// is not the answer: at the instant this window rolls, its whole count
    /// moves into the decaying half and the ceiling is exactly as full as it
    /// was. What is solved for here is the first instant at which
    /// [`ProjectQuotas::admit`] would return 1, which is the only time worth
    /// telling a sender about.
    fn retry_after_ms_at(&self, project: &str, millis: u64) -> u64 {
        let Some(slot) = self.slot_for(project) else { return 0 };
        let limit = u64::from(slot.limit);
        if limit == 0 {
            // A ceiling of zero admits nothing, ever. `main.rs` refuses one on
            // the way in, so this only guards the arithmetic below.
            return 2 * QUOTA_WINDOW_MS;
        }
        let index = millis / QUOTA_WINDOW_MS;
        let into = millis % QUOTA_WINDOW_MS;
        let (prev, current) = counts_at(slot.packed.load(Ordering::Relaxed), index);

        // `admit_at` permits one more while
        // `current + (prev * (W - x)) / W < limit`, with `x` the offset into
        // the window. Only the previous window's term moves, so within this
        // window the wait is the smallest `x` that shrinks it far enough.
        if current < limit {
            if prev == 0 {
                return 0;
            }
            let room = (limit - current) * QUOTA_WINDOW_MS;
            let x = QUOTA_WINDOW_MS.saturating_sub((room - 1) / prev);
            if x <= into {
                return 0;
            }
            if x < QUOTA_WINDOW_MS {
                return x - into;
            }
        }

        // Not in this window. In the next one this window's count is the
        // decaying half and the new one starts empty, so the same solve runs
        // again from the boundary.
        let ahead = QUOTA_WINDOW_MS - into;
        if current == 0 {
            return ahead;
        }
        let x = QUOTA_WINDOW_MS.saturating_sub((limit * QUOTA_WINDOW_MS - 1) / current);
        if x < QUOTA_WINDOW_MS {
            return ahead + x;
        }
        // Two windows on, both counts have aged out of the sum entirely, so
        // this is the arithmetic ceiling on the answer.
        ahead + QUOTA_WINDOW_MS
    }

    /// Milliseconds since this set of counters was built.
    fn elapsed_ms(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    /// [`ProjectQuotas::admit`] at a stated instant, so the rolling window can
    /// be tested without a test that sleeps for a minute.
    fn admit_at(&self, project: &str, wanted: u32, millis: u64) -> u32 {
        let Some(slot) = self.slot_for(project) else { return wanted };
        let index = millis / QUOTA_WINDOW_MS;
        let into = millis % QUOTA_WINDOW_MS;
        let mut observed = slot.packed.load(Ordering::Relaxed);
        loop {
            let (prev, current) = counts_at(observed, index);
            // The window before this one still counts, weighted by how much of
            // it the minute ending now still covers. A plain per-minute bucket
            // would let a project spend its whole ceiling at 11:59:59 and the
            // whole of it again a second later. This stops that, and it does
            // not stop the same two spends a whole minute apart: see the
            // paragraph on `ProjectQuotas` about what the ceiling bounds. The
            // integer division truncates, which errs towards ageing the old
            // window out slightly fast, by at most one signal.
            let used = current + (prev * (QUOTA_WINDOW_MS - into)) / QUOTA_WINDOW_MS;
            let take = u64::from(wanted).min(u64::from(slot.limit).saturating_sub(used));
            if take == 0 {
                return 0;
            }
            let next = pack(index, prev, (current + take).min(QUOTA_COUNT_MASK));
            // Relaxed on both sides: the word guards nothing but itself, so
            // there is no other write being published or observed through it.
            match slot.packed.compare_exchange_weak(
                observed,
                next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return u32::try_from(take).unwrap_or(u32::MAX),
                Err(seen) => observed = seen,
            }
        }
    }

    /// [`ProjectQuotas::refund`] at a stated instant.
    ///
    /// A refund that arrives after the window rolled is dropped. The charge sits
    /// in the previous window by then, and it ages out within the minute.
    fn refund_at(&self, project: &str, amount: u32, millis: u64) {
        if amount == 0 {
            return;
        }
        let Some(slot) = self.slot_for(project) else { return };
        let index = millis / QUOTA_WINDOW_MS;
        let mut observed = slot.packed.load(Ordering::Relaxed);
        loop {
            if observed >> (QUOTA_COUNT_BITS * 2) != index & QUOTA_INDEX_MASK {
                return;
            }
            let prev = (observed >> QUOTA_COUNT_BITS) & QUOTA_COUNT_MASK;
            let current = observed & QUOTA_COUNT_MASK;
            let next = pack(index, prev, current.saturating_sub(u64::from(amount)));
            match slot.packed.compare_exchange_weak(
                observed,
                next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return,
                Err(seen) => observed = seen,
            }
        }
    }
}

/// The counts a packed word holds as of `index`, ageing out anything older.
fn counts_at(packed: u64, index: u64) -> (u64, u64) {
    let stored = packed >> (QUOTA_COUNT_BITS * 2);
    let prev = (packed >> QUOTA_COUNT_BITS) & QUOTA_COUNT_MASK;
    let current = packed & QUOTA_COUNT_MASK;
    match (index & QUOTA_INDEX_MASK).wrapping_sub(stored) & QUOTA_INDEX_MASK {
        0 => (prev, current),
        1 => (current, 0),
        _ => (0, 0),
    }
}

/// A window index and its two counts, in one word.
fn pack(index: u64, prev: u64, current: u64) -> u64 {
    ((index & QUOTA_INDEX_MASK) << (QUOTA_COUNT_BITS * 2))
        | ((prev & QUOTA_COUNT_MASK) << QUOTA_COUNT_BITS)
        | (current & QUOTA_COUNT_MASK)
}

/// Remove every entry that is empty or only whitespace from one key list.
fn drop_blank(keys: &mut Vec<String>) {
    keys.retain(|k| !k.trim().is_empty());
}

impl ProjectConfig {
    /// Remove every blank entry from this project's six key lists. See
    /// [`ServerConfig::drop_blank_keys`] for why.
    pub fn drop_blank_keys(&mut self) {
        for keys in [
            &mut self.ingest_keys,
            &mut self.read_keys,
            &mut self.write_keys,
            &mut self.admin_keys,
            &mut self.execute_keys,
            &mut self.worker_keys,
        ] {
            drop_blank(keys);
        }
    }
}

impl ServerConfig {
    /// Remove every key entry that is empty or only whitespace, from the six
    /// top-level lists and from every project's.
    ///
    /// clap reads `MCPD_EXECUTE_KEYS=` as one empty value rather than none, and
    /// the compose file passes every optional key variable in that form. The
    /// empty entry made its list non-empty, so the scope did not fall back,
    /// `mcpd config` printed `key required`, and a request that sent an empty
    /// key header matched it. The scope was open while it read as closed.
    ///
    /// No key anybody was given is blank, so dropping these entries loses
    /// nothing. A list left empty then falls back exactly as an unset one does.
    /// The daemon grades a list that held only blank entries before this runs,
    /// and refuses to start on it, because a variable set to nothing was meant
    /// to close its scope. This drop is what keeps an embedder safe all the same.
    /// `AppState::new` calls this, and `main` calls it before `mcpd config`,
    /// `mcpd doctor` and the start-up checks read the lists, so all three
    /// describe the lists the server enforces.
    pub fn drop_blank_keys(&mut self) {
        for keys in [
            &mut self.ingest_keys,
            &mut self.read_keys,
            &mut self.write_keys,
            &mut self.admin_keys,
            &mut self.execute_keys,
            &mut self.worker_keys,
        ] {
            drop_blank(keys);
        }
        for project in self.projects.values_mut() {
            project.drop_blank_keys();
        }
    }

    /// The top-level keys accepted for one scope, applying the same fallback
    /// [`ProjectConfig::keys_for`] applies.
    #[must_use]
    pub fn keys_for(&self, scope: crate::auth::Scope) -> &[String] {
        match scope {
            crate::auth::Scope::Ingest => &self.ingest_keys,
            crate::auth::Scope::Read => &self.read_keys,
            crate::auth::Scope::Write => fall_back(&self.write_keys, &self.ingest_keys),
            crate::auth::Scope::Admin => fall_back(&self.admin_keys, &self.read_keys),
            crate::auth::Scope::Execute => {
                fall_back(&self.execute_keys, self.keys_for(crate::auth::Scope::Write))
            }
            crate::auth::Scope::Worker => {
                fall_back(&self.worker_keys, self.keys_for(crate::auth::Scope::Write))
            }
        }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            default_project: "default".to_owned(),
            // 32 MiB: large enough for a batch of real prompts, small enough
            // that one request cannot exhaust a small container's memory.
            max_body_bytes: 32 * 1024 * 1024,
            max_batch_signals: 10_000,
            retention_days: None,
            ingest_keys: Vec::new(),
            read_keys: Vec::new(),
            write_keys: Vec::new(),
            admin_keys: Vec::new(),
            execute_keys: Vec::new(),
            worker_keys: Vec::new(),
            run_config: mcpd_runs::RunConfig::default(),
            sampling: Policy::always(),
            quota_per_minute: None,
            monitors: Vec::new(),
            slos: Vec::new(),
            // A minute is short enough that an outage is noticed while it is
            // still happening, and long enough that the query never competes
            // with real traffic.
            monitor_interval_seconds: 60,
            // Off. A daemon nobody has configured spawns no analysis timer,
            // takes no connection for it and writes none of its four tables.
            aspects: crate::aspects::AspectSettings::default(),
            forward: None,
            judge: None,
            projects: std::collections::BTreeMap::new(),
            trusted_identity: crate::identity::TrustedIdentity::default(),
            signup: false,
            client_ip: crate::identity::ClientIp::default(),
            cors_origins: Vec::new(),
            derived_metrics: crate::derive::DerivedMetrics::default(),
            // Long enough for a large batch to be read and stored, short
            // enough that a stalled backend is reported rather than waited on.
            request_timeout_seconds: 30,
        }
    }
}

/// Counters describing mcpd's own behaviour, exported at `/metrics`.
///
/// An observability tool that cannot be observed is a liability, so these are
/// not optional and not behind a feature flag.
#[derive(Default)]
pub struct SelfMetrics {
    /// Signals accepted since start.
    pub accepted: AtomicU64,
    /// Signals refused since start.
    pub rejected: AtomicU64,
    /// Batches recognised as replays.
    pub duplicates: AtomicU64,
    /// Traces dropped by the sampler.
    pub sampled_out: AtomicU64,
    /// Signals refused for being over a project's ceiling.
    ///
    /// Counted separately from `rejected`, which it is also part of. A batch
    /// refused for a bad trace id and a batch refused for flooding need
    /// different answers from an operator, and one counter cannot tell them
    /// apart.
    pub over_quota: AtomicU64,
    /// Signals that passed the ceiling step with no ceiling over them.
    ///
    /// Zero on a deployment that configured no ceilings, where nothing is meant
    /// to be bounded, and zero once a top-level ceiling exists, because then
    /// every project has one. It moves in exactly one case: ceilings are set
    /// under `projects` and there is no shared one, so a sender that names
    /// anything else is charged to nothing. That is a hole an operator has to
    /// be able to see from the outside, and `mcpd doctor` is not running at the
    /// moment the hole is used. See [`ProjectQuotas::unnamed_are_unbounded`].
    ///
    /// Read it as a floor rather than an exact total. Two things pull it away
    /// from the number of rows that reached the disk. `ingest::charge_one` adds
    /// to it before `create_score` and `create_feedback` have validated the
    /// record, so a request those handlers then refuse with 400 is counted
    /// although nothing was stored. And `POST /v1/api/score` writes through
    /// `store.ingest` with no ceiling step at all, so however many score rows
    /// it stores for an unbounded project, this counter does not move.
    pub unbounded: AtomicU64,
    /// Ingest requests handled.
    pub ingest_requests: AtomicU64,
    /// Query requests handled.
    pub query_requests: AtomicU64,
    /// Requests that failed with a 5xx.
    ///
    /// Counted by a layer around the whole router rather than in the error
    /// type, so a server error produced by a layer rather than a handler, a
    /// timeout for one, is counted too. Before that layer existed this was
    /// rendered and never moved, which is worse than not rendering it: an
    /// operator alerting on it was alerting on a number that could not change.
    pub errors: AtomicU64,
    /// Spans forwarding gave up on.
    ///
    /// Forwarding never fails the ingest that triggered it, so a downstream
    /// that has been refusing every batch for a week is otherwise a warning in
    /// the log and nothing else. Counted after the retries are spent, by the
    /// number of spans in the batch, never by the number of requests: what an
    /// operator wants to know is how much of the copy is missing.
    pub forward_dropped_spans: AtomicU64,
    /// Runs started through `POST /v1/api/runs`.
    pub runs_started: AtomicU64,
    /// Leases granted to workers.
    pub steps_claimed: AtomicU64,
    /// Steps a worker reported finished.
    pub steps_completed: AtomicU64,
    /// Steps a worker reported failed.
    pub steps_failed: AtomicU64,
    /// Run spans the outbox drain could not write to the signal store and
    /// released for another pass. Non-zero and rising means the signal store is
    /// refusing the daemon's own spans.
    pub run_spans_deferred: AtomicU64,
    /// Audit rows that could not be written.
    ///
    /// An audit write never fails the read it describes, so a broken audit log
    /// is otherwise invisible. This counter is how it becomes visible, and it is
    /// worth alerting on: a read served with no record of it is the thing the
    /// audit log exists to prevent.
    pub audit_write_failures: AtomicU64,
    /// Tokens and cost by provider, model and currency, from every span stored.
    ///
    /// Bounded at [`MODEL_SERIES_MAX`] series: the model name is whatever a
    /// producer sent, so an unbounded map is a way for one sender to grow the
    /// scrape without limit. Spans past the bound are added to one `other`
    /// series rather than dropped, so the totals stay true.
    pub model_usage:
        std::sync::Mutex<std::collections::BTreeMap<(String, String, String), [f64; 3]>>,
}

/// The most provider and model series `/metrics` keeps before the rest are
/// counted under `other`.
pub const MODEL_SERIES_MAX: usize = 200;

/// A Prometheus label value, escaped as the exposition format requires.
fn label(v: &str) -> String {
    v.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
}

impl SelfMetrics {
    /// What a batch's spans add to the per-model counters, read before the
    /// batch is handed to the store and applied with [`Self::add_models`] once
    /// it is written, so a failed write counts nothing.
    #[must_use]
    pub fn model_tally(spans: &[mcpd_core::Span]) -> Vec<((String, String, String), [f64; 3])> {
        let mut out = Vec::new();
        for s in spans {
            let Some(model) = s.model.as_deref() else { continue };
            let usage = s.usage.as_ref();
            let input = usage.and_then(|u| u.input_tokens).unwrap_or(0) as f64;
            let output = usage.and_then(|u| u.output_tokens).unwrap_or(0) as f64;
            let (cost, currency) = s
                .cost
                .as_ref()
                .and_then(|c| c.total_cost.map(|t| (t, c.currency.clone())))
                .unwrap_or((0.0, String::new()));
            if input == 0.0 && output == 0.0 && cost == 0.0 {
                continue;
            }
            out.push((
                (s.provider.clone().unwrap_or_default(), model.to_owned(), currency),
                [input, output, cost],
            ));
        }
        out
    }

    /// Add a tally from [`Self::model_tally`] to the per-model counters.
    pub fn add_models(&self, tally: Vec<((String, String, String), [f64; 3])>) {
        if tally.is_empty() {
            return;
        }
        let Ok(mut map) = self.model_usage.lock() else { return };
        for (mut key, v) in tally {
            if !map.contains_key(&key) && map.len() >= MODEL_SERIES_MAX {
                key = ("other".to_owned(), "other".to_owned(), key.2);
            }
            let e = map.entry(key).or_insert([0.0; 3]);
            e[0] += v[0];
            e[1] += v[1];
            e[2] += v[2];
        }
    }

    /// Render in Prometheus text exposition format.
    #[must_use]
    pub fn render(&self, uptime_seconds: u64) -> String {
        let g = |a: &AtomicU64| a.load(Ordering::Relaxed);
        let mut s = String::with_capacity(1024);
        let mut metric = |name: &str, help: &str, kind: &str, value: u64| {
            s.push_str(&format!("# HELP {name} {help}\n# TYPE {name} {kind}\n{name} {value}\n"));
        };
        metric(
            "mcpd_signals_accepted_total",
            "Signals written to storage.",
            "counter",
            g(&self.accepted),
        );
        metric(
            "mcpd_signals_rejected_total",
            "Signals refused, with a reason returned to the sender.",
            "counter",
            g(&self.rejected),
        );
        metric(
            "mcpd_batches_duplicate_total",
            "Batches recognised as a replay and skipped.",
            "counter",
            g(&self.duplicates),
        );
        metric(
            "mcpd_traces_sampled_out_total",
            "Traces the sampling policy discarded.",
            "counter",
            g(&self.sampled_out),
        );
        metric(
            "mcpd_signals_over_quota_total",
            "Signals refused for exceeding a project's per-minute ceiling.",
            "counter",
            g(&self.over_quota),
        );
        metric(
            "mcpd_signals_unbounded_total",
            "Signals that reached the ceiling step for a project no ceiling covers. \
             Non-zero means a sender can leave the configured ceilings by naming a project \
             the configuration does not list; set --quota-per-minute to bound them.",
            "counter",
            g(&self.unbounded),
        );
        metric(
            "mcpd_ingest_requests_total",
            "Ingest requests handled.",
            "counter",
            g(&self.ingest_requests),
        );
        metric(
            "mcpd_query_requests_total",
            "Query requests handled.",
            "counter",
            g(&self.query_requests),
        );
        metric(
            "mcpd_request_errors_total",
            "Requests that failed with a server error.",
            "counter",
            g(&self.errors),
        );
        metric(
            "mcpd_forward_dropped_spans_total",
            "Spans not copied to the forwarding destination after every retry was spent. \
             The local write succeeded; only the forwarded copy is missing.",
            "counter",
            g(&self.forward_dropped_spans),
        );
        metric("mcpd_runs_started_total", "Runs started.", "counter", g(&self.runs_started));
        metric(
            "mcpd_steps_claimed_total",
            "Step leases granted to workers.",
            "counter",
            g(&self.steps_claimed),
        );
        metric(
            "mcpd_steps_completed_total",
            "Steps a worker reported finished.",
            "counter",
            g(&self.steps_completed),
        );
        metric(
            "mcpd_steps_failed_total",
            "Steps a worker reported failed.",
            "counter",
            g(&self.steps_failed),
        );
        metric(
            "mcpd_run_spans_deferred_total",
            "Run spans the outbox drain could not write and will retry. Rising means the signal \
             store is refusing the daemon's own spans.",
            "counter",
            g(&self.run_spans_deferred),
        );
        metric(
            "mcpd_audit_write_failures_total",
            "Audit rows that could not be written. The request they describe still succeeded.",
            "counter",
            g(&self.audit_write_failures),
        );
        metric(
            "mcpd_uptime_seconds",
            "Seconds since this process started.",
            "gauge",
            uptime_seconds,
        );
        if let Ok(map) = self.model_usage.lock() {
            if !map.is_empty() {
                for (i, (name, help)) in [
                    ("mcpd_model_input_tokens_total", "Input tokens on stored spans, by model."),
                    ("mcpd_model_output_tokens_total", "Output tokens on stored spans, by model."),
                    (
                        "mcpd_model_cost_total",
                        "Priced cost of stored spans, by model and currency.",
                    ),
                ]
                .into_iter()
                .enumerate()
                {
                    s.push_str(&format!("# HELP {name} {help}\n# TYPE {name} counter\n"));
                    for ((provider, model, currency), v) in map.iter() {
                        let cur = if i == 2 {
                            format!(",currency=\"{}\"", label(currency))
                        } else {
                            String::new()
                        };
                        s.push_str(&format!(
                            "{name}{{provider=\"{}\",model=\"{}\"{cur}}} {}\n",
                            label(provider),
                            label(model),
                            v[i]
                        ));
                    }
                }
            }
        }
        s
    }
}

/// Shared application state.
pub struct AppState {
    /// The storage backend.
    pub store: Arc<dyn Store>,
    /// Server configuration.
    pub config: ServerConfig,
    /// Redaction pipeline, applied before anything is written.
    pub redactor: Redactor,
    /// Price book used to cost model calls at ingest.
    pub price_book: PriceBook,
    /// Self-observability counters.
    pub metrics: SelfMetrics,
    /// When the process started.
    pub started_at: OffsetDateTime,
    /// The latched condition of every configured objective. Derived state, held
    /// here for the same reason the monitors' is.
    pub slos: std::sync::RwLock<Vec<crate::monitor::SloState>>,
    /// The latched condition of every configured monitor.
    ///
    /// Held here rather than in the store because it is derived state: it is
    /// rebuilt from readings on every start, and persisting it would mean a
    /// restart could resurrect an alert that has already recovered.
    pub monitors: std::sync::RwLock<Vec<crate::monitor::MonitorState>>,
    /// The client used for forwarding. One client, so connections are pooled.
    pub outbound: reqwest::Client,
    /// Per-project sampling, resolved once at construction.
    pub project_policies: std::collections::BTreeMap<String, Policy>,
    /// Per-project ingest ceilings and their counters, resolved once at
    /// construction for the same reason the policies are.
    pub quotas: ProjectQuotas,
    /// Background work the process owns: in-flight forwards, and the loops
    /// that run for the life of the process.
    ///
    /// Spawn through this rather than `tokio::spawn`, so shutdown can wait for
    /// what is still running. A forward spawned bare was dropped the moment
    /// `axum::serve` returned, and a batch that had already been stored locally
    /// was silently missing from the destination.
    pub tasks: tokio_util::task::TaskTracker,
    /// Cancelled when the stop signal arrives, before the listener has closed.
    /// A loop that sleeps between passes watches this so it stops between
    /// passes rather than mid-pass, a retry that is waiting to try again gives
    /// up rather than holding the shutdown to its full bound, and a long-poll
    /// answers at once, which is what lets the listener close at all.
    pub shutdown: tokio_util::sync::CancellationToken,
    /// Room for forwarded copies still waiting on the destination. See
    /// `forward::MAX_IN_FLIGHT` for why it is bounded and what happens when
    /// it is full.
    pub forward_slots: Arc<tokio::sync::Semaphore>,
    /// The durable run store, when this daemon serves runs. `None` answers
    /// every run endpoint with a 503 that says so, which is the state a test
    /// router is in until it is given one.
    pub runs: Option<Arc<dyn mcpd_runs::RunStore>>,
    /// Held by an outbox drain pass from its claim to its acknowledgement, and
    /// by a subject erasure around both of its store erasures.
    ///
    /// A pass holds up to a batch of a run's spans in memory between the
    /// claim and the write. An erasure that ran in that window removed the run
    /// and the trace, answered that both were gone, and the pass then wrote the
    /// spans back into the trace store. Taking turns closes the window: a pass
    /// that claimed first has written by the time the erasure reads the trace
    /// store, and a pass that comes after finds the rows already deleted.
    pub outbox_gate: tokio::sync::Mutex<()>,
    /// The accounts, when the daemon runs with `--signup`. `None` otherwise,
    /// and then no credential is looked up anywhere but the configuration.
    pub accounts: Option<Arc<crate::accounts::AccountStore>>,
    /// The rate limits and the hashing bound on sign-up and log-in.
    pub account_guards: crate::accounts::AccountGuards,
}

impl ServerConfig {
    /// The per-project sampling policies, derived from the projects block.
    ///
    /// Built once at construction rather than per batch: the map is small and
    /// fixed for the life of the process, and the ingest path is the hottest
    /// code in the daemon.
    #[must_use]
    pub fn project_policies(&self) -> std::collections::BTreeMap<String, Policy> {
        self.projects
            .iter()
            .filter_map(|(name, p)| {
                p.sample.map(|fraction| (name.clone(), Policy::deterministic(fraction)))
            })
            .collect()
    }

    /// The per-project ingest ceilings, derived from the projects block.
    ///
    /// Built the same way the retention plan is: a project's own number, else
    /// the global one, plus the default project so a single-project deployment
    /// gets a ceiling from the flag alone. With no flag and no project setting
    /// this is empty, and an empty set of quotas does no work at ingest.
    ///
    /// The top-level number is also handed to the shared counter, so a project
    /// this block does not name is charged for what it stores instead of being
    /// charged to nothing. The sender chooses the project name, so without that
    /// number the ceiling stays optional from the sender's side, and this
    /// function cannot invent one: a limit derived from the named ceilings is a
    /// number nobody wrote, and it would start refusing an unnamed project that
    /// an upgrade found already ingesting. `unnamed_are_unbounded` is how that
    /// state is reported instead.
    #[must_use]
    pub fn project_quotas(&self) -> ProjectQuotas {
        let mut limits: std::collections::BTreeMap<String, u32> = self
            .projects
            .iter()
            .filter_map(|(name, p)| {
                p.quota_per_minute.or(self.quota_per_minute).map(|n| (name.clone(), n))
            })
            .collect();
        // The default project keeps a counter of its own rather than falling
        // into the shared one. An operator named it with --project, and it
        // carries the traffic of a deployment that never opened the projects
        // block at all.
        if let Some(n) = self.quota_per_minute {
            limits.entry(self.default_project.clone()).or_insert(n);
        }
        ProjectQuotas::new(limits, self.quota_per_minute)
    }
}

impl AppState {
    /// Build state around a store, with sensible defaults for everything else.
    #[must_use]
    pub fn new(store: Arc<dyn Store>, mut config: ServerConfig) -> Self {
        // Here as well as in `main`, so a configuration built by an embedder or
        // a test cannot carry a blank key into the server either.
        config.drop_blank_keys();
        let project_policies = config.project_policies();
        let quotas = config.project_quotas();
        let config_monitors = config.monitors.clone();
        let config_slos = config.slos.clone();
        Self {
            store,
            config,
            redactor: Redactor::with_defaults(),
            price_book: mcpd_core::cost::builtin_book(),
            metrics: SelfMetrics::default(),
            started_at: OffsetDateTime::now_utc(),
            monitors: std::sync::RwLock::new(
                config_monitors.into_iter().map(crate::monitor::MonitorState::new).collect(),
            ),
            slos: std::sync::RwLock::new(
                config_slos.into_iter().map(crate::monitor::SloState::new).collect(),
            ),
            project_policies,
            quotas,
            outbound: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
            tasks: tokio_util::task::TaskTracker::new(),
            shutdown: tokio_util::sync::CancellationToken::new(),
            forward_slots: Arc::new(tokio::sync::Semaphore::new(crate::forward::MAX_IN_FLIGHT)),
            runs: None,
            outbox_gate: tokio::sync::Mutex::new(()),
            accounts: None,
            account_guards: crate::accounts::AccountGuards::default(),
        }
    }

    /// Serve runs from this store.
    #[must_use]
    pub fn with_run_store(mut self, runs: Arc<dyn mcpd_runs::RunStore>) -> Self {
        self.runs = Some(runs);
        self
    }

    /// Keep accounts in this store. Only consulted when `config.signup` is on.
    #[must_use]
    pub fn with_account_store(mut self, accounts: Arc<crate::accounts::AccountStore>) -> Self {
        self.accounts = Some(accounts);
        self
    }

    /// Stop the background work and wait for it, for at most `grace`.
    ///
    /// Called after the listener has closed. Cancels the loops, refuses new
    /// tracked tasks, and waits for the tracked ones to finish. Returns how many
    /// were still running when the grace period ran out, which is zero on a
    /// clean stop and worth logging otherwise: those are forwards whose spans
    /// were stored locally and never reached the destination.
    pub async fn drain(&self, grace: std::time::Duration) -> usize {
        self.shutdown.cancel();
        self.tasks.close();
        match tokio::time::timeout(grace, self.tasks.wait()).await {
            Ok(()) => 0,
            Err(_) => self.tasks.len(),
        }
    }

    /// Replace the redactor, for a deployment with its own rules.
    #[must_use]
    pub fn with_redactor(mut self, r: Redactor) -> Self {
        self.redactor = r;
        self
    }

    /// Replace the price book with negotiated rates.
    ///
    /// Nothing in this repository calls it. `mcpd` installs `builtin_book` in
    /// `new` and no flag, config file key or endpoint reaches here, so this is
    /// the injection point for a fork, for an embedder, and for a flag if one is
    /// ever added. The cost page used to send an operator here, which read as a
    /// capability the daemon has; it now says plainly that a book cannot be
    /// supplied. Wiring this up is what would make that paragraph true.
    #[must_use]
    pub fn with_price_book(mut self, b: PriceBook) -> Self {
        self.price_book = b;
        self
    }

    /// Seconds since start.
    #[must_use]
    pub fn uptime_seconds(&self) -> u64 {
        (OffsetDateTime::now_utc() - self.started_at).whole_seconds().max(0) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A single named project with one ceiling, and its counter at zero. No
    /// ceiling for anything the configuration does not name.
    fn quotas(project: &str, limit: u32) -> ProjectQuotas {
        ProjectQuotas::new(std::collections::BTreeMap::from([(project.to_owned(), limit)]), None)
    }

    #[test]
    fn a_project_with_no_ceiling_is_admitted_in_full_and_touches_no_counter() {
        let q = quotas("alpha", 10);
        assert!(q.limit_for("beta").is_none());
        // Far more than alpha's ceiling, twice, to show nothing accumulates.
        assert_eq!(q.admit_at("beta", 5_000, 0), 5_000);
        assert_eq!(q.admit_at("beta", 5_000, 1_000), 5_000);
    }

    #[test]
    fn a_default_configuration_configures_no_ceilings_at_all() {
        let config = ServerConfig::default();
        assert!(config.quota_per_minute.is_none());
        assert!(config.project_quotas().is_empty());
    }

    #[test]
    fn only_the_excess_is_refused_and_the_rest_is_charged() {
        let q = quotas("alpha", 10);
        assert_eq!(q.admit_at("alpha", 4, 0), 4);
        // Six left, so nine asked for is six permitted, not nine and not zero.
        assert_eq!(q.admit_at("alpha", 9, 0), 6);
        assert_eq!(q.admit_at("alpha", 1, 0), 0);
    }

    #[test]
    fn the_window_rolls_so_a_project_that_was_over_is_accepted_again() {
        let q = quotas("alpha", 10);
        assert_eq!(q.admit_at("alpha", 10, 0), 10, "the first minute is inside the ceiling");
        assert_eq!(q.admit_at("alpha", 10, 1_000), 0, "the same minute is full");
        // Two windows on, nothing from the first one is in view any more.
        assert_eq!(q.admit_at("alpha", 10, 120_000), 10, "a rolled window is a fresh ceiling");
    }

    #[test]
    fn the_window_before_this_one_still_counts_so_a_boundary_is_not_a_free_ceiling() {
        let q = quotas("alpha", 10);
        assert_eq!(q.admit_at("alpha", 10, 0), 10);
        // A per-minute bucket would hand back all ten here, which is twice the
        // configured ceiling inside one second.
        assert_eq!(q.admit_at("alpha", 10, 60_000), 0, "the whole of the last minute overlaps");
        // Half of the old window has fallen out of view, so half the ceiling is
        // free again.
        assert_eq!(q.admit_at("alpha", 10, 90_000), 5, "half the last minute overlaps");
    }

    #[test]
    fn an_unspent_charge_can_be_handed_back_within_the_same_window() {
        let q = quotas("alpha", 10);
        assert_eq!(q.admit_at("alpha", 10, 0), 10);
        q.refund_at("alpha", 4, 0);
        assert_eq!(q.admit_at("alpha", 10, 0), 4, "the refund is available again");
    }

    #[test]
    fn a_refund_arriving_after_the_window_rolled_is_dropped() {
        let q = quotas("alpha", 10);
        assert_eq!(q.admit_at("alpha", 10, 0), 10);
        // The charge is in the previous window by now. Crediting the current one
        // would let a project carry an allowance forward.
        q.refund_at("alpha", 4, 60_000);
        assert_eq!(q.admit_at("alpha", 10, 60_000), 0);
    }

    /// The wait a 429 advertises has to be a time at which the retry works.
    /// A number that is too small produces the retry storm the ceiling exists
    /// to prevent, and one that is too large costs a sender data it could have
    /// sent. This is the property, checked against the counter itself.
    #[test]
    fn the_advertised_wait_is_an_instant_the_counter_actually_admits() {
        for (limit, at) in [(10u32, 0u64), (10, 30_000), (10, 59_999), (1, 12_345), (500, 45_000)] {
            let q = quotas("alpha", limit);
            assert_eq!(q.admit_at("alpha", limit, at), limit, "spend the whole ceiling");
            assert_eq!(q.admit_at("alpha", 1, at), 0, "and confirm it is spent");

            let wait = q.retry_after_ms_at("alpha", at);
            assert!(wait > 0, "a spent ceiling should not advertise no wait at all");
            // One millisecond before the advertised instant, still refused.
            assert_eq!(
                q.admit_at("alpha", 1, at + wait - 1),
                0,
                "limit {limit} at {at}: the wait of {wait}ms is longer than it needs to be"
            );
            // And at it, admitted.
            assert_eq!(
                q.admit_at("alpha", 1, at + wait),
                1,
                "limit {limit} at {at}: the counter is still full after the advertised {wait}ms"
            );
        }
    }

    /// The window boundary is not the answer, and this is why. At the instant
    /// the window rolls, the whole of the spent count moves into the decaying
    /// half and the ceiling is exactly as full as it was.
    #[test]
    fn the_wait_is_not_the_time_until_the_window_rolls() {
        let q = quotas("alpha", 10);
        assert_eq!(q.admit_at("alpha", 10, 0), 10);
        assert_eq!(q.admit_at("alpha", 1, 60_000), 0, "the boundary is not a fresh ceiling");
        assert!(
            q.retry_after_ms_at("alpha", 0) > 60_000,
            "the wait stopped at the boundary, where the counter is still full"
        );
    }

    /// It shrinks as the window it is measured in advances, which is the whole
    /// difference between a derived number and a constant.
    #[test]
    fn the_wait_shrinks_as_the_window_advances() {
        let q = quotas("alpha", 10);
        assert_eq!(q.admit_at("alpha", 10, 0), 10);
        let early = q.retry_after_ms_at("alpha", 1_000);
        let later = q.retry_after_ms_at("alpha", 30_000);
        assert!(later < early, "the wait did not move with the window: {early} then {later}");
        // Two windows is the arithmetic ceiling: by then both counts have aged
        // out of the sum entirely.
        assert!(early <= 2 * QUOTA_WINDOW_MS, "the wait is longer than the counter can be full");
    }

    /// A project inside its ceiling is not waiting for anything, and a project
    /// with no ceiling at all never was.
    #[test]
    fn nothing_to_wait_for_is_reported_as_no_wait() {
        let q = quotas("alpha", 10);
        assert_eq!(q.retry_after_ms_at("alpha", 0), 0);
        assert_eq!(q.retry_after_ms_at("beta", 0), 0, "beta has no ceiling");
        // The public form never advertises zero, whatever the arithmetic says:
        // a client told to wait no time at all retries into the same refusal.
        assert_eq!(q.retry_after_seconds("beta"), 1);
    }

    #[test]
    fn a_project_setting_beats_the_global_one_and_the_default_project_gets_a_slot() {
        let config = ServerConfig {
            default_project: "default".to_owned(),
            quota_per_minute: Some(100),
            projects: std::collections::BTreeMap::from([
                (
                    "alpha".to_owned(),
                    ProjectConfig { quota_per_minute: Some(5), ..Default::default() },
                ),
                ("beta".to_owned(), ProjectConfig::default()),
            ]),
            ..Default::default()
        };
        let q = config.project_quotas();
        assert_eq!(
            q.limits().collect::<Vec<_>>(),
            vec![("alpha", 5), ("beta", 100), ("default", 100)]
        );
    }

    #[test]
    fn without_the_flag_only_projects_that_asked_for_a_ceiling_have_one() {
        let config = ServerConfig {
            default_project: "default".to_owned(),
            quota_per_minute: None,
            projects: std::collections::BTreeMap::from([
                (
                    "alpha".to_owned(),
                    ProjectConfig { quota_per_minute: Some(5), ..Default::default() },
                ),
                ("beta".to_owned(), ProjectConfig::default()),
            ]),
            ..Default::default()
        };
        let q = config.project_quotas();
        assert_eq!(q.limits().collect::<Vec<_>>(), vec![("alpha", 5)]);
        assert_eq!(q.unnamed_limit(), None, "no top-level number, so no shared ceiling");
        assert_eq!(q.admit_at("default", 9_999, 0), 9_999);
    }

    #[test]
    fn ceilings_written_only_under_projects_leave_every_other_name_unbounded() {
        // The behaviour this configuration actually has, stated rather than
        // implied. Writing `quota_per_minute` under each project is the natural
        // way to set per-project limits, and it bounds those projects and
        // nothing else: the sender picks the project name in the body of its
        // own request, so it can leave the ceiling by naming anything else.
        // Nothing here refuses that traffic, because refusing a name nobody
        // listed would drop the data of a deployment that upgraded without
        // changing its configuration. It is reported instead.
        let config = ServerConfig {
            default_project: "default".to_owned(),
            quota_per_minute: None,
            projects: std::collections::BTreeMap::from([(
                "alpha".to_owned(),
                ProjectConfig { quota_per_minute: Some(5), ..Default::default() },
            )]),
            ..Default::default()
        };
        let q = config.project_quotas();

        assert!(q.unnamed_are_unbounded(), "this is the configuration that leaves the gap open");
        assert_eq!(q.limit_for("invented-by-the-sender"), None, "no counter to charge it to");
        assert_eq!(q.admit_at("invented-by-the-sender", 9_999, 0), 9_999, "stored, not refused");
        // And the ceiling that was configured still holds for the project it
        // names, which is the half that must not regress.
        assert_eq!(q.admit_at("alpha", 9_999, 0), 5);
    }

    #[test]
    fn a_deployment_with_no_ceilings_has_no_gap_to_report() {
        // Nothing is being walked past when nothing was limited. Warning an
        // operator who set no ceiling that theirs can be bypassed would be
        // noise, and it would fire on every default deployment there is.
        let q = ServerConfig::default().project_quotas();
        assert!(q.is_empty());
        assert!(!q.unnamed_are_unbounded());
    }

    #[test]
    fn the_top_level_ceiling_closes_the_gap_because_every_name_has_a_counter() {
        let config = ServerConfig {
            default_project: "default".to_owned(),
            quota_per_minute: Some(10),
            projects: std::collections::BTreeMap::from([(
                "alpha".to_owned(),
                ProjectConfig { quota_per_minute: Some(5), ..Default::default() },
            )]),
            ..Default::default()
        };
        let q = config.project_quotas();
        assert!(!q.unnamed_are_unbounded(), "the shared counter is what closes it");
        assert_eq!(q.limit_for("invented-by-the-sender"), Some(10));
    }

    #[test]
    fn a_project_no_configuration_names_is_still_charged_for_what_it_stores() {
        // The hole this closes: the project name is chosen by whoever sends the
        // batch, so before this an unlisted name was charged to nothing at all
        // and the ceiling was optional from the sender's side.
        let config = ServerConfig {
            default_project: "default".to_owned(),
            quota_per_minute: Some(10),
            ..Default::default()
        };
        let q = config.project_quotas();
        assert_eq!(q.unnamed_limit(), Some(10));
        assert_eq!(q.limit_for("nobody-configured-this"), Some(10));
        assert!(q.is_shared("nobody-configured-this"));
        assert_eq!(q.admit_at("nobody-configured-this", 9_999, 0), 10);
        assert_eq!(q.admit_at("nobody-configured-this", 1, 0), 0);
    }

    #[test]
    fn inventing_a_second_name_buys_no_second_allowance() {
        // One counter for every unnamed project, so spreading a flood over a
        // thousand invented names is bounded by the same number as using one.
        let q = ProjectQuotas::new(std::collections::BTreeMap::new(), Some(10));
        assert_eq!(q.admit_at("made-up-1", 6, 0), 6);
        assert_eq!(q.admit_at("made-up-2", 6, 0), 4, "the remainder of the shared ceiling");
        assert_eq!(q.admit_at("made-up-3", 6, 0), 0);
    }

    #[test]
    fn a_named_project_keeps_its_own_counter_and_is_not_charged_the_shared_one() {
        let q = ProjectQuotas::new(
            std::collections::BTreeMap::from([("alpha".to_owned(), 10)]),
            Some(10),
        );
        assert!(!q.is_shared("alpha"));
        assert_eq!(q.admit_at("made-up", 10, 0), 10, "the shared counter is spent");
        assert_eq!(q.admit_at("alpha", 10, 0), 10, "alpha's own is untouched");
    }

    #[test]
    fn a_refund_from_an_unnamed_project_returns_to_the_shared_counter() {
        let q = ProjectQuotas::new(std::collections::BTreeMap::new(), Some(10));
        assert_eq!(q.admit_at("made-up", 10, 0), 10);
        q.refund_at("made-up", 4, 0);
        assert_eq!(q.admit_at("another-made-up", 10, 0), 4);
    }

    /// The most one project stored inside any 60-second span, given when it
    /// asked and how much for.
    ///
    /// "Per minute" has to mean a minute a reader could point at, not the
    /// counter's own window, so this slides over the charges that actually
    /// landed rather than reading the counter.
    fn busiest_minute(q: &ProjectQuotas, project: &str, asks: &[(u64, u32)]) -> u32 {
        let charges: Vec<(u64, u32)> = asks
            .iter()
            .map(|(at, wanted)| (*at, q.admit_at(project, *wanted, *at)))
            .filter(|(_, taken)| *taken > 0)
            .collect();
        charges
            .iter()
            .map(|(from, _)| {
                charges
                    .iter()
                    .filter(|(t, _)| *t >= *from && *t < from + QUOTA_WINDOW_MS)
                    .map(|(_, n)| *n)
                    .sum::<u32>()
            })
            .max()
            .unwrap_or(0)
    }

    #[test]
    fn a_ceiling_bounds_the_sustained_rate_and_permits_one_burst_on_top() {
        // The bound the doc comment on `ProjectQuotas` states, measured rather
        // than asserted, against the pattern that is worst for it: wait to the
        // last millisecond of a window, spend the whole ceiling, wait again.
        // Both bursts land inside one 60-second span.
        const LIMIT: u32 = 1_000;
        let patient = quotas("alpha", LIMIT);
        let worst = busiest_minute(
            &patient,
            "alpha",
            &[(QUOTA_WINDOW_MS - 1, LIMIT), (2 * QUOTA_WINDOW_MS - 2, LIMIT)],
        );
        // Twice, and not a signal more. Both halves matter: a larger number is
        // a ceiling that stopped bounding, and a smaller one means the burst an
        // SDK produces when it flushes its queue is now being refused.
        assert_eq!(worst, 2 * LIMIT, "worst minute stored {worst} against a ceiling of {LIMIT}");

        // And the bound holds against a flood as well as against patience: a
        // sender asking for the whole ceiling ten times a second for five
        // minutes never gets past it.
        let greedy = quotas("beta", LIMIT);
        let asks: Vec<(u64, u32)> =
            (0..5 * QUOTA_WINDOW_MS / 100).map(|n| (n * 100, LIMIT)).collect();
        let flood = busiest_minute(&greedy, "beta", &asks);
        assert!(flood <= 2 * LIMIT, "a flood stored {flood} in one minute");

        // The other half of the same fact. A sender holding steady at exactly
        // its ceiling is not throttled below it, which is what a counter that
        // removed the burst by refusing it would cost.
        let steady = quotas("gamma", LIMIT);
        let mut stored = 0;
        for minute in 0..10u64 {
            for slice in 0..100u64 {
                stored += steady.admit_at(
                    "gamma",
                    LIMIT / 100,
                    minute * QUOTA_WINDOW_MS + slice * (QUOTA_WINDOW_MS / 100),
                );
            }
        }
        assert!(stored >= 9_900, "a sender at its ceiling stored {stored} of 10000");
    }
}
