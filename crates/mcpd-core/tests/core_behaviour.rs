//! Behaviour of the four pieces every stored signal passes through.
//!
//! Redaction, sampling, cost and identifiers each carry one promise a user
//! relies on, and each of those promises is bigger than any single function:
//!
//! - Redaction is the privacy control, so what it does **not** touch matters as
//!   much as what it does.
//! - Sampling has to agree across a fleet that never coordinates, which is a
//!   claim about other processes and other builds, not about one call.
//! - Cost is a number somebody puts in a budget, so the arithmetic has to hold
//!   at the edges as well as in the middle.
//! - Identifiers have to be OpenTelemetry-shaped, or every downstream tool
//!   rejects them.
//!
//! The unit tests inside `redact.rs`, `sampling.rs`, `cost.rs` and `ids.rs`
//! already cover the ordinary paths: masking by key name, the partial and hash
//! modes, the built-in book's plain arithmetic, gateway prefixes, `traceparent`
//! parsing and the generated id shape. None of that is repeated here. This file
//! is the behaviour those tests leave open, and it is written from outside the
//! crate so it sees only what a caller sees.
//!
//! NOT COVERED HERE, and deliberately: that a cost supplied by the caller
//! survives ingest untouched. `mcpd_core::cost::compute` is a pure function and
//! cannot overwrite anything; the guard is `if span.cost.is_none()` in
//! `crates/mcpd-server/src/pipeline.rs`, so the test belongs beside that crate.

use mcpd_core::cost::{self, ModelRates, PriceBook};
use mcpd_core::ids::{self, TraceParent};
use mcpd_core::redact::{Redactor, Rule};
use mcpd_core::sampling::{Decision, Policy};
use mcpd_core::{InputTokenDetails, OutputTokenDetails, RedactionMode, SamplingKind, TokenUsage};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

// ---------------------------------------------------------------------------
// Redaction: the privacy control.
// ---------------------------------------------------------------------------

#[test]
fn free_text_is_not_scanned_for_secrets_and_that_is_the_documented_boundary() {
    // Redaction matches key names, never values. A key called `content` holding
    // a secret keeps it verbatim. This is asserted from the leaking side on
    // purpose: the day someone adds value scanning, this test fails and the
    // change has to be deliberate rather than accidental.
    let r = Redactor::with_defaults();
    let out = r.redact(&json!({
        "content": "my api_key is sk-live-9999 and the password is hunter2",
        "api_key": "sk-live-9999"
    }));
    assert_eq!(
        out["content"],
        json!("my api_key is sk-live-9999 and the password is hunter2"),
        "free text is matched by key name only, so it is stored as sent",
    );
    assert_eq!(out["api_key"], json!("[redacted]"), "the same value under a matched key is masked");
}

#[test]
fn a_secret_nested_deeper_than_the_depth_cap_is_cut_off_rather_than_leaked() {
    // The depth cap exists to stop a pathological document pinning a CPU. The
    // question nobody had answered is what happens to a secret on the far side
    // of it: a guard that gives up and returns the subtree verbatim would be a
    // privacy hole reachable by anyone who can nest 40 objects.
    let mut v = json!({ "api_key": "sk-live-DEEP", "email": "alice@example.com" });
    for _ in 0..40 {
        v = json!({ "n": v });
    }
    let out = Redactor::with_defaults().redact(&v).to_string();
    assert!(!out.contains("sk-live-DEEP"), "a secret past the depth cap was stored: {out}");
    assert!(!out.contains("alice@example.com"), "an address past the depth cap was stored: {out}");
    assert!(out.contains("too deeply nested"), "the cut should be visible in the output: {out}");
}

#[test]
fn the_depth_cap_falls_between_thirty_two_and_thirty_three_levels() {
    // Pinning where the cut lands, because "deep enough" is not a specification
    // and a reader of a truncated document needs to know what it cost them.
    let nest = |levels: usize| {
        let mut v = json!("leaf");
        for _ in 0..levels {
            v = json!({ "n": v });
        }
        Redactor::with_defaults().redact(&v).to_string()
    };
    assert!(!nest(32).contains("too deeply nested"), "32 levels must survive whole");
    assert!(nest(33).contains("too deeply nested"), "33 levels must be cut");
}

#[test]
fn the_first_matching_rule_wins_so_a_later_rule_cannot_loosen_an_earlier_one() {
    // `with_rule` appends, and matching takes the first hit. A key that matches
    // two rules therefore gets the earlier rule's mode. An operator adding a
    // rule for a key the defaults already cover finds their rule ignored, which
    // is the safe direction: the defaults cannot be weakened from configuration.
    let r = Redactor::with_defaults()
        .with_rule(Rule { key_contains: vec!["secret".into()], mode: RedactionMode::Partial });
    assert_eq!(
        r.redact(&json!({ "client_secret": "sk-live-abcdefgh" }))["client_secret"],
        json!("[redacted]"),
        "the built-in masking rule must beat a partial rule added after it",
    );

    // A key matching two default rules resolves the same way, by declaration
    // order: masking is declared before hashing, so this is masked.
    assert_eq!(
        r.redact(&json!({ "user_id_password": "u-1" }))["user_id_password"],
        json!("[redacted]"),
    );
}

#[test]
fn the_drop_mode_removes_the_key_entirely_including_a_whole_subtree() {
    // Drop is the one mode with no test anywhere: it is reachable only through
    // a configured rule, and it is the mode that changes the shape of the
    // document rather than a value inside it.
    let r = Redactor::disabled()
        .with_rule(Rule { key_contains: vec!["internal".into()], mode: RedactionMode::Drop });
    let out = r.redact(&json!({
        "internal_notes": { "reviewer": "ana", "verdict": "reject" },
        "internal_id": 7,
        "public": "kept"
    }));
    assert_eq!(out, json!({ "public": "kept" }), "dropped keys must be absent, not null");
    assert!(out.get("internal_notes").is_none());
}

#[test]
fn a_runaway_string_is_truncated_to_the_cap_and_says_how_much_it_lost() {
    // The existing unit test proves the cut lands on a character boundary. What
    // it does not check is the number in the marker, which is the only thing
    // telling a reader the value they are looking at is incomplete.
    let r = Redactor::with_defaults().with_max_string(8);
    assert_eq!(
        r.redact(&json!({ "content": "0123456789ab" }))["content"],
        json!("01234567… [4 bytes truncated]"),
    );
    // Multi-byte input: the cap is bytes, so eight bytes is four of these.
    assert_eq!(
        r.redact(&json!({ "content": "ααααααααααα" }))["content"],
        json!("αααα… [14 bytes truncated]"),
    );
    // A matched key is masked before the cap is ever consulted, so a huge
    // secret costs one token of storage rather than the cap.
    assert_eq!(
        r.redact(&json!({ "api_key": "0123456789ab".repeat(500) }))["api_key"],
        json!("[redacted]"),
    );
}

#[test]
fn redaction_reaches_a_secret_at_the_top_level_and_inside_a_bare_array() {
    // Span input and output are arbitrary JSON, not always an object. A walker
    // that only handled objects would pass a bare array through untouched.
    let r = Redactor::with_defaults().with_max_string(8);
    assert_eq!(
        r.redact(&json!([{ "api_key": "sk-live" }, [{ "password": "hunter2" }]])),
        json!([{ "api_key": "[redacted]" }, [{ "password": "[redacted]" }]]),
    );
    assert_eq!(r.redact(&json!("0123456789ab")), json!("01234567… [4 bytes truncated]"));
    assert_eq!(r.redact(&json!(null)), json!(null));
}

