//! Cost accounting.
//!
//! mcpd computes cost at ingest and stores the result. That is a deliberate
//! choice: a price book that changes next month must not silently rewrite what
//! last month cost. Every stored [`Cost`] records which price book produced it.
//!
//! Rates are expressed per million tokens, which is how every provider
//! publishes them, so a rate copied off a pricing page can be pasted in without
//! arithmetic.

use crate::{Cost, InputTokenDetails, OutputTokenDetails, TokenUsage};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Per-meter rates for one model, in currency units per million tokens.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelRates {
    /// Ordinary input tokens.
    #[serde(default)]
    pub input: f64,
    /// Ordinary output tokens.
    #[serde(default)]
    pub output: f64,
    /// Tokens read from a prompt cache. Usually far cheaper than `input`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read: Option<f64>,
    /// Tokens written to a prompt cache. Usually dearer than `input`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write: Option<f64>,
    /// Tokens written to a five-minute ephemeral cache, where priced apart.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_5m: Option<f64>,
    /// Tokens written to a one-hour ephemeral cache, where priced apart.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_1h: Option<f64>,
    /// Audio input tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_input: Option<f64>,
    /// Audio output tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_output: Option<f64>,
    /// Image input tokens, where a provider prices them apart from text.
    ///
    /// OpenAI's image models bill image input above text input on the same
    /// model. Most models do not, which is why this is optional: when it is
    /// unset the image share is billed at `input`, as before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_input: Option<f64>,
    /// Image output tokens, where a provider prices them apart from text.
    ///
    /// Google and OpenAI bill image output at ten to forty times the text
    /// output rate on the same model, and `gpt-image-2` sets the text output
    /// rate to zero outright. Falling back to `output` there bills nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_output: Option<f64>,
    /// Reasoning tokens, when a provider bills them apart from output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<f64>,
}

/// A named set of model rates.
///
/// mcpd ships one small built-in book and runs it. Nothing loads another: there
/// is no flag, no config file key, and `AppState::with_price_book` has no caller
/// outside a fork. So an organisation with negotiated rates cannot price its own
/// traffic without building its own binary, and this type being `Deserialize`
/// does not change that. A book is data and always was; what is missing is the
/// door.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceBook {
    /// Identifier recorded on every [`Cost`] this book produces.
    pub id: String,
    /// ISO 4217 currency for every rate in the book.
    #[serde(default = "default_currency")]
    pub currency: String,
    /// The date the rates in this book were last checked, as `YYYY-MM-DD`.
    ///
    /// Rates move and a released binary does not, so a book with no date cannot
    /// be told apart from one that was right two model generations ago. `None`
    /// means the book's author did not say.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub as_of: Option<String>,
    /// Rates by model id, used for every provider that has no table below.
    #[serde(default)]
    pub models: HashMap<String, ModelRates>,
    /// Rates that apply only to one provider, keyed by provider and then by
    /// model id.
    ///
    /// The same model costs different money depending on who serves it, and a
    /// span already records the provider it went to, so the pair is the honest
    /// key. An entry here wins over the `models` entry for the same id, and
    /// `models` is the fallback: for a provider with no table, for a model the
    /// table does not name, and for a span that never said who served it.
    ///
    /// Absent by default, so a book written before this existed loads and
    /// prices exactly as it did.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub providers: HashMap<String, HashMap<String, ModelRates>>,
}

fn default_currency() -> String {
    "USD".to_owned()
}

