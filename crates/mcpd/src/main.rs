//! `mcpd` — the daemon that holds an agent's runs between steps and records each one.
//!
//! One binary. Run it, point an agent at it, read the trace.

#![forbid(unsafe_code)]

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use mcpd_core::sampling::Policy;
use mcpd_core::SamplingKind;
use mcpd_runs::RunStore;
use mcpd_server::{AppState, ServerConfig};
use mcpd_store::{SqliteStore, Store};
use std::net::SocketAddr;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

/// Durable runs and observability for agents, in a single binary.
#[derive(Parser, Debug)]
#[command(name = "mcpd", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Address to listen on.
    #[arg(long, env = "MCPD_LISTEN", default_value = "0.0.0.0:4317", global = true)]
    listen: String,

    /// Where to keep data. Use `:memory:` for a store that leaves nothing behind.
    #[arg(long, env = "MCPD_DATA", default_value = "./mcpd.db", global = true)]
    data: String,

    /// A ClickHouse HTTP endpoint to store into instead of the embedded file.
    ///
    /// The embedded store is one writer on one file, which is right on a laptop
    /// and a ceiling in a cluster. Setting this switches backend; nothing else
    /// about the daemon changes.
    #[arg(long, env = "MCPD_CLICKHOUSE_URL", global = true)]
    clickhouse_url: Option<String>,

    /// ClickHouse database name.
    #[arg(long, env = "MCPD_CLICKHOUSE_DATABASE", default_value = "mcpd", global = true)]
    clickhouse_database: String,

    /// ClickHouse user.
    #[arg(long, env = "MCPD_CLICKHOUSE_USER", global = true)]
    clickhouse_user: Option<String>,

    /// ClickHouse password.
    #[arg(long, env = "MCPD_CLICKHOUSE_PASSWORD", global = true)]
    clickhouse_password: Option<String>,

    /// Project a signal lands in when the sender did not name one.
    #[arg(long, env = "MCPD_PROJECT", default_value = "default", global = true)]
    project: String,

    /// Delete signals older than this many days. Omit to keep everything.
    #[arg(long, env = "MCPD_RETENTION_DAYS", global = true)]
    retention_days: Option<u32>,

    /// Keys permitted to write. Repeat for several. With none, ingest is open.
    #[arg(long = "ingest-key", env = "MCPD_INGEST_KEYS", value_delimiter = ',', global = true)]
    ingest_keys: Vec<String>,

    /// Keys permitted to read. With none, the API is open.
    #[arg(long = "read-key", env = "MCPD_READ_KEYS", value_delimiter = ',', global = true)]
    read_keys: Vec<String>,

    /// Keys permitted to change stored data: scores, feedback, datasets,
    /// experiments and acknowledgements.
    ///
    /// With none, the ingest keys are accepted here. That fallback is what makes
    /// the write scope safe to add to a release: a deployment that never sets
    /// this keeps exactly the reach it had before the scope existed.
    #[arg(long = "write-key", env = "MCPD_WRITE_KEYS", value_delimiter = ',', global = true)]
    write_keys: Vec<String>,

    /// Keys permitted to delete data and to read the audit log.
    ///
    /// With none, the read keys are accepted here, because deletion used to be
    /// authorised by the read scope. Set this to make a read key strictly
    /// read-only, which is what the reference documentation has always said the
    /// two scopes mean.
    #[arg(long = "admin-key", env = "MCPD_ADMIN_KEYS", value_delimiter = ',', global = true)]
    admin_keys: Vec<String>,

    /// Keys permitted to start, cancel and signal runs, and to publish events
    /// to them. With none, the write keys are accepted here, and an admin key
    /// always is.
    #[arg(long = "execute-key", env = "MCPD_EXECUTE_KEYS", value_delimiter = ',', global = true)]
    execute_keys: Vec<String>,

    /// Keys permitted to claim steps and report their outcome. This is the key
    /// that lives in the process running step bodies. With none, the write keys
    /// are accepted here, and an admin key always is.
    #[arg(long = "worker-key", env = "MCPD_WORKER_KEYS", value_delimiter = ',', global = true)]
    worker_keys: Vec<String>,

    /// Where to keep runs: the definitions, the journal of step attempts, the
    /// deadlines and the waits. An embedded file beside the signal store,
    /// whatever the signal store is, so the run surface is one daemon replica.
    /// Defaults to the `--data` path with `.runs` before its extension, or
    /// `:memory:` when `--data` is. Set it to `off` to serve no runs at all.
    #[arg(long, env = "MCPD_RUNS_DATA", global = true)]
    runs_data: Option<String>,

    /// Fraction of traces to keep, 0.0 to 1.0. Decided by hashing the trace id,
    /// so every instance in a fleet makes the same choice.
    #[arg(long, env = "MCPD_SAMPLE", global = true)]
    sample: Option<f64>,

    /// Most signals one project may store per minute. Omit for no ceiling.
    ///
    /// Applies to every project named in the config file that does not set its
    /// own `quota_per_minute`, and to `--project`. Signals over the ceiling are
    /// refused and reported in the response, never dropped quietly, so a sender
    /// can see it is being limited and slow down.
    #[arg(long, env = "MCPD_QUOTA_PER_MINUTE", global = true)]
    quota_per_minute: Option<u32>,

    /// Turn off the built-in redaction of secrets and personal data.
    /// Only sensible when something upstream already redacts.
    #[arg(long, env = "MCPD_NO_REDACT", global = true)]
    no_redact: bool,

    /// Key used when hashing identifiers during redaction. Set this in any
    /// deployment that hashes, or the digests are guessable.
    #[arg(long, env = "MCPD_REDACT_KEY", global = true)]
    redact_key: Option<String>,

    /// Also mask any value whose key contains this, on top of the built-in
    /// rules. Matched on lowercase substrings of the key, so `ticket` covers
    /// `ticket_id` and `supportTicket`. Repeat for several.
    ///
    /// The built-in rules know the names credentials are usually stored under.
    /// They do not know the field your own tools put a customer's account
    /// number in, and until this flag the only way to add one was to build
    /// the daemon from source.
    #[arg(
        long = "redact-key-contains",
        env = "MCPD_REDACT_KEY_CONTAINS",
        value_delimiter = ',',
        global = true
    )]
    redact_key_contains: Vec<String>,

    /// Longest single string stored, in bytes. Longer ones are cut at that
    /// length with a marker naming how much was dropped. The built-in cap is
    /// 128 KiB; set this lower to bound what a runaway prompt costs on disk.
    #[arg(long, env = "MCPD_REDACT_MAX_STRING", global = true)]
    redact_max_string: Option<usize>,

    /// A JSON price book with your own rates, such as negotiated prices or a
    /// model the built-in book does not name. Its rates layer over the built-in
    /// book: an entry here wins, and every model it leaves out keeps the
    /// built-in rate. A book in another currency replaces the built-in one.
    /// Its `id` is recorded on every cost it prices.
    #[arg(long = "price-book", env = "MCPD_PRICE_BOOK", global = true)]
    price_book: Option<String>,

    /// A browser origin allowed to call the query API from another site, such
    /// as `https://console.example`. Repeat for several. With none, no
    /// cross-origin browser request is answered with the headers it needs, so
    /// only the bundled interface, which is served from this daemon's own
    /// origin, can read the API from a browser. `*` allows every origin, which
    /// is what earlier builds did without asking.
    ///
    /// The health, readiness and metrics routes stay readable from any origin
    /// whatever this says; they carry no stored data.
    #[arg(long = "cors-origin", env = "MCPD_CORS_ORIGINS", value_delimiter = ',', global = true)]
    cors_origins: Vec<String>,

    /// Longest a request may take, reading the body included, before it is
    /// answered with a 503 and the flag to raise. Bounds a handler stuck on a
    /// storage backend that accepts connections and never answers.
    #[arg(long, env = "MCPD_REQUEST_TIMEOUT_SECONDS", default_value_t = 30, global = true)]
    request_timeout_seconds: u64,

    /// Largest request body accepted, in bytes. Default 33554432, which is
    /// 32 MiB. A larger body is refused with 413 naming both numbers.
    ///
    /// Raise it for a fleet that batches hard, and lower it on a small
    /// container: this number, not the batch count, is what bounds how much
    /// memory one request can take before anything has been decoded.
    #[arg(long, env = "MCPD_MAX_BODY_BYTES", global = true)]
    max_body_bytes: Option<usize>,

    /// Most signals accepted in one request, counted after decoding.
    /// Default 10000.
    ///
    /// Counted across the whole request on every ingest door, so a batch, an
    /// array of batches and an NDJSON stream are all held to the same number.
    #[arg(long, env = "MCPD_MAX_BATCH_SIGNALS", global = true)]
    max_batch_signals: Option<usize>,

    /// Log format: `text` for a terminal, `json` for a log pipeline.
    #[arg(long, env = "MCPD_LOG_FORMAT", default_value = "text", global = true)]
    log_format: String,

    /// A Postgres connection string to store into instead of the embedded file.
    ///
    /// The middle option: it removes the single-writer and single-replica
    /// limits without asking a team to operate a database they do not run.
    #[arg(long, env = "MCPD_POSTGRES_URL", global = true)]
    postgres_url: Option<String>,

    /// A JSON file holding the settings that are too structured for a flag:
    /// `monitors`, `slos`, `monitor_interval_seconds`, `forward` (the endpoint
    /// and its headers), `judge`, per-project keys, retention, sampling and
    /// ceilings under `projects`, the run store's timings under `runs`,
    /// `max_body_bytes`, `max_batch_signals`, `derived_metrics` and `aspects`.
    ///
    /// JSON rather than YAML or TOML because mcpd already parses JSON for
    /// everything else, so the file needs no new dependency and no second
    /// syntax to learn. It mounts as a Kubernetes ConfigMap unchanged.
    #[arg(long = "config-file", env = "MCPD_CONFIG_FILE", global = true)]
    config_file: Option<String>,

    /// Copy every stored span on to another OTLP endpoint.
    ///
    /// mcpd becomes one hop in a pipeline rather than the destination. The
    /// local write always happens first, so a downstream outage never costs
    /// you the data.
    #[arg(long, env = "MCPD_FORWARD_ENDPOINT", global = true)]
    forward_endpoint: Option<String>,

    /// A header to send with every forwarded batch, as `Name: value`. Repeat
    /// for more than one.
    #[arg(
        long = "forward-header",
        env = "MCPD_FORWARD_HEADERS",
        value_delimiter = ',',
        global = true
    )]
    forward_headers: Vec<String>,

    /// A header your ingress sets to name the person behind a request. Its
    /// value is recorded as the audit log's subject. Unset means no subject is
    /// ever recorded, which is the default.
    ///
    /// Common names are `x-forwarded-user`, `x-forwarded-email` and
    /// `x-auth-request-user`. There is no default on purpose: a daemon that
    /// believed a header nobody opted into would accept whatever reached it.
    #[arg(long, env = "MCPD_TRUSTED_IDENTITY_HEADER", global = true)]
    trusted_identity_header: Option<String>,

    /// An address range permitted to set that header, in CIDR form. Repeat for
    /// several.
    ///
    /// Required alongside the header, and the reason the feature is safe: the
    /// header is a string anyone who can open a socket to this port can send,
    /// so it is read only from an address you have named.
    #[arg(
        long = "trusted-proxy-cidr",
        env = "MCPD_TRUSTED_PROXY_CIDRS",
        value_delimiter = ',',
        global = true
    )]
    trusted_proxy_cidrs: Vec<String>,

    /// A header your proxy sets to the client's address, such as
    /// `cf-connecting-ip`. Read only from an address in --trusted-proxy-cidr;
    /// from a trusted proxy without it, the left-most `X-Forwarded-For` entry is
    /// used. The address is what the sign-up and log-in limits count. Unset
    /// means the peer address, which is right with no proxy in front.
    #[arg(long, env = "MCPD_CLIENT_IP_HEADER", global = true)]
    client_ip_header: Option<String>,

    /// Let anyone create an account. Each account owns one new project and
    /// manages its own API keys, and nothing it holds reaches another project.
    ///
    /// Off by default. On, a request with no key is refused everywhere except
    /// the health probes, sign-up and log-in, even where no key is configured.
    #[arg(long, env = "MCPD_SIGNUP", global = true)]
    signup: bool,

    /// Where to keep accounts when --signup is on. Defaults to the --data path
    /// with `.accounts` before its extension, or `:memory:` when --data is.
    #[arg(long, env = "MCPD_ACCOUNTS_DATA", global = true)]
    accounts_data: Option<String>,

    /// Accounts without public sign-up, and the mail they send.
    #[command(flatten)]
    account_flags: AccountFlags,
}

