//! `mcpd doctor`: check a deployment and say what is wrong in an operator's
//! terms.
//!
//! The failures that hurt a team running mcpd are the quiet ones. A key list
//! with a stray space in it, so the key that was handed out never works. A
//! `projects` block naming a project that does not exist, so its retention
//! window sweeps nothing. A forward endpoint that has been refusing every batch
//! since it was configured. None of these stop the daemon, none of them appear
//! in a log anybody reads, and each one is only discovered when somebody needs
//! the data that was never kept.
//!
//! This command turns them into a list. Every finding is one of `ok`, a
//! `warning`, or a `fault`, and every warning and fault names the flag or the
//! config key that fixes it. A fault exits 2, so a deployment pipeline can gate
//! on it. A warning never changes the exit code, because a warning that fails a
//! deploy is a warning people learn to switch off.
//!
//! ## What it will not do
//!
//! It reports `skipped` rather than `ok` for anything it did not actually test.
//! A doctor that says a judge is healthy because a judge is configured is worse
//! than one that stays quiet, because the reader believes it. So:
//!
//! * The judge is checked as configuration only. Proving a judge answers means
//!   spending money on a completion, which a diagnostic command must not do
//!   without being asked.
//! * Each forwarding destination is checked by sending it an export with
//!   nothing in it. That proves the URL, the network path and the credential,
//!   and it writes nothing downstream. It does not prove a batch with spans in
//!   it is accepted. What the destination's spool holds is read from disk.
//! * Free disk space is not checked at all. There is no way to read it from the
//!   standard library, and this crate does not carry a dependency that can. The
//!   honest stand-in is under `retention`, which reports how far back each
//!   project's data goes and whether anything will ever delete it.
//! * The schema version is not checked. It is private to the store and there is
//!   no method on the `Store` trait that reports it. `mcpd` applies migrations
//!   on every start, and a failure there stops the process before this runs.
//! * A subsystem the configuration does not mention is `skipped`, not `ok`.
//!   Retention, forwarding, the judge and the monitors all read as absent when
//!   `--config-file` names the wrong file, and a clean bill of health for three
//!   broken subsystems is the worst thing this command could print. The header
//!   names the file that was read, so you can see which deployment was graded.
//! * `--offline` means one thing: no request to any service other than the
//!   store. The store is still read, and written unless `--read-only` is given
//!   too, and against Postgres or ClickHouse that is network traffic. The flag
//!   used to say "make no network request", which was untrue. It was reworded
//!   rather than narrowed, because a doctor that skips the store as well has
//!   nothing left to say about the deployment it was pointed at.

use anyhow::Result;
use mcpd_core::cost::PriceBook;
use mcpd_server::monitor::Watch;
use mcpd_server::ServerConfig;
use mcpd_store::{Order, ScoreQuery, SpanQuery, Store, TraceQuery, TraceSort};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use time::{Duration, OffsetDateTime};

/// The store this command writes its probe span into, and deletes it from
/// again. A project of its own so a probe can never land in a real project's
/// analytics, even if the delete fails.
const PROBE_PROJECT: &str = "_mcpd_doctor";

/// How many runs the clock check reads from each end of a project's history.
///
/// The newest run alone is a single sample, and one run whose producer had a
/// wrong clock would be reported as if the whole deployment were skewed. A page
/// is enough to see whether the skew is the deployment or one sender.
///
/// Both ends, because the only ordering the store offers is by `start_time`,
/// which is the field under suspicion. A sender whose clock runs ahead sorts to
/// the top of a descending page and a sender whose clock runs behind sorts to
/// the bottom, so reading only the newest page is how the skew this check
/// exists to find escapes it.
const CLOCK_SAMPLE: u32 = 50;

/// How long the write probe keeps trying before it reports that the store is
/// not taking writes.
///
/// A wall-clock budget, not a count of attempts. One attempt returns in a
/// millisecond against Postgres and sits on SQLite's own five second busy
/// timeout, so "three attempts" was anything between a moment and a quarter of
/// a minute, and the number in the report told an operator nothing about how
/// long the store had been refusing. Ten seconds is far longer than any single
/// write a healthy daemon holds the lock for, and short enough that a deploy
/// gate waiting on it is not what anybody blames.
const PROBE_BUDGET: std::time::Duration = std::time::Duration::from_secs(10);

/// The pause after the first refused write. Each later pause doubles, up to
/// `PROBE_BACKOFF_MAX`.
///
/// It grows because a writer that retries on a fixed short interval against a
/// contended store is part of the contention it is trying to measure.
const PROBE_BACKOFF: std::time::Duration = std::time::Duration::from_millis(100);

/// The longest pause between two attempts, so a long budget is still spent on
/// attempts spread across it rather than on one long sleep at the end.
const PROBE_BACKOFF_MAX: std::time::Duration = std::time::Duration::from_secs(2);

/// How many leftover probe traces one run will clear.
///
/// A probe is one trace and the delete almost always works, so anything above
/// one is an earlier run that failed. A hundred is far past that and bounds the
/// work when something is deleting nothing at all.
const PROBE_SWEEP: u32 = 100;

/// The widest `--window-days` accepted, a hundred years.
///
/// Not a preference. `now - window` leaves the range `OffsetDateTime` can hold
/// long before `u32` runs out, and the subtraction panics before a line of the
/// report is printed.
const MAX_WINDOW_DAYS: u32 = 36_525;

/// How many recent spans the price-book check reads per project.
const PRICE_SAMPLE: u32 = 500;

/// How far apart a run's start and its arrival may be before it is reported.
///
/// Five minutes is far past any plausible queue or batch delay, and far short
/// of the hour-sized errors a wrong time zone or an unsynchronised clock
/// produces.
const CLOCK_TOLERANCE: Duration = Duration::minutes(5);

/// How old the oldest run may be, with no retention configured, before it is
/// worth saying so.
const UNBOUNDED_AGE: Duration = Duration::days(90);

/// A key shorter than this is worth mentioning. It is advice, not a rule, so it
/// is a warning and it is aggregated into one line.
const SHORT_KEY: usize = 16;

/// Groups, in the order they are printed. Storage first because nothing below
/// it is trustworthy if storage is broken.
const GROUPS: [&str; 12] = [
    "storage",
    "runs",
    "keys",
    "identity",
    "projects",
    "retention",
    "limits",
    "forwarding",
    "judge",
    "monitors",
    "clock",
    "pricing",
];

/// What `mcpd doctor` checks, and what it is allowed to do while checking.
#[derive(Debug, Clone, clap::Args)]
pub struct Args {
    /// Contact no service other than the store. The forwarding destinations
    /// are then reported as skipped rather than checked.
    ///
    /// The store is still read, and still written unless --read-only is given
    /// too. A remote store is reached over the network like anything else, and
    /// skipping it would leave nothing to report.
    #[arg(long)]
    pub offline: bool,

    /// Write nothing. The storage check then proves the store can be read and
    /// not that it can be written, and says so.
    #[arg(long)]
    pub read_only: bool,

    /// How many days of history the activity, clock and price-book checks
    /// read. Between 1 and 36525.
    #[arg(long, default_value_t = 7)]
    pub window_days: u32,
}

/// How much attention one finding deserves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// Checked, and correct.
    Ok,
    /// Not checked. Never reported as `ok`.
    Skipped,
    /// Works, and is probably not what you meant.
    Warn,
    /// Broken, or open when it should not be. This is what exits 2.
    ///
    /// One condition produces it for openness rather than breakage: the read
    /// scope requires no key and the daemon does not listen on a loopback
    /// address, so stored prompts and completions are readable by anyone who
    /// can reach the port. That is a disclosure nobody can take back, so it
    /// gates a deploy. An open ingest scope on the same address stays a
    /// warning: it costs rows, and retention removes rows.
    Fault,
}

impl Level {
    /// The label printed in the left column, padded so the messages line up.
    const fn tag(self) -> &'static str {
        match self {
            Self::Ok => "ok     ",
            Self::Skipped => "skipped",
            Self::Warn => "warning",
            Self::Fault => "FAULT  ",
        }
    }
}

/// One line of the report.
#[derive(Debug, Clone)]
pub struct Finding {
    /// Which group it prints under.
    pub group: &'static str,
    /// How much attention it deserves.
    pub level: Level,
    /// What is true, and for anything but `ok`, what to change.
    pub message: String,
}

impl Finding {
    fn new(group: &'static str, level: Level, message: impl Into<String>) -> Self {
        Self { group, level, message: message.into() }
    }
    fn ok(group: &'static str, m: impl Into<String>) -> Self {
        Self::new(group, Level::Ok, m)
    }
    fn skipped(group: &'static str, m: impl Into<String>) -> Self {
        Self::new(group, Level::Skipped, m)
    }
    fn warn(group: &'static str, m: impl Into<String>) -> Self {
        Self::new(group, Level::Warn, m)
    }
    fn fault(group: &'static str, m: impl Into<String>) -> Self {
        Self::new(group, Level::Fault, m)
    }
}

/// Run every check, print the report, and exit 2 if anything is a fault.
///
/// `listen` is the address the daemon would bind, which decides whether an open
/// scope is the laptop default or a hole. `blank_lists` is what
/// [`check_blank_key_lists`] found before `main` dropped the blank entries that
/// `config` no longer holds. `store` is already open and migrated:
/// this is dispatched after `main` has built it, so a store that cannot be
/// opened at all fails before it reaches here, with `main`'s error rather than
/// a finding.
///
/// # Errors
/// Returns an error only when a check could not be run at all. Anything the
/// deployment got wrong is a finding, not an error, because the report is the
/// point of the command.
pub async fn run(
    args: &Args,
    config: &ServerConfig,
    blank_lists: Vec<Finding>,
    store: &Arc<dyn Store>,
    runs: Option<&Arc<dyn mcpd_runs::RunStore>>,
    listen: &str,
    book: &PriceBook,
) -> Result<()> {
    // Before anything is read or printed, because the failure is a panic rather
    // than a bad report.
    let window = window(args)?;
    let now = OffsetDateTime::now_utc();
    let mut findings = Vec::new();

    // The project list is read before the write probe, which is what the
    // comment here has always claimed and what the order of the calls did not
    // do. Findings print by group, so reading it first moves nothing in the
    // report.
    let projects = match store.list_projects().await {
        Ok(names) => Some(gradable(names)),
        Err(e) => {
            findings.push(Finding::fault(
                "projects",
                format!(
                    "the project list could not be read: {e}. Every check that compares the \
                     configuration against the projects that hold runs is skipped below. An \
                     empty list reads as `every configured project is a typo`, which is five \
                     confident statements about a call that failed."
                ),
            ));
            None
        }
    };

    findings.extend(check_storage(store.as_ref(), args, window, now).await);
    findings.extend(check_runs(config, runs.map(AsRef::as_ref), listen).await);
    findings.extend(check_keys(blank_lists, config, listen));
    if runs.is_some() {
        findings.extend(check_run_key_reach(config));
    }
    findings.extend(check_identity(config, listen));
    findings.extend(check_limits(config));
    findings.extend(check_forwarding(config, &probe_forward(config, args).await));
    findings.extend(check_judge(config));

    match &projects {
        Some(with_data) => {
            let activity = gather_activity(store.as_ref(), with_data, window, now).await;
            findings.extend(check_projects(config, with_data));
            findings.extend(check_retention(config, Some(with_data), &activity, now));
            findings.extend(check_monitors(
                config,
                Some(with_data),
                &activity,
                &empty_scorers(store.as_ref(), config).await,
                window,
            ));
            findings.extend(check_clock(&activity, now));

            let (considered, unpriced) =
                unpriced_models(store.as_ref(), with_data, book, now - window, now).await;
            findings.push(check_pricing(considered, &unpriced, window));
        }
        None => {
            findings.extend(check_retention(config, None, &BTreeMap::new(), now));
            findings.extend(check_monitors(
                config,
                None,
                &BTreeMap::new(),
                &BTreeSet::new(),
                window,
            ));
            findings.push(Finding::skipped(
                "projects",
                "which projects hold runs was not read, so no configured name was compared \
                 against one.",
            ));
            findings.push(Finding::skipped(
                "clock",
                "no run was read, so no sender's clock was compared with this machine's.",
            ));
            findings.push(Finding::skipped(
                "pricing",
                "no span was read, so no model was looked up in the price book.",
            ));
        }
    }

    print_report(&findings, &resolved_inputs(store.as_ref(), listen));

    if findings.iter().any(|f| f.level == Level::Fault) {
        // Exit 2 rather than 1, so a pipeline can tell "the deployment has a
        // fault" from "doctor could not run", which leaves through `main` as 1.
        std::process::exit(2);
    }
    Ok(())
}

/// The window the activity, clock and price-book checks read.
///
/// # Errors
/// Returns an error naming the limit when the window is outside it. The top of
/// the `u32` range is not a wide window, it is a panic: `now - window` leaves
/// the range `OffsetDateTime` can represent, and the process dies before a line
/// of the report is printed.
fn window(args: &Args) -> Result<Duration> {
    if !(1..=MAX_WINDOW_DAYS).contains(&args.window_days) {
        anyhow::bail!(
            "--window-days must be between 1 and {MAX_WINDOW_DAYS}, got {}",
            args.window_days
        );
    }
    Ok(Duration::days(i64::from(args.window_days)))
}

/// The projects to grade: every project the store holds, less the probe's own.
///
/// Unconditionally, not only when the delete succeeded. A probe left behind by
/// a run that could not clean up is this command's litter, and grading it as a
/// project somebody created would make doctor report on itself.
fn gradable(names: Vec<String>) -> BTreeSet<String> {
    names.into_iter().filter(|n| n != PROBE_PROJECT).collect()
}

// ---------------------------------------------------------------- preflight --

/// The half of the report that reads the configuration and nothing else, for
/// the daemon to run on every start.
///
/// `mcpd doctor` is a command you have to know exists, and the mistakes it
/// finds are the quiet ones, so the operator who most needs the report is the
/// one who never runs it. A `judge` block with no criteria is the clearest
/// case: every judged scorer declines for ever with `is not a configured
/// criterion`, no score is ever produced, and the daemon serving it answers its
/// health check and looks well from every angle. Running the configuration-only
/// checks at startup costs microseconds and closes that gap.
///
/// # What is in it, and what is not
///
/// Only checks that need no network, no store round trip and no running daemon,
/// because a start must not wait on any of those. That is
/// [`check_key_entries`], [`check_key_reach`], [`check_run_key_reach`],
/// [`check_identity`], [`check_limits`] and [`check_judge`].
///
/// Everything else in the report reads a store: `check_storage`,
/// `check_projects`, `check_retention`, `check_monitors`, `check_clock`,
/// `check_pricing` and `check_runs`. Against Postgres or ClickHouse each of
/// those is a network round trip, and `check_forwarding` is graded on a POST to
/// somebody else's collector. Those stay in `mcpd doctor`, where the operator
/// asked for them.
///
/// [`check_open_scopes`] is left out for a different reason, written on that
/// function.
///
/// # What refuses to start, and what only warns
///
/// A [`Level::Fault`] refuses; a [`Level::Warn`] is logged and the daemon
/// carries on. `Ok` and `Skipped` are dropped: a start is not a report, and a
/// line saying a subsystem is absent is noise in a log that runs for months.
///
/// The rule is the one `mcpd_server::identity` already applies, and it is the
/// reason that module refuses a half-configured trusted identity at startup: a
/// feature that silently does nothing is worse than one that fails, because
/// nobody goes looking for it. Every fault this returns has that shape.
///
/// * A key entry that is still the placeholder from an example file is a key
///   published in this repository, so its scope is open to anyone who read
///   the file. One with whitespace at an end can never match, so the person
///   holding it is refused for ever. A blank entry is not here: `main` drops
///   it before this runs. A list that held only blank entries is refused by
///   [`check_blank_key_lists`], which `main` calls before the drop.
/// * A trusted range containing the whole internet puts a name anybody can
///   choose on every audit row, which is worse than the empty column it
///   replaced. Off a loopback address only, which is the judgement
///   `check_identity` already makes.
/// * A project `sample` outside 0.0 to 1.0 is already refused when it arrives
///   as `--sample`, so refusing it here only makes the two ways in agree.
/// * A judge that can never answer, for any of the three reasons
///   `check_judge` names.
///
/// A warning is the other case, and it is the larger one: something that works
/// and is probably not what you meant. A key short enough to guess. A key
/// listed under two projects, so the two are not separated. A project whose
/// ingest key cannot read the data it wrote. A ceiling smaller than one legal
/// batch. Each of those is a deployment somebody could have chosen on purpose,
/// so refusing would be mcpd overruling its operator, and the daemon says so
/// and starts.
///
/// `serves_runs` says whether this daemon has a run store. The run scopes guard
/// nothing on a daemon that answers every run endpoint with 503, so what they
/// inherit is only worth saying when there is something for them to open.
#[must_use]
pub fn preflight(config: &ServerConfig, listen: &str, serves_runs: bool) -> Vec<Finding> {
    let mut out = check_key_entries(config);
    out.extend(check_key_reach(config));
    if serves_runs {
        out.extend(check_run_key_reach(config));
    }
    out.extend(check_identity(config, listen));
    out.extend(check_limits(config));
    out.extend(check_judge(config));
    out.retain(|f| matches!(f.level, Level::Warn | Level::Fault));
    out
}

// ------------------------------------------------------------------- inputs --

/// What the report was computed from.
///
/// A doctor pointed at the wrong config file grades a deployment that is not
/// the one in front of you, and every subsystem that file configures then reads
/// as one nobody turned on. Printing the inputs is how a reader notices. They
/// are laid out the way `mcpd config` lays out its resolved settings, so the
/// two commands read alike.
struct Inputs {
    store: String,
    config_file: String,
    listen: String,
}

/// The store, the config file and the listen address this run graded.
fn resolved_inputs(store: &dyn Store, listen: &str) -> Inputs {
    let argv: Vec<String> = std::env::args().collect();
    let (flag, env) = match store.name() {
        "clickhouse" => ("--clickhouse-url", "MCPD_CLICKHOUSE_URL"),
        "postgres" => ("--postgres-url", "MCPD_POSTGRES_URL"),
        _ => ("--data", "MCPD_DATA"),
    };
    Inputs {
        store: cli_value(&argv, flag, env).map_or_else(
            || format!("{} (no {flag} given)", store.name()),
            |v| format!("{} at {}", store.name(), without_credentials(&v)),
        ),
        config_file: cli_value(&argv, "--config-file", "MCPD_CONFIG_FILE")
            .unwrap_or_else(|| "none given".to_owned()),
        listen: listen.to_owned(),
    }
}

/// The value a global flag was given, read back from the process's own
/// arguments.
///
/// `main` parses these into a `ServerConfig` and keeps neither the path of the
/// config file nor the location of the store, and doctor wants them only to say
/// what it graded. Reading the arguments back is exact for the two forms clap
/// accepts, `--flag value` and `--flag=value`, and falls back to the
/// environment variable clap would have read instead.
fn cli_value(argv: &[String], flag: &str, env: &str) -> Option<String> {
    let mut it = argv.iter();
    while let Some(a) = it.next() {
        if a == flag {
            return it.next().cloned();
        }
        if let Some(v) = a.strip_prefix(flag).and_then(|r| r.strip_prefix('=')) {
            return Some(v.to_owned());
        }
    }
    std::env::var(env).ok().filter(|v| !v.is_empty())
}

/// A URL or a connection string with any credentials taken out of it.
///
/// Everything that prints a store, a forward endpoint or a judge endpoint goes
/// through this: `mcpd config`, the start-up log, and the doctor findings people
/// paste into tickets. The scheme is kept, because the line names a URL you
/// compare against a config file.
///
/// Three shapes used to defeat it, and all are the kind that ends up in a
/// ticket. A credential in a query parameter went out in full, because the rule
/// only ever looked before an `@`. An `@` inside a query string on a URL with
/// no path was read as the end of the credentials, so
/// `https://host?to=a@b.example` printed as `https://b.example`: the host was
/// gone and the reader was comparing a URL that does not exist against their
/// configuration. And a Postgres connection string in its `key=value` form,
/// `host=db user=mcpd password=...`, has no `@` at all, so `main`'s own rule of
/// printing the part after the `@` printed the password.
///
/// A fourth was a password holding a `/`, `?`, `#` or `@`. Nobody escapes a
/// password they paste into a URL, and the first three end the authority, so
/// `postgres://mcpd:s3cr/etpw@db/mcpd` printed with the password in it. A URL
/// with credentials now loses everything up to its last `@`. When its path or
/// query holds an `@` as well, the host goes with it and the line names less
/// than it could. That is the safe way to be wrong: the two cannot be told
/// apart, and the other way prints a password.
pub fn without_credentials(url: &str) -> String {
    if !url.contains("://") && url.contains('=') {
        return redact_key_values(url);
    }
    let Some((scheme, rest)) = url.split_once("//") else { return redact_query(url) };
    let rest = if carries_credentials(rest) {
        rest.rsplit_once('@').map_or(rest, |(_, after)| after)
    } else {
        rest
    };
    // The authority ends at the first `/`, `?` or `#`, not at the first `/`
    // alone. That is the whole of the second bug: with no path, the query
    // string was inside what this treated as the authority.
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (host, tail) = rest.split_at(end);
    format!("{scheme}//{host}{}", redact_query(tail))
}

/// Whether what follows a URL's `//` starts with a user name and password.
///
/// An `@` before the first `/`, `?` or `#` is the ordinary case. The other is a
/// password holding one of those three. What reads as the authority is then
/// `user:part-of-the-password`, with the `@` further on. A host with a port and
/// an `@` later in its path reads exactly the same, and is taken as credentials
/// too, because printing too little is the safe mistake. An `@` after an
/// authority with no colon, as in `https://host?to=a@b.example`, is not a
/// credential, and the host stays.
fn carries_credentials(rest: &str) -> bool {
    let Some(first_at) = rest.find('@') else { return false };
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    if first_at < end {
        return true;
    }
    // An IPv6 literal is full of colons and is never a user name, so only a
    // colon after it counts.
    let authority = &rest[..end];
    let outside_literal = authority
        .strip_prefix('[')
        .map_or(authority, |r| r.split_once(']').map_or("", |(_, port)| port));
    outside_literal.contains(':')
}

