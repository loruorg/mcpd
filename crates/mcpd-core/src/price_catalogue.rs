//! Building a price book from a public model catalogue.
//!
//! Rates move every few weeks and a released binary does not, so the built-in
//! book is data, written by `mcpd prices refresh` from a catalogue that lists
//! every provider's rates in one JSON document. The same command writes a book
//! an operator loads with `--price-book`, so a deployment can take new rates
//! without a new release.
//!
//! What this module decides, and the catalogue does not:
//!
//! - which catalogue providers are first-party (their rates fill `models`, the
//!   table used when a span names no provider) and which are clouds or
//!   gateways (their rates go in a provider table, keyed the way spans spell
//!   the provider);
//! - the other spellings of each provider;
//! - the batch and flex discounts, which no catalogue lists per model;
//! - Anthropic's one-hour cache write rate, which is twice the input rate and
//!   which the catalogue leaves out.
//!
//! A model the previous book priced and the catalogue no longer lists is kept,
//! because traffic to a retired model goes on arriving long after the
//! catalogue drops it. `prune` drops them.

use crate::cost::{ModelRates, PriceBook, RateTier};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

/// The catalogue `mcpd prices refresh` reads when no `--source` is given.
pub const DEFAULT_SOURCE: &str = "https://models.dev/api.json";

/// Catalogue providers whose rates are the vendor's own list prices, in the
/// order that settles a model id two of them list.
const FIRST_PARTY: &[&str] = &[
    "openai",
    "anthropic",
    "google",
    "mistral",
    "xai",
    "deepseek",
    "cohere",
    "perplexity",
    "moonshotai",
    "zai",
    "alibaba",
    "minimax",
];

/// Catalogue providers that resell other vendors' models, and the key their
/// table takes in the book. The key is the provider as OpenTelemetry's
/// `gen_ai.system` spells it, with the vendor segment the lookup strips.
const TABLES: &[(&str, &str)] = &[
    ("amazon-bedrock", "bedrock"),
    ("google-vertex", "vertex_ai"),
    ("google-vertex-anthropic", "vertex_ai"),
    ("azure", "az.ai.openai"),
    ("groq", "groq"),
    ("togetherai", "together"),
    ("fireworks-ai", "fireworks"),
    ("deepinfra", "deepinfra"),
    ("cerebras", "cerebras"),
    ("openrouter", "openrouter"),
    ("vercel", "vercel"),
];

/// Other spellings producers use, mapped to the book's key.
const ALIASES: &[(&str, &str)] = &[
    ("azure", "az.ai.openai"),
    ("azure.ai.openai", "az.ai.openai"),
    ("azure_openai", "az.ai.openai"),
    ("az.ai.inference", "az.ai.openai"),
    ("amazon-bedrock", "bedrock"),
    ("aws_bedrock", "bedrock"),
    ("vertex", "vertex_ai"),
    ("vertexai", "vertex_ai"),
    ("google-vertex", "vertex_ai"),
    ("togetherai", "together"),
    ("together_ai", "together"),
    ("fireworks_ai", "fireworks"),
    ("fireworks-ai", "fireworks"),
    ("gemini", "google"),
    ("google_genai", "google"),
    ("mistral_ai", "mistral"),
    ("mistralai", "mistral"),
    ("x_ai", "xai"),
    ("vercel_ai_gateway", "vercel"),
];

/// Service-tier discounts each provider publishes as a flat fraction of the
/// standard rate.
const MULTIPLIERS: &[(&str, &str, f64)] = &[
    ("openai", "batch", 0.5),
    ("openai", "flex", 0.5),
    ("az.ai.openai", "batch", 0.5),
    ("anthropic", "batch", 0.5),
    ("google", "batch", 0.5),
    ("vertex_ai", "batch", 0.5),
    ("bedrock", "batch", 0.5),
    ("mistral", "batch", 0.5),
];

/// What a refresh did, for the line the command prints.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Summary {
    /// Entries in `models`.
    pub models: usize,
    /// Provider tables.
    pub providers: usize,
    /// Entries across every provider table.
    pub provider_entries: usize,
    /// Entries kept from the previous book because the catalogue no longer
    /// lists them.
    pub kept: usize,
}

fn num(v: &Value, key: &str) -> Option<f64> {
    v.get(key).and_then(Value::as_f64).filter(|n| n.is_finite() && *n >= 0.0)
}

