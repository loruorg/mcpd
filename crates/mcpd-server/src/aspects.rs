//! Aspects, aspect notes and themes.
//!
//! An **aspect** is an axis a trace is described along. An **aspect note** is
//! one sentence recorded against one trace along that axis. A **theme** is a
//! cluster of notes that share their wording. Those three words are the whole
//! vocabulary and nothing here uses any other; `signal`, `dimension`, `label`
//! and `finding` all already mean something else in this repository.
//!
//! Three things about this module are worth reading before changing it.
//!
//! **No model, prompt or credential reaches the daemon.** A note is written
//! outside mcpd and posted through the ordinary ingest door, except for the one
//! derived aspect below, which the daemon writes itself from facts it already
//! holds. mcpd never sits in the request path of a model call.
//!
//! **There is no floating point in the clustering.** A sketch is 64 integers,
//! a similarity is a count out of 64, and a cluster boundary is an integer
//! comparison, so two replicas of one deployment reading the same notes cannot
//! disagree about where that boundary falls. Every ordering below states its
//! tie-break, and anything that reaches a response is built in a `BTreeMap`, so
//! the same input produces the same bytes.
//!
//! **Reads never recompute.** A pass writes a snapshot on a timer and every
//! read is a lookup of what it wrote, so an operator refreshing a screen sees
//! the same numbers rather than a fresh clustering.

use crate::state::AppState;
use mcpd_core::sketch::{Sketch, BANDS};
use mcpd_core::{
    Aspect, AspectLimits, AspectReadiness, EntityKind, Theme, ThemeSnapshot, ThemeState,
};
use mcpd_store::{
    AspectNoteQuery, Order, SnapshotQuery, SnapshotWrite, StoredTheme, ThemeLineage, ThemeMember,
    ThemeQuery, NOISE_THEME_ID,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use time::{Duration, OffsetDateTime};

/* ------------------------------------------------------------- the bounds -- */

/// How many notes one aspect on one entity needs before a pass runs at all.
///
/// Fifty. It is the same shape of decision as `MIN_BASELINE_RUNS` in
/// `monitor.rs` and it is here for the same reason: below a floor the honest
/// answer is that the daemon declined to look, not a handful of sentences
/// sorted into groups nobody should draw a conclusion from. Fifty is higher
/// than the divergence watch's twelve because that statistic describes one
/// number and this one describes a partition: a cluster needs members before
/// it means anything, and a partition of a dozen sentences is a list.
///
/// Below it the aspect reads `collecting` and no snapshot is written at all.
/// It is a floor, not a target: an entity that never reaches fifty notes stays
/// `collecting` for ever, which is the truth about it.
pub const MIN_NOTES_TO_CLUSTER: u64 = 50;

/// The most notes one pass reads, taken newest first.
///
/// Twenty thousand, copied from `DIVERGENCE_RUN_CAP`'s reasoning rather than
/// its value: a pass runs on a timer, so a slow one delays every pass behind
/// it, and a window on a busy project can hold more notes than there is any
/// point clustering. Applied newest first, so what the cap drops is the oldest
/// end of the window, and the snapshot is marked `capped` so every share in it
/// is read as a share of what was read.
pub const MAX_NOTES_PER_PASS: u64 = 20_000;

/// How many notes one page of that walk asks for.
pub const PASS_PAGE: u32 = 500;

/// How many aspects one project may define, the four built-ins included.
///
/// Published in [`AspectLimits`] so a client that lets an operator add an
/// aspect knows where to stop rather than finding out from a refusal. It bounds
/// the flow route too: a flow reads one stage per aspect, so an unbounded
/// catalogue would be an unbounded read.
pub const MAX_ASPECTS: u32 = 32;

/// How alike two notes must be, out of 64, to sit in one theme.
///
/// A similarity is the bottom-k estimate of how much two notes' wording
/// overlaps, scaled to a denominator of 64. Twenty-six is about two fifths.
/// Lower and sentences that share only their common words start joining;
/// higher and one rewording of the same sentence splits away from it.
///
/// It is a starting point to tune from and not a measured constant, and it is
/// safe to move: every ordering in the pass states its tie-break, so a
/// different threshold produces different groups and never a different answer
/// for the same groups.
pub const MIN_SHARED: u32 = 26;

/// The fewest notes a theme can hold.
///
/// Two. Noise is documented as holding "patterns that have not recurred yet",
/// so a group of one has not recurred and is noise by that definition. Nothing
/// is lost by it: a singleton's note is still readable from the noise node of
/// the stage it belongs to.
pub const MIN_THEME_NOTES: usize = 2;

/// How many earlier snapshots a theme may be absent from and still come back
/// under its own id.
///
/// Eight. A theme that comes and goes should read as one theme with gaps rather
/// than as a run of short-lived ones, and this is how far back the pass looks
/// for the id to reuse. Bounded because the alternative is a search of the
/// entity's whole history on every pass, which grows without limit while the
/// answer it can give does not.
pub const RESURRECTION_LOOKBACK: usize = 8;

/// How many entities one tick clusters.
///
/// A pass walks the entities that have notes, and a project can have more of
/// them than one tick should hold. The cap is applied to the roll-up, which is
/// ordered by when each entity was last written to, so what it drops is the
/// entity nothing has written about for longest.
pub const ENTITIES_PER_PASS: usize = 200;

/* --------------------------------------------------------- configuration -- */

/// The shortest interval a pass runs on, in seconds. A smaller
/// `interval_seconds` is raised to this, and `mcpd config` prints the raised
/// value, because that is the one that runs.
pub const MIN_INTERVAL_SECONDS: u64 = 30;

/// What the analysis pass is armed with.
///
/// Off by default, and that is the rule this whole module is written under: a
/// daemon nobody has configured starts, takes no connection, writes no table
/// and spawns no timer. [`spawn`] returns before spawning anything, exactly as
/// `monitor::spawn` returns when no monitor is armed.
///
/// The derived `outcome` aspect is what stops that being useless. Switching
/// this on is the only configuration the analysis needs: no model, no prompt
/// and no credential, because the daemon writes the outcome notes itself from
/// facts it already holds. A deployment therefore gets a working themes screen
/// from one boolean, before anyone wires a model to anything.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AspectSettings {
    /// Whether the daemon clusters notes and derives the outcome aspect.
    #[serde(default)]
    pub enabled: bool,
    /// How often a pass runs, in seconds.
    #[serde(default = "default_interval")]
    pub interval_seconds: u64,
    /// How far back one pass reads notes, in seconds.
    ///
    /// The window moves with the pass, so two snapshots overlap and their
    /// counts must never be added together. A day by default: short enough
    /// that a theme appearing means something recent, long enough that a
    /// project writing a few notes an hour reaches the floor of fifty.
    #[serde(default = "default_window")]
    pub window_seconds: u64,
    /// Aspects switched off in this deployment, by name.
    ///
    /// Notes already recorded under one stay readable and the aspect stays in
    /// the catalogue: a name that vanished from the list would make the notes
    /// filed under it look like a mistake. What changes is that no pass
    /// clusters it and no progress row counts towards readiness.
    #[serde(default)]
    pub disabled: Vec<String>,
}

const fn default_interval() -> u64 {
    300
}

const fn default_window() -> u64 {
    86_400
}

impl Default for AspectSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_seconds: default_interval(),
            window_seconds: default_window(),
            disabled: Vec::new(),
        }
    }
}

/// The longest window one pass reads, in seconds: a year.
///
/// The window's start is the pass's time minus this, and a number past the
/// calendar's range made that subtraction panic inside the timer on every
/// pass.
pub const WINDOW_SECONDS_MAX: u64 = 365 * 86_400;

/// The longest wait between two passes, in seconds: a week. Past it the
/// timer's own arithmetic can overflow, and a themes screen a week stale has
/// stopped describing anything.
pub const INTERVAL_SECONDS_MAX: u64 = 7 * 86_400;

