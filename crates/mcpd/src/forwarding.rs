//! The command-line half of forwarding and of `mcpd relay`: turning flags and
//! the config file's `forward` block into settings, the two `mcpd config`
//! lines that read them back, and the relay's serve loop.
//!
//! Apart from `main.rs` so the flag parsing there stays a list of flags.

use anyhow::{Context, Result};
use mcpd_server::forward::{Destination, Protocol, Settings, Vendor, DEFAULT_SPOOL_MAX_BYTES};
use mcpd_server::relay::{RelayConfig, RelayState};
use mcpd_server::ServerConfig;
use std::path::PathBuf;
use std::sync::Arc;

/// The spool bound for `mcpd relay` when `--spool-max-bytes` is not set.
/// Larger than a forwarding destination's, because the relay's spool is the
/// only copy of what it holds.
pub const RELAY_SPOOL_MAX_BYTES: u64 = 1024 * 1024 * 1024;

/// What the flags say about forwarding and spooling.
pub struct Flags<'a> {
    /// `--forward-endpoint`.
    pub endpoint: Option<&'a str>,
    /// `--forward-header`, already split into name and value.
    pub headers: Vec<(String, String)>,
    /// `--forward-vendor`.
    pub vendor: Option<&'a str>,
    /// `--forward-protocol`.
    pub protocol: Option<&'a str>,
    /// `--spool-dir`.
    pub spool_dir: Option<&'a str>,
    /// `--spool-max-bytes`.
    pub spool_max_bytes: Option<u64>,
    /// `--data`, which the spool directory is derived from.
    pub data: &'a str,
}

/// Where the spools live: the flag, `memory`, or a `.spool` directory beside
/// the data file. `None` holds them in memory, which is what a store of
/// `:memory:` gets, so a daemon that keeps nothing leaves nothing behind.
#[must_use]
pub fn spool_dir(data: &str, flag: Option<&str>) -> Option<PathBuf> {
    match flag.map(str::trim) {
        Some("memory" | ":memory:") => None,
        Some(p) if !p.is_empty() => Some(PathBuf::from(p)),
        _ if data == ":memory:" => None,
        _ => {
            let path = std::path::Path::new(data);
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("mcpd");
            Some(path.with_file_name(format!("{stem}.spool")))
        }
    }
}

/// Build the forwarding settings, and refuse any destination that cannot
/// work, before anything starts.
///
/// A flag destination replaces the file's `forward` block, so one instance can
/// be pointed somewhere else without editing a ConfigMap others share, which
/// is the rule the flag has always followed.
///
/// # Errors
/// The first destination that does not resolve, worded for the operator.
pub fn settings(flags: &Flags<'_>, file: Option<&serde_json::Value>) -> Result<Settings> {
    let mut destinations = match file {
        Some(v) => mcpd_server::forward::config::parse_block(v).map_err(anyhow::Error::msg)?,
        None => Vec::new(),
    };
    if flags.endpoint.is_some() || flags.vendor.is_some() {
        let vendor = match flags.vendor {
            Some(v) => Vendor::parse(v).map_err(|e| anyhow::anyhow!("--forward-vendor {e}"))?,
            None => Vendor::Otlp,
        };
        let mut d = Destination::new(vendor, flags.endpoint.map(str::to_owned));
        d.headers = flags.headers.clone();
        d.protocol = flags
            .protocol
            .map(Protocol::parse)
            .transpose()
            .map_err(|e| anyhow::anyhow!("--forward-protocol {e}"))?;
        destinations = vec![d];
    } else if flags.protocol.is_some() {
        anyhow::bail!(
            "--forward-protocol is set with no --forward-endpoint or --forward-vendor, so there \
             is no destination for it to apply to"
        );
    }
    if flags.spool_max_bytes == Some(0) {
        anyhow::bail!(
            "--spool-max-bytes must be at least 1, got 0; a spool that holds nothing drops \
             every batch the moment its destination is slow"
        );
    }
    let settings = Settings {
        destinations,
        spool_dir: spool_dir(flags.data, flags.spool_dir),
        spool_max_bytes: flags.spool_max_bytes.unwrap_or(DEFAULT_SPOOL_MAX_BYTES),
    };
    settings.check().map_err(anyhow::Error::msg)?;
    Ok(settings)
}