/// Rates for one catalogue `cost` object, without tiers.
fn rates_of(cost: &Value) -> Option<ModelRates> {
    let input = num(cost, "input")?;
    let output = num(cost, "output")?;
    Some(ModelRates {
        input,
        output,
        cache_read: num(cost, "cache_read"),
        cache_write: num(cost, "cache_write"),
        audio_input: num(cost, "input_audio"),
        audio_output: num(cost, "output_audio"),
        reasoning: num(cost, "reasoning"),
        ..ModelRates::default()
    })
}

/// One catalogue model as book rates, or `None` when it carries no usable
/// price. A model listed at zero for both meters is left out: the catalogue
/// uses zero for "free on this route" and for "not known", and mcpd reports an
/// unknown model as unpriced, never as free.
fn convert(id: &str, model: &Value) -> Option<ModelRates> {
    let cost = model.get("cost")?;
    let mut rates = rates_of(cost)?;
    if rates.input == 0.0 && rates.output == 0.0 {
        return None;
    }
    let mut tiers: Vec<RateTier> = Vec::new();
    if let Some(list) = cost.get("tiers").and_then(Value::as_array) {
        for t in list {
            let Some(spec) = t.get("tier") else { continue };
            if spec.get("type").and_then(Value::as_str) != Some("context") {
                continue;
            }
            let Some(size) = spec.get("size").and_then(Value::as_u64) else { continue };
            let Some(r) = rates_of(t) else { continue };
            // `200001` and `200000` state one boundary two ways: "from 200 001"
            // and "above 200 000".
            let above = if size % 1000 == 1 { size - 1 } else { size };
            tiers.push(RateTier { above_input_tokens: above, rates: r });
        }
    }
    if tiers.is_empty() {
        if let Some(r) = cost.get("context_over_200k").and_then(rates_of) {
            tiers.push(RateTier { above_input_tokens: 200_000, rates: r });
        }
    }
    tiers.sort_by_key(|t| t.above_input_tokens);
    tiers.dedup_by_key(|t| t.above_input_tokens);
    rates.tiers = tiers;
    // Anthropic prices a one-hour cache write at twice the input rate and a
    // five-minute one at 1.25 times. The catalogue's single write rate is the
    // five-minute one, and leaving the hour out would bill an hour-long write
    // at the shorter rate.
    if id.contains("claude") {
        if let Some(w) = rates.cache_write {
            rates.cache_write_5m = Some(w);
            rates.cache_write_1h = Some(round(rates.input * 2.0));
            for t in &mut rates.tiers {
                if t.rates.cache_write.is_some() {
                    t.rates.cache_write_1h = Some(round(t.rates.input * 2.0));
                }
            }
        }
    }
    let limit = model.get("limit");
    rates.context_window = limit.and_then(|l| l.get("context")).and_then(Value::as_u64);
    rates.max_output = limit.and_then(|l| l.get("output")).and_then(Value::as_u64);
    Some(rates)
}

/// Six decimal places, so a derived rate prints as the number a pricing page
/// shows and not as `6.000000000000001`.
fn round(x: f64) -> f64 {
    (x * 1_000_000.0).round() / 1_000_000.0
}

