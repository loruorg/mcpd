//! One conformance suite for every storage backend.
//!
//! mcpd publishes one logical storage trait with three implementations. Until
//! this file existed that claim rested on 30 hand-written SQLite tests, 8
//! Postgres tests and 3 ClickHouse tests, each written separately, and the
//! three backends did not agree. A `group_by=name` breakdown counted runs on
//! SQLite and steps on the other two, so the same request answered with a list
//! of runs or a list of step names depending on where the data happened to
//! live, and nothing in the repository noticed. ClickHouse was changed to
//! agree, because `GroupedStat::count` is specified as a count of runs;
//! Postgres still counts steps and every case that proves it is listed in
//! [`KNOWN_GAPS`] with the function to change.
//!
//! The shape here is deliberate and it is not "compare the backends to each
//! other". Every case names the answer the specification gives, in
//! [`documented_answers`], and each backend is measured against that. Three
//! backends agreeing on a wrong number is the failure this suite exists to
//! catch, and a suite that only asked whether they matched would report it as
//! a pass.
//!
//! ## Running it
//!
//! SQLite needs nothing. Postgres runs when `MCPD_TEST_POSTGRES_URL` names a
//! server and ClickHouse when `MCPD_TEST_CLICKHOUSE_URL` does. A backend with
//! no URL is skipped LOUDLY: the report at the end names it and names every
//! case that therefore proved nothing. With `MCPD_REQUIRE_LIVE` set, a missing
//! URL is a failure rather than a skip, which is the guard every other live
//! suite here carries.
//!
//! ## Known gaps
//!
//! [`KNOWN_GAPS`] records a difference this suite has found and nobody has
//! fixed yet, so the run still reports rather than failing on a bug somebody
//! already knows about. The mechanism is taken from `tests/e2e/run.mjs`, and
//! the part that matters is the audit at the bottom: an entry whose case now
//! passes FAILS the run and names the line to delete. A suppression that
//! outlives its bug is where the next real regression lands unreported.

use std::collections::BTreeMap;

use mcpd_core::{
    Correlation, Cost, EntityKind, LogLevel, LogRecord, MetricKind, MetricPoint, Span, SpanKind,
    SpanStatus, TokenUsage, Trace,
};
use mcpd_store::{
    Batch, ClickHouseConfig, ClickHouseStore, Delta, DeltaQuery, DeltaSignal, Discovered,
    DiscoveryQuery, Facet, Include, LogQuery, PolledSignal, PostgresConfig, PostgresStore,
    RetentionPolicy, SqliteStore, Store, TraceQuery,
};
use serde_json::json;
use time::{Duration, OffsetDateTime};

/// What one backend answered, keyed by what the answer means.
///
/// A string rather than a number so one map can hold a count, a boolean, an
/// absence and a rendered group breakdown, and so a mismatch prints as the two
/// values side by side rather than as a type error.
type Answers = BTreeMap<&'static str, String>;

/// Differences this suite has found and nobody has fixed yet.
///
/// Each entry is `(backend, case, why it is here and what closes it)`. The
/// value is a work item with a file in it, not an excuse.
///
/// Deleting an entry is how a fix is proved. The audit in
/// [`report`] fails the run when an entry's case passes and names the line to
/// delete, so an accepted difference cannot outlive the bug it describes.
const KNOWN_GAPS: &[(&str, &str, &str)] = &[];

/// Every dimension `group_by` accepts, and the table each one describes.
///
/// Nine, not the three that happened to work. Six describe the run and three
/// describe the step, and reading a run dimension off the step table is the
/// divergence this suite was written for.
const DIMENSIONS: [&str; 9] = [
    "name",
    "environment",
    "status",
    "session_id",
    "user_id",
    "service_name",
    "model",
    "provider",
    "entity_name",
];

/* ------------------------------------------------------------- the fixture -- */

/// Five runs, chosen so that a step-scoped answer and a run-scoped answer
/// cannot be mistaken for each other.
///
/// Two runs share the name `checkout` while every step under them has a
/// different name, so grouping by `name` gives four groups over runs and seven
/// over steps. One run holds an event span, one holds a step that never ended,
/// and one is nothing but an event.
struct Fixture {
    base: OffsetDateTime,
    project: String,
}

/// A trace id for one run of one fixture.
///
/// Derived from the project rather than fixed, because a Postgres or
/// ClickHouse server keeps its rows between runs of this file, and a span id is
/// owned by the first project that stored it. A fixed id would make the second
/// run's spans collide with the first run's, which is refused, and every count
/// after that would describe the first run. FNV over the two strings, rendered
/// as the 32 hex characters an id is.
fn trace_id(project: &str, label: &str) -> String {
    let mut h: u128 = 0xcbf2_9ce4_8422_2325;
    for b in project.bytes().chain(label.bytes()) {
        h = (h ^ u128::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:032x}")
}

impl Fixture {
    fn id(&self, label: &str) -> String {
        trace_id(&self.project, label)
    }

    fn window(&self) -> (OffsetDateTime, OffsetDateTime) {
        (self.base - Duration::minutes(1), self.base + Duration::minutes(10))
    }

    fn corr(&self, environment: &str, session: &str, user: &str) -> Correlation {
        Correlation {
            project: self.project.clone(),
            environment: Some(environment.into()),
            service_name: Some("api".into()),
            service_version: None,
            session_id: Some(session.into()),
            thread_id: None,
            user_id: Some(user.into()),
            resource_id: None,
            run_id: None,
            request_id: None,
            experiment_id: None,
            release: None,
        }
    }

    /// One step, from the description of it.
    ///
    /// A struct rather than eight positional arguments: the fixture is read far
    /// more often than it is written, and `(0, Some(500))` at the end of a call
    /// says nothing about which is the offset and which the length.
    fn span(&self, d: Step<'_>) -> Span {
        let start = self.base + Duration::milliseconds(d.at_ms);
        let mut s = Span {
            trace_id: d.trace.into(),
            span_id: d.id.into(),
            parent_span_id: d.parent.map(Into::into),
            external_parent_span_id: None,
            name: d.name.into(),
            kind: d.kind,
            status: SpanStatus::Ok,
            start_time: start,
            end_time: d.len_ms.map(|n| start + Duration::milliseconds(n)),
            duration_ns: None,
            is_root: d.parent.is_none(),
            is_event: false,
            entity_kind: None,
            entity_id: None,
            entity_name: None,
            input: None,
            output: None,
            attributes: None,
            metadata: None,
            usage: None,
            cost: None,
            model_parameters: None,
            error: None,
            links: None,
            tags: None,
            correlation: d.corr,
            model: None,
            provider: None,
            received_at: start,
            ingest_key: None,
        };
        s.normalise();
        s
    }

    fn spans(&self) -> Vec<Span> {
        let (a_id, b_id, c_id, d_id, e_id) =
            (self.id(A), self.id(B), self.id(C), self.id(D), self.id(E));
        let mut out = Vec::new();

        // A: a finished run. Its window is base .. base+500ms.
        let a = self.corr("production", "s1", "u1");
        let mut a_root = self.span(Step {
            trace: &a_id,
            id: "a1",
            parent: None,
            name: "checkout",
            kind: SpanKind::AgentRun,
            at_ms: 0,
            len_ms: Some(500),
            corr: a.clone(),
        });
        a_root.input = Some(json!({ "q": "where is my order" }));
        a_root.output = Some(json!({ "text": "shipped from Leeds on Tuesday via Café" }));
        out.push(a_root);
        let mut a_model = self.span(Step {
            trace: &a_id,
            id: "a2",
            parent: Some("a1"),
            name: "generate",
            kind: SpanKind::ModelGeneration,
            at_ms: 50,
            len_ms: Some(200),
            corr: a.clone(),
        });
        a_model.model = Some("gpt-4o".into());
        a_model.provider = Some("openai".into());
        a_model.usage = Some(tokens(1000, 200));
        a_model.cost = Some(cost(0.006));
        out.push(a_model);
        let mut a_tool = self.span(Step {
            trace: &a_id,
            id: "a3",
            parent: Some("a1"),
            name: "lookup",
            kind: SpanKind::ToolCall,
            at_ms: 300,
            len_ms: Some(20),
            corr: a,
        });
        a_tool.entity_name = Some("order_lookup".into());
        out.push(a_tool);

        // B: a run that failed inside a step the root reported as fine.
        let b = self.corr("production", "s2", "u2");
        out.push(self.span(Step {
            trace: &b_id,
            id: "b1",
            parent: None,
            name: "refund",
            kind: SpanKind::AgentRun,
            at_ms: 0,
            len_ms: Some(300),
            corr: b.clone(),
        }));
        let mut b_model = self.span(Step {
            trace: &b_id,
            id: "b2",
            parent: Some("b1"),
            name: "generate",
            kind: SpanKind::ModelGeneration,
            at_ms: 20,
            len_ms: Some(100),
            corr: b,
        });
        b_model.status = SpanStatus::Error;
        b_model.model = Some("claude-3-5-sonnet".into());
        b_model.provider = Some("anthropic".into());
        b_model.usage = Some(tokens(500, 100));
        b_model.cost = Some(cost(0.002));
        out.push(b_model);

        // C: a finished run carrying an event. An event is a point in time and
        // never has an end, so a backend that counts it as an open span never
        // calls this run complete.
        let c = self.corr("staging", "s3", "u3");
        out.push(self.span(Step {
            trace: &c_id,
            id: "c1",
            parent: None,
            name: "checkout",
            kind: SpanKind::AgentRun,
            at_ms: 0,
            len_ms: Some(400),
            corr: c.clone(),
        }));
        let mut c_event = self.span(Step {
            trace: &c_id,
            id: "c2",
            parent: Some("c1"),
            name: "cache_miss",
            kind: SpanKind::ModelChunk,
            at_ms: 100,
            len_ms: None,
            corr: c,
        });
        c_event.is_event = true;
        c_event.normalise();
        out.push(c_event);

        // D: a run still in flight. Its root has not ended; a step under it has.
        let d = self.corr("production", "s4", "u4");
        out.push(self.span(Step {
            trace: &d_id,
            id: "d1",
            parent: None,
            name: "stalled",
            kind: SpanKind::AgentRun,
            at_ms: 0,
            len_ms: None,
            corr: d.clone(),
        }));
        out.push(self.span(Step {
            trace: &d_id,
            id: "d2",
            parent: Some("d1"),
            name: "lookup",
            kind: SpanKind::ToolCall,
            at_ms: 10,
            len_ms: Some(50),
            corr: d,
        }));

        // E: nothing but an event, which is what a run looks like between the
        // first signal arriving and its root span arriving.
        let mut e_event = self.span(Step {
            trace: &e_id,
            id: "e1",
            parent: None,
            name: "beacon",
            kind: SpanKind::ModelChunk,
            at_ms: 0,
            len_ms: None,
            corr: self.corr("staging", "s5", "u5"),
        });
        e_event.is_event = true;
        e_event.normalise();
        out.push(e_event);

        out
    }
}

/// One step of the fixture. `at_ms` and `len_ms` are relative to the fixture's
/// base, so every duration this suite asserts is exact rather than "about".
struct Step<'a> {
    trace: &'a str,
    id: &'a str,
    parent: Option<&'a str>,
    name: &'a str,
    kind: SpanKind,
    at_ms: i64,
    len_ms: Option<i64>,
    corr: Correlation,
}