/// A byte count in MiB when it is a whole number of them.
fn mib(bytes: u64) -> String {
    const MIB: u64 = 1024 * 1024;
    if bytes >= MIB && bytes.is_multiple_of(MIB) {
        format!("{} MiB", bytes / MIB)
    } else {
        format!("{bytes} bytes")
    }
}

fn spool_text(dir: Option<&std::path::Path>, bytes: u64, each: bool) -> String {
    format!(
        "spool {}, {}{}",
        dir.map_or_else(|| "in memory".to_owned(), |d| format!("at {}", d.display())),
        mib(bytes),
        if each { " each" } else { "" }
    )
}

/// The `forwarding` line of `mcpd config`. Header names only: a header is how
/// a destination authenticates, so its value is a credential, and every URL
/// goes through `without_credentials`.
#[must_use]
pub fn config_line(config: &ServerConfig) -> String {
    let f = &config.forward;
    if f.destinations.is_empty() {
        return "off".to_owned();
    }
    let on: Vec<&Destination> = f.enabled().collect();
    if on.is_empty() {
        return "off: the forward block sets enabled to false".to_owned();
    }
    let parts: Vec<String> = on
        .iter()
        .map(|d| match d.resolve() {
            Ok(r) => {
                let mut s = format!(
                    "{} at {} ({}, {} per request)",
                    r.name,
                    crate::doctor::without_credentials(
                        &r.url(mcpd_server::forward::Signal::Traces)
                    ),
                    r.protocol.as_str(),
                    d.batch_size
                );
                if !r.headers.is_empty() {
                    s.push_str(&format!(
                        ", headers {}",
                        r.headers.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>().join(", ")
                    ));
                }
                s
            }
            Err(e) => format!("{}: {e}", d.name()),
        })
        .collect();
    format!("{}; {}", parts.join("; "), spool_text(f.spool_dir.as_deref(), f.spool_max_bytes, true))
}

/// The `relay` line of `mcpd config`.
#[must_use]
pub fn relay_line(
    upstreams: &[String],
    upstream_key: Option<&str>,
    spool_dir: Option<&std::path::Path>,
    spool_max_bytes: Option<u64>,
) -> String {
    let ups: Vec<&String> = upstreams.iter().filter(|u| !u.trim().is_empty()).collect();
    if ups.is_empty() {
        return "off: no --upstream".to_owned();
    }
    format!(
        "{} upstream(s) in order, {}; {}; {}",
        ups.len(),
        ups.iter().map(|u| crate::doctor::without_credentials(u)).collect::<Vec<_>>().join(", "),
        if upstream_key.is_some_and(|k| !k.trim().is_empty()) {
            "upstream key set"
        } else {
            "each sender's key passed on"
        },
        spool_text(
            spool_dir.map(|d| d.join("relay")).as_deref(),
            spool_max_bytes.unwrap_or(RELAY_SPOOL_MAX_BYTES),
            false
        )
    )
}