/// The same string with the value of any query parameter that names a
/// credential replaced.
///
/// A token in a query parameter is a credential in a URL exactly as much as one
/// before an `@`, and it is the shape a judge endpoint is most likely to carry.
/// The name is matched generously and the value is never inspected: redacting a
/// version number costs the reader nothing, and printing a key costs them the
/// key.
fn redact_query(tail: &str) -> String {
    let Some((before, rest)) = tail.split_once('?') else { return tail.to_owned() };
    let (query, fragment) = rest.split_once('#').map_or((rest, None), |(q, f)| (q, Some(f)));
    let mut out = String::with_capacity(tail.len());
    out.push_str(before);
    out.push('?');
    for (n, pair) in query.split('&').enumerate() {
        if n > 0 {
            out.push('&');
        }
        match pair.split_once('=') {
            Some((name, _)) if names_a_credential(name) => {
                out.push_str(name);
                out.push_str("=[redacted]");
            }
            _ => out.push_str(pair),
        }
    }
    if let Some(f) = fragment {
        out.push('#');
        out.push_str(f);
    }
    out
}

/// A libpq `key=value` connection string with the value of every key that names
/// a credential replaced.
///
/// Values may be single-quoted with a backslash escape, which is how libpq
/// writes a password that holds a space, so a quoted value is read as one
/// value. A word that is not a pair is not valid in this form and is replaced
/// too: an unquoted password with a space in it arrives as a pair followed by
/// a bare word, and the bare word is the rest of the password.
fn redact_key_values(conn: &str) -> String {
    let mut out = Vec::new();
    let mut chars = conn.chars().peekable();
    let skip_space = |chars: &mut std::iter::Peekable<std::str::Chars<'_>>| {
        while chars.peek().is_some_and(|c| c.is_whitespace()) {
            chars.next();
        }
    };
    loop {
        skip_space(&mut chars);
        if chars.peek().is_none() {
            break;
        }
        let mut key = String::new();
        while let Some(&c) = chars.peek() {
            if c == '=' || c.is_whitespace() {
                break;
            }
            key.push(c);
            chars.next();
        }
        skip_space(&mut chars);
        if chars.peek() != Some(&'=') {
            out.push("[redacted]".to_owned());
            continue;
        }
        chars.next();
        skip_space(&mut chars);
        let mut value = String::new();
        if chars.peek() == Some(&'\'') {
            value.push('\'');
            chars.next();
            while let Some(c) = chars.next() {
                value.push(c);
                if c == '\\' {
                    if let Some(escaped) = chars.next() {
                        value.push(escaped);
                    }
                } else if c == '\'' {
                    break;
                }
            }
        } else {
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() {
                    break;
                }
                value.push(c);
                chars.next();
            }
        }
        if names_a_credential(&key) {
            out.push(format!("{key}=[redacted]"));
        } else {
            out.push(format!("{key}={value}"));
        }
    }
    out.join(" ")
}

/// Whether a query parameter's name says its value is a credential.
///
/// Substrings rather than whole names, because the same secret arrives as
/// `key`, `api_key`, `apiKey` and `subscription-key` depending on who wrote the
/// endpoint. `api-version` matches none of them, which is the case worth
/// keeping readable.
fn names_a_credential(name: &str) -> bool {
    const NEEDLES: [&str; 9] =
        ["key", "secret", "token", "password", "passwd", "pwd", "credential", "auth", "signature"];
    let lower = name.to_ascii_lowercase();
    NEEDLES.iter().any(|needle| lower.contains(needle))
}

// ------------------------------------------------------------------ storage --

/// Ping the store, and unless asked not to, prove it can be written.
async fn check_storage(
    store: &dyn Store,
    args: &Args,
    window: Duration,
    now: OffsetDateTime,
) -> Vec<Finding> {
    let mut out = Vec::new();
    let name = store.name();

    match store.ping().await {
        Ok(()) => out.push(Finding::ok("storage", format!("{name} answered a ping"))),
        Err(e) => {
            // Nothing below this is worth reporting if the store is unreachable,
            // but the checks that need it degrade to their own findings rather
            // than being skipped silently.
            out.push(Finding::fault("storage", format!("{name} did not answer a ping: {e}")));
            return out;
        }
    }

    if args.read_only {
        out.push(Finding::skipped(
            "storage",
            "whether the store can be written was not checked (--read-only). \
             A read-only store accepts no spans and the daemon will not say so \
             until the first batch arrives.",
        ));
        return out;
    }

    out.push(write_probe(store, window, now, PROBE_BUDGET).await);
    out
}

/// Whether a store error is another connection holding the write lock, rather
/// than a store that cannot be written.
///
/// Every failure the probe exists to catch names itself and none of them says
/// "locked": SQLite answers a read-only file with "attempt to write a readonly
/// database" and a full disk with "database or disk is full", and Postgres
/// refuses a role without INSERT with "permission denied". A lock means some
/// other connection is writing right now, which is the state a healthy daemon
/// under load is in every second of the day.
///
/// This decides how long to keep trying, and nothing else. It used to decide
/// the level as well, and that is how a store which refused every write with a
/// lock-shaped message passed a deploy gate: see `write_failure`.
///
/// Matched on the message because `mcpd_store::Error` folds every backend
/// failure into one `Backend(String)`. A variant carried through the trait would
/// be better and is a change to the store crate, which this command only reads.
/// The phrases are listed rather than matched on the word "lock", because
/// "locking protocol" is a corrupt WAL and that is a real fault.
fn is_contention(e: &mcpd_store::Error) -> bool {
    const BUSY: [&str; 5] = [
        // SQLITE_BUSY and SQLITE_LOCKED.
        "database is locked",
        "database table is locked",
        // Postgres, whichever way the wait ended.
        "deadlock detected",
        "lock timeout",
        "could not obtain lock",
    ];
    let message = e.to_string().to_ascii_lowercase();
    BUSY.iter().any(|needle| message.contains(needle))
}

/// What the store took from other connections while the probe could not write.
///
/// This is the distinction the write check turns on. A lock held by somebody
/// who is making progress is a busy deployment. A lock nothing gets through is
/// a stopped one, and it looks exactly the same from inside a single failed
/// write, which is why the answer has to come from outside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Progress {
    /// Other connections recorded this many spans in that time, so the lock was
    /// being granted to somebody.
    Landed(u64),
    /// The store recorded nothing at all in that time, from anybody.
    Stalled,
    /// Not observed. Nothing is claimed either way, and the write is graded on
    /// its own.
    Unknown,
}

/// What one write probe cost, and what the store did while it was being spent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Spent {
    /// How many writes were attempted, including the one that worked.
    attempts: u32,
    /// How long the whole probe took.
    waited: std::time::Duration,
    /// What other connections managed in that time.
    progress: Progress,
}

/// The finding a probe write that never landed produces.
///
/// Pure, and separate from the loop, so every branch can be shown to hold
/// without a store that fails to order.
///
/// The history is the reason this function is shaped the way it is. Reporting
/// contention as a fault made the command worst exactly when the deployment was
/// healthiest, because an embedded store under real ingest holds its write lock
/// almost continuously. Reporting it as `skipped` fixed that by going blind:
/// `Level::Skipped` never changes the exit code, so a store answering a ping and
/// refusing every write with a lock-shaped message passed the deploy gate at the
/// same moment ingest was rejecting every batch.
///
/// So the message no longer decides the level. The budget does, and the
/// corroborating count breaks the tie: a store that took nothing from anybody
/// for a bounded, deliberate wait is one ingest is not landing in either,
/// whatever the error says. Only positive evidence that another connection was
/// granted the lock in the same window keeps it out of the exit code.
fn write_failure(error: &mcpd_store::Error, spent: Spent) -> Finding {
    let Spent { attempts, waited, progress } = spent;
    let tries = plural(attempts as usize, "attempt");
    let waited = seconds(waited);

    if !is_contention(error) {
        return Finding::fault(
            "storage",
            format!(
                "a probe span could not be stored: {error}. The store answers reads and \
                 refuses writes, so every batch that arrives is being dropped. Check the \
                 free space, the mount, and what the account the daemon runs as may write."
            ),
        );
    }

    match progress {
        Progress::Landed(landed) if landed > 0 => Finding::warn(
            "storage",
            format!(
                "the store recorded {landed} spans from other connections while it refused \
                 this command's own probe write for {waited} ({tries}, each answered with \
                 {error}). Writes are landing, so this is another connection holding the \
                 write lock and not a store that cannot be written. The write path was not \
                 proved from here. Run this again when ingest is quieter to prove it, or \
                 pass --read-only to say you never wanted it checked."
            ),
        ),
        Progress::Landed(_) | Progress::Stalled => Finding::fault(
            "storage",
            format!(
                "nothing reached the store for {waited}: {tries} were each answered with \
                 {error}, and no other connection recorded a span in the same time either. \
                 A lock nothing gets through is not another writer working, it is a writer \
                 that stopped, and every batch arriving at ingest is being refused the same \
                 way right now. Find the connection holding the write lock and end it: on \
                 SQLite that is another process left in a transaction it never committed, \
                 on Postgres it is a row in pg_locks."
            ),
        ),
        Progress::Unknown => Finding::fault(
            "storage",
            format!(
                "nothing reached the store for {waited}: {tries} were each answered with \
                 {error}. Whether any other connection recorded a span in the same time \
                 could not be read, so the store is graded on this write alone and this \
                 write did not land. Every batch arriving at ingest is being refused the \
                 same way. Find the connection holding the write lock and end it, then run \
                 this again."
            ),
        ),
    }
}

/// What a run of retried writes ended in.
struct Retried {
    /// How many writes were attempted, including the ones that failed.
    attempts: u32,
    /// The last answer the store gave.
    result: mcpd_store::Result<mcpd_core::IngestResult>,
}

/// Attempt `write` until it lands, until it fails for a reason that is not
/// contention, or until `budget` is spent, whichever comes first.
///
/// Generic over the write rather than over the store, so the schedule can be
/// driven from a test with no store in it at all. A retry loop that has only
/// ever been watched succeeding on its first attempt is exactly the shape of
/// defect this repository keeps finding.
///
/// The budget bounds the waiting and not the attempting: one more write is
/// always tried at the far edge of it, so the last thing the report is based on
/// is a real answer from the store rather than a stale one from ten seconds ago.
async fn keep_trying<W, F>(mut write: W, budget: std::time::Duration) -> Retried
where
    W: FnMut() -> F,
    F: std::future::Future<Output = mcpd_store::Result<mcpd_core::IngestResult>>,
{
    let started = std::time::Instant::now();
    let mut backoff = PROBE_BACKOFF;
    let mut attempts = 0u32;
    loop {
        attempts += 1;
        let result = write().await;
        if !result.as_ref().err().is_some_and(is_contention) {
            return Retried { attempts, result };
        }
        let left = budget.saturating_sub(started.elapsed());
        if left.is_zero() {
            return Retried { attempts, result };
        }
        tokio::time::sleep(backoff.min(left)).await;
        backoff = backoff.saturating_mul(2).min(PROBE_BACKOFF_MAX);
    }
}

/// How many spans the store holds across every project, inside one fixed
/// window.
///
/// Read twice around a probe that lost the write lock. What the difference
/// proves is that some other connection was granted the lock this command could
/// not get, and that is the whole difference between a store that is busy and
/// one that is stuck.
///
/// Spans rather than runs, because a daemon adding steps to runs that already
/// exist is still a daemon whose writes are landing.
///
/// The window is wide on purpose, and `now` is fixed by the caller so both
/// readings cover the same range. A window drawn tightly around the probe would
/// miss a sender whose clock is wrong or whose exporter batches for half a
/// minute, and reading those writes as "nothing landed" would be the false
/// fault this whole check exists to avoid.
///
/// The project list is re-read each time rather than passed in, because a
/// project that appears between the two readings is itself rows that landed.
/// The probe's own project is left out: those are this command's writes, and
/// they are the ones that are failing.
///
/// `None` when the list or any project's count could not be read. A partial sum
/// can fall as well as rise, and a fall read as a stall would be a fault
/// invented by a failed query.
async fn counted_spans(store: &dyn Store, window: Duration, now: OffsetDateTime) -> Option<u64> {
    let names = store.list_projects().await.ok()?;
    let mut total = 0u64;
    for project in gradable(names) {
        let s = store.analytics_summary(&project, now - window, now + window, None).await.ok()?;
        total = total.saturating_add(s.span_count);
    }
    Some(total)
}

/// Store one span, read it back, and delete it.
///
/// A store that answers a ping can still be unwritable: a full disk, a
/// read-only mount, a Postgres role with SELECT and nothing else. Each of those
/// looks healthy until the first batch arrives and is lost. The probe writes
/// into a project of its own so it can never be mistaken for traffic.
///
/// A write that loses one race for the lock is not one of those, so a
/// lock-shaped refusal is retried with a growing pause until `budget` is spent.
/// What happens when the budget runs out is `write_failure`'s decision, and the
/// two readings taken here are what it decides on: the spans the store recorded
/// for everybody else over exactly the interval this command could not write in.
///
/// The probe is the only check here that needs the lock. Everything below it
/// reads, and readers do not queue behind a writer.
async fn write_probe(
    store: &dyn Store,
    window: Duration,
    now: OffsetDateTime,
    budget: std::time::Duration,
) -> Finding {
    // Started before the sweep, not after it. The sweep is a run of writes, and
    // a write on a store that is refusing them sits on the driver's own busy
    // timeout: measuring from after it would report ten seconds for a command
    // that stood still for minutes, and the number an operator reads is the one
    // they use to decide the store was only briefly busy.
    let started = std::time::Instant::now();
    clear_probes(store, budget).await;

    let span = probe_span(now);
    let trace_id = span.trace_id.clone();
    // The same span every attempt. Nothing was stored by an attempt that
    // failed, and a row that did land is overwritten by its own identifier
    // rather than counted twice.
    let batch = || mcpd_store::Batch { spans: vec![span.clone()], ..mcpd_store::Batch::default() };

    let mut attempts = 1u32;
    let mut progress = Progress::Unknown;

    // The first attempt sits outside the retry loop because losing the lock
    // once is what decides whether the corroborating count is worth reading at
    // all. Nearly every run takes the write here and pays for none of the rest.
    let mut result = store.ingest(batch()).await;
    if result.as_ref().err().is_some_and(is_contention) {
        let before = counted_spans(store, window, now).await;
        let retried =
            keep_trying(|| store.ingest(batch()), budget.saturating_sub(started.elapsed())).await;
        attempts += retried.attempts;
        result = retried.result;
        if result.is_err() {
            let after = counted_spans(store, window, now).await;
            progress = match (before, after) {
                (Some(b), Some(a)) if a > b => Progress::Landed(a - b),
                (Some(b), Some(a)) if a == b => Progress::Stalled,
                // A count that fell is rows deleted, which is also a write that
                // was granted the lock. It is also what a retention sweep does
                // on a store nothing is ingesting into, so it is not read as
                // progress and it is not read as a stall.
                _ => Progress::Unknown,
            };
        }
    }

    let spent = Spent { attempts, waited: started.elapsed(), progress };

    match result {
        Ok(r) if r.accepted == 0 => {
            return Finding::fault(
                "storage",
                format!(
                    "a probe span was refused by the store: {} rejected. The store is \
                     taking the write and dropping the row, so a batch from a sender is \
                     dropped the same way. Check the daemon's log for what it named.",
                    r.rejected
                ),
            )
        }
        Ok(_) => {}
        Err(e) => return write_failure(&e, spent),
    }

    let read_back =
        store.get_trace(PROBE_PROJECT, &trace_id, &mcpd_store::Include::default()).await.is_ok();

    match store.delete_trace(PROBE_PROJECT, &trace_id).await {
        Ok(_) if read_back => Finding::ok(
            "storage",
            format!(
                "a probe span was stored, read back and deleted ({})",
                plural(spent.attempts as usize, "attempt")
            ),
        ),
        Ok(_) => Finding::fault(
            "storage",
            "a probe span was accepted but could not be read back. The store is \
             taking writes and not returning them.",
        ),
        // The write worked, which is what was being tested. Say the write
        // worked and name the row that was left behind, rather than reporting a
        // storage fault for a failed cleanup. A delete that lost the lock says
        // so, because "remove this by hand" reads as damage and a contended
        // delete is the next run's `clear_probes` call.
        Err(e) if is_contention(&e) => Finding::warn(
            "storage",
            format!(
                "the store is writable, but the probe span could not be deleted while \
                 another connection held the write lock ({e}). The next run of this command \
                 clears it. Trace {trace_id} in project {PROBE_PROJECT} until then."
            ),
        ),
        Err(e) => Finding::warn(
            "storage",
            format!(
                "the store is writable, but the probe span could not be deleted: {e}. \
                 Remove trace {trace_id} from project {PROBE_PROJECT} by hand."
            ),
        ),
    }
}

/// Delete anything an earlier run left in the probe's project.
///
/// Best effort, and deliberately silent: a delete that fails here fails again
/// on this run's own probe a moment later, where it is reported once with the
/// trace id to remove by hand. Reporting it twice would read as two problems.
///
/// Bounded by the same wall clock the probe is, because every one of these is a
/// write. On a store that is refusing writes each one sits on the driver's busy
/// timeout, and `PROBE_SWEEP` of them is eight minutes of a deploy gate standing
/// still before the check it exists for has begun. Litter that outlives the
/// budget is left for the next run, which is what "best effort" already meant.
async fn clear_probes(store: &dyn Store, budget: std::time::Duration) {
    let started = std::time::Instant::now();
    let q = TraceQuery {
        project: PROBE_PROJECT.to_owned(),
        limit: PROBE_SWEEP,
        ..TraceQuery::default()
    };
    let Ok(page) = store.list_traces(&q).await else { return };
    for t in &page.items {
        if started.elapsed() >= budget {
            return;
        }
        let _ = store.delete_trace(PROBE_PROJECT, &t.trace_id).await;
    }
}

/// A span to write and delete again.
///
/// The identifiers are derived from the clock and the process id rather than a
/// random source, because this crate carries no random-number dependency and
/// two doctors racing on one store is not a case worth a new one. Both halves
/// are recorded so a leftover row is obviously this command's.
fn probe_span(now: OffsetDateTime) -> mcpd_core::Span {
    let seed = format!("{:016x}{:08x}", now.unix_timestamp_nanos() as u64, std::process::id());
    mcpd_core::Span {
        trace_id: format!("{seed:0<32}")[..32].to_owned(),
        span_id: seed[..16].to_owned(),
        parent_span_id: None,
        external_parent_span_id: None,
        name: "mcpd doctor write probe".to_owned(),
        kind: mcpd_core::SpanKind::AgentRun,
        status: mcpd_core::SpanStatus::Ok,
        start_time: now,
        end_time: Some(now),
        duration_ns: Some(0),
        is_root: true,
        is_event: false,
        entity_kind: None,
        entity_id: None,
        entity_name: None,
        input: None,
        output: None,
        attributes: None,
        metadata: Some(serde_json::json!({ "source": "mcpd doctor", "synthetic": true })),
        usage: None,
        cost: None,
        model_parameters: None,
        error: None,
        links: None,
        tags: None,
        correlation: mcpd_core::Correlation {
            project: PROBE_PROJECT.to_owned(),
            environment: None,
            service_name: Some("mcpd-doctor".to_owned()),
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
        model: None,
        provider: None,
        received_at: now,
        ingest_key: None,
    }
}

// --------------------------------------------------------------------- keys --

/// One configured list of keys, named the way its owner would edit it.
struct KeyList {
    /// The flag or config path to name in a message.
    where_set: String,
    /// Which scope it grants.
    scope: &'static str,
    /// The project it is scoped to, or `None` for a top-level list.
    project: Option<String>,
    /// The keys.
    keys: Vec<String>,
}

/// Every key list in the configuration, top level first.
fn key_lists(config: &ServerConfig) -> Vec<KeyList> {
    let mut out = vec![
        KeyList {
            where_set: "--ingest-key".to_owned(),
            scope: "ingest",
            project: None,
            keys: config.ingest_keys.clone(),
        },
        KeyList {
            where_set: "--read-key".to_owned(),
            scope: "read",
            project: None,
            keys: config.read_keys.clone(),
        },
        KeyList {
            where_set: "--write-key".to_owned(),
            scope: "write",
            project: None,
            keys: config.write_keys.clone(),
        },
        KeyList {
            where_set: "--admin-key".to_owned(),
            scope: "admin",
            project: None,
            keys: config.admin_keys.clone(),
        },
        KeyList {
            where_set: "--execute-key".to_owned(),
            scope: "execute",
            project: None,
            keys: config.execute_keys.clone(),
        },
        KeyList {
            where_set: "--worker-key".to_owned(),
            scope: "worker",
            project: None,
            keys: config.worker_keys.clone(),
        },
    ];
    for (name, p) in &config.projects {
        for (scope, keys) in [
            ("ingest", &p.ingest_keys),
            ("read", &p.read_keys),
            ("write", &p.write_keys),
            ("admin", &p.admin_keys),
            ("execute", &p.execute_keys),
            ("worker", &p.worker_keys),
        ] {
            out.push(KeyList {
                where_set: format!("projects.{name}.{scope}_keys"),
                scope,
                project: Some(name.clone()),
                keys: keys.clone(),
            });
        }
    }
    out
}

/// Whether a configured key is one of the placeholders the example files ship.
///
/// `deploy/k8s/secret.yaml` carries `REPLACE_WITH_A_RANDOM_KEY` and
/// `deploy/docker/.env.example` carries `replace-me-ingest`. Both are longer
/// than [`SHORT_KEY`], so the length check passed them, and both are published
/// in this repository. Matched by prefix and case-insensitively, so every value
/// either file holds is caught, and so is the same placeholder retyped.
pub fn is_placeholder(key: &str) -> bool {
    let k = key.trim().to_ascii_lowercase();
    k.starts_with("replace_with") || k.starts_with("replace-me")
}

/// Whether a scope is open to anyone, applying the same fallback the server
/// applies: an unset write list inherits ingest, an unset admin list inherits
/// read, and a scope with no key anywhere is open.
fn scope_is_open(config: &ServerConfig, scope: mcpd_server::auth::Scope) -> bool {
    config.keys_for(scope).is_empty()
        && !config.projects.values().any(|p| !p.keys_for(scope).is_empty())
}

/// Check the key configuration.
///
/// This is the longest group because it is where the expensive mistakes live: a
/// key that cannot work, a scope that is open when it reads as closed, and a
/// key that reaches further than the block it was written under suggests.
///
/// Three parts rather than one because [`preflight`] takes two of them and
/// leaves the third. The findings, and the order they print in, are the same as
/// when this was one function.
///
/// `blank_lists` is what [`check_blank_key_lists`] found. It is graded in
/// `main` before the blank entries are dropped, so it arrives from outside. It
/// leads the group, and it keeps away the all-clear line, which would otherwise
/// print beside a list that holds no key.
fn check_keys(blank_lists: Vec<Finding>, config: &ServerConfig, listen: &str) -> Vec<Finding> {
    let mut out = blank_lists;
    out.extend(check_key_entries(config));
    out.extend(check_open_scopes(config, listen));
    out.extend(check_key_reach(config));
    if out.is_empty() {
        out.push(Finding::ok("keys", "every scope requires a key, and every key is usable"));
    }
    out
}

/// Grade each configured key on its own: whether it can ever match a presented
/// one, and whether it is long enough to be worth having.
fn check_key_entries(config: &ServerConfig) -> Vec<Finding> {
    let mut out = Vec::new();
    let lists = key_lists(config);

    // A presented key is trimmed before it is compared, and a configured one is
    // not, so anything with an edge of whitespace is unmatchable. A blank entry
    // never reaches here: `main` drops them before this reads the lists, and
    // `check_blank_key_lists` has already refused a list that held nothing else.
    let mut short = 0usize;
    for list in &lists {
        for (i, key) in list.keys.iter().enumerate() {
            let position = i + 1;
            if is_placeholder(key) {
                // The scope and the position, never the value. The value is
                // printed in the example file already, which is the problem.
                out.push(Finding::fault(
                    "keys",
                    format!(
                        "entry {position} of {} is the placeholder from an example file, not a \
                         key. Anyone who has read that file can present it, so the {} scope is \
                         open to them. Generate a key with `openssl rand -hex 32` and set that \
                         in its place.",
                        list.where_set, list.scope
                    ),
                ));
            } else if key.trim() != key.as_str() {
                out.push(Finding::fault(
                    "keys",
                    format!(
                        "entry {position} of {} has whitespace at one end. A presented key is \
                         trimmed before it is compared and this one is not, so it can never \
                         match and whoever holds it is refused. A space after a comma in the \
                         environment variable produces one.",
                        list.where_set
                    ),
                ));
            } else if key.chars().count() < SHORT_KEY {
                short += 1;
            }
        }
    }
    if short > 0 {
        out.push(Finding::warn(
            "keys",
            format!(
                "{short} configured {} shorter than {SHORT_KEY} characters. A key is only as \
                 hard to guess as the string itself, so generate them rather than choosing them.",
                if short == 1 { "key is" } else { "keys are" }
            ),
        ));
    }

    out
}

/// The environment variable clap reads for a top-level key flag.
fn key_variable(flag: &str) -> &'static str {
    match flag {
        "--ingest-key" => "MCPD_INGEST_KEYS",
        "--read-key" => "MCPD_READ_KEYS",
        "--write-key" => "MCPD_WRITE_KEYS",
        "--admin-key" => "MCPD_ADMIN_KEYS",
        "--execute-key" => "MCPD_EXECUTE_KEYS",
        _ => "MCPD_WORKER_KEYS",
    }
}