/// The five runs, named so a failure reads as a run rather than a hex string.
const A: &str = "A";
const B: &str = "B";
const C: &str = "C";
const D: &str = "D";
const E: &str = "E";

fn tokens(input: u64, output: u64) -> TokenUsage {
    TokenUsage {
        input_tokens: Some(input),
        output_tokens: Some(output),
        total_tokens: None,
        input_details: None,
        output_details: None,
    }
}

fn cost(total: f64) -> Cost {
    Cost {
        currency: "USD".into(),
        input_cost: None,
        output_cost: None,
        total_cost: Some(total),
        price_book: Some("builtin".into()),
        is_estimated: true,
    }
}

/* --------------------------------------------------------------- the cases -- */

/// The answer the specification gives for every case, in one place.
///
/// `spec/mcpd.spec.json` is the source. `GroupedStat.count` is "How many runs
/// fell in this group", `AnalyticsSummary.trace_count` is "Runs started in the
/// window", `Trace.is_complete` is "True once the root span has ended and no
/// span is still open", and `q` is "Full-text search across span names,
/// prompts and completions".
fn documented_answers() -> Answers {
    let mut e = Answers::new();

    // Tenancy. A trace id is shared across a service boundary by design, so
    // two projects can each hold a run under it; every other id is chosen by
    // the sender and belongs to the first project that stored it.
    e.insert("a trace id two projects share is a run in each", "1,1".into());
    e.insert(
        "a note about a shared trace id is filed under its own project's agent",
        "agent-b".into(),
    );
    e.insert(
        "each project's run of a shared trace id holds only its own signals",
        "a: spans=2/2 logs=1 scores=1 feedback=1; b: spans=1/1 logs=1 scores=1 feedback=1".into(),
    );
    e.insert("the score filter does not reach another project's score", "0".into());
    e.insert(
        "deleting a shared trace in one project leaves the other's run",
        "spans=2 logs=1 scores=1 feedback=1".into(),
    );

    // Five runs, ten steps, one failed run.
    e.insert("the summary counts the runs in the window", "5".into());
    e.insert("the summary counts the steps in the window", "10".into());
    e.insert("the summary counts the runs that failed", "1".into());

    // Six dimensions describe the run. Two runs are named `checkout`, and no
    // two steps share a name, so a breakdown counted over steps cannot produce
    // any of these.
    e.insert("group by name counts runs", "beacon=1,checkout=2,refund=1,stalled=1".into());
    e.insert("group by environment counts runs", "production=3,staging=2".into());
    e.insert("group by status counts runs", "error=1,ok=4".into());
    e.insert("group by session counts runs", "s1=1,s2=1,s3=1,s4=1,s5=1".into());
    e.insert("group by user counts runs", "u1=1,u2=1,u3=1,u4=1,u5=1".into());
    e.insert("group by service counts runs", "api=5".into());

    // Three describe the step. A run that called two models belongs to both,
    // so these are counted over steps, and a step that names no model is not a
    // model rather than a group called `(none)`.
    e.insert("group by model counts steps", "claude-3-5-sonnet=1,gpt-4o=1".into());
    e.insert("group by provider counts steps", "anthropic=1,openai=1".into());
    e.insert("group by entity counts steps", "order_lookup=1".into());

    // A breakdown over a run dimension partitions the runs, so its groups add
    // up to the total in the same payload. This is the check that cannot be
    // passed by accident: it compares two numbers the same response returned.
    e.insert(
        "a run dimension adds up to the summary total",
        "environment=yes,name=yes,session_id=yes,status=yes,user_id=yes".into(),
    );

    // The cost series takes the same branch as the totals beside it.
    e.insert(
        "the cost series over a run dimension is keyed by run",
        "beacon,checkout,refund,stalled".into(),
    );
    e.insert("the cost breakdown totals the window's spend", "0.0080".into());

    // ---- the analysis surface -----------------------------------------
    //
    // The claim this block proves: all three backends give the same themes. The clustering itself is a pure function in the daemon and is
    // tested there; what is measured here is everything the storage layer
    // decides, which is what a wrong answer would come from.

    // One agent, six notes about a refund and four about a delivery, all
    // written against traces the fixture stored first.
    e.insert("a note is filed under the entity of the trace it describes", "agent/support".into());
    e.insert("the notes about one trace come back by trace id", "2".into());
    e.insert(
        "the roll-up counts notes and the traces behind them",
        "behaviour=4/4,goal=7/7".into(),
    );
    // Writing a note lags ingestion, so a note about a trace that is not stored
    // is ordinary. It is readable by trace id and absent from the roll-up,
    // which is what stops it being filed under an entity nobody named.
    e.insert("a note whose trace is not stored has no entity", "(none)".into());
    e.insert("a note whose trace is not stored is still readable by trace id", "1".into());
    e.insert("a note whose trace is not stored is outside the roll-up", "11".into());

    // A snapshot is written whole and read back whole, in one order.
    e.insert("a snapshot reads back with the themes it was written with", "2".into());
    e.insert("themes come back oldest snapshot first", "refund,delivery".into());
    e.insert("a theme's examples come back most alike its label first", "n1,n2,n3".into());
    e.insert("noise is read through the same route as a theme", "1".into());
    e.insert("a snapshot list reports how many the filter matched", "1".into());
    e.insert("a theme history is one point per snapshot that held it", "1".into());

    // Erasure. A note is one sentence about what a user asked for, so it is
    // the most direct copy of a person's words in this schema after the span
    // payloads, and a theme's label is a copy of one note.
    e.insert("erasing a subject erases the notes about them", "0".into());
    e.insert("erasing a subject leaves the notes about everybody else", "4".into());
    e.insert("erasing a subject empties a theme label taken from their run", "true".into());
    e.insert("erasing a subject drops the memberships of their notes", "0".into());

    // Completeness and duration.
    e.insert("a finished run is complete", "true".into());
    e.insert("a finished run reports its wall-clock duration", "500000000".into());
    e.insert("a run holding an event span is complete", "true".into());
    e.insert("a run holding an event span reports its wall-clock duration", "400000000".into());
    e.insert("a run holding an event span counts the event as a step", "2".into());
    e.insert("a run with a span still open is not complete", "false".into());
    e.insert("a run with a span still open reports the time elapsed so far", "60000000".into());
    e.insert("a run of nothing but an event is not complete", "false".into());
    e.insert("a run of nothing but an event has no duration", "(none)".into());
    e.insert("a run whose step failed is failed", "error".into());

    // Ingest is idempotent, by span identity and by batch key.
    e.insert("re-sending a batch does not duplicate its steps", "3".into());
    e.insert("re-sending a batch does not duplicate its runs", "5".into());
    e.insert("a replayed batch key is reported as a duplicate", "true".into());
    e.insert("a replayed batch key accepts nothing", "0".into());
    e.insert("a replayed batch key leaves one run", "1".into());

    // A window with nothing in it is an ordinary answer, not an error. It is
    // what the overview screen asks on a project's first day.
    e.insert("a window holding nothing counts nothing", "0".into());
    e.insert("a window holding nothing reports no latency", "(none)".into());

    // Full-text search is a search for words.
    e.insert("a word in a completion is found", "A".into());
    e.insert("a word that is not there is not found", "".into());
    e.insert("search is case insensitive", "A".into());
    e.insert("a prefix of a word is not a word", "".into());
    e.insert("a substring inside a word is not a word", "".into());
    e.insert("every term in a search must match", "".into());
    e.insert("a step's name is searchable", "C".into());
    e.insert("case folding reaches a letter that is not ASCII", "A".into());

    // A run's steps do not all arrive in one batch. A child step finishes and
    // is exported before the root that started earlier, so the second batch
    // moves the run's start time backwards.
    e.insert("a run split across two batches is one run", "1".into());
    e.insert("a run split across two batches counts its steps once", "3".into());
    e.insert("a run split across two batches is listed once", "1".into());
    e.insert("a run split across two batches falls in one group", "production=1".into());

    // ------------------------------------------------------------- discovery
    //
    // Ordered by value, ascending, on every backend. A filter list somebody is
    // reading must not reshuffle under them between polls, which is what
    // ordering by recency would do.
    e.insert("the filter box offers every environment", "production,staging".into());
    e.insert("the filter box offers every service", "checkout-api".into());
    e.insert("the filter box offers every entity kind", "tool".into());
    e.insert("the filter box offers every entity name", "order_lookup".into());
    e.insert("the filter box offers every tag", "beta,checkout".into());
    e.insert("the filter box offers every metric name", "mcpd.model.duration,mcpd.tokens".into());
    e.insert("the filter box offers every metric label key", "model,region".into());
    e.insert(
        "the filter box offers the values of one label key",
        "claude-3-5-sonnet,gpt-4o".into(),
    );
    e.insert("a facet prefix narrows the list", "gpt-4o".into());
    e.insert("another project's values are not offered", String::new());
    e.insert("label values without a key are refused", "refused".into());
    e.insert("discovery answers exactly when the capability says it does", "true".into());

    // --------------------------------------------------------------- polling
    e.insert("a first poll returns what is already there", "one,two".into());
    e.insert("polling again from the cursor returns nothing", String::new());
    e.insert("an empty poll leaves the cursor where it was", "true".into());
    e.insert("a run written after the cursor arrives on the next poll", "three".into());
    e.insert("a full page says there is more", "true".into());
    e.insert("a cursor this store did not issue is refused", "refused".into());

    // ------------------------------------------------------------- retention
    //
    // One run and one log line, both forty days old, under a seven day window
    // on logs alone.
    e.insert("expiring logs alone leaves the runs", "1".into());
    e.insert("expiring logs alone removes the logs", "0".into());
    e.insert("a second identical retention pass removes nothing", "0".into());
    e.insert("a per-project pass declares nothing", "false".into());
    e.insert(
        "declarative retention answers exactly when the capability says it does",
        "true".into(),
    );
    e.insert("a policy with no window keeps everything", "0,1".into());
    e.insert("expiring runs removes them", "0".into());

    e
}

/* ------------------------------------------------------------ the observer -- */