/// Accounts without public sign-up, the mail they send, and the defaults an
/// organisation gets. Every one is read by `crate::accounts` in the server.
#[derive(clap::Args, Debug, Clone)]
struct AccountFlags {
    /// Turn on accounts, organisations, roles, single sign-on and SCIM without
    /// public sign-up. People arrive by invitation, single sign-on or SCIM.
    /// Configured keys keep working beside them. `--signup` implies this.
    #[arg(long, env = "MCPD_ACCOUNTS", global = true)]
    accounts: bool,

    /// Where people open the interface, such as https://mcpd.example.com. The
    /// links in mail and the single sign-on redirect URL start here. Defaults
    /// to http://localhost:4317.
    #[arg(long, env = "MCPD_PUBLIC_URL", global = true)]
    public_url: Option<String>,

    /// The SMTP server mail goes through. With none, links are written to the
    /// log, except with --signup on a public address, where the features that
    /// need mail are refused.
    #[arg(long, env = "MCPD_SMTP_HOST", global = true)]
    smtp_host: Option<String>,

    /// The SMTP port. Defaults to 587 for starttls, 465 for tls and 25 for none.
    #[arg(long, env = "MCPD_SMTP_PORT", global = true)]
    smtp_port: Option<u16>,

    /// The SMTP user, when the server wants one.
    #[arg(long, env = "MCPD_SMTP_USER", global = true)]
    smtp_user: Option<String>,

    /// The SMTP password. Prefer the environment variable to the flag, so it is
    /// not in the process list.
    #[arg(long, env = "MCPD_SMTP_PASSWORD", global = true, hide_env_values = true)]
    smtp_password: Option<String>,

    /// The From address, such as "mcpd <no-reply@example.com>". Required with
    /// --smtp-host.
    #[arg(long, env = "MCPD_SMTP_FROM", global = true)]
    smtp_from: Option<String>,

    /// How the SMTP connection is secured: starttls (the default), tls or none.
    #[arg(long, env = "MCPD_SMTP_TLS", global = true)]
    smtp_tls: Option<String>,

    /// The signals-per-minute ceiling an organisation shares across its
    /// projects when it sets none, and the most it may set. With --signup it
    /// defaults to 60000, so one tenant cannot starve the others.
    #[arg(long, env = "MCPD_ORG_QUOTA_PER_MINUTE", global = true)]
    org_quota_per_minute: Option<u32>,

    /// How long an organisation's projects keep data when it sets none, in
    /// days, and the longest it may set. Falls back to --retention-days.
    #[arg(long, env = "MCPD_ORG_RETENTION_DAYS", global = true)]
    org_retention_days: Option<u32>,
}

/// The `accounts` block of the config file.
#[derive(Debug, Default, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct FileAccounts {
    /// Whether an account may create organisations. Defaults to whether
    /// --signup is on.
    #[serde(default)]
    allow_org_creation: Option<bool>,
    /// Let an organisation admin point single sign-on at an http issuer or one
    /// on a private network. For tests and closed networks.
    #[serde(default)]
    allow_private_issuers: bool,
    /// Organisations this file declares, with their owners, projects, single
    /// sign-on and SCIM token.
    #[serde(default)]
    orgs: Vec<mcpd_server::accounts::ConfigOrg>,
}

/// The account settings, from the flags and the config file.
fn account_settings(
    cli: &Cli,
    file: &FileConfig,
) -> anyhow::Result<mcpd_server::accounts::AccountSettings> {
    use mcpd_server::mail::{MailConfig, SmtpConfig, SmtpTls};
    let f = &cli.account_flags;
    let enabled = f.accounts || cli.signup;
    let tls = SmtpTls::parse(f.smtp_tls.as_deref().unwrap_or("")).map_err(anyhow::Error::msg)?;
    let loopback = cli.listen.parse::<SocketAddr>().is_ok_and(|a| a.ip().is_loopback());
    let mail = match f.smtp_host.as_deref().map(str::trim).filter(|h| !h.is_empty()) {
        Some(host) => {
            let Some(from) = f.smtp_from.clone().filter(|f| !f.trim().is_empty()) else {
                anyhow::bail!("--smtp-host is set with no --smtp-from; add the address mail is sent from, such as no-reply@example.com");
            };
            MailConfig::Smtp(SmtpConfig {
                host: host.to_owned(),
                port: f.smtp_port,
                user: f.smtp_user.clone(),
                password: f.smtp_password.clone(),
                from,
                tls,
            })
        }
        None if cli.signup && !loopback => MailConfig::Refuse,
        None => MailConfig::Log,
    };
    // Built once here so a bad From address stops the start, rather than the
    // first sign-up.
    mcpd_server::mail::Mailer::new(mail.clone()).map_err(anyhow::Error::msg)?;
    if let Some(url) = &f.public_url {
        let ok = url.starts_with("https://") || url.starts_with("http://");
        if !ok {
            anyhow::bail!("--public-url must start with https:// or http://, got {url:?}");
        }
    }
    let org_quota = match f.org_quota_per_minute {
        Some(0) => anyhow::bail!(
            "--org-quota-per-minute must be at least 1, got 0; omit it for the default"
        ),
        Some(n) if n > mcpd_server::state::QUOTA_MAX => {
            anyhow::bail!(
                "--org-quota-per-minute must be at most {}, got {n}",
                mcpd_server::state::QUOTA_MAX
            )
        }
        Some(n) => Some(n),
        None if cli.signup => Some(60_000),
        None => None,
    };
    if f.org_retention_days == Some(0) {
        anyhow::bail!(
            "--org-retention-days must be at least 1, got 0; omit it to keep --retention-days"
        );
    }
    let block = file.accounts.clone().unwrap_or_default();
    if !block.orgs.is_empty() && !enabled {
        anyhow::bail!("the config file declares organisations, and accounts are off; add --accounts or --signup");
    }
    Ok(mcpd_server::accounts::AccountSettings {
        enabled,
        public_url: f.public_url.clone(),
        mail,
        org_quota_per_minute: org_quota,
        org_retention_days: f.org_retention_days,
        allow_org_creation: block.allow_org_creation,
        allow_private_issuers: block.allow_private_issuers,
        orgs: block.orgs,
    })
}

