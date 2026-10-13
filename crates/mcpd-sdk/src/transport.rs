//! HTTP transport: how a batch actually reaches mcpd.
//!
//! Three decisions live here and nowhere else.
//!
//! * **Compression.** Bodies above a kilobyte are gzipped. Prompts and
//!   completions are the bulk of what an agent sends and they compress by
//!   roughly an order of magnitude; below a kilobyte the header costs more than
//!   it saves.
//! * **Idempotency.** The `ingest_key` on a batch is fixed before the first
//!   attempt and reused by every retry, so a reply lost on the way back costs a
//!   duplicate request but never a duplicate span.
//! * **Which failures are worth repeating.** A 429 or a 5xx from a load
//!   balancer is a moment in time; a 400 is a malformed batch that will still be
//!   malformed in a second. Retrying the second kind is how a client turns one
//!   bad request into a self-inflicted flood.

use crate::{Config, Error};
use mcpd_core::{IngestBatch, IngestResult, StepConflict};
use rand::RngExt;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::io::Write;
use std::time::Duration;

/// Bodies at or above this size are gzipped.
const GZIP_THRESHOLD: usize = 1024;

/// The longest a `Retry-After` may park a batch. A server asking for an hour
/// would otherwise stall the queue until it overflows.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(30);

/// The HTTP client, plus the retry policy the contract fixes.
#[derive(Debug)]
pub(crate) struct Transport {
    http: reqwest::Client,
    endpoint: String,
    api_key: Option<String>,
    /// The configured request timeout, kept so a long poll can outlive it.
    timeout: Duration,
    max_attempts: u32,
    base_backoff: Duration,
}

/// What one attempt came to.
enum Outcome {
    Accepted(IngestResult),
    /// Worth another attempt, after an optional server-requested delay.
    Retry {
        after: Option<Duration>,
        why: String,
    },
    /// Not worth another attempt.
    Fatal(Error),
}

impl Transport {
    /// Build a transport from the client configuration.
    ///
    /// # Errors
    /// Returns [`Error::Config`] only if the platform will not give us a TLS
    /// stack or a resolver, which is a broken environment rather than a
    /// mistake in the caller's code.
    pub(crate) fn new(config: &Config) -> Result<Self, Error> {
        let http = reqwest::Client::builder()
            .timeout(config.timeout)
            .user_agent(concat!("mcpd-rust/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| Error::Config(format!("could not build an HTTP client: {e}")))?;
        Ok(Self {
            http,
            endpoint: config.endpoint.trim_end_matches('/').to_owned(),
            api_key: config.api_key.clone(),
            timeout: config.timeout,
            max_attempts: 3,
            base_backoff: Duration::from_millis(250),
        })
    }

    /// Absolute URL for an API path.
    pub(crate) fn url(&self, path: &str) -> String {
        format!("{}{}", self.endpoint, path)
    }

    /// A request with the API key attached, when one is configured.
    pub(crate) fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let req = self.http.request(method, self.url(path));
        match &self.api_key {
            Some(key) => req.bearer_auth(key),
            None => req,
        }
    }

    /// Send one batch, retrying the failures that are worth retrying.
    ///
    /// # Errors
    /// Returns the last error when every attempt failed. Callers on the
    /// instrumentation path swallow it; `flush` reports it.
    pub(crate) async fn send(&self, batch: &IngestBatch) -> Result<IngestResult, Error> {
        let body = serde_json::to_vec(batch)?;
        let (body, gzipped) = maybe_gzip(body)?;

        let mut last = String::from("no attempt was made");
        let mut delay: Option<Duration> = None;
        for attempt in 1..=self.max_attempts {
            if let Some(d) = delay.take() {
                tokio::time::sleep(d).await;
            }
            match self.attempt(&body, gzipped).await {
                Outcome::Accepted(result) => return Ok(result),
                Outcome::Fatal(e) => return Err(e),
                Outcome::Retry { after, why } => {
                    last = why;
                    // A server that said how long to wait knows better than our
                    // backoff curve does; a cap keeps a hostile or mistaken
                    // value from parking the queue until it overflows.
                    delay = Some(
                        after.map_or_else(|| self.backoff(attempt), |d| d.min(MAX_RETRY_AFTER)),
                    );
                }
            }
        }
        Err(Error::Export(format!(
            "gave up after {} attempts; last failure: {last}",
            self.max_attempts
        )))
    }

    /// Exponential backoff with full jitter on top.
    ///
    /// The jitter matters more than the curve: without it a fleet that all lost
    /// the same server comes back as one synchronised stampede.
    fn backoff(&self, attempt: u32) -> Duration {
        let base = self.base_backoff.saturating_mul(1u32 << (attempt - 1).min(10));
        let jitter = rand::rng().random_range(0.0..1.0);
        base + Duration::from_secs_f64(self.base_backoff.as_secs_f64() * jitter)
    }

    async fn attempt(&self, body: &[u8], gzipped: bool) -> Outcome {
        let mut req = self
            .request(reqwest::Method::POST, "/v1/ingest")
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body.to_vec());
        if gzipped {
            req = req.header(reqwest::header::CONTENT_ENCODING, "gzip");
        }

        let response = match req.send().await {
            Ok(r) => r,
            // A connection error is the case retrying exists for: the server is
            // restarting, the DNS entry has just moved, the socket was reset.
            Err(e) => return Outcome::Retry { after: None, why: e.to_string() },
        };

        let status = response.status();
        if status.is_success() {
            return match response.json::<IngestResult>().await {
                Ok(r) => Outcome::Accepted(r),
                // The batch landed; we simply cannot read the receipt. Saying so
                // and moving on beats sending it again.
                Err(e) => Outcome::Fatal(Error::Api {
                    status: status.as_u16(),
                    message: format!("mcpd accepted the batch but the reply was unreadable: {e}"),
                }),
            };
        }

        let retry_after = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<u64>().ok())
            .map(Duration::from_secs);
        let code = status.as_u16();
        let message = describe(status, response.text().await.ok());