/// Ask one backend every case, and report what it said.
///
/// One function, so a case cannot exist for one backend and not another. That
/// is the whole point of the file: the 30, 8 and 3 tests it replaces were
/// written separately, which is how the backends drifted.
async fn observe(store: &dyn Store, project: String) -> Answers {
    let mut a = Answers::new();
    let f = Fixture { base: OffsetDateTime::now_utc() - Duration::hours(1), project };
    let (from, to) = f.window();
    let spans = f.spans();

    store
        .ingest(Batch { spans: spans.clone(), ..Batch::default() })
        .await
        .expect("the fixture must ingest");

    let summary = store
        .analytics_summary(&f.project, from, to, None)
        .await
        .expect("a summary with no grouping");
    a.insert("the summary counts the runs in the window", summary.trace_count.to_string());
    a.insert("the summary counts the steps in the window", summary.span_count.to_string());
    a.insert("the summary counts the runs that failed", summary.error_count.to_string());

    let mut breakdowns = BTreeMap::new();
    for dim in DIMENSIONS {
        let s = store
            .analytics_summary(&f.project, from, to, Some(dim))
            .await
            .unwrap_or_else(|e| panic!("a summary grouped by {dim}: {e}"));
        let groups = s.groups.unwrap_or_default();
        breakdowns.insert(dim, groups);
    }
    let render = |dim: &str| -> String {
        let mut pairs: Vec<String> =
            breakdowns[dim].iter().map(|g| format!("{}={}", g.key, g.count)).collect();
        pairs.sort();
        pairs.join(",")
    };
    a.insert("group by name counts runs", render("name"));
    a.insert("group by environment counts runs", render("environment"));
    a.insert("group by status counts runs", render("status"));
    a.insert("group by session counts runs", render("session_id"));
    a.insert("group by user counts runs", render("user_id"));
    a.insert("group by service counts runs", render("service_name"));
    a.insert("group by model counts steps", render("model"));
    a.insert("group by provider counts steps", render("provider"));
    a.insert("group by entity counts steps", render("entity_name"));

    // Every run-scoped dimension must partition the runs.
    let mut adds_up: Vec<String> = Vec::new();
    for dim in ["environment", "name", "session_id", "status", "user_id"] {
        let total: u64 = breakdowns[dim].iter().map(|g| g.count).sum();
        adds_up.push(format!("{dim}={}", if total == summary.trace_count { "yes" } else { "no" }));
    }
    adds_up.sort();
    a.insert("a run dimension adds up to the summary total", adds_up.join(","));

    let cost = store
        .analytics_cost(&f.project, from, to, "name", Some(3600))
        .await
        .expect("a cost breakdown bucketed by run name");
    let mut series_keys: Vec<String> =
        cost.series.unwrap_or_default().into_iter().map(|b| b.key).collect();
    series_keys.sort();
    series_keys.dedup();
    a.insert("the cost series over a run dimension is keyed by run", series_keys.join(","));
    a.insert("the cost breakdown totals the window's spend", format!("{:.4}", cost.total_cost));

    let run = |label: &'static str| {
        let (project, id) = (f.project.clone(), f.id(label));
        async move { store.get_trace(&project, &id, &Include::default()).await }
    };
    let ta = run(A).await.expect("run A").trace;
    a.insert("a finished run is complete", ta.is_complete.to_string());
    a.insert("a finished run reports its wall-clock duration", nanos(&ta));
    let tc = run(C).await.expect("run C").trace;
    a.insert("a run holding an event span is complete", tc.is_complete.to_string());
    a.insert("a run holding an event span reports its wall-clock duration", nanos(&tc));
    a.insert("a run holding an event span counts the event as a step", tc.span_count.to_string());
    let td = run(D).await.expect("run D").trace;
    a.insert("a run with a span still open is not complete", td.is_complete.to_string());
    a.insert("a run with a span still open reports the time elapsed so far", nanos(&td));
    let te = run(E).await.expect("run E").trace;
    a.insert("a run of nothing but an event is not complete", te.is_complete.to_string());
    a.insert("a run of nothing but an event has no duration", nanos(&te));
    let tb = run(B).await.expect("run B").trace;
    a.insert("a run whose step failed is failed", tb.status.as_str().to_owned());

    // Idempotency by span identity: the same steps sent again.
    store
        .ingest(Batch { spans: spans.clone(), ..Batch::default() })
        .await
        .expect("the replayed fixture must ingest");
    let ta = run(A).await.expect("run A after a replay").trace;
    a.insert("re-sending a batch does not duplicate its steps", ta.span_count.to_string());
    let after = store
        .analytics_summary(&f.project, from, to, None)
        .await
        .expect("a summary after a replay");
    a.insert("re-sending a batch does not duplicate its runs", after.trace_count.to_string());

    let empty = store
        .analytics_summary(
            &f.project,
            f.base - Duration::hours(3),
            f.base - Duration::hours(2),
            None,
        )
        .await
        .expect("a summary over a window holding nothing");
    a.insert("a window holding nothing counts nothing", empty.trace_count.to_string());
    a.insert(
        "a window holding nothing reports no latency",
        empty.latency.p50.map_or_else(|| "(none)".to_owned(), |v| format!("{v:.3}")),
    );

    // Full-text search, over the same fixture.
    let label = |t: &Trace| {
        [A, B, C, D, E]
            .into_iter()
            .find(|l| f.id(l) == t.trace_id)
            .unwrap_or(&t.trace_id)
            .to_owned()
    };
    let search = |q: &'static str| {
        let project = f.project.clone();
        async move {
            let hits = store
                .list_traces(&TraceQuery {
                    project,
                    q: Some(q.into()),
                    limit: 50,
                    ..Default::default()
                })
                .await
                .unwrap_or_else(|e| panic!("searching for {q:?}: {e}"));
            let mut names: Vec<String> = hits.items.iter().map(&label).collect();
            names.sort();
            names.join(",")
        }
    };
    a.insert("a word in a completion is found", search("Leeds").await);
    a.insert("a word that is not there is not found", search("aardvark").await);
    a.insert("search is case insensitive", search("leeds").await);
    a.insert("a prefix of a word is not a word", search("Leed").await);
    a.insert("a substring inside a word is not a word", search("sda").await);
    a.insert("every term in a search must match", search("Leeds aardvark").await);
    a.insert("a step's name is searchable", search("cache_miss").await);
    a.insert("case folding reaches a letter that is not ASCII", search("CAFÉ").await);

    a
}

/// The batch-key half of idempotency, in a project of its own.
///
/// Separate because a second run would change every count the cases above
/// assert, and a fixture that has to be read in a particular order is a
/// fixture that will be read in the wrong one.
async fn observe_replay(store: &dyn Store, project: String) -> Answers {
    let mut a = Answers::new();
    let f = Fixture { base: OffsetDateTime::now_utc() - Duration::hours(1), project };
    let (from, to) = f.window();
    let corr = f.corr("production", "s1", "u1");
    let spans = vec![f.span(Step {
        trace: &f.id(A),
        id: "a1",
        parent: None,
        name: "checkout",
        kind: SpanKind::AgentRun,
        at_ms: 0,
        len_ms: Some(500),
        corr,
    })];
    let key = Some(format!("key-{}", f.project));

    store
        .ingest(Batch { spans: spans.clone(), ingest_key: key.clone(), ..Batch::default() })
        .await
        .expect("the keyed batch must ingest");
    let replay = store
        .ingest(Batch { spans, ingest_key: key, ..Batch::default() })
        .await
        .expect("the replayed keyed batch must be answered, not refused");

    a.insert("a replayed batch key is reported as a duplicate", (replay.duplicate > 0).to_string());
    a.insert("a replayed batch key accepts nothing", replay.accepted.to_string());
    let s = store.analytics_summary(&f.project, from, to, None).await.expect("a summary");
    a.insert("a replayed batch key leaves one run", s.trace_count.to_string());
    a
}

/// One run whose steps arrive in two batches, in a project of its own.
///
/// This is the ordinary shape, not an edge case: a child step ends before the
/// parent it hangs off, so an exporter ships it first, and the root that
/// arrives later started earlier. The second batch therefore moves the run's
/// start time backwards.
///
/// Separate because it has to ingest twice with different contents, and the
/// fixture above asserts a count after a replay of the same contents, which is
/// a different question and answers it on a table that never moved.
async fn observe_out_of_order(store: &dyn Store, project: String) -> Answers {
    let mut a = Answers::new();
    let f = Fixture { base: OffsetDateTime::now_utc() - Duration::hours(1), project };
    let (from, to) = f.window();
    let id = f.id(A);
    let corr = f.corr("production", "s1", "u1");

    let child = f.span(Step {
        trace: &id,
        id: "a2",
        parent: Some("a1"),
        name: "generate",
        kind: SpanKind::ModelGeneration,
        at_ms: 50,
        len_ms: Some(100),
        corr: corr.clone(),
    });
    let tool = f.span(Step {
        trace: &id,
        id: "a3",
        parent: Some("a1"),
        name: "lookup",
        kind: SpanKind::ToolCall,
        at_ms: 200,
        len_ms: Some(30),
        corr: corr.clone(),
    });
    let root = f.span(Step {
        trace: &id,
        id: "a1",
        parent: None,
        name: "checkout",
        kind: SpanKind::AgentRun,
        at_ms: 0,
        len_ms: Some(500),
        corr,
    });

    store
        .ingest(Batch { spans: vec![child, tool], ..Batch::default() })
        .await
        .expect("the steps must ingest");
    store
        .ingest(Batch { spans: vec![root], ..Batch::default() })
        .await
        .expect("the root must ingest");

    let s = store
        .analytics_summary(&f.project, from, to, Some("environment"))
        .await
        .expect("a summary grouped by environment");
    a.insert("a run split across two batches is one run", s.trace_count.to_string());
    a.insert("a run split across two batches counts its steps once", s.span_count.to_string());
    let mut pairs: Vec<String> =
        s.groups.unwrap_or_default().iter().map(|g| format!("{}={}", g.key, g.count)).collect();
    pairs.sort();
    a.insert("a run split across two batches falls in one group", pairs.join(","));

    let listed = store
        .list_traces(&TraceQuery { project: f.project.clone(), limit: 50, ..Default::default() })
        .await
        .expect("the run list");
    a.insert("a run split across two batches is listed once", listed.items.len().to_string());

    a
}

fn nanos(t: &Trace) -> String {
    t.duration_ns.map_or_else(|| "(none)".to_owned(), |n| n.to_string())
}

/* ------------------------------------------------ what each backend can do -- */