/// Refuse a key list that was given and holds no key at all.
///
/// `main` calls this before [`ServerConfig::drop_blank_keys`], because after
/// the drop such a list looks like one nobody set. Dropping the blank entries
/// is right when a real key sits beside them, as with a trailing comma. When
/// nothing else is in the list, the drop turns `MCPD_READ_KEYS=`, or a
/// Kubernetes Secret with an empty value, into an unset list, and the scope
/// falls back, or is open, with only a log line to say so. Whoever set the
/// variable meant to close the scope, so the daemon refuses to start instead.
///
/// The message names the flag, the variable or the config path, never a value.
/// A config file entry of `[]` is not caught: it reads the same as an absent
/// one, and it holds no blank entry for this to find.
#[must_use]
pub fn check_blank_key_lists(config: &ServerConfig) -> Vec<Finding> {
    key_lists(config)
        .into_iter()
        .filter(|l| !l.keys.is_empty() && l.keys.iter().all(|k| k.trim().is_empty()))
        .map(|l| {
            let message = if l.project.is_some() {
                format!(
                    "{} in the config file is set but holds no key, only blank entries. The \
                     daemon would read it as unset, so the {} scope of that project would \
                     fall back to another list or be open. Put a key in it, or remove it from \
                     the config file to leave the scope unset.",
                    l.where_set, l.scope
                )
            } else {
                let variable = key_variable(&l.where_set);
                format!(
                    "{flag} (or {variable}) is set but holds no key, only blank entries. The \
                     daemon would read it as unset, so the {scope} scope would fall back to \
                     another list or be open. An empty variable, or an empty value in a \
                     Kubernetes Secret, produces this. Set a key in it, or remove {variable} \
                     to leave the scope unset.",
                    flag = l.where_set,
                    scope = l.scope,
                )
            };
            Finding::fault("keys", message)
        })
        .collect()
}

/// Whether a scope that needs no key is the first-run default or a hole.
///
/// Its own function because the two callers grade it differently. `mcpd doctor`
/// prints it with the rest of the key group. [`preflight`] leaves it out: the
/// daemon already warns about both of these when it starts on a non-loopback
/// address, and the quickstart and deploy pages quote that wording line for
/// line, so saying it again here would be two problems where there is one.
fn check_open_scopes(config: &ServerConfig, listen: &str) -> Vec<Finding> {
    use mcpd_server::auth::Scope;
    let mut out = Vec::new();

    // An open scope is the first-run default and correct on a loopback address.
    // On anything else it is worth a line, which is the same judgement `main`
    // makes when it starts.
    //
    // An open read scope off loopback is the one openness this command calls a
    // fault, because `Level::Fault` says a fault is something broken or open
    // and nothing here produced one. It discloses stored prompts and
    // completions to anyone who can reach the port, and that cannot be taken
    // back. An open ingest scope on the same address costs rows, and retention
    // removes rows, so it stays a warning and does not gate a deploy.
    let loopback = listen.parse::<std::net::SocketAddr>().is_ok_and(|a| a.ip().is_loopback());
    for (scope, flag, consequence, level) in [
        (
            Scope::Ingest,
            "--ingest-key",
            "anyone who can reach the port can write to it",
            Level::Warn,
        ),
        (
            Scope::Read,
            "--read-key",
            "stored prompts and completions are readable by anyone who can reach the port",
            Level::Fault,
        ),
    ] {
        if scope_is_open(config, scope) {
            if loopback {
                out.push(Finding::ok(
                    "keys",
                    format!("the {} scope is open, which is correct on {listen}", scope.as_str()),
                ));
            } else {
                out.push(Finding::new(
                    "keys",
                    level,
                    format!(
                        "the {} scope is open and the daemon listens on {listen}, so \
                         {consequence}. Set {flag}, or bind --listen to a loopback address.",
                        scope.as_str()
                    ),
                ));
            }
        }
    }

    out
}

/// How far each key reaches: the two scope fallbacks, and every list one key
/// appears in.
///
/// Nothing here is graded against the listen address, so it reads the same on a
/// laptop and in a cluster.
fn check_key_reach(config: &ServerConfig) -> Vec<Finding> {
    use mcpd_server::auth::Scope;
    let mut out = Vec::new();
    let lists = key_lists(config);

    // The two fallbacks. Each is deliberate and each is a surprise the first
    // time somebody meets it, which is exactly what this command is for.
    if config.admin_keys.is_empty() && !config.read_keys.is_empty() {
        out.push(Finding::warn(
            "keys",
            "no --admin-key is set, so every read key also deletes traces and reads the \
             audit log. Set --admin-key to make a read key strictly read-only.",
        ));
    }
    if config.write_keys.is_empty() && !config.ingest_keys.is_empty() {
        out.push(Finding::warn(
            "keys",
            "no --write-key is set, so every ingest key also writes scores, feedback, \
             datasets and experiments. The ingest key is in every container that emits a \
             span. Set --write-key to separate them.",
        ));
    }

    for (name, p) in &config.projects {
        if p.admin_keys.is_empty() && !p.read_keys.is_empty() {
            out.push(Finding::warn(
                "keys",
                format!(
                    "projects.{name} sets read_keys and no admin_keys, so those read keys \
                     also delete from {name}. Add projects.{name}.admin_keys."
                ),
            ));
        }
        // The case that reads as protected and is not: a team is given a key
        // that writes to their project, and no key that reads it back.
        if !p.ingest_keys.is_empty() && p.read_keys.is_empty() {
            if scope_is_open(config, Scope::Read) {
                out.push(Finding::warn(
                    "keys",
                    format!(
                        "projects.{name} restricts who may write to it, but no read key is \
                         configured anywhere, so anyone can read {name}. Add \
                         projects.{name}.read_keys, or set --read-key."
                    ),
                ));
            } else {
                out.push(Finding::warn(
                    "keys",
                    format!(
                        "projects.{name} sets ingest_keys and no read_keys, so the key that \
                         writes to {name} is refused when it reads {name} back. Add \
                         projects.{name}.read_keys."
                    ),
                ));
            }
        }
    }

    // A key reaches every list it appears in. Two projects sharing one is a
    // tenancy boundary that is not there, and it is invisible in the file
    // because the two blocks are far apart.
    let mut seen: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut top_level: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for list in &lists {
        for key in &list.keys {
            if key.trim().is_empty() {
                continue;
            }
            let digest = mcpd_server::auth::key_digest(key);
            match &list.project {
                Some(project) => {
                    seen.entry(digest).or_default().insert(project.clone());
                }
                None => {
                    top_level.entry(digest).or_default().insert(list.where_set.clone());
                }
            }
        }
    }
    for (digest, projects) in &seen {
        if projects.len() > 1 {
            out.push(Finding::warn(
                "keys",
                format!(
                    "the key {digest} is configured under {} projects ({}). A key reaches \
                     every project it is listed under, so those projects are not separated \
                     from each other.",
                    projects.len(),
                    projects.iter().cloned().collect::<Vec<_>>().join(", ")
                ),
            ));
        }
        if let Some(flags) = top_level.get(digest) {
            out.push(Finding::warn(
                "keys",
                format!(
                    "the key {digest} is set at the top level ({}) and again under {}. A \
                     top-level key reaches every project, so the project block does not \
                     narrow it.",
                    flags.iter().cloned().collect::<Vec<_>>().join(", "),
                    projects.iter().cloned().collect::<Vec<_>>().join(", ")
                ),
            ));
        }
    }

    out
}

/// Whether the ingest keys also open the run surface.
///
/// The execute and worker scopes fall back to the write keys, and the write
/// scope falls back to the ingest keys. A deployment that sets only
/// `--ingest-key` and `--read-key`, which is the shape every example file in
/// `deploy/` starts from, therefore hands the key in every container that emits
/// a span the right to start runs and to claim steps, and a claimed step
/// carries its rendered input. The fallback is deliberate and stays, because
/// removing it would refuse every worker of a deployment that upgraded. What
/// was missing is anybody saying so: the open-surface warning does not fire,
/// because the scopes are not open, and `mcpd config` printed the line in the
/// tone of a closed one.
///
/// Its own function because only a daemon with a run store has a run surface
/// for the keys to reach.
pub fn check_run_key_reach(config: &ServerConfig) -> Vec<Finding> {
    let mut out = Vec::new();
    // Which run scopes land on the ingest list, given the three lists that
    // decide it. Empty when the write list is set: then the run scopes inherit
    // a key that is not in every container.
    let inherited = |ingest: &[String], write: &[String], execute: &[String], worker: &[String]| {
        if ingest.is_empty() || !write.is_empty() {
            return Vec::new();
        }
        let mut scopes = Vec::new();
        if execute.is_empty() {
            scopes.push("execute");
        }
        if worker.is_empty() {
            scopes.push("worker");
        }
        scopes
    };
    let reach = |scopes: &[&str]| match scopes {
        ["execute"] => "start, cancel and signal runs",
        ["worker"] => "claim steps, which hands over each step's rendered input",
        _ => "start runs and claim steps, which hands over each step's rendered input",
    };

    let top = inherited(
        &config.ingest_keys,
        &config.write_keys,
        &config.execute_keys,
        &config.worker_keys,
    );
    if !top.is_empty() {
        let flags = top.iter().map(|s| format!("--{s}-key")).collect::<Vec<_>>().join(" and ");
        out.push(Finding::warn(
            "keys",
            format!(
                "no {flags} is set and no --write-key either, so every ingest key can also {}. \
                 The ingest key is in every container that emits a span. Set {flags} to give \
                 the run surface keys of its own.",
                reach(&top)
            ),
        ));
    }
    for (name, p) in &config.projects {
        let scopes = inherited(&p.ingest_keys, &p.write_keys, &p.execute_keys, &p.worker_keys);
        if scopes.is_empty() {
            continue;
        }
        let keys = scopes
            .iter()
            .map(|s| format!("projects.{name}.{s}_keys"))
            .collect::<Vec<_>>()
            .join(" and ");
        out.push(Finding::warn(
            "keys",
            format!(
                "projects.{name} sets ingest_keys and neither write_keys nor {}, so its ingest \
                 keys can also {} in {name}. Add {keys}.",
                scopes.iter().map(|s| format!("{s}_keys")).collect::<Vec<_>>().join(" nor "),
                reach(&scopes)
            ),
        ));
    }
    out
}

// ----------------------------------------------------------------- identity --

/// Public addresses used to decide whether a trusted range reaches the whole
/// internet.
///
/// A range is graded by what it contains rather than by the prefix written in
/// it, because `0.0.0.0/0`, `0.0.0.0/1` and `::/0` are the same mistake in three
/// spellings. Three unrelated addresses, all of which have to be inside it, so
/// a genuine public proxy on a `/32` is never mistaken for an open range.
const PUBLIC_PROBES: [&str; 3] = ["8.8.8.8", "203.0.113.7", "198.51.100.9"];

/// The IPv6 half of the same question.
const PUBLIC_PROBES_V6: [&str; 3] = ["2001:4860:4860::8888", "2001:db8::1", "2606:4700::1"];

/// Check the trusted identity header, which is what puts a subject on an audit
/// row.
///
/// It is graded rather than left out because there is nothing else to grade it
/// against: `mcpd` has no identity provider, so with this off the audit log
/// records what was done and never who did it, and nothing in the report said
/// so. A reader could not tell the difference between "checked, and nobody is
/// identified" and "not checked".
fn check_identity(config: &ServerConfig, listen: &str) -> Vec<Finding> {
    let identity = &config.trusted_identity;
    if !identity.is_enabled() {
        return vec![Finding::skipped(
            "identity",
            "no identity header is configured. Every audit row records what was done and \
             not who did it, and nothing here was checked. Set \
             --trusted-identity-header together with --trusted-proxy-cidr to record a \
             subject from your ingress.",
        )];
    }

    let header = identity.header().map_or_else(String::new, |h| h.as_str().to_owned());
    let ranges: Vec<String> = identity.ranges().iter().map(ToString::to_string).collect();
    let listed = ranges.join(", ");

    // Reachable from the whole internet means the header is a string anybody
    // who can open a socket can set, and every subject in the audit log is then
    // whatever they typed. That is worse than recording no subject at all,
    // because a reader believes the name.
    let open_v4 = PUBLIC_PROBES.iter().all(|a| trusts(identity, a));
    let open_v6 = PUBLIC_PROBES_V6.iter().all(|a| trusts(identity, a));
    if open_v4 || open_v6 {
        // The same rule the open read scope follows: on a loopback address it
        // is a laptop, and off one it is a hole.
        let loopback = listen.parse::<std::net::SocketAddr>().is_ok_and(|a| a.ip().is_loopback());
        let message = format!(
            "the trusted range ({listed}) contains the whole internet, so anyone who can \
             reach this port can set {header} and be recorded as whoever they name. Every \
             subject in the audit log is then unverifiable. Narrow it to the address of \
             your ingress."
        );
        return vec![if loopback {
            Finding::warn("identity", format!("{message} It listens on {listen} for now."))
        } else {
            Finding::fault("identity", format!("{message} The daemon listens on {listen}."))
        }];
    }

    vec![Finding::ok(
        "identity",
        format!("an audit subject is read from {header}, for requests from {listed}"),
    )]
}

/// Whether a trusted-identity configuration accepts an address written as text.
///
/// An address this command cannot parse is not trusted, which is the safe
/// answer: the probes are literals, so a parse failure is a bug in the probe
/// list rather than in the configuration being graded.
fn trusts(identity: &mcpd_server::identity::TrustedIdentity, address: &str) -> bool {
    address.parse::<std::net::IpAddr>().is_ok_and(|a| identity.trusts(a))
}

// ------------------------------------------------------------------- limits --

/// Check the two settings that decide how much of what arrives is kept: the
/// per-project ceilings, and sampling.
///
/// Both were ungraded, and both fail the same silent way. A ceiling nobody set
/// is a disk nothing bounds. A sampling fraction set to the wrong number throws
/// away runs that were supposed to be kept, and nothing says so until somebody
/// looks for a trace that was never stored.
fn check_limits(config: &ServerConfig) -> Vec<Finding> {
    let mut out = check_quotas(config);
    out.extend(check_sampling(config));
    out
}

/// Grade the ingest ceilings.
fn check_quotas(config: &ServerConfig) -> Vec<Finding> {
    let quotas = config.project_quotas();
    if quotas.is_empty() {
        return vec![Finding::skipped(
            "limits",
            "no ingest ceiling is configured. Neither --quota-per-minute nor \
             `quota_per_minute` under a project is set, so nothing bounds how many signals \
             one project can store, and nothing here was checked.",
        )];
    }

    let mut out = Vec::new();
    let named: Vec<String> =
        quotas.limits().map(|(project, limit)| format!("{project} {limit}/min")).collect();
    if !named.is_empty() {
        out.push(Finding::ok("limits", format!("limited: {}", named.join(", "))));
    }

    match quotas.unnamed_limit() {
        Some(shared) => out.push(Finding::ok(
            "limits",
            format!(
                "every project the configuration does not name shares one ceiling of \
                 {shared} per minute"
            ),
        )),
        // The hole this exists to find. Per-project ceilings alone bound only
        // the projects somebody wrote down, and the project name arrives inside
        // the request body, so a sender can leave the ceiling by naming
        // something else.
        None => out.push(Finding::warn(
            "limits",
            "a ceiling is set per project and --quota-per-minute is not, so a project no \
             `projects` block names has no ceiling at all. The sender chooses the project \
             name in the body of its own request, so the ceiling is optional from its \
             side. Set --quota-per-minute to give every unnamed project a shared one.",
        )),
    }

    // A batch larger than a whole minute's ceiling is refused in part every
    // time, however well behaved the sender is. That is a number nobody meant
    // to configure, and it reads at ingest as a fleet that cannot send.
    let batch = u32::try_from(config.max_batch_signals).unwrap_or(u32::MAX);
    let smaller: Vec<String> = quotas
        .limits()
        .chain(quotas.unnamed_limit().map(|n| ("unnamed projects", n)))
        .filter(|(_, limit)| *limit < batch)
        .map(|(project, limit)| format!("{project} ({limit}/min)"))
        .collect();
    if !smaller.is_empty() {
        out.push(Finding::warn(
            "limits",
            format!(
                "{} below --max-batch-signals ({batch}), so one legal batch can exceed a \
                 whole minute's ceiling and is trimmed however well behaved the sender \
                 is: {}. Raise the ceiling, or lower the batch size the senders use.",
                plural(smaller.len(), "ceiling"),
                smaller.join(", ")
            ),
        ));
    }
    out
}

/// Grade the sampling policy, globally and per project.
fn check_sampling(config: &ServerConfig) -> Vec<Finding> {
    use mcpd_core::SamplingKind;

    let global = &config.sampling;
    let per_project: Vec<(&String, f64)> =
        config.projects.iter().filter_map(|(name, p)| p.sample.map(|f| (name, f))).collect();

    if matches!(global.kind, SamplingKind::Always) && per_project.is_empty() {
        return vec![Finding::skipped(
            "limits",
            "no sampling is configured. Every trace that arrives is kept, so there is no \
             fraction to check and nothing here was checked.",
        )];
    }

    let mut out = Vec::new();

    // A fraction outside the scale is refused for the flag and was never
    // checked for the config file, so a project could carry one. It is a fault
    // rather than a warning because there is no reading of it that is right:
    // the number decides what is kept and it is not a fraction.
    let bad: Vec<String> = per_project
        .iter()
        .filter(|(_, f)| !(0.0..=1.0).contains(f) || f.is_nan())
        .map(|(name, f)| format!("{name} ({f})"))
        .collect();
    if !bad.is_empty() {
        out.push(Finding::fault(
            "limits",
            format!(
                "a project sets `sample` outside 0.0 to 1.0: {}. --sample is refused at \
                 startup for the same value and this one is not, so the daemon is running \
                 a fraction that means nothing.",
                bad.join(", ")
            ),
        ));
    }

    // Zero keeps nothing. It is legal and occasionally meant, so it is a
    // warning, but it is worth saying out loud: every scorer, monitor and
    // objective reading that project sees an empty store.
    let silent: Vec<&str> =
        per_project.iter().filter(|(_, f)| *f == 0.0).map(|(name, _)| name.as_str()).collect();
    if !silent.is_empty() {
        out.push(Finding::warn(
            "limits",
            format!(
                "{} keeps no traces at all (`sample` is 0.0): {}. Every monitor and \
                 objective watching it reads an empty store.",
                plural(silent.len(), "project"),
                silent.join(", ")
            ),
        ));
    }

    if matches!(global.kind, SamplingKind::Never) {
        out.push(Finding::warn(
            "limits",
            "the global sampling policy keeps nothing, so any project without its own \
             `sample` stores no traces.",
        ));
    }

    if !bad.is_empty() {
        // No summary of what is kept while a fraction in it means nothing. The
        // number would be printed as fact next to the line saying it is not one.
        return out;
    }

    // Ahead of the warnings rather than instead of them. The fraction is the
    // thing a reader came to check, and a project sampling nothing does not
    // stop the rest of the policy being worth printing.
    let global_says = match global.kind {
        SamplingKind::Always => "every trace is kept".to_owned(),
        SamplingKind::Never => "no trace is kept".to_owned(),
        _ => format!("{} of traces are kept", fraction(global.probability)),
    };
    let mut message = format!("globally, {global_says}");
    if !per_project.is_empty() {
        let each: Vec<String> =
            per_project.iter().map(|(name, f)| format!("{name} {}", fraction(*f))).collect();
        message.push_str(&format!("; per project, {}", each.join(", ")));
    }
    out.insert(0, Finding::ok("limits", message));
    out
}

/// A sampling fraction as a percentage, for a line somebody reads.
fn fraction(f: f64) -> String {
    format!("{:.3}%", f * 100.0).replace(".000%", "%")
}

// ----------------------------------------------------------------- projects --