impl AspectSettings {
    /// Refuse the settings a pass would fail on.
    ///
    /// Called at start-up, so a window the pass cannot subtract is a refusal
    /// to start with the key named, rather than a panic on every tick of a
    /// timer nobody is watching.
    ///
    /// # Errors
    /// Returns a message naming the key and the range it must be in.
    pub fn check(&self) -> Result<(), String> {
        if self.window_seconds == 0 || self.window_seconds > WINDOW_SECONDS_MAX {
            return Err(format!(
                "aspects.window_seconds must be from 1 to {WINDOW_SECONDS_MAX}, a year, got {}; \
                 omit it for the default of {}",
                self.window_seconds,
                default_window()
            ));
        }
        if self.interval_seconds > INTERVAL_SECONDS_MAX {
            return Err(format!(
                "aspects.interval_seconds must be at most {INTERVAL_SECONDS_MAX}, a week, got {}; \
                 omit it for the default of {}",
                self.interval_seconds,
                default_interval()
            ));
        }
        Ok(())
    }

    /// Whether one aspect is switched on here.
    ///
    /// The deny list and nothing else. It deliberately does **not** also read
    /// `enabled`, which arms the pass: a daemon with the pass off still accepts
    /// notes through the ingest door and still hands them back, so reporting
    /// every aspect as switched off there would misdescribe what the deployment
    /// is actually storing and turn an entity's `2 of 4` into `0 of 0`.
    ///
    /// Whether anything clusters is a separate fact and it is reported
    /// separately, as `pass_armed` on [`limits`]. Folding the two into this one
    /// answer is what made the catalogue say `enabled` on a daemon where
    /// nothing ran, and splitting them is what lets it say both things at once.
    #[must_use]
    pub fn allows(&self, aspect: &str) -> bool {
        !self.disabled.iter().any(|d| d == aspect)
    }
}

/* -------------------------------------------------------- the catalogue -- */

/// The name of the one aspect the daemon writes itself.
pub const OUTCOME: &str = "outcome";

/// One aspect as it ships.
struct BuiltIn {
    name: &'static str,
    label: &'static str,
    description: &'static str,
    /// The text handed to whatever writes a note along this aspect. Absent for
    /// a derived aspect, which needs no instruction because no model reads one.
    instruction: Option<&'static str>,
    derived: bool,
}

/// The four aspects that ship.
///
/// They are the source of truth for what exists, and there is no second list
/// inside a client that can disagree with it: `GET /v1/api/aspects` serves this
/// and every screen reads that route.
const BUILT_INS: [BuiltIn; 4] = [
    BuiltIn {
        name: "goal",
        label: "Goal",
        description: "What the user wanted.",
        instruction: Some(
            "In one sentence, say what the user was trying to achieve in this run. Write \
             about the request and not about how it went. Use the user's own words where \
             they are clear. Do not include names, addresses, card numbers or any other \
             detail that identifies a person.",
        ),
        derived: false,
    },
    BuiltIn {
        name: "sentiment",
        label: "Sentiment",
        description: "The tone the user expressed.",
        instruction: Some(
            "In one sentence, say how the user sounded in this run: what tone they used and \
             whether it changed. Describe the tone, not whether the agent deserved it. Do \
             not include any detail that identifies a person.",
        ),
        derived: false,
    },
    BuiltIn {
        name: "behaviour",
        label: "Behaviour",
        description: "What the agent did about it.",
        instruction: Some(
            "In one sentence, say what the agent actually did in this run: the steps it \
             took and the tools it called. Describe what happened, not whether it was \
             right. Do not include any detail that identifies a person.",
        ),
        derived: false,
    },
    BuiltIn {
        name: OUTCOME,
        label: "Outcome",
        description: "How the interaction ended.",
        instruction: None,
        derived: true,
    },
];

/// The bounds a pass runs under in this deployment, and whether one runs.
///
/// `pass_armed` is the answer to the question an operator opens this endpoint
/// to ask. Without it the surface had no way to say that clustering is switched
/// off: every aspect reported `enabled`, every readiness sat at `collecting` or
/// `processing` for ever, and nothing anywhere explained why. It is not the
/// same thing as the 501 the theme routes give, which says the storage backend
/// cannot hold the tables at all.
#[must_use]
pub fn limits(settings: &AspectSettings) -> AspectLimits {
    AspectLimits {
        min_notes_to_cluster: MIN_NOTES_TO_CLUSTER,
        max_notes_per_pass: MAX_NOTES_PER_PASS,
        page_size: PASS_PAGE,
        max_aspects: MAX_ASPECTS,
        pass_armed: settings.enabled,
    }
}

/// Every aspect a project describes traces along, in `sort_order`.
///
/// Version 1 on every row. A definition is stored rather than derived, and this
/// build ships one version of each; editing one mints a new version rather than
/// changing this one, so a note recorded a month ago still names what was
/// actually asked for at the time.
#[must_use]
pub fn catalogue(project: &str, settings: &AspectSettings, at: OffsetDateTime) -> Vec<Aspect> {
    BUILT_INS
        .iter()
        .enumerate()
        .map(|(i, b)| Aspect {
            project: project.to_owned(),
            name: b.name.to_owned(),
            label: b.label.to_owned(),
            description: b.description.to_owned(),
            instruction: b.instruction.map(str::to_owned),
            derived: b.derived,
            built_in: true,
            enabled: settings.allows(b.name),
            sort_order: u32::try_from(i).unwrap_or(0),
            version: 1,
            created_at: at,
        })
        .collect()
}

/// The names of the aspects notes are written along here, in catalogue order.
#[must_use]
pub fn enabled_names(settings: &AspectSettings) -> Vec<String> {
    BUILT_INS.iter().filter(|b| settings.allows(b.name)).map(|b| b.name.to_owned()).collect()
}

/// Whether a name is one this deployment knows.
#[must_use]
pub fn is_known(name: &str) -> bool {
    BUILT_INS.iter().any(|b| b.name == name)
}

/* ---------------------------------------------------------- readiness -- */

/// How far one aspect has got for one entity.
///
/// Computed from stored rows every time it is asked for, so it cannot go stale.
/// The three states look identical on a screen and mean different things:
/// `collecting` is not enough notes yet, `processing` is enough notes and no
/// snapshot yet, `ready` is themes to read. Reading zero notes as `collecting`
/// rather than as anything else is deliberate: nothing has been recorded, which
/// is a wait and not a fault.
#[must_use]
pub const fn readiness(note_count: u64, covered_by_a_snapshot: bool) -> AspectReadiness {
    if covered_by_a_snapshot {
        AspectReadiness::Ready
    } else if note_count >= MIN_NOTES_TO_CLUSTER {
        AspectReadiness::Processing
    } else {
        AspectReadiness::Collecting
    }
}

/// How many more notes an aspect needs before a pass will run.
#[must_use]
pub const fn notes_needed(note_count: u64) -> u64 {
    MIN_NOTES_TO_CLUSTER.saturating_sub(note_count)
}

/// The state to report for a whole entity, over its enabled aspects.
///
/// The least advanced of them, and `collecting` when there are none. That empty
/// case is not a corner: a deployment can switch every aspect off, and folding
/// an empty run from `ready` would report an entity as ready with nothing ready
/// in it, which is the one answer this surface must never give.
#[must_use]
pub fn overall(states: impl IntoIterator<Item = AspectReadiness>) -> AspectReadiness {
    states
        .into_iter()
        .fold(None, |acc: Option<AspectReadiness>, s| Some(acc.map_or(s, |have| least(have, s))))
        .unwrap_or(AspectReadiness::Collecting)
}