/// The capabilities the specification says each backend has.
///
/// Per backend, and that is the point of it. Every other case in this file has
/// one answer for all three because the specification gives one answer; these
/// are the places where the three genuinely differ, and the difference is
/// written down here rather than discovered by a reader whose search returned
/// nothing.
///
/// Two properties are checked, and together they are what stops
/// `StoreCapabilities` becoming a decoration:
///
/// * The advertised value must be the value written here.
/// * The behaviour cases below must agree with it. A backend advertising
///   `discovery` whose `Store::discover` refuses fails, and so does one that
///   denies it and answers.
///
/// A backend that cannot do something is a documented `false` here, never a
/// case this file declines to ask.
fn documented_capabilities(backend: &str) -> Answers {
    let mut e = Answers::new();
    // A reused id never changes the row its owner wrote, on any backend. How
    // the second write is answered differs. SQLite and Postgres key these rows
    // by the id alone, so the id can belong to one project and the second
    // write is refused and counted. ClickHouse leads every sort key with the
    // project, so the second write is a separate row in the sender's own
    // project; checking first would mean a scan, because nothing is sorted by
    // the id.
    let (refused, stored, item) = if backend == "clickhouse" {
        ("rejected=0", "stored", "1,1")
    } else {
        ("rejected=1", "conflict", "1,0")
    };
    e.insert(
        "a span id another project holds leaves its span alone",
        format!("{refused} name=a-step"),
    );
    e.insert(
        "a log id another project holds leaves its line alone",
        format!("{refused} kept=a line"),
    );
    e.insert("a metric id another project holds is not taken from it", refused.to_owned());
    e.insert(
        "a score id another project holds leaves its score alone",
        format!("{refused} kept=0.9"),
    );
    e.insert(
        "a feedback id another project holds leaves its feedback alone",
        format!("{refused} kept=1"),
    );
    e.insert(
        "a note id another project holds leaves its note alone",
        format!("{refused} kept=a note"),
    );
    e.insert(
        "an experiment id another project holds leaves its experiment alone",
        format!("{stored} kept=a-exp"),
    );
    e.insert(
        "a dataset item id another dataset holds leaves its item alone",
        format!("{stored} kept={item}"),
    );
    // Every backend matches whole words rather than substrings: `checkout` is
    // not found by searching for `heck` on any of them.
    e.insert("capability text_search", "words".into());
    e.insert("capability discovery", "true".into());
    e.insert("capability discovery_label_values", "true".into());
    e.insert("capability delta_polling", "true".into());
    e.insert("capability per_signal_retention", "true".into());
    e.insert("capability idempotent_schema_init", "true".into());
    // Every backend folds case across Unicode, so `CAFÉ` finds `Café`.
    e.insert("capability unicode_case_folding", "true".into());
    if backend == "clickhouse" {
        // A ClickHouse delete is a mutation: accepted now, applied later.
        e.insert("capability immediate_deletes", "false".into());
        // One partition per day, so expiring a day is a metadata operation
        // rather than a read and rewrite of every row in it.
        e.insert("capability partitioning", "day".into());
        e.insert("capability partition_drop_retention", "true".into());
        // The tables can carry their own TTL, so a deployment-wide window is
        // handed to the engine once instead of being deleted on a timer.
        e.insert("capability declarative_retention", "true".into());
        e.insert("declaring a window changes the tables the first time", "true".into());
        e.insert("declaring the same window again changes nothing", "false".into());
    } else {
        e.insert("capability immediate_deletes", "true".into());
        // Neither of these two partitions. Retention is a row delete, which is
        // correct and reads every row it removes, and saying so is what stops a
        // reader assuming an expiry here is as cheap as it is on ClickHouse.
        e.insert("capability partitioning", "none".into());
        e.insert("capability partition_drop_retention", "false".into());
        // Neither has an engine-level expiry to hand a window to, so both
        // refuse rather than pretending. The refusal is the documented answer:
        // a case a backend cannot do is asked and answered, never skipped.
        e.insert("capability declarative_retention", "false".into());
        e.insert(
            "declaring a window changes the tables the first time",
            "unsupported:declarative_retention".into(),
        );
        e.insert(
            "declaring the same window again changes nothing",
            "unsupported:declarative_retention".into(),
        );
    }
    e
}

/// Read every capability field, so one added without a documented answer fails
/// rather than going unmeasured.
fn observe_capabilities(store: &dyn Store) -> Answers {
    let caps = store.capabilities();
    assert_eq!(caps.backend, store.name(), "a backend must describe itself under its own name");
    let mut a = Answers::new();
    for (field, value) in caps.fields() {
        // `fields` returns the names as `&'static str` already; the leak turns
        // the formatted case name into one. The suite runs once per process and
        // the set is fixed, so this is a handful of strings for the life of the
        // test binary rather than anything that grows.
        a.insert(Box::leak(format!("capability {field}").into_boxed_str()), value);
    }
    a
}

/// A correlation naming a project and nothing else.
fn only_project(project: &str) -> Correlation {
    Correlation {
        project: project.to_owned(),
        environment: None,
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
    }
}

/// A finished root span with nothing on it but the fields named.
fn root(project: &str, label: &str, at: OffsetDateTime, corr: Correlation) -> Span {
    let mut s = Span {
        trace_id: trace_id(project, label),
        span_id: format!("{label}1"),
        parent_span_id: None,
        external_parent_span_id: None,
        name: label.to_owned(),
        kind: SpanKind::AgentRun,
        status: SpanStatus::Ok,
        start_time: at,
        end_time: Some(at + Duration::milliseconds(5)),
        duration_ns: None,
        is_root: true,
        is_event: false,
        entity_kind: None,
        entity_id: None,
        entity_name: None,
        input: None,
        output: None,
        attributes: None,
        metadata: None,
        usage: None,
        cost: None,
        model_parameters: None,
        error: None,
        links: None,
        tags: None,
        correlation: corr,
        model: None,
        provider: None,
        ingest_key: None,
        // Stamped when the fixture is built, the way the daemon stamps a batch
        // on arrival. The poll cursor is (arrival time, id), so a fixture that
        // left every row at the epoch would make a run written later invisible
        // whenever its id happened to sort before the cursor's.
        received_at: OffsetDateTime::now_utc(),
    };
    s.normalise();
    s
}

/// How many runs a project holds.
async fn run_count(store: &dyn Store, project: &str) -> usize {
    store
        .list_traces(&TraceQuery {
            project: project.to_owned(),
            limit: 100,
            ..TraceQuery::default()
        })
        .await
        .map_or(usize::MAX, |p| p.items.len())
}

/// How many log lines a project holds.
async fn log_count(store: &dyn Store, project: &str) -> usize {
    store
        .list_logs(&LogQuery { project: project.to_owned(), limit: 100, ..LogQuery::default() })
        .await
        .map_or(usize::MAX, |p| p.items.len())
}

/// How many runs in `project` carry this trace id.
async fn runs_under(store: &dyn Store, project: &str, trace: &str) -> usize {
    store
        .list_traces(&TraceQuery {
            project: project.to_owned(),
            limit: 100,
            ..TraceQuery::default()
        })
        .await
        .map_or(usize::MAX, |page| page.items.iter().filter(|t| t.trace_id == trace).count())
}

/// What one project's run of a trace holds, and what its projection says.
async fn run_contents(store: &dyn Store, project: &str, trace: &str) -> String {
    let all = Include { logs: true, scores: true, feedback: true };
    match store.get_trace(project, trace, &all).await {
        Ok(d) => format!(
            "spans={}/{} logs={} scores={} feedback={}",
            d.spans.len(),
            d.trace.span_count,
            d.logs.map_or(0, |v| v.len()),
            d.scores.map_or(0, |v| v.len()),
            d.feedback.map_or(0, |v| v.len()),
        ),
        Err(e) => format!("error: {e}"),
    }
}

/* ----------------------------------------------------------------- tenancy -- */