#[test]
fn two_keys_differing_only_in_case_are_both_redacted() {
    // JSON allows both spellings in one object and serde_json keeps both. A
    // reader who lowercased once and compared once would redact only one.
    let out = Redactor::with_defaults().redact(&json!({
        "api_key": "a", "API_KEY": "b", "Api_Key": "c"
    }));
    assert_eq!(out["api_key"], json!("[redacted]"));
    assert_eq!(out["API_KEY"], json!("[redacted]"));
    assert_eq!(out["Api_Key"], json!("[redacted]"));
}

#[test]
fn a_non_ascii_key_matches_on_the_ascii_part_and_never_panics() {
    // Keys arrive from the wire, so they can be any valid UTF-8. Lowercasing
    // and substring matching must both survive that.
    let out = Redactor::with_defaults().redact(&json!({
        "clé_secret": "x", "Æmail": "alice@example.com", "🙂": "kept"
    }));
    assert_eq!(out["clé_secret"], json!("[redacted]"), "the ascii substring still matches");
    assert_eq!(out["🙂"], json!("kept"));
}

#[test]
fn redacting_twice_degrades_further_and_never_reveals_more() {
    // Worth pinning because the pipeline is not idempotent and a reader might
    // assume it is. A partial value shortens to nothing on a second pass, and a
    // hash is hashed again, which silently breaks grouping across a replay.
    let r = Redactor::with_defaults();
    let once = r.redact(&json!({ "email": "alice@example.com", "user_id": "u-1" }));
    let twice = r.redact(&once);
    assert_eq!(once["email"], json!("al…om"));
    assert_eq!(twice["email"], json!("[redacted]"), "a second pass reveals less, never more");
    assert_ne!(
        once["user_id"], twice["user_id"],
        "hashing a hash yields a different digest, so a replayed batch loses its grouping",
    );
    assert!(!twice.to_string().contains("alice@example.com"));
}

#[test]
fn an_absent_identifier_stays_absent_and_an_empty_one_is_still_hashed() {
    // The distinction matters downstream: `None` groups as "no user", whereas a
    // digest of the empty string would group every anonymous run together under
    // a value that looks like a real user id.
    let r = Redactor::with_defaults();
    assert_eq!(r.hash_identifier(None), None);
    let empty = r.hash_identifier(Some("")).expect("an empty id is a value, so it is hashed");
    assert!(empty.starts_with("sha256:"));
    assert_ne!(r.hash_identifier(Some("u-1")).unwrap(), empty);
}

#[test]
fn nothing_under_a_masked_key_comes_back_partially_readable() {
    // `walk` resolves the mode as `self.mode_for(k).or(inherited)`, so a key
    // that names a weaker rule beats the mode inherited from its parent. Under
    // a `password` key, an `email` key is therefore partial rather than masked
    // and four characters of the value are stored.
    let r = Redactor::with_defaults();
    let out = r.redact(&json!({ "password": { "email": "alice@example.com" } }));
    assert_eq!(
        out["password"]["email"],
        json!("[redacted]"),
        "a value under a masked key must not be readable at all",
    );
}

// ---------------------------------------------------------------------------
// Sampling: a fleet has to agree without coordinating.
// ---------------------------------------------------------------------------

/// Trace ids with known decisions, including the two shapes the hash treats
/// specially: 32 hex characters, and anything else.
const FLEET_IDS: [&str; 8] = [
    "4bf92f3577b34da6a3ce929d0e0e4736",
    "9c1b0e5a7f2d43886ae1c0d9b4f37521",
    "00f067aa0ba902b700f067aa0ba902b7",
    "ffffffffffffffffffffffffffffffff",
    "00000000000000000000000000000001",
    "deadbeefdeadbeefdeadbeefdeadbeef",
    "not-a-hex-trace-id",
    "",
];

/// What a 25% deterministic policy decides for [`FLEET_IDS`], in order.
///
/// This vector is the fleet agreement written down. Two collectors agree only
/// if they compute the same hash, so changing the hash silently splits a
/// rolling upgrade into two populations that keep different traces. That is a
/// deliberate, breaking change, and it should have to edit this line.
const FLEET_AT_25: &str = "DDKDKDDD";

/// The marker the child process prints, so the parent can find it in libtest's
/// own output.
const FLEET_MARKER: &str = "fleet-decisions:";

fn fleet_decisions(rate: f64) -> String {
    let p = Policy::deterministic(rate);
    FLEET_IDS
        .iter()
        .map(|id| if p.decide(id, None) == Decision::Keep { 'K' } else { 'D' })
        .collect()
}

#[test]
fn the_deterministic_sampler_keeps_a_fixed_published_set_of_traces() {
    assert_eq!(fleet_decisions(0.25), FLEET_AT_25);
}

#[test]
fn a_second_process_reaches_the_identical_decisions() {
    // The in-crate test repeats one decision inside one process, which would
    // still pass if the hash were seeded per process the way `HashMap` is. The
    // claim is about a fleet, so this runs the same code in a second process
    // and compares. `current_exe` is this test binary; the child is filtered to
    // the one test below, so it cannot recurse.
    let exe = std::env::current_exe().expect("the test binary should have a path");
    let out = std::process::Command::new(&exe)
        .args(["--exact", "a_fleet_member_prints_its_decisions", "--nocapture"])
        .output()
        .expect("should be able to run this test binary again");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let line = stdout
        .lines()
        .find_map(|l| l.trim().strip_prefix(FLEET_MARKER))
        .unwrap_or_else(|| panic!("child printed no decisions.\nstdout:\n{stdout}"))
        .trim();
    assert_eq!(line, fleet_decisions(0.25), "two processes must keep the same traces");
    assert_eq!(line, FLEET_AT_25);
}

#[test]
fn a_fleet_member_prints_its_decisions() {
    // Run on its own by the test above, and harmless in an ordinary run.
    println!("{FLEET_MARKER} {}", fleet_decisions(0.25));
    assert_eq!(fleet_decisions(0.25), FLEET_AT_25);
}

#[test]
fn raising_the_rate_only_ever_adds_traces_to_the_kept_set() {
    // Mid-rollout a fleet runs two rates at once. If the kept sets were not
    // nested, a trace could be kept by the old rate and dropped by the new one
    // in the same second, and the trace would end up half-recorded.
    let ids: Vec<String> = (0..400).map(|_| ids::new_trace_id()).collect();
    let mut previous: Vec<&String> = Vec::new();
    for rate in [0.0, 0.1, 0.25, 0.5, 0.75, 1.0] {
        let p = Policy::deterministic(rate);
        let kept: Vec<&String> =
            ids.iter().filter(|i| p.decide(i, None) == Decision::Keep).collect();
        for id in &previous {
            assert!(kept.contains(id), "raising the rate to {rate} dropped {id}");
        }
        previous = kept;
    }
    assert_eq!(previous.len(), ids.len(), "a rate of 1.0 must keep every id");
}