/// Serve as a relay until a stop signal, then drain.
///
/// # Errors
/// The relay could not be configured or bound.
pub async fn run_relay(
    config: &ServerConfig,
    listen: &str,
    upstreams: &[String],
    upstream_key: Option<&str>,
    spool_max_bytes: Option<u64>,
) -> Result<()> {
    let relay = RelayConfig {
        upstreams: upstreams
            .iter()
            .map(|u| u.trim().to_owned())
            .filter(|u| !u.is_empty())
            .collect(),
        upstream_key: upstream_key.map(str::trim).filter(|k| !k.is_empty()).map(str::to_owned),
        ingest_keys: config.ingest_keys.clone(),
        spool_dir: config.forward.spool_dir.as_ref().map(|d| d.join("relay")),
        spool_max_bytes: spool_max_bytes.unwrap_or(RELAY_SPOOL_MAX_BYTES),
        max_body_bytes: config.max_body_bytes,
        request_timeout_seconds: config.request_timeout_seconds,
        workers: 4,
    };
    let state = Arc::new(RelayState::new(relay).map_err(anyhow::Error::msg)?);
    mcpd_server::relay::spawn(&state);

    let addr: std::net::SocketAddr = listen
        .parse()
        .with_context(|| format!("{listen} is not a valid address; try 0.0.0.0:4317"))?;
    if config.ingest_keys.is_empty() && !addr.ip().is_loopback() {
        tracing::warn!(
            "relaying on {addr} with open ingest. Anyone who can reach this port can send \
             through it. Set --ingest-key (or MCPD_INGEST_KEYS) before exposing it."
        );
    }
    let listener =
        tokio::net::TcpListener::bind(addr).await.with_context(|| format!("binding {addr}"))?;
    tracing::info!("mcpd {} relaying on http://{addr}", mcpd_core::VERSION);
    for (i, u) in state.upstreams.iter().enumerate() {
        tracing::info!("  upstream {}  {}", i + 1, crate::doctor::without_credentials(&u.url));
    }
    tracing::info!(
        "  spool       {}",
        state.spool.dir().map_or_else(|| "in memory".to_owned(), |d| d.display().to_string())
    );

    let stop = state.shutdown.clone();
    axum::serve(listener, mcpd_server::relay::router(Arc::clone(&state)))
        .with_graceful_shutdown(async move {
            crate::shutdown_signal().await;
            stop.cancel();
        })
        .await
        .context("relay failed")?;

    tracing::info!("shutting down");
    let unfinished = state.drain(std::time::Duration::from_secs(5)).await;
    if unfinished > 0 {
        tracing::warn!(tasks = unfinished, "stopped with shipping still in progress");
    }
    if !state.spool.is_empty() {
        tracing::info!(
            entries = state.spool.len(),
            "requests still in the spool are shipped on the next start{}",
            if state.spool.dir().is_none() {
                "; this spool is in memory, so they are lost"
            } else {
                ""
            }
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flags<'a>(endpoint: Option<&'a str>, vendor: Option<&'a str>) -> Flags<'a> {
        Flags {
            endpoint,
            headers: Vec::new(),
            vendor,
            protocol: None,
            spool_dir: None,
            spool_max_bytes: None,
            data: "/data/mcpd.db",
        }
    }

    #[test]
    fn the_spool_sits_beside_the_data_file_unless_told_otherwise() {
        assert_eq!(spool_dir("/data/mcpd.db", None), Some(PathBuf::from("/data/mcpd.spool")));
        assert_eq!(spool_dir(":memory:", None), None);
        assert_eq!(spool_dir("/data/mcpd.db", Some("memory")), None);
        assert_eq!(spool_dir("/data/mcpd.db", Some("/spool")), Some(PathBuf::from("/spool")));
    }

    #[test]
    fn a_flag_destination_replaces_the_file_block() {
        let file = serde_json::json!([{ "vendor": "otlp", "endpoint": "http://a" }, { "vendor": "otlp", "name": "b", "endpoint": "http://b" }]);
        let s = settings(&flags(Some("http://flag"), None), Some(&file)).expect("settings");
        assert_eq!(s.destinations.len(), 1);
        assert_eq!(s.destinations[0].endpoint.as_deref(), Some("http://flag"));
        let s = settings(&flags(None, None), Some(&file)).expect("settings");
        assert_eq!(s.destinations.len(), 2);
    }

    #[test]
    fn a_vendor_flag_with_a_default_endpoint_needs_no_endpoint_flag() {
        let mut f = flags(None, Some("honeycomb"));
        f.headers = vec![("x-honeycomb-team".into(), "k".into())];
        let s = settings(&f, None).expect("settings");
        assert_eq!(s.destinations[0].vendor, Vendor::Honeycomb);
    }

    #[test]
    fn a_protocol_with_nowhere_to_apply_is_refused() {
        let mut f = flags(None, None);
        f.protocol = Some("grpc");
        assert!(settings(&f, None).is_err());
    }
}
