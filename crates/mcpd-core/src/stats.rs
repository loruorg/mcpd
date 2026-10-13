//! The statistics an experiment comparison needs, and no more.
//!
//! A mean that moved is not a regression until the move is larger than the
//! noise, so a comparison reports an interval and a p-value beside the delta.
//! Welch's t-test is used because two experiments rarely vary alike: a new
//! prompt can make a scorer steadier as well as higher.

/// The result of comparing two samples' means.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Welch {
    /// Candidate mean minus baseline mean.
    pub delta: f64,
    /// Lower end of the 95% confidence interval on `delta`.
    pub ci_low: f64,
    /// Upper end of that interval.
    pub ci_high: f64,
    /// Two-sided p-value for "the means are equal".
    pub p_value: f64,
}

fn mean_var(xs: &[f64]) -> (f64, f64) {
    let n = xs.len() as f64;
    let m = xs.iter().sum::<f64>() / n;
    let v = xs.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (n - 1.0);
    (m, v)
}

/// Welch's t-test of `b` (candidate) against `a` (baseline).
///
/// `None` when either side has fewer than two observations, which leaves no
/// variance to estimate. Two samples with no variance at all give an interval
/// of exactly the delta and a p-value of 0 or 1.
#[must_use]
pub fn welch(a: &[f64], b: &[f64]) -> Option<Welch> {
    if a.len() < 2 || b.len() < 2 {
        return None;
    }
    let (ma, va) = mean_var(a);
    let (mb, vb) = mean_var(b);
    let (na, nb) = (a.len() as f64, b.len() as f64);
    let delta = mb - ma;
    let se2 = va / na + vb / nb;
    if se2 <= 0.0 || !se2.is_finite() {
        let p = if delta == 0.0 { 1.0 } else { 0.0 };
        return Some(Welch { delta, ci_low: delta, ci_high: delta, p_value: p });
    }
    let se = se2.sqrt();
    let df = se2.powi(2)
        / ((va / na).powi(2) / (na - 1.0) + (vb / nb).powi(2) / (nb - 1.0)).max(f64::MIN_POSITIVE);
    let t = delta / se;
    let p_value = (2.0 * (1.0 - student_t_cdf(t.abs(), df))).clamp(0.0, 1.0);
    let crit = t_quantile_975(df);
    Some(Welch { delta, ci_low: delta - crit * se, ci_high: delta + crit * se, p_value })
}

/// The CDF of Student's t with `df` degrees of freedom.
#[must_use]
pub fn student_t_cdf(t: f64, df: f64) -> f64 {
    let x = df / (df + t * t);
    let tail = 0.5 * incomplete_beta(df / 2.0, 0.5, x);
    if t >= 0.0 {
        1.0 - tail
    } else {
        tail
    }
}

/// The 97.5th percentile of Student's t, by bisection on the CDF.
fn t_quantile_975(df: f64) -> f64 {
    let (mut lo, mut hi) = (0.0_f64, 1000.0_f64);
    for _ in 0..100 {
        let mid = (lo + hi) / 2.0;
        if student_t_cdf(mid, df) < 0.975 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    (lo + hi) / 2.0
}

fn ln_gamma(x: f64) -> f64 {
    // Lanczos approximation, g = 7.
    const C: [f64; 9] = [
        0.999_999_999_999_809_9,
        676.520_368_121_885_1,
        -1_259.139_216_722_402_8,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507_343_278_686_905,
        -0.138_571_095_265_720_12,
        9.984_369_578_019_572e-6,
        1.505_632_735_149_311_6e-7,
    ];
    if x < 0.5 {
        let pi = std::f64::consts::PI;
        return (pi / (pi * x).sin()).ln() - ln_gamma(1.0 - x);
    }
    let x = x - 1.0;
    let mut a = C[0];
    let t = x + 7.5;
    for (i, c) in C.iter().enumerate().skip(1) {
        a += c / (x + i as f64);
    }
    0.5 * (2.0 * std::f64::consts::PI).ln() + (x + 0.5) * t.ln() - t + a.ln()
}

/// The regularised incomplete beta function I_x(a, b).
fn incomplete_beta(a: f64, b: f64, x: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let front =
        (ln_gamma(a + b) - ln_gamma(a) - ln_gamma(b) + a * x.ln() + b * (1.0 - x).ln()).exp();
    if x < (a + 1.0) / (a + b + 2.0) {
        front * beta_cf(a, b, x) / a
    } else {
        1.0 - front * beta_cf(b, a, 1.0 - x) / b
    }
}

/// Lentz's continued fraction for the incomplete beta function.
fn beta_cf(a: f64, b: f64, x: f64) -> f64 {
    const TINY: f64 = 1e-30;
    let mut c = 1.0;
    let mut d = 1.0 - (a + b) * x / (a + 1.0);
    if d.abs() < TINY {
        d = TINY;
    }
    d = 1.0 / d;
    let mut h = d;
    for m in 1..300 {
        let m = f64::from(m);
        let m2 = 2.0 * m;
        let aa = m * (b - m) * x / ((a + m2 - 1.0) * (a + m2));
        d = 1.0 + aa * d;
        d = if d.abs() < TINY { TINY } else { d };
        c = 1.0 + aa / c;
        c = if c.abs() < TINY { TINY } else { c };
        d = 1.0 / d;
        h *= d * c;
        let aa = -(a + m) * (a + b + m) * x / ((a + m2) * (a + m2 + 1.0));
        d = 1.0 + aa * d;
        d = if d.abs() < TINY { TINY } else { d };
        c = 1.0 + aa / c;
        c = if c.abs() < TINY { TINY } else { c };
        d = 1.0 / d;
        let del = d * c;
        h *= del;
        if (del - 1.0).abs() < 1e-12 {
            break;
        }
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_t_distribution_matches_published_tables() {
        // t(0.975) for 10 and 30 degrees of freedom: 2.228 and 2.042.
        assert!((t_quantile_975(10.0) - 2.228).abs() < 0.001);
        assert!((t_quantile_975(30.0) - 2.042).abs() < 0.001);
        assert!((student_t_cdf(0.0, 5.0) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn a_clear_drop_is_significant_and_noise_is_not() {
        let base: Vec<f64> = (0..30).map(|i| 0.8 + f64::from(i % 3) * 0.01).collect();
        let worse: Vec<f64> = (0..30).map(|i| 0.6 + f64::from(i % 3) * 0.01).collect();
        let w = welch(&base, &worse).unwrap();
        assert!(w.ci_high < 0.0 && w.p_value < 0.001, "{w:?}");
        let noisy: Vec<f64> = (0..30).map(|i| if i % 2 == 0 { 0.0 } else { 1.0 }).collect();
        let shifted: Vec<f64> = (0..30).map(|i| if i % 2 == 0 { 0.0 } else { 0.97 }).collect();
        let w = welch(&noisy, &shifted).unwrap();
        assert!(w.ci_low < 0.0 && w.ci_high > 0.0, "{w:?}");
        assert!(welch(&[1.0], &[1.0, 2.0]).is_none());
    }
}