#[test]
fn rate_zero_keeps_nothing_and_rate_one_keeps_everything_over_many_ids() {
    // The unit test checks one id at each extreme. One id cannot tell an exact
    // rule from a rule that is nearly always right.
    let none = Policy::deterministic(0.0);
    let all = Policy::deterministic(1.0);
    for _ in 0..2000 {
        let id = ids::new_trace_id();
        assert_eq!(none.decide(&id, None), Decision::Drop, "{id} survived a zero rate");
        assert_eq!(all.decide(&id, None), Decision::Keep, "{id} was dropped at a full rate");
    }
}

#[test]
fn a_probability_outside_the_range_is_clamped_and_a_nonsense_one_fails_closed() {
    // `probability` is deserialised from an operator's configuration file, so
    // it can be anything. Failing closed is the right direction: dropping data
    // is recoverable, and a panic in the ingest path is not.
    let id = FLEET_IDS[0];
    let with = |probability: f64| {
        Policy { kind: SamplingKind::Deterministic, probability, ..Policy::default() }
            .decide(id, None)
    };
    assert_eq!(with(5.0), Decision::Keep, "above 1.0 must behave as 1.0");
    assert_eq!(with(-3.0), Decision::Drop, "below 0.0 must behave as 0.0");
    assert_eq!(with(f64::NAN), Decision::Drop, "a value that is not a number must not keep data");
    assert_eq!(with(f64::INFINITY), Decision::Keep);
}

