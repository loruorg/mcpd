//! Sampling.
//!
//! Two rules make sampling correct, and both are easy to get wrong:
//!
//! 1. **The decision is made once, at the root, and inherited.** A trace that is
//!    half-recorded is worse than one that is absent, because the gaps look like
//!    bugs in the system under observation.
//! 2. **Every collector must agree.** [`SamplingKind::Deterministic`] decides by
//!    hashing the trace id, so a fleet of mcpd instances behind a load balancer
//!    keeps or drops the same traces without talking to each other.

use crate::SamplingKind;
use serde::{Deserialize, Serialize};

/// A sampling policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy {
    /// Which strategy to apply.
    pub kind: SamplingKind,
    /// Fraction to keep, 0.0 to 1.0. Used by `ratio` and `deterministic`.
    #[serde(default = "one")]
    pub probability: f64,
    /// Always keep a trace containing an error, whatever the policy says.
    ///
    /// On by default: the traces you most need are the rare ones, and a ratio
    /// sampler is most likely to throw them away.
    #[serde(default = "yes")]
    pub always_keep_errors: bool,
    /// Always keep a trace slower than this, in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub always_keep_slower_than_ms: Option<u64>,
}

fn one() -> f64 {
    1.0
}
fn yes() -> bool {
    true
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            kind: SamplingKind::Always,
            probability: 1.0,
            always_keep_errors: true,
            always_keep_slower_than_ms: None,
        }
    }
}

/// What a policy decided, and why.
///
/// The reason is carried so the UI can explain a gap: "this project samples at
/// 10%" is an answer, whereas a silently missing trace is a support ticket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Record the trace.
    Keep,
    /// Discard it.
    Drop,
    /// Buffer it and decide when it ends. Only [`SamplingKind::Tail`] returns
    /// this; the caller must eventually call [`Policy::decide_tail`].
    Defer,
}

impl Policy {
    /// Keep everything. The right default until volume says otherwise.
    #[must_use]
    pub fn always() -> Self {
        Self::default()
    }

    /// Keep a deterministic fraction, consistent across every collector.
    #[must_use]
    pub fn deterministic(probability: f64) -> Self {
        Self { kind: SamplingKind::Deterministic, probability, ..Self::default() }
    }

    /// Decide at the head of a trace, before anything about it is known.
    ///
    /// `upstream_sampled` carries a `traceparent`'s sampled flag when one was
    /// present. An upstream decision always wins: a distributed trace that some
    /// services keep and others drop is not a trace.
    #[must_use]
    pub fn decide(&self, trace_id: &str, upstream_sampled: Option<bool>) -> Decision {
        if let Some(up) = upstream_sampled {
            return if up { Decision::Keep } else { Decision::Drop };
        }
        match self.kind {
            SamplingKind::Always => Decision::Keep,
            SamplingKind::Never => Decision::Drop,
            SamplingKind::Ratio => {
                if self.probability >= 1.0 {
                    Decision::Keep
                } else if self.probability <= 0.0 {
                    Decision::Drop
                } else if rand::random::<f64>() < self.probability {
                    Decision::Keep
                } else {
                    Decision::Drop
                }
            }
            SamplingKind::Deterministic => {
                if trace_hash(trace_id) < self.threshold() {
                    Decision::Keep
                } else {
                    Decision::Drop
                }
            }
            // Tail sampling cannot answer yet: the interesting properties of a
            // trace are only known once it has finished.
            SamplingKind::Tail => Decision::Defer,
        }
    }

    /// Decide once a deferred trace has completed.
    #[must_use]
    pub fn decide_tail(
        &self,
        trace_id: &str,
        had_error: bool,
        duration_ms: Option<u64>,
    ) -> Decision {
        if self.always_keep_errors && had_error {
            return Decision::Keep;
        }
        if let (Some(limit), Some(d)) = (self.always_keep_slower_than_ms, duration_ms) {
            if d >= limit {
                return Decision::Keep;
            }
        }
        if trace_hash(trace_id) < self.threshold() {
            Decision::Keep
        } else {
            Decision::Drop
        }
    }

    fn threshold(&self) -> u64 {
        let p = self.probability.clamp(0.0, 1.0);
        (p * u64::MAX as f64) as u64
    }
}