/// The least advanced of two states.
///
/// An entity's overall readiness is the least advanced of its enabled aspects,
/// so a row says `processing` while any one of them is still short of a
/// snapshot. It never reports the best case, because a reader who opens a row
/// that says `ready` and finds three empty columns has been told something
/// untrue.
#[must_use]
pub fn least(a: AspectReadiness, b: AspectReadiness) -> AspectReadiness {
    const fn rank(r: AspectReadiness) -> u8 {
        match r {
            AspectReadiness::Collecting => 0,
            AspectReadiness::Processing => 1,
            AspectReadiness::Ready => 2,
        }
    }
    if rank(a) <= rank(b) {
        a
    } else {
        b
    }
}

/* ---------------------------------------------------------- clustering -- */

/// One note as the clustering sees it.
///
/// The text is not here. A cluster is decided on the sketch alone, and the
/// label is chosen by note id, so the pass never holds a project's prose in
/// memory to make a partition out of it.
#[derive(Debug, Clone)]
pub struct Candidate {
    /// Which note.
    pub note_id: String,
    /// The trace it was written about.
    pub trace_id: String,
    /// The sketch of its wording.
    pub sketch: Sketch,
    /// When that run began.
    pub started_at: OffsetDateTime,
}

/// What one clustering produced.
#[derive(Debug, Clone, Default)]
pub struct Grouped {
    /// One entry per theme: the indexes into the input, largest first.
    pub clusters: Vec<Vec<usize>>,
    /// The index of each theme's label note, in step with `clusters`.
    pub labels: Vec<usize>,
    /// The indexes that joined no theme.
    pub noise: Vec<usize>,
}

/// Leader clustering behind a banded prefilter.
///
/// The shape is: walk the notes in a fixed order; for each one, look only at
/// leaders that share a whole band of their sketch; join the most alike of
/// those if it clears [`MIN_SHARED`]; otherwise become a leader. Comparing
/// every note against every other would be two hundred million comparisons at
/// the twenty thousand note ceiling, which is what the bands exist to avoid.
///
/// Every ordering states its tie-break, because the answer has to be the same
/// on two replicas reading the same notes:
///
/// * notes are walked by **note id ascending**, so the leader set does not
///   depend on what order a page came back in;
/// * on an equal similarity the **earlier leader** wins, which is the leader
///   with the smaller note id, because that is the order they were made in;
/// * clusters are returned **largest first, then by the label's note id
///   ascending**, so two clusters of the same size come back the same way
///   round every time;
/// * the label is the **medoid**: the member most like the rest of them,
///   breaking a tie on the note id ascending. It is a real sentence a model
///   wrote about a real trace and never a summary invented for the screen.
#[must_use]
pub fn cluster(items: &[Candidate]) -> Grouped {
    // The walk order. Sorting indexes rather than the notes themselves keeps
    // every index in this function pointing into the caller's slice.
    let mut order: Vec<usize> = (0..items.len()).collect();
    order.sort_by(|a, b| items[*a].note_id.cmp(&items[*b].note_id));

    // Band key to the leaders that carry it. `BTreeMap` so a walk of it is in
    // one order on every machine, which matters because the candidate list a
    // note is scored against is built from it.
    let mut bands: BTreeMap<(usize, u64), Vec<usize>> = BTreeMap::new();
    let mut leaders: Vec<usize> = Vec::new();
    let mut members: Vec<Vec<usize>> = Vec::new();

    for &i in &order {
        let sketch = &items[i].sketch;
        if sketch.is_empty() {
            // A note whose text has no words in it is like nothing, itself
            // included. It cannot pull a theme together and it cannot join
            // one, so it goes straight to noise below.
            continue;
        }

        // Candidates, deduplicated, in leader order. A leader can share more
        // than one band with this note and must not be scored twice.
        let mut seen: BTreeSet<usize> = BTreeSet::new();
        for band in 0..BANDS {
            if let Some(key) = sketch.band(band) {
                if let Some(here) = bands.get(&(band, key)) {
                    seen.extend(here.iter().copied());
                }
            }
        }

        let mut best: Option<(u32, usize)> = None;
        for slot in seen {
            let shared = sketch.shared(&items[leaders[slot]].sketch);
            if shared < MIN_SHARED {
                continue;
            }
            // Strictly greater, so an equal similarity leaves the earlier
            // leader in place. `seen` is ascending and a leader's slot rises
            // with its note id, so "earlier" is exactly the smaller note id.
            match best {
                Some((have, _)) if shared <= have => {}
                _ => best = Some((shared, slot)),
            }
        }

        match best {
            Some((_, slot)) => members[slot].push(i),
            None => {
                let slot = leaders.len();
                leaders.push(i);
                members.push(vec![i]);
                for band in 0..BANDS {
                    if let Some(key) = sketch.band(band) {
                        bands.entry((band, key)).or_default().push(slot);
                    }
                }
            }
        }
    }

    let mut out = Grouped::default();
    // Notes with an empty sketch never entered the walk above.
    for &i in &order {
        if items[i].sketch.is_empty() {
            out.noise.push(i);
        }
    }

    let mut kept: Vec<(Vec<usize>, usize)> = Vec::new();
    for group in members {
        if group.len() < MIN_THEME_NOTES {
            out.noise.extend(group);
            continue;
        }
        let label = medoid(items, &group);
        kept.push((group, label));
    }

    // Largest first, then by the label's note id ascending. Both halves are
    // needed: size alone leaves two clusters of equal size in whatever order
    // the walk happened to make them.
    kept.sort_by(|a, b| {
        b.0.len().cmp(&a.0.len()).then_with(|| items[a.1].note_id.cmp(&items[b.1].note_id))
    });
    for (group, label) in kept {
        out.clusters.push(group);
        out.labels.push(label);
    }
    out.noise.sort_by(|a, b| items[*a].note_id.cmp(&items[*b].note_id));
    out
}

/// The member most like the rest of the group.
///
/// Summed similarity, highest wins, tie broken on the note id ascending. It is
/// O(n²) in the size of one cluster, which is bounded by the pass ceiling and
/// is the only place this pass is quadratic.
fn medoid(items: &[Candidate], group: &[usize]) -> usize {
    let mut best = (0u64, &items[group[0]].note_id, group[0]);
    for &i in group {
        let total: u64 =
            group.iter().map(|&j| u64::from(items[i].sketch.shared(&items[j].sketch))).sum();
        let id = &items[i].note_id;
        if total > best.0 || (total == best.0 && id < best.1) {
            best = (total, id, i);
        }
    }
    best.2
}

/* ------------------------------------------------------------- the pass -- */

/// Run the analysis on a timer for the life of the process.
///
/// Returns before spawning anything when nothing is armed, which is the whole
/// of the "an unconfigured daemon costs nothing" rule: no timer, no connection
/// taken, no table written. Three things can leave it unarmed, and all three
/// are checked here rather than on every tick.
pub fn spawn(state: Arc<AppState>) {
    let settings = &state.config.aspects;
    if !settings.enabled {
        return;
    }
    if !state.store.capabilities().theme_analysis {
        // The backend does not hold the tables. Saying so once at start is
        // worth a line: the nine read routes will refuse with 501 naming the
        // same capability, and an operator who switched the analysis on
        // deserves to know why nothing is filling.
        tracing::warn!(
            backend = state.store.name(),
            "aspect analysis is switched on and this storage backend does not hold the note \
             and theme tables, so nothing will be clustered"
        );
        return;
    }
    // `allows` reads the master switch too, so this is the same question a
    // second time. It stays because an aspect deny list that names every
    // built-in leaves nothing to cluster, and that is not the same mistake as
    // leaving the feature switched off.
    let names = enabled_names(settings);
    if names.is_empty() {
        return;
    }

    let every = std::time::Duration::from_secs(settings.interval_seconds.max(MIN_INTERVAL_SECONDS));
    tracing::info!(
        aspects = names.len(),
        interval_seconds = every.as_secs(),
        "clustering aspect notes"
    );

    // One interval before the first pass, so a pass never clusters whatever
    // happened to be on disk at the moment of a restart before the daemon has
    // taken a single request.
    let looped = Arc::clone(&state);
    crate::spawn_loop(
        &state,
        "aspect analysis",
        every,
        crate::FirstPass::AfterInterval,
        move || {
            let state = Arc::clone(&looped);
            async move {
                if let Err(e) = run_once(&state, OffsetDateTime::now_utc()).await {
                    // Logged and dropped. A pass is a lookup and a write of derived
                    // rows: failing one costs the next snapshot and nothing else,
                    // and a pass that took the process down with it would be a
                    // worse thing than a themes screen that is one window stale.
                    // A panic in the pass gets the same answer from `spawn_loop`.
                    tracing::warn!(error = %e, "an aspect analysis pass could not finish");
                }
            }
        },
    );
}