/// The parts of the configuration that do not fit on a command line.
///
/// Every field is optional, so an empty file, or no file at all, is valid and
/// means "nothing configured" rather than an error.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    /// Conditions to watch.
    #[serde(default)]
    monitors: Vec<mcpd_server::monitor::Monitor>,
    /// Service level objectives, evaluated on the same timer.
    #[serde(default)]
    slos: Vec<mcpd_server::monitor::Slo>,
    /// How often to evaluate them, in seconds.
    #[serde(default)]
    monitor_interval_seconds: Option<u64>,
    /// Where to copy spans on to.
    #[serde(default)]
    forward: Option<mcpd_server::forward::Forwarder>,
    /// The model that grades runs, and what it grades against.
    #[serde(default)]
    judge: Option<mcpd_server::judge::JudgeConfig>,
    /// Per-project keys, retention and sampling. This is what makes a project a
    /// tenancy boundary rather than a label.
    #[serde(default)]
    projects: std::collections::BTreeMap<String, mcpd_server::state::ProjectConfig>,
    /// The run store's timings. Every field is optional and the defaults are
    /// right for one daemon on one machine.
    #[serde(default)]
    runs: RunSettings,
    /// Largest request body accepted, in bytes. `--max-body-bytes` beats it.
    #[serde(default)]
    max_body_bytes: Option<usize>,
    /// Most signals accepted in one request. `--max-batch-signals` beats it.
    #[serde(default)]
    max_batch_signals: Option<usize>,
    /// Which metrics the daemon derives from the spans it stores. Both halves
    /// are on when the block is absent. Omitting one field inside the block
    /// leaves that half on, so `{ "tokens": false }` keeps the durations.
    #[serde(default)]
    derived_metrics: Option<mcpd_server::derive::DerivedMetrics>,
    /// Whether the daemon clusters aspect notes, and the bounds it does it
    /// under. Off when the block is absent, so a daemon nobody has configured
    /// spawns no analysis timer, takes no connection for it and writes none of
    /// its four tables. Switching it on needs no model and no credential: the
    /// `outcome` aspect is derived, so the daemon fills it from facts it
    /// already holds.
    #[serde(default)]
    aspects: Option<mcpd_server::aspects::AspectSettings>,
    /// Accounts: who may create organisations, and the organisations this file
    /// declares with their owners, projects, single sign-on and SCIM token.
    #[serde(default)]
    accounts: Option<FileAccounts>,
}

/// The knobs the run store's ticks read, as a config file spells them: whole
/// milliseconds, the unit every duration on the wire uses.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RunSettings {
    /// How long a lease lasts without a heartbeat. Default 30000.
    #[serde(default)]
    lease_ttl_ms: Option<u64>,
    /// How long an expired lease waits for evidence before anything decides
    /// for it. Default 30000.
    #[serde(default)]
    reclaim_window_ms: Option<u64>,
    /// How long a run may sit still before the reconciler looks at it.
    /// Default 300000.
    #[serde(default)]
    stall_window_ms: Option<u64>,
    /// How deep a run tree may go. Default 8.
    #[serde(default)]
    max_depth: Option<u32>,
    /// How long a run's idempotency key is held. Default 86400000.
    #[serde(default)]
    idempotency_window_ms: Option<u64>,
    /// How long a parked event waits for a run. Default 86400000.
    #[serde(default)]
    park_ttl_ms: Option<u64>,
    /// How many times one step key may fail with an identical rendered input
    /// before the run is ended as a repetition. Default 16. Zero turns the
    /// breaker off. Not a duration, so it is a count and not `_ms`.
    #[serde(default)]
    max_step_repeats: Option<u32>,
}

impl RunSettings {
    fn into_config(self) -> Result<mcpd_runs::RunConfig> {
        let mut c = mcpd_runs::RunConfig::default();
        // The run store adds each of these to the clock and refuses a sum past
        // its own bound, so a value over it was accepted here and then failed
        // every claim with "out of range". It is refused at start instead, with
        // the key, the bound and the value.
        let max = mcpd_runs::MAX_DURATION_MS;
        let ms = |name: &str, v: Option<u64>| -> Result<Option<time::Duration>> {
            match v {
                None => Ok(None),
                Some(0) => anyhow::bail!("runs.{name} must be at least 1, got 0"),
                Some(n) => match i64::try_from(n) {
                    Ok(n) if n <= max => Ok(Some(time::Duration::milliseconds(n))),
                    _ => anyhow::bail!(
                        "runs.{name} must be at most {max} ms (3650 days), the longest duration \
                         the run store holds, got {n}; omit it for the default"
                    ),
                },
            }
        };
        if let Some(d) = ms("lease_ttl_ms", self.lease_ttl_ms)? {
            c.lease_ttl = d;
        }
        if let Some(d) = ms("reclaim_window_ms", self.reclaim_window_ms)? {
            c.reclaim_window = d;
        }
        if let Some(d) = ms("stall_window_ms", self.stall_window_ms)? {
            c.stall_window = d;
        }
        if let Some(d) = ms("idempotency_window_ms", self.idempotency_window_ms)? {
            c.idempotency_window = d;
        }
        if let Some(d) = ms("park_ttl_ms", self.park_ttl_ms)? {
            c.park_ttl = d;
        }
        if let Some(n) = self.max_depth {
            if n == 0 {
                anyhow::bail!("runs.max_depth must be at least 1, got 0");
            }
            c.max_depth = i64::from(n);
        }
        // Zero is accepted here, unlike every other setting above, because for
        // this one it is the honest way to write "off" rather than a way to
        // take the daemon down: a breaker that never trips is the behaviour
        // there was before it existed.
        if let Some(n) = self.max_step_repeats {
            if n == 1 {
                anyhow::bail!(
                    "runs.max_step_repeats must be 0 to turn the breaker off or at least 2, \
                     got 1; a ceiling of 1 would end a run on the first failure of any step, \
                     whatever its retry policy says"
                );
            }
            c.max_step_repeats = n;
        }
        Ok(c)
    }
}

/// A limit from the flag, then the config file, then the built-in default.
///
/// A flag beats the file so one instance can be given a different bound
/// without editing a ConfigMap every other instance shares, which is the rule
/// `--forward-endpoint` already follows.
///
/// Zero is refused rather than read as "no limit". It is a plausible way to
/// write one and it means the opposite: a body cap of zero refuses every
/// request and a signal cap of zero refuses every batch, so an operator who
/// meant "unlimited" would take their own ingest down. There is no way to turn
/// either bound off, which is deliberate.
fn limit_from(
    flag_name: &str,
    flag: Option<usize>,
    file: Option<usize>,
    default: usize,
) -> Result<usize> {
    match flag.or(file) {
        None => Ok(default),
        Some(0) => anyhow::bail!(
            "{flag_name} must be at least 1, got 0; there is no way to turn the limit off, \
             and omitting it gives the default of {default}"
        ),
        Some(n) => Ok(n),
    }
}

/// The longest retention window accepted, in days: about a hundred years.
///
/// Far past any window a deployment means, and well inside the dates the
/// clock can hold. The cutoff is now minus the window, and a window past the
/// calendar's range made that subtraction panic on the first pass.
const RETENTION_DAYS_MAX: u32 = 36_500;

/// Refuse a retention window that does the opposite of what it says.
///
/// Zero is the dangerous one. It reads like "no retention" and it means a
/// cutoff of now, so the pass that runs at boot deletes every signal the
/// project holds, and on ClickHouse it becomes a TTL of zero days. Refused the
/// way `limit_from` refuses a cap of zero, with the way to say what was meant.
fn check_retention(name: &str, days: Option<u32>) -> Result<()> {
    match days {
        Some(0) => anyhow::bail!(
            "{name} must be at least 1, got 0; a window of 0 days deletes every signal on the \
             first pass, which runs at start-up. To keep everything, omit it"
        ),
        Some(n) if n > RETENTION_DAYS_MAX => anyhow::bail!(
            "{name} must be at most {RETENTION_DAYS_MAX} days, about a hundred years, got {n}; \
             to keep everything, omit it"
        ),
        _ => Ok(()),
    }
}

/// One `--forward-header`, as the name and value it sends.
///
/// A value with no colon used to be dropped without a word, so an OTel-style
/// `key=value` header, the form `OTEL_EXPORTER_OTLP_HEADERS` uses, never
/// reached the destination and every batch was refused for want of the key it
/// carried. It is refused at start-up instead, with the form that works. The
/// name and value are checked the way the HTTP client will check them, so a
/// header it could never send is found here rather than on every batch.
fn forward_header(raw: &str) -> Result<(String, String)> {
    let Some((name, value)) = raw.split_once(':') else {
        let hint = match raw.split_once('=') {
            Some((k, v)) => format!("; for this one write \"{}: {}\"", k.trim(), v.trim()),
            None => String::new(),
        };
        anyhow::bail!(
            "--forward-header {raw:?} has no colon; write it as \"Name: value\", such as \
             \"Authorization: Bearer sk-...\"{hint}. The flag and MCPD_FORWARD_HEADERS are \
             split on commas, so a value that holds a comma arrives here in pieces"
        );
    };
    let (name, value) = (name.trim(), value.trim());
    if reqwest::header::HeaderName::from_bytes(name.as_bytes()).is_err() {
        anyhow::bail!(
            "--forward-header {raw:?} names the header {name:?}, which is not a valid header \
             name; a name is letters, digits and dashes before the colon"
        );
    }
    if reqwest::header::HeaderValue::from_bytes(value.as_bytes()).is_err() {
        anyhow::bail!(
            "--forward-header {raw:?} has a value that cannot be sent in a header; remove the \
             control characters from it"
        );
    }
    Ok((name.to_owned(), value.to_owned()))
}

/// Where the run store lives, from the flag or from the data path.
fn runs_path(data: &str, runs_data: Option<&str>) -> Option<String> {
    match runs_data.map(str::trim) {
        Some("off") => None,
        Some(p) if !p.is_empty() => Some(p.to_owned()),
        _ if data == ":memory:" => Some(":memory:".to_owned()),
        _ => {
            let path = std::path::Path::new(data);
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("mcpd");
            let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("db");
            Some(path.with_file_name(format!("{stem}.runs.{ext}")).to_string_lossy().into_owned())
        }
    }
}