#[test]
fn a_policy_that_omits_its_optional_fields_keeps_everything() {
    // The dangerous default is the opposite one. If `probability` defaulted to
    // zero, a configuration naming only a kind would silently drop every trace
    // in the project, and the symptom would be an empty UI with no error.
    let p: Policy = serde_json::from_str(r#"{"kind":"deterministic"}"#).expect("should parse");
    assert!((p.probability - 1.0).abs() < f64::EPSILON, "probability defaults to 1.0");
    assert!(p.always_keep_errors, "errors are kept unless someone opts out");
    assert_eq!(p.always_keep_slower_than_ms, None);
    assert_eq!(p.decide(FLEET_IDS[0], None), Decision::Keep);
}

#[test]
fn the_ratio_sampler_is_the_one_that_cannot_agree_across_a_fleet() {
    // Ratio draws at random, so the same trace id gets different answers. That
    // is the whole reason `deterministic` exists, and no test said so.
    let p = Policy { kind: SamplingKind::Ratio, probability: 0.5, ..Policy::default() };
    let id = FLEET_IDS[0];
    let kept = (0..500).filter(|_| p.decide(id, None) == Decision::Keep).count();
    assert!(kept > 0 && kept < 500, "a ratio sampler should not be stable for one id, kept {kept}");

    // Its extremes are exact even so, so a project pinned to 0.0 or 1.0 is not
    // at the mercy of a random draw.
    let all = Policy { kind: SamplingKind::Ratio, probability: 1.0, ..Policy::default() };
    let none = Policy { kind: SamplingKind::Ratio, probability: 0.0, ..Policy::default() };
    for _ in 0..200 {
        let id = ids::new_trace_id();
        assert_eq!(all.decide(&id, None), Decision::Keep);
        assert_eq!(none.decide(&id, None), Decision::Drop);
    }
}

#[test]
fn an_upstream_decision_beats_a_tail_policy_before_anything_is_buffered() {
    // Deferring a trace an upstream service has already excluded would buffer
    // work that can never be kept, and the two hops would disagree.
    let p = Policy { kind: SamplingKind::Tail, ..Policy::default() };
    assert_eq!(p.decide(FLEET_IDS[0], Some(false)), Decision::Drop);
    assert_eq!(p.decide(FLEET_IDS[0], Some(true)), Decision::Keep);
    assert_eq!(p.decide(FLEET_IDS[0], None), Decision::Defer);
}

#[test]
fn a_tail_policy_that_keeps_no_errors_falls_back_to_the_head_decision() {
    // With the error rule off, tail and deterministic sampling must select the
    // same traces, so switching a project from one to the other does not change
    // which traces exist.
    let head = Policy::deterministic(0.5);
    let tail = Policy {
        kind: SamplingKind::Tail,
        probability: 0.5,
        always_keep_errors: false,
        always_keep_slower_than_ms: None,
    };
    for _ in 0..500 {
        let id = ids::new_trace_id();
        assert_eq!(
            tail.decide_tail(&id, true, Some(60_000)),
            head.decide(&id, None),
            "with both rules off, tail sampling must agree with the head decision for {id}",
        );
    }
}

#[test]
fn tail_sampling_keeps_an_error_whatever_the_rate_or_the_id() {
    // The rarest traces are the ones a rate sampler is most likely to throw
    // away, which is exactly backwards. Checked across many ids because a
    // single id can pass by luck of the hash.
    let p = Policy { kind: SamplingKind::Tail, probability: 0.0, ..Policy::default() };
    for _ in 0..500 {
        let id = ids::new_trace_id();
        assert_eq!(p.decide_tail(&id, true, None), Decision::Keep);
        assert_eq!(p.decide_tail(&id, false, None), Decision::Drop);
    }
}

// ---------------------------------------------------------------------------
// Cost: a number somebody puts in a budget.
// ---------------------------------------------------------------------------

fn usage(input: Option<u64>, output: Option<u64>) -> TokenUsage {
    TokenUsage {
        input_tokens: input,
        output_tokens: output,
        total_tokens: None,
        input_details: None,
        output_details: None,
    }
}

fn input_details(details: InputTokenDetails, total: Option<u64>) -> TokenUsage {
    TokenUsage { input_details: Some(details), ..usage(total, Some(0)) }
}

#[test]
fn a_call_that_used_no_tokens_costs_zero_rather_than_nothing() {
    // Zero and unknown are different answers and the UI shows them differently.
    // A call that really did use no tokens has a cost, and it is zero.
    let c = cost::compute(&cost::builtin_book(), "gpt-4o", &usage(Some(0), Some(0)))
        .expect("a known model with reported usage is priced");
    assert_eq!(c.total_cost, Some(0.0));
    assert_eq!(c.input_cost, Some(0.0));
    assert_eq!(c.output_cost, Some(0.0));
}

#[test]
fn a_known_model_with_no_usage_reported_is_not_priced_at_all() {
    // Nothing was reported, so there is nothing to price. Charging zero here
    // would quietly claim a call was free.
    assert!(cost::compute(&cost::builtin_book(), "gpt-4o", &usage(None, None)).is_none());
}

#[test]
fn an_implausibly_large_token_count_produces_a_finite_number() {
    // Token counts come off the wire and nothing validates them. The arithmetic
    // has to stay finite so an aggregate over the column is still readable.
    let c = cost::compute(&cost::builtin_book(), "gpt-4o", &usage(Some(u64::MAX), Some(u64::MAX)))
        .expect("should still price");
    let total = c.total_cost.expect("a total");
    assert!(total.is_finite(), "cost overflowed to {total}");
    assert!(total > 0.0);
}

#[test]
fn a_cache_count_larger_than_the_input_total_prices_the_total_as_the_uncached_share() {
    // The shape a real Anthropic response has: `input_tokens` counts what was
    // NOT served from cache, and the cache counts sit beside it. Reading that
    // total as gross and subtracting the cache from it floors the plain share
    // at zero, so 100 uncached tokens bill as nothing. The earlier version of
    // this test asserted 0.0003 here, which is half the bill: it recorded the
    // under-bill as intended behaviour and kept it alive.
    let u = input_details(
        InputTokenDetails { cache_read: Some(1000), ..Default::default() },
        Some(100),
    );
    let c = cost::compute(&cost::builtin_book(), "claude-sonnet-4", &u).expect("should price");
    // 100 uncached at 3.00 plus 1000 cached at 0.30, per million.
    assert!((c.input_cost.unwrap() - 0.000_6).abs() < 1e-12, "got {:?}", c.input_cost);
}

#[test]
fn a_breakdown_larger_than_the_text_share_reported_beside_it_never_goes_negative() {
    // The safety property the test above used to carry, on a shape that is
    // genuinely inconsistent rather than merely net. The producer said the
    // uncached share was 50 and the cache read was 1000, against a total of
    // 100: no reading of those three reconciles. The arithmetic still has to
    // stay non-negative and finite, and price what it was told.
    let u = input_details(
        InputTokenDetails { text: Some(50), cache_read: Some(1000), ..Default::default() },
        Some(100),
    );
    let c = cost::compute(&cost::builtin_book(), "claude-sonnet-4", &u).expect("should price");
    // 50 at 3.00 plus 1000 at 0.30, per million.
    assert!((c.input_cost.unwrap() - 0.000_45).abs() < 1e-12, "got {:?}", c.input_cost);
    let total = c.total_cost.expect("a total");
    assert!(total >= 0.0, "cost went negative: {total}");
    assert!(total.is_finite(), "cost is not finite: {total}");
}

#[test]
fn a_reasoning_count_larger_than_the_output_total_is_read_the_same_way() {
    // The two halves of `compute` have drifted apart before, so the rule is
    // pinned on both. 800 reasoning tokens cannot sit inside an output total of
    // 100, so that total is the share left beside them, not a gross count to
    // subtract from.
    let book = PriceBook {
        id: "negotiated".into(),
        currency: "USD".into(),
        as_of: None,
        models: [(
            "r-model".to_owned(),
            ModelRates { input: 1.0, output: 10.0, reasoning: Some(30.0), ..Default::default() },
        )]
        .into_iter()
        .collect(),
        providers: HashMap::new(),
    };
    let u = TokenUsage {
        output_details: Some(OutputTokenDetails { reasoning: Some(800), ..Default::default() }),
        ..usage(Some(0), Some(100))
    };
    let c = cost::compute(&book, "r-model", &u).expect("should price");
    // 100 remaining output at 10.00 plus 800 reasoning at 30.00, per million.
    assert!((c.output_cost.unwrap() - 0.025).abs() < 1e-12, "got {:?}", c.output_cost);
}

#[test]
fn audio_tokens_fall_back_to_the_plain_rate_when_the_book_prices_no_audio() {
    // The built-in book has no audio rate. Falling back to the input rate is
    // better than dropping the tokens, because the total still reconciles.
    let u = input_details(InputTokenDetails { audio: Some(400), ..Default::default() }, Some(1000));
    let c = cost::compute(&cost::builtin_book(), "gpt-4o", &u).expect("should price");
    // 600 plain plus 400 audio, all at 2.50 per million.
    assert!((c.input_cost.unwrap() - 0.002_5).abs() < 1e-12, "got {:?}", c.input_cost);
}

#[test]
fn reasoning_tokens_are_billed_once_at_the_output_rate_when_no_reasoning_rate_exists() {
    // Reasoning tokens are already inside the output total. Pricing them again
    // would double the bill of every reasoning model.
    let u = TokenUsage {
        output_details: Some(OutputTokenDetails {
            text: None,
            reasoning: Some(800),
            audio: None,
            image: None,
        }),
        ..usage(Some(0), Some(1000))
    };
    let c = cost::compute(&cost::builtin_book(), "o3", &u).expect("should price");
    // 1000 output tokens at 8.00 per million, counted once.
    assert!((c.output_cost.unwrap() - 0.008).abs() < 1e-12, "got {:?}", c.output_cost);
}

#[test]
fn a_book_with_its_own_reasoning_rate_prices_the_remainder_and_the_reasoning_apart() {
    let book = PriceBook {
        id: "negotiated".into(),
        currency: "GBP".into(),
        as_of: None,
        models: [(
            "r-model".to_owned(),
            ModelRates { input: 1.0, output: 10.0, reasoning: Some(30.0), ..Default::default() },
        )]
        .into_iter()
        .collect(),
        providers: HashMap::new(),
    };
    let u = TokenUsage {
        output_details: Some(OutputTokenDetails {
            text: None,
            reasoning: Some(800),
            audio: None,
            image: None,
        }),
        ..usage(Some(0), Some(1000))
    };
    let c = cost::compute(&book, "r-model", &u).expect("should price");
    // 200 remaining output at 10.00 plus 800 reasoning at 30.00, per million.
    assert!((c.output_cost.unwrap() - 0.026).abs() < 1e-12, "got {:?}", c.output_cost);
    assert_eq!(c.currency, "GBP", "the book's currency travels with the figure");
    assert_eq!(c.price_book.as_deref(), Some("negotiated"));
}

#[test]
fn a_compact_date_suffix_resolves_to_the_same_model_as_the_dashed_one() {
    // Both spellings are in the wild for the same model, and the unit test only
    // covers the dashed one.
    let b = cost::builtin_book();
    let dashed = cost::compute(&b, "gpt-4o-2024-08-06", &usage(Some(1000), Some(1000)));
    let compact = cost::compute(&b, "gpt-4o-20240806", &usage(Some(1000), Some(1000)));
    assert_eq!(dashed.map(|c| c.total_cost), compact.map(|c| c.total_cost));
}

#[test]
fn a_cost_is_only_added_to_another_of_the_same_currency() {
    // The unit test starts from an empty accumulator. This one starts from a
    // populated one and checks the rejected figure leaves nothing behind.
    let gbp = mcpd_core::Cost {
        currency: "GBP".into(),
        input_cost: Some(1.0),
        output_cost: Some(2.0),
        total_cost: Some(3.0),
        price_book: Some("negotiated".into()),
        is_estimated: true,
    };
    let usd = mcpd_core::Cost { currency: "USD".into(), ..gbp.clone() };
    let mut acc = Some(gbp.clone());
    assert!(!cost::accumulate(&mut acc, &usd));
    assert_eq!(acc, Some(gbp), "a refused figure must not change the accumulator");
}

#[test]
fn image_input_tokens_are_priced_rather_than_dropped_from_the_bill() {
    // `metered_in` lists cache and audio meters but not `image`, and the plain
    // share comes from `text` when the provider sent it. So a call reporting
    // 600 text and 400 image tokens is billed for 600, while the spec says the
    // total is authoritative.
    let u = input_details(
        InputTokenDetails { text: Some(600), image: Some(400), ..Default::default() },
        Some(1000),
    );
    let c = cost::compute(&cost::builtin_book(), "gpt-4o", &u).expect("should price");
    // 1000 input tokens at 2.50 per million.
    assert!(
        (c.input_cost.unwrap() - 0.002_5).abs() < 1e-12,
        "image tokens were not billed, got {:?}",
        c.input_cost,
    );
}

#[test]
fn a_price_book_prices_the_model_id_its_operator_wrote() {
    // `lookup` lowercases the incoming model id but compares it against the
    // book's keys as written. A book naming `Claude-Sonnet-4` therefore matches
    // nothing at all, not even that exact string, and the cost column is
    // silently blank rather than an error.
    let book = PriceBook {
        id: "negotiated".into(),
        currency: "USD".into(),
        as_of: None,
        models: [(
            "Claude-Sonnet-4".to_owned(),
            ModelRates { input: 1.0, output: 1.0, ..Default::default() },
        )]
        .into_iter()
        .collect(),
        providers: HashMap::new(),
    };
    assert!(
        cost::compute(&book, "Claude-Sonnet-4", &usage(Some(1000), Some(0))).is_some(),
        "the exact key the operator wrote should resolve",
    );
}

// ---------------------------------------------------------------------------
// The built-in book itself. Its rates are data, and wrong data is a wrong bill
// that no amount of correct arithmetic above it can rescue.
// ---------------------------------------------------------------------------

#[test]
fn a_one_hour_cache_write_is_not_billed_at_the_five_minute_rate() {
    // Anthropic prices the two TTLs apart: a five-minute write is 1.25x the
    // base input rate and a one-hour write is 2x it. `compute` already falls
    // back from `cache_write_1h` to `cache_write`, so an entry that omits the
    // one-hour rate bills a one-hour write at 62.5% of what it cost, with no
    // error and no flag. That is the same silent shape as the 65% under-bill.
    let u = input_details(
        InputTokenDetails { cache_write_1h: Some(1_000_000), text: Some(0), ..Default::default() },
        Some(1_000_000),
    );
    let c = cost::compute(&cost::builtin_book(), "claude-sonnet-4", &u).expect("should price");
    assert!(
        (c.input_cost.unwrap() - 6.00).abs() < 1e-9,
        "a one-hour cache write billed at {:?}, expected 6.00",
        c.input_cost,
    );
}

#[test]
fn audio_input_is_billed_at_the_audio_rate_where_the_provider_charges_one() {
    // Gemini 2.5 Flash charges 1.00 per million for audio input against 0.30
    // for text. `ModelRates` has carried an `audio_input` field the whole time
    // and the built-in entry never set it, so an audio call billed at a third
    // of its price.
    let u = input_details(
        InputTokenDetails { audio: Some(1_000_000), text: Some(0), ..Default::default() },
        Some(1_000_000),
    );
    let c = cost::compute(&cost::builtin_book(), "gemini-2.5-flash", &u).expect("should price");
    assert!(
        (c.input_cost.unwrap() - 1.00).abs() < 1e-9,
        "audio input billed at {:?}, expected 1.00",
        c.input_cost,
    );
}

#[test]
fn the_built_in_book_prices_the_generation_of_models_agents_actually_run() {
    // `claude-haiku-4` was never an Anthropic model id. It sat in the book at
    // Claude 3.5 Haiku's rates, so the book looked like it covered a Haiku 4
    // generation it did not, while the id Anthropic does serve resolved to
    // nothing. A book that names a model nobody sells is worse than a short
    // book, because it reads as coverage.
    let b = cost::builtin_book();
    assert!(b.lookup("claude-haiku-4").is_none(), "the book still names a model nobody serves");
    for id in ["claude-sonnet-4-5", "claude-opus-4-5", "claude-haiku-4-5"] {
        assert!(b.lookup(id).is_some(), "{id} is unpriced");
    }
    // Dated ids are what a provider actually returns, and they must reach the
    // same entry through the date-suffix rule.
    let dated = b.lookup("claude-haiku-4-5-20251001").expect("a dated id should resolve");
    assert!((dated.input - 1.00).abs() < 1e-9, "haiku 4.5 input rate is {}", dated.input);
    assert!((dated.output - 5.00).abs() < 1e-9, "haiku 4.5 output rate is {}", dated.output);
}

#[test]
fn the_built_in_book_says_when_its_rates_were_last_checked() {
    // The book cannot be kept current by the release cycle, so it has to be
    // able to say how old it is. Without a date an operator cannot tell a rate
    // that is right from one that was right two model generations ago, and the
    // failure is silent either way.
    let stamp = cost::builtin_book().as_of.expect("the built-in book must carry a date");
    assert_eq!(stamp.len(), 10, "expected an ISO date, got {stamp}");
    assert!(
        stamp.chars().enumerate().all(|(i, c)| if i == 4 || i == 7 {
            c == '-'
        } else {
            c.is_ascii_digit()
        }),
        "expected YYYY-MM-DD, got {stamp}",
    );
}

#[test]
fn a_price_book_written_before_the_new_meters_existed_still_loads() {
    // A price book is a file an operator wrote, and it outlives the binary that
    // reads it. Every field added to `ModelRates` or `PriceBook` has to be
    // optional on the way in, or the next release stops loading books that were
    // correct when they were written and prices nothing at all.
    let json = r#"{
        "id": "negotiated",
        "currency": "USD",
        "models": { "house-model": { "input": 2.0, "output": 6.0 } }
    }"#;
    let book: PriceBook = serde_json::from_str(json).expect("an older book must still parse");
    assert!(book.as_of.is_none(), "a book that gave no date must not acquire one");
    let rates = book.lookup("house-model").expect("the model should resolve");
    assert_eq!(rates.image_input, None);
    assert_eq!(rates.image_output, None);
    let c = cost::compute(&book, "house-model", &usage(Some(1_000_000), Some(1_000_000)))
        .expect("should price");
    assert!((c.total_cost.unwrap() - 8.0).abs() < 1e-12, "got {:?}", c.total_cost);
}