/// One pass over every project this daemon stores.
///
/// Public so a caller can force one rather than waiting for the timer, which is
/// what the tests do: a pass on a five minute interval is not something a test
/// can wait for, and a test that reached inside the module would be testing a
/// second copy of the pass rather than the one that runs.
///
/// It does not consult `enabled`. That is [`spawn`]'s decision and it is taken
/// once, at start: this function is the pass itself, and a caller that has
/// asked for one has already decided.
///
/// # Errors
/// Returns a storage error if a read or the snapshot write fails.
pub async fn run_once(state: &Arc<AppState>, now: OffsetDateTime) -> Result<(), mcpd_store::Error> {
    let settings = &state.config.aspects;
    let window = Duration::seconds(i64::try_from(settings.window_seconds).unwrap_or(86_400));
    // Saturating, as a second line behind `AspectSettings::check`: a caller
    // that builds settings without start-up must not panic the pass.
    let from = now.checked_sub(window).unwrap_or(OffsetDateTime::UNIX_EPOCH);

    for project in state.store.list_projects().await? {
        // The derived aspect first, so the notes it writes are inside the
        // window the clustering below reads.
        if settings.allows(OUTCOME) {
            derive_outcomes(state, &project, from, now).await?;
        }

        let rollup = state
            .store
            .aspect_note_rollup(&mcpd_store::AspectRollupQuery {
                project: project.clone(),
                entity_kind: None,
                entity: None,
                max_groups: u32::try_from(ENTITIES_PER_PASS * BUILT_INS.len()).unwrap_or(u32::MAX),
            })
            .await?;

        // Group the roll-up by entity, keeping only the aspects that have
        // reached the floor. An entity with nothing over the floor is skipped
        // without a read: that is what stops a project of a thousand quiet
        // agents costing a thousand scans per tick.
        // Keyed by the entity's name and the wire spelling of its kind, and not
        // by the kind itself, because a `BTreeMap` needs an order and an enum
        // of eight names has no meaningful one. The spelling is the order the
        // catalogue and every response already use.
        let mut ready: BTreeMap<(String, String), (EntityKind, Vec<String>)> = BTreeMap::new();
        for row in rollup {
            if !settings.allows(&row.aspect) || row.note_count < MIN_NOTES_TO_CLUSTER {
                continue;
            }
            ready
                .entry((row.entity_kind.as_str().to_owned(), row.entity_name))
                .or_insert_with(|| (row.entity_kind, Vec::new()))
                .1
                .push(row.aspect);
        }

        for ((_, name), (kind, mut aspects)) in ready.into_iter().take(ENTITIES_PER_PASS) {
            // Catalogue order, so a snapshot's `aspects` list reads the way the
            // catalogue does rather than the way a group-by happened to return.
            aspects
                .sort_by_key(|a| BUILT_INS.iter().position(|b| b.name == a).unwrap_or(usize::MAX));
            pass_for_entity(state, &project, kind, &name, &aspects, from, now).await?;
        }
    }
    Ok(())
}

/// One clustering pass over one entity.
#[allow(clippy::too_many_arguments)]
async fn pass_for_entity(
    state: &Arc<AppState>,
    project: &str,
    kind: EntityKind,
    name: &str,
    aspects: &[String],
    from: OffsetDateTime,
    now: OffsetDateTime,
) -> Result<(), mcpd_store::Error> {
    // What the previous snapshots held, read once and shared by every aspect.
    // Newest first, bounded by the resurrection lookback, because that is the
    // furthest back a theme id can be reused from.
    let previous = state
        .store
        .list_theme_snapshots(&SnapshotQuery {
            project: project.to_owned(),
            entity: Some((kind, name.to_owned())),
            order: Order::Desc,
            limit: u32::try_from(RESURRECTION_LOOKBACK).unwrap_or(8),
            ..SnapshotQuery::default()
        })
        .await?;

    let snapshot_id = snapshot_id(project, kind, name, now);
    let mut themes: Vec<StoredTheme> = Vec::new();
    let mut members: Vec<ThemeMember> = Vec::new();
    let mut traces: BTreeSet<String> = BTreeSet::new();
    let mut covered: Vec<String> = Vec::new();
    let mut notes_read: u64 = 0;
    let mut capped = false;

    for aspect in aspects {
        let (items, hit_cap) = read_notes(state, project, kind, name, aspect, from, now).await?;
        capped |= hit_cap;
        if (items.len() as u64) < MIN_NOTES_TO_CLUSTER {
            // The roll-up said this aspect had enough. It counted every note
            // for the entity and this read is bounded by the window, so a
            // difference here means the notes are older than the window. That
            // is not a failure: the aspect is still collecting inside the
            // window a snapshot describes.
            continue;
        }
        notes_read += items.len() as u64;
        for it in &items {
            traces.insert(it.trace_id.clone());
        }
        covered.push(aspect.clone());

        let grouped = cluster(&items);
        let denominator = items.len() as f64;
        let carried = carry_ids(
            state,
            project,
            kind,
            name,
            aspect,
            &previous,
            &grouped,
            &items,
            &snapshot_id,
            from,
            now,
        )
        .await?;

        for (slot, group) in grouped.clusters.iter().enumerate() {
            let label = &items[grouped.labels[slot]];
            let id = carried.ids[slot].clone();
            let distinct: BTreeSet<&str> =
                group.iter().map(|&i| items[i].trace_id.as_str()).collect();
            themes.push(StoredTheme {
                theme: Theme {
                    theme_id: id.clone(),
                    snapshot_id: snapshot_id.clone(),
                    aspect: aspect.clone(),
                    entity_kind: kind,
                    entity_name: name.to_owned(),
                    label: String::new(),
                    label_note_id: label.note_id.clone(),
                    label_trace_id: label.trace_id.clone(),
                    state: carried.states[slot],
                    note_count: group.len() as u64,
                    trace_count: distinct.len() as u64,
                    // The one place a float is produced, and it is produced
                    // after every decision has been taken. Coverage is a share
                    // for a reader; nothing in the clustering reads it.
                    coverage: group.len() as f64 / denominator,
                },
                project: project.to_owned(),
                cutoff_at: now,
                window_from: from,
                window_to: now,
                lineage: carried.lineage[slot].clone(),
            });
            for &i in group {
                members.push(ThemeMember {
                    snapshot_id: snapshot_id.clone(),
                    project: project.to_owned(),
                    aspect: aspect.clone(),
                    theme_id: id.clone(),
                    note_id: items[i].note_id.clone(),
                    trace_id: items[i].trace_id.clone(),
                    similarity: items[i].sketch.shared(&label.sketch),
                    trace_started_at: items[i].started_at,
                });
            }
        }

        for &i in &grouped.noise {
            members.push(ThemeMember {
                snapshot_id: snapshot_id.clone(),
                project: project.to_owned(),
                aspect: aspect.clone(),
                theme_id: NOISE_THEME_ID.to_owned(),
                note_id: items[i].note_id.clone(),
                trace_id: items[i].trace_id.clone(),
                // Noise is like nothing in particular, so there is nothing for
                // a similarity to be measured against.
                similarity: 0,
                trace_started_at: items[i].started_at,
            });
        }

        themes.extend(carried.tombstones);
    }

    if covered.is_empty() {
        return Ok(());
    }

    // The labels are filled in last, from the notes the pass has already read,
    // so a label is always the text of a note this snapshot holds.
    let texts = label_texts(state, project, &themes).await?;
    for t in &mut themes {
        if let Some(text) = texts.get(&t.theme.label_note_id) {
            t.theme.label.clone_from(text);
        }
    }

    let ordinal = previous.total.unwrap_or(0) + 1;
    state
        .store
        .write_theme_snapshot(&SnapshotWrite {
            snapshot: ThemeSnapshot {
                snapshot_id,
                project: project.to_owned(),
                entity_kind: kind,
                entity_name: name.to_owned(),
                aspects: covered,
                // The position among the snapshots stored when this pass ran.
                // Retention removing the oldest leaves the numbering starting
                // above one rather than renumbering what is left, because
                // rewriting every stored row to keep a display number tidy is
                // a worse trade than a series that starts at four.
                ordinal,
                cutoff_at: now,
                window_from: from,
                window_to: now,
                trace_count: traces.len() as u64,
                note_count: notes_read,
                capped,
            },
            themes,
            members,
        })
        .await
}

