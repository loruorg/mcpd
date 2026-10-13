//! `mcpd eval`: compare two experiments on a running daemon and exit non-zero
//! on a regression, so a CI job can gate a deploy on it.
//!
//! The decision is the daemon's own: the same Welch interval and the same
//! direction per scorer that `GET /v1/api/experiments/compare` reports. This
//! command only prints it and turns it into an exit code: 0 for no
//! regression, 1 for a regression, 2 when the comparison could not be made.

use anyhow::{Context, Result};
use mcpd_core::{ExperimentComparison, ScoreDirection};

/// The `eval` subcommand.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// The experiment to compare against.
    #[arg(long)]
    baseline: String,

    /// The experiment under test.
    #[arg(long)]
    candidate: String,

    /// The running daemon to ask. This is a client: it opens no store.
    #[arg(long, env = "MCPD_ENDPOINT", default_value = "http://127.0.0.1:4317")]
    endpoint: String,

    /// A read key for that daemon. Omit when the daemon has no keys set.
    #[arg(long, env = "MCPD_API_KEY")]
    api_key: Option<String>,

    /// Fail when a scorer has too few observations to decide, as well as on a
    /// regression. Off by default: an undecidable comparison is not a failure.
    #[arg(long)]
    require_decidable: bool,

    /// Print the comparison as JSON instead of a table.
    #[arg(long)]
    json: bool,
}

fn fixed(v: Option<f64>) -> String {
    v.map_or_else(|| "-".to_owned(), |v| format!("{v:.3}"))
}

/// Run `mcpd eval` and exit with its verdict.
///
/// # Errors
/// Never returns on success: the process exits with the verdict. Returns an
/// error only for a failure to ask, which `main` reports.
pub async fn run(args: &Args, project: &str) -> Result<()> {
    let url = format!("{}/v1/api/experiments/compare", args.endpoint.trim_end_matches('/'));
    let mut req = reqwest::Client::new()
        .get(&url)
        .query(&[
            ("project", project),
            ("baseline", args.baseline.as_str()),
            ("candidate", args.candidate.as_str()),
        ])
        .timeout(std::time::Duration::from_secs(30));
    if let Some(k) = &args.api_key {
        req = req.bearer_auth(k);
    }
    let res = match req.send().await.with_context(|| format!("asking {} to compare", args.endpoint))
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("mcpd eval: {e:#}");
            std::process::exit(2);
        }
    };
    let status = res.status();
    let body = res.text().await.unwrap_or_default();
    if !status.is_success() {
        eprintln!("mcpd eval: the daemon answered {status}: {body}");
        std::process::exit(2);
    }
    let c: ExperimentComparison = match serde_json::from_str(&body) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("mcpd eval: the answer is not a comparison: {e}");
            std::process::exit(2);
        }
    };
    if args.json {
        println!("{}", serde_json::to_string_pretty(&c)?);
    } else {
        println!(
            "{:<24} {:>16} {:>16} {:>8} {:>20}  reading",
            "scorer", "baseline", "candidate", "delta", "95% interval"
        );
        for s in &c.scorers {
            let reading = if !s.decidable {
                "too few runs to decide"
            } else if s.regressed {
                "REGRESSED"
            } else if s.direction == ScoreDirection::LowerIsBetter {
                "ok, lower is better"
            } else {
                "ok"
            };
            let interval = match (s.ci_low, s.ci_high) {
                (Some(l), Some(h)) => format!("{l:.3} to {h:.3}"),
                _ => "-".to_owned(),
            };
            println!(
                "{:<24} {:>16} {:>16} {:>8} {:>20}  {reading}",
                s.scorer_id,
                format!("{} n={}", fixed(s.baseline_mean), s.baseline_n),
                format!("{} n={}", fixed(s.candidate_mean), s.candidate_n),
                fixed(s.delta),
                interval,
            );
        }
    }
    let undecided = c.scorers.iter().any(|s| !s.decidable);
    if c.regressed {
        eprintln!("mcpd eval: {} regressed against {}", c.candidate, c.baseline);
        std::process::exit(1);
    }
    if args.require_decidable && undecided {
        eprintln!(
            "mcpd eval: at least one scorer has fewer than twenty observations a side, and \
             --require-decidable is set"
        );
        std::process::exit(1);
    }
    if c.scorers.is_empty() {
        eprintln!(
            "mcpd eval: neither experiment has a score in project {project:?}; tag scores with \
             the experiment id"
        );
        std::process::exit(2);
    }
    Ok(())
}
