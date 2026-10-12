//! One attempt to hand a request to another system, and what its answer means.
//!
//! Shared by forwarding and the relay, so both read an answer the same way:
//! a success is final, a refusal that will not change is final, and anything
//! that might be different on the next attempt is retried. What counts as
//! "might be different" is wider here than in an SDK, because the work is in a
//! spool rather than in a caller's memory: a 401 after a key rotation, or a 404
//! from a proxy that is being redeployed, is held and retried rather than
//! thrown away, and the destination's health says so while it lasts.

use std::time::Duration;

/// What one attempt came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Accepted.
    Delivered,
    /// Not accepted this time. `after` is how long the destination asked for.
    Retry {
        /// What happened, without the URL.
        why: String,
        /// The wait a `Retry-After` header asked for, capped.
        after: Option<Duration>,
    },
    /// Refused in a way no retry changes: the body is wrong for this
    /// destination.
    Refused {
        /// What happened, without the URL.
        why: String,
    },
}

/// The longest wait a `Retry-After` is honoured for. The work is on disk, so
/// a long wait costs nothing but delay, but a destination asking for a day is
/// more likely confused than serious.
pub const RETRY_AFTER_CAP: Duration = Duration::from_secs(300);

/// The first retry wait. Doubles each time, to [`BACKOFF_CAP`].
const BACKOFF_BASE: Duration = Duration::from_millis(500);

/// The longest wait between retries when the destination named none.
pub const BACKOFF_CAP: Duration = Duration::from_secs(60);

/// The wait before retry number `attempt`, counted from zero: exponential to a
/// cap, then a random point in its upper half.
///
/// The jitter is the part that matters at scale. Every relay in a region loses
/// the same upstream at the same moment, and without it they all come back at
/// the same moment too, as a burst the recovering upstream answers with the
/// 503 that started it.
#[must_use]
pub fn backoff(attempt: u32) -> Duration {
    let ceiling = BACKOFF_BASE.saturating_mul(1u32 << attempt.min(16)).min(BACKOFF_CAP);
    let half = ceiling / 2;
    half + half.mul_f64(rand::random::<f64>())
}

/// The wait a `Retry-After` header asks for, in seconds or as an HTTP date,
/// capped at [`RETRY_AFTER_CAP`].
#[must_use]
pub fn retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    let raw = headers.get(reqwest::header::RETRY_AFTER)?.to_str().ok()?.trim();
    if let Ok(secs) = raw.parse::<u64>() {
        return Some(Duration::from_secs(secs).min(RETRY_AFTER_CAP));
    }
    let format = time::macros::format_description!(
        "[weekday repr:short], [day] [month repr:short] [year] [hour]:[minute]:[second] GMT"
    );
    let at = time::PrimitiveDateTime::parse(raw, format).ok()?.assume_utc();
    let wait = at - time::OffsetDateTime::now_utc();
    Some(Duration::try_from(wait).unwrap_or(Duration::ZERO).min(RETRY_AFTER_CAP))
}

/// Whether an HTTP status is worth another attempt.
#[must_use]
pub fn retryable(status: reqwest::StatusCode) -> bool {
    status.is_server_error() || matches!(status.as_u16(), 401 | 403 | 404 | 408 | 409 | 425 | 429)
}

/// POST a body over HTTP.
pub async fn post(
    client: &reqwest::Client,
    url: &str,
    headers: &[(String, String)],
    content_type: &str,
    body: Vec<u8>,
    gzip: bool,
) -> Outcome {
    let mut req = client.post(url).header(reqwest::header::CONTENT_TYPE, content_type);
    for (k, v) in headers {
        req = req.header(k, v);
    }
    // OTLP receivers are required to accept gzip, and a batch of prompts
    // compresses several times over, which is the difference that matters on
    // a link between regions.
    let body = if gzip && body.len() > 1024 {
        match gzipped(&body) {
            Some(z) => {
                req = req.header(reqwest::header::CONTENT_ENCODING, "gzip");
                z
            }
            None => body,
        }
    } else {
        body
    };
    match req.body(body).send().await {
        Ok(res) if res.status().is_success() => Outcome::Delivered,
        Ok(res) if retryable(res.status()) => Outcome::Retry {
            why: format!("answered {}", res.status()),
            after: retry_after(res.headers()),
        },
        Ok(res) => {
            let status = res.status();
            // The first line of what the destination said, which is usually
            // the reason, bounded so a large error page cannot flood the log.
            let said = res.text().await.unwrap_or_default();
            let said: String = said.lines().next().unwrap_or("").chars().take(200).collect();
            Outcome::Refused {
                why: if said.is_empty() {
                    format!("answered {status}")
                } else {
                    format!("answered {status}: {said}")
                },
            }
        }
        // Without the URL: the error repeats it, query and all.
        Err(e) => Outcome::Retry {
            why: format!("could not be reached: {}", crate::outbound_error(e)),
            after: None,
        },
    }
}

fn gzipped(body: &[u8]) -> Option<Vec<u8>> {
    use std::io::Write;
    let mut z = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    z.write_all(body).ok()?;
    z.finish().ok()
}

/// gRPC status codes OTLP says to retry, plus the two that a key rotation
/// produces, for the reason the module comment gives.
fn grpc_retryable(code: u32) -> bool {
    matches!(code, 1 | 4 | 7 | 8 | 10 | 11 | 14 | 15 | 16)
}