/// Two projects that share a trace id, and a second project that reuses the
/// first one's signal ids.
///
/// The first half is ordinary distributed tracing: service A calls service B,
/// each exports to its own project, and both carry the caller's trace id. The
/// second half is the attack the shared id makes possible: a span id travels
/// to every downstream service in `traceparent`, so project B can name A's
/// span, and every other id a sender chooses can be learned the same way.
async fn observe_tenancy(store: &dyn Store, a: String, b: String) -> Answers {
    use mcpd_core::{AspectNote, Feedback, FeedbackKind, Score, ScoreSource};
    let mut out = Answers::new();
    let now = OffsetDateTime::now_utc() - Duration::minutes(5);
    let shared = trace_id(&a, "shared");

    let step = |project: &str, span_id: &str, parent: Option<&str>, name: &str| {
        let mut s = root(project, "shared", now, only_project(project));
        s.trace_id.clone_from(&shared);
        s.span_id = span_id.to_owned();
        s.parent_span_id = parent.map(str::to_owned);
        s.is_root = parent.is_none();
        s.name = name.to_owned();
        s.kind = if parent.is_none() { SpanKind::AgentRun } else { SpanKind::ToolCall };
        // Each project's run is filed under its own agent, so a note can be
        // checked for which run it took its entity from.
        s.entity_kind = Some(EntityKind::Agent);
        s.entity_name = Some(format!("agent-{}", &name[..1]));
        s.normalise();
        s
    };
    let log = |project: &str, id: &str, message: &str| LogRecord {
        log_id: id.to_owned(),
        trace_id: Some(shared.clone()),
        span_id: None,
        timestamp: now,
        level: LogLevel::Info,
        message: message.to_owned(),
        fields: None,
        logger: None,
        correlation: only_project(project),
        received_at: OffsetDateTime::UNIX_EPOCH,
    };
    let metric = |project: &str, id: &str, value: f64| MetricPoint {
        metric_id: id.to_owned(),
        name: "tenancy.points".into(),
        kind: MetricKind::Gauge,
        value,
        unit: None,
        timestamp: now,
        labels: None,
        buckets: None,
        trace_id: Some(shared.clone()),
        span_id: None,
        correlation: only_project(project),
        received_at: OffsetDateTime::UNIX_EPOCH,
    };
    let score = |project: &str, id: &str, scorer: &str, value: f64| Score {
        score_id: id.to_owned(),
        trace_id: Some(shared.clone()),
        span_id: None,
        scorer_id: scorer.to_owned(),
        scorer_version: None,
        source: ScoreSource::Live,
        value,
        min: Some(0.0),
        max: Some(1.0),
        label: None,
        reason: None,
        score_trace_id: None,
        cost: None,
        metadata: None,
        timestamp: now,
        correlation: only_project(project),
    };
    let feedback = |project: &str, id: &str, value: i64| Feedback {
        feedback_id: id.to_owned(),
        trace_id: Some(shared.clone()),
        span_id: None,
        kind: FeedbackKind::Rating,
        value: json!(value),
        comment: None,
        author_id: None,
        author_kind: None,
        timestamp: now,
        metadata: None,
        correlation: only_project(project),
    };
    let note = |project: &str, id: &str, text: &str| AspectNote {
        note_id: id.to_owned(),
        trace_id: shared.clone(),
        aspect: "goal".into(),
        text: text.to_owned(),
        definition_version: 1,
        model: None,
        created_at: now,
        correlation: only_project(project),
    };

    // Ids every write below uses, unique to this run because a live server
    // keeps rows between runs.
    let id = |what: &str| format!("{a}-{what}");

    // A: the caller. A root span and one step, and one of everything else.
    store
        .ingest(Batch {
            spans: vec![
                step(&a, "a000000000000001", None, "a-root"),
                step(&a, "a000000000000002", Some("a000000000000001"), "a-step"),
            ],
            logs: vec![log(&a, &id("log"), "a line")],
            metrics: vec![metric(&a, &id("metric"), 1.0)],
            scores: vec![score(&a, &id("score"), "a-scorer", 0.9)],
            feedback: vec![feedback(&a, &id("feedback"), 1)],
            aspect_notes: vec![note(&a, &id("note"), "a note")],
            ..Batch::default()
        })
        .await
        .expect("project a must ingest");

    // B: the callee, in the same trace. Its step's parent is A's step, which B
    // does not hold, and that is the ordinary shape of a downstream service.
    store
        .ingest(Batch {
            spans: vec![step(&b, "b000000000000001", Some("a000000000000002"), "b-step")],
            logs: vec![log(&b, &format!("{b}-log"), "b line")],
            scores: vec![score(&b, &format!("{b}-score"), "b-scorer", 0.2)],
            feedback: vec![feedback(&b, &format!("{b}-feedback"), 5)],
            ..Batch::default()
        })
        .await
        .expect("project b must ingest");

    // A note B writes about its own run of the shared id, after the run is
    // stored. It must be filed under B's agent, never under A's.
    store
        .ingest(Batch {
            aspect_notes: vec![note(&b, &format!("{b}-note"), "b note")],
            ..Batch::default()
        })
        .await
        .expect("project b's note must ingest");
    let filed = match store
        .list_aspect_notes(&mcpd_store::AspectNoteQuery {
            project: b.clone(),
            note_ids: vec![format!("{b}-note")],
            limit: 10,
            ..mcpd_store::AspectNoteQuery::default()
        })
        .await
    {
        Ok(p) => p
            .items
            .into_iter()
            .next()
            .map_or("gone".to_owned(), |n| n.entity_name.unwrap_or_else(|| "none".into())),
        Err(e) => format!("error: {e}"),
    };
    out.insert("a note about a shared trace id is filed under its own project's agent", filed);

    out.insert(
        "a trace id two projects share is a run in each",
        format!(
            "{},{}",
            runs_under(store, &a, &shared).await,
            runs_under(store, &b, &shared).await
        ),
    );
    out.insert(
        "each project's run of a shared trace id holds only its own signals",
        format!(
            "a: {}; b: {}",
            run_contents(store, &a, &shared).await,
            run_contents(store, &b, &shared).await
        ),
    );
    let all = Include { logs: true, scores: true, feedback: true };

    // B names A's ids. Each write must be refused, and A's row must be what A
    // wrote. Sent one signal per batch so each refusal is counted on its own.
    let refused = |r: mcpd_store::Result<mcpd_core::IngestResult>| {
        r.map_or_else(|e| format!("error: {e}"), |r| format!("rejected={}", r.rejected))
    };

    let r = store
        .ingest(Batch {
            spans: vec![step(&b, "a000000000000002", Some("a000000000000001"), "b-took-it")],
            ..Batch::default()
        })
        .await;
    let name = store
        .get_trace(&a, &shared, &Include::default())
        .await
        .ok()
        .and_then(|d| d.spans.into_iter().find(|s| s.span_id == "a000000000000002"))
        .map_or("gone".to_owned(), |s| s.name);
    out.insert(
        "a span id another project holds leaves its span alone",
        format!("{} name={name}", refused(r)),
    );

    let r = store
        .ingest(Batch { logs: vec![log(&b, &id("log"), "b took it")], ..Batch::default() })
        .await;
    let kept = store
        .list_logs(&LogQuery { project: a.clone(), limit: 100, ..LogQuery::default() })
        .await
        .ok()
        .and_then(|p| p.items.into_iter().find(|l| l.log_id == id("log")))
        .map_or("gone".to_owned(), |l| l.message);
    out.insert(
        "a log id another project holds leaves its line alone",
        format!("{} kept={kept}", refused(r)),
    );

    let r = store
        .ingest(Batch { metrics: vec![metric(&b, &id("metric"), 99.0)], ..Batch::default() })
        .await;
    out.insert("a metric id another project holds is not taken from it", refused(r));

    let r = store
        .ingest(Batch { scores: vec![score(&b, &id("score"), "b-reuse", 0.0)], ..Batch::default() })
        .await;
    let kept = store
        .list_scores(&mcpd_store::ScoreQuery {
            project: a.clone(),
            limit: 100,
            ..mcpd_store::ScoreQuery::default()
        })
        .await
        .ok()
        .and_then(|p| p.items.into_iter().find(|s| s.score_id == id("score")))
        .map_or("gone".to_owned(), |s| format!("{}", s.value));
    out.insert(
        "a score id another project holds leaves its score alone",
        format!("{} kept={kept}", refused(r)),
    );

    let r = store
        .ingest(Batch { feedback: vec![feedback(&b, &id("feedback"), 0)], ..Batch::default() })
        .await;
    let kept = store
        .get_trace(&a, &shared, &all)
        .await
        .ok()
        .and_then(|d| d.feedback)
        .and_then(|f| f.into_iter().find(|f| f.feedback_id == id("feedback")))
        .map_or("gone".to_owned(), |f| f.value.to_string());
    out.insert(
        "a feedback id another project holds leaves its feedback alone",
        format!("{} kept={kept}", refused(r)),
    );

    let r = store
        .ingest(Batch {
            aspect_notes: vec![note(&b, &id("note"), "b took it")],
            ..Batch::default()
        })
        .await;
    let kept = match store
        .list_aspect_notes(&mcpd_store::AspectNoteQuery {
            project: a.clone(),
            note_ids: vec![id("note")],
            limit: 10,
            ..mcpd_store::AspectNoteQuery::default()
        })
        .await
    {
        Ok(p) => p.items.into_iter().next().map_or("gone".to_owned(), |n| n.note.text),
        Err(e) => format!("error: {e}"),
    };
    out.insert(
        "a note id another project holds leaves its note alone",
        format!("{} kept={kept}", refused(r)),
    );

    // B filters its own runs by a scorer only A used. B's run shares the
    // trace id with A's scored run, which is exactly what a join on the trace
    // id alone would mistake for a match.
    let filtered = store
        .list_traces(&TraceQuery {
            project: b.clone(),
            has_score: Some("a-scorer".into()),
            limit: 100,
            ..TraceQuery::default()
        })
        .await
        .map_or(usize::MAX, |p| p.items.len());
    out.insert("the score filter does not reach another project's score", filtered.to_string());

    // B deletes its run. A's run under the same id must not lose anything.
    let _ = store.delete_trace(&b, &shared).await;
    let after = match store.get_trace(&a, &shared, &all).await {
        Ok(d) => format!(
            "spans={} logs={} scores={} feedback={}",
            d.spans.len(),
            d.logs.map_or(0, |v| v.len()),
            d.scores.map_or(0, |v| v.len()),
            d.feedback.map_or(0, |v| v.len()),
        ),
        Err(e) => format!("error: {e}"),
    };
    out.insert("deleting a shared trace in one project leaves the other's run", after);

    // Experiments and dataset items are not signals but carry an id the
    // caller may choose, and both used to move to whoever wrote it last.
    let experiment = |project: &str, name: &str| mcpd_core::Experiment {
        experiment_id: id("experiment"),
        name: name.to_owned(),
        dataset_id: String::new(),
        status: mcpd_core::ExperimentStatus::Running,
        started_at: Some(now),
        finished_at: None,
        item_count: 0,
        summary: None,
        baseline_id: None,
        cost: None,
        error: None,
        metadata: None,
        correlation: only_project(project),
    };
    let refusal = |r: mcpd_store::Result<()>| match r {
        Err(mcpd_store::Error::Conflict(_)) => "conflict".to_owned(),
        Err(e) => format!("error: {e}"),
        Ok(()) => "stored".to_owned(),
    };
    let _ = store.put_experiment(&experiment(&a, "a-exp")).await;
    let r = refusal(store.put_experiment(&experiment(&b, "b-exp")).await);
    let kept = store
        .list_experiments(&a)
        .await
        .ok()
        .and_then(|v| v.into_iter().find(|e| e.experiment_id == id("experiment")))
        .map_or("gone".to_owned(), |e| e.name);
    out.insert(
        "an experiment id another project holds leaves its experiment alone",
        format!("{r} kept={kept}"),
    );

    let dataset = |project: &str, which: &str| mcpd_core::Dataset {
        dataset_id: format!("{a}-{which}"),
        name: which.to_owned(),
        description: None,
        item_count: 0,
        created_at: now,
        correlation: only_project(project),
    };
    let item = |dataset_id: &str| mcpd_core::DatasetItem {
        item_id: id("item"),
        dataset_id: dataset_id.to_owned(),
        input: json!({ "q": "a question" }),
        expected: None,
        metadata: None,
        source_trace_id: None,
    };
    let (da, db) = (dataset(&a, "ds-a"), dataset(&b, "ds-b"));
    let _ = store.put_dataset(&da).await;
    let _ = store.put_dataset(&db).await;
    let _ = store.add_dataset_items(&da.dataset_id, &[item(&da.dataset_id)]).await;
    let r =
        refusal(store.add_dataset_items(&db.dataset_id, &[item(&db.dataset_id)]).await.map(|_| ()));
    let count = |d: String| async move {
        store.list_dataset_items(&d, 100).await.map_or(usize::MAX, |v| v.len())
    };
    let kept =
        format!("{},{}", count(da.dataset_id.clone()).await, count(db.dataset_id.clone()).await);
    out.insert(
        "a dataset item id another dataset holds leaves its item alone",
        format!("{r} kept={kept}"),
    );

    out
}

/* --------------------------------------------------------------- discovery -- */

/// Render a discovery answer, keeping a refusal as a value rather than
/// panicking on it. A backend that cannot do this has to be able to say so
/// here, or the case turns into a skip.
fn offered(r: mcpd_store::Result<Vec<Discovered>>) -> String {
    match r {
        Ok(v) => v.into_iter().map(|d| d.value).collect::<Vec<_>>().join(","),
        Err(mcpd_store::Error::Unsupported { capability, .. }) => {
            format!("unsupported:{capability}")
        }
        Err(e) => format!("error:{e}"),
    }
}