#[test]
fn image_input_tokens_are_billed_at_the_image_rate_when_the_book_prices_one() {
    // The mirror of the output case, and it exists because the two halves of
    // `compute` have drifted apart before: an audit found the output half still
    // dropping image tokens after the input half had been fixed. OpenAI bills
    // image input on `gpt-image-2` at 8.00 against 5.00 for text.
    let book = PriceBook {
        id: "negotiated".into(),
        currency: "USD".into(),
        as_of: None,
        models: [(
            "image-model".to_owned(),
            ModelRates { input: 5.0, output: 0.0, image_input: Some(8.0), ..Default::default() },
        )]
        .into_iter()
        .collect(),
        providers: HashMap::new(),
    };
    let u = input_details(
        InputTokenDetails { text: Some(600), image: Some(400), ..Default::default() },
        Some(1000),
    );
    let c = cost::compute(&book, "image-model", &u).expect("should price");
    // 600 text at 5.00 plus 400 image at 8.00, per million. Billing the image
    // share twice would give 0.0082, and dropping it would give 0.0030.
    assert!(
        (c.input_cost.unwrap() - 0.006_2).abs() < 1e-12,
        "image input billed at {:?}, expected 0.0062",
        c.input_cost,
    );
}

#[test]
fn image_output_tokens_are_billed_at_the_image_rate_when_the_book_prices_one() {
    // Google and OpenAI bill image output tokens at ten to forty times the text
    // output rate on the same model, and gpt-image-2 sets text output to zero
    // outright, so folding image tokens into the text share bills nothing at
    // all. `OutputTokenDetails.image` is already on the wire, so mcpd receives
    // the count and had no rate to price it with.
    let book = PriceBook {
        id: "negotiated".into(),
        currency: "USD".into(),
        as_of: None,
        models: [(
            "flash-image".to_owned(),
            ModelRates {
                input: 0.30,
                output: 2.50,
                image_output: Some(30.0),
                ..Default::default()
            },
        )]
        .into_iter()
        .collect(),
        providers: HashMap::new(),
    };
    let u = TokenUsage {
        output_details: Some(OutputTokenDetails {
            text: Some(600),
            image: Some(400),
            ..Default::default()
        }),
        ..usage(Some(0), Some(1000))
    };
    let c = cost::compute(&book, "flash-image", &u).expect("should price");
    // 600 text at 2.50 plus 400 image at 30.00, per million.
    assert!(
        (c.output_cost.unwrap() - 0.013_5).abs() < 1e-12,
        "image output billed at {:?}, expected 0.0135",
        c.output_cost,
    );
}