/// Read one aspect's notes for one entity, newest first, to the cap.
///
/// Returns the notes and whether the cap was reached. Paged at [`PASS_PAGE`],
/// so what the pass holds is bounded by the cap and not by the window.
async fn read_notes(
    state: &Arc<AppState>,
    project: &str,
    kind: EntityKind,
    name: &str,
    aspect: &str,
    from: OffsetDateTime,
    to: OffsetDateTime,
) -> Result<(Vec<Candidate>, bool), mcpd_store::Error> {
    let mut items: Vec<Candidate> = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let page = state
            .store
            .list_aspect_notes(&AspectNoteQuery {
                project: project.to_owned(),
                aspect: Some(aspect.to_owned()),
                entity: Some((kind, name.to_owned())),
                from: Some(from),
                to: Some(to),
                order: Order::Desc,
                limit: PASS_PAGE,
                cursor,
                ..AspectNoteQuery::default()
            })
            .await?;
        for n in page.items {
            items.push(Candidate {
                // A stored sketch when there is one, and the text again when
                // there is not. The two are the same function of the same
                // string, so a row written before the column existed clusters
                // exactly as one written after it.
                sketch: n.sketch.unwrap_or_else(|| Sketch::of(&n.note.text)),
                started_at: n.trace_started_at.unwrap_or(n.note.created_at),
                note_id: n.note.note_id,
                trace_id: n.note.trace_id,
            });
            if items.len() as u64 >= MAX_NOTES_PER_PASS {
                return Ok((items, true));
            }
        }
        match page.next_cursor {
            Some(c) => cursor = Some(c),
            None => return Ok((items, false)),
        }
    }
}

/// The text of every theme's label note, read once.
async fn label_texts(
    state: &Arc<AppState>,
    project: &str,
    themes: &[StoredTheme],
) -> Result<BTreeMap<String, String>, mcpd_store::Error> {
    let ids: Vec<String> = themes.iter().map(|t| t.theme.label_note_id.clone()).collect();
    let mut out = BTreeMap::new();
    for chunk in ids.chunks(PASS_PAGE as usize) {
        let page = state
            .store
            .list_aspect_notes(&AspectNoteQuery {
                project: project.to_owned(),
                note_ids: chunk.to_vec(),
                limit: PASS_PAGE,
                ..AspectNoteQuery::default()
            })
            .await?;
        for n in page.items {
            out.insert(n.note.note_id, n.note.text);
        }
    }
    Ok(out)
}

/// What the previous snapshot decides about this one's theme ids.
struct Carried {
    /// The id each cluster keeps or is given, in step with `clusters`.
    ids: Vec<String>,
    /// What happened to it between the previous snapshot and this one.
    states: Vec<ThemeState>,
    /// Its lineage at this point, in both directions.
    lineage: Vec<ThemeLineage>,
    /// Rows for the themes that ended here.
    ///
    /// A death has to be a row or a history could never report one, and it
    /// carries no notes, so every read but a history excludes it.
    tombstones: Vec<StoredTheme>,
}