/// One signal of each kind, carrying the values a filter box would offer.
async fn observe_discovery(store: &dyn Store, project: String) -> Answers {
    let mut a = Answers::new();
    let now = OffsetDateTime::now_utc() - Duration::hours(1);

    let mut span = root(
        &project,
        "checkout",
        now,
        Correlation {
            environment: Some("production".into()),
            service_name: Some("checkout-api".into()),
            ..only_project(&project)
        },
    );
    span.entity_kind = Some(EntityKind::Tool);
    span.entity_name = Some("order_lookup".into());
    span.tags = Some(vec!["beta".into(), "checkout".into()]);

    let metric = MetricPoint {
        metric_id: format!("{project}-m1"),
        name: "mcpd.model.duration".into(),
        kind: MetricKind::Gauge,
        value: 12.0,
        unit: None,
        timestamp: now,
        labels: Some(json!({ "model": "gpt-4o", "region": "eu-west-1" })),
        buckets: None,
        trace_id: None,
        span_id: None,
        correlation: Correlation { environment: Some("staging".into()), ..only_project(&project) },
        received_at: OffsetDateTime::UNIX_EPOCH,
    };
    let metric_two = MetricPoint {
        metric_id: format!("{project}-m2"),
        name: "mcpd.tokens".into(),
        labels: Some(json!({ "model": "claude-3-5-sonnet" })),
        ..metric.clone()
    };

    store
        .ingest(Batch { spans: vec![span], metrics: vec![metric, metric_two], ..Batch::default() })
        .await
        .expect("the discovery fixture must ingest");

    // A second project, so "scoped to one project" is measured rather than
    // assumed. A filter box offering another tenant's environments is a leak,
    // not a cosmetic bug.
    let other = format!("{project}-other");
    let stray = root(
        &other,
        "elsewhere",
        now,
        Correlation { environment: Some("canary".into()), ..only_project(&other) },
    );
    store.ingest(Batch { spans: vec![stray], ..Batch::default() }).await.expect("the stray run");

    let ask = |facet: Facet, key: Option<&str>, prefix: Option<&str>| DiscoveryQuery {
        project: project.clone(),
        facet,
        key: key.map(ToOwned::to_owned),
        prefix: prefix.map(ToOwned::to_owned),
        limit: 50,
    };

    a.insert(
        "the filter box offers every environment",
        offered(store.discover(&ask(Facet::Environment, None, None)).await),
    );
    a.insert(
        "the filter box offers every service",
        offered(store.discover(&ask(Facet::ServiceName, None, None)).await),
    );
    a.insert(
        "the filter box offers every entity kind",
        offered(store.discover(&ask(Facet::EntityKind, None, None)).await),
    );
    a.insert(
        "the filter box offers every entity name",
        offered(store.discover(&ask(Facet::EntityName, None, None)).await),
    );
    a.insert(
        "the filter box offers every tag",
        offered(store.discover(&ask(Facet::Tag, None, None)).await),
    );
    a.insert(
        "the filter box offers every metric name",
        offered(store.discover(&ask(Facet::MetricName, None, None)).await),
    );
    a.insert(
        "the filter box offers every metric label key",
        offered(store.discover(&ask(Facet::MetricLabelKey, None, None)).await),
    );
    a.insert(
        "the filter box offers the values of one label key",
        offered(store.discover(&ask(Facet::MetricLabelValue, Some("model"), None)).await),
    );
    a.insert(
        "a facet prefix narrows the list",
        offered(store.discover(&ask(Facet::MetricLabelValue, Some("model"), Some("g"))).await),
    );
    // The stray run's environment belongs to another project. Asking for it by
    // name here must find nothing.
    a.insert(
        "another project's values are not offered",
        offered(store.discover(&ask(Facet::Environment, None, Some("canary"))).await),
    );
    // Asking for label values with no key would mix every label's values
    // together, so it is refused rather than answered wrongly.
    a.insert(
        "label values without a key are refused",
        match store.discover(&ask(Facet::MetricLabelValue, None, None)).await {
            Err(mcpd_store::Error::InvalidQuery(_)) => "refused".into(),
            other => format!("not refused: {}", offered(other)),
        },
    );
    // The advertisement has to match the behaviour, in both directions. This is
    // the check that stops the capability type becoming a decoration.
    let answered = !a["the filter box offers every environment"].starts_with("unsupported:");
    a.insert(
        "discovery answers exactly when the capability says it does",
        (store.capabilities().discovery == answered).to_string(),
    );
    a
}

/* ----------------------------------------------------------------- polling -- */

/// The run names in one poll, sorted, so an arrival order that two backends
/// break ties in differently does not read as a difference in what they
/// returned.
fn polled(d: &Delta<PolledSignal>) -> String {
    let mut v: Vec<String> = d
        .items
        .iter()
        .map(|i| match i {
            PolledSignal::Trace(t) => t.name.clone(),
            other => format!("wrong signal: {}", other.signal().as_str()),
        })
        .collect();
    v.sort();
    v.join(",")
}

/// A list refreshing from a cursor rather than re-running its query.
async fn observe_polling(store: &dyn Store, project: String) -> Answers {
    let mut a = Answers::new();
    let now = OffsetDateTime::now_utc() - Duration::hours(1);

    store
        .ingest(Batch {
            spans: vec![
                root(&project, "one", now, only_project(&project)),
                root(&project, "two", now, only_project(&project)),
            ],
            ..Batch::default()
        })
        .await
        .expect("the first two runs");

    let ask = |cursor: Option<String>| DeltaQuery {
        project: project.clone(),
        signal: DeltaSignal::Traces,
        cursor,
        since: None,
        limit: 50,
    };

    let first = store.poll(&ask(None)).await.expect("a first poll");
    a.insert("a first poll returns what is already there", polled(&first));

    // Polling again from the cursor must return nothing: everything before it
    // was handed over once already.
    let again = store.poll(&ask(Some(first.cursor.clone()))).await.expect("a second poll");
    a.insert("polling again from the cursor returns nothing", polled(&again));
    // An empty poll must leave the cursor exactly where it was. Advancing it to
    // "now" is what would drop a row that lands a moment later carrying an
    // earlier arrival time.
    a.insert(
        "an empty poll leaves the cursor where it was",
        (again.cursor == first.cursor).to_string(),
    );

    store
        .ingest(Batch {
            spans: vec![root(
                &project,
                "three",
                now + Duration::minutes(1),
                only_project(&project),
            )],
            ..Batch::default()
        })
        .await
        .expect("the third run");
    let third = store.poll(&ask(Some(again.cursor.clone()))).await.expect("a third poll");
    a.insert("a run written after the cursor arrives on the next poll", polled(&third));

    // A page that filled says so, so a caller draining a backlog polls again at
    // once rather than waiting for its next tick.
    let one = store.poll(&DeltaQuery { limit: 1, ..ask(None) }).await.expect("a poll of one");
    a.insert("a full page says there is more", one.has_more.to_string());

    // The cursor is opaque. Anything a caller invented is refused rather than
    // read as a timestamp and silently skipping every row before it.
    a.insert(
        "a cursor this store did not issue is refused",
        match store.poll(&ask(Some("not-a-cursor".into()))).await {
            Err(mcpd_store::Error::InvalidQuery(_)) => "refused".into(),
            Err(e) => format!("wrong error: {e}"),
            Ok(_) => "accepted".into(),
        },
    );
    a
}

/* --------------------------------------------------------------- retention -- */