impl PriceBook {
    /// An empty book. Costs computed against it are always `None`, which is the
    /// honest answer when no rates are known.
    #[must_use]
    pub fn empty(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            currency: default_currency(),
            as_of: None,
            models: HashMap::new(),
            providers: HashMap::new(),
        }
    }

    /// Lowercase every key, so a book an operator wrote by hand is reachable.
    ///
    /// `lookup` lowercases the model it is given, and `models` is public, so a
    /// book carrying `GPT-4o` could never be matched by anything. The failure
    /// was silent: the span simply had no cost, which reads as a model nobody
    /// has priced rather than as a typo in the key.
    ///
    /// Provider names have the same problem twice over, because a provider
    /// arrives on the span in whatever case the producer wrote it.
    pub fn normalise(&mut self) {
        if self.models.keys().any(|k| k.chars().any(char::is_uppercase)) {
            self.models = self.models.drain().map(|(k, v)| (k.to_ascii_lowercase(), v)).collect();
        }
        if self.providers.keys().any(|k| k.chars().any(char::is_uppercase))
            || self.providers.values().any(|m| m.keys().any(|k| k.chars().any(char::is_uppercase)))
        {
            self.providers = self
                .providers
                .drain()
                .map(|(p, models)| {
                    let models =
                        models.into_iter().map(|(k, v)| (k.to_ascii_lowercase(), v)).collect();
                    (p.to_ascii_lowercase(), models)
                })
                .collect();
        }
    }

    /// Look up a model, tolerating the prefixes and suffixes that gateways and
    /// clouds add to an otherwise standard model id.
    ///
    /// This is the provider-blind form. It reads `models` and never the
    /// provider tables, so it answers "what does this book charge for this
    /// model when nobody said who served it". Use
    /// [`PriceBook::lookup_with_provider`] wherever the span names a provider,
    /// which is the case that can find a different rate.
    #[must_use]
    pub fn lookup(&self, model: &str) -> Option<&ModelRates> {
        lookup_in(&self.models, model)
    }

    /// Look up a model as served by one provider, falling back to the
    /// model-only entry.
    ///
    /// The same model id costs different money on different providers, so a
    /// figure billed at one provider's rate and one billed at the fallback are
    /// different claims. [`PriceMatch::provider`] says which of the two
    /// happened, because mcpd prices at ingest and stores the answer: nothing
    /// downstream can work it out again later.
    ///
    /// `provider` is whatever the span carried, which is `gen_ai.system` or
    /// `llm.provider` written the way the producer writes it. `aws.bedrock` and
    /// `bedrock` are one provider spelled two ways, so the whole string is
    /// tried first and then the string with ONE leading vendor segment
    /// stripped. Nothing else is guessed, and the difference matters: the
    /// semantic conventions spell Azure OpenAI `az.ai.openai`, so taking the
    /// LAST dot-separated segment instead would price every Azure span off a
    /// table keyed `openai`. Those are two providers with two rate cards, which
    /// is the whole reason this table exists.
    #[must_use]
    pub fn lookup_with_provider(
        &self,
        model: &str,
        provider: Option<&str>,
    ) -> Option<PriceMatch<'_>> {
        if let Some((name, table)) = provider.and_then(|p| self.provider_table(p)) {
            if let Some(rates) = lookup_in(table, model) {
                return Some(PriceMatch { rates, provider: Some(name) });
            }
        }
        // A provider table that does not name this model is not an answer, so
        // the model-only entry still applies. Declining instead would leave a
        // blank cost for every model an operator had not got round to listing
        // under the provider they route through.
        self.lookup(model).map(|rates| PriceMatch { rates, provider: None })
    }

    /// The rate table for one provider, and the key that matched it.
    fn provider_table(&self, provider: &str) -> Option<(&str, &HashMap<String, ModelRates>)> {
        // Most books have no provider tables, and this runs once per span at
        // ingest, so that case does no work at all.
        if self.providers.is_empty() {
            return None;
        }
        let lower = provider.to_ascii_lowercase();
        // One leading segment, not the last one. `aws.bedrock` is `bedrock`
        // with a vendor prefix; `az.ai.openai` is not `openai` with one.
        let tail = lower.split_once('.').map(|(_, t)| t.to_owned());
        for want in std::iter::once(lower.as_str()).chain(tail.as_deref()) {
            if let Some((k, table)) = self.providers.get_key_value(want) {
                return Some((k.as_str(), table));
            }
            // The smallest matching key, never the first one iteration reaches,
            // for the reason `lookup_in` gives below: HashMap order is seeded
            // per process, and a cost computed at ingest is kept forever.
            let mut matches: Vec<&String> =
                self.providers.keys().filter(|k| k.eq_ignore_ascii_case(want)).collect();
            matches.sort_unstable();
            if let Some(k) = matches.first() {
                if let Some((k, table)) = self.providers.get_key_value(k.as_str()) {
                    return Some((k.as_str(), table));
                }
            }
        }
        None
    }
}

/// Which rates a lookup found, and where they came from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PriceMatch<'a> {
    /// The rates to price with.
    pub rates: &'a ModelRates,
    /// The provider table that supplied them, as the book spells it, or `None`
    /// when the model-only entry was used.
    pub provider: Option<&'a str>,
}