// ---------------------------------------------------------------------------
// Identifiers: OpenTelemetry's shapes, or downstream tools reject them.
// ---------------------------------------------------------------------------

#[test]
fn a_generated_id_survives_the_whole_round_trip_it_will_actually_take() {
    // Generated here, rendered into a `traceparent`, parsed back by the next
    // hop, and handed to the sampler. Each step is unit tested alone; nothing
    // checked that the output of one is accepted by the next.
    for _ in 0..200 {
        let trace_id = ids::new_trace_id();
        let parent_id = ids::new_span_id();
        let header = TraceParent { trace_id: trace_id.clone(), parent_id, flags: 0x01 }.to_header();
        let parsed = ids::parse_traceparent(&header).expect("our own header should parse");
        assert_eq!(parsed.to_header(), header);
        assert_eq!(parsed.trace_id, trace_id);
        assert!(parsed.sampled());
        assert_eq!(Policy::always().decide(&parsed.trace_id, None), Decision::Keep);
    }
}

#[test]
fn generated_span_ids_do_not_collide_over_many_draws() {
    // Span ids are half the width of trace ids, so they are the ones worth
    // counting. A duplicate would merge two unrelated spans in the UI.
    const N: usize = 20_000;
    let spans: HashSet<String> = (0..N).map(|_| ids::new_span_id()).collect();
    assert_eq!(spans.len(), N, "span ids collided");
    let traces: HashSet<String> = (0..N).map(|_| ids::new_trace_id()).collect();
    assert_eq!(traces.len(), N, "trace ids collided");
    assert!(spans.iter().all(|s| s.len() == ids::SPAN_ID_LEN));
    assert!(!spans.contains(&"0".repeat(ids::SPAN_ID_LEN)), "the reserved id was generated");
}

#[test]
fn otlp_bytes_always_convert_into_something_validation_accepts() {
    // The protobuf path hands raw bytes to `hex_from_bytes` and the result goes
    // straight into a stored span, so the two must not disagree about what a
    // valid id is.
    for _ in 0..200 {
        let trace: [u8; 16] = rand::random();
        let span: [u8; 8] = rand::random();
        if trace.iter().any(|b| *b != 0) {
            let hex = ids::hex_from_bytes("trace", &trace).expect("non-zero bytes convert");
            ids::validate_trace_id(&hex).expect("what we produce, we must accept");
        }
        if span.iter().any(|b| *b != 0) {
            let hex = ids::hex_from_bytes("span", &span).expect("non-zero bytes convert");
            ids::validate_span_id(&hex).expect("what we produce, we must accept");
        }
    }
    assert!(ids::hex_from_bytes("span", &[0u8; 8]).is_err(), "all-zero span bytes are reserved");
    assert!(ids::hex_from_bytes("span", &[1u8; 16]).is_err(), "a span id is eight bytes");
}

#[test]
fn only_bit_zero_of_the_trace_flags_means_sampled() {
    // Other bits are already allocated by later drafts of the specification.
    // Reading the whole byte would turn an unrelated flag into a sampling
    // decision the upstream service never made.
    let header =
        |flags: &str| format!("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-{flags}");
    for (flags, sampled) in
        [("00", false), ("01", true), ("02", false), ("03", true), ("fe", false)]
    {
        let p = ids::parse_traceparent(&header(flags)).expect("should parse {flags}");
        assert_eq!(p.sampled(), sampled, "flags {flags} read wrongly");
    }
}

#[test]
fn a_traceparent_is_accepted_with_surrounding_space_and_rendered_back_in_lowercase() {
    // Header values arrive with whitespace often enough to matter. Uppercase
    // flags are accepted where uppercase ids are not, and rendering normalises
    // them, so a header we forward is the canonical spelling of what we read.
    let p = ids::parse_traceparent("  00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-FF  ")
        .expect("should parse");
    assert_eq!(p.flags, 0xff);
    assert_eq!(p.to_header(), "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-ff");
    assert!(p.sampled());
}

#[test]
fn a_hostile_traceparent_is_refused_without_panicking() {
    // Anyone who can reach `/v1/traces` can send this header. Every case here
    // is a byte-offset or a parse that would panic if it were written the
    // obvious way, and the answer to all of them is a fresh trace.
    let bad = [
        "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-0",
        "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-zz",
        "zz-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
        "0-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
        "00-4BF92F3577B34DA6A3CE929D0E0E4736-00f067aa0ba902b7-01",
        "00-€€€€€€€€€€€€€€€€-00f067aa0ba902b7-01",
        "00-4bf92f3577b34da6a3ce929d0e0e4736-🙂🙂-01",
        "-----",
        "00",
    ];
    for h in bad {
        assert!(ids::parse_traceparent(h).is_none(), "should have refused {h:?}");
    }
}

#[test]
fn an_id_that_is_thirty_two_bytes_but_not_thirty_two_characters_is_refused() {
    // The length check counts bytes. A multi-byte string can pass it, so the
    // hex check is what actually stops it, and it has to run on bytes too.
    let sixteen_two_byte_chars = "é".repeat(16);
    assert_eq!(sixteen_two_byte_chars.len(), 32);
    assert!(ids::validate_trace_id(&sixteen_two_byte_chars).is_err());
    assert!(ids::validate_span_id(&"é".repeat(8)).is_err());
}

#[test]
fn an_invalid_id_says_which_rule_it_broke() {
    // "invalid input" is a bug report waiting to happen. The reason is what an
    // operator reads when a whole batch is refused.
    let reason = |v: &str| match ids::validate_trace_id(v) {
        Err(mcpd_core::Error::InvalidId { reason, .. }) => reason,
        other => panic!("expected a rejection for {v:?}, got {other:?}"),
    };
    assert_eq!(reason("abc"), "wrong length");
    assert_eq!(reason(&"z".repeat(32)), "not hexadecimal");
    assert_eq!(reason(&"A".repeat(32)), "must be lowercase");
    assert_eq!(reason(&"0".repeat(32)), "all-zero ids are reserved");
}

/// A last check that the redactor leaves an id fit to store, because a hashed
/// value goes into the same column an OpenTelemetry id came from elsewhere.
#[test]
fn a_hashed_identifier_is_still_a_plain_printable_string() {
    let r = Redactor::with_defaults();
    let hashed = r.hash_identifier(Some("user@example.com")).expect("a value hashes");
    assert!(hashed.is_ascii(), "a digest must not carry the input's encoding: {hashed}");
    assert!(!hashed.contains("user@example.com"));
    assert_eq!(serde_json::to_value(&hashed).unwrap(), Value::String(hashed.clone()));
}

