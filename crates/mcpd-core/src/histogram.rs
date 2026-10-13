//! Reading histograms back: merging bucketed points into one distribution.
//!
//! A histogram point from OpenTelemetry carries bucket boundaries and a count
//! per bucket, and its `value` is the sum of what it observed. Averaging those
//! sums, or taking a percentile of them, answers nothing anyone asked. The
//! distribution is in the buckets, so this module adds the buckets up and reads
//! quantiles out of the merged counts.
//!
//! Two kinds of point arrive under one histogram name:
//!
//! - **bucketed** points, from an OTLP histogram, with `buckets.bounds` and
//!   `buckets.counts`. A point reported with cumulative temporality carries
//!   everything since its series began, so for those only the growth across
//!   the window is counted, and a series whose counts fell (a restart) counts
//!   from its last point.
//! - **observations**, one value per point, which is how mcpd's own derived
//!   duration metrics are written. Their quantiles are exact, and they are
//!   placed into OpenTelemetry's default boundaries so a chart can draw them.

use crate::MetricPoint;
use serde_json::Value;
use std::collections::BTreeMap;

/// OpenTelemetry's default explicit bucket boundaries, used for observations.
pub const DEFAULT_BOUNDS: [f64; 15] = [
    0.0, 5.0, 10.0, 25.0, 50.0, 75.0, 100.0, 250.0, 500.0, 750.0, 1000.0, 2500.0, 5000.0, 7500.0,
    10000.0,
];

/// One merged distribution.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Distribution {
    /// Upper bounds of every bucket but the last, which is unbounded.
    pub bounds: Vec<f64>,
    /// One count per bucket: `bounds.len() + 1` entries.
    pub counts: Vec<u64>,
    /// Every observation counted.
    pub count: u64,
    /// Their sum, when known.
    pub sum: Option<f64>,
    /// The smallest observation, when known.
    pub min: Option<f64>,
    /// The largest observation, when known.
    pub max: Option<f64>,
    /// Whether quantiles come from the observations themselves rather than
    /// being interpolated inside a bucket.
    pub exact: bool,
    observations: Vec<f64>,
}

impl Distribution {
    /// The `q` quantile, 0 to 1. Exact over observations; otherwise linear
    /// inside the bucket the rank falls in, with the open-ended first and last
    /// buckets clamped to the known minimum and maximum, or to their one bound.
    #[must_use]
    pub fn quantile(&self, q: f64) -> Option<f64> {
        if self.count == 0 {
            return None;
        }
        if self.exact {
            let mut v = self.observations.clone();
            v.sort_by(f64::total_cmp);
            // Nearest rank, which is what the metric query reports too.
            let rank = ((q * v.len() as f64).ceil() as usize).clamp(1, v.len());
            return Some(v[rank - 1]);
        }
        let target = q * self.count as f64;
        let mut seen = 0.0;
        for (i, c) in self.counts.iter().enumerate() {
            let c = *c as f64;
            if c == 0.0 {
                continue;
            }
            if seen + c >= target {
                let lo = if i == 0 {
                    self.min.unwrap_or_else(|| self.bounds.first().copied().unwrap_or(0.0))
                } else {
                    self.bounds[i - 1]
                };
                let hi =
                    if i < self.bounds.len() { self.bounds[i] } else { self.max.unwrap_or(lo) };
                let into = ((target - seen) / c).clamp(0.0, 1.0);
                return Some(lo + (hi - lo).max(0.0) * into);
            }
            seen += c;
        }
        self.max.or_else(|| self.bounds.last().copied())
    }
}