/// The model-resolution ladder, run against one table of rates.
///
/// Tries, in order: the id as given; the id with a vendor prefix stripped
/// (`openai/gpt-4o`, `us.anthropic.claude-x`); and the id with a trailing date
/// version stripped (`gpt-4o-2024-08-06`). Returning `None` is a normal
/// outcome, not an error.
fn lookup_in<'a>(models: &'a HashMap<String, ModelRates>, model: &str) -> Option<&'a ModelRates> {
    let lower = model.to_ascii_lowercase();
    if let Some(r) = models.get(&lower) {
        return Some(r);
    }
    // A book an operator wrote by hand may not be lowercase, and `models` is
    // public so nothing forces it to be. Every lookup below compares against
    // `lower`, so a key like `GPT-4o` could never be matched by anything and
    // the span simply had no cost, which reads as an unpriced model rather
    // than as a typo. The scan runs only when a key actually carries an
    // uppercase character, so a normalised book pays nothing for it.
    if models.keys().any(|k| k.bytes().any(|b| b.is_ascii_uppercase())) {
        // The smallest matching key, not the first one iteration happens to
        // reach. HashMap ordering is seeded per process, so `find` made this
        // non-deterministic: a book holding both `GPT-4o` and `GPT-4O` with
        // different rates resolved to one or the other depending on the
        // process, measured at 13 runs to 7 out of 20. mcpd prices at ingest
        // and stores the result, so that is not a transient wobble: two
        // spans for the same model in one deployment would carry
        // permanently different costs.
        //
        // A book with two keys differing only in case is a mistake either
        // way, and `mcpd doctor` is the place to say so. What this must not
        // do is pick differently each time it is asked.
        let mut matches: Vec<&String> =
            models.keys().filter(|k| k.eq_ignore_ascii_case(&lower)).collect();
        matches.sort_unstable();
        if let Some(k) = matches.first() {
            return models.get(*k);
        }
    }
    // Strip a routing prefix: everything before the last '/' or the first
    // two dot-separated segments used by cloud model ids.
    if let Some((_, rest)) = lower.rsplit_once('/') {
        if let Some(r) = models.get(rest) {
            return Some(r);
        }
    }
    let dotted: Vec<&str> = lower.split('.').collect();
    if dotted.len() > 1 {
        if let Some(r) = models.get(dotted[dotted.len() - 1]) {
            return Some(r);
        }
        let tail = dotted[1..].join(".");
        if let Some(r) = models.get(&tail) {
            return Some(r);
        }
    }
    // Strip a trailing -YYYY-MM-DD or -YYYYMMDD version suffix.
    if let Some(base) = strip_date_suffix(&lower) {
        if let Some(r) = models.get(base) {
            return Some(r);
        }
        if let Some((_, rest)) = base.rsplit_once('/') {
            if let Some(r) = models.get(rest) {
                return Some(r);
            }
        }
    }
    None
}

fn strip_date_suffix(model: &str) -> Option<&str> {
    // A model id arrives from a `gen_ai.request.model` attribute, so it is
    // whatever the caller sent. Every comparison here is on bytes, never on
    // `str` slices: a `str` slice at a fixed offset panics when the offset
    // lands inside a multi-byte character, and `model-ab€cdefg` put one inside
    // the year. The ingest door is open by default, so that was one request
    // from anyone to stop the daemon. A byte slice has no boundary to miss,
    // and the digits and dashes being matched are all ASCII.
    let bytes = model.as_bytes();
    let digits = |run: &[u8]| run.iter().all(u8::is_ascii_digit);
    // -YYYY-MM-DD
    if let Some(at) = bytes.len().checked_sub(11).filter(|at| *at > 0) {
        let t = &bytes[at..];
        if t[0] == b'-'
            && digits(&t[1..5])
            && t[5] == b'-'
            && digits(&t[6..8])
            && t[8] == b'-'
            && digits(&t[9..11])
        {
            // `at` holds an ASCII dash, so it is a character boundary; `get`
            // says so without a second way to panic.
            return model.get(..at);
        }
    }
    // -YYYYMMDD
    if let Some(at) = bytes.len().checked_sub(9).filter(|at| *at > 0) {
        let t = &bytes[at..];
        if t[0] == b'-' && digits(&t[1..]) {
            return model.get(..at);
        }
    }
    None
}