// ---------------------------------------------------------- audit findings ---
// Four defects an adversarial audit found in fixes made an hour before it ran.
// Three were regressions the fixes introduced. They are pinned here because a
// hurried fix to a correctness path is exactly what nobody re-reads.

/// Anthropic reports input_tokens NET of cache tokens, so deriving the plain
/// share as "total minus metered" underflows to zero and bills the entire
/// uncached input as nothing. Measured at a 65% under-bill, silently.
#[test]
fn a_provider_that_reports_its_total_net_of_cache_is_not_billed_zero() {
    let usage = TokenUsage {
        input_tokens: Some(50_000),
        input_details: Some(InputTokenDetails {
            text: Some(50_000),
            cache_read: Some(200_000),
            cache_write: Some(5_000),
            ..Default::default()
        }),
        ..Default::default()
    };
    let c = cost::compute(&cost::builtin_book(), "claude-sonnet-4", &usage)
        .expect("a known model should price");
    let input = c.input_cost.unwrap_or_default();
    assert!(
        input > 0.2,
        "the uncached input share was billed as nothing: got {input}, expected about 0.229"
    );
}

/// The image share has no meter of its own, so it belongs in the plain share.
/// The input half was fixed and the output half was left, which is how two
/// halves of one function come to disagree.
#[test]
fn image_output_tokens_are_billed_like_image_input_tokens() {
    let usage = TokenUsage {
        output_tokens: Some(1_000),
        output_details: Some(OutputTokenDetails {
            text: Some(600),
            image: Some(400),
            ..Default::default()
        }),
        ..Default::default()
    };
    let c = cost::compute(&cost::builtin_book(), "gpt-4o", &usage).expect("should price");
    let out = c.output_cost.unwrap_or_default();
    assert!(
        (out - 0.010).abs() < 1e-9,
        "400 of 1000 output tokens left the bill: got {out}, expected 0.010"
    );
}

/// A digest is a control only while the input space is large. A five-digit value
/// was recovered from its digest in under a hundred thousand guesses, using the
/// built-in key, which is a constant in the source and therefore public.
#[test]
fn a_payload_value_short_enough_to_guess_is_masked_rather_than_hashed() {
    let r = Redactor::with_defaults();
    let out = r.redact(&serde_json::json!({ "user_id": { "phone": "12345" } }));
    let inner = out["user_id"]["phone"].as_str().unwrap_or_default();
    assert!(!inner.starts_with("sha256:"), "a guessable value was stored as a digest: {inner}");
}

/// Correlation fields are the other half of that decision and must NOT change.
/// They exist to be grouped on, so equal inputs have to stay equal whatever the
/// value, and the privacy page says plainly that a short one can be guessed.
#[test]
fn a_short_correlation_identifier_still_hashes_so_grouping_survives() {
    let r = Redactor::with_defaults();
    let a = r.hash_identifier(Some("42")).expect("a value in gives a value out");
    let b = r.hash_identifier(Some("42")).expect("a value in gives a value out");
    assert!(a.starts_with("sha256:"), "a correlation id must stay hashed, got {a}");
    assert_eq!(a, b, "the same identifier must group with itself");
}

/// HashMap iteration is seeded per process. mcpd prices at ingest and stores the
/// result, so a lookup that picks by iteration order would give two spans for
/// one model permanently different costs depending which process took them.
#[test]
fn a_book_whose_keys_differ_only_in_case_resolves_the_same_way_every_time() {
    let mut book = cost::PriceBook::empty("test");
    for (key, input) in [("GPT-4o", 1.0), ("GPT-4O", 999.0)] {
        book.models
            .insert(key.to_owned(), cost::ModelRates { input, output: 1.0, ..Default::default() });
    }
    // Asserting self-consistency would prove nothing: HashMap ordering is stable
    // WITHIN a process and the defect was across processes, so a loop here
    // passes under the bug. This pins the RULE instead. The smallest key wins,
    // and "GPT-4O" sorts before "GPT-4o" because uppercase O is the smaller
    // byte, so the 999 rate is the answer every time and on every machine.
    assert_eq!(
        book.lookup("gpt-4o").map(|r| r.input),
        Some(999.0),
        "the smallest matching key must win, so this cannot vary by hash seed"
    );
}

// ---------------------------------------------------------------------------
// Price is keyed by model AND provider. The same model id costs different
// money depending on who served it, mcpd prices at ingest, and the figure it
// stores is the one a bill is reconciled against, so a wrong key is permanent.
// ---------------------------------------------------------------------------

/// A book that prices one model twice: once for everybody, once for Bedrock.
fn two_rate_book() -> PriceBook {
    let bedrock: HashMap<String, ModelRates> =
        [("house-model".to_owned(), ModelRates { input: 4.5, output: 15.0, ..Default::default() })]
            .into_iter()
            .collect();
    PriceBook {
        id: "negotiated".into(),
        currency: "USD".into(),
        as_of: None,
        models: [(
            "house-model".to_owned(),
            ModelRates { input: 3.0, output: 15.0, ..Default::default() },
        )]
        .into_iter()
        .collect(),
        providers: [("bedrock".to_owned(), bedrock)].into_iter().collect(),
    }
}

#[test]
fn a_provider_qualified_rate_is_used_when_the_span_says_who_served_the_call() {
    let book = two_rate_book();
    let u = usage(Some(1_000_000), Some(0));

    let bedrock = cost::compute_with_provider(&book, "house-model", Some("bedrock"), &u)
        .expect("a provider-qualified rate should price");
    assert!((bedrock.input_cost.unwrap() - 4.5).abs() < 1e-12, "got {:?}", bedrock.input_cost);
    // The figure is stored rather than recomputed, so it has to carry which of
    // the two rates produced it. The book id alone cannot say.
    assert_eq!(bedrock.price_book.as_deref(), Some("negotiated@bedrock"));

    let direct = cost::compute_with_provider(&book, "house-model", Some("anthropic"), &u)
        .expect("a provider with no table falls back");
    assert!((direct.input_cost.unwrap() - 3.0).abs() < 1e-12, "got {:?}", direct.input_cost);
    assert_eq!(direct.price_book.as_deref(), Some("negotiated"));

    let unnamed = cost::compute(&book, "house-model", &u).expect("no provider still prices");
    assert!((unnamed.input_cost.unwrap() - 3.0).abs() < 1e-12, "got {:?}", unnamed.input_cost);
    assert_eq!(unnamed.price_book.as_deref(), Some("negotiated"));
}

#[test]
fn a_model_the_provider_table_does_not_name_still_gets_the_model_only_rate() {
    // An operator lists the handful of models whose rate actually differs. The
    // rest of that provider's traffic must keep pricing, or adding one
    // negotiated rate would blank the cost column for every other model.
    let mut book = two_rate_book();
    book.models.insert("other-model".to_owned(), ModelRates { input: 2.0, ..Default::default() });
    let c = cost::compute_with_provider(
        &book,
        "other-model",
        Some("bedrock"),
        &usage(Some(1_000_000), Some(0)),
    )
    .expect("the model-only entry still applies");
    assert!((c.input_cost.unwrap() - 2.0).abs() < 1e-12, "got {:?}", c.input_cost);
    assert_eq!(
        c.price_book.as_deref(),
        Some("negotiated"),
        "this rate is the fallback, not Bedrock's"
    );
}