/// Match this pass's clusters against the previous snapshot's themes.
///
/// A theme keeps its id while its wording holds, which is what makes a history
/// possible. The match is on the label sketches, by the same rule and the same
/// threshold the clustering itself uses, so a theme and a cluster that would
/// have been one theme are one theme.
#[allow(clippy::too_many_arguments)]
async fn carry_ids(
    state: &Arc<AppState>,
    project: &str,
    kind: EntityKind,
    name: &str,
    aspect: &str,
    previous: &mcpd_store::Paged<ThemeSnapshot>,
    grouped: &Grouped,
    items: &[Candidate],
    snapshot_id: &str,
    from: OffsetDateTime,
    now: OffsetDateTime,
) -> Result<Carried, mcpd_store::Error> {
    let n = grouped.clusters.len();
    let mut carried = Carried {
        ids: vec![String::new(); n],
        states: vec![ThemeState::Birth; n],
        lineage: vec![ThemeLineage::default(); n],
        tombstones: Vec::new(),
    };

    // The previous snapshots, newest first. The first is the one a continue, a
    // split, a merge or a death is decided against; the rest are only there so
    // a theme that came back can find its own id again.
    let mut older: Vec<(usize, Vec<StoredTheme>)> = Vec::new();
    for (depth, snap) in previous.items.iter().enumerate() {
        let page = state
            .store
            .list_themes(&ThemeQuery {
                project: project.to_owned(),
                snapshot_id: Some(snap.snapshot_id.clone()),
                entity: Some((kind, name.to_owned())),
                aspect: Some(aspect.to_owned()),
                limit: 1000,
                ..ThemeQuery::default()
            })
            .await?;
        older.push((depth, page.items));
    }

    let sketch_of = |t: &StoredTheme| Sketch::of(&t.theme.label);
    let immediate: &[StoredTheme] = older.first().map_or(&[], |(_, v)| v.as_slice());

    // Each cluster's best match in the snapshot immediately before this one.
    // Strictly greater, so an equal similarity leaves the earlier theme, which
    // is the one with the smaller id, holding the match.
    let mut best_for: Vec<Option<usize>> = vec![None; n];
    for slot in 0..n {
        let here = items[grouped.labels[slot]].sketch;
        let mut best: Option<(u32, usize)> = None;
        for (j, t) in immediate.iter().enumerate() {
            if t.theme.state == ThemeState::Death {
                continue;
            }
            let shared = here.shared(&sketch_of(t));
            if shared < MIN_SHARED {
                continue;
            }
            let better = match best {
                None => true,
                Some((have, at)) => {
                    shared > have
                        || (shared == have && t.theme.theme_id < immediate[at].theme.theme_id)
                }
            };
            if better {
                best = Some((shared, j));
            }
        }
        best_for[slot] = best.map(|(_, j)| j);
    }

    // A previous theme matched by more than one cluster split; a cluster
    // matching more than one previous theme merged. Both are read off the same
    // map.
    let mut claims: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (slot, m) in best_for.iter().enumerate() {
        if let Some(j) = m {
            claims.entry(*j).or_default().push(slot);
        }
    }

    let mut taken: BTreeSet<String> = BTreeSet::new();
    for (j, mut slots) in claims {
        let prev_id = immediate[j].theme.theme_id.clone();
        // The largest cluster keeps the id, tie broken on the label's note id
        // ascending, so the same split resolves the same way on two replicas.
        slots.sort_by(|a, b| {
            grouped.clusters[*b].len().cmp(&grouped.clusters[*a].len()).then_with(|| {
                items[grouped.labels[*a]].note_id.cmp(&items[grouped.labels[*b]].note_id)
            })
        });
        let keeper = slots[0];
        carried.ids[keeper] = prev_id.clone();
        taken.insert(prev_id.clone());

        if slots.len() == 1 {
            carried.states[keeper] = ThemeState::Continue;
        } else {
            // One theme where this snapshot holds several.
            carried.states[keeper] = ThemeState::Split;
            carried.lineage[keeper].split_from = vec![prev_id.clone()];
            for &slot in &slots[1..] {
                let id =
                    new_theme_id(project, kind, name, aspect, &items[grouped.labels[slot]].note_id);
                carried.ids[slot] = id.clone();
                carried.states[slot] = ThemeState::Split;
                carried.lineage[slot].split_from = vec![prev_id.clone()];
                carried.lineage[keeper].split_into.push(id);
            }
        }
    }

    // A cluster that matched nothing immediately before it. It is a
    // resurrection when an earlier snapshot held its wording, and a birth
    // otherwise.
    for slot in 0..n {
        if !carried.ids[slot].is_empty() {
            continue;
        }
        let here = items[grouped.labels[slot]].sketch;
        let mut found: Option<String> = None;
        for (depth, themes) in &older {
            if *depth == 0 {
                continue;
            }
            for t in themes {
                if t.theme.state == ThemeState::Death || taken.contains(&t.theme.theme_id) {
                    continue;
                }
                if here.shared(&sketch_of(t)) >= MIN_SHARED {
                    found = Some(t.theme.theme_id.clone());
                    break;
                }
            }
            if found.is_some() {
                break;
            }
        }
        match found {
            Some(id) => {
                taken.insert(id.clone());
                carried.ids[slot] = id;
                carried.states[slot] = ThemeState::Resurrection;
            }
            None => {
                carried.ids[slot] =
                    new_theme_id(project, kind, name, aspect, &items[grouped.labels[slot]].note_id);
                carried.states[slot] = ThemeState::Birth;
            }
        }
    }

    // Several previous themes matched by one cluster is a merge, read off the
    // ids that cluster took and the ones it did not.
    let mut merged_by: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for slot in 0..n {
        let mine = &carried.ids[slot];
        let here = items[grouped.labels[slot]].sketch;
        let absorbed: Vec<String> = immediate
            .iter()
            .filter(|t| {
                t.theme.state != ThemeState::Death
                    && &t.theme.theme_id != mine
                    && !taken.contains(&t.theme.theme_id)
                    && here.shared(&sketch_of(t)) >= MIN_SHARED
            })
            .map(|t| t.theme.theme_id.clone())
            .collect();
        if !absorbed.is_empty() {
            merged_by.insert(mine.clone(), absorbed);
        }
    }
    for (slot, id) in carried.ids.iter().enumerate() {
        if let Some(absorbed) = merged_by.get(id) {
            carried.states[slot] = ThemeState::Merge;
            carried.lineage[slot].merged_from.clone_from(absorbed);
        }
    }

    // Everything the previous snapshot held that nothing here matched. It gets
    // a tombstone row: no notes, no coverage, and `merged_into` naming the
    // theme it became when it was absorbed rather than simply gone.
    let absorbed_into: BTreeMap<&str, &str> = merged_by
        .iter()
        .flat_map(|(into, gone)| gone.iter().map(move |f| (f.as_str(), into.as_str())))
        .collect();
    for t in immediate {
        if t.theme.state == ThemeState::Death || taken.contains(&t.theme.theme_id) {
            continue;
        }
        // A theme absorbed by two clusters at once names the later of them by
        // id, because `merged_by` is walked in id order. Arbitrary, and the
        // same arbitrary answer on every replica, which is the property that
        // matters: the alternative is a lineage that differs between two nodes
        // reading the same notes.
        let into = absorbed_into.get(t.theme.theme_id.as_str()).map(|s| (*s).to_owned());
        // The tombstone belongs to THIS snapshot and carries this snapshot's
        // times, because the death happened here. Copying the previous
        // snapshot's id and cutoff would file the point under the window the
        // theme was last alive in, so a history would show it dying before it
        // was last seen.
        carried.tombstones.push(StoredTheme {
            theme: Theme {
                snapshot_id: snapshot_id.to_owned(),
                state: ThemeState::Death,
                note_count: 0,
                trace_count: 0,
                coverage: 0.0,
                ..t.theme.clone()
            },
            project: project.to_owned(),
            cutoff_at: now,
            window_from: from,
            window_to: now,
            lineage: ThemeLineage {
                merged_into: into.into_iter().collect(),
                ..ThemeLineage::default()
            },
        });
    }

    Ok(carried)
}

/// A snapshot's id: derived, not allocated.
///
/// Two replicas running a pass for the same entity in the same second write the
/// same id, so one overwrites the other's rows instead of doubling the series.
/// Nothing is encoded in the shape of it and no caller may read one.
fn snapshot_id(project: &str, kind: EntityKind, name: &str, at: OffsetDateTime) -> String {
    let mut h = Sha256::new();
    h.update(b"mcpd-theme-snapshot");
    for part in [project, kind.as_str(), name, &at.unix_timestamp().to_string()] {
        // Length-prefixed, so two entities whose fields concatenate to the same
        // bytes cannot hash the same and silently replace each other.
        h.update((part.len() as u64).to_le_bytes());
        h.update(part.as_bytes());
    }
    format!("snap-{}", hex::encode(&h.finalize()[..12]))
}

/// A new theme's id, derived from the note it was labelled by.
///
/// Derived rather than random for the same reason: two replicas clustering the
/// same notes have to produce the same ids, or a history would fork.
fn new_theme_id(
    project: &str,
    kind: EntityKind,
    name: &str,
    aspect: &str,
    label_note: &str,
) -> String {
    let mut h = Sha256::new();
    h.update(b"mcpd-theme");
    for part in [project, kind.as_str(), name, aspect, label_note] {
        h.update((part.len() as u64).to_le_bytes());
        h.update(part.as_bytes());
    }
    format!("theme-{}", hex::encode(&h.finalize()[..12]))
}

/* ------------------------------------------------------ derived outcome -- */

/// Write the `outcome` notes for the runs in one window.
///
/// **No model anywhere.** Every sentence here is chosen from a closed list by
/// facts the daemon already holds: the run's status, the error class of the
/// step that failed, the silent producer flag the cause report computes, and
/// the structural divergence watch. That is what makes the pipeline provable
/// before anyone wires a model to anything.
///
/// The note id is derived from the trace id, so a second pass over the same
/// window rewrites the note it wrote before rather than adding another beside
/// it. That is what lets this run on a timer with no record of what it has
/// already done.
async fn derive_outcomes(
    state: &Arc<AppState>,
    project: &str,
    from: OffsetDateTime,
    now: OffsetDateTime,
) -> Result<(), mcpd_store::Error> {
    let traces = state
        .store
        .list_traces(&mcpd_store::TraceQuery {
            project: project.to_owned(),
            from: Some(from),
            to: Some(now),
            limit: PASS_PAGE,
            ..mcpd_store::TraceQuery::default()
        })
        .await?;

    // The runs in this window that strayed from the shape their own definition
    // version usually takes, read once for the whole window rather than once
    // per trace. Empty when this daemon serves no durable runs, which is the
    // ordinary case and costs nothing.
    let diverged = crate::monitor::diverged_in_window(state, project, from).await;

    let mut notes = Vec::new();
    for trace in &traces.items {
        let Some(text) = outcome_sentence(state, trace, &diverged).await else { continue };
        // Stamped when the run ended, which is when its outcome became a fact,
        // and not at `now`. The clustering below reads notes recorded before
        // `now`, so a note stamped `now` sat outside its own pass, and the next
        // pass rewrote it to that pass's `now`: the outcome aspect never reached
        // a snapshot. The end time also makes a rewrite byte-for-byte the same.
        let recorded = trace.end_time.map_or(now, |end| end.min(now - Duration::microseconds(1)));
        notes.push(mcpd_core::AspectNote {
            note_id: derived_note_id(project, &trace.trace_id),
            trace_id: trace.trace_id.clone(),
            aspect: OUTCOME.to_owned(),
            text,
            definition_version: 1,
            // Absent, and it is the field that says so: no model wrote this.
            model: None,
            created_at: recorded,
            correlation: trace.correlation.clone(),
        });
    }
    if notes.is_empty() {
        return Ok(());
    }
    state
        .store
        .ingest(mcpd_store::Batch { aspect_notes: notes, ..mcpd_store::Batch::default() })
        .await
        .map(|_| ())
}