const PER_MILLION: f64 = 1_000_000.0;

fn rate(tokens: Option<u64>, per_million: Option<f64>) -> Option<f64> {
    match (tokens, per_million) {
        (Some(t), Some(r)) => Some(t as f64 * r / PER_MILLION),
        _ => None,
    }
}

/// Compute the cost of one model call, without knowing who served it.
///
/// Returns `None` when the model is unknown to the book or no usage was
/// reported. mcpd never guesses: a blank cost column means "not priced", which
/// the UI shows as such rather than as zero.
///
/// Prefer [`compute_with_provider`] wherever the span names a provider. This
/// form can only ever find the model-only rate, and the same model id costs
/// different money depending on who served it.
#[must_use]
pub fn compute(book: &PriceBook, model: &str, usage: &TokenUsage) -> Option<Cost> {
    compute_with_provider(book, model, None, usage)
}

/// Compute the cost of one model call as served by `provider`.
///
/// A provider-qualified rate wins; the model-only rate is the fallback. The
/// returned [`Cost`] records which of the two was used, in `price_book`.
#[must_use]
pub fn compute_with_provider(
    book: &PriceBook,
    model: &str,
    provider: Option<&str>,
    usage: &TokenUsage,
) -> Option<Cost> {
    let found = book.lookup_with_provider(model, provider)?;
    let rates = found.rates;

    let empty_in = InputTokenDetails::default();
    let empty_out = OutputTokenDetails::default();
    let din = usage.input_details.as_ref().unwrap_or(&empty_in);
    let dout = usage.output_details.as_ref().unwrap_or(&empty_out);

    // Input side. When a provider reported a breakdown, price each meter at its
    // own rate and price only the remainder at the plain input rate; otherwise
    // price the reported total. Double-counting cached tokens is the classic
    // way to overstate a bill, so the breakdown always wins when present.
    // A detail only counts as metered when the book has a rate that prices it
    // apart. Otherwise it stays in the plain share, so the total still
    // reconciles instead of the tokens leaving the bill.
    let image_in = din.image.filter(|_| rates.image_input.is_some());
    let metered_in: u64 = [
        din.cache_read,
        din.cache_write,
        din.cache_write_5m,
        din.cache_write_1h,
        din.audio,
        image_in,
    ]
    .into_iter()
    .flatten()
    .sum();

    // `text` wins when a provider reported it, because that is what it means:
    // the share left after the meters priced separately below. Deriving the
    // plain share from the total instead is wrong, because providers disagree
    // about what the total counts. Anthropic reports input_tokens NET of cache
    // tokens, so `total - metered` underflows to zero and the whole uncached
    // share bills as nothing. That was measured at a 65% under-bill on a real
    // usage shape, silently, with no error and no flag.
    //
    // An unmetered `image` share is added here, because a call reporting 600
    // text and 400 image tokens must be billed for 1000 rather than 600. Any
    // future detail with no meter of its own belongs here too. When the book
    // does price images the share moves to its own meter above, so adding it
    // here as well would bill it twice.
    let unmetered_image_in = if rates.image_input.is_some() { 0 } else { din.image.unwrap_or(0) };
    // Where the producer sent no `text`, the total is read as gross, which is
    // right for a provider that counts cached tokens inside it. When the
    // metered share is larger than the total, the likelier reading is that the
    // total is the net uncached share, which is Anthropic's convention.
    // Flooring the remainder to zero instead billed the whole uncached share as
    // nothing, measured at a 50% under-bill on an Anthropic span carrying 100
    // input tokens and 1000 read from cache.
    //
    // This arm is a judgement, not the proof it reads as. "No total contains a
    // part bigger than itself" holds only for parts that do not overlap, and
    // providers report overlapping slices of one total: a Gemini prompt of 1000
    // tokens can carry 900 served from cache and 800 counted as audio, each a
    // subset of the same 1000 and of each other. That shape takes this arm and
    // bills the whole prompt at the plain rate on top of the 1700 already
    // metered, where flooring to zero billed 1700. Neither is right; this one
    // is further out. An audit measured it at 0.0011675 against 0.0008675 on
    // gemini-2.5-flash. Where both readings are consistent the OTLP receiver
    // decides, from the provider and the attribute names the producer stated.
    let plain_in = match (din.text, usage.input_tokens) {
        (Some(t), _) => Some(t.saturating_add(unmetered_image_in)),
        (None, Some(total)) if total < metered_in => Some(total),
        (None, Some(total)) => Some(total.saturating_sub(metered_in)),
        (None, None) => None,
    };

    let mut input_cost = 0.0f64;
    let mut any_input = false;
    for (tokens, r) in [
        (plain_in, Some(rates.input)),
        (din.cache_read, rates.cache_read.or(Some(rates.input))),
        (din.cache_write, rates.cache_write.or(Some(rates.input))),
        (din.cache_write_5m, rates.cache_write_5m.or(rates.cache_write).or(Some(rates.input))),
        (din.cache_write_1h, rates.cache_write_1h.or(rates.cache_write).or(Some(rates.input))),
        (din.audio, rates.audio_input.or(Some(rates.input))),
        (image_in, rates.image_input),
    ] {
        if let Some(c) = rate(tokens, r) {
            input_cost += c;
            any_input = true;
        }
    }

    // Output side, on the same principle.
    let image_out = dout.image.filter(|_| rates.image_output.is_some());
    let metered_out: u64 =
        [dout.audio, dout.reasoning.filter(|_| rates.reasoning.is_some()), image_out]
            .into_iter()
            .flatten()
            .sum();
    // The same rule, and the image share matters here for the same reason. An
    // audit found this half still dropping image output tokens after the input
    // half was fixed: 400 of 1000 output tokens, 40% of that bill, left it
    // entirely. Two halves of one function disagreeing is how that happens, so
    // they are written identically now.
    let unmetered_image_out =
        if rates.image_output.is_some() { 0 } else { dout.image.unwrap_or(0) };
    // The same three arms as the input half, for the same reasons.
    let plain_out = match (dout.text, usage.output_tokens) {
        (Some(t), _) => Some(t.saturating_add(unmetered_image_out)),
        (None, Some(total)) if total < metered_out => Some(total),
        (None, Some(total)) => Some(total.saturating_sub(metered_out)),
        (None, None) => None,
    };

    let mut output_cost = 0.0f64;
    let mut any_output = false;
    for (tokens, r) in [
        (plain_out, Some(rates.output)),
        (dout.audio, rates.audio_output.or(Some(rates.output))),
        (dout.reasoning, rates.reasoning),
        (image_out, rates.image_output),
    ] {
        if let Some(c) = rate(tokens, r) {
            output_cost += c;
            any_output = true;
        }
    }

    if !any_input && !any_output {
        return None;
    }

    Some(Cost {
        currency: book.currency.clone(),
        input_cost: any_input.then_some(input_cost),
        output_cost: any_output.then_some(output_cost),
        total_cost: Some(input_cost + output_cost),
        // The book id alone cannot say whether a provider-qualified rate or the
        // model-only fallback produced this figure, and the figure is stored
        // rather than recomputed, so that has to travel with it. A book with no
        // provider tables records exactly what it always did.
        price_book: Some(match found.provider {
            Some(p) => format!("{}@{p}", book.id),
            None => book.id.clone(),
        }),
        is_estimated: true,
    })
}