#[test]
fn a_provider_is_matched_however_the_producer_spelled_it() {
    // `provider` is whatever the span carried. OpenTelemetry writes
    // `aws.bedrock` for the same thing OpenInference writes as `bedrock`, and
    // nothing normalises the case on the way in.
    let book = two_rate_book();
    let u = usage(Some(1_000_000), Some(0));
    for spelling in ["bedrock", "aws.bedrock", "AWS.Bedrock", "BEDROCK"] {
        let c = cost::compute_with_provider(&book, "house-model", Some(spelling), &u)
            .unwrap_or_else(|| panic!("{spelling} should price"));
        assert!(
            (c.input_cost.unwrap() - 4.5).abs() < 1e-12,
            "{spelling} priced at {:?}, expected Bedrock's 4.50",
            c.input_cost,
        );
    }
    // Not a licence to guess. A provider that shares no name with the table is
    // the fallback, not a near miss.
    let c = cost::compute_with_provider(&book, "house-model", Some("bedrock-eu"), &u)
        .expect("should still price");
    assert!((c.input_cost.unwrap() - 3.0).abs() < 1e-12, "got {:?}", c.input_cost);
}

/// The model-keyed defect, one dimension out. HashMap iteration is seeded per
/// process, so a provider chosen by iteration order would give two spans for one
/// model permanently different costs depending which process took them.
#[test]
fn two_provider_keys_differing_only_in_case_resolve_the_same_way_every_time() {
    let mut book = PriceBook::empty("test");
    book.models.insert("m".to_owned(), ModelRates { input: 1.0, ..Default::default() });
    for (key, input) in [("Bedrock", 2.0), ("BEDROCK", 999.0)] {
        let table: HashMap<String, ModelRates> =
            [("m".to_owned(), ModelRates { input, ..Default::default() })].into_iter().collect();
        book.providers.insert(key.to_owned(), table);
    }
    // Self-consistency inside one process would prove nothing: HashMap ordering
    // is stable within a process and the defect is across processes. This pins
    // the rule instead. The smallest key wins, and "BEDROCK" sorts before
    // "Bedrock" because uppercase E is the smaller byte.
    let c =
        cost::compute_with_provider(&book, "m", Some("bedrock"), &usage(Some(1_000_000), Some(0)))
            .expect("should price");
    assert!(
        (c.input_cost.unwrap() - 999.0).abs() < 1e-12,
        "the smallest matching provider key must win, so this cannot vary by hash seed: got {:?}",
        c.input_cost,
    );
}

#[test]
fn normalising_a_book_reaches_the_provider_tables_as_well_as_the_models() {
    // `normalise` is what makes a hand-written book reachable at all. A
    // provider table it skipped would be a table that never priced anything,
    // silently, which reads as a provider whose rates nobody supplied.
    let mut book = PriceBook::empty("hand-written");
    let table: HashMap<String, ModelRates> =
        [("House-Model".to_owned(), ModelRates { input: 4.5, ..Default::default() })]
            .into_iter()
            .collect();
    book.providers.insert("AWS.Bedrock".to_owned(), table);
    book.normalise();
    assert!(book.providers.contains_key("aws.bedrock"), "the provider key was not lowercased");
    let c = cost::compute_with_provider(
        &book,
        "house-model",
        Some("aws.bedrock"),
        &usage(Some(1_000_000), Some(0)),
    )
    .expect("a normalised book should price");
    assert!((c.input_cost.unwrap() - 4.5).abs() < 1e-12, "got {:?}", c.input_cost);
}

#[test]
fn a_book_with_provider_rates_survives_the_file_it_is_written_in() {
    // Books are files an operator writes and mcpd reads back. The layer has to
    // parse, and a book without one must not grow an empty `providers` key on
    // the way out, because a book that changes shape when mcpd rewrites it is
    // a book an operator stops trusting.
    let json = r#"{
        "id": "negotiated",
        "currency": "USD",
        "models": { "house-model": { "input": 3.0, "output": 15.0 } },
        "providers": { "bedrock": { "house-model": { "input": 4.5, "output": 15.0 } } }
    }"#;
    let book: PriceBook =
        serde_json::from_str(json).expect("a book with provider rates must parse");
    let c = cost::compute_with_provider(
        &book,
        "house-model",
        Some("bedrock"),
        &usage(Some(1_000_000), Some(0)),
    )
    .expect("should price");
    assert!((c.input_cost.unwrap() - 4.5).abs() < 1e-12, "got {:?}", c.input_cost);

    let plain: PriceBook =
        serde_json::from_str(r#"{ "id": "old", "models": { "house-model": { "input": 3.0 } } }"#)
            .expect("a book written before provider rates must still parse");
    assert!(plain.providers.is_empty());
    let written = serde_json::to_value(&plain).expect("a book must serialise");
    assert!(written.get("providers").is_none(), "an empty layer must not be written out");
}

/// Two providers, one shared last segment.
///
/// BUG, in `PriceBook::provider_table` in `crates/mcpd-core/src/cost.rs`: the
/// fallback took the LAST dot-separated segment of the span's provider, so
/// `az.ai.openai` resolved to a table keyed `openai`. The semantic conventions
/// spell Azure OpenAI exactly that way, and Azure and OpenAI publish different
/// rate cards, which is the case this whole table exists to serve. The doc
/// beside it said "nothing else is guessed" while the code guessed this.
///
/// Input: a book with `providers["openai"]["m"].input = 99.0` and
/// `models["m"].input = 2.0`, priced for a span whose provider is
/// `az.ai.openai`.
/// Expected: 2.0, the model-only fallback, because this book says nothing about
/// Azure.
/// Observed: 99.0, OpenAI's rate, recorded as `b@openai`.
#[test]
fn a_provider_that_merely_ends_in_another_ones_name_is_not_that_provider() {
    let openai: HashMap<String, ModelRates> =
        [("m".to_owned(), ModelRates { input: 99.0, ..Default::default() })].into_iter().collect();
    let mut book = PriceBook::empty("b");
    book.models.insert("m".to_owned(), ModelRates { input: 2.0, ..Default::default() });
    book.providers.insert("openai".to_owned(), openai);
    let u = usage(Some(1_000_000), Some(0));

    for spelling in ["az.ai.openai", "az.ai.inference"] {
        let c = cost::compute_with_provider(&book, "m", Some(spelling), &u)
            .unwrap_or_else(|| panic!("{spelling} should still price off the fallback"));
        assert!(
            (c.input_cost.unwrap() - 2.0).abs() < 1e-12,
            "{spelling} was priced off OpenAI's table: {:?}",
            c.input_cost,
        );
        assert_eq!(c.price_book.as_deref(), Some("b"), "{spelling} claimed a provider match");
    }

    // The prefix rule the doc actually promises still holds, in both spellings.
    for spelling in ["openai", "azure.openai"] {
        let c = cost::compute_with_provider(&book, "m", Some(spelling), &u)
            .unwrap_or_else(|| panic!("{spelling} should price"));
        assert!((c.input_cost.unwrap() - 99.0).abs() < 1e-12, "got {:?}", c.input_cost);
    }
}