/// Export over gRPC: one unary call, the protobuf body in gRPC's
/// length-prefixed framing, and the answer read from `grpc-status`, which a
/// server puts in the trailers or, when it fails before sending a body, in
/// the headers.
pub async fn post_grpc(
    client: &reqwest::Client,
    url: &str,
    headers: &[(String, String)],
    body: &[u8],
) -> Outcome {
    use http_body_util::BodyExt;

    let Ok(len) = u32::try_from(body.len()) else {
        return Outcome::Refused { why: "the batch is larger than gRPC can frame".to_owned() };
    };
    let mut framed = Vec::with_capacity(body.len() + 5);
    framed.push(0);
    framed.extend_from_slice(&len.to_be_bytes());
    framed.extend_from_slice(body);

    let mut req = client
        .post(url)
        .header(reqwest::header::CONTENT_TYPE, "application/grpc")
        .header("te", "trailers");
    for (k, v) in headers {
        req = req.header(k, v);
    }
    let res = match req.body(framed).send().await {
        Ok(res) => res,
        Err(e) => {
            return Outcome::Retry {
                why: format!("could not be reached: {}", crate::outbound_error(e)),
                after: None,
            }
        }
    };
    let status = res.status();
    if !status.is_success() {
        return if retryable(status) {
            Outcome::Retry {
                why: format!("answered HTTP {status}"),
                after: retry_after(res.headers()),
            }
        } else {
            Outcome::Refused { why: format!("answered HTTP {status}") }
        };
    }
    let header_status = grpc_status(res.headers());
    let res: axum::http::Response<reqwest::Body> = res.into();
    let (code, message) = match header_status {
        Some(s) => s,
        None => match res.into_body().collect().await {
            Ok(collected) => match collected.trailers().and_then(grpc_status_http) {
                Some(s) => s,
                None => {
                    return Outcome::Retry {
                        why: "answered with no grpc-status".to_owned(),
                        after: None,
                    }
                }
            },
            Err(e) => {
                return Outcome::Retry {
                    why: format!("the answer could not be read: {}", crate::outbound_error(e)),
                    after: None,
                }
            }
        },
    };
    match code {
        0 => Outcome::Delivered,
        c if grpc_retryable(c) => {
            Outcome::Retry { why: format!("answered gRPC status {c}{message}"), after: None }
        }
        c => Outcome::Refused { why: format!("answered gRPC status {c}{message}") },
    }
}

fn grpc_status(h: &reqwest::header::HeaderMap) -> Option<(u32, String)> {
    let code = h.get("grpc-status")?.to_str().ok()?.trim().parse().ok()?;
    let msg = h
        .get("grpc-message")
        .and_then(|v| v.to_str().ok())
        .map(|m| format!(": {}", m.chars().take(200).collect::<String>()))
        .unwrap_or_default();
    Some((code, msg))
}

fn grpc_status_http(h: &axum::http::HeaderMap) -> Option<(u32, String)> {
    let code = h.get("grpc-status")?.to_str().ok()?.trim().parse().ok()?;
    let msg = h
        .get("grpc-message")
        .and_then(|v| v.to_str().ok())
        .map(|m| format!(": {}", m.chars().take(200).collect::<String>()))
        .unwrap_or_default();
    Some((code, msg))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_retry_after_in_seconds_or_as_a_date_is_honoured_and_capped() {
        let mut h = reqwest::header::HeaderMap::new();
        assert_eq!(retry_after(&h), None);
        h.insert(reqwest::header::RETRY_AFTER, "2".parse().unwrap());
        assert_eq!(retry_after(&h), Some(Duration::from_secs(2)));
        h.insert(reqwest::header::RETRY_AFTER, "86400".parse().unwrap());
        assert_eq!(retry_after(&h), Some(RETRY_AFTER_CAP));
        // A date in the past asks for no wait at all.
        h.insert(reqwest::header::RETRY_AFTER, "Wed, 21 Oct 2015 07:28:00 GMT".parse().unwrap());
        assert_eq!(retry_after(&h), Some(Duration::ZERO));
        let soon = time::OffsetDateTime::now_utc() + time::Duration::seconds(30);
        let format = time::macros::format_description!(
            "[weekday repr:short], [day] [month repr:short] [year] [hour]:[minute]:[second] GMT"
        );
        h.insert(reqwest::header::RETRY_AFTER, soon.format(format).unwrap().parse().unwrap());
        let got = retry_after(&h).expect("a date is read");
        assert!(got <= Duration::from_secs(30) && got >= Duration::from_secs(28), "{got:?}");
    }

    #[test]
    fn backoff_grows_to_its_cap_and_stays_in_the_upper_half() {
        for attempt in 0..30 {
            let ceiling = BACKOFF_BASE.saturating_mul(1u32 << attempt.min(16)).min(BACKOFF_CAP);
            let b = backoff(attempt);
            assert!(b >= ceiling / 2 && b <= ceiling, "attempt {attempt}: {b:?}");
        }
    }

    #[test]
    fn which_statuses_are_retried() {
        for s in [401u16, 403, 404, 408, 429, 500, 502, 503] {
            assert!(retryable(reqwest::StatusCode::from_u16(s).unwrap()), "{s}");
        }
        for s in [400u16, 413, 415, 422] {
            assert!(!retryable(reqwest::StatusCode::from_u16(s).unwrap()), "{s}");
        }
    }
}
