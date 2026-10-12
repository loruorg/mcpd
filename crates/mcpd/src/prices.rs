//! `mcpd prices`: write a price book from a model catalogue, and show what a
//! book charges for a model.
//!
//! `refresh` is how the built-in book is made (`make prices`) and how an
//! operator takes new rates without a new release: write a book, point
//! `--price-book` at it, restart. `show` answers "what will this span cost"
//! before any traffic arrives, with the same lookup ingest uses.

use anyhow::{Context, Result};
use mcpd_core::cost::{price_call, Call, PriceBook};
use mcpd_core::price_catalogue::{self, DEFAULT_SOURCE};
use mcpd_core::{InputTokenDetails, TokenUsage};
use std::path::{Path, PathBuf};

/// The `prices` subcommand.
#[derive(Debug, clap::Args)]
pub struct Args {
    #[command(subcommand)]
    pub action: Action,
}

/// What `mcpd prices` does.
#[derive(Debug, clap::Subcommand)]
pub enum Action {
    /// Write a price book from a model catalogue.
    ///
    /// Models the book at --out already prices and the catalogue no longer
    /// lists are kept, because traffic to a retired model goes on arriving.
    /// Pass --prune to drop them.
    Refresh {
        /// The catalogue: an https URL or a file.
        #[arg(long, default_value = DEFAULT_SOURCE)]
        source: String,
        /// Where to write the book. Read first, when it exists, so its models
        /// survive the refresh.
        #[arg(long)]
        out: PathBuf,
        /// Drop models the catalogue no longer lists.
        #[arg(long)]
        prune: bool,
        /// The date to stamp on the book, as YYYY-MM-DD. Today when omitted.
        #[arg(long)]
        as_of: Option<String>,
    },
    /// Show the rates a model is priced at, and what a sample call costs.
    Show {
        /// The model id, as a span records it.
        model: String,
        /// The provider, as a span records it (`gen_ai.system`).
        #[arg(long)]
        provider: Option<String>,
        /// A service tier such as batch or flex.
        #[arg(long)]
        service_tier: Option<String>,
        /// Prompt tokens for the sample call.
        #[arg(long, default_value_t = 1_000_000)]
        input_tokens: u64,
        /// Output tokens for the sample call.
        #[arg(long, default_value_t = 1_000_000)]
        output_tokens: u64,
    },
}

/// Read a book from `path`, laid over the built-in one when it says
/// `"extends": "builtin"`.
///
/// # Errors
/// Names the file and what is wrong with it.
pub fn load(path: &Path) -> Result<PriceBook> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading the price book at {}", path.display()))?;
    let mut book = PriceBook::from_json(&text)
        .map_err(|e| anyhow::anyhow!("{} is not a usable price book: {e}", path.display()))?;
    if book.extends.as_deref() == Some("builtin") {
        book.extend(mcpd_core::cost::builtin_book());
    }
    Ok(book)
}

/// The `prices` line of `mcpd config`.
#[must_use]
pub fn describe(book: &PriceBook, path: Option<&str>) -> String {
    let entries = book.models.len() + book.providers.values().map(|t| t.len()).sum::<usize>();
    let mut line = match path {
        Some(p) => format!("{} from {p}", book.id),
        None => book.id.clone(),
    };
    if let Some(base) = &book.extends {
        line.push_str(&format!(" over {base}"));
    }
    line.push_str(&format!(", {entries} rates in {}", book.currency));
    if let Some(d) = &book.as_of {
        line.push_str(&format!(", as of {d}"));
    }
    line
}

async fn fetch(source: &str) -> Result<String> {
    if source.starts_with("https://") || source.starts_with("http://") {
        let res = reqwest::Client::new()
            .get(source)
            .timeout(std::time::Duration::from_secs(120))
            .send()
            .await
            .with_context(|| format!("fetching {source}"))?;
        let status = res.status();
        if !status.is_success() {
            anyhow::bail!("{source} answered {status}; pass --source with a file to work offline");
        }
        return res.text().await.with_context(|| format!("reading {source}"));
    }
    std::fs::read_to_string(source).with_context(|| format!("reading the catalogue at {source}"))
}

/// Run `mcpd prices`.
///
/// # Errors
/// Any failure to read the catalogue or the book, or to write the result.
pub async fn run(args: &Args, book: &PriceBook) -> Result<()> {
    match &args.action {
        Action::Refresh { source, out, prune, as_of } => {
            let text = fetch(source).await?;
            let catalogue: serde_json::Value =
                serde_json::from_str(&text).with_context(|| format!("{source} is not JSON"))?;
            let previous = if out.exists() {
                let text = std::fs::read_to_string(out)
                    .with_context(|| format!("reading {}", out.display()))?;
                Some(
                    PriceBook::from_json(&text)
                        .map_err(|e| anyhow::anyhow!("{}: {e}", out.display()))?,
                )
            } else {
                None
            };
            let date = match as_of {
                Some(d) => d.clone(),
                None => time::OffsetDateTime::now_utc().date().to_string(),
            };
            let (fresh, summary) = price_catalogue::from_catalogue(
                &catalogue,
                previous.as_ref(),
                *prune,
                &date,
                source,
            )
            .map_err(|e| anyhow::anyhow!(e))?;
            fresh.check().map_err(|e| anyhow::anyhow!("the refreshed book does not check: {e}"))?;
            std::fs::write(out, price_catalogue::to_json(&fresh))
                .with_context(|| format!("writing {}", out.display()))?;
            println!(
                "wrote {} models and {} provider rates in {} provider tables to {}{}",
                summary.models,
                summary.provider_entries,
                summary.providers,
                out.display(),
                if summary.kept > 0 {
                    format!(", keeping {} the catalogue no longer lists", summary.kept)
                } else {
                    String::new()
                }
            );
            Ok(())
        }
        Action::Show { model, provider, service_tier, input_tokens, output_tokens } => {
            let Some(found) = book.lookup_with_provider(model, provider.as_deref()) else {
                anyhow::bail!(
                    "{} prices nothing for {model}{}; spans for it are stored with no cost. Add \
                     it to a book and load that with --price-book",
                    book.id,
                    provider.as_deref().map(|p| format!(" from {p}")).unwrap_or_default()
                );
            };
            println!(
                "table    {}",
                found.provider.map_or_else(|| "models".to_owned(), |p| format!("providers.{p}"))
            );
            println!("rates    {}", serde_json::to_string(found.rates)?);
            let usage = TokenUsage {
                input_tokens: Some(*input_tokens),
                output_tokens: Some(*output_tokens),
                total_tokens: None,
                input_details: Some(InputTokenDetails::default()),
                output_details: None,
            };
            let call = Call {
                model,
                provider: provider.as_deref(),
                usage: &usage,
                service_tier: service_tier.as_deref(),
            };
            if let Some(cost) = price_call(book, &call) {
                println!(
                    "sample   {input_tokens} in, {output_tokens} out: {:.6} {} ({})",
                    cost.total_cost.unwrap_or_default(),
                    cost.currency,
                    cost.price_book.unwrap_or_default()
                );
            }
            Ok(())
        }
    }
}