        if matches!(code, 429 | 502 | 503 | 504) {
            Outcome::Retry { after: retry_after, why: message }
        } else {
            Outcome::Fatal(Error::Api { status: code, message })
        }
    }

    /// GET a path and decode the response.
    ///
    /// # Errors
    /// Returns [`Error::Api`] for a non-2xx reply and [`Error::Http`] when the
    /// request could not be made at all. Reads are not retried: a caller
    /// awaiting an answer would rather have the failure than the delay.
    pub(crate) async fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        params: &[(&str, String)],
    ) -> Result<T, Error> {
        let response = self.request(reqwest::Method::GET, path).query(params).send().await?;
        self.decode(response).await
    }

    /// POST a JSON body to a run endpoint and decode the answer.
    ///
    /// No retry: a worker call that is retried blindly can land twice, and
    /// the daemon's own 409 table is how the caller learns what happened.
    pub(crate) async fn post<B: Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        path: &str,
        params: &[(&str, String)],
        body: &B,
    ) -> Result<T, Error> {
        self.post_held(path, params, body, Duration::ZERO).await
    }

    /// [`Transport::post`] for a long poll.
    ///
    /// `wait` is how long the daemon may hold the request open before it
    /// answers. The request's timeout is that plus the configured timeout;
    /// otherwise the client would cancel a poll longer than its own timeout,
    /// which is every idle claim.
    pub(crate) async fn post_held<B: Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        path: &str,
        params: &[(&str, String)],
        body: &B,
        wait: Duration,
    ) -> Result<T, Error> {
        let response = self
            .request(reqwest::Method::POST, path)
            .query(params)
            .json(body)
            .timeout(self.timeout.saturating_add(wait))
            .send()
            .await?;
        self.decode(response).await
    }

    /// PUT a JSON body and decode the answer. Not retried, like a POST.
    pub(crate) async fn put<B: Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        path: &str,
        params: &[(&str, String)],
        body: &B,
    ) -> Result<T, Error> {
        let response =
            self.request(reqwest::Method::PUT, path).query(params).json(body).send().await?;
        self.decode(response).await
    }

    async fn decode<T: DeserializeOwned>(&self, response: reqwest::Response) -> Result<T, Error> {
        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.ok();
            if status == reqwest::StatusCode::CONFLICT {
                // The run surface answers a refused worker call with a closed
                // reason. Anything else that is 409 (a version already taken)
                // has no `reason` and stays an API error.
                if let Some(conflict) =
                    text.as_deref().and_then(|t| serde_json::from_str::<StepConflict>(t).ok())
                {
                    return Err(Error::Conflict(Box::new(conflict)));
                }
            }
            return Err(Error::Api { status: status.as_u16(), message: describe(status, text) });
        }
        Ok(response.json::<T>().await?)
    }
}