/// Compare the projects named in the configuration against the projects that
/// hold runs.
///
/// A project name is free text on the wire, so a `projects` block naming
/// `chekout` is accepted, does nothing, and reads as if it works.
fn check_projects(config: &ServerConfig, with_data: &BTreeSet<String>) -> Vec<Finding> {
    let mut out = Vec::new();

    if with_data.is_empty() {
        out.push(Finding::warn(
            "projects",
            "the store holds no runs at all. On a new deployment that is expected. On one \
             that has been running, check --data (or --postgres-url, --clickhouse-url) \
             points at the store the daemon writes to.",
        ));
    } else {
        out.push(Finding::ok(
            "projects",
            format!(
                "runs are stored in {}: {}",
                plural(with_data.len(), "project"),
                preview(with_data, 8)
            ),
        ));
    }

    for name in config.projects.keys() {
        if !with_data.contains(name) {
            out.push(Finding::warn(
                "projects",
                format!(
                    "projects.{name} is configured but no run has ever been stored under \
                     {name}. Check the spelling against the project the senders set, or \
                     remove the block. Retention may also have deleted everything it held."
                ),
            ));
        }
    }

    if !with_data.is_empty() && !with_data.contains(&config.default_project) {
        out.push(Finding::warn(
            "projects",
            format!(
                "--project is {}, and no run has been stored under it. A sender that names \
                 no project lands there, so either nothing does, or the name is wrong.",
                config.default_project
            ),
        ));
    }
    out
}

// ---------------------------------------------------------------- retention --

/// The projects a retention pass will actually visit.
///
/// This mirrors the plan `main` builds. The part worth checking is what it
/// leaves out: `--retention-days` reaches the default project and the projects
/// named in the config file, and nothing else. A project that a sender created
/// by naming it on a span is swept by neither.
fn swept_projects(config: &ServerConfig) -> BTreeMap<String, u32> {
    let mut plan = BTreeMap::new();
    for (name, p) in &config.projects {
        if let Some(days) = p.retention_days.or(config.retention_days) {
            plan.insert(name.clone(), days);
        }
    }
    if let Some(days) = config.retention_days {
        plan.entry(config.default_project.clone()).or_insert(days);
    }
    plan
}

/// Check retention.
///
/// `with_data` is `None` when the project list could not be read. The plan is
/// still worth printing, because it comes from the configuration, but nothing
/// is compared against the store.
fn check_retention(
    config: &ServerConfig,
    with_data: Option<&BTreeSet<String>>,
    activity: &BTreeMap<String, Activity>,
    now: OffsetDateTime,
) -> Vec<Finding> {
    let mut out = Vec::new();
    let plan = swept_projects(config);

    if plan.is_empty() {
        // Skipped, not ok. Nothing was verified: this is the configuration
        // saying nothing about retention, which is also what it says when
        // --config-file names the wrong file.
        out.push(Finding::skipped(
            "retention",
            "no retention is configured. Neither --retention-days nor `retention_days` \
             under a project is set, so nothing is ever deleted and there is no sweep to \
             check.",
        ));
    } else {
        let mut swept: Vec<String> =
            plan.iter().map(|(name, days)| format!("{name} at {days} days")).collect();
        swept.sort();
        out.push(Finding::ok("retention", format!("swept every hour: {}", swept.join(", "))));
    }

    let Some(with_data) = with_data else {
        out.push(Finding::skipped(
            "retention",
            "which projects hold runs outside the sweep was not checked, because the \
             project list could not be read.",
        ));
        return out;
    };

    for project in with_data {
        if plan.contains_key(project) {
            continue;
        }
        let oldest = activity.get(project).and_then(|a| a.oldest_start);
        if config.retention_days.is_some() {
            out.push(Finding::warn(
                "retention",
                format!(
                    "{project} holds runs and is never swept. --retention-days reaches the \
                     project named by --project and the projects named under `projects` in \
                     --config-file, and {project} is neither.{}",
                    oldest.map_or(String::new(), |t| format!(
                        " Its oldest run is {} old.",
                        human(now - t)
                    ))
                ),
            ));
        } else if let Some(t) = oldest {
            if now - t > UNBOUNDED_AGE {
                out.push(Finding::warn(
                    "retention",
                    format!(
                        "{project} holds runs going back {}, and no retention is configured, \
                         so the store grows without bound. Set --retention-days, or \
                         retention_days under projects.{project}.",
                        human(now - t)
                    ),
                ));
            }
        }
    }
    out
}

// --------------------------------------------------------------- forwarding --

/// What a probe of one forwarding destination found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Probe {
    /// No probe was attempted, for the reason given.
    Skipped(String),
    /// The endpoint answered with this status.
    Answered(u16),
    /// The request never completed, or was refused with this reason.
    Failed(String),
}

/// Probe every enabled destination, in order, with an export that carries
/// nothing: it proves the URL, the route and the credential without writing
/// anything downstream.
async fn probe_forward(config: &ServerConfig, args: &Args) -> Vec<Probe> {
    let mut out = Vec::new();
    for d in config.forward.enabled() {
        out.push(probe_one(d, args).await);
    }
    out
}

async fn probe_one(d: &mcpd_server::forward::Destination, args: &Args) -> Probe {
    use mcpd_server::forward::{deliver, Protocol};
    if args.offline {
        return Probe::Skipped("--offline".to_owned());
    }
    if let Some(e) = &d.endpoint {
        if !e.starts_with("http://") && !e.starts_with("https://") && !e.starts_with("${") {
            return Probe::Failed("the endpoint has no http:// or https:// scheme".to_owned());
        }
    }
    // The message names the field and never its value.
    let r = match d.resolve() {
        Ok(r) => r,
        Err(e) => return Probe::Failed(e),
    };
    let url = r.url(mcpd_server::forward::Signal::Traces);
    match r.protocol {
        Protocol::Api => Probe::Skipped(
            "Helicone's logging API has no request that writes nothing, so it is checked by \
             the first model call it is sent"
                .to_owned(),
        ),
        Protocol::Grpc => {
            let client = match reqwest::Client::builder()
                .http2_prior_knowledge()
                .timeout(std::time::Duration::from_secs(10))
                .build()
            {
                Ok(c) => c,
                Err(e) => return Probe::Failed(e.to_string()),
            };
            match deliver::post_grpc(&client, &url, &r.headers, &[]).await {
                deliver::Outcome::Delivered => Probe::Answered(200),
                deliver::Outcome::Retry { why, .. } | deliver::Outcome::Refused { why } => {
                    Probe::Failed(why)
                }
            }
        }
        Protocol::Json | Protocol::Protobuf => {
            let client = match reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
            {
                Ok(c) => c,
                Err(e) => return Probe::Failed(e.to_string()),
            };
            let (ct, body): (&str, Vec<u8>) = if r.protocol == Protocol::Json {
                ("application/json", br#"{"resourceSpans":[]}"#.to_vec())
            } else {
                // An empty ExportTraceServiceRequest is zero bytes of protobuf.
                ("application/x-protobuf", Vec::new())
            };
            let mut req = client.post(url).header("content-type", ct).body(body);
            for (k, v) in &r.headers {
                req = req.header(k, v);
            }
            match req.send().await {
                Ok(res) => Probe::Answered(res.status().as_u16()),
                // Without the URL. The finding prints the endpoint through
                // `without_credentials`, and the client's error repeats it whole.
                Err(e) => Probe::Failed(mcpd_server::outbound_error(e)),
            }
        }
    }
}

/// What a destination's spool directory holds, as a clause for its finding,
/// and whether it deserves a warning of its own.
fn spool_note(config: &ServerConfig, name: &str) -> (String, Option<String>) {
    let Some(root) = &config.forward.spool_dir else {
        return (" Its spool is held in memory.".to_owned(), None);
    };
    let dir = root.join(format!("forward-{name}"));
    match mcpd_server::spool::Spool::inspect(&dir) {
        Ok(c) => {
            let mut note = if c.entries == 0 {
                format!(" Its spool at {} is empty.", dir.display())
            } else {
                format!(
                    " {} batch(es), {} bytes, wait in its spool at {}, the oldest {} old.",
                    c.entries,
                    c.bytes,
                    dir.display(),
                    human(Duration::seconds(
                        i64::try_from(c.oldest_age_seconds.unwrap_or(0)).unwrap_or(i64::MAX)
                    ))
                )
            };
            let mut warn = None;
            if c.dead > 0 {
                note.push_str(&format!(" {} refused batch(es) are kept in its dead/.", c.dead));
                warn = Some(format!(
                    "{name}: {} batch(es) were refused for good or expired and are kept in {}. \
                     Read one to see what the destination objected to; nothing retries them.",
                    c.dead,
                    dir.join("dead").display()
                ));
            }
            if c.oldest_age_seconds.unwrap_or(0) > 3600 {
                warn = Some(format!(
                    "{name}: the oldest batch in its spool has waited {}. The destination is \
                     not accepting what it is sent; a batch older than a day is given up on.",
                    human(Duration::seconds(
                        i64::try_from(c.oldest_age_seconds.unwrap_or(0)).unwrap_or(i64::MAX)
                    ))
                ));
            }
            (note, warn)
        }
        Err(e) => (format!(" Its spool at {} could not be read: {e}.", dir.display()), None),
    }
}

fn check_forwarding(config: &ServerConfig, probes: &[Probe]) -> Vec<Finding> {
    if config.forward.destinations.is_empty() {
        return vec![Finding::skipped("forwarding", ABSENT_FORWARD)];
    }
    let mut out = Vec::new();
    for d in config.forward.destinations.iter().filter(|d| !d.enabled) {
        out.push(Finding::warn(
            "forwarding",
            format!(
                "{} ({}) is configured and `enabled` is false, so nothing is copied to it. Set \
                 it to true, or remove it from the `forward` block.",
                d.name(),
                without_credentials(&d.traces_url())
            ),
        ));
    }
    for (d, probe) in config.forward.enabled().zip(probes) {
        let name = d.name();
        // The endpoint carries a credential when one was written into the URL,
        // and this string is printed in every finding below.
        let url = without_credentials(&d.traces_url());
        let who = format!("{name} ({}, {url})", d.vendor.as_str());
        let (note, warn) = spool_note(config, &name);
        out.push(match probe {
            Probe::Answered(s) if (200..300).contains(s) => Finding::ok(
                "forwarding",
                format!("{who} answered {s} to an export with nothing in it.{note}"),
            ),
            Probe::Answered(s) if *s == 401 || *s == 403 => Finding::fault(
                "forwarding",
                format!(
                    "{who} answered {s}. Every batch is being refused; mcpd keeps its own copy, \
                     holds the batches in the spool and retries them for up to a day. Check this \
                     destination's credential (`api_key`, `token` or `headers`) in \
                     --config-file.{note}"
                ),
            ),
            Probe::Answered(s) => Finding::fault(
                "forwarding",
                format!(
                    "{who} answered {s}. Check this destination's `endpoint` in --config-file: \
                     it is the base URL, and the signal path is appended to it.{note}"
                ),
            ),
            Probe::Failed(e) => Finding::fault(
                "forwarding",
                format!(
                    "{who} could not be reached: {e}. Batches wait in the spool and are retried \
                     for up to a day. Check this destination in --config-file.{note}"
                ),
            ),
            Probe::Skipped(why) => Finding::skipped(
                "forwarding",
                format!("{who} is configured and was not contacted ({why}).{note}"),
            ),
        });
        if let Some(w) = warn {
            out.push(Finding::warn("forwarding", w));
        }
    }
    out
}

/// What an absent `forward` block means.
const ABSENT_FORWARD: &str = "there is no `forward` block in the configuration, so nothing is \
                              copied on and no endpoint was contacted.";

// -------------------------------------------------------------------- judge --

/// Check the judge as configuration.
///
/// Nothing here contacts the model. A judge answers a chat completion, and a
/// diagnostic that spends money every time it runs is one nobody runs.
fn check_judge(config: &ServerConfig) -> Vec<Finding> {
    let Some(j) = config.judge.as_ref() else {
        return vec![Finding::skipped(
            "judge",
            "there is no `judge` block in the configuration. A judged scorer declines with \
             `no judge is configured`, and nothing here was checked.",
        )];
    };
    let mut out = Vec::new();
    // Printed in four findings, and an endpoint is a place people put a key.
    let endpoint = without_credentials(&j.endpoint);

    if j.criteria.is_empty() {
        out.push(Finding::fault(
            "judge",
            format!(
                "the judge at {endpoint} has no criteria, so every judged scorer declines \
                 with `is not a configured criterion` and no judged score is ever produced. \
                 Add entries under `judge.criteria` in --config-file."
            ),
        ));
    } else {
        out.push(Finding::ok(
            "judge",
            format!(
                "{} at {endpoint}, {}. Whether the model answers was not checked: a probe \
                 would spend money.",
                j.model,
                plural(j.criteria.len(), "criterion")
            ),
        ));
    }

    if !j.endpoint.starts_with("http://") && !j.endpoint.starts_with("https://") {
        out.push(Finding::fault(
            "judge",
            format!(
                "`judge.endpoint` is {endpoint}, which has no http:// or https:// scheme, \
                 so every request fails before it is sent."
            ),
        ));
    }
    // The request is built as `{endpoint}/chat/completions`, so an endpoint that
    // already carries the path produces a URL with it twice.
    if j.endpoint.trim_end_matches('/').ends_with("/chat/completions") {
        out.push(Finding::fault(
            "judge",
            format!(
                "`judge.endpoint` ends in /chat/completions. mcpd appends that path, so \
                 requests go to {}/chat/completions. Set the endpoint to the base URL, such \
                 as http://localhost:11434/v1.",
                endpoint.trim_end_matches('/')
            ),
        ));
    }
    if j.api_key.is_empty() && !is_local_url(&endpoint) {
        out.push(Finding::warn(
            "judge",
            format!(
                "`judge.api_key` is empty and {endpoint} is not a local address, so the \
                 request carries no Authorization header. A model on your own machine wants \
                 none; a hosted one needs it."
            ),
        ));
    }
    out
}

/// Whether a URL names this machine. Used only to decide whether a missing
/// credential is expected, never to decide whether something is trusted.
fn is_local_url(url: &str) -> bool {
    let host = url.split("//").nth(1).unwrap_or(url).split(['/', ':']).next().unwrap_or("");
    host == "localhost" || host == "127.0.0.1" || host == "::1" || host == "0.0.0.0"
}

// ----------------------------------------------------------------- monitors --

/// Check the monitors and the objectives.
///
/// `with_data` is `None` when the project list could not be read. Everything
/// that is a fact about the configuration is still checked; the comparisons
/// against the store are skipped rather than run against an empty set, which
/// would report every monitor as watching a project that holds nothing.
fn check_monitors(
    config: &ServerConfig,
    with_data: Option<&BTreeSet<String>>,
    activity: &BTreeMap<String, Activity>,
    scorers_with_no_scores: &BTreeSet<String>,
    window: Duration,
) -> Vec<Finding> {
    let mut out = Vec::new();

    if config.monitors.is_empty() && config.slos.is_empty() {
        return vec![Finding::skipped(
            "monitors",
            "there is no monitor and no objective in the configuration. Nothing is watched, \
             nothing is notified, and nothing here was evaluated.",
        )];
    }

    let armed = config.monitors.iter().filter(|m| m.enabled).count();
    let objectives = config.slos.iter().filter(|s| s.enabled).count();
    out.push(Finding::ok(
        "monitors",
        format!(
            "{} and {} evaluated every {} seconds",
            plural(armed, "monitor"),
            plural(objectives, "objective"),
            config.monitor_interval_seconds
        ),
    ));

    if with_data.is_none() {
        out.push(Finding::skipped(
            "monitors",
            "whether each monitor watches a project that holds runs was not checked, \
             because the project list could not be read.",
        ));
    }

    for m in &config.monitors {
        if !m.enabled {
            out.push(Finding::warn(
                "monitors",
                format!("monitor {} is disabled and is never evaluated. Set `enabled`.", m.id),
            ));
            continue;
        }
        if m.webhook.is_none() {
            out.push(Finding::warn(
                "monitors",
                format!(
                    "monitor {} has no webhook, so when it trips it changes condition in the \
                     API and notifies nobody. Add `webhook` to the monitor in --config-file.",
                    m.id
                ),
            ));
        }
        if with_data.is_some_and(|d| !d.contains(&m.project)) {
            out.push(Finding::warn(
                "monitors",
                format!(
                    "monitor {} watches project {}, which holds no runs. Check the name.",
                    m.id, m.project
                ),
            ));
        }
        if m.watch == Watch::ScoreMean {
            match m.scorer_id.as_deref() {
                None | Some("") => out.push(Finding::fault(
                    "monitors",
                    format!(
                        "monitor {} watches score_mean with no `scorer_id`, so it looks for \
                         scores from a scorer with an empty name, finds none and can never \
                         trip. Set `scorer_id`.",
                        m.id
                    ),
                )),
                Some(id) if scorers_with_no_scores.contains(id) => out.push(Finding::warn(
                    "monitors",
                    format!(
                        "monitor {} watches the scorer {id}, and no score from {id} has ever \
                         been stored in {}. Check the scorer id.",
                        m.id, m.project
                    ),
                )),
                Some(_) => {}
            }
        }
        // A threshold that cannot be reached is not an alert, it is a line in a
        // config file. `min_samples` is the usual reason.
        //
        // Not for the two watches that read the durable run store. The count
        // below is the trace count, and neither of them consults it:
        // `step_count_divergence` does not read `min_samples` at all, and
        // `run_repetition` counts the runs it read from the run store, which a
        // deployment can serve while its spans are ingested somewhere else
        // entirely. Warning from a number they never look at would tell an
        // operator to lower a setting that is not the problem.
        let reads_traces = !matches!(m.watch, Watch::StepCountDivergence | Watch::RunRepetition);
        if let Some(a) = activity.get(&m.project).filter(|_| reads_traces) {
            let windows = (window.whole_seconds() as f64) / f64::from(m.window_seconds.max(1));
            if windows >= 1.0 {
                let per_window = a.recent_traces as f64 / windows;
                if per_window * 2.0 < m.min_samples as f64 {
                    // The raw count rather than the average, because a busy
                    // monitor on a quiet project averages to 0.0 and a reader
                    // cannot tell a rounded number from a broken one.
                    out.push(Finding::warn(
                        "monitors",
                        format!(
                            "monitor {} needs {} runs in each {} second window. {} stored {} \
                             runs over the last {}, which is {windows:.0} such windows, so \
                             most of them will not reach {} and it will report \
                             `insufficient` rather than trip. Lower `min_samples` or widen \
                             `window_seconds`.",
                            m.id,
                            m.min_samples,
                            m.window_seconds,
                            m.project,
                            a.recent_traces,
                            human(window),
                            m.min_samples
                        ),
                    ));
                }
            }
        }
    }

    for s in &config.slos {
        if !s.enabled {
            out.push(Finding::warn(
                "monitors",
                format!("objective {} is disabled and is never evaluated. Set `enabled`.", s.id),
            ));
            continue;
        }
        // The burn rate divides by `1 - objective`, so a percentage written as
        // 99.5 leaves an error budget of nothing and every window burns.
        if !(0.0..1.0).contains(&s.objective) {
            out.push(Finding::fault(
                "monitors",
                format!(
                    "objective {} has an objective of {}, which is not a fraction between 0 \
                     and 1. The error budget is 1 minus this number, so it leaves none and \
                     the objective can never be met. Write 99.5% as 0.995.",
                    s.id, s.objective
                ),
            ));
        }
        if s.webhook.is_none() {
            out.push(Finding::warn(
                "monitors",
                format!(
                    "objective {} has no webhook, so a burning error budget notifies nobody. \
                     Add `webhook` to the objective in --config-file.",
                    s.id
                ),
            ));
        }
        if with_data.is_some_and(|d| !d.contains(&s.project)) {
            out.push(Finding::warn(
                "monitors",
                format!(
                    "objective {} covers project {}, which holds no runs. Check the name.",
                    s.id, s.project
                ),
            ));
        }
    }
    out
}

// -------------------------------------------------------------------- clock --

fn check_clock(activity: &BTreeMap<String, Activity>, now: OffsetDateTime) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut checked = 0usize;
    let mut sampled = 0usize;

    for (project, a) in activity {
        let Some(newest) = a.newest_received else { continue };
        checked += 1;
        sampled += a.clock_samples;

        if newest - now > CLOCK_TOLERANCE {
            out.push(Finding::warn(
                "clock",
                format!(
                    "{project} holds a run received {} in the future of this machine's \
                     clock. One of the two is wrong. Check NTP on both, and note that a \
                     sender may set received_at itself.",
                    human(newest - now)
                ),
            ));
        }

        // start_time comes from the sender and received_at from whoever wrote
        // the row. Trace time-range filters compare against start_time, so a
        // sender with a wrong clock silently drops its own runs out of every
        // windowed query while the data sits in the store.
        if let Some((trace_id, skew)) = &a.worst_skew {
            if skew.abs() > CLOCK_TOLERANCE {
                out.push(Finding::warn(
                    "clock",
                    format!(
                        "in {project}, run {trace_id} started {} {} it was received. A \
                         time-range query filters on start_time, so runs from that sender \
                         do not appear in the window they actually happened in.",
                        human(skew.abs()),
                        if skew.is_negative() { "before" } else { "after" }
                    ),
                ));
            }
        }
    }

    if out.is_empty() {
        out.push(if checked == 0 {
            Finding::skipped("clock", "no run was found to compare a clock against")
        } else {
            Finding::ok(
                "clock",
                format!(
                    "{} sampled from the newest and the oldest in {}, each starting within \
                     {} of when it was received. Runs between those two ends were not read.",
                    plural(sampled, "run"),
                    plural(checked, "project"),
                    human(CLOCK_TOLERANCE)
                ),
            )
        });
    }
    out
}

// ------------------------------------------------------------ store reading --

/// What the store says about one project.
#[derive(Debug, Default, Clone)]
struct Activity {
    /// Runs started inside the window.
    recent_traces: u64,
    /// The oldest run's start time.
    oldest_start: Option<OffsetDateTime>,
    /// The newest `received_at` seen in the sampled page.
    newest_received: Option<OffsetDateTime>,
    /// The largest gap between a run's start and its arrival, and the run it
    /// came from.
    worst_skew: Option<(String, Duration)>,
    /// How many distinct runs that gap was measured over.
    clock_samples: usize,
}