/// The sentence for one run, or `None` while it is still going.
///
/// A run that has not ended has no outcome to describe, and writing one would
/// put a sentence on a screen that the next pass contradicts.
async fn outcome_sentence(
    state: &Arc<AppState>,
    trace: &mcpd_core::Trace,
    diverged: &BTreeSet<String>,
) -> Option<String> {
    // A run that has not ended has no outcome to describe.
    trace.end_time?;

    // The durable run behind this trace, when there is one. It knows things the
    // trace projection does not: which step failed, what class of error it was,
    // and whether a step it read from succeeded quietly and produced nothing.
    if let (Some(runs), Some(run_id)) = (state.runs.as_ref(), trace.correlation.run_id.as_deref()) {
        if let Ok(run) = runs.get_run(run_id).await {
            // The divergence watch first. It is the one reading here that no
            // single run can produce on its own: a step count is only unusual
            // against the recent runs of the same definition version, which is
            // why this comes from the same statistic `monitor.rs` publishes
            // rather than from a second copy of it.
            if diverged.contains(run_id) {
                return Some(
                    "the run finished and took an unusual number of steps for its definition"
                        .to_owned(),
                );
            }
            return match run.status {
                mcpd_runs::RunStatus::Running => None,
                mcpd_runs::RunStatus::Failed => Some(match failed_step_class(runs, run_id).await {
                    Some(class) => format!("the run failed with a {class} error"),
                    None => "the run failed".to_owned(),
                }),
                mcpd_runs::RunStatus::TimedOut => {
                    Some("the run ran out of time before it finished".to_owned())
                }
                mcpd_runs::RunStatus::Cancelled => {
                    Some("the run was cancelled before it finished".to_owned())
                }
                mcpd_runs::RunStatus::Succeeded => {
                    if silent_producer(runs, run_id).await {
                        Some(
                            "the run reported success and a step it read from produced nothing"
                                .to_owned(),
                        )
                    } else {
                        Some("the run finished with no error".to_owned())
                    }
                }
            };
        }
    }

    // No durable run, which is the ordinary case: the trace projection is what
    // is left, and it carries the status and how many steps errored.
    Some(match (trace.status, trace.error_count) {
        (mcpd_core::SpanStatus::Error, 0 | 1) => "the run ended in an error at one step".to_owned(),
        (mcpd_core::SpanStatus::Error, n) => {
            format!("the run ended in an error after {n} steps errored")
        }
        (_, 0) => "the run finished with no error".to_owned(),
        (_, 1) => "the run finished after one step errored".to_owned(),
        (_, n) => format!("the run finished after {n} steps errored"),
    })
}

/// The error class of the step a failed run stopped at.
///
/// The first failed attempt in the order the store returns them, which is by
/// step key, so two reads of one run name the same step.
async fn failed_step_class(
    runs: &Arc<dyn mcpd_runs::RunStore>,
    run_id: &str,
) -> Option<&'static str> {
    let attempts = runs.attempts(run_id).await.ok()?;
    attempts
        .iter()
        .find(|a| a.status == mcpd_runs::AttemptStatus::Failed)
        .and_then(|a| a.error_class)
        .map(mcpd_runs::StepErrorClass::as_str)
}

/// Whether a step this run read from succeeded and produced nothing.
///
/// The same flag `GET /v1/api/runs/{run_id}/cause` publishes, read through the
/// same method, so this sentence and that report cannot disagree.
///
/// Asked about one step rather than every step, and bounded to a depth of two.
/// A cause walk is a read per ancestor and this runs over a whole window on a
/// timer, so asking it of every step of every run would make the pass cost more
/// than everything else in this module together. The step asked about is the
/// last one the run made an attempt at, which is the one whose ancestors decide
/// what the run produced.
async fn silent_producer(runs: &Arc<dyn mcpd_runs::RunStore>, run_id: &str) -> bool {
    let Ok(attempts) = runs.attempts(run_id).await else { return false };
    let Some(last) = attempts.last() else { return false };
    let Ok(cause) = runs.step_cause(run_id, &last.step_key, 2).await else { return false };
    cause.chain.iter().any(|l| l.silent_producer)
}