/// Where the account store lives, from the flag or from the data path. `None`
/// when sign-up is off, because then no file is opened at all.
fn accounts_path(data: &str, enabled: bool, accounts_data: Option<&str>) -> Option<String> {
    if !enabled {
        return None;
    }
    match accounts_data.map(str::trim) {
        Some(p) if !p.is_empty() => Some(p.to_owned()),
        _ if data == ":memory:" => Some(":memory:".to_owned()),
        _ => {
            let path = std::path::Path::new(data);
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("mcpd");
            let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("db");
            Some(
                path.with_file_name(format!("{stem}.accounts.{ext}"))
                    .to_string_lossy()
                    .into_owned(),
            )
        }
    }
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Run the daemon. This is the default.
    Serve,
    /// Fill the store with a realistic sample run, then serve, so the
    /// interface has something in it the first time you open it.
    Demo,
    /// Print the configuration this invocation would use, then exit.
    Config,
    /// Serve the Model Context Protocol over stdio, so an agent can read its
    /// own traces from a running daemon.
    Mcp(mcp::Args),
    /// Check this deployment and report what is misconfigured. Exits 2 on a
    /// fault, so it can gate a deploy.
    Doctor(doctor::Args),
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Dispatched before the subscriber is installed, and the order is
    // load-bearing. `tracing_subscriber::fmt()` writes to stdout, and `mcpd mcp`
    // owns stdout for JSON-RPC: one log line in that stream ends the session for
    // every MCP client. Returning here means no subscriber is ever installed, so
    // tracing events inside the server are no-ops rather than corruption.
    if let Some(Command::Mcp(args)) = &cli.command {
        return mcp::run(args, &cli.project).await;
    }

    let filter = EnvFilter::try_from_env("MCPD_LOG")
        .unwrap_or_else(|_| EnvFilter::new("mcpd=info,mcpd_server=info,tower_http=warn"));
    if cli.log_format == "json" {
        tracing_subscriber::fmt().json().with_env_filter(filter).init();
    } else {
        tracing_subscriber::fmt().with_env_filter(filter).with_target(false).init();
    }

    // `deny_unknown_fields` above means a typo in a monitor's key fails the
    // start rather than silently watching nothing.
    let file: FileConfig = match &cli.config_file {
        Some(path) => {
            let text = std::fs::read_to_string(path)
                .with_context(|| format!("reading the config file at {path}"))?;
            serde_json::from_str(&text)
                .with_context(|| format!("{path} is not a valid mcpd config file"))?
        }
        None => FileConfig::default(),
    };

    // A ceiling larger than the counter can hold would be clamped without
    // saying so, and the daemon would enforce a number nobody chose. The flag
    // is checked the same way below; this is the other way in.
    for (name, project) in &file.projects {
        match project.quota_per_minute {
            Some(0) => {
                anyhow::bail!("project {name} sets quota_per_minute to 0; remove it for no ceiling")
            }
            Some(n) if n > mcpd_server::state::QUOTA_MAX => anyhow::bail!(
                "project {name} sets quota_per_minute to {n}; the most this daemon enforces \
                 is {}",
                mcpd_server::state::QUOTA_MAX
            ),
            _ => {}
        }
    }

    // Both ways a window is set, refused before anything is opened: the first
    // retention pass runs at boot, so a bad window acts before anyone reads a
    // log line about it.
    check_retention("--retention-days", cli.retention_days)?;
    for (name, project) in &file.projects {
        check_retention(&format!("project {name} retention_days"), project.retention_days)?;
    }
    // The settings the background timers would otherwise panic on, or read as
    // something other than what was written. Refused here, before any timer
    // exists, with the key named.
    mcpd_server::monitor::check_config(&file.slos, file.monitor_interval_seconds)
        .map_err(anyhow::Error::msg)?;
    if let Some(aspects) = &file.aspects {
        aspects.check().map_err(anyhow::Error::msg)?;
    }
    if let Some(judge) = &file.judge {
        judge.check().map_err(anyhow::Error::msg)?;
    }

    // A flag beats the file, so a deployment can point one instance somewhere
    // else without editing a shared ConfigMap.
    let forward = match &cli.forward_endpoint {
        Some(endpoint) => Some(mcpd_server::forward::Forwarder {
            endpoint: endpoint.clone(),
            headers: cli
                .forward_headers
                .iter()
                .map(|h| forward_header(h))
                .collect::<Result<_>>()?,
            enabled: true,
            batch_size: 256,
        }),
        None => file.forward.clone(),
    };

    // Parsed before anything starts. A range mcpd cannot read is a trust list
    // nobody knows the reach of, and carrying on would leave an operator unable
    // to tell whether their proxy is trusted.
    //
    // The ranges serve two headers. Given only for the client address, they
    // are not handed to the identity half, which would otherwise refuse them
    // as ranges with no header to read.
    let client_ip = mcpd_server::identity::ClientIp::new(
        cli.client_ip_header.as_deref(),
        &cli.trusted_proxy_cidrs,
    )?;
    let identity_ranges: &[String] =
        if cli.trusted_identity_header.is_none() && client_ip.header().is_some() {
            &[]
        } else {
            &cli.trusted_proxy_cidrs
        };
    let trusted_identity = mcpd_server::identity::TrustedIdentity::new(
        cli.trusted_identity_header.as_deref(),
        identity_ranges,
    )?;

    // Each origin is checked before anything starts. A typo here is an
    // interface that can never connect and a daemon that says nothing about
    // why, so it is a refusal to start with the flag named instead. Lowercased
    // because a browser sends the origin lowercased and the match is exact.
    let cors_origins: Vec<String> = cli
        .cors_origins
        .iter()
        .map(|o| o.trim().to_ascii_lowercase())
        .filter(|o| !o.is_empty())
        .map(|o| match mcpd_server::layers::check_origin(&o) {
            Ok(()) => Ok(o),
            Err(why) => Err(anyhow::anyhow!("--cors-origin {why}")),
        })
        .collect::<Result<_>>()?;

    if cli.request_timeout_seconds == 0 {
        anyhow::bail!(
            "--request-timeout-seconds must be at least 1, got 0; there is no way to turn the \
             deadline off"
        );
    }

    // The two redaction additions contradict `--no-redact`, which turns the
    // whole pipeline off. Applying them to a disabled redactor would store
    // everything and print a configuration that says it does not.
    if cli.no_redact && !cli.redact_key_contains.is_empty() {
        anyhow::bail!(
            "--no-redact turns redaction off, so --redact-key-contains has nothing to apply \
             to; remove one of them"
        );
    }
    if cli.no_redact && cli.redact_max_string.is_some() {
        anyhow::bail!(
            "--no-redact turns redaction off, so --redact-max-string has nothing to apply to; \
             remove one of them"
        );
    }
    if cli.redact_max_string == Some(0) {
        anyhow::bail!(
            "--redact-max-string must be at least 1, got 0; a cap of 0 would store no text at all"
        );
    }
    let redact_keys: Vec<String> = cli
        .redact_key_contains
        .iter()
        .map(|k| k.trim().to_ascii_lowercase())
        .filter(|k| !k.is_empty())
        .collect();

    // Read once, so the numbers a flag and a config key fall back to are the
    // ones the server itself declares rather than a copy of them here. Every
    // field of `ServerConfig` is named below, which is why there is no
    // `..defaults` at the end: a field added there stops this compiling until
    // somebody decides how an operator sets it, which is the whole subject of
    // this block.
    let defaults = ServerConfig::default();
    let account_settings = account_settings(&cli, &file)?;
    let mut config = ServerConfig {
        default_project: cli.project.clone(),
        max_body_bytes: limit_from(
            "--max-body-bytes",
            cli.max_body_bytes,
            file.max_body_bytes,
            defaults.max_body_bytes,
        )?,
        max_batch_signals: limit_from(
            "--max-batch-signals",
            cli.max_batch_signals,
            file.max_batch_signals,
            defaults.max_batch_signals,
        )?,
        cors_origins,
        request_timeout_seconds: cli.request_timeout_seconds,
        derived_metrics: file.derived_metrics.unwrap_or(defaults.derived_metrics),
        monitors: file.monitors.clone(),
        slos: file.slos.clone(),
        judge: file.judge.clone(),
        projects: file.projects.clone(),
        monitor_interval_seconds: file.monitor_interval_seconds.unwrap_or(60),
        aspects: file.aspects.clone().unwrap_or_default(),
        forward,
        retention_days: cli.retention_days,
        ingest_keys: cli.ingest_keys.clone(),
        read_keys: cli.read_keys.clone(),
        write_keys: cli.write_keys.clone(),
        admin_keys: cli.admin_keys.clone(),
        execute_keys: cli.execute_keys.clone(),
        worker_keys: cli.worker_keys.clone(),
        run_config: file.runs.into_config()?,
        trusted_identity,
        signup: cli.signup,
        account_settings,
        client_ip,
        sampling: match cli.sample {
            Some(p) if (0.0..=1.0).contains(&p) => Policy::deterministic(p),
            Some(p) => anyhow::bail!("--sample must be between 0.0 and 1.0, got {p}"),
            None => Policy { kind: SamplingKind::Always, ..Policy::default() },
        },
        quota_per_minute: match cli.quota_per_minute {
            Some(0) => anyhow::bail!(
                "--quota-per-minute must be at least 1, got 0; omit the flag for no ceiling"
            ),
            Some(n) if n > mcpd_server::state::QUOTA_MAX => anyhow::bail!(
                "--quota-per-minute must be at most {}, got {n}",
                mcpd_server::state::QUOTA_MAX
            ),
            other => other,
        },
    };
    // Graded before the drop below, which makes a list that held only blank
    // entries look like one nobody set. Refused at start and reported by
    // `mcpd doctor`; `doctor::check_blank_key_lists` says why.
    let blank_lists = doctor::check_blank_key_lists(&config);
    // Before anything reads a key list: `mcpd config`, `mcpd doctor`, the
    // start-up checks and the server all see the lists without blank entries.
    // `ServerConfig::drop_blank_keys` says why an empty variable produces one.
    config.drop_blank_keys();

    if matches!(cli.command, Some(Command::Config)) {
        for line in config_lines(&cli, &config, &redact_keys) {
            println!("{line}");
        }
        return Ok(());
    }

    // Naming two backends is a configuration mistake, not a preference, and
    // silently picking one would put the data somewhere the operator did not
    // intend.
    if cli.clickhouse_url.is_some() && cli.postgres_url.is_some() {
        anyhow::bail!("--clickhouse-url and --postgres-url both name a backend; set one of them");
    }

    let store: Arc<dyn Store> = match (&cli.clickhouse_url, &cli.postgres_url) {
        (Some(url), _) => {
            let store = mcpd_store::ClickHouseStore::new(mcpd_store::ClickHouseConfig {
                url: url.clone(),
                database: cli.clickhouse_database.clone(),
                user: cli.clickhouse_user.clone(),
                password: cli.clickhouse_password.clone(),
                retention_days: cli.retention_days,
                ..Default::default()
            })
            .context("configuring the ClickHouse store")?;
            store.migrate().await.context("applying ClickHouse migrations")?;
            Arc::new(store)
        }
        (_, Some(url)) => {
            let store = mcpd_store::PostgresStore::new(mcpd_store::PostgresConfig {
                url: url.clone(),
                ..Default::default()
            })
            .context("configuring the Postgres store")?;
            store.migrate().await.context("applying Postgres migrations")?;
            Arc::new(store)
        }
        (None, None) => {
            let store = SqliteStore::open(&cli.data, 4)
                .with_context(|| format!("opening the store at {}", cli.data))?;
            store.migrate().await.context("applying migrations")?;
            Arc::new(store)
        }
    };

    // The run store is a second embedded file, whatever the signal store is,
    // and it is opened after the signal store so a daemon that cannot open its
    // data refuses to start before it has written anything anywhere.
    let run_store: Option<Arc<dyn mcpd_runs::RunStore>> =
        match runs_path(&cli.data, cli.runs_data.as_deref()) {
            None => None,
            Some(path) => {
                let store = mcpd_runs::SqliteRunStore::open(&path, config.run_config)
                    .with_context(|| format!("opening the run store at {path}"))?;
                store.migrate().await.context("applying run store migrations")?;
                Some(Arc::new(store))
            }
        };

    // A path with nothing to put in it is a sign the operator thinks sign-up is
    // on. Refused rather than ignored, so the daemon they start is the one
    // they meant.
    if cli.accounts_data.as_deref().is_some_and(|p| !p.trim().is_empty())
        && !config.accounts_enabled()
    {
        anyhow::bail!(
            "--accounts-data is set with no --signup or --accounts, so no account would ever be \
             stored; add one of them, or drop --accounts-data"
        );
    }
    // The third embedded file, opened only with --signup. Without it no file is
    // created and nothing about the daemon changes.
    let account_store: Option<Arc<mcpd_server::accounts::AccountStore>> =
        match accounts_path(&cli.data, config.accounts_enabled(), cli.accounts_data.as_deref()) {
            None => None,
            Some(path) => Some(Arc::new(
                mcpd_server::accounts::AccountStore::open(&path)
                    .with_context(|| format!("opening the account store at {path}"))?,
            )),
        };

    // After both stores are built and migrated, so doctor has something to
    // probe, and before AppState, the monitor spawn and the retention timer, so
    // it does not start background work it will never use.
    if let Some(Command::Doctor(args)) = &cli.command {
        return doctor::run(args, &config, blank_lists, &store, run_store.as_ref(), &cli.listen)
            .await;
    }

    // The configuration-only half of that report, on every start.
    //
    // `mcpd doctor` finds mistakes that never announce themselves, which is
    // exactly why the deployment carrying one is usually the deployment nobody
    // ran doctor against. These checks read `config` and the listen address and
    // touch neither the store nor the network, so serving them costs
    // microseconds. `doctor::preflight` says which checks those are, and why a
    // fault refuses here while a warning does not.
    //
    // Below the two dispatches above, deliberately. `mcpd config` exists to
    // show you the settings you got wrong and `mcpd doctor` exists to print the
    // whole report for them, so neither can be the command that refuses to run.
    let preflight = doctor::preflight(&config, &cli.listen, run_store.is_some());
    for finding in &preflight {
        if finding.level == doctor::Level::Warn {
            tracing::warn!("{}: {}", finding.group, finding.message);
        }
    }
    let mut refusals: Vec<String> = blank_lists
        .iter()
        .chain(&preflight)
        .filter(|f| f.level == doctor::Level::Fault)
        .map(|f| format!("  {}: {}", f.group, f.message))
        .collect();
    // The redaction key ships as a placeholder in the same example files as the
    // API keys, and it is not part of `ServerConfig`, so it is checked here. A
    // published hashing key makes every hashed identifier guessable.
    if cli.redact_key.as_deref().is_some_and(doctor::is_placeholder) {
        refusals.push(
            "  keys: --redact-key is the placeholder from an example file, not a key, so every \
             identifier it hashes can be recovered by anyone who read that file. Generate one \
             with `openssl rand -hex 32` and set that in its place."
                .to_owned(),
        );
    }
    if !refusals.is_empty() {
        anyhow::bail!(
            "the configuration cannot do what it says, so mcpd will not start:\n{}\nFix the \
             lines above, or run `mcpd doctor` for the whole report.",
            refusals.join("\n")
        );
    }

    let redactor = if cli.no_redact {
        mcpd_core::redact::Redactor::disabled()
    } else {
        let mut r = mcpd_core::redact::Redactor::with_defaults();
        if let Some(k) = &cli.redact_key {
            r = r.with_hash_key(k.clone());
        }
        if !redact_keys.is_empty() {
            r = r.with_rule(mcpd_core::redact::Rule {
                key_contains: redact_keys.clone(),
                mode: mcpd_core::RedactionMode::Mask,
            });
        }
        if let Some(n) = cli.redact_max_string {
            r = r.with_max_string(n);
        }
        r
    };

    let mut state = AppState::new(Arc::clone(&store), config.clone()).with_redactor(redactor);
    if let Some(path) = &cli.price_book {
        state = state.with_price_book(load_price_book(path)?);
    }
    if let Some(runs) = run_store.clone() {
        state = state.with_run_store(runs);
    }
    if let Some(accounts) = account_store.clone() {
        state = state.with_account_store(accounts);
    }
    let state = Arc::new(state);
    // Organisations the config file declares, applied before the first request
    // so their owners, projects and single sign-on are in place from the start.
    if state.accounts.is_some() {
        mcpd_server::orgs::apply_config(&state).await.map_err(anyhow::Error::msg)?;
    }

    // Monitors run in the background for the life of the process. `spawn` is a
    // no-op when none are configured, so this costs nothing by default.
    mcpd_server::monitor::spawn(Arc::clone(&state));
    // The run store's deadline tick and its span outbox drain. A no-op when
    // the daemon serves no runs.
    mcpd_server::runs::spawn(Arc::clone(&state));
    // Aspect clustering, on the same terms: `spawn` returns before spawning
    // anything unless the config file switched it on, so this costs nothing by
    // default.
    mcpd_server::aspects::spawn(Arc::clone(&state));

    if matches!(cli.command, Some(Command::Demo)) {
        let n = demo::seed(
            &*store,
            &cli.project,
            &state.redactor,
            &state.price_book,
            &state.config.sampling,
        )
        .await?;
        tracing::info!("seeded {n} demo signals");
        if let Some(runs) = &run_store {
            let seeded = demo::seed_runs(runs.as_ref(), &cli.project).await?;
            tracing::info!("seeded {seeded} demo runs");
            // Written now rather than on the drain's first pass, so the trace of
            // the demo run is on screen the moment the interface opens.
            mcpd_server::runs::drain_once(&state).await;
        }
    }

    let addr: SocketAddr = cli
        .listen
        .parse()
        .with_context(|| format!("{} is not a valid address; try 0.0.0.0:4317", cli.listen))?;

    // Warn loudly about the one genuinely dangerous default: open ingest on a
    // non-loopback address. Refusing to start would be worse — it breaks the
    // five-minute path — but silence would be irresponsible.
    //
    // With sign-up on no scope is open, so these three would be false. The one
    // warning that applies instead is about the limits on sign-up and log-in.
    let open_warnings = !config.signup;
    if config.signup
        && matches!(config.account_settings.mail, mcpd_server::mail::MailConfig::Refuse)
    {
        tracing::warn!(
            "sign-up is on with no SMTP server, so sign-up, password reset, address changes and \
             invitations are refused: each needs a mailed link. Set --smtp-host and --smtp-from."
        );
    }
    if config.accounts_enabled()
        && config.account_settings.public_url.is_none()
        && !addr.ip().is_loopback()
    {
        tracing::warn!(
            "accounts are on with no --public-url, so mailed links and the single sign-on \
             redirect point at http://localhost:4317. Set --public-url to where people open the \
             interface."
        );
    }
    if config.signup && config.client_ip.header().is_none() && !addr.ip().is_loopback() {
        tracing::warn!(
            "sign-up is on and every client is counted by its peer address. Behind a proxy that \
             is the proxy's address, so one client can use up the sign-up and log-in limits for \
             everybody. Set --client-ip-header and --trusted-proxy-cidr."
        );
    }
    if open_warnings && config.ingest_keys.is_empty() && !addr.ip().is_loopback() {
        tracing::warn!(
            "listening on {addr} with open ingest. Anyone who can reach this port can write \
             to it. Set --ingest-key (or MCPD_INGEST_KEYS) before exposing it."
        );
    }
    if open_warnings && config.read_keys.is_empty() && !addr.ip().is_loopback() {
        tracing::warn!(
            "listening on {addr} with an open read API. Stored prompts and completions are \
             readable by anyone who can reach this port. Set --read-key to require a key."
        );
    }
    // The run scopes inherit from the write list, which inherits from the
    // ingest list. With no keys at all the run surface is open, like the rest
    // of the daemon, and the same warning applies on a non-loopback address.
    if open_warnings
        && run_store.is_some()
        && config.keys_for(mcpd_server::auth::Scope::Execute).is_empty()
        && !addr.ip().is_loopback()
    {
        tracing::warn!(
            "listening on {addr} with an open run surface. Anyone who can reach this port can \
             start runs and claim steps. Set --execute-key and --worker-key before exposing it."
        );
    }
    // A scope that falls back to a wider list is not warned about here: the
    // unset --admin-key that widens the read keys, and the ingest keys that
    // start runs and claim steps when neither --execute-key nor --write-key is
    // set, are findings `doctor::preflight` logs above, so one place decides
    // what a fallback means rather than two that can drift apart. The three
    // warnings here are about scopes that need no key at all, and the first two
    // are the ones the quickstart and deploy pages quote line for line.

    // Retention runs on a timer rather than at query time, so a large delete
    // never lands in a user's request.
    //
    // Every configured project gets its own window, falling back to the global
    // one. It used to run for `--project` alone, so on a shared daemon four
    // teams' data was kept for ever while the fifth was trimmed, and nothing
    // said so.
    let retention_plan: Vec<(String, u32)> = {
        let mut plan: Vec<(String, u32)> = config
            .projects
            .iter()
            .filter_map(|(name, p)| {
                p.retention_days.or(cli.retention_days).map(|d| (name.clone(), d))
            })
            .collect();
        if let Some(days) = cli.retention_days {
            if !plan.iter().any(|(n, _)| n == &cli.project) {
                plan.push((cli.project.clone(), days));
            }
        }
        plan
    };

    if !retention_plan.is_empty() || account_store.is_some() {
        // Tracked and cancellable, so a stop between passes is immediate and a
        // stop during a pass waits for the delete in flight rather than
        // dropping it halfway. A panic in one pass is logged with the loop's
        // name and the next pass runs on time, rather than retention ending
        // for good while the daemon looked healthy.
        let plan = Arc::new(retention_plan);
        let store = Arc::clone(&store);
        let runs = run_store.clone();
        let accounts = account_store.clone();
        let global_days = config.account_settings.org_retention_days.or(cli.retention_days);
        mcpd_server::spawn_loop(
            &state,
            "retention",
            std::time::Duration::from_secs(3600),
            mcpd_server::FirstPass::AtStart,
            move || {
                let (plan, store, runs) = (Arc::clone(&plan), Arc::clone(&store), runs.clone());
                let accounts = accounts.clone();
                async move {
                    // Organisations' projects are created while the daemon
                    // runs, so they are read on every pass. Each takes its
                    // organisation's window, else the organisation default,
                    // else the global one. A project the configuration names
                    // keeps the window the configuration gave it.
                    let mut plan: Vec<(String, u32)> = plan.as_ref().clone();
                    if let Some(accounts) = accounts {
                        match tokio::task::spawn_blocking(move || {
                            accounts.retention_plan(global_days)
                        })
                        .await
                        {
                            Ok(Ok(projects)) => {
                                for (p, d) in projects {
                                    if !plan.iter().any(|(n, _)| *n == p) {
                                        plan.push((p, d));
                                    }
                                }
                            }
                            Ok(Err(e)) => tracing::error!("retention could not list accounts: {e}"),
                            Err(e) => tracing::error!("retention could not list accounts: {e}"),
                        }
                    }
                    for (project, days) in &plan {
                        // The saturating form, so no window can panic the
                        // pass. Start-up already refuses one past the bound;
                        // this is the second line, for a plan built some
                        // other way.
                        let Some(cutoff) = mcpd_store::RetentionPolicy::cutoff(
                            Some(*days),
                            time::OffsetDateTime::now_utc(),
                        ) else {
                            continue;
                        };
                        match store.enforce_retention(project, cutoff).await {
                            Ok(0) => {}
                            Ok(n) => tracing::info!(project, days, "retention removed {n} rows"),
                            Err(e) => tracing::error!(project, "retention pass failed: {e}"),
                        }
                        // The run store on the same window. It keeps every
                        // run's input, output and state as sent, so a project
                        // trimmed in the trace store and not here kept the
                        // payloads anyway. Only run trees that have ended go.
                        if let Some(runs) = &runs {
                            match runs.apply_retention(project, cutoff).await {
                                Ok(r) if r.runs_deleted == 0 => {}
                                Ok(r) => tracing::info!(
                                    project,
                                    days,
                                    runs = r.runs_deleted,
                                    rows = r.rows_deleted,
                                    "retention removed ended runs"
                                ),
                                Err(e) => {
                                    tracing::error!(project, "run retention pass failed: {e}");
                                }
                            }
                        }
                    }
                }
            },
        );
    }

    let app = mcpd_server::router(Arc::clone(&state));
    let listener =
        tokio::net::TcpListener::bind(addr).await.with_context(|| format!("binding {addr}"))?;

    tracing::info!("mcpd {} listening on http://{addr}", mcpd_core::VERSION);
    tracing::info!("  interface   http://{addr}/");
    tracing::info!("  OTLP        http://{addr}/v1/traces");
    tracing::info!("  native      http://{addr}/v1/ingest");
    if run_store.is_some() {
        tracing::info!("  runs        http://{addr}/v1/api/runs");
    }
    tracing::info!(
        "  storage     {}",
        // Named from the store that was actually built, not from a flag: this
        // line printed the SQLite path while running on Postgres, which is the
        // one place an operator looks to confirm where the data is going. A URL
        // or a connection string can carry a password, so it goes through the
        // same filter `mcpd config` and `mcpd doctor` use.
        match (&cli.clickhouse_url, &cli.postgres_url) {
            (Some(u), _) | (_, Some(u)) => {
                format!("{} at {}", store.name(), doctor::without_credentials(u))
            }
            (None, None) => format!("{} at {}", store.name(), cli.data),
        }
    );

    // `into_make_service_with_connect_info` is what inserts the peer address
    // extension. Without it the ConnectInfo extractor in query.rs is always
    // None, the trusted-proxy check never sees an address, and the identity
    // feature is silently dead in production while its unit tests pass.
    //
    // The token is cancelled the moment the signal arrives, not once `serve`
    // returns. `serve` returns only when every open request has finished, and
    // a claim long-poll or a run event stream finishes when that token says
    // so: cancelled afterwards, they held the stop open until their own wait
    // ran out, and the five second drain below then ran after the
    // orchestrator's SIGKILL, if it ran at all.
    let stop = state.shutdown.clone();
    axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>())
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            stop.cancel();
        })
        .await
        .context("server failed")?;

    // The listener is closed; what is left is work the requests already
    // answered started. A forward still in flight is a batch the sender was
    // told was stored, and dropping it here is the one place the daemon would
    // lose a copy without counting it. Bounded, because a destination that is
    // down must not turn a stop into a hang past the container's grace period.
    tracing::info!("shutting down");
    let grace = std::time::Duration::from_secs(5);
    let started = std::time::Instant::now();
    // The outbox drain made its last pass when the token was cancelled, and
    // requests answered after that, during the graceful stop, can have queued
    // run spans since. One more pass writes them now rather than on the next
    // start; the rows are durable either way, so this only decides when.
    if state.runs.is_some() {
        let _ = tokio::time::timeout(grace, async {
            while mcpd_server::runs::drain_once(&state).await > 0 {}
        })
        .await;
    }
    let unfinished = state.drain(grace.saturating_sub(started.elapsed())).await;
    if unfinished > 0 {
        tracing::warn!(
            tasks = unfinished,
            "stopped with background work still running after the 5s grace period; a \
             forward in flight has not reached its destination"
        );
    }
    Ok(())
}