fn models_of(catalogue: &Value, provider: &str) -> Vec<(String, ModelRates)> {
    let Some(models) = catalogue.get(provider).and_then(|p| p.get("models")) else {
        return Vec::new();
    };
    let Some(map) = models.as_object() else { return Vec::new() };
    let mut out: Vec<(String, ModelRates)> = map
        .iter()
        .filter_map(|(id, m)| convert(id, m).map(|r| (id.to_ascii_lowercase(), r)))
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Build a book from a catalogue document.
///
/// `previous` is the book being replaced; its entries survive when the
/// catalogue no longer lists them, unless `prune` is set.
///
/// # Errors
/// Returns a message when the document is not a catalogue: no first-party
/// provider in it carries a single priced model.
pub fn from_catalogue(
    catalogue: &Value,
    previous: Option<&PriceBook>,
    prune: bool,
    as_of: &str,
    source: &str,
) -> Result<(PriceBook, Summary), String> {
    let mut models: HashMap<String, ModelRates> = HashMap::new();
    for provider in FIRST_PARTY {
        for (id, rates) in models_of(catalogue, provider) {
            models.entry(id).or_insert(rates);
        }
    }
    if models.is_empty() {
        return Err(format!(
            "{source} holds no priced model for any of {}; it is not a model catalogue in the \
             shape mcpd reads",
            FIRST_PARTY.join(", ")
        ));
    }
    let mut providers: HashMap<String, HashMap<String, ModelRates>> = HashMap::new();
    for (provider, key) in TABLES {
        let table = providers.entry((*key).to_owned()).or_default();
        for (id, rates) in models_of(catalogue, provider) {
            table.entry(id).or_insert(rates);
        }
    }
    providers.retain(|_, t| !t.is_empty());

    let mut summary = Summary::default();
    if let (Some(prev), false) = (previous, prune) {
        for (id, rates) in &prev.models {
            if !models.contains_key(id) {
                models.insert(id.clone(), rates.clone());
                summary.kept += 1;
            }
        }
        for (p, table) in &prev.providers {
            let mine = providers.entry(p.clone()).or_default();
            for (id, rates) in table {
                if !mine.contains_key(id) {
                    mine.insert(id.clone(), rates.clone());
                    summary.kept += 1;
                }
            }
        }
    }

    let mut multipliers: HashMap<String, HashMap<String, f64>> = HashMap::new();
    for (p, tier, m) in MULTIPLIERS {
        multipliers.entry((*p).to_owned()).or_default().insert((*tier).to_owned(), *m);
    }
    let book = PriceBook {
        id: "builtin".to_owned(),
        currency: "USD".to_owned(),
        as_of: Some(as_of.to_owned()),
        models,
        providers,
        provider_aliases: ALIASES.iter().map(|(a, b)| ((*a).to_owned(), (*b).to_owned())).collect(),
        service_tier_multipliers: multipliers,
        extends: None,
        source: Some(source.to_owned()),
    };
    summary.models = book.models.len();
    summary.providers = book.providers.len();
    summary.provider_entries = book.providers.values().map(HashMap::len).sum();
    Ok((book, summary))
}

/// A book as JSON, one model to a line and every key sorted, so a refresh
/// reads as a diff of the rates that moved.
#[must_use]
pub fn to_json(book: &PriceBook) -> String {
    fn line(v: &impl serde::Serialize) -> String {
        // A sorted `Value` first: `serde_json` keeps a map's keys in order.
        serde_json::to_value(v).and_then(|v| serde_json::to_string(&v)).unwrap_or_default()
    }
    fn table(out: &mut String, indent: &str, t: &HashMap<String, ModelRates>) {
        let sorted: BTreeMap<&String, &ModelRates> = t.iter().collect();
        let n = sorted.len();
        for (i, (id, rates)) in sorted.into_iter().enumerate() {
            let comma = if i + 1 < n { "," } else { "" };
            out.push_str(&format!("{indent}{}: {}{comma}\n", line(id), line(rates)));
        }
    }
    let mut out = String::from("{\n");
    out.push_str(&format!("  \"id\": {},\n", line(&book.id)));
    out.push_str(&format!("  \"currency\": {},\n", line(&book.currency)));
    if let Some(d) = &book.as_of {
        out.push_str(&format!("  \"as_of\": {},\n", line(d)));
    }
    if let Some(s) = &book.source {
        out.push_str(&format!("  \"source\": {},\n", line(s)));
    }
    if let Some(e) = &book.extends {
        out.push_str(&format!("  \"extends\": {},\n", line(e)));
    }
    let aliases: BTreeMap<&String, &String> = book.provider_aliases.iter().collect();
    out.push_str(&format!("  \"provider_aliases\": {},\n", line(&aliases)));
    let mult: BTreeMap<&String, BTreeMap<&String, &f64>> =
        book.service_tier_multipliers.iter().map(|(p, t)| (p, t.iter().collect())).collect();
    out.push_str(&format!("  \"service_tier_multipliers\": {},\n", line(&mult)));
    out.push_str("  \"models\": {\n");
    table(&mut out, "    ", &book.models);
    out.push_str("  },\n  \"providers\": {\n");
    let providers: BTreeMap<&String, &HashMap<String, ModelRates>> =
        book.providers.iter().collect();
    let n = providers.len();
    for (i, (p, t)) in providers.into_iter().enumerate() {
        out.push_str(&format!("    {}: {{\n", line(p)));
        table(&mut out, "      ", t);
        out.push_str(if i + 1 < n { "    },\n" } else { "    }\n" });
    }
    out.push_str("  }\n}\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cost::{price_call, Call};
    use crate::{InputTokenDetails, TokenUsage};

    fn catalogue() -> Value {
        serde_json::json!({
            "anthropic": { "models": {
                "claude-x-1": { "cost": { "input": 3, "output": 15, "cache_read": 0.3,
                    "cache_write": 3.75, "context_over_200k": { "input": 6, "output": 22.5 } },
                    "limit": { "context": 1000000, "output": 64000 } },
                "free-thing": { "cost": { "input": 0, "output": 0 } }
            }},
            "openai": { "models": {
                "gpt-x": { "cost": { "input": 2, "output": 8, "cache_read": 0.5,
                    "tiers": [ { "input": 4, "output": 16, "tier": { "type": "context", "size": 272001 } } ] } }
            }},
            "amazon-bedrock": { "models": {
                "anthropic.claude-x-1-v1:0": { "cost": { "input": 3.3, "output": 16.5 } }
            }}
        })
    }

    #[test]
    fn first_party_rates_fill_models_and_clouds_get_tables() {
        let (book, s) = from_catalogue(&catalogue(), None, false, "2026-10-12", "test").unwrap();
        assert_eq!(s.models, 2, "{:?}", book.models.keys());
        assert!(!book.models.contains_key("free-thing"), "a zero price is not a price");
        let claude = &book.models["claude-x-1"];
        assert_eq!(claude.cache_write_1h, Some(6.0));
        assert_eq!(claude.cache_write_5m, Some(3.75));
        assert_eq!(claude.context_window, Some(1_000_000));
        assert_eq!(claude.tiers[0].above_input_tokens, 200_000);
        assert_eq!(book.models["gpt-x"].tiers[0].above_input_tokens, 272_000);
        assert!(book.providers["bedrock"].contains_key("anthropic.claude-x-1-v1:0"));
        book.check().unwrap();
    }

    #[test]
    fn a_retired_model_survives_unless_pruned() {
        let mut prev = PriceBook::empty("builtin");
        prev.models.insert(
            "old-model".into(),
            ModelRates { input: 1.0, output: 2.0, ..Default::default() },
        );
        let (kept, s) = from_catalogue(&catalogue(), Some(&prev), false, "d", "t").unwrap();
        assert!(kept.models.contains_key("old-model"));
        assert_eq!(s.kept, 1);
        let (pruned, _) = from_catalogue(&catalogue(), Some(&prev), true, "d", "t").unwrap();
        assert!(!pruned.models.contains_key("old-model"));
    }

    #[test]
    fn a_document_with_no_priced_model_is_refused() {
        let e = from_catalogue(&serde_json::json!({"x": 1}), None, false, "d", "src").unwrap_err();
        assert!(e.contains("src"), "{e}");
    }

    #[test]
    fn the_written_book_reads_back_identically() {
        let (book, _) = from_catalogue(&catalogue(), None, false, "2026-10-12", "t").unwrap();
        let text = to_json(&book);
        let again = PriceBook::from_json(&text).unwrap();
        assert_eq!(to_json(&again), text);
        assert_eq!(again.models, book.models);
    }

    #[test]
    fn a_long_prompt_and_a_batch_call_are_priced_at_their_own_rates() {
        let (book, _) = from_catalogue(&catalogue(), None, false, "d", "t").unwrap();
        let usage = |input: u64| TokenUsage {
            input_tokens: Some(input),
            output_tokens: Some(1_000_000),
            input_details: Some(InputTokenDetails::default()),
            ..Default::default()
        };
        let call = |u: &TokenUsage, tier: Option<&'static str>| {
            let c = Call {
                model: "claude-x-1",
                provider: Some("anthropic"),
                usage: u,
                service_tier: tier,
            };
            price_call(&book, &c).unwrap()
        };
        let short = usage(100_000);
        assert!((call(&short, None).total_cost.unwrap() - (0.3 + 15.0)).abs() < 1e-9);
        let long = usage(300_000);
        let c = call(&long, None);
        assert!((c.total_cost.unwrap() - (1.8 + 22.5)).abs() < 1e-9, "{c:?}");
        let b = call(&short, Some("batch"));
        assert!((b.total_cost.unwrap() - (0.3 + 15.0) / 2.0).abs() < 1e-9);
        assert_eq!(b.price_book.as_deref(), Some("builtin+batch"));
        // A tier the provider does not discount prices at the list rate and
        // says nothing about it.
        let p = call(&short, Some("priority"));
        assert_eq!(p.price_book.as_deref(), Some("builtin"));
        assert_eq!(call(&short, Some("standard")).price_book.as_deref(), Some("builtin"));
    }

    #[test]
    fn a_cloud_spelling_reaches_its_table_through_an_alias() {
        let (book, _) = from_catalogue(&catalogue(), None, false, "d", "t").unwrap();
        let m = book.lookup_with_provider("us.anthropic.claude-x-1-v1:0", Some("aws.bedrock"));
        assert_eq!(m.unwrap().provider, Some("bedrock"));
        assert_eq!(m.unwrap().rates.input, 3.3);
        // Vertex's `@version` and a dotted version both resolve.
        assert!(book.lookup("claude-x-1@20250929").is_some());
    }
}