/// A derived note's id.
///
/// Derived from the trace and the project, so a pass that reads the same window
/// twice replaces the note it wrote rather than writing a second copy of it.
fn derived_note_id(project: &str, trace_id: &str) -> String {
    let mut h = Sha256::new();
    h.update(b"mcpd-derived-aspect-note");
    for part in [project, trace_id, OUTCOME] {
        h.update((part.len() as u64).to_le_bytes());
        h.update(part.as_bytes());
    }
    format!("note-{}", hex::encode(&h.finalize()[..16]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(id: &str, trace: &str, text: &str) -> Candidate {
        Candidate {
            note_id: id.to_owned(),
            trace_id: trace.to_owned(),
            sketch: Sketch::of(text),
            started_at: OffsetDateTime::UNIX_EPOCH,
        }
    }

    /// Twelve notes about two things, and the two things must come back as two
    /// themes with the odd one out in noise.
    fn two_groups() -> Vec<Candidate> {
        let mut v = Vec::new();
        for i in 0..6 {
            v.push(candidate(
                &format!("n-a{i}"),
                &format!("t-a{i}"),
                "the customer wants a refund for a late delivery",
            ));
        }
        for i in 0..5 {
            v.push(candidate(
                &format!("n-b{i}"),
                &format!("t-b{i}"),
                "the customer is asking where their parcel has got to",
            ));
        }
        v.push(candidate("n-z", "t-z", "please book me a flight to lisbon on tuesday"));
        v
    }

    #[test]
    fn notes_that_share_their_wording_land_in_one_theme() {
        let items = two_groups();
        let g = cluster(&items);
        assert_eq!(g.clusters.len(), 2, "expected two themes, got {:?}", g.clusters.len());
        assert_eq!(g.clusters[0].len(), 6, "the larger theme comes first");
        assert_eq!(g.clusters[1].len(), 5);
        assert_eq!(g.noise.len(), 1, "the sentence nothing matched is noise");
        assert_eq!(items[g.noise[0]].note_id, "n-z");
    }

    #[test]
    fn the_same_notes_in_another_order_produce_the_same_clustering() {
        // The property the whole design rests on: two replicas reading the same
        // notes must write the same themes. The order a page came back in is
        // the one thing that can differ between them.
        let items = two_groups();
        let forward = cluster(&items);
        let mut backward_items = items.clone();
        backward_items.reverse();
        let backward = cluster(&backward_items);

        let ids = |g: &Grouped, src: &[Candidate]| -> Vec<Vec<String>> {
            g.clusters
                .iter()
                .map(|c| {
                    let mut v: Vec<String> = c.iter().map(|&i| src[i].note_id.clone()).collect();
                    v.sort();
                    v
                })
                .collect()
        };
        assert_eq!(ids(&forward, &items), ids(&backward, &backward_items));
        assert_eq!(
            items[forward.labels[0]].note_id, backward_items[backward.labels[0]].note_id,
            "the label must not depend on the order the notes arrived in"
        );
    }

    #[test]
    fn a_theme_label_is_one_of_its_own_notes() {
        let items = two_groups();
        let g = cluster(&items);
        for (slot, group) in g.clusters.iter().enumerate() {
            assert!(
                group.contains(&g.labels[slot]),
                "a label has to be a member, never a sentence written for the screen"
            );
        }
    }

    #[test]
    fn a_group_of_one_is_noise_and_not_a_theme() {
        let items = vec![
            candidate("n-1", "t-1", "cancel my subscription please"),
            candidate("n-2", "t-2", "where is the nearest branch"),
        ];
        let g = cluster(&items);
        assert!(g.clusters.is_empty(), "two unrelated notes are two groups of one, so no theme");
        assert_eq!(g.noise.len(), 2);
    }

    #[test]
    fn a_note_with_no_words_in_it_is_noise() {
        let mut items = two_groups();
        items.push(candidate("n-empty", "t-empty", "!!! ..."));
        let g = cluster(&items);
        let noise: Vec<&str> = g.noise.iter().map(|&i| items[i].note_id.as_str()).collect();
        assert!(noise.contains(&"n-empty"), "got {noise:?}");
    }

    #[test]
    fn every_note_lands_in_exactly_one_place() {
        let items = two_groups();
        let g = cluster(&items);
        let mut seen: BTreeSet<usize> = BTreeSet::new();
        for group in &g.clusters {
            for &i in group {
                assert!(seen.insert(i), "note {i} is in two themes");
            }
        }
        for &i in &g.noise {
            assert!(seen.insert(i), "note {i} is both a member and noise");
        }
        assert_eq!(seen.len(), items.len(), "a stage's nodes have to account for every note");
    }

    #[test]
    fn readiness_separates_the_three_cases() {
        assert_eq!(readiness(0, false), AspectReadiness::Collecting);
        assert_eq!(readiness(MIN_NOTES_TO_CLUSTER - 1, false), AspectReadiness::Collecting);
        assert_eq!(readiness(MIN_NOTES_TO_CLUSTER, false), AspectReadiness::Processing);
        assert_eq!(readiness(MIN_NOTES_TO_CLUSTER, true), AspectReadiness::Ready);
        // The one that matters most: a snapshot exists, so it is ready even
        // though the window has since fallen below the floor.
        assert_eq!(readiness(1, true), AspectReadiness::Ready);
    }

    #[test]
    fn an_entity_with_no_enabled_aspects_is_not_ready() {
        // Folding an empty run from `ready` would report an entity as ready
        // with nothing ready in it.
        assert_eq!(overall(Vec::new()), AspectReadiness::Collecting);
        assert_eq!(overall([AspectReadiness::Ready]), AspectReadiness::Ready);
        assert_eq!(
            overall([AspectReadiness::Ready, AspectReadiness::Processing]),
            AspectReadiness::Processing
        );
    }

    #[test]
    fn an_entity_never_reports_the_best_case() {
        assert_eq!(
            least(AspectReadiness::Ready, AspectReadiness::Collecting),
            AspectReadiness::Collecting
        );
        assert_eq!(
            least(AspectReadiness::Ready, AspectReadiness::Processing),
            AspectReadiness::Processing
        );
        assert_eq!(least(AspectReadiness::Ready, AspectReadiness::Ready), AspectReadiness::Ready);
    }

    #[test]
    fn notes_needed_reaches_zero_at_the_floor_and_stays_there() {
        assert_eq!(notes_needed(0), MIN_NOTES_TO_CLUSTER);
        assert_eq!(notes_needed(MIN_NOTES_TO_CLUSTER), 0);
        assert_eq!(notes_needed(MIN_NOTES_TO_CLUSTER + 500), 0);
    }

    #[test]
    fn the_catalogue_ships_four_aspects_and_one_of_them_is_derived() {
        let at = OffsetDateTime::UNIX_EPOCH;
        let all = catalogue("default", &AspectSettings::default(), at);
        assert_eq!(all.len(), 4);
        assert_eq!(all.iter().filter(|a| a.derived).count(), 1);
        let derived = all.iter().find(|a| a.derived).expect("one derived aspect");
        assert_eq!(derived.name, OUTCOME);
        assert!(
            derived.instruction.is_none(),
            "a derived aspect needs no instruction, because no model reads one"
        );
        for (i, a) in all.iter().enumerate() {
            assert_eq!(a.sort_order as usize, i, "sort order is the catalogue order");
            assert!(a.built_in);
            assert_eq!(a.version, 1);
        }
    }

    #[test]
    fn a_disabled_aspect_stays_in_the_catalogue() {
        // The deny list alone, because that is the only thing that decides
        // this. Arming the pass here would test nothing extra and would suggest
        // the two settings are one.
        let settings =
            AspectSettings { disabled: vec!["sentiment".to_owned()], ..AspectSettings::default() };
        let all = catalogue("default", &settings, OffsetDateTime::UNIX_EPOCH);
        assert_eq!(all.len(), 4, "a name that vanished would make its notes look like a mistake");
        let off = all.iter().find(|a| a.name == "sentiment").expect("still listed");
        assert!(!off.enabled);
        assert_eq!(enabled_names(&settings), vec!["goal", "behaviour", OUTCOME]);
    }

    #[test]
    fn nothing_is_armed_by_default() {
        let s = AspectSettings::default();
        assert!(!s.enabled, "an unconfigured daemon spawns no timer and writes no table");
    }

    #[test]
    fn an_unconfigured_daemon_says_the_pass_is_not_armed() {
        // Found by running the daemon rather than by reading it: the catalogue
        // said `enabled` for all four aspects while `spawn` had already
        // returned without arming a timer, so the one endpoint an operator
        // opens to ask "is this on" answered a different question.
        //
        // The fix is not to report the aspects as off. They are on: a note
        // along one is accepted, stored and handed back whatever the pass is
        // doing. What was missing was anywhere to say that nothing clusters,
        // and that is `pass_armed`.
        let settings = AspectSettings::default();
        assert!(!settings.enabled, "an unconfigured daemon arms nothing");
        assert!(
            !limits(&settings).pass_armed,
            "the catalogue has to be able to say that nothing will ever cluster"
        );

        let all = catalogue("default", &settings, OffsetDateTime::UNIX_EPOCH);
        assert_eq!(all.len(), 4, "the names still ship, so a note along one is not a mistake");
        assert!(
            all.iter().all(|a| a.enabled),
            "an aspect is on unless the deny list says otherwise; the pass being off is a \
             separate fact, reported separately"
        );
        assert_eq!(enabled_names(&settings).len(), 4);

        let armed = AspectSettings { enabled: true, ..AspectSettings::default() };
        assert!(limits(&armed).pass_armed);
    }

    #[test]
    fn a_derived_note_id_is_a_function_of_the_run_it_describes() {
        let a = derived_note_id("default", "trace-1");
        assert_eq!(a, derived_note_id("default", "trace-1"), "a second pass must not double it");
        assert_ne!(a, derived_note_id("other", "trace-1"), "two projects are two notes");
        assert_ne!(a, derived_note_id("default", "trace-2"));
    }

    #[test]
    fn a_snapshot_id_is_a_function_of_the_entity_and_the_second_it_ran_in() {
        let at = OffsetDateTime::UNIX_EPOCH;
        let a = snapshot_id("default", EntityKind::Agent, "support", at);
        assert_eq!(a, snapshot_id("default", EntityKind::Agent, "support", at));
        assert_ne!(a, snapshot_id("default", EntityKind::Workflow, "support", at));
        assert_ne!(
            a,
            snapshot_id("default", EntityKind::Agent, "support", at + Duration::seconds(1))
        );
    }

    #[test]
    fn the_published_bounds_are_the_ones_the_pass_runs_under() {
        let l = limits(&AspectSettings { enabled: true, ..AspectSettings::default() });
        assert!(l.pass_armed, "the published bounds say whether a pass runs at all");
        assert_eq!(l.min_notes_to_cluster, MIN_NOTES_TO_CLUSTER);
        assert_eq!(l.max_notes_per_pass, MAX_NOTES_PER_PASS);
        assert_eq!(l.page_size, PASS_PAGE);
        assert_eq!(l.max_aspects, MAX_ASPECTS);
        assert!(
            (BUILT_INS.len() as u32) <= MAX_ASPECTS,
            "the built-ins have to fit inside the ceiling they publish"
        );
        assert!(MIN_SHARED <= mcpd_core::sketch::SKETCH_LEN as u32);
    }
}