/// A byte count in the unit it was most likely written in.
///
/// The configured number is printed back in a form an operator can compare
/// with what they set: 33554432 and "32 MiB" are the same bound, and only one
/// of them can be checked at a glance.
fn human_bytes(bytes: usize) -> String {
    const MIB: usize = 1024 * 1024;
    if bytes >= MIB && bytes.is_multiple_of(MIB) {
        format!("{} MiB", bytes / MIB)
    } else if bytes >= 1024 && bytes.is_multiple_of(1024) {
        format!("{} KiB", bytes / 1024)
    } else {
        format!("{bytes} bytes")
    }
}

/// What `mcpd config` prints, one line per setting, label then value.
///
/// Every flag and every config file key reaches one of these lines, and
/// `tools/check-consistency.mjs` holds the table of which, so a setting added
/// without a line fails `make check` rather than being impossible to read back.
/// Nothing secret is printed: keys are counted, passwords are `set` or `unset`,
/// and every URL goes through `doctor::without_credentials`.
fn config_lines(cli: &Cli, config: &ServerConfig, redact_keys: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut row = |label: &str, value: String| out.push(format!("{label:<16}{value}"));

    row("listen", cli.listen.clone());
    row(
        "storage",
        match (&cli.clickhouse_url, &cli.postgres_url) {
            (Some(u), _) => format!(
                "clickhouse at {} (database {}, user {}, password {})",
                doctor::without_credentials(u),
                cli.clickhouse_database,
                cli.clickhouse_user.as_deref().unwrap_or("none"),
                if cli.clickhouse_password.is_some() { "set" } else { "unset" }
            ),
            (_, Some(u)) => format!("postgres at {}", doctor::without_credentials(u)),
            (None, None) => format!("sqlite at {}", cli.data),
        },
    );
    row("config file", cli.config_file.clone().unwrap_or_else(|| "none".to_owned()));
    // The format the subscriber was given, which is `text` for anything that
    // is not `json`, so a misspelling is named rather than read as a choice.
    row(
        "log format",
        match cli.log_format.as_str() {
            "json" => "json".to_owned(),
            "text" => "text".to_owned(),
            other => format!("text ({other:?} is not a format; use text or json)"),
        },
    );
    row("project", config.default_project.clone());
    // Counted, never listed. The count is what tells an operator the block was
    // read; the keys themselves are not something this command prints.
    row(
        "projects",
        if config.projects.is_empty() {
            "none".to_owned()
        } else {
            config
                .projects
                .iter()
                .map(|(name, p)| {
                    let keys = [
                        &p.ingest_keys,
                        &p.read_keys,
                        &p.write_keys,
                        &p.admin_keys,
                        &p.execute_keys,
                        &p.worker_keys,
                    ]
                    .iter()
                    .map(|l| l.len())
                    .sum::<usize>();
                    match keys {
                        0 => format!("{name} (no keys)"),
                        1 => format!("{name} (1 key)"),
                        n => format!("{name} ({n} keys)"),
                    }
                })
                .collect::<Vec<_>>()
                .join(", ")
        },
    );
    // The global window, then every project that sets its own. The line used
    // to print the flag alone, so a project kept for seven days read as thirty.
    row(
        "retention",
        with_exceptions(
            config.retention_days.map_or("forever".to_owned(), |d| format!("{d} days")),
            config
                .projects
                .iter()
                .filter_map(|(name, p)| p.retention_days.map(|d| format!("{name} {d} days"))),
        ),
    );
    // With sign-up on, a scope with no configured key is not open: it takes an
    // account's key or session. The line says so rather than "open".
    let closed = |line: String| {
        if config.signup && line == "open" {
            "account key or session required".to_owned()
        } else {
            line
        }
    };
    row("ingest auth", closed(open_or_required(&config.ingest_keys)));
    row("read auth", closed(open_or_required(&config.read_keys)));
    // Printed as three states rather than two. "key required" and "key
    // required (ingest keys)" are different deployments, and an operator
    // reading this line is reading it to find out which one they have.
    row("write auth", closed(auth_line(&config.write_keys, &config.ingest_keys, "ingest keys")));
    row("admin auth", closed(auth_line(&config.admin_keys, &config.read_keys, "read keys")));
    // The run scopes inherit from the effective write list, which may itself
    // be the ingest list, and the line says which it landed on.
    let write_effective = config.keys_for(mcpd_server::auth::Scope::Write);
    let inherited_name = if config.write_keys.is_empty() { "ingest keys" } else { "write keys" };
    row("execute auth", closed(auth_line(&config.execute_keys, write_effective, inherited_name)));
    row("worker auth", closed(auth_line(&config.worker_keys, write_effective, inherited_name)));
    // Where the accounts go is the line an operator needs: it is a file to
    // back up, and on a container it has to be on the volume.
    row(
        "signup",
        match accounts_path(&cli.data, config.accounts_enabled(), cli.accounts_data.as_deref()) {
            None => "off".to_owned(),
            Some(path) => {
                let s = &config.account_settings;
                let mode = if config.signup {
                    "open"
                } else {
                    "closed, accounts by invitation, sso or scim"
                };
                let mut line = format!(
                    "{mode}, accounts in sqlite at {path}; mail {}; links to {}",
                    s.mail.describe(),
                    s.base_url()
                );
                if let Some(q) = s.org_quota_per_minute {
                    line.push_str(&format!("; org quota {q}/min"));
                }
                if let Some(d) = s.org_retention_days {
                    line.push_str(&format!("; org retention {d} days"));
                }
                if !s.orgs.is_empty() {
                    let names: Vec<&str> = s.orgs.iter().map(|o| o.slug.as_str()).collect();
                    line.push_str(&format!("; config orgs {}", names.join(", ")));
                }
                line
            }
        },
    );
    let runs = runs_path(&cli.data, cli.runs_data.as_deref());
    row("runs", runs.as_ref().map_or("off".to_owned(), |p| format!("sqlite at {p}")));
    // In the units and under the names the config file uses, so the line can
    // be compared with the `runs` block that set it.
    row(
        "run settings",
        if runs.is_none() {
            "not used: runs are off".to_owned()
        } else {
            let r = &config.run_config;
            format!(
                "lease_ttl_ms {}, reclaim_window_ms {}, stall_window_ms {}, \
                 idempotency_window_ms {}, park_ttl_ms {}, max_depth {}, max_step_repeats {}",
                r.lease_ttl.whole_milliseconds(),
                r.reclaim_window.whole_milliseconds(),
                r.stall_window.whole_milliseconds(),
                r.idempotency_window.whole_milliseconds(),
                r.park_ttl.whole_milliseconds(),
                r.max_depth,
                if r.max_step_repeats == 0 {
                    "0 (off)".to_owned()
                } else {
                    r.max_step_repeats.to_string()
                }
            )
        },
    );
    // Both halves on one line, because either alone tells an operator nothing:
    // a header nobody is allowed to send records no subject, and a range with
    // no header to read is inert. Reading a range back is the point of the
    // line. `--trusted-proxy-cidr 10.0.0.0/8` and `10.0.0.7/32` are one
    // character apart and trust a whole network or one host, and this is where
    // you find out which one you have.
    row(
        "identity",
        match config.trusted_identity.header() {
            None => "off".to_owned(),
            Some(header) => format!(
                "{header} from {}",
                config
                    .trusted_identity
                    .ranges()
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        },
    );
    // The address the sign-up and log-in limits count. Behind a proxy with this
    // off, that is the proxy's address for every client.
    row(
        "client ip",
        match config.client_ip.header() {
            None => "peer address".to_owned(),
            Some(header) => format!(
                "{header} from {}, else the peer address",
                config
                    .client_ip
                    .ranges()
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        },
    );
    row(
        "sampling",
        with_exceptions(
            match cli.sample {
                Some(p) => format!("deterministic {p}"),
                None => "keep everything".into(),
            },
            config.projects.iter().filter_map(|(name, p)| p.sample.map(|f| format!("{name} {f}"))),
        ),
    );
    // Every limited project by name, not a count. A ceiling is set from two
    // places, and the mistake it hides is a project you thought was covered and
    // is not: one that appears in no `projects` block has no ceiling, whatever
    // the flag says. Reading the list back is how you find out.
    let quotas = config.project_quotas();
    row(
        "quota",
        if quotas.is_empty() {
            "unlimited".to_owned()
        } else {
            quotas
                .limits()
                .map(|(project, limit)| format!("{project} {limit}/min"))
                .collect::<Vec<_>>()
                .join(", ")
        },
    );
    // The additions are printed by name. A rule is a substring, and `ticket`
    // and `tickets` mask different keys, so the only way to confirm the one you
    // set is the one running is to read it back.
    row(
        "redaction",
        if cli.no_redact {
            "off".to_owned()
        } else {
            let mut line = "on".to_owned();
            if !redact_keys.is_empty() {
                line.push_str(&format!(
                    ", also masking keys containing {}",
                    redact_keys.join(", ")
                ));
            }
            if let Some(n) = cli.redact_max_string {
                line.push_str(&format!(", strings cut at {n} bytes"));
            }
            line
        },
    );
    row(
        "price book",
        cli.price_book.as_ref().map_or_else(
            || "built in".to_owned(),
            |p| format!("{p}, layered over the built-in book"),
        ),
    );
    // Whether a hashing key was given, never the key. The built-in one is in
    // the source, so a digest made with it can be reversed by guessing.
    row(
        "redact key",
        match (&cli.redact_key, cli.no_redact) {
            (_, true) => "not used: redaction is off".to_owned(),
            (Some(_), false) => "set".to_owned(),
            (None, false) => "built in, which is public; set --redact-key".to_owned(),
        },
    );
    // Both halves of the origin policy on one line: "same-origin only" is the
    // default and the thing a deployment serving the interface from elsewhere
    // has to see to know why the interface cannot connect.
    row(
        "cors",
        if config.cors_origins.is_empty() {
            "same-origin only".to_owned()
        } else if config.cors_origins.iter().any(|o| o == "*") {
            "any origin".to_owned()
        } else {
            config.cors_origins.join(", ")
        },
    );
    row("timeout", format!("{}s per request", config.request_timeout_seconds));
    // Both bounds on one line, because a request meets both and an operator
    // reading this is finding out which one refused a batch.
    row(
        "limits",
        format!(
            "{} per request, {} signals per batch",
            human_bytes(config.max_body_bytes),
            config.max_batch_signals
        ),
    );
    row(
        "derived",
        match (config.derived_metrics.durations, config.derived_metrics.tokens) {
            (true, true) => "duration and token metrics".to_owned(),
            (true, false) => "duration metrics only".to_owned(),
            (false, true) => "token metrics only".to_owned(),
            (false, false) => "off".to_owned(),
        },
    );
    let aspects = &config.aspects;
    row(
        "aspects",
        if aspects.enabled {
            let mut line = format!(
                "on, {} over the last {}s",
                every(aspects.interval_seconds, mcpd_server::aspects::MIN_INTERVAL_SECONDS),
                aspects.window_seconds
            );
            if !aspects.disabled.is_empty() {
                line.push_str(&format!(", not clustering {}", aspects.disabled.join(", ")));
            }
            line
        } else {
            "off".to_owned()
        },
    );
    // The interval the timer runs on, which is not always the one written:
    // a value under the floor is raised, and the line says so.
    let monitor_every =
        every(config.monitor_interval_seconds, mcpd_server::monitor::MIN_INTERVAL_SECONDS);
    row(
        "monitors",
        if config.monitors.is_empty() {
            "none".to_owned()
        } else {
            let off = config.monitors.iter().filter(|m| !m.enabled).count();
            if off == 0 {
                format!("{}, {monitor_every}", config.monitors.len())
            } else {
                format!("{} ({off} disabled), {monitor_every}", config.monitors.len())
            }
        },
    );
    row(
        "objectives",
        if config.slos.is_empty() {
            "none".to_owned()
        } else {
            format!("{}, multi-window burn, {monitor_every}", config.slos.len())
        },
    );
    row(
        "judge",
        config.judge.as_ref().map_or_else(
            || "off".to_owned(),
            |j| {
                format!(
                    "{} at {} ({} criteria, {}s per call)",
                    j.model,
                    doctor::without_credentials(&j.endpoint),
                    j.criteria.len(),
                    j.timeout_seconds
                )
            },
        ),
    );
    // Header names only. A forward header is how the destination
    // authenticates, so its value is a credential.
    row(
        "forwarding",
        match &config.forward {
            None => "off".to_owned(),
            Some(f) if !f.enabled => "off: the forward block sets enabled to false".to_owned(),
            Some(f) => {
                let mut line = format!(
                    "{} ({} spans per request)",
                    doctor::without_credentials(&f.traces_url()),
                    f.batch_size
                );
                if !f.headers.is_empty() {
                    line.push_str(&format!(
                        ", headers {}",
                        f.headers.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>().join(", ")
                    ));
                }
                line
            }
        },
    );
    out
}

/// A setting's global value, then every project that sets its own.
fn with_exceptions(global: String, projects: impl Iterator<Item = String>) -> String {
    let projects: Vec<String> = projects.collect();
    if projects.is_empty() {
        global
    } else {
        format!("{global}, except {}", projects.join(", "))
    }
}

/// An interval as the timer runs it, naming the floor when it raised one.
fn every(configured: u64, floor: u64) -> String {
    if configured < floor {
        format!("every {floor}s (raised from {configured}s, the shortest interval)")
    } else {
        format!("every {configured}s")
    }
}

/// A scope with no fallback: open with no keys, closed with any.
fn open_or_required(keys: &[String]) -> String {
    if keys.is_empty() { "open" } else { "key required" }.to_owned()
}

/// Describe one scope that falls back to a wider list when it is unset.
fn auth_line(own: &[String], inherited: &[String], inherited_name: &str) -> String {
    if !own.is_empty() {
        "key required".to_owned()
    } else if inherited.is_empty() {
        "open".to_owned()
    } else {
        format!("key required ({inherited_name})")
    }
}

/// Wait for SIGINT or SIGTERM.
///
/// Kubernetes sends SIGTERM; a container that only handles SIGINT is killed
/// after the grace period instead of stopping cleanly.
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.expect("install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => tracing::info!("received interrupt"),
        () = terminate => tracing::info!("received termination signal"),
    }
}

mod demo;
mod doctor;
mod mcp;

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    /// The environment variable a flag reads, from clap itself rather than
    /// from a copy of the name in a test.
    fn env_of(flag: &str) -> Option<String> {
        Cli::command()
            .get_arguments()
            .find(|a| a.get_id() == flag)
            .and_then(clap::Arg::get_env)
            .map(|s| s.to_string_lossy().into_owned())
    }

    /// Both caps existed on `ServerConfig` and neither was reachable: no flag
    /// set either, and the config file declared neither key. An operator whose
    /// batches were being refused had to rebuild the binary.
    #[test]
    fn the_body_and_batch_caps_are_reachable_from_the_command_line() {
        let cli = Cli::try_parse_from([
            "mcpd",
            "--max-body-bytes",
            "1048576",
            "--max-batch-signals",
            "250",
        ])
        .expect("both flags parse");
        assert_eq!(cli.max_body_bytes, Some(1_048_576));
        assert_eq!(cli.max_batch_signals, Some(250));
    }

    /// Every other setting a deployment sets from the environment follows this
    /// naming, and a chart that has to special-case one of them is a chart
    /// nobody can read.
    #[test]
    fn each_cap_reads_the_environment_variable_the_daemon_names_elsewhere() {
        assert_eq!(env_of("max_body_bytes").as_deref(), Some("MCPD_MAX_BODY_BYTES"));
        assert_eq!(env_of("max_batch_signals").as_deref(), Some("MCPD_MAX_BATCH_SIGNALS"));
    }

    #[test]
    fn the_same_two_caps_are_reachable_from_the_config_file() {
        let file: FileConfig =
            serde_json::from_str(r#"{ "max_body_bytes": 2097152, "max_batch_signals": 500 }"#)
                .expect("the config file declares both keys");
        assert_eq!(file.max_body_bytes, Some(2_097_152));
        assert_eq!(file.max_batch_signals, Some(500));
    }

    #[test]
    fn a_flag_beats_the_file_and_the_file_beats_the_built_in_default() {
        assert_eq!(limit_from("--max-body-bytes", Some(1), Some(2), 3).expect("the flag wins"), 1);
        assert_eq!(limit_from("--max-body-bytes", None, Some(2), 3).expect("the file wins"), 2);
        assert_eq!(limit_from("--max-body-bytes", None, None, 3).expect("the default"), 3);
    }

    /// Zero is a plausible way to write "no limit" and it is the opposite: a
    /// body cap of zero refuses every request.
    #[test]
    fn a_cap_of_zero_is_refused_rather_than_read_as_unlimited() {
        let refused =
            limit_from("--max-batch-signals", Some(0), None, 10).expect_err("zero is refused");
        assert!(refused.to_string().contains("--max-batch-signals"), "{refused}");
        assert!(refused.to_string().contains("at least 1"), "{refused}");
        assert!(
            limit_from("--max-batch-signals", None, Some(0), 10).is_err(),
            "the file is the other way in and it has to be checked too"
        );
    }

    /// The message was written across two source lines without a line
    /// continuation, so it reached the operator with a run of fourteen spaces
    /// in the middle of a sentence.
    #[test]
    fn the_refusal_of_a_zero_cap_reads_as_one_sentence() {
        let refused =
            limit_from("--max-body-bytes", Some(0), None, 10).expect_err("zero is refused");
        assert!(!refused.to_string().contains("  "), "{refused}");
    }

    /// Every interval line names the one the timer runs on.
    #[test]
    fn an_interval_under_the_floor_is_printed_as_the_floor() {
        assert_eq!(every(60, 5), "every 60s");
        assert_eq!(every(2, 5), "every 5s (raised from 2s, the shortest interval)");
    }

    #[test]
    fn a_run_timing_past_the_run_store_bound_is_refused() {
        let max = u64::try_from(mcpd_runs::MAX_DURATION_MS).expect("a positive bound");
        let read = |v: u64| RunSettings { lease_ttl_ms: Some(v), ..RunSettings::default() };
        let refused = read(max + 1).into_config().expect_err("past the bound");
        assert!(refused.to_string().contains("runs.lease_ttl_ms"), "{refused}");
        assert!(refused.to_string().contains(&format!("at most {max}")), "{refused}");
        assert!(refused.to_string().contains(&format!("got {}", max + 1)), "{refused}");
        assert!(read(u64::MAX).into_config().is_err(), "past i64 as well");
        let at = read(max).into_config().expect("at the bound");
        assert_eq!(at.lease_ttl, time::Duration::milliseconds(mcpd_runs::MAX_DURATION_MS));
    }

    /// The repetition breaker is the one run setting where zero means "off"
    /// rather than "refuse everything", and one is the value that would be
    /// read as a small ceiling and is really a way to end every run on its
    /// first failure.
    #[test]
    fn the_repeat_breaker_accepts_zero_as_off_and_refuses_one() {
        let read = |v: Option<u32>| {
            RunSettings { max_step_repeats: v, ..RunSettings::default() }.into_config()
        };
        assert_eq!(read(None).expect("the default").max_step_repeats, 16);
        assert_eq!(read(Some(0)).expect("off").max_step_repeats, 0);
        assert_eq!(read(Some(2)).expect("a real ceiling").max_step_repeats, 2);
        let refused = read(Some(1)).expect_err("one is refused");
        assert!(refused.to_string().contains("max_step_repeats"), "{refused}");
        assert!(refused.to_string().contains("at least 2"), "{refused}");
    }
}

/// Read `--price-book` and layer it over the built-in book.
fn load_price_book(path: &str) -> anyhow::Result<mcpd_core::cost::PriceBook> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading the price book at {path}"))?;
    let own: mcpd_core::cost::PriceBook =
        serde_json::from_str(&text).with_context(|| format!("{path} is not a valid price book"))?;
    let mut book = mcpd_core::cost::builtin_book();
    // A book in another currency replaces the built-in one rather than mixing
    // two currencies in one table.
    if own.currency != book.currency {
        return Ok(own);
    }
    book.id = own.id;
    book.currency = own.currency;
    if own.as_of.is_some() {
        book.as_of = own.as_of;
    }
    book.models.extend(own.models);
    for (provider, rates) in own.providers {
        book.providers.entry(provider).or_default().extend(rates);
    }
    Ok(book)
}