/// A window per signal, applied twice.
/// The analysis surface: notes in, a snapshot in, and everything read back.
///
/// Written as one function because the answers depend on each other: a note is
/// filed under the entity of its trace, a theme's examples are the notes it was
/// written with, and an erasure has to reach both. Splitting it would mean
/// three fixtures that have to agree with each other by hand.
async fn observe_themes(store: &dyn Store, project: String) -> Answers {
    let mut a = Answers::new();
    if !store.capabilities().theme_analysis {
        // A backend that does not hold the tables answers nothing here rather
        // than answering wrongly. The report names it: an absent answer is a
        // mismatch, which is the right way round.
        return a;
    }
    let now = OffsetDateTime::now_utc();
    let corr = |user: &str| Correlation {
        project: project.clone(),
        environment: Some("production".into()),
        service_name: Some("api".into()),
        service_version: None,
        session_id: None,
        thread_id: None,
        user_id: Some(user.to_owned()),
        resource_id: None,
        run_id: None,
        request_id: None,
        experiment_id: None,
        release: None,
    };

    // Ten runs of one agent. The entity lives on the root span, which is where
    // every backend resolves a note's entity from.
    let mut spans = Vec::new();
    for i in 0..10 {
        let id = trace_id(&project, &format!("note-run-{i}"));
        let mut s = Span {
            trace_id: id,
            span_id: format!("{i:016x}"),
            parent_span_id: None,
            external_parent_span_id: None,
            name: "support".into(),
            kind: SpanKind::AgentRun,
            status: SpanStatus::Ok,
            start_time: now - Duration::minutes(30) + Duration::seconds(i),
            end_time: Some(now - Duration::minutes(29) + Duration::seconds(i)),
            duration_ns: None,
            is_root: true,
            is_event: false,
            entity_kind: Some(EntityKind::Agent),
            entity_id: None,
            entity_name: Some("support".into()),
            input: None,
            output: None,
            attributes: None,
            metadata: None,
            usage: None,
            cost: None,
            error: None,
            links: None,
            tags: None,
            model: None,
            model_parameters: None,
            provider: None,
            correlation: corr(if i == 0 { "subject" } else { "somebody-else" }),
            received_at: now - Duration::minutes(30),
            ingest_key: None,
        };
        s.normalise();
        spans.push(s);
    }
    store.ingest(Batch { spans, ..Batch::default() }).await.expect("the fixture must ingest");

    // Six notes along `goal` and four along `behaviour`. The first note of each
    // belongs to the subject erased at the end.
    //
    // Note ids carry the project. `note_id` is the primary key on every
    // backend, and a Postgres or ClickHouse server keeps its rows between runs
    // of this suite, so an id that was the same in every run would find the
    // previous run's note under the previous run's project and update it there
    // rather than write one here.
    let note = |i: usize, aspect: &str, text: &str| mcpd_core::AspectNote {
        note_id: format!("{project}-n{i}"),
        trace_id: trace_id(&project, &format!("note-run-{i}")),
        aspect: aspect.to_owned(),
        text: text.to_owned(),
        definition_version: 1,
        model: Some("a-model".into()),
        created_at: now - Duration::minutes(20) + Duration::seconds(i as i64),
        correlation: corr(if i == 0 { "subject" } else { "somebody-else" }),
    };
    let mut notes: Vec<mcpd_core::AspectNote> = (0..6)
        .map(|i| note(i, "goal", "the customer wants a refund for a late delivery"))
        .collect();
    notes.extend(
        (6..10).map(|i| note(i, "behaviour", "the agent looked the order up and apologised")),
    );
    // One note about a trace nothing has stored. Writing a note lags ingestion,
    // so this is an ordinary state and not a fault.
    notes.push(mcpd_core::AspectNote {
        note_id: format!("{project}-n-orphan"),
        trace_id: trace_id(&project, "note-run-never-stored"),
        aspect: "goal".into(),
        text: "a sentence about a run nothing has stored yet".into(),
        definition_version: 1,
        model: None,
        created_at: now - Duration::minutes(19),
        correlation: corr("somebody-else"),
    });
    store
        .ingest(Batch { aspect_notes: notes, ..Batch::default() })
        .await
        .expect("the notes must ingest");

    let by_trace = store
        .list_aspect_notes(&mcpd_store::AspectNoteQuery {
            project: project.clone(),
            trace_id: Some(trace_id(&project, "note-run-6")),
            limit: 50,
            ..mcpd_store::AspectNoteQuery::default()
        })
        .await
        .expect("the notes about one trace");
    a.insert(
        "a note is filed under the entity of the trace it describes",
        by_trace.items.first().map_or_else(
            || "(none)".to_owned(),
            |n| {
                format!(
                    "{}/{}",
                    n.entity_kind.map_or("(none)".to_owned(), |k| k.as_str().to_owned()),
                    n.entity_name.clone().unwrap_or_else(|| "(none)".into())
                )
            },
        ),
    );
    // A second note about run 6, along another aspect and under an id of its
    // own, so the read below proves it is by trace and not by aspect. A note id
    // identifies the sentence and never the run: writing this under `n6` would
    // have replaced the behaviour note instead of joining it, which is the
    // mistake the two names exist to prevent.
    let mut second = note(6, "goal", "the customer wants a refund for a late delivery");
    second.note_id = format!("{project}-n6-goal");
    store
        .ingest(Batch { aspect_notes: vec![second], ..Batch::default() })
        .await
        .expect("a second note about one run");
    let by_trace = store
        .list_aspect_notes(&mcpd_store::AspectNoteQuery {
            project: project.clone(),
            trace_id: Some(trace_id(&project, "note-run-6")),
            limit: 50,
            ..mcpd_store::AspectNoteQuery::default()
        })
        .await
        .expect("the notes about one trace");
    a.insert("the notes about one trace come back by trace id", by_trace.items.len().to_string());

    let orphan = store
        .list_aspect_notes(&mcpd_store::AspectNoteQuery {
            project: project.clone(),
            note_ids: vec![format!("{project}-n-orphan")],
            limit: 10,
            ..mcpd_store::AspectNoteQuery::default()
        })
        .await
        .expect("the orphan note");
    a.insert(
        "a note whose trace is not stored has no entity",
        orphan
            .items
            .first()
            .and_then(|n| n.entity_name.clone())
            .unwrap_or_else(|| "(none)".to_owned()),
    );
    a.insert(
        "a note whose trace is not stored is still readable by trace id",
        orphan.items.len().to_string(),
    );

    let rollup = store
        .aspect_note_rollup(&mcpd_store::AspectRollupQuery {
            project: project.clone(),
            entity_kind: None,
            entity: Some((EntityKind::Agent, "support".into())),
            max_groups: 50,
        })
        .await
        .expect("the roll-up");
    let mut pairs: Vec<String> =
        rollup.iter().map(|r| format!("{}={}/{}", r.aspect, r.note_count, r.trace_count)).collect();
    pairs.sort();
    a.insert("the roll-up counts notes and the traces behind them", pairs.join(","));
    a.insert(
        "a note whose trace is not stored is outside the roll-up",
        rollup.iter().map(|r| r.note_count).sum::<u64>().to_string(),
    );

    // One snapshot, two themes, and a note in noise. Written whole and read
    // back whole: the clustering that chose these groups is a pure function in
    // the daemon and is tested there.
    let snapshot_id = format!("snap-{project}");
    let theme = |id: &str, label: &str, note: usize, count: u64| mcpd_store::StoredTheme {
        theme: mcpd_core::Theme {
            theme_id: id.to_owned(),
            snapshot_id: snapshot_id.clone(),
            aspect: "goal".into(),
            entity_kind: EntityKind::Agent,
            entity_name: "support".into(),
            label: label.to_owned(),
            label_note_id: format!("{project}-n{note}"),
            label_trace_id: trace_id(&project, &format!("note-run-{note}")),
            state: mcpd_core::ThemeState::Birth,
            note_count: count,
            trace_count: count,
            coverage: count as f64 / 6.0,
        },
        project: project.clone(),
        cutoff_at: now,
        window_from: now - Duration::hours(1),
        window_to: now,
        lineage: mcpd_store::ThemeLineage::default(),
    };
    let member = |theme_id: &str, note: usize, similarity: u32| mcpd_store::ThemeMember {
        snapshot_id: snapshot_id.clone(),
        project: project.clone(),
        aspect: "goal".into(),
        theme_id: theme_id.to_owned(),
        note_id: format!("{project}-n{note}"),
        trace_id: trace_id(&project, &format!("note-run-{note}")),
        similarity,
        trace_started_at: now - Duration::minutes(30),
    };
    store
        .write_theme_snapshot(&mcpd_store::SnapshotWrite {
            snapshot: mcpd_core::ThemeSnapshot {
                snapshot_id: snapshot_id.clone(),
                project: project.clone(),
                entity_kind: EntityKind::Agent,
                entity_name: "support".into(),
                aspects: vec!["goal".into()],
                ordinal: 1,
                cutoff_at: now,
                window_from: now - Duration::hours(1),
                window_to: now,
                trace_count: 6,
                note_count: 6,
                capped: false,
            },
            themes: vec![theme("t-refund", "refund", 0, 3), theme("t-delivery", "delivery", 3, 2)],
            members: vec![
                // Deliberately out of order, and with a tie on similarity, so
                // the read has to impose the documented order rather than
                // return them as they were written.
                member("t-refund", 3, 40),
                member("t-refund", 2, 50),
                member("t-refund", 1, 50),
                member("t-delivery", 4, 60),
                member("t-delivery", 5, 55),
                member(mcpd_store::NOISE_THEME_ID, 0, 0),
            ],
        })
        .await
        .expect("a snapshot must write");

    let back = store
        .list_themes(&mcpd_store::ThemeQuery {
            project: project.clone(),
            snapshot_id: Some(snapshot_id.clone()),
            limit: 50,
            ..mcpd_store::ThemeQuery::default()
        })
        .await
        .expect("the themes of one snapshot");
    a.insert(
        "a snapshot reads back with the themes it was written with",
        back.items.len().to_string(),
    );
    let mut labels: Vec<String> = back.items.iter().map(|t| t.theme.label.clone()).collect();
    labels.sort_by_key(|l| if l == "refund" { 0 } else { 1 });
    a.insert("themes come back oldest snapshot first", labels.join(","));

    let examples = store
        .list_theme_members(&mcpd_store::ThemeMemberQuery {
            project: project.clone(),
            snapshot_id: snapshot_id.clone(),
            aspect: Some("goal".into()),
            theme_id: Some("t-refund".into()),
            limit: 50,
            cursor: None,
        })
        .await
        .expect("a theme's examples");
    // Reported without the project prefix the fixture puts on every note id,
    // so the expectation reads as the three notes and not as a run stamp.
    let prefix = format!("{project}-");
    a.insert(
        "a theme's examples come back most alike its label first",
        examples
            .items
            .iter()
            .map(|m| m.note_id.strip_prefix(&prefix).unwrap_or(&m.note_id))
            .collect::<Vec<_>>()
            .join(","),
    );

    let noise = store
        .list_theme_members(&mcpd_store::ThemeMemberQuery {
            project: project.clone(),
            snapshot_id: snapshot_id.clone(),
            aspect: Some("goal".into()),
            theme_id: Some(mcpd_store::NOISE_THEME_ID.into()),
            limit: 50,
            cursor: None,
        })
        .await
        .expect("the noise of one stage");
    a.insert("noise is read through the same route as a theme", noise.items.len().to_string());

    let listed = store
        .list_theme_snapshots(&mcpd_store::SnapshotQuery {
            project: project.clone(),
            entity: Some((EntityKind::Agent, "support".into())),
            aspects: vec!["goal".into()],
            limit: 10,
            ..mcpd_store::SnapshotQuery::default()
        })
        .await
        .expect("the snapshots of one entity");
    a.insert(
        "a snapshot list reports how many the filter matched",
        listed.total.map_or_else(|| "(none)".to_owned(), |t| t.to_string()),
    );

    let history = store
        .list_themes(&mcpd_store::ThemeQuery {
            project: project.clone(),
            entity: Some((EntityKind::Agent, "support".into())),
            aspect: Some("goal".into()),
            theme_id: Some("t-refund".into()),
            limit: 50,
            ..mcpd_store::ThemeQuery::default()
        })
        .await
        .expect("one theme across every snapshot");
    a.insert(
        "a theme history is one point per snapshot that held it",
        history.items.len().to_string(),
    );

    // The erasure. `n0` is the subject's note, and `t-refund` is labelled from
    // the subject's run, so both have to be reached by one request.
    store.erase_subject(&project, "subject").await.expect("an erasure");
    let after = store
        .list_aspect_notes(&mcpd_store::AspectNoteQuery {
            project: project.clone(),
            note_ids: vec!["n0".into()],
            limit: 10,
            ..mcpd_store::AspectNoteQuery::default()
        })
        .await
        .expect("the subject's notes after the erasure");
    a.insert("erasing a subject erases the notes about them", after.items.len().to_string());

    let others = store
        .list_aspect_notes(&mcpd_store::AspectNoteQuery {
            project: project.clone(),
            aspect: Some("behaviour".into()),
            limit: 50,
            ..mcpd_store::AspectNoteQuery::default()
        })
        .await
        .expect("everybody else's notes");
    a.insert(
        "erasing a subject leaves the notes about everybody else",
        others.items.len().to_string(),
    );

    let labelled = store
        .list_themes(&mcpd_store::ThemeQuery {
            project: project.clone(),
            snapshot_id: Some(snapshot_id.clone()),
            theme_id: Some("t-refund".into()),
            limit: 10,
            ..mcpd_store::ThemeQuery::default()
        })
        .await
        .expect("the theme labelled from the erased run");
    a.insert(
        "erasing a subject empties a theme label taken from their run",
        labelled.items.first().map_or_else(
            || "(none)".to_owned(),
            |t| (t.theme.label == mcpd_store::ERASED_ITEM).to_string(),
        ),
    );

    let left = store
        .list_theme_members(&mcpd_store::ThemeMemberQuery {
            project: project.clone(),
            snapshot_id,
            aspect: Some("goal".into()),
            theme_id: Some(mcpd_store::NOISE_THEME_ID.into()),
            limit: 50,
            cursor: None,
        })
        .await
        .expect("the memberships of the erased run");
    a.insert(
        "erasing a subject drops the memberships of their notes",
        left.items.len().to_string(),
    );

    a
}