fn number(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

/// A point's buckets: bounds, counts, and whether they are cumulative.
fn buckets_of(p: &MetricPoint) -> Option<(Vec<f64>, Vec<u64>, bool)> {
    let b = p.buckets.as_ref()?;
    let bounds: Vec<f64> = b.get("bounds")?.as_array()?.iter().filter_map(number).collect();
    let counts: Vec<u64> =
        b.get("counts")?.as_array()?.iter().filter_map(number).map(|n| n as u64).collect();
    if counts.len() != bounds.len() + 1 {
        return None;
    }
    // OTLP's AGGREGATION_TEMPORALITY_CUMULATIVE is 2.
    let cumulative =
        b.get("temporality").is_some_and(|t| t == "cumulative" || number(t) == Some(2.0));
    Some((bounds, counts, cumulative))
}

/// The series a point belongs to: the values of `group_by`, as one key.
fn series_key(p: &MetricPoint, group_by: &[String]) -> (String, Option<Value>) {
    if group_by.is_empty() {
        return (String::new(), None);
    }
    let mut labels = serde_json::Map::new();
    for k in group_by {
        let v = p
            .labels
            .as_ref()
            .and_then(|l| l.get(k))
            .cloned()
            .unwrap_or_else(|| Value::String("(none)".into()));
        labels.insert(k.clone(), v);
    }
    let v = Value::Object(labels);
    (v.to_string(), Some(v))
}

/// One bucketed point: its counts, whether they are cumulative, and its sum.
type Reading = (Vec<u64>, bool, f64);
/// Bucketed points by stream (their full label set), each in time order.
type Streams = BTreeMap<String, Vec<Reading>>;

/// Merge points into one distribution per series.
///
/// Points whose buckets disagree on their boundaries cannot be added up, so a
/// series keeps the boundaries most of its points use and counts how many it
/// had to leave out.
#[must_use]
pub fn merge(
    points: &[MetricPoint],
    group_by: &[String],
) -> Vec<(Option<Value>, Distribution, u64)> {
    struct Acc {
        labels: Option<Value>,
        observations: Vec<f64>,
        // By boundaries, then by the full label set (one OTLP stream), the
        // points in time order.
        bucketed: BTreeMap<String, (Vec<f64>, Streams)>,
    }
    let mut series: BTreeMap<String, Acc> = BTreeMap::new();
    let mut ordered: Vec<&MetricPoint> = points.iter().collect();
    ordered.sort_by_key(|p| p.timestamp);
    for p in ordered {
        let (key, labels) = series_key(p, group_by);
        let acc = series.entry(key).or_insert_with(|| Acc {
            labels,
            observations: Vec::new(),
            bucketed: BTreeMap::new(),
        });
        match buckets_of(p) {
            Some((bounds, counts, cumulative)) => {
                let bk = format!("{bounds:?}");
                let stream = p.labels.as_ref().map(ToString::to_string).unwrap_or_default();
                acc.bucketed
                    .entry(bk)
                    .or_insert_with(|| (bounds, BTreeMap::new()))
                    .1
                    .entry(stream)
                    .or_default()
                    .push((counts, cumulative, p.value));
            }
            None => acc.observations.push(p.value),
        }
    }

    series
        .into_values()
        .map(|acc| {
            if acc.bucketed.is_empty() {
                let obs = acc.observations;
                let mut counts = vec![0u64; DEFAULT_BOUNDS.len() + 1];
                for v in &obs {
                    let i =
                        DEFAULT_BOUNDS.iter().position(|b| v <= b).unwrap_or(DEFAULT_BOUNDS.len());
                    counts[i] += 1;
                }
                let d = Distribution {
                    bounds: DEFAULT_BOUNDS.to_vec(),
                    counts,
                    count: obs.len() as u64,
                    sum: Some(obs.iter().sum()),
                    min: obs.iter().copied().reduce(f64::min),
                    max: obs.iter().copied().reduce(f64::max),
                    exact: true,
                    observations: obs,
                };
                return (acc.labels, d, 0);
            }
            // The boundaries most points share win; the rest are reported.
            let total_points: usize =
                acc.bucketed.values().map(|(_, s)| s.values().map(Vec::len).sum::<usize>()).sum();
            let (bounds, streams) = acc
                .bucketed
                .into_values()
                .max_by_key(|(_, s)| s.values().map(Vec::len).sum::<usize>())
                .unwrap_or_default();
            let used: usize = streams.values().map(Vec::len).sum();
            let mut counts = vec![0u64; bounds.len() + 1];
            let mut sum = 0.0;
            for pts in streams.values() {
                let cumulative = pts.iter().any(|(_, c, _)| *c);
                if cumulative {
                    let (last, _, last_sum) = pts.last().cloned().unwrap_or_default();
                    let (first, _, first_sum) = pts.first().cloned().unwrap_or_default();
                    let grew = pts.len() > 1
                        && pts.windows(2).all(|w| w[0].0.iter().zip(&w[1].0).all(|(a, b)| a <= b));
                    for (i, c) in last.iter().enumerate() {
                        counts[i] += if grew { c - first[i] } else { *c };
                    }
                    sum += if grew { last_sum - first_sum } else { last_sum };
                } else {
                    for (cs, _, s) in pts {
                        for (i, c) in cs.iter().enumerate() {
                            counts[i] += c;
                        }
                        sum += s;
                    }
                }
            }
            let count = counts.iter().sum();
            let d = Distribution {
                bounds,
                counts,
                count,
                sum: Some(sum),
                min: None,
                max: None,
                exact: false,
                observations: Vec::new(),
            };
            (acc.labels, d, (total_points - used) as u64)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use time::OffsetDateTime;

    fn point(value: f64, buckets: Option<Value>, labels: Value, t: i64) -> MetricPoint {
        let at = OffsetDateTime::from_unix_timestamp(t)
            .unwrap()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap();
        serde_json::from_value(json!({
            "metric_id": format!("m{t}{value}"), "name": "latency", "kind": "histogram",
            "value": value, "timestamp": at,
            "labels": labels, "buckets": buckets, "correlation": { "project": "default" },
            "received_at": at
        }))
        .unwrap()
    }

    #[test]
    fn delta_points_add_up_and_quantiles_interpolate_inside_a_bucket() {
        let b =
            |c: [u64; 3]| Some(json!({ "bounds": [10, 100], "counts": c.map(|n| n.to_string()) }));
        let pts =
            [point(500.0, b([0, 10, 0]), json!({}), 1), point(500.0, b([0, 10, 0]), json!({}), 2)];
        let out = merge(&pts, &[]);
        let d = &out[0].1;
        assert_eq!(d.counts, vec![0, 20, 0]);
        assert_eq!(d.sum, Some(1000.0));
        assert!((d.quantile(0.5).unwrap() - 55.0).abs() < 1e-9, "{:?}", d.quantile(0.5));
    }

    #[test]
    fn cumulative_points_count_only_the_growth_in_the_window() {
        let b = |c: [u64; 2]| Some(json!({ "bounds": [1], "counts": c, "temporality": 2 }));
        let pts =
            [point(0.0, b([5, 5]), json!({"a": 1}), 1), point(0.0, b([7, 9]), json!({"a": 1}), 2)];
        assert_eq!(merge(&pts, &[])[0].1.counts, vec![2, 4]);
        // A restart: counts fell, so the last point stands alone.
        let pts =
            [point(0.0, b([9, 9]), json!({"a": 1}), 1), point(0.0, b([1, 2]), json!({"a": 1}), 2)];
        assert_eq!(merge(&pts, &[])[0].1.counts, vec![1, 2]);
    }

    #[test]
    fn observations_give_exact_quantiles_and_default_buckets() {
        let pts: Vec<MetricPoint> =
            (1..=100).map(|i| point(f64::from(i), None, json!({}), i64::from(i))).collect();
        let d = &merge(&pts, &[])[0].1;
        assert!(d.exact);
        assert_eq!(d.quantile(0.95), Some(95.0));
        assert_eq!(d.count, 100);
        assert_eq!(d.counts.iter().sum::<u64>(), 100);
    }

    #[test]
    fn series_split_on_the_group_by_labels_and_mismatched_bounds_are_reported() {
        let pts = [
            point(1.0, Some(json!({ "bounds": [1], "counts": [1, 0] })), json!({"model": "a"}), 1),
            point(1.0, Some(json!({ "bounds": [1], "counts": [1, 0] })), json!({"model": "b"}), 2),
            point(1.0, Some(json!({ "bounds": [2], "counts": [1, 0] })), json!({"model": "b"}), 3),
            point(1.0, Some(json!({ "bounds": [1], "counts": [0, 1] })), json!({"model": "b"}), 4),
        ];
        let out = merge(&pts, &["model".to_owned()]);
        assert_eq!(out.len(), 2);
        let b = out.iter().find(|(l, _, _)| l.as_ref().unwrap()["model"] == "b").unwrap();
        assert_eq!(b.1.counts, vec![1, 1]);
        assert_eq!(b.2, 1, "one point with other boundaries was left out");
    }
}