/// Map a trace id onto the full `u64` range, uniformly.
///
/// Uses the trailing bytes of the id: OpenTelemetry requires trace ids to be
/// random, but some producers derive the leading bytes from a timestamp, which
/// would make a leading-byte sampler correlate with time of day.
fn trace_hash(trace_id: &str) -> u64 {
    let tail = trace_id.as_bytes();
    let n = tail.len();
    if n >= 16 {
        // `get` rather than a slice: a trace id is whatever the producer sent,
        // and a byte offset that lands inside a multi-byte character panics.
        // The fallback hash below is the correct answer for anything that is
        // not 16 hex digits, including a string this offset cannot cut.
        if let Some(v) = trace_id.get(n - 16..).and_then(|t| u64::from_str_radix(t, 16).ok()) {
            return v;
        }
    }
    // Not hex, or too short: fall back to a real hash so behaviour stays
    // uniform rather than degenerating to "keep everything".
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in tail {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    // FNV-1a alone has poor avalanche in its high bits, and the threshold
    // comparison below reads the whole word — without this finalising mix,
    // ids that differ only in their last characters cluster badly and the
    // realised sample rate drifts well off the requested one.
    fmix64(h)
}

/// MurmurHash3's 64-bit finaliser: cheap, and enough avalanche that a one-bit
/// input change redistributes the whole output.
const fn fmix64(mut h: u64) -> u64 {
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^= h >> 33;
    h = h.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    h ^= h >> 33;
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::new_trace_id;

    #[test]
    fn always_and_never_are_absolute() {
        let t = new_trace_id();
        assert_eq!(Policy::always().decide(&t, None), Decision::Keep);
        let never = Policy { kind: SamplingKind::Never, ..Policy::default() };
        assert_eq!(never.decide(&t, None), Decision::Drop);
    }

    #[test]
    fn upstream_decision_always_wins() {
        // Otherwise a distributed trace is kept by some hops and dropped by
        // others, which produces a trace full of holes.
        let never = Policy { kind: SamplingKind::Never, ..Policy::default() };
        assert_eq!(never.decide(&new_trace_id(), Some(true)), Decision::Keep);
        let always = Policy::always();
        assert_eq!(always.decide(&new_trace_id(), Some(false)), Decision::Drop);
    }

    #[test]
    fn deterministic_is_stable_for_the_same_trace() {
        let p = Policy::deterministic(0.5);
        let t = new_trace_id();
        let first = p.decide(&t, None);
        for _ in 0..50 {
            assert_eq!(p.decide(&t, None), first, "decision must not vary");
        }
    }

    #[test]
    fn deterministic_hits_roughly_the_requested_rate() {
        let p = Policy::deterministic(0.25);
        let n = 20_000;
        let kept = (0..n).filter(|_| p.decide(&new_trace_id(), None) == Decision::Keep).count();
        let rate = kept as f64 / n as f64;
        assert!((rate - 0.25).abs() < 0.02, "rate was {rate}");
    }

    #[test]
    fn deterministic_extremes_are_exact() {
        assert_eq!(Policy::deterministic(1.0).decide(&new_trace_id(), None), Decision::Keep);
        assert_eq!(Policy::deterministic(0.0).decide(&new_trace_id(), None), Decision::Drop);
    }

    #[test]
    fn tail_defers_then_keeps_errors() {
        let p = Policy { kind: SamplingKind::Tail, probability: 0.0, ..Policy::default() };
        let t = new_trace_id();
        assert_eq!(p.decide(&t, None), Decision::Defer);
        // Probability is zero, so only the error rule can save this trace.
        assert_eq!(p.decide_tail(&t, true, None), Decision::Keep);
        assert_eq!(p.decide_tail(&t, false, None), Decision::Drop);
    }

    #[test]
    fn tail_keeps_slow_traces() {
        let p = Policy {
            kind: SamplingKind::Tail,
            probability: 0.0,
            always_keep_slower_than_ms: Some(1000),
            ..Policy::default()
        };
        let t = new_trace_id();
        assert_eq!(p.decide_tail(&t, false, Some(2500)), Decision::Keep);
        assert_eq!(p.decide_tail(&t, false, Some(10)), Decision::Drop);
    }

    #[test]
    fn non_hex_ids_still_sample_uniformly() {
        let p = Policy::deterministic(0.5);
        let n = 5000;
        let kept = (0..n)
            .filter(|i| p.decide(&format!("not-a-hex-id-{i}"), None) == Decision::Keep)
            .count();
        let rate = kept as f64 / n as f64;
        assert!((rate - 0.5).abs() < 0.05, "rate was {rate}");
    }
}

#[cfg(test)]
mod hostile_id_tests {
    use super::*;

    /// The sampler hashes a producer-supplied trace id. A byte offset into it
    /// panicked on any multi-byte character.
    #[test]
    fn a_non_hex_multibyte_trace_id_still_gets_a_decision() {
        let p = Policy::deterministic(0.5);
        for id in ["€€€€€€", "ééééééééééééééééé", "", "not-hex-at-all", "🙂🙂🙂🙂🙂🙂🙂🙂"]
        {
            let _ = p.decide(id, None);
        }
    }
}