/// Add `b` into `a`, used to roll a trace's cost up from its spans.
///
/// Refuses to mix currencies: adding a EUR figure to a USD one would produce a
/// number that means nothing, so the mismatched value is skipped and reported.
pub fn accumulate(a: &mut Option<Cost>, b: &Cost) -> bool {
    match a {
        None => {
            *a = Some(b.clone());
            true
        }
        Some(acc) => {
            if acc.currency != b.currency {
                return false;
            }
            fn add(x: &mut Option<f64>, y: Option<f64>) {
                if let Some(v) = y {
                    *x = Some(x.unwrap_or(0.0) + v);
                }
            }
            add(&mut acc.input_cost, b.input_cost);
            add(&mut acc.output_cost, b.output_cost);
            add(&mut acc.total_cost, b.total_cost);
            if acc.price_book != b.price_book {
                // Mixed books in one trace: the figure is still a valid sum, but
                // it no longer belongs to a single book.
                acc.price_book = None;
            }
            true
        }
    }
}

/// The rates mcpd ships with, so `docker run` produces real cost figures with
/// no configuration.
///
/// Deliberately small. It covers the three vendors whose models agent
/// frameworks emit most, at their first-party list prices, and nothing else. An
/// operator who cares about accuracy supplies their own book. The book carries
/// an `as_of` date because rates move and a released binary does not: without
/// it, a rate that is right and a rate that is two model generations stale look
/// the same.
///
/// Two rules for editing this table. Write a published rate as a literal rather
/// than deriving it, so it can be read straight off a pricing page. And never
/// add a model id a provider does not serve: an id that resolves to another
/// model's rates reads as coverage and bills as a guess.
///
/// The `as_of` stamp below records a check against a third-party model
/// catalogue, not against each vendor's own pricing page. Re-check it at the
/// vendors before a release and move the date then.
#[must_use]
pub fn builtin_book() -> PriceBook {
    let mut models = HashMap::new();
    let mut add = |name: &str, rates: ModelRates| {
        models.insert(name.to_owned(), rates);
    };
    // Anthropic publishes three input rates per model: base, a cache read at
    // 10% of base, and two cache writes, five-minute at 1.25x base and one-hour
    // at 2x. `compute` falls back from the one-hour rate to the five-minute one,
    // so omitting `cache_write_1h` does not fail, it under-bills by 37.5% in
    // silence. Both TTLs are listed on every entry for that reason.
    add(
        "claude-opus-4",
        ModelRates {
            input: 15.0,
            output: 75.0,
            cache_read: Some(1.50),
            cache_write: Some(18.75),
            cache_write_1h: Some(30.0),
            ..Default::default()
        },
    );
    add(
        "claude-opus-4-1",
        ModelRates {
            input: 15.0,
            output: 75.0,
            cache_read: Some(1.50),
            cache_write: Some(18.75),
            cache_write_1h: Some(30.0),
            ..Default::default()
        },
    );
    add(
        "claude-opus-4-5",
        ModelRates {
            input: 5.0,
            output: 25.0,
            cache_read: Some(0.50),
            cache_write: Some(6.25),
            cache_write_1h: Some(10.0),
            ..Default::default()
        },
    );
    add(
        "claude-sonnet-4",
        ModelRates {
            input: 3.0,
            output: 15.0,
            cache_read: Some(0.30),
            cache_write: Some(3.75),
            cache_write_1h: Some(6.0),
            ..Default::default()
        },
    );
    add(
        "claude-sonnet-4-5",
        ModelRates {
            input: 3.0,
            output: 15.0,
            cache_read: Some(0.30),
            cache_write: Some(3.75),
            cache_write_1h: Some(6.0),
            ..Default::default()
        },
    );
    // There was never a `claude-haiku-4`. The id sat here at Claude 3.5 Haiku's
    // rates, so the book looked like it covered a generation it did not, while
    // the id Anthropic serves resolved to nothing at all.
    add(
        "claude-haiku-4-5",
        ModelRates {
            input: 1.0,
            output: 5.0,
            cache_read: Some(0.10),
            cache_write: Some(1.25),
            cache_write_1h: Some(2.0),
            ..Default::default()
        },
    );
    add(
        "claude-3-5-haiku",
        ModelRates {
            input: 0.80,
            output: 4.0,
            cache_read: Some(0.08),
            cache_write: Some(1.0),
            cache_write_1h: Some(1.60),
            ..Default::default()
        },
    );
    // OpenAI charges nothing to write to its prompt cache, so there is no cache
    // write rate to record. A cache write reported against one of these models
    // therefore bills at the plain input rate, which is what it costs.
    let openai = |input: f64, output: f64, cache_read: f64| ModelRates {
        input,
        output,
        cache_read: Some(cache_read),
        ..Default::default()
    };
    add("gpt-4o", openai(2.50, 10.0, 1.25));
    add("gpt-4o-mini", openai(0.15, 0.60, 0.075));
    add("gpt-4.1", openai(2.0, 8.0, 0.50));
    add("gpt-4.1-mini", openai(0.40, 1.60, 0.10));
    add("o3", openai(2.0, 8.0, 0.50));
    add("o4-mini", openai(1.10, 4.40, 0.275));
    // Google. `cache_read` here is the implicit cache rate, 25% of base input,
    // which is what a caller gets without asking. Google's explicit context
    // cache is cheaper again, 10% of base, and `InputTokenDetails.cache_read`
    // is one field that cannot say which of the two happened. The implicit rate
    // is the conservative choice: it is what an uninstructed caller pays.
    add(
        "gemini-2.5-pro",
        ModelRates { input: 1.25, output: 10.0, cache_read: Some(0.31), ..Default::default() },
    );
    // Gemini 2.5 Flash prices audio input above text, at 1.00 against 0.30.
    // Gemini 2.5 Pro does not: audio there follows the text rate, which is why
    // only this entry carries `audio_input`.
    add(
        "gemini-2.5-flash",
        ModelRates {
            input: 0.30,
            output: 2.50,
            cache_read: Some(0.075),
            audio_input: Some(1.0),
            ..Default::default()
        },
    );
    // Embeddings produce no output tokens, so the output rate is a real zero
    // rather than an unknown.
    add("text-embedding-3-small", ModelRates { input: 0.02, output: 0.0, ..Default::default() });
    add("text-embedding-3-large", ModelRates { input: 0.13, output: 0.0, ..Default::default() });

    PriceBook {
        id: "builtin".to_owned(),
        currency: "USD".to_owned(),
        as_of: Some("2026-08-28".to_owned()),
        models,
        // No provider-qualified rates. Every model above is published at the
        // same rate by each provider that serves it, at the date stamped
        // beside them, so a provider table here would only restate the entry
        // below it. An entry that repeats the fallback is worse than no entry:
        // it reads as a checked provider-specific rate and is not one. The
        // mechanism is in `lookup_with_provider` for the operator whose rates
        // do differ, and for the day a vendor's do.
        providers: HashMap::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(input: u64, output: u64) -> TokenUsage {
        TokenUsage {
            input_tokens: Some(input),
            output_tokens: Some(output),
            total_tokens: None,
            input_details: None,
            output_details: None,
        }
    }

    #[test]
    fn prices_a_plain_call() {
        let b = builtin_book();
        let c = compute(&b, "claude-sonnet-4", &usage(1_000_000, 1_000_000)).unwrap();
        assert_eq!(c.input_cost, Some(3.0));
        assert_eq!(c.output_cost, Some(15.0));
        assert_eq!(c.total_cost, Some(18.0));
        assert_eq!(c.price_book.as_deref(), Some("builtin"));
        assert!(c.is_estimated);
    }

    #[test]
    fn unknown_model_is_none_not_zero() {
        // The difference matters: zero is a claim, None is an admission.
        let b = builtin_book();
        assert!(compute(&b, "totally-made-up", &usage(100, 100)).is_none());
    }

    #[test]
    fn cached_tokens_are_not_double_counted() {
        let b = builtin_book();
        let u = TokenUsage {
            input_tokens: Some(1_000_000),
            output_tokens: Some(0),
            total_tokens: None,
            input_details: Some(InputTokenDetails {
                cache_read: Some(900_000),
                ..Default::default()
            }),
            output_details: None,
        };
        let c = compute(&b, "claude-sonnet-4", &u).unwrap();
        // 100k at 3.00/M plus 900k at 0.30/M = 0.30 + 0.27
        assert!((c.input_cost.unwrap() - 0.57).abs() < 1e-9, "got {:?}", c.input_cost);
    }

    #[test]
    fn cache_writes_are_charged_at_the_premium_rate() {
        let b = builtin_book();
        let u = TokenUsage {
            input_tokens: Some(1_000_000),
            output_tokens: Some(0),
            total_tokens: None,
            input_details: Some(InputTokenDetails {
                cache_write: Some(1_000_000),
                text: Some(0),
                ..Default::default()
            }),
            output_details: None,
        };
        let c = compute(&b, "claude-sonnet-4", &u).unwrap();
        assert!((c.input_cost.unwrap() - 3.75).abs() < 1e-9);
    }

    #[test]
    fn resolves_gateway_and_cloud_prefixes() {
        let b = builtin_book();
        for m in [
            "openai/gpt-4o",
            "us.anthropic.claude-sonnet-4",
            "gpt-4o-2024-08-06",
            "openai/gpt-4o-2024-08-06",
            "GPT-4o",
        ] {
            assert!(compute(&b, m, &usage(1000, 1000)).is_some(), "failed to resolve {m}");
        }
    }

    #[test]
    fn accumulate_sums_and_refuses_mixed_currencies() {
        let mut acc = None;
        let usd = Cost {
            currency: "USD".into(),
            input_cost: Some(1.0),
            output_cost: Some(2.0),
            total_cost: Some(3.0),
            price_book: Some("builtin".into()),
            is_estimated: true,
        };
        assert!(accumulate(&mut acc, &usd));
        assert!(accumulate(&mut acc, &usd));
        assert_eq!(acc.as_ref().unwrap().total_cost, Some(6.0));

        let eur = Cost { currency: "EUR".into(), ..usd.clone() };
        assert!(!accumulate(&mut acc, &eur), "must refuse to add EUR to USD");
        assert_eq!(acc.as_ref().unwrap().total_cost, Some(6.0));
    }

    #[test]
    fn mixed_price_books_clear_the_attribution() {
        let mut acc = None;
        let a = Cost {
            currency: "USD".into(),
            input_cost: Some(1.0),
            output_cost: None,
            total_cost: Some(1.0),
            price_book: Some("builtin".into()),
            is_estimated: true,
        };
        let b = Cost { price_book: Some("negotiated".into()), ..a.clone() };
        accumulate(&mut acc, &a);
        accumulate(&mut acc, &b);
        assert!(acc.unwrap().price_book.is_none());
    }

    #[test]
    fn empty_book_prices_nothing() {
        let b = PriceBook::empty("none");
        assert!(compute(&b, "gpt-4o", &usage(10, 10)).is_none());
    }
}

#[cfg(test)]
mod hostile_model_tests {
    use super::*;

    /// A model id comes off the wire, so it is whatever the caller sent.
    /// Slicing it at a fixed byte offset panicked and unwound the whole batch.
    #[test]
    fn a_non_ascii_model_id_is_priced_or_declined_but_never_panics() {
        let book = builtin_book();
        let usage = TokenUsage {
            input_tokens: Some(10),
            output_tokens: Some(10),
            total_tokens: None,
            input_details: None,
            output_details: None,
        };
        for id in ["éééééé", "€€€€€€€€€€€€", "gpt-4o-\u{1f600}", "", "-", "模型-2024-01-01"]
        {
            let _ = compute(&book, id, &usage);
        }
    }

    /// The shape the audit found: a three-byte character placed so that a
    /// fixed offset of the eleven-byte date tail falls inside it. The list
    /// after it puts a multi-byte character across every offset the two
    /// suffix forms read, so a fix that moved the panic to another offset
    /// still fails here.
    #[test]
    fn a_multi_byte_character_across_a_date_offset_does_not_panic() {
        let usage = TokenUsage {
            input_tokens: Some(10),
            output_tokens: Some(10),
            total_tokens: None,
            input_details: None,
            output_details: None,
        };
        let book = builtin_book();
        let mut ids: Vec<String> = vec![
            "model-ab€cdefg".to_owned(),
            "gpt-4o-20€4-08-06".to_owned(),
            "gpt-4o-2024-0€-06".to_owned(),
            "gpt-4o-2024-08-0€".to_owned(),
            "gpt-4o-2024€0806".to_owned(),
            "x-ü2024-08-06".to_owned(),
            "ü-2024-08-06".to_owned(),
            "ü-20240806".to_owned(),
            "\u{1f600}\u{1f600}\u{1f600}".to_owned(),
        ];
        // Every position of a two, three and four byte character inside the
        // last eleven bytes of an ASCII id.
        for ch in ['é', '€', '\u{1f600}'] {
            for at in 0..12 {
                let mut id = "gpt-4o-2024-08-06".to_owned();
                id.insert(id.len() - at, ch);
                ids.push(id);
            }
        }
        for id in &ids {
            assert!(strip_date_suffix(id).is_none_or(|base| id.starts_with(base)), "{id}");
            let _ = compute(&book, id, &usage);
        }
        // The fix still strips a real suffix, including after a non-ASCII base.
        assert_eq!(strip_date_suffix("模型-2024-01-01"), Some("模型"));
        assert_eq!(strip_date_suffix("模型-20240101"), Some("模型"));
        assert_eq!(strip_date_suffix("gpt-4o-2024-08-06"), Some("gpt-4o"));
        assert!(compute(&book, "gpt-4o-2024-08-06", &usage).is_some());
        // A bare suffix with nothing before it is not a model with a date.
        assert_eq!(strip_date_suffix("-2024-08-06"), None);
        assert_eq!(strip_date_suffix("-20240806"), None);
    }
}