async fn gather_activity(
    store: &dyn Store,
    projects: &BTreeSet<String>,
    window: Duration,
    now: OffsetDateTime,
) -> BTreeMap<String, Activity> {
    let mut out = BTreeMap::new();
    for project in projects {
        let mut a = Activity::default();
        let mut seen: BTreeSet<String> = BTreeSet::new();

        // Both ends of the ordering. The store can only order runs by
        // start_time, which is the field a skewed sender got wrong, so the runs
        // this check exists to find are the ones a page of the newest runs
        // leaves out: a clock that runs behind puts them under every honest run
        // in the store. Reading the oldest page as well is what catches them.
        for order in [Order::Desc, Order::Asc] {
            let q = TraceQuery {
                project: project.clone(),
                sort: TraceSort::StartTime,
                order,
                limit: CLOCK_SAMPLE,
                ..TraceQuery::default()
            };
            let Ok(page) = store.list_traces(&q).await else { continue };
            if order == Order::Asc {
                a.oldest_start = page.items.first().map(|t| t.start_time);
            }
            for t in &page.items {
                // A store holding fewer runs than the page returns the same
                // rows twice, and a sample count that double counts them is its
                // own small lie.
                if !seen.insert(t.trace_id.clone()) {
                    continue;
                }
                a.newest_received = Some(match a.newest_received {
                    Some(prev) if prev > t.received_at => prev,
                    _ => t.received_at,
                });
                let skew = t.start_time - t.received_at;
                if a.worst_skew.as_ref().is_none_or(|(_, w)| skew.abs() > w.abs()) {
                    a.worst_skew = Some((t.trace_id.clone(), skew));
                }
            }
        }
        a.clock_samples = seen.len();

        if let Ok(s) = store.analytics_summary(project, now - window, now, None).await {
            a.recent_traces = s.trace_count;
        }

        out.insert(project.clone(), a);
    }
    out
}

/// The scorer ids named by a `score_mean` monitor that have never produced a
/// score in the project the monitor watches.
async fn empty_scorers(store: &dyn Store, config: &ServerConfig) -> BTreeSet<String> {
    let mut empty = BTreeSet::new();
    for m in &config.monitors {
        if m.watch != Watch::ScoreMean {
            continue;
        }
        let Some(id) = m.scorer_id.as_deref().filter(|s| !s.is_empty()) else { continue };
        let q = ScoreQuery {
            project: m.project.clone(),
            scorer_id: Some(id.to_owned()),
            limit: 1,
            ..ScoreQuery::default()
        };
        // A query that fails is not evidence of an absent scorer, so only an
        // empty page counts.
        if let Ok(page) = store.list_scores(&q).await {
            if page.items.is_empty() {
                empty.insert(id.to_owned());
            }
        }
    }
    empty
}

/// Models on recent spans that the price book does not know.
///
/// A span is only counted when it carries a model and a token count, because
/// those are the spans that would have had a cost. `cost` is computed once at
/// ingest and only when it is absent, so a missing cost on such a span means
/// the lookup found no rates.
async fn unpriced_models(
    store: &dyn Store,
    projects: &BTreeSet<String>,
    book: &PriceBook,
    from: OffsetDateTime,
    to: OffsetDateTime,
) -> (u64, BTreeMap<String, u64>) {
    let mut considered = 0u64;
    let mut unpriced: BTreeMap<String, u64> = BTreeMap::new();
    for project in projects {
        let q = SpanQuery {
            project: project.clone(),
            from: Some(from),
            to: Some(to),
            order: Order::Desc,
            limit: PRICE_SAMPLE,
            ..SpanQuery::default()
        };
        let Ok(page) = store.list_spans(&q).await else { continue };
        for s in &page.items {
            let (Some(model), Some(usage)) = (s.model.as_deref(), s.usage.as_ref()) else {
                continue;
            };
            if usage.input_tokens.is_none() && usage.output_tokens.is_none() {
                continue;
            }
            considered += 1;
            if book.lookup_with_provider(model, s.provider.as_deref()).is_none() {
                *unpriced.entry(model.to_owned()).or_default() += 1;
            }
        }
    }
    (considered, unpriced)
}

/// The run store: whether it answers, what it holds, and whether the run
/// surface is open on an address other people can reach.
///
/// Read only. A doctor that claimed a step or fired a deadline to prove the
/// store works would be a doctor that moves runs, so it reads the definitions
/// and the running runs of every project the configuration names and nothing
/// else.
async fn check_runs(
    config: &ServerConfig,
    runs: Option<&dyn mcpd_runs::RunStore>,
    listen: &str,
) -> Vec<Finding> {
    let Some(runs) = runs else {
        return vec![Finding::skipped(
            "runs",
            "the run store is off (--runs-data off). Definitions, runs and the worker \
             protocol answer 503, and nothing here was evaluated.",
        )];
    };
    let mut out = Vec::new();
    let mut projects: BTreeSet<String> = config.projects.keys().cloned().collect();
    projects.insert(config.default_project.clone());
    let mut definitions = 0usize;
    let mut running = 0usize;
    for project in &projects {
        match runs.list_definitions(project).await {
            Ok(defs) => definitions += defs.len(),
            Err(e) => {
                out.push(Finding::fault(
                    "runs",
                    format!("the run store could not list definitions for project {project}: {e}"),
                ));
                return out;
            }
        }
        match runs
            .list_runs(
                mcpd_runs::RunFilter {
                    project: project.clone(),
                    status: Some(mcpd_runs::RunStatus::Running),
                    definition_id: None,
                },
                500,
                None,
            )
            .await
        {
            Ok(page) => running += page.runs.len(),
            Err(e) => {
                out.push(Finding::fault(
                    "runs",
                    format!("the run store could not list runs for project {project}: {e}"),
                ));
                return out;
            }
        }
    }
    out.push(Finding::ok(
        "runs",
        format!(
            "the run store answers: {} across {} and {} running",
            plural(definitions, "definition version"),
            plural(projects.len(), "project"),
            plural(running, "run")
        ),
    ));
    if definitions == 0 {
        out.push(Finding::warn(
            "runs",
            "no definition is stored in any configured project. A worker registers one with \
             POST /v1/api/workers/register the first time it starts; until then there is \
             nothing to run.",
        ));
    }
    let loopback = listen
        .rsplit_once(':')
        .and_then(|(host, _)| {
            host.trim_matches(|c| c == '[' || c == ']').parse::<std::net::IpAddr>().ok()
        })
        .is_some_and(|ip| ip.is_loopback());
    if config.keys_for(mcpd_server::auth::Scope::Execute).is_empty() && !loopback {
        out.push(Finding::warn(
            "runs",
            "the execute and worker scopes need no key, and the daemon is not listening on \
             a loopback address. Anyone who can reach the port can start runs and claim \
             steps. Set --execute-key and --worker-key.",
        ));
    }
    out
}

fn check_pricing(considered: u64, unpriced: &BTreeMap<String, u64>, window: Duration) -> Finding {
    if considered == 0 {
        return Finding::skipped(
            "pricing",
            format!("no model span with a token count was stored in the last {}", human(window)),
        );
    }
    if unpriced.is_empty() {
        return Finding::ok(
            "pricing",
            format!("every one of the {considered} recent model spans sampled was priced"),
        );
    }
    let missed: u64 = unpriced.values().sum();
    let names: Vec<String> = unpriced.iter().map(|(m, n)| format!("{m} ({n})")).take(6).collect();
    // Worded so the sentence reads correctly for one span and for a thousand.
    Finding::warn(
        "pricing",
        format!(
            "no cost was recorded for {missed} of {considered} recent model spans \
             sampled, because the built-in price book has no rates for {}. That spend is \
             reported as zero everywhere it is summed.",
            names.join(", ")
        ),
    )
}

// ----------------------------------------------------------------- printing --

fn print_report(findings: &[Finding], inputs: &Inputs) {
    println!("mcpd doctor {}\n", mcpd_core::VERSION);
    println!("what was graded");
    println!("  store        {}", inputs.store);
    println!("  config file  {}", inputs.config_file);
    println!("  listen       {}\n", inputs.listen);

    for group in GROUPS {
        let mut any = false;
        for f in findings.iter().filter(|f| f.group == group) {
            if !any {
                println!("{group}");
                any = true;
            }
            // Wrap by hand rather than pulling in a dependency for it. The
            // continuation is indented past the tag so a long message stays
            // readable next to a short one.
            for (i, line) in wrap(&f.message, 76).into_iter().enumerate() {
                if i == 0 {
                    println!("  {}  {line}", f.level.tag());
                } else {
                    println!("           {line}");
                }
            }
        }
        if any {
            println!();
        }
    }

    let count = |l: Level| findings.iter().filter(|f| f.level == l).count();
    let (ok, skipped, warn, fault) =
        (count(Level::Ok), count(Level::Skipped), count(Level::Warn), count(Level::Fault));
    println!("{ok} ok, {skipped} skipped, {}, {}", plural(warn, "warning"), plural(fault, "fault"));
    if skipped > 0 {
        // The distinction the summary exists to draw. Everything absent from
        // the configuration lands in the skipped count, and a reader who takes
        // it for a pass has read a clean bill of health for a subsystem nobody
        // looked at.
        println!("A skipped check proved nothing: it was not run, or there was nothing there");
        println!("to run it against. Only the {ok} ok were verified.");
    }
    if fault > 0 {
        println!("A fault is something broken or open. Exiting 2.");
    }
}

/// Break a message into lines no longer than `width`, at spaces.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// "1 project", "3 projects".
fn plural(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else if let Some(stem) = noun.strip_suffix("on") {
        // criterion, criteria. The only irregular noun this command prints.
        format!("{n} {stem}a")
    } else {
        format!("{n} {noun}s")
    }
}

/// The first few names of a set, with a count of the rest.
fn preview(names: &BTreeSet<String>, max: usize) -> String {
    let shown: Vec<&str> = names.iter().take(max).map(String::as_str).collect();
    if names.len() > max {
        format!("{}, and {} more", shown.join(", "), names.len() - max)
    } else {
        shown.join(", ")
    }
}

/// A wait in seconds, for a line somebody reads.
///
/// Not `human`, which is for the age of stored data and rounds a three hundred
/// millisecond wait to "0 seconds". The write probe's number is the budget it
/// spent, and the point of printing it is that it was deliberate.
fn seconds(d: std::time::Duration) -> String {
    format!("{:.1} seconds", d.as_secs_f64())
}