#[cfg(test)]
mod price_book_tests {
    use super::load_price_book;

    fn write(text: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "mcpd-price-book-{}-{}.json",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&path, text).expect("written");
        path
    }

    #[test]
    fn a_book_layers_over_the_built_in_one() {
        let builtin = mcpd_core::cost::builtin_book();
        let kept = builtin.models.keys().next().expect("a built-in model").clone();
        let f = write(
            r#"{"id":"acme","currency":"USD","models":{"acme-model":{"input":1.0,"output":2.0}}}"#,
        );
        let book = load_price_book(f.to_str().unwrap()).expect("loads");
        assert_eq!(book.id, "acme");
        assert!(book.models.contains_key("acme-model"));
        assert!(book.models.contains_key(&kept), "a model it leaves out keeps the built-in rate");
    }

    #[test]
    fn a_book_in_another_currency_replaces_the_built_in_one() {
        let f = write(r#"{"id":"eu","currency":"EUR","models":{"m":{"input":1.0,"output":2.0}}}"#);
        let book = load_price_book(f.to_str().unwrap()).expect("loads");
        assert_eq!(book.models.len(), 1);
    }

    #[test]
    fn a_bad_book_names_the_file() {
        let f = write("not json");
        let err = load_price_book(f.to_str().unwrap()).unwrap_err();
        assert!(format!("{err:#}").contains("not a valid price book"));
    }
}