/// Gzip a body when it is large enough to be worth it.
fn maybe_gzip(body: Vec<u8>) -> Result<(Vec<u8>, bool), Error> {
    if body.len() < GZIP_THRESHOLD {
        return Ok((body, false));
    }
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder
        .write_all(&body)
        .and_then(|()| encoder.finish())
        .map(|compressed| (compressed, true))
        .map_err(|e| Error::Config(format!("could not gzip the batch: {e}")))
}

/// Turn a failed response into a sentence worth logging.
///
/// mcpd answers with `{"error":..,"message":..}`; anything else in front of it
/// (a proxy, a load balancer) will not, so fall back to the raw body.
fn describe(status: reqwest::StatusCode, body: Option<String>) -> String {
    let body = body.unwrap_or_default();
    let detail = serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| v.get("message").and_then(|m| m.as_str()).map(str::to_owned))
        .unwrap_or_else(|| body.chars().take(200).collect());
    if detail.trim().is_empty() {
        status.to_string()
    } else {
        format!("{status}: {detail}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transport() -> Transport {
        Transport::new(&Config::default()).unwrap()
    }

    #[test]
    fn small_bodies_are_sent_uncompressed() {
        let (body, gzipped) = maybe_gzip(b"{}".to_vec()).unwrap();
        assert!(!gzipped);
        assert_eq!(body, b"{}");
    }

    #[test]
    fn large_bodies_are_gzipped_and_shrink() {
        let raw = vec![b'a'; 8192];
        let (body, gzipped) = maybe_gzip(raw.clone()).unwrap();
        assert!(gzipped);
        assert!(body.len() < raw.len() / 2, "gzip did not help: {} bytes", body.len());
        assert_eq!(&body[..2], &[0x1f, 0x8b], "not a gzip stream");
    }

    #[test]
    fn backoff_grows_and_stays_inside_its_jitter_window() {
        let t = transport();
        for attempt in 1..4u32 {
            let base = Duration::from_millis(250) * (1 << (attempt - 1));
            let d = t.backoff(attempt);
            assert!(d >= base, "attempt {attempt} backed off less than its base");
            assert!(d < base + Duration::from_millis(250), "jitter exceeded one base interval");
        }
    }

    #[test]
    fn an_mcpd_error_body_is_reported_by_its_message() {
        let d = describe(
            reqwest::StatusCode::BAD_REQUEST,
            Some(r#"{"error":"bad_parameter","message":"limit=\"x\" is not a number"}"#.to_owned()),
        );
        assert!(d.contains("is not a number"), "{d}");
    }

    #[test]
    fn a_body_from_something_that_is_not_mcpd_still_reads() {
        let d = describe(reqwest::StatusCode::BAD_GATEWAY, Some("<html>nginx</html>".to_owned()));
        assert!(d.contains("nginx"), "{d}");
        let empty = describe(reqwest::StatusCode::BAD_GATEWAY, None);
        assert!(empty.contains("502"), "{empty}");
    }

    #[test]
    fn paths_are_joined_without_doubling_the_slash() {
        let config = Config { endpoint: "http://localhost:4317/".to_owned(), ..Config::default() };
        let t = Transport::new(&config).unwrap();
        assert_eq!(t.url("/v1/ingest"), "http://localhost:4317/v1/ingest");
    }
}