/// A duration in the largest unit that keeps it readable.
fn human(d: Duration) -> String {
    let s = d.whole_seconds().abs();
    match s {
        0..=119 => format!("{s} seconds"),
        120..=7199 => format!("{} minutes", s / 60),
        7200..=172_799 => format!("{} hours", s / 3600),
        _ => format!("{} days", s / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcpd_server::forward::{Destination, Vendor};
    use mcpd_server::judge::JudgeConfig;
    use mcpd_server::monitor::{Compare, Monitor, Slo};
    use mcpd_server::state::ProjectConfig;
    use mcpd_store::SqliteStore;

    // ------------------------------------------------------------- helpers --

    fn levels(findings: &[Finding], level: Level) -> Vec<&str> {
        findings.iter().filter(|f| f.level == level).map(|f| f.message.as_str()).collect()
    }

    /// Whether any finding at `level` mentions every one of `needles`.
    fn says(findings: &[Finding], level: Level, needles: &[&str]) -> bool {
        levels(findings, level).iter().any(|m| needles.iter().all(|n| m.contains(n)))
    }

    fn config() -> ServerConfig {
        ServerConfig::default()
    }

    fn keys(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    fn set(v: &[&str]) -> BTreeSet<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    fn monitor(id: &str) -> Monitor {
        Monitor {
            id: id.to_owned(),
            name: "a monitor".to_owned(),
            project: "default".to_owned(),
            watch: Watch::ErrorRate,
            scorer_id: None,
            compare: Compare::Above,
            threshold: 0.1,
            window_seconds: 300,
            min_samples: 20,
            z_bound: 3.5,
            webhook: Some("https://example.invalid/hook".to_owned()),
            enabled: true,
        }
    }

    fn slo(id: &str) -> Slo {
        Slo {
            id: id.to_owned(),
            name: "an objective".to_owned(),
            project: "default".to_owned(),
            objective: 0.995,
            window_days: 30,
            min_samples: 20,
            webhook: Some("https://example.invalid/hook".to_owned()),
            enabled: true,
        }
    }

    /// A store on its own file, removed when the test ends.
    ///
    /// Not `:memory:`: every in-memory `SqliteStore` in one process shares the
    /// same `file:mcpd-mem?cache=shared` database, so two tests running at once
    /// would read each other's rows. The name carries the test's own name, so
    /// two paths can never collide however close together they start.
    struct TempStore {
        store: SqliteStore,
        path: std::path::PathBuf,
    }

    impl Drop for TempStore {
        fn drop(&mut self) {
            for suffix in ["", "-wal", "-shm"] {
                let _ = std::fs::remove_file(format!("{}{suffix}", self.path.display()));
            }
        }
    }

    async fn temp_store(test: &str) -> TempStore {
        let path =
            std::env::temp_dir().join(format!("mcpd-doctor-{test}-{}.db", std::process::id()));
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
        }
        let store = SqliteStore::open(&path, 2).expect("open a temp store");
        store.migrate().await.expect("migrate a temp store");
        TempStore { store, path }
    }

    /// A stored span, with the two timestamps the clock check compares.
    fn span_at(
        project: &str,
        trace: &str,
        start: OffsetDateTime,
        received: OffsetDateTime,
    ) -> mcpd_core::Span {
        let mut s = probe_span(start);
        s.trace_id = trace.to_owned();
        s.span_id = trace[..16].to_owned();
        s.name = "run".to_owned();
        s.correlation.project = project.to_owned();
        s.start_time = start;
        s.end_time = Some(start);
        s.received_at = received;
        s
    }

    // ---------------------------------------------------------------- keys --

    /// Every placeholder the example files in `deploy/` ship, as they ship.
    const PLACEHOLDERS: [&str; 8] = [
        "REPLACE_WITH_A_RANDOM_KEY",
        "REPLACE_WITH_A_DIFFERENT_RANDOM_KEY",
        "REPLACE_WITH_A_THIRD_RANDOM_KEY",
        "REPLACE_WITH_A_FIFTH_RANDOM_KEY",
        "REPLACE_WITH_A_SIXTH_RANDOM_KEY",
        "replace-me-ingest",
        "replace-me-read",
        "Replace-Me-Retyped",
    ];

    #[test]
    fn a_placeholder_key_is_a_fault_in_every_scope_and_names_the_scope_not_the_key() {
        // Each one is longer than SHORT_KEY, so the length check passed them,
        // and each one is printed in a file anybody can read.
        type Slot = fn(&mut ServerConfig) -> &mut Vec<String>;
        let lists: [(&str, Slot); 6] = [
            ("--ingest-key", |c| &mut c.ingest_keys),
            ("--read-key", |c| &mut c.read_keys),
            ("--write-key", |c| &mut c.write_keys),
            ("--admin-key", |c| &mut c.admin_keys),
            ("--execute-key", |c| &mut c.execute_keys),
            ("--worker-key", |c| &mut c.worker_keys),
        ];
        for placeholder in PLACEHOLDERS {
            for (flag, slot) in lists {
                let mut c = config();
                *slot(&mut c) = keys(&["a-real-key-that-was-generated", placeholder]);
                let f = check_keys(Vec::new(), &c, "0.0.0.0:4317");
                assert!(
                    says(&f, Level::Fault, &[&format!("entry 2 of {flag}"), "placeholder"]),
                    "{placeholder} under {flag} was not refused: {f:?}"
                );
                let all = f.iter().map(|f| f.message.clone()).collect::<String>();
                assert!(!all.contains(placeholder), "the key itself was printed: {all}");
            }
        }
    }

    #[test]
    fn a_key_list_that_holds_only_blank_entries_is_a_fault_naming_the_flag_and_variable() {
        type Field = fn(&mut ServerConfig) -> &mut Vec<String>;
        let lists: [(&str, &str, Field); 6] = [
            ("--ingest-key", "MCPD_INGEST_KEYS", |c| &mut c.ingest_keys),
            ("--read-key", "MCPD_READ_KEYS", |c| &mut c.read_keys),
            ("--write-key", "MCPD_WRITE_KEYS", |c| &mut c.write_keys),
            ("--admin-key", "MCPD_ADMIN_KEYS", |c| &mut c.admin_keys),
            ("--execute-key", "MCPD_EXECUTE_KEYS", |c| &mut c.execute_keys),
            ("--worker-key", "MCPD_WORKER_KEYS", |c| &mut c.worker_keys),
        ];
        for (flag, variable, field) in lists {
            for blank in [&[""][..], &["", ""], &[" "], &["\t", "  "]] {
                let mut c = config();
                *field(&mut c) = keys(blank);
                let f = check_blank_key_lists(&c);
                assert!(
                    says(
                        &f,
                        Level::Fault,
                        &[
                            &format!("{flag} (or {variable}) is set but holds no key"),
                            &format!("remove {variable} to leave the scope unset"),
                        ]
                    ),
                    "{flag} {blank:?}: {f:?}"
                );
                // The doctor report carries it in the keys group, without the
                // all-clear the dropped lists alone would earn.
                c.drop_blank_keys();
                let report = check_keys(f, &c, "127.0.0.1:4317");
                assert!(says(&report, Level::Fault, &[flag]), "{report:?}");
                assert!(!says(&report, Level::Ok, &["every key is usable"]), "{report:?}");
            }

            // A blank entry beside a real key is a trailing comma, not a list
            // with nothing in it.
            let mut c = config();
            *field(&mut c) = keys(&["9f86d081884c7d659a2feaa0c55ad015", ""]);
            assert!(check_blank_key_lists(&c).is_empty(), "{flag}");
        }
        // Unset is unset.
        assert!(check_blank_key_lists(&config()).is_empty());
    }

    #[test]
    fn a_project_key_list_that_holds_only_blank_entries_is_a_fault_too() {
        for scope in ["ingest", "read", "write", "admin", "execute", "worker"] {
            let mut p = ProjectConfig::default();
            let blank = keys(&["", " "]);
            match scope {
                "ingest" => p.ingest_keys = blank,
                "read" => p.read_keys = blank,
                "write" => p.write_keys = blank,
                "admin" => p.admin_keys = blank,
                "execute" => p.execute_keys = blank,
                _ => p.worker_keys = blank,
            }
            let mut c = config();
            c.projects.insert("checkout".to_owned(), p);
            let f = check_blank_key_lists(&c);
            assert!(
                says(
                    &f,
                    Level::Fault,
                    &[
                        &format!("projects.checkout.{scope}_keys in the config file"),
                        "remove it from the config file",
                    ]
                ),
                "{scope}: {f:?}"
            );
        }
    }

    #[test]
    fn a_placeholder_under_a_project_is_a_fault_too() {
        for (scope, project) in [
            (
                "execute",
                ProjectConfig { execute_keys: keys(&[PLACEHOLDERS[0]]), ..Default::default() },
            ),
            (
                "worker",
                ProjectConfig { worker_keys: keys(&[PLACEHOLDERS[5]]), ..Default::default() },
            ),
            ("read", ProjectConfig { read_keys: keys(&[PLACEHOLDERS[6]]), ..Default::default() }),
        ] {
            let mut c = config();
            c.projects.insert("checkout".to_owned(), project);
            let f = preflight(&c, OFF_LOOPBACK, true);
            assert!(
                says(
                    &f,
                    Level::Fault,
                    &[&format!("projects.checkout.{scope}_keys"), "openssl rand -hex 32"]
                ),
                "{f:?}"
            );
        }
    }

    #[test]
    fn a_key_that_only_starts_like_a_word_is_not_a_placeholder() {
        assert!(!is_placeholder("replacement-ingest-key-0f3a9c"));
        assert!(!is_placeholder("9f86d081884c7d659a2feaa0c55ad015"));
        assert!(is_placeholder("  replace-me-ingest"));
    }

    #[test]
    fn the_run_scopes_are_graded_like_the_other_four() {
        // `key_lists` named four scopes, so an execute or worker key that could
        // never match was never reported, at the top level or under a project.
        let mut c = config();
        c.execute_keys = keys(&[" an-execute-key-with-a-leading-space"]);
        c.projects.insert(
            "checkout".to_owned(),
            ProjectConfig {
                worker_keys: keys(&["a-worker-key-with-a-trailing-space "]),
                ..ProjectConfig::default()
            },
        );
        let f = check_keys(Vec::new(), &c, "0.0.0.0:4317");
        assert!(says(&f, Level::Fault, &["entry 1 of --execute-key", "trimmed"]), "{f:?}");
        assert!(
            says(&f, Level::Fault, &["entry 1 of projects.checkout.worker_keys", "trimmed"]),
            "{f:?}"
        );
    }

    #[test]
    fn an_ingest_key_that_also_opens_the_run_surface_is_named_with_the_flags_that_close_it() {
        // The shape every example file in deploy/ starts from: an ingest key and
        // a read key. Execute and worker fall back to write, write falls back to
        // ingest, so the fleet's key starts runs and reads step inputs.
        let mut c = config();
        c.ingest_keys = keys(&["the-fleets-ingest-key-0123"]);
        c.read_keys = keys(&["the-readers-key-0123456789"]);
        let f = preflight(&c, OFF_LOOPBACK, true);
        assert!(
            says(
                &f,
                Level::Warn,
                &[
                    "every ingest key can also start runs and claim steps",
                    "--execute-key and --worker-key"
                ]
            ),
            "{f:?}"
        );
        assert!(!f.iter().any(|x| x.level == Level::Fault), "a fallback is not a fault: {f:?}");

        // One flag set closes half of it, and the line names the other half.
        c.execute_keys = keys(&["the-execute-key-0123456789"]);
        let f = preflight(&c, OFF_LOOPBACK, true);
        assert!(says(&f, Level::Warn, &["no --worker-key is set", "claim steps"]), "{f:?}");
        assert!(!says(&f, Level::Warn, &["--execute-key"]), "{f:?}");

        // A write key moves the inheritance off the ingest list entirely.
        c.execute_keys.clear();
        c.write_keys = keys(&["the-write-key-0123456789ab"]);
        assert!(check_run_key_reach(&c).is_empty(), "{:?}", check_run_key_reach(&c));
    }

    #[test]
    fn the_same_is_said_of_a_project_block() {
        let mut c = config();
        c.projects.insert(
            "checkout".to_owned(),
            ProjectConfig {
                ingest_keys: keys(&["the-checkout-ingest-key-01"]),
                read_keys: keys(&["the-checkout-read-key-0123"]),
                admin_keys: keys(&["the-checkout-admin-key-012"]),
                ..ProjectConfig::default()
            },
        );
        let f = check_run_key_reach(&c);
        assert!(
            says(
                &f,
                Level::Warn,
                &[
                    "projects.checkout",
                    "projects.checkout.execute_keys and projects.checkout.worker_keys"
                ]
            ),
            "{f:?}"
        );
    }

    #[test]
    fn a_daemon_with_no_run_store_is_not_told_about_its_run_scopes() {
        let mut c = config();
        c.ingest_keys = keys(&["the-fleets-ingest-key-0123"]);
        c.read_keys = keys(&["the-readers-key-0123456789"]);
        c.admin_keys = keys(&["the-admins-key-0123456789"]);
        c.write_keys = Vec::new();
        assert!(says(&preflight(&c, OFF_LOOPBACK, true), Level::Warn, &["claim steps"]));
        let f = preflight(&c, OFF_LOOPBACK, false);
        assert!(!says(&f, Level::Warn, &["claim steps"]), "{f:?}");
    }

    #[test]
    fn a_key_with_an_edge_of_whitespace_is_a_fault() {
        // `MCPD_READ_KEYS="good, spaced"` yields " spaced", and a presented key
        // is trimmed before it is compared. Verified against a running daemon:
        // that key is refused in every form.
        let mut c = config();
        c.read_keys = keys(&["goodkeygoodkeygood", " spacedspacedspaced"]);
        let f = check_keys(Vec::new(), &c, "0.0.0.0:4317");
        assert!(says(&f, Level::Fault, &["entry 2 of --read-key", "trimmed"]), "{f:?}");
    }

    #[test]
    fn a_usable_key_is_not_reported() {
        let mut c = config();
        c.read_keys = keys(&["a-perfectly-ordinary-key"]);
        c.ingest_keys = keys(&["another-ordinary-key"]);
        c.write_keys = keys(&["a-third-ordinary-key"]);
        c.admin_keys = keys(&["a-fourth-ordinary-key"]);
        let f = check_keys(Vec::new(), &c, "0.0.0.0:4317");
        assert!(levels(&f, Level::Fault).is_empty(), "{f:?}");
        assert!(levels(&f, Level::Warn).is_empty(), "{f:?}");
    }

    #[test]
    fn a_short_key_is_only_a_warning_and_is_counted_once() {
        let mut c = config();
        c.read_keys = keys(&["short", "alsoshort"]);
        c.ingest_keys = keys(&["tiny"]);
        let f = check_keys(Vec::new(), &c, "127.0.0.1:4317");
        let warns: Vec<&str> =
            levels(&f, Level::Warn).into_iter().filter(|m| m.contains("shorter than")).collect();
        assert_eq!(warns.len(), 1, "{f:?}");
        assert!(warns[0].contains("3 configured keys are"), "{f:?}");
    }

    #[test]
    fn an_open_read_scope_is_ok_on_loopback_and_a_fault_on_a_routable_address() {
        let c = config();
        let local = check_keys(Vec::new(), &c, "127.0.0.1:4317");
        assert!(says(&local, Level::Ok, &["ingest scope is open"]), "{local:?}");
        assert!(says(&local, Level::Ok, &["read scope is open"]), "{local:?}");
        assert!(levels(&local, Level::Warn).is_empty(), "{local:?}");

        let public = check_keys(Vec::new(), &c, "0.0.0.0:4317");
        // Ingest costs rows, which retention removes. An open read scope
        // discloses stored prompts and completions, which nothing takes back,
        // so that one is the fault and it gates a deploy.
        assert!(
            says(&public, Level::Warn, &["ingest scope is open", "--ingest-key"]),
            "{public:?}"
        );
        assert!(
            says(&public, Level::Fault, &["read scope is open", "--read-key", "--listen"]),
            "{public:?}"
        );
    }

    #[test]
    fn an_unset_admin_key_warns_that_every_reader_can_delete() {
        let mut c = config();
        c.read_keys = keys(&["a-perfectly-ordinary-key"]);
        let f = check_keys(Vec::new(), &c, "0.0.0.0:4317");
        assert!(says(&f, Level::Warn, &["no --admin-key", "audit log"]), "{f:?}");
    }

    #[test]
    fn an_unset_write_key_warns_that_every_ingest_key_can_write() {
        let mut c = config();
        c.ingest_keys = keys(&["a-perfectly-ordinary-key"]);
        let f = check_keys(Vec::new(), &c, "0.0.0.0:4317");
        assert!(says(&f, Level::Warn, &["no --write-key", "datasets"]), "{f:?}");
    }

    #[test]
    fn a_project_that_can_write_but_not_read_itself_warns() {
        // The team is handed a key that writes to their project. Reading it
        // back is a 403, because the project block names no read key and the
        // read scope is closed by a top-level key they do not hold.
        let mut c = config();
        c.read_keys = keys(&["the-platform-teams-read-key"]);
        c.projects.insert(
            "checkout".to_owned(),
            ProjectConfig {
                ingest_keys: keys(&["the-checkout-teams-key"]),
                ..ProjectConfig::default()
            },
        );
        let f = check_keys(Vec::new(), &c, "0.0.0.0:4317");
        assert!(says(&f, Level::Warn, &["projects.checkout", "refused when it reads"]), "{f:?}");
    }

    #[test]
    fn a_project_with_ingest_keys_and_nobody_guarding_reads_warns_differently() {
        // Same block, but no read key anywhere. The read scope is open, so the
        // problem is the opposite: everyone can read it.
        let mut c = config();
        c.projects.insert(
            "checkout".to_owned(),
            ProjectConfig {
                ingest_keys: keys(&["the-checkout-teams-key"]),
                ..ProjectConfig::default()
            },
        );
        let f = check_keys(Vec::new(), &c, "0.0.0.0:4317");
        assert!(says(&f, Level::Warn, &["anyone can read checkout"]), "{f:?}");
    }

    #[test]
    fn one_key_under_two_projects_warns_that_they_are_not_separated() {
        let mut c = config();
        for name in ["checkout", "search"] {
            c.projects.insert(
                name.to_owned(),
                ProjectConfig {
                    read_keys: keys(&["the-same-key-in-both-blocks"]),
                    ..ProjectConfig::default()
                },
            );
        }
        let f = check_keys(Vec::new(), &c, "0.0.0.0:4317");
        assert!(says(&f, Level::Warn, &["under 2 projects", "checkout, search"]), "{f:?}");
    }

    #[test]
    fn a_key_set_at_both_levels_warns_that_the_project_block_does_nothing() {
        let mut c = config();
        c.read_keys = keys(&["a-key-in-two-places-at-once"]);
        c.projects.insert(
            "checkout".to_owned(),
            ProjectConfig {
                read_keys: keys(&["a-key-in-two-places-at-once"]),
                ..ProjectConfig::default()
            },
        );
        let f = check_keys(Vec::new(), &c, "0.0.0.0:4317");
        assert!(says(&f, Level::Warn, &["--read-key", "does not narrow it"]), "{f:?}");
    }

    #[test]
    fn a_key_digest_is_printed_and_the_key_never_is() {
        let secret = "a-key-in-two-places-at-once";
        let mut c = config();
        c.read_keys = keys(&[secret]);
        c.projects.insert(
            "checkout".to_owned(),
            ProjectConfig { read_keys: keys(&[secret]), ..ProjectConfig::default() },
        );
        let f = check_keys(Vec::new(), &c, "0.0.0.0:4317");
        let all = f.iter().map(|f| f.message.clone()).collect::<String>();
        assert!(!all.contains(secret), "the key itself was printed: {all}");
        assert!(all.contains(&mcpd_server::auth::key_digest(secret)), "{all}");
    }

    // ------------------------------------------------------------ projects --

    // ------------------------------------------------ limits and identity --

    #[test]
    fn nothing_limiting_ingest_is_skipped_rather_than_left_out_of_the_report() {
        // Before this, quotas and sampling appeared nowhere at all: not ok, not
        // skipped, not mentioned. A reader could not tell an unbounded store
        // from one this command had checked and approved.
        let f = check_limits(&config());
        assert!(says(&f, Level::Skipped, &["no ingest ceiling", "--quota-per-minute"]), "{f:?}");
        assert!(says(&f, Level::Skipped, &["no sampling", "Every trace"]), "{f:?}");
        assert!(levels(&f, Level::Ok).is_empty(), "nothing was checked, so nothing is ok");
    }

    #[test]
    fn a_ceiling_on_named_projects_alone_warns_that_any_other_name_escapes_it() {
        // The configuration that reads as limited and is not. `alpha` is
        // bounded; a sender that writes `alpha-staging` in the body is not.
        let mut c = config();
        c.projects.insert(
            "alpha".to_owned(),
            ProjectConfig { quota_per_minute: Some(500), ..Default::default() },
        );
        let f = check_limits(&c);
        assert!(says(&f, Level::Ok, &["alpha 500/min"]), "{f:?}");
        assert!(
            says(
                &f,
                Level::Warn,
                &["no `projects` block names has no ceiling", "--quota-per-minute"]
            ),
            "{f:?}"
        );
    }

    #[test]
    fn a_top_level_ceiling_reports_the_one_every_unnamed_project_shares() {
        let mut c = config();
        c.quota_per_minute = Some(20_000);
        let f = check_limits(&c);
        assert!(says(&f, Level::Ok, &["default 20000/min"]), "{f:?}");
        assert!(says(&f, Level::Ok, &["does not name shares one ceiling of 20000"]), "{f:?}");
        assert!(levels(&f, Level::Warn).iter().all(|m| !m.contains("no ceiling at all")), "{f:?}");
    }

    #[test]
    fn a_ceiling_below_one_batch_warns_that_a_well_behaved_sender_is_trimmed() {
        let mut c = config();
        c.max_batch_signals = 10_000;
        c.quota_per_minute = Some(100);
        let f = check_limits(&c);
        assert!(
            says(&f, Level::Warn, &["--max-batch-signals (10000)", "default (100/min)"]),
            "{f:?}"
        );
    }

    #[test]
    fn a_project_sampling_outside_the_scale_is_a_fault_the_flag_would_have_refused() {
        // `--sample 4.2` is refused at startup. The same number under a project
        // in the config file was accepted, and the daemon then ran a fraction
        // that means nothing.
        let mut c = config();
        c.projects
            .insert("alpha".to_owned(), ProjectConfig { sample: Some(4.2), ..Default::default() });
        c.projects
            .insert("beta".to_owned(), ProjectConfig { sample: Some(-1.0), ..Default::default() });
        let f = check_limits(&c);
        assert!(says(&f, Level::Fault, &["alpha (4.2)", "beta (-1)"]), "{f:?}");
    }

    #[test]
    fn a_project_that_keeps_nothing_warns_that_its_monitors_read_an_empty_store() {
        let mut c = config();
        c.projects
            .insert("alpha".to_owned(), ProjectConfig { sample: Some(0.0), ..Default::default() });
        let f = check_limits(&c);
        assert!(says(&f, Level::Warn, &["keeps no traces at all", "alpha"]), "{f:?}");
    }

    #[test]
    fn a_sampling_fraction_that_is_configured_and_sane_reads_back_as_a_percentage() {
        let mut c = config();
        c.sampling = mcpd_core::sampling::Policy::deterministic(0.1);
        c.projects
            .insert("alpha".to_owned(), ProjectConfig { sample: Some(0.5), ..Default::default() });
        let f = check_limits(&c);
        assert!(says(&f, Level::Ok, &["globally, 10% of traces are kept", "alpha 50%"]), "{f:?}");

        // And it survives a warning beside it. The fraction is the thing a
        // reader came to check; one project sampling nothing must not take the
        // whole policy off the report.
        c.projects
            .insert("beta".to_owned(), ProjectConfig { sample: Some(0.0), ..Default::default() });
        let f = check_limits(&c);
        assert!(says(&f, Level::Ok, &["alpha 50%", "beta 0%"]), "{f:?}");
        assert!(says(&f, Level::Warn, &["keeps no traces at all"]), "{f:?}");

        // But not beside a fault: a fraction that means nothing is not summarised
        // as though it did.
        c.projects
            .insert("beta".to_owned(), ProjectConfig { sample: Some(9.0), ..Default::default() });
        let f = check_limits(&c);
        assert!(says(&f, Level::Fault, &["beta (9)"]), "{f:?}");
        assert!(levels(&f, Level::Ok).iter().all(|m| !m.contains("of traces are kept")), "{f:?}");
    }

    #[test]
    fn no_identity_header_is_skipped_and_says_what_the_audit_log_loses() {
        let f = check_identity(&config(), "127.0.0.1:4317");
        assert!(
            says(&f, Level::Skipped, &["what was done and not who did it", "--trusted-proxy-cidr"]),
            "{f:?}"
        );
        assert!(levels(&f, Level::Ok).is_empty(), "nothing was checked, so nothing is ok");
    }

    #[test]
    fn a_configured_identity_header_names_the_header_and_the_ranges() {
        let mut c = config();
        c.trusted_identity = mcpd_server::identity::TrustedIdentity::new(
            Some("x-forwarded-user"),
            &["10.0.0.0/8".to_owned()],
        )
        .expect("a valid configuration");
        let f = check_identity(&c, "0.0.0.0:4317");
        assert!(says(&f, Level::Ok, &["x-forwarded-user", "10.0.0.0/8"]), "{f:?}");
    }

    #[test]
    fn a_trusted_range_covering_the_internet_is_a_fault_off_loopback() {
        // The header is a string. Trusting every address means anybody who can
        // reach the port can set it, and every subject in the audit log is then
        // whatever they typed, which is worse than recording none.
        let mut c = config();
        c.trusted_identity = mcpd_server::identity::TrustedIdentity::new(
            Some("x-forwarded-user"),
            &["0.0.0.0/0".to_owned()],
        )
        .expect("a valid configuration");
        let f = check_identity(&c, "0.0.0.0:4317");
        assert!(says(&f, Level::Fault, &["contains the whole internet", "0.0.0.0/0"]), "{f:?}");

        // The same rule the open read scope follows: on a loopback address it
        // is a laptop, not a hole, and a warning must not fail a deploy.
        let f = check_identity(&c, "127.0.0.1:4317");
        assert!(says(&f, Level::Warn, &["contains the whole internet"]), "{f:?}");
        assert!(levels(&f, Level::Fault).is_empty(), "{f:?}");
    }

    #[test]
    fn a_range_written_in_ipv6_is_graded_by_what_it_contains_not_by_its_prefix() {
        let mut c = config();
        c.trusted_identity = mcpd_server::identity::TrustedIdentity::new(
            Some("x-forwarded-user"),
            &["::/0".to_owned()],
        )
        .expect("a valid configuration");
        assert!(
            says(&check_identity(&c, "0.0.0.0:4317"), Level::Fault, &["whole internet"]),
            "an IPv6 default route is the same mistake in another spelling"
        );
    }

    #[test]
    fn a_public_proxy_on_a_single_address_is_not_mistaken_for_an_open_range() {
        // The probe addresses must not make a real, narrow, publicly routable
        // proxy read as a hole.
        let mut c = config();
        c.trusted_identity = mcpd_server::identity::TrustedIdentity::new(
            Some("x-forwarded-user"),
            &["198.51.100.9/32".to_owned()],
        )
        .expect("a valid configuration");
        let f = check_identity(&c, "0.0.0.0:4317");
        assert!(says(&f, Level::Ok, &["198.51.100.9/32"]), "{f:?}");
    }

    #[test]
    fn a_configured_project_that_holds_nothing_warns_about_the_spelling() {
        let mut c = config();
        c.projects.insert("chekout".to_owned(), ProjectConfig::default());
        let f = check_projects(&c, &set(&["default", "checkout"]));
        assert!(says(&f, Level::Warn, &["projects.chekout", "spelling"]), "{f:?}");
    }

    #[test]
    fn a_configured_project_that_holds_runs_is_not_reported() {
        let mut c = config();
        c.projects.insert("checkout".to_owned(), ProjectConfig::default());
        let f = check_projects(&c, &set(&["default", "checkout"]));
        assert!(levels(&f, Level::Warn).is_empty(), "{f:?}");
        assert!(says(&f, Level::Ok, &["stored in 2 projects", "checkout, default"]), "{f:?}");
    }

    #[test]
    fn an_empty_store_points_at_the_data_flag() {
        let f = check_projects(&config(), &BTreeSet::new());
        assert!(says(&f, Level::Warn, &["no runs at all", "--data"]), "{f:?}");
    }

    #[test]
    fn a_default_project_nothing_writes_to_warns() {
        let c = config();
        let f = check_projects(&c, &set(&["checkout"]));
        assert!(says(&f, Level::Warn, &["--project is default"]), "{f:?}");
    }

    // ----------------------------------------------------------- retention --

    #[test]
    fn the_retention_plan_mirrors_the_one_main_builds() {
        // `main` sweeps the projects named in the config file plus the default
        // project, and nothing else. A project a sender created by naming it on
        // a span is in neither list.
        let mut c = config();
        c.retention_days = Some(30);
        c.projects.insert(
            "checkout".to_owned(),
            ProjectConfig { retention_days: Some(90), ..ProjectConfig::default() },
        );
        c.projects.insert("search".to_owned(), ProjectConfig::default());

        let plan = swept_projects(&c);
        assert_eq!(plan.get("checkout"), Some(&90), "a project's own window wins");
        assert_eq!(plan.get("search"), Some(&30), "and falls back to the global one");
        assert_eq!(plan.get("default"), Some(&30), "--project is always swept");
        assert_eq!(plan.len(), 3);
    }

    #[test]
    fn a_project_with_no_window_at_all_is_not_in_the_plan() {
        let mut c = config();
        c.projects.insert("checkout".to_owned(), ProjectConfig::default());
        assert!(swept_projects(&c).is_empty());
    }

    #[test]
    fn a_project_holding_runs_outside_the_plan_warns() {
        let mut c = config();
        c.retention_days = Some(30);
        let now = OffsetDateTime::now_utc();
        let mut activity = BTreeMap::new();
        activity.insert(
            "adhoc".to_owned(),
            Activity { oldest_start: Some(now - Duration::days(200)), ..Activity::default() },
        );
        let f = check_retention(&c, Some(&set(&["default", "adhoc"])), &activity, now);
        assert!(says(&f, Level::Warn, &["adhoc holds runs and is never swept"]), "{f:?}");
        assert!(says(&f, Level::Ok, &["default at 30 days"]), "{f:?}");
    }

    #[test]
    fn old_data_with_no_retention_configured_warns_about_unbounded_growth() {
        let now = OffsetDateTime::now_utc();
        let mut activity = BTreeMap::new();
        activity.insert(
            "default".to_owned(),
            Activity { oldest_start: Some(now - Duration::days(400)), ..Activity::default() },
        );
        let f = check_retention(&config(), Some(&set(&["default"])), &activity, now);
        assert!(says(&f, Level::Warn, &["400 days", "--retention-days"]), "{f:?}");
    }

    #[test]
    fn young_data_with_no_retention_is_not_reported() {
        let now = OffsetDateTime::now_utc();
        let mut activity = BTreeMap::new();
        activity.insert(
            "default".to_owned(),
            Activity { oldest_start: Some(now - Duration::days(3)), ..Activity::default() },
        );
        let f = check_retention(&config(), Some(&set(&["default"])), &activity, now);
        assert!(levels(&f, Level::Warn).is_empty(), "{f:?}");
    }

    // ---------------------------------------------------------- forwarding --

    #[test]
    fn forwarding_that_is_not_configured_is_skipped_rather_than_ok() {
        // Absent is not healthy. A --config-file that names the wrong path
        // makes every one of these blocks absent at once.
        let f = check_forwarding(&config(), &[]);
        assert_eq!(f[0].level, Level::Skipped, "{f:?}");
        assert!(says(&f, Level::Skipped, &["no `forward` block"]), "{f:?}");
    }

    #[test]
    fn a_forwarder_configured_and_disabled_warns() {
        let mut c = config();
        c.forward.destinations = vec![{
            let mut d =
                Destination::new(Vendor::Otlp, Some("https://otlp.example.invalid".to_owned()));
            d.enabled = false;
            d
        }];
        let f = check_forwarding(&c, &[]);
        assert!(says(&f, Level::Warn, &["enabled` is false", "nothing is copied"]), "{f:?}");
    }

    #[test]
    fn a_forward_endpoint_that_answers_is_ok_and_names_the_url_it_posts_to() {
        let mut c = config();
        c.forward.destinations =
            vec![Destination::new(Vendor::Otlp, Some("https://otlp.example.invalid".to_owned()))];
        let f = check_forwarding(&c, &[Probe::Answered(200)]);
        assert!(
            says(&f, Level::Ok, &["https://otlp.example.invalid/v1/traces", "answered 200"]),
            "{f:?}"
        );
    }

    #[test]
    fn a_refused_forward_batch_is_a_fault_that_names_the_headers() {
        let mut c = config();
        c.forward.destinations =
            vec![Destination::new(Vendor::Otlp, Some("https://otlp.example.invalid".to_owned()))];
        let f = check_forwarding(&c, &[Probe::Answered(401)]);
        assert!(says(&f, Level::Fault, &["answered 401", "`headers`"]), "{f:?}");
    }

    #[test]
    fn an_unreachable_forward_endpoint_is_a_fault() {
        let mut c = config();
        c.forward.destinations =
            vec![Destination::new(Vendor::Otlp, Some("https://otlp.example.invalid".to_owned()))];
        let f = check_forwarding(&c, &[Probe::Failed("dns error".to_owned())]);
        assert!(says(&f, Level::Fault, &["could not be reached", "dns error"]), "{f:?}");
    }

    #[test]
    fn a_forward_endpoint_that_was_not_contacted_is_skipped_and_never_ok() {
        let mut c = config();
        c.forward.destinations =
            vec![Destination::new(Vendor::Otlp, Some("https://otlp.example.invalid".to_owned()))];
        let f = check_forwarding(&c, &[Probe::Skipped("--offline".to_owned())]);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].level, Level::Skipped);
    }

    #[tokio::test]
    async fn an_unreachable_forward_endpoint_never_prints_the_token_in_its_url() {
        // The finding cut the credential out of the URL it printed, and then
        // quoted the client's error, which repeats the whole URL. A port that
        // was free a moment ago refuses the connection at once.
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .and_then(|l| l.local_addr())
            .expect("a free port")
            .port();
        let mut c = config();
        c.forward.destinations = vec![Destination::new(
            Vendor::Otlp,
            Some(format!("http://127.0.0.1:{port}/?api_key=s3cr3t-token")),
        )];
        let args = Args { offline: false, read_only: true, window_days: 7 };
        let probe = probe_forward(&c, &args).await;
        assert!(matches!(probe.as_slice(), [Probe::Failed(_)]), "{probe:?}");
        let f = check_forwarding(&c, &probe);
        assert!(says(&f, Level::Fault, &["could not be reached"]), "{f:?}");
        let all: String = f.iter().map(|x| x.message.clone()).collect();
        assert!(!all.contains("s3cr3t-token"), "the token was printed: {all}");
    }

    #[tokio::test]
    async fn an_endpoint_with_no_scheme_never_reaches_the_network() {
        let mut c = config();
        c.forward.destinations =
            vec![Destination::new(Vendor::Otlp, Some("otlp.example.invalid:4318".to_owned()))];
        let args = Args { offline: false, read_only: true, window_days: 7 };
        let probe = probe_forward(&c, &args).await;
        assert_eq!(
            probe,
            vec![Probe::Failed("the endpoint has no http:// or https:// scheme".to_owned())]
        );
    }

    // --------------------------------------------------------------- judge --

    #[test]
    fn a_judge_that_is_not_configured_is_skipped_and_says_what_a_scorer_does_instead() {
        let f = check_judge(&config());
        assert_eq!(f[0].level, Level::Skipped, "{f:?}");
        assert!(says(&f, Level::Skipped, &["no judge is configured"]), "{f:?}");
    }

    #[test]
    fn a_judge_with_no_criteria_is_a_fault() {
        let mut c = config();
        c.judge = Some(JudgeConfig {
            endpoint: "https://api.example.invalid/v1".to_owned(),
            model: "a-model".to_owned(),
            api_key: "k".to_owned(),
            criteria: BTreeMap::new(),
            timeout_seconds: 30,
        });
        let f = check_judge(&c);
        assert!(says(&f, Level::Fault, &["no criteria", "judge.criteria"]), "{f:?}");
    }

    #[test]
    fn a_judge_endpoint_that_already_carries_the_path_is_a_fault() {
        // The request is built as `{endpoint}/chat/completions`, so this one
        // would be posted to a URL with the path in it twice.
        let mut c = config();
        c.judge = Some(JudgeConfig {
            endpoint: "https://api.example.invalid/v1/chat/completions".to_owned(),
            model: "a-model".to_owned(),
            api_key: "k".to_owned(),
            criteria: BTreeMap::from([("helpfulness".to_owned(), "is it helpful".to_owned())]),
            timeout_seconds: 30,
        });
        let f = check_judge(&c);
        assert!(says(&f, Level::Fault, &["appends that path"]), "{f:?}");
    }

    #[test]
    fn a_remote_judge_with_no_credential_warns_and_a_local_one_does_not() {
        let mut c = config();
        let criteria = BTreeMap::from([("helpfulness".to_owned(), "is it helpful".to_owned())]);
        c.judge = Some(JudgeConfig {
            endpoint: "https://api.example.invalid/v1".to_owned(),
            model: "a-model".to_owned(),
            api_key: String::new(),
            criteria: criteria.clone(),
            timeout_seconds: 30,
        });
        assert!(says(&check_judge(&c), Level::Warn, &["judge.api_key` is empty"]));

        c.judge = Some(JudgeConfig {
            endpoint: "http://localhost:11434/v1".to_owned(),
            model: "a-model".to_owned(),
            api_key: String::new(),
            criteria,
            timeout_seconds: 30,
        });
        let local = check_judge(&c);
        assert!(levels(&local, Level::Warn).is_empty(), "{local:?}");
    }

    #[test]
    fn the_judge_is_never_reported_as_working() {
        let mut c = config();
        c.judge = Some(JudgeConfig {
            endpoint: "https://api.example.invalid/v1".to_owned(),
            model: "a-model".to_owned(),
            api_key: "k".to_owned(),
            criteria: BTreeMap::from([("helpfulness".to_owned(), "is it helpful".to_owned())]),
            timeout_seconds: 30,
        });
        assert!(says(&check_judge(&c), Level::Ok, &["was not checked"]));
    }

    // ------------------------------------------------------------ monitors --

    #[test]
    fn no_monitors_at_all_is_skipped_rather_than_ok() {
        let f = check_monitors(
            &config(),
            Some(&set(&["default"])),
            &BTreeMap::new(),
            &BTreeSet::new(),
            Duration::days(7),
        );
        assert_eq!(f[0].level, Level::Skipped, "{f:?}");
        assert!(says(&f, Level::Skipped, &["no monitor and no objective"]), "{f:?}");
    }

    #[test]
    fn no_retention_at_all_is_skipped_rather_than_ok() {
        let f = check_retention(
            &config(),
            Some(&BTreeSet::new()),
            &BTreeMap::new(),
            OffsetDateTime::now_utc(),
        );
        assert_eq!(f[0].level, Level::Skipped, "{f:?}");
        assert!(says(&f, Level::Skipped, &["no retention is configured"]), "{f:?}");
    }

    #[test]
    fn a_monitor_with_no_webhook_warns_that_it_notifies_nobody() {
        let mut c = config();
        let mut m = monitor("errors");
        m.webhook = None;
        c.monitors = vec![m];
        let f = check_monitors(
            &c,
            Some(&set(&["default"])),
            &BTreeMap::new(),
            &BTreeSet::new(),
            Duration::days(7),
        );
        assert!(
            says(&f, Level::Warn, &["monitor errors has no webhook", "notifies nobody"]),
            "{f:?}"
        );
    }

    #[test]
    fn a_disabled_monitor_warns_once_and_is_not_checked_further() {
        let mut c = config();
        let mut m = monitor("errors");
        m.enabled = false;
        m.webhook = None;
        c.monitors = vec![m];
        let f = check_monitors(
            &c,
            Some(&set(&["default"])),
            &BTreeMap::new(),
            &BTreeSet::new(),
            Duration::days(7),
        );
        let warns = levels(&f, Level::Warn);
        assert_eq!(warns.len(), 1, "{f:?}");
        assert!(warns[0].contains("is disabled"), "{f:?}");
    }

    #[test]
    fn score_mean_with_no_scorer_id_is_a_fault() {
        // `monitor.rs` reads `scorer_id.unwrap_or_default()`, so the query asks
        // for scores from a scorer named "" and the monitor never trips.
        let mut c = config();
        let mut m = monitor("quality");
        m.watch = Watch::ScoreMean;
        m.scorer_id = None;
        c.monitors = vec![m];
        let f = check_monitors(
            &c,
            Some(&set(&["default"])),
            &BTreeMap::new(),
            &BTreeSet::new(),
            Duration::days(7),
        );
        assert!(says(&f, Level::Fault, &["no `scorer_id`", "can never trip"]), "{f:?}");
    }

    #[test]
    fn a_scorer_that_has_never_produced_a_score_warns() {
        let mut c = config();
        let mut m = monitor("quality");
        m.watch = Watch::ScoreMean;
        m.scorer_id = Some("answer_relevence".to_owned());
        c.monitors = vec![m];
        let f = check_monitors(
            &c,
            Some(&set(&["default"])),
            &BTreeMap::new(),
            &set(&["answer_relevence"]),
            Duration::days(7),
        );
        assert!(says(&f, Level::Warn, &["answer_relevence", "has ever been stored"]), "{f:?}");
    }

    #[test]
    fn a_monitor_watching_a_project_with_no_runs_warns() {
        let mut c = config();
        let mut m = monitor("errors");
        m.project = "chekout".to_owned();
        c.monitors = vec![m];
        let f = check_monitors(
            &c,
            Some(&set(&["checkout"])),
            &BTreeMap::new(),
            &BTreeSet::new(),
            Duration::days(7),
        );
        assert!(says(&f, Level::Warn, &["watches project chekout", "holds no runs"]), "{f:?}");
    }

    #[test]
    fn a_monitor_that_can_never_gather_enough_samples_warns() {
        let mut c = config();
        c.monitors = vec![monitor("errors")];
        let mut activity = BTreeMap::new();
        // 7 days of 5 minute windows is 2016 windows, and 100 runs across them
        // is far below the 20 the monitor needs in each one.
        activity
            .insert("default".to_owned(), Activity { recent_traces: 100, ..Activity::default() });
        let f = check_monitors(
            &c,
            Some(&set(&["default"])),
            &activity,
            &BTreeSet::new(),
            Duration::days(7),
        );
        assert!(says(&f, Level::Warn, &["stored 100 runs", "2016 such windows"]), "{f:?}");
    }

    #[test]
    fn a_busy_project_does_not_trigger_the_sample_warning() {
        let mut c = config();
        c.monitors = vec![monitor("errors")];
        let mut activity = BTreeMap::new();
        activity.insert(
            "default".to_owned(),
            Activity { recent_traces: 500_000, ..Activity::default() },
        );
        let f = check_monitors(
            &c,
            Some(&set(&["default"])),
            &activity,
            &BTreeSet::new(),
            Duration::days(7),
        );
        assert!(levels(&f, Level::Warn).is_empty(), "{f:?}");
    }

    #[test]
    fn an_objective_written_as_a_percentage_is_a_fault() {
        // The burn rate divides by `1 - objective`, so 99.5 leaves no budget.
        let mut c = config();
        let mut s = slo("availability");
        s.objective = 99.5;
        c.slos = vec![s];
        let f = check_monitors(
            &c,
            Some(&set(&["default"])),
            &BTreeMap::new(),
            &BTreeSet::new(),
            Duration::days(7),
        );
        assert!(says(&f, Level::Fault, &["not a fraction between 0", "0.995"]), "{f:?}");
    }

    #[test]
    fn an_objective_with_no_webhook_warns() {
        let mut c = config();
        let mut s = slo("availability");
        s.webhook = None;
        c.slos = vec![s];
        let f = check_monitors(
            &c,
            Some(&set(&["default"])),
            &BTreeMap::new(),
            &BTreeSet::new(),
            Duration::days(7),
        );
        assert!(says(&f, Level::Warn, &["objective availability has no webhook"]), "{f:?}");
    }

    // --------------------------------------------------------------- clock --

    #[test]
    fn a_sender_whose_clock_disagrees_with_the_store_warns() {
        let now = OffsetDateTime::now_utc();
        let mut activity = BTreeMap::new();
        activity.insert(
            "default".to_owned(),
            Activity {
                newest_received: Some(now - Duration::minutes(1)),
                worst_skew: Some(("abc123".to_owned(), Duration::hours(-2))),
                ..Activity::default()
            },
        );
        let f = check_clock(&activity, now);
        assert!(says(&f, Level::Warn, &["abc123", "2 hours before", "start_time"]), "{f:?}");
    }

    #[test]
    fn a_run_received_in_the_future_warns() {
        let now = OffsetDateTime::now_utc();
        let mut activity = BTreeMap::new();
        activity.insert(
            "default".to_owned(),
            Activity { newest_received: Some(now + Duration::hours(3)), ..Activity::default() },
        );
        let f = check_clock(&activity, now);
        assert!(says(&f, Level::Warn, &["in the future"]), "{f:?}");
    }

    #[test]
    fn clocks_that_agree_are_ok_and_an_empty_store_is_skipped() {
        let now = OffsetDateTime::now_utc();
        let mut activity = BTreeMap::new();
        activity.insert(
            "default".to_owned(),
            Activity {
                newest_received: Some(now - Duration::seconds(10)),
                worst_skew: Some(("abc123".to_owned(), Duration::seconds(2))),
                ..Activity::default()
            },
        );
        assert_eq!(check_clock(&activity, now)[0].level, Level::Ok);

        let empty = check_clock(&BTreeMap::new(), now);
        assert_eq!(empty[0].level, Level::Skipped);
    }

    // ------------------------------------------------------------- pricing --

    #[test]
    fn a_model_the_price_book_does_not_know_warns_that_its_spend_reads_as_zero() {
        let unpriced = BTreeMap::from([("acme-turbo-9".to_owned(), 12u64)]);
        let f = check_pricing(40, &unpriced, Duration::days(7));
        assert_eq!(f.level, Level::Warn);
        assert!(f.message.contains("acme-turbo-9 (12)"), "{}", f.message);
        assert!(f.message.contains("reported as zero"), "{}", f.message);
    }

    #[test]
    fn nothing_to_price_is_skipped_rather_than_ok() {
        assert_eq!(check_pricing(0, &BTreeMap::new(), Duration::days(7)).level, Level::Skipped);
        assert_eq!(check_pricing(9, &BTreeMap::new(), Duration::days(7)).level, Level::Ok);
    }

    // ------------------------------------------------------------- helpers --

    #[test]
    fn durations_read_in_the_largest_useful_unit() {
        assert_eq!(human(Duration::seconds(45)), "45 seconds");
        assert_eq!(human(Duration::minutes(30)), "30 minutes");
        assert_eq!(human(Duration::hours(5)), "5 hours");
        assert_eq!(human(Duration::days(400)), "400 days");
        // A negative skew reads the same; the direction is a separate word.
        assert_eq!(human(Duration::hours(-2)), "2 hours");
    }

    #[test]
    fn a_message_wraps_at_word_boundaries_and_never_loses_a_word() {
        let text = "the quick brown fox jumps over the lazy dog and keeps on going for a while";
        let lines = wrap(text, 20);
        assert!(lines.iter().all(|l| l.chars().count() <= 20), "{lines:?}");
        assert_eq!(lines.join(" "), text);
    }

    #[test]
    fn a_window_at_the_top_of_the_range_names_the_limit_rather_than_panicking() {
        // `now - Duration::days(u32::MAX)` leaves the range OffsetDateTime can
        // hold, and the panic came before a line of the report was printed.
        let args = |days| Args { offline: false, read_only: true, window_days: days };
        let e = window(&args(u32::MAX)).unwrap_err().to_string();
        assert!(e.contains("--window-days must be between 1 and 36525"), "{e}");
        assert!(e.contains("got 4294967295"), "{e}");
        assert!(window(&args(0)).is_err(), "zero is a window nothing can be read from");
        assert_eq!(window(&args(7)).unwrap(), Duration::days(7));

        // The widest window accepted, proved to be a subtraction rather than a
        // panic.
        let widest = window(&args(MAX_WINDOW_DAYS)).unwrap();
        let _ = OffsetDateTime::now_utc() - widest;
    }

    #[test]
    fn a_url_loses_its_credentials_and_keeps_everything_else() {
        assert_eq!(without_credentials("https://u:p@h.invalid/v1"), "https://h.invalid/v1");
        assert_eq!(
            without_credentials("postgres://u:p@db.invalid:5432/mcpd"),
            "postgres://db.invalid:5432/mcpd"
        );
        assert_eq!(without_credentials("https://u:p@h.invalid"), "https://h.invalid");
        assert_eq!(without_credentials("https://h.invalid/v1"), "https://h.invalid/v1");
        // An @ in a path is not a credential, and the rule must not eat it.
        assert_eq!(without_credentials("https://h.invalid/v1/@team"), "https://h.invalid/v1/@team");
        assert_eq!(without_credentials("./mcpd.db"), "./mcpd.db");
        assert_eq!(without_credentials(":memory:"), ":memory:");
    }

    #[test]
    fn an_at_sign_in_a_query_string_does_not_rewrite_the_host() {
        // A URL with a query and no path. The authority used to be read as
        // everything up to the first `/`, so the `@` in the query looked like
        // the end of a credential and the host was replaced by the tail of a
        // query value. The reader was then comparing a URL that does not exist
        // against their configuration.
        assert_eq!(
            without_credentials("https://otlp.example.invalid?to=alice@example.invalid"),
            "https://otlp.example.invalid?to=alice@example.invalid"
        );
        // The same URL when it does carry a credential loses everything up to
        // the last `@`. A password can hold a `?` and an `@`, so the query
        // here could be part of one, and printing it could print a password.
        let shown =
            without_credentials("https://u:p@otlp.example.invalid?to=alice@example.invalid");
        assert_eq!(shown, "https://example.invalid");
        assert!(!shown.contains("u:p"), "{shown}");
        // A fragment is past the query and is left alone.
        assert_eq!(without_credentials("https://h.invalid?a=1#top"), "https://h.invalid?a=1#top");
    }

    #[test]
    fn a_password_holding_a_url_delimiter_is_never_printed() {
        // Each of these used to end what was read as the authority, so the
        // password, or the part of it after the delimiter, printed as a path.
        for password in [
            "s3cr/etpw",
            "s3cr?etpw",
            "s3cr#etpw",
            "s3cr@etpw",
            "/s3cretpw",
            "1234/5678",
            "a/b?c#d@e",
            "s3cr@et/pw?x=1",
        ] {
            let url = format!("postgres://mcpd:{password}@db.internal:5432/mcpd");
            let shown = without_credentials(&url);
            assert_eq!(shown, "postgres://db.internal:5432/mcpd", "{url}");
            let with_query = format!("{url}?sslmode=require");
            assert_eq!(
                without_credentials(&with_query),
                "postgres://db.internal:5432/mcpd?sslmode=require",
                "{with_query}"
            );
        }
        // A user name alone goes the same way.
        assert_eq!(without_credentials("https://bot@h.invalid/v1"), "https://h.invalid/v1");
        // An IPv6 literal is not a user name, and an `@` in its path is a path.
        assert_eq!(without_credentials("http://[::1]/v1/@team"), "http://[::1]/v1/@team");
        assert_eq!(
            without_credentials("postgres://u:p/w@[::1]:5432/mcpd"),
            "postgres://[::1]:5432/mcpd"
        );
        // The key=value form is still read as one.
        let kv = without_credentials("host=db.invalid user=mcpd password=s3cr/et@pw dbname=mcpd");
        assert!(!kv.contains("s3cr"), "{kv}");
        assert!(kv.contains("host=db.invalid"), "{kv}");
    }

    #[test]
    fn a_credential_in_a_query_parameter_is_taken_out_like_any_other() {
        assert_eq!(
            without_credentials("https://api.example.invalid/v1?api_key=s3kr1t"),
            "https://api.example.invalid/v1?api_key=[redacted]"
        );
        // Several parameters, only the credential-shaped ones. The version is
        // the part a reader needs, and it is not a secret.
        assert_eq!(
            without_credentials(
                "https://api.example.invalid/chat?api-version=2024-02-01&subscription-key=s3kr1t"
            ),
            "https://api.example.invalid/chat?api-version=2024-02-01&subscription-key=[redacted]"
        );
        // With no path at all, which is the shape that used to eat the host.
        assert_eq!(
            without_credentials("https://h.invalid?access_token=s3kr1t&region=eu"),
            "https://h.invalid?access_token=[redacted]&region=eu"
        );
        // And on a store connection string, where `sslmode` must survive.
        assert_eq!(
            without_credentials("postgres://u:p@db.invalid:5432/mcpd?sslmode=require"),
            "postgres://db.invalid:5432/mcpd?sslmode=require"
        );
    }

    #[test]
    fn a_flag_is_read_back_in_both_the_forms_clap_accepts() {
        let argv: Vec<String> =
            ["mcpd", "doctor", "--config-file", "/etc/mcpd/c.json", "--data=/data/mcpd.db"]
                .iter()
                .map(|s| (*s).to_owned())
                .collect();
        let unset = "MCPD_DOCTOR_UNSET_IN_TESTS";
        assert_eq!(cli_value(&argv, "--config-file", unset), Some("/etc/mcpd/c.json".to_owned()));
        assert_eq!(cli_value(&argv, "--data", unset), Some("/data/mcpd.db".to_owned()));
        assert_eq!(cli_value(&argv, "--postgres-url", unset), None);
    }

    #[test]
    fn the_probe_project_is_never_graded() {
        // Even when the delete failed and it is still there, because it is this
        // command's own litter and not a project anybody created.
        let names = vec!["default".to_owned(), PROBE_PROJECT.to_owned(), "checkout".to_owned()];
        assert_eq!(gradable(names), set(&["default", "checkout"]));
    }

    #[test]
    fn a_credential_written_into_an_endpoint_is_never_printed() {
        let mut c = config();
        c.forward.destinations = vec![Destination::new(
            Vendor::Otlp,
            Some("https://otel:s3kr1t@otlp.example.invalid".to_owned()),
        )];
        c.judge = Some(JudgeConfig {
            endpoint: "https://key:s3kr1t@api.example.invalid/v1".to_owned(),
            model: "a-model".to_owned(),
            api_key: String::new(),
            criteria: BTreeMap::new(),
            timeout_seconds: 30,
        });

        let mut all = String::new();
        for f in check_forwarding(&c, &[Probe::Answered(401)]).iter().chain(&check_judge(&c)) {
            all.push_str(&f.message);
        }
        assert!(!all.contains("s3kr1t"), "the credential was printed: {all}");
        assert!(all.contains("https://otlp.example.invalid/v1/traces"), "{all}");
        assert!(all.contains("https://api.example.invalid/v1"), "{all}");
    }

    #[test]
    fn an_unreadable_project_list_skips_the_checks_that_would_compare_against_it() {
        // One failed call used to become five confident statements: every
        // configured project a typo, every monitor watching an empty project.
        let mut c = config();
        c.retention_days = Some(30);
        let mut m = monitor("errors");
        m.project = "checkout".to_owned();
        c.monitors = vec![m];

        let r = check_retention(&c, None, &BTreeMap::new(), OffsetDateTime::now_utc());
        assert!(says(&r, Level::Skipped, &["project list could not be read"]), "{r:?}");
        assert!(levels(&r, Level::Warn).is_empty(), "{r:?}");

        let mo = check_monitors(&c, None, &BTreeMap::new(), &BTreeSet::new(), Duration::days(7));
        assert!(says(&mo, Level::Skipped, &["project list could not be read"]), "{mo:?}");
        assert!(!says(&mo, Level::Warn, &["holds no runs"]), "{mo:?}");
    }

    #[test]
    fn plurals_read_correctly_including_the_one_irregular_noun() {
        assert_eq!(plural(1, "project"), "1 project");
        assert_eq!(plural(3, "project"), "3 projects");
        assert_eq!(plural(1, "criterion"), "1 criterion");
        assert_eq!(plural(4, "criterion"), "4 criteria");
    }

    // ------------------------------------------------------- against a store --

    #[tokio::test]
    async fn the_write_probe_stores_reads_back_and_cleans_up() {
        let t = temp_store("write-probe").await;
        let before = t.store.list_projects().await.unwrap();

        let f =
            write_probe(&t.store, Duration::days(7), OffsetDateTime::now_utc(), PROBE_BUDGET).await;
        assert_eq!(f.level, Level::Ok, "{}", f.message);
        assert!(f.message.contains("read back and deleted"), "{}", f.message);
        assert!(f.message.contains("1 attempt"), "{}", f.message);

        // The probe must leave the store exactly as it found it, or the next
        // run of `check_projects` would report a project nobody created.
        let after = t.store.list_projects().await.unwrap();
        assert_eq!(before, after);
        assert!(!after.iter().any(|p| p == PROBE_PROJECT), "{after:?}");
    }

    /// A probe that spent its whole budget and never landed the write.
    fn gave_up(progress: Progress) -> Spent {
        Spent { attempts: 6, waited: std::time::Duration::from_secs(10), progress }
    }

    #[test]
    fn a_lock_nothing_gets_through_is_a_fault_however_it_names_itself() {
        // The regression this whole check turns on. `Level::Skipped` never
        // changes the exit code, so a store that answered a ping and refused
        // every write with this exact message passed a deploy gate at the same
        // moment ingest was rejecting every batch.
        let busy = mcpd_store::Error::Backend("database is locked".to_owned());
        let f = write_failure(&busy, gave_up(Progress::Stalled));
        assert_eq!(f.level, Level::Fault, "{}", f.message);
        assert!(f.message.contains("10.0 seconds"), "{}", f.message);
        assert!(f.message.contains("6 attempts"), "{}", f.message);
        // Every other finding in this file names the next thing to do, and a
        // line that fails a deploy has to earn it more than the rest.
        assert!(f.message.contains("holding the write lock and end it"), "{}", f.message);
    }

    #[test]
    fn a_lock_another_connection_is_getting_through_is_not_a_fault() {
        // The other half, and the reason the message on its own cannot decide
        // this. An embedded store under real ingest holds its write lock almost
        // continuously, and failing a deploy because the daemon is busy made
        // this command worst exactly when the deployment was healthiest.
        let busy = mcpd_store::Error::Backend("database is locked".to_owned());
        let f = write_failure(&busy, gave_up(Progress::Landed(412)));
        assert_eq!(f.level, Level::Warn, "{}", f.message);
        assert!(f.message.contains("412 spans from other connections"), "{}", f.message);
        assert!(f.message.contains("--read-only"), "{}", f.message);
    }

    #[test]
    fn a_lock_with_nothing_to_corroborate_it_is_graded_on_the_write_alone() {
        // A store whose project list or counts could not be read. The write is
        // the only evidence there is, and the write did not land.
        let busy = mcpd_store::Error::Backend("deadlock detected".to_owned());
        let f = write_failure(&busy, gave_up(Progress::Unknown));
        assert_eq!(f.level, Level::Fault, "{}", f.message);
        assert!(f.message.contains("could not be read"), "{}", f.message);
    }

    #[test]
    fn every_write_failure_that_is_not_contention_is_still_a_fault() {
        // The three the probe exists to catch. None of them may be softened by
        // the reading that keeps a busy store out of the exit code, so each is
        // graded here with the most forgiving corroboration there is.
        for message in [
            "attempt to write a readonly database",
            "database or disk is full",
            "permission denied for table spans",
        ] {
            let e = mcpd_store::Error::Backend(message.to_owned());
            assert!(!is_contention(&e), "{message} must not read as contention");
            let f = write_failure(&e, gave_up(Progress::Landed(9_000)));
            assert_eq!(f.level, Level::Fault, "{message}: {}", f.message);
            assert!(f.message.contains(message), "{}", f.message);
        }
    }

    #[test]
    fn a_corrupt_lock_is_not_contention_because_it_carries_the_word_lock() {
        // SQLITE_PROTOCOL. It says "locking protocol" and it is real damage, so
        // matching on the word rather than the phrase would hide it.
        let e = mcpd_store::Error::Backend("locking protocol".to_owned());
        assert!(!is_contention(&e));
        assert_eq!(write_failure(&e, gave_up(Progress::Landed(9_000))).level, Level::Fault);
    }

    #[tokio::test]
    async fn a_store_that_is_busy_and_then_free_is_written_to_rather_than_reported() {
        // Two refusals, then a real store takes the write. Nothing here reaches
        // `write_failure`, which is the only way this check becomes a fault.
        let t = temp_store("busy-then-free").await;
        let store = &t.store;
        let now = OffsetDateTime::now_utc();
        let span = probe_span(now);
        let trace_id = span.trace_id.clone();

        let attempts = std::cell::Cell::new(0u32);
        let out = keep_trying(
            || {
                attempts.set(attempts.get() + 1);
                let busy = attempts.get() < 3;
                let batch =
                    mcpd_store::Batch { spans: vec![span.clone()], ..mcpd_store::Batch::default() };
                async move {
                    if busy {
                        Err(mcpd_store::Error::Backend("database is locked".to_owned()))
                    } else {
                        store.ingest(batch).await
                    }
                }
            },
            std::time::Duration::from_secs(5),
        )
        .await;

        assert_eq!(out.attempts, 3, "the first two answers were contention");
        assert_eq!(out.result.expect("the third attempt landed").accepted, 1);
        assert!(
            store
                .get_trace(PROBE_PROJECT, &trace_id, &mcpd_store::Include::default())
                .await
                .is_ok(),
            "the row that was retried is not in the store"
        );
    }

    #[tokio::test]
    async fn a_write_refused_for_the_whole_budget_stops_when_the_budget_does() {
        // The budget bounds the waiting and not the attempting: one more write
        // is tried at the far edge of it, so the report is based on an answer
        // the store gave a moment ago rather than one from ten seconds back.
        let attempts = std::cell::Cell::new(0u32);
        let started = std::time::Instant::now();
        let out = keep_trying(
            || {
                attempts.set(attempts.get() + 1);
                async { Err(mcpd_store::Error::Backend("database is locked".to_owned())) }
            },
            std::time::Duration::from_millis(400),
        )
        .await;
        let took = started.elapsed();

        assert!(out.attempts > 1, "a lock-shaped refusal is retried, got {}", out.attempts);
        assert!(out.result.is_err(), "it never landed");
        assert!(took >= std::time::Duration::from_millis(400), "it gave up early: {took:?}");
        assert!(took < std::time::Duration::from_secs(3), "it ran past its budget: {took:?}");
    }

    #[tokio::test]
    async fn a_write_refused_for_a_reason_that_is_not_contention_is_not_retried() {
        // Waiting changes nothing about a read-only mount, and a diagnostic
        // command that sits still for ten seconds to prove it is one nobody
        // runs.
        let attempts = std::cell::Cell::new(0u32);
        let out = keep_trying(
            || {
                attempts.set(attempts.get() + 1);
                async {
                    Err(mcpd_store::Error::Backend(
                        "attempt to write a readonly database".to_owned(),
                    ))
                }
            },
            std::time::Duration::from_secs(5),
        )
        .await;
        assert_eq!(out.attempts, 1, "it waited for a store that will not change");
    }

    #[tokio::test]
    async fn the_count_the_probe_corroborates_with_moves_when_a_span_lands() {
        // Without this the corroboration could return the same number for ever,
        // every contended probe would read as a stall, and the false fault this
        // command was fixed for would be back.
        let t = temp_store("counted-spans").await;
        let now = OffsetDateTime::now_utc();
        let window = Duration::days(7);

        let before = counted_spans(&t.store, window, now).await.expect("a count");
        t.store
            .ingest(mcpd_store::Batch {
                spans: vec![span_at("default", &"f".repeat(32), now, now)],
                ..mcpd_store::Batch::default()
            })
            .await
            .unwrap();
        let after = counted_spans(&t.store, window, now).await.expect("a count");
        assert!(after > before, "a span landed and the count did not move: {before} to {after}");

        // The probe's own project is left out. Those are this command's writes,
        // and on the path where this is read they are the ones failing.
        t.store
            .ingest(mcpd_store::Batch {
                spans: vec![probe_span(now)],
                ..mcpd_store::Batch::default()
            })
            .await
            .unwrap();
        let with_probe = counted_spans(&t.store, window, now).await.expect("a count");
        assert_eq!(with_probe, after, "the probe's own project was counted as other traffic");
    }

    #[tokio::test]
    async fn a_read_only_run_reports_the_write_as_skipped_and_writes_nothing() {
        let t = temp_store("read-only").await;
        let args = Args { offline: true, read_only: true, window_days: 7 };
        let f = check_storage(&t.store, &args, Duration::days(7), OffsetDateTime::now_utc()).await;
        assert!(says(&f, Level::Ok, &["answered a ping"]), "{f:?}");
        assert!(says(&f, Level::Skipped, &["--read-only"]), "{f:?}");
        assert!(levels(&f, Level::Ok).iter().all(|m| !m.contains("probe")), "{f:?}");
        assert!(t.store.list_projects().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn activity_reports_the_oldest_the_newest_and_the_worst_skew() {
        let t = temp_store("activity").await;
        let now = OffsetDateTime::now_utc();

        // Three runs: an old one, a recent one, and one whose sender stamped a
        // start time two hours before it arrived.
        let spans = vec![
            span_at(
                "default",
                &"a".repeat(32),
                now - Duration::days(200),
                now - Duration::days(200),
            ),
            span_at("default", &"b".repeat(32), now - Duration::hours(1), now - Duration::hours(1)),
            span_at("default", &"c".repeat(32), now - Duration::hours(3), now - Duration::hours(1)),
        ];
        t.store.ingest(mcpd_store::Batch { spans, ..mcpd_store::Batch::default() }).await.unwrap();

        let projects = t.store.list_projects().await.unwrap().into_iter().collect();
        let activity = gather_activity(&t.store, &projects, Duration::days(7), now).await;
        let a = activity.get("default").expect("the default project");

        assert_eq!(a.recent_traces, 2, "the 200 day old run is outside a 7 day window");
        let oldest = a.oldest_start.expect("an oldest run");
        assert!((now - oldest).whole_days() >= 199, "{oldest}");
        let (trace, skew) = a.worst_skew.clone().expect("a skew");
        assert_eq!(trace, "c".repeat(32));
        assert_eq!(skew.whole_hours(), -2, "started two hours before it was received");

        // And the finding that follows from it.
        let f = check_clock(&activity, now);
        assert!(says(&f, Level::Warn, &["2 hours before"]), "{f:?}");
    }

    #[tokio::test]
    async fn a_sender_whose_clock_runs_behind_is_under_the_page_of_newest_runs() {
        // The check reads runs ordered by start_time, which is the field a
        // skewed sender got wrong. A clock that runs behind puts that sender's
        // runs under every honest run in the store, so a page of the newest
        // runs is exactly where the skew cannot be seen.
        let t = temp_store("clock-skew").await;
        let now = OffsetDateTime::now_utc();

        let skewed = "0".repeat(32);
        let mut spans = vec![span_at("default", &skewed, now - Duration::days(30), now)];
        for i in 1..=(CLOCK_SAMPLE + 5) {
            let at = now - Duration::minutes(i64::from(i));
            let mut honest = span_at("default", &format!("{i:032}"), at, at);
            honest.span_id = format!("{i:016}");
            spans.push(honest);
        }
        t.store.ingest(mcpd_store::Batch { spans, ..mcpd_store::Batch::default() }).await.unwrap();

        // The page the check used to read, and what it does not contain.
        let newest = TraceQuery {
            project: "default".to_owned(),
            sort: TraceSort::StartTime,
            order: Order::Desc,
            limit: CLOCK_SAMPLE,
            ..TraceQuery::default()
        };
        let page = t.store.list_traces(&newest).await.unwrap();
        assert_eq!(page.items.len(), CLOCK_SAMPLE as usize);
        assert!(!page.items.iter().any(|t| t.trace_id == skewed), "the skewed run was on it");

        let projects = t.store.list_projects().await.unwrap().into_iter().collect();
        let activity = gather_activity(&t.store, &projects, Duration::days(7), now).await;
        let a = activity.get("default").expect("the default project");

        let (trace, skew) = a.worst_skew.clone().expect("a skew");
        assert_eq!(trace, skewed, "the worst skew is the run that was not on the page");
        assert_eq!(skew.whole_days(), -30);
        assert_eq!(a.clock_samples, CLOCK_SAMPLE as usize + 6, "both ends, counted once each");

        let f = check_clock(&activity, now);
        assert!(says(&f, Level::Warn, &[&skewed, "30 days before"]), "{f:?}");
    }

    #[tokio::test]
    async fn a_probe_left_by_an_earlier_run_is_cleared_before_this_one_writes() {
        let t = temp_store("probe-litter").await;
        let now = OffsetDateTime::now_utc();

        // A probe from a run whose delete failed. Left alone it is graded as a
        // project somebody created, and it is visible to anybody reading the
        // project list in the meantime.
        let stale = probe_span(now - Duration::days(1));
        t.store
            .ingest(mcpd_store::Batch { spans: vec![stale], ..mcpd_store::Batch::default() })
            .await
            .unwrap();
        assert!(t.store.list_projects().await.unwrap().iter().any(|p| p == PROBE_PROJECT));

        let f = write_probe(&t.store, Duration::days(7), now, PROBE_BUDGET).await;
        assert_eq!(f.level, Level::Ok, "{}", f.message);
        assert!(t.store.list_projects().await.unwrap().is_empty(), "litter survived the probe");
    }

    #[tokio::test]
    async fn unpriced_models_are_found_and_priced_ones_are_not() {
        let t = temp_store("pricing").await;
        let now = OffsetDateTime::now_utc();
        let book = mcpd_core::cost::builtin_book();

        let mut known = span_at("default", &"d".repeat(32), now, now);
        known.kind = mcpd_core::SpanKind::ModelStep;
        known.model = Some("claude-sonnet-4".to_owned());
        known.usage = Some(mcpd_core::TokenUsage {
            input_tokens: Some(100),
            output_tokens: Some(50),
            ..Default::default()
        });
        let mut unknown = known.clone();
        unknown.trace_id = "e".repeat(32);
        unknown.span_id = "e".repeat(16);
        unknown.model = Some("acme-turbo-9".to_owned());

        t.store
            .ingest(mcpd_store::Batch {
                spans: vec![known, unknown],
                ..mcpd_store::Batch::default()
            })
            .await
            .unwrap();

        let projects: BTreeSet<String> =
            t.store.list_projects().await.unwrap().into_iter().collect();
        let (considered, unpriced) = unpriced_models(
            &t.store,
            &projects,
            &book,
            now - Duration::days(1),
            now + Duration::days(1),
        )
        .await;

        assert_eq!(considered, 2);
        assert_eq!(unpriced.get("acme-turbo-9"), Some(&1));
        assert!(!unpriced.contains_key("claude-sonnet-4"), "{unpriced:?}");

        let f = check_pricing(considered, &unpriced, Duration::days(1));
        assert_eq!(f.level, Level::Warn);
        assert!(f.message.contains("acme-turbo-9"), "{}", f.message);
    }

    #[tokio::test]
    async fn a_scorer_with_no_scores_is_reported_and_one_with_scores_is_not() {
        let t = temp_store("scorers").await;
        let now = OffsetDateTime::now_utc();

        let score = mcpd_core::Score {
            score_id: "s1".to_owned(),
            trace_id: Some("f".repeat(32)),
            span_id: None,
            scorer_id: "answer_relevance".to_owned(),
            scorer_version: None,
            source: mcpd_core::ScoreSource::Live,
            value: 0.9,
            min: Some(0.0),
            max: Some(1.0),
            label: None,
            reason: None,
            score_trace_id: None,
            cost: None,
            metadata: None,
            timestamp: now,
            // The project matters: `empty_scorers` looks for the scorer inside
            // the project its monitor watches, not anywhere in the store.
            correlation: mcpd_core::Correlation {
                project: "default".to_owned(),
                ..probe_span(now).correlation
            },
        };
        t.store
            .ingest(mcpd_store::Batch { scores: vec![score], ..mcpd_store::Batch::default() })
            .await
            .unwrap();

        let mut c = config();
        let mut good = monitor("quality");
        good.watch = Watch::ScoreMean;
        good.scorer_id = Some("answer_relevance".to_owned());
        let mut typo = monitor("quality-typo");
        typo.watch = Watch::ScoreMean;
        typo.scorer_id = Some("answer_relevence".to_owned());
        c.monitors = vec![good, typo];

        let empty = empty_scorers(&t.store, &c).await;
        assert!(empty.contains("answer_relevence"), "{empty:?}");
        assert!(!empty.contains("answer_relevance"), "{empty:?}");
    }

    // ----------------------------------------------------------- preflight --

    /// The address the daemon binds by default, which is not a loopback one.
    /// Every openness finding turns on that distinction, so the default is what
    /// the preflight has to be safe on.
    const OFF_LOOPBACK: &str = "0.0.0.0:4317";

    fn judge_with(criteria: &[(&str, &str)]) -> JudgeConfig {
        JudgeConfig {
            endpoint: "https://api.example.invalid/v1".to_owned(),
            model: "a-model".to_owned(),
            api_key: "k".to_owned(),
            criteria: criteria.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect(),
            timeout_seconds: 30,
        }
    }

    #[test]
    fn a_judge_with_no_criteria_refuses_the_start() {
        let mut c = config();
        c.judge = Some(judge_with(&[]));
        let f = preflight(&c, OFF_LOOPBACK, true);
        assert!(says(&f, Level::Fault, &["no criteria", "judge.criteria"]), "{f:?}");
    }

    #[test]
    fn a_judge_that_can_answer_is_nothing_the_start_has_to_say() {
        let mut c = config();
        c.judge = Some(judge_with(&[("helpfulness", "is it helpful")]));
        // The doctor prints an `ok` line for this. A start is not a report, so
        // the preflight drops it rather than logging a subsystem that is well.
        assert!(
            preflight(&c, OFF_LOOPBACK, true).is_empty(),
            "{:?}",
            preflight(&c, OFF_LOOPBACK, true)
        );
    }

    #[test]
    fn the_first_run_defaults_have_nothing_for_the_preflight_to_report() {
        // `docker run mcpd` with no arguments: no keys, no config file, bound
        // to 0.0.0.0. Every scope is open, and the daemon warns about that
        // itself in the wording the quickstart prints. If this ever returns a
        // finding, the quickstart's terminal cast has become wrong.
        assert!(preflight(&config(), OFF_LOOPBACK, true).is_empty());
    }

    #[test]
    fn an_open_read_scope_off_loopback_never_stops_the_daemon_starting() {
        // `mcpd doctor` calls this a fault and exits 2, which is right for a
        // deploy gate and would be wrong here: it is the shipped default, and
        // refusing would mean a first run that never starts.
        let mut c = config();
        c.ingest_keys = keys(&["a-long-enough-ingest-key"]);
        let doctored = check_keys(Vec::new(), &c, OFF_LOOPBACK);
        assert!(says(&doctored, Level::Fault, &["the read scope is open"]), "{doctored:?}");
        assert!(
            !preflight(&c, OFF_LOOPBACK, true).iter().any(|f| f.level == Level::Fault),
            "{:?}",
            preflight(&c, OFF_LOOPBACK, true)
        );
    }

    #[test]
    fn a_key_that_can_never_match_refuses_the_start() {
        let mut c = config();
        c.read_keys = keys(&[" a-key-with-a-space-at-one-end"]);
        let f = preflight(&c, OFF_LOOPBACK, true);
        assert!(says(&f, Level::Fault, &["whitespace at one end", "--read-key"]), "{f:?}");
    }

    #[test]
    fn a_short_key_is_logged_and_the_daemon_still_starts() {
        let mut c = config();
        c.read_keys = keys(&["short"]);
        let f = preflight(&c, OFF_LOOPBACK, true);
        assert!(says(&f, Level::Warn, &["shorter than"]), "{f:?}");
        assert!(!f.iter().any(|x| x.level == Level::Fault), "{f:?}");
    }

    #[test]
    fn a_trusted_range_holding_the_whole_internet_refuses_the_start() {
        let mut c = config();
        c.trusted_identity = mcpd_server::identity::TrustedIdentity::new(
            Some("x-forwarded-user"),
            &["0.0.0.0/0".to_owned()],
        )
        .expect("a parseable range");
        let f = preflight(&c, OFF_LOOPBACK, true);
        assert!(says(&f, Level::Fault, &["contains the whole internet"]), "{f:?}");
        // On a laptop the same configuration is a warning, because the port is
        // not reachable. `check_identity` makes that call; the preflight only
        // carries it through.
        let laptop = preflight(&c, "127.0.0.1:4317", true);
        assert!(!laptop.iter().any(|x| x.level == Level::Fault), "{laptop:?}");
    }

    #[test]
    fn a_project_sample_outside_the_scale_refuses_the_start() {
        // `--sample 4.2` is already refused by `main`. A project that sets the
        // same number in the config file used to start and keep a fraction that
        // means nothing.
        let mut c = config();
        c.projects.insert(
            "checkout".to_owned(),
            ProjectConfig { sample: Some(4.2), ..ProjectConfig::default() },
        );
        let f = preflight(&c, OFF_LOOPBACK, true);
        assert!(says(&f, Level::Fault, &["outside 0.0 to 1.0", "checkout"]), "{f:?}");
    }

    #[test]
    fn the_preflight_reports_nothing_that_would_need_a_store_or_the_network() {
        // The groups it may produce, which is the list of checks that read the
        // configuration and nothing else. Storage, runs, projects, retention,
        // monitors, clock, pricing and forwarding all need a store or a socket,
        // and a start must wait on neither.
        const CONFIGURATION_ONLY: [&str; 4] = ["keys", "identity", "limits", "judge"];

        let mut c = config();
        c.judge = Some(judge_with(&[]));
        c.read_keys = keys(&["short", " spaced-key-that-is-long-enough"]);
        c.forward.destinations = vec![{
            let mut d =
                Destination::new(Vendor::Otlp, Some("https://otlp.example.invalid".to_owned()));
            d.enabled = false;
            d
        }];
        c.monitors = vec![monitor("quality")];
        c.slos = vec![slo("availability")];
        c.retention_days = Some(30);

        let f = preflight(&c, OFF_LOOPBACK, true);
        assert!(!f.is_empty(), "this configuration has plenty wrong with it");
        for finding in &f {
            assert!(
                CONFIGURATION_ONLY.contains(&finding.group),
                "the preflight reported a {} finding, which needs something a start must \
                 not wait on: {finding:?}",
                finding.group
            );
        }
    }

    #[test]
    fn a_key_value_connection_string_keeps_its_host_and_loses_its_password() {
        // `main` printed the part of a connection string after the `@`, and
        // this form has none, so the whole string went out, password and all.
        let shown = without_credentials(
            "host=db.invalid port=5432 user=mcpd password=hunter2-secret dbname=mcpd",
        );
        assert_eq!(shown, "host=db.invalid port=5432 user=mcpd password=[redacted] dbname=mcpd");
        let quoted = without_credentials(
            "host=db.invalid password = 'hunter2 \\' secret' sslpassword=x sslmode=require",
        );
        assert_eq!(
            quoted,
            "host=db.invalid password=[redacted] sslpassword=[redacted] sslmode=require"
        );
        // An unquoted password with a space is a pair and a bare word, and the
        // bare word is the rest of the password.
        let spaced = without_credentials("host=db.invalid password=hunter2 secret dbname=mcpd");
        assert!(!spaced.contains("hunter2") && !spaced.contains("secret"), "{spaced}");
        assert!(spaced.contains("dbname=mcpd"), "{spaced}");
    }
}