async fn observe_retention(store: &dyn Store, project: String) -> Answers {
    let mut a = Answers::new();
    let now = OffsetDateTime::now_utc();
    let old = now - Duration::days(40);

    let log = LogRecord {
        log_id: format!("{project}-l1"),
        trace_id: Some(trace_id(&project, "kept")),
        span_id: None,
        timestamp: old,
        level: LogLevel::Info,
        message: "an old line".into(),
        fields: None,
        logger: None,
        correlation: only_project(&project),
        received_at: OffsetDateTime::UNIX_EPOCH,
    };
    store
        .ingest(Batch {
            spans: vec![root(&project, "kept", old, only_project(&project))],
            logs: vec![log],
            ..Batch::default()
        })
        .await
        .expect("the retention fixture must ingest");

    // Logs alone. A window on one signal must not reach any other, which is
    // exactly what one cutoff for the whole project could never express.
    let logs_only = RetentionPolicy { logs: Some(7), ..RetentionPolicy::default() };
    let first = store
        .apply_retention(&project, &logs_only, now)
        .await
        .expect("a per-signal retention pass");
    a.insert("expiring logs alone leaves the runs", run_count(store, &project).await.to_string());
    a.insert("expiring logs alone removes the logs", log_count(store, &project).await.to_string());
    // A per-project pass must never declare anything: a declared retention is a
    // property of a table, and every project shares the tables. One project's
    // window written onto them would expire every other tenant's data.
    a.insert("a per-project pass declares nothing", first.ttl_changed.to_string());

    // The same pass again. Nothing is left to expire, so it must remove
    // nothing. This is half of the idempotency the requirement asks for,
    // measured rather than asserted in a comment.
    let second =
        store.apply_retention(&project, &logs_only, now).await.expect("a second retention pass");
    a.insert("a second identical retention pass removes nothing", second.removed.to_string());

    // The other half: the deployment-wide window, handed to the engine. The
    // first call changes the tables and the second changes nothing, which is
    // what stops a replicated deployment issuing the same metadata statement on
    // every retention tick.
    //
    // A year, not a month. The fixture's run started forty days ago, and a
    // backend that honours a declared window honours it at once: ClickHouse
    // rewrites the parts when a TTL is modified, so a thirty day window would
    // expire the run here and the empty-policy case below would then be
    // counting what the declaration removed rather than what the empty policy
    // kept. The declaration is under test for whether it changes the tables,
    // not for what it expires.
    let declared = RetentionPolicy::uniform(365);
    let declare = |r: mcpd_store::Result<mcpd_store::RetentionOutcome>| match r {
        Ok(o) => o.ttl_changed.to_string(),
        Err(mcpd_store::Error::Unsupported { capability, .. }) => {
            format!("unsupported:{capability}")
        }
        Err(e) => format!("error:{e}"),
    };
    a.insert(
        "declaring a window changes the tables the first time",
        declare(store.declare_retention(&declared).await),
    );
    a.insert(
        "declaring the same window again changes nothing",
        declare(store.declare_retention(&declared).await),
    );
    // The advertisement has to match the behaviour, in both directions. Without
    // this a backend could claim `declarative_retention` and refuse, and the
    // two cases above would still read as documented because the refusal is
    // what they expect from a backend that denies it.
    let answered =
        !a["declaring the same window again changes nothing"].starts_with("unsupported:");
    a.insert(
        "declarative retention answers exactly when the capability says it does",
        (store.capabilities().declarative_retention == answered).to_string(),
    );

    // A policy with no window does nothing at all, which is what an operator
    // who has set no retention has asked for.
    let nothing = store
        .apply_retention(&project, &RetentionPolicy::default(), now)
        .await
        .expect("an empty policy");
    a.insert(
        "a policy with no window keeps everything",
        format!("{},{}", nothing.removed, run_count(store, &project).await),
    );

    // Now the runs. Their steps go with them: a run whose steps have expired is
    // a row that lists a duration and opens onto nothing.
    let runs_too = RetentionPolicy { traces: Some(7), ..logs_only };
    store.apply_retention(&project, &runs_too, now).await.expect("expiring the runs");
    a.insert("expiring runs removes them", run_count(store, &project).await.to_string());
    a
}

/* ------------------------------------------------------------- the harness -- */

/// A name no other run of this suite will produce.
///
/// Postgres and ClickHouse keep what earlier runs wrote, so a fixed project
/// name would make the second run of this file read the first run's rows.
fn unique(prefix: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    format!(
        "{prefix}-{}-{}-{}",
        std::process::id(),
        OffsetDateTime::now_utc().unix_timestamp_nanos(),
        N.fetch_add(1, Ordering::Relaxed)
    )
}

/// Fail rather than skip when a job that started a service loses the variable
/// pointing at it.
///
/// SQLite is always reachable, so with no URL this file still reports every
/// case green having proved on one backend what it exists to prove on three.
/// That silence is worse than a failure, and it is the silence six suites here
/// were already reporting before the same guard was added to them.
///
/// It asks for one live URL rather than naming which: the Postgres job supplies
/// Postgres and the ClickHouse job supplies ClickHouse, and neither has the
/// other.
fn require_live() {
    if std::env::var("MCPD_REQUIRE_LIVE").is_err() {
        return;
    }
    let names = ["MCPD_TEST_POSTGRES_URL", "MCPD_TEST_CLICKHOUSE_URL"];
    assert!(
        names.iter().any(|n| std::env::var(n).is_ok()),
        "MCPD_REQUIRE_LIVE is set but neither MCPD_TEST_POSTGRES_URL nor \
         MCPD_TEST_CLICKHOUSE_URL is, so this suite would have reported green having \
         reached SQLite and nothing else. That is the exact silence this variable exists \
         to break."
    );
}

/// Every backend this build can reach, each already migrated.
async fn backends() -> (Vec<(&'static str, Box<dyn Store>)>, Vec<&'static str>) {
    require_live();
    let mut reached: Vec<(&'static str, Box<dyn Store>)> = Vec::new();
    let mut missing = Vec::new();

    let dir = std::env::temp_dir().join(format!(
        "mcpd-conformance-{}-{}",
        std::process::id(),
        unique("run")
    ));
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let sqlite = SqliteStore::open(dir.join("store.db"), 2).expect("open sqlite");
    sqlite.migrate().await.expect("migrate sqlite");
    reached.push(("sqlite", Box::new(sqlite)));

    match std::env::var("MCPD_TEST_POSTGRES_URL") {
        Ok(url) => {
            let pg = PostgresStore::new(PostgresConfig { url, ..Default::default() })
                .expect("build the postgres store");
            pg.migrate().await.expect("migrate postgres");
            reached.push(("postgres", Box::new(pg)));
        }
        Err(_) => missing.push("postgres"),
    }

    match std::env::var("MCPD_TEST_CLICKHOUSE_URL") {
        Ok(url) => {
            // The database name carries the pid alone, and a pid is recycled,
            // so a later run of this file can open a database an earlier one
            // wrote. That is safe here and nowhere else: every query below is
            // scoped to a project this run invented, and the trace ids are
            // derived from that project, so nothing an earlier run left behind
            // is reachable. `tests/clickhouse.rs` has the same name and fixed
            // trace ids, and reads its predecessor's rows.
            let ch = ClickHouseStore::new(ClickHouseConfig {
                url,
                database: format!("mcpd_conformance_{}", std::process::id()),
                ..Default::default()
            })
            .expect("build the clickhouse store");
            ch.migrate().await.expect("migrate clickhouse");
            reached.push(("clickhouse", Box::new(ch)));
        }
        Err(_) => missing.push("clickhouse"),
    }

    (reached, missing)
}

/// Compare what each backend said against what the specification says, audit
/// the gap list, and describe the run.
fn report(
    expected: &Answers,
    observed: &BTreeMap<&'static str, Answers>,
    missing: &[&'static str],
) -> Vec<String> {
    let mut failures = Vec::new();
    let mut gaps = Vec::new();
    let mut fired: Vec<(&str, &str)> = Vec::new();

    for (backend, answers) in observed {
        // The cases with one answer for every backend, and then the ones the
        // specification gives a different answer for on this backend. A case
        // must be in exactly one of the two: a case in both would have two
        // documented answers and the run would silently prefer one of them.
        let mut expected = expected.clone();
        for (case, want) in documented_capabilities(backend) {
            if let Some(common) = expected.insert(case, want) {
                failures.push(format!(
                    "{case:?} is documented twice, as {common:?} for every backend and again \
                     for {backend} alone. Delete one: a case with two documented answers is a \
                     case that proves whichever one it happens to compare against."
                ));
            }
        }
        // Every backend that ran must answer every case, including the ones it
        // cannot do. Those answer with the refusal, which is a value, and never
        // by not being asked.
        for (case, want) in &expected {
            let Some(got) = answers.get(case) else {
                failures.push(format!("{backend}: no answer recorded for {case:?}"));
                continue;
            };
            if got == want {
                continue;
            }
            let line = format!("{backend}: {case}\n      wanted {want:?}, got {got:?}");
            match KNOWN_GAPS.iter().find(|(b, c, _)| b == backend && c == case) {
                Some((_, _, why)) => {
                    fired.push((backend, case));
                    gaps.push(format!("{line}\n      {why}"));
                }
                None => failures.push(line),
            }
        }
    }

    // The audit. An entry naming a case nobody asks is a typo, and an entry
    // whose case now passes is a fix nobody recorded: both are failures,
    // because a suppression with nothing under it is exactly where the next
    // real regression lands unreported.
    for (backend, case, _) in KNOWN_GAPS {
        if !expected.contains_key(case) && !documented_capabilities(backend).contains_key(case) {
            failures.push(format!(
                "KNOWN_GAPS names the case {case:?}, which this suite does not ask. Rename \
                 the entry to a case in documented_answers, or delete it: an entry that \
                 matches no case suppresses nothing and reads as coverage that is not there."
            ));
            continue;
        }
        // A backend that did not run says nothing about its gaps.
        if !observed.contains_key(backend) {
            continue;
        }
        if !fired.iter().any(|(b, c)| b == backend && c == case) {
            failures.push(format!(
                "KNOWN_GAPS still names {backend} under {case:?}, and that case now passes. \
                 Delete the entry from crates/mcpd-store/tests/conformance.rs. Deleting it \
                 is how the fix is proved: the case fails again the moment the behaviour \
                 comes back."
            ));
        }
    }

    // Counted over the union, so the number a skipped backend is reported as
    // not having proved includes the cases documented for it alone.
    let total = expected.len() + documented_capabilities("sqlite").len();
    println!(
        "\nconformance: {total} case(s) against {} backend(s): {}",
        observed.len(),
        observed.keys().copied().collect::<Vec<_>>().join(", ")
    );
    if !gaps.is_empty() {
        println!("\n{} known difference(s), each with a work item:\n", gaps.len());
        for g in &gaps {
            println!("  !  {g}");
        }
    }
    if !missing.is_empty() {
        // Loud, not silent. A skipped backend is a backend this run proved
        // nothing about, and the number of cases says how much was not proved.
        println!(
            "\n{}\nSKIPPED: {} backend(s) were not reached: {}.\n\
             All {} case(s) above are UNPROVEN on each of them. Set {} to include them.\n{}",
            "=".repeat(78),
            missing.len(),
            missing.join(", "),
            total,
            missing
                .iter()
                .map(|b| match *b {
                    "postgres" => "MCPD_TEST_POSTGRES_URL",
                    _ => "MCPD_TEST_CLICKHOUSE_URL",
                })
                .collect::<Vec<_>>()
                .join(" and "),
            "=".repeat(78),
        );
    }
    failures
}

#[tokio::test]
async fn every_backend_gives_the_documented_answer_to_the_same_question() {
    let (reached, missing) = backends().await;
    let expected = documented_answers();
    let mut observed: BTreeMap<&'static str, Answers> = BTreeMap::new();

    for (name, store) in &reached {
        let mut answers = observe(store.as_ref(), unique(&format!("conf-{name}"))).await;
        answers.extend(observe_replay(store.as_ref(), unique(&format!("key-{name}"))).await);
        answers.extend(observe_out_of_order(store.as_ref(), unique(&format!("ooo-{name}"))).await);
        answers.extend(observe_capabilities(store.as_ref()));
        answers.extend(observe_discovery(store.as_ref(), unique(&format!("disc-{name}"))).await);
        answers.extend(observe_polling(store.as_ref(), unique(&format!("poll-{name}"))).await);
        answers.extend(observe_retention(store.as_ref(), unique(&format!("ret-{name}"))).await);
        answers.extend(observe_themes(store.as_ref(), unique(&format!("thm-{name}"))).await);
        answers.extend(
            observe_tenancy(
                store.as_ref(),
                unique(&format!("ten-a-{name}")),
                unique(&format!("ten-b-{name}")),
            )
            .await,
        );
        observed.insert(name, answers);
    }

    let failures = report(&expected, &observed, &missing);
    assert!(
        failures.is_empty(),
        "\n{} backend(s) disagree with the specification:\n\n  x  {}\n",
        failures.len(),
        failures.join("\n\n  x  ")
    );
}
