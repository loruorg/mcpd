//! The layers around every route.
//!
//! Five things apply to a request whatever route it reaches, and they live here
//! rather than in `lib.rs` so the router stays a list of routes: the body
//! limit, the request timeout, the origin policy on the query API, the answer
//! to a handler that panicked, and the counter of server errors.
//!
//! Each refusal leaves in the same JSON shape a handler's does. The body-limit
//! and timeout layers from `tower-http` answer with an empty body, and a client
//! written against `error.rs`, which promises `error` and `message` on every
//! failure, would parse nothing and report nothing useful.

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tower_http::cors::{AllowOrigin, CorsLayer};

/// Whether a response already carries mcpd's own error body.
///
/// A handler's refusal is JSON and is left alone. The layers' refusals carry
/// no content type at all, which is how the two are told apart.
fn carries_json(res: &Response) -> bool {
    res.headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("application/json"))
}

/// Turn the body-limit layer's empty 413 into a refusal that names the limit.
///
/// The declared length is read before the request goes in, because the layer
/// consumes the request. A body sent without a length is refused as the reading
/// crosses the limit, and the message says only that it did.
pub async fn refuse_oversize(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Response {
    let declared = req
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    let res = next.run(req).await;
    if res.status() != StatusCode::PAYLOAD_TOO_LARGE || carries_json(&res) {
        return res;
    }
    ApiError::TooLarge(oversize_message(declared, state.config.max_body_bytes)).into_response()
}

/// The message for a body over the limit.
fn oversize_message(declared: Option<u64>, limit: usize) -> String {
    match declared {
        Some(n) => format!(
            "request body is {n} bytes and this server accepts {limit} bytes per request; \
             send fewer signals per request"
        ),
        None => format!(
            "request body is larger than the {limit} bytes this server accepts per request; \
             send fewer signals per request"
        ),
    }
}

/// Turn the timeout layer's empty 408 into a refusal that names the deadline.
///
/// It leaves as a 503, not the 408 the layer produced. A 408 tells the client
/// it was slow to send, and the deadline here covers the handler as well as
/// the upload: a storage backend that accepts a connection and never answers
/// produces exactly this, and the client did nothing wrong. A 503 is what the
/// SDKs retry, and it counts as the server error it is.
pub async fn answer_for_timeout(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Response {
    let res = next.run(req).await;
    if res.status() != StatusCode::REQUEST_TIMEOUT || carries_json(&res) {
        return res;
    }
    ApiError::Timeout(format!(
        "the request took longer than the {}s this server allows; retry it, and raise \
         --request-timeout-seconds if the operation needs longer",
        state.config.request_timeout_seconds
    ))
    .into_response()
}

/// What a panic carried, as text a log line can hold.
///
/// `panic!` with a literal carries a `&str` and with arguments a `String`;
/// anything else is a payload nobody wrote to be read.
#[must_use]
pub fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_owned()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "a panic with no message".to_owned()
    }
}

/// Answer a request whose handler panicked, in the JSON shape every other
/// failure leaves in.
///
/// The panic's own text goes to the log and not to the client: it names code
/// paths and values from inside the daemon, and the person who can act on it
/// is the operator. The client is told what it can do, which is retry.
pub fn answer_for_panic(payload: Box<dyn std::any::Any + Send + 'static>) -> Response {
    tracing::error!(panic = %panic_message(&*payload), "a request handler panicked");
    ApiError::Internal(
        "mcpd hit a fault inside the daemon while handling this request, and the request was \
         not completed; the fault is in the daemon's log. Retry it, and report it if it repeats"
            .to_owned(),
    )
    .into_response()
}

/// Count every response that left as a server error.
///
/// Outermost, so it sees what the client sees: a 500 from a handler and a 503
/// from the timeout layer both count, and a 5xx that a layer turned into
/// something else does not.
pub async fn count_server_errors(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Response {
    let res = next.run(req).await;
    if res.status().is_server_error() {
        state.metrics.errors.fetch_add(1, Ordering::Relaxed);
    }
    res
}

/// The origin policy for the query API, or `None` when no origin is allowed.
///
/// `None` rather than a layer that allows nothing, because there is nothing to
/// add: a response with no `access-control-allow-origin` header is one a
/// browser refuses to hand to a page on another origin, and a same-origin page,
/// which is what the bundled interface is, never asks. `*` is the old
/// behaviour, every origin, and is only ever there because somebody typed it.
#[must_use]
pub fn cors_for(origins: &[String]) -> Option<CorsLayer> {
    if origins.is_empty() {
        return None;
    }
    if origins.iter().any(|o| o == "*") {
        return Some(CorsLayer::permissive());
    }
    let list: Vec<HeaderValue> = origins
        .iter()
        .filter_map(|o| match HeaderValue::from_str(o) {
            Ok(v) => Some(v),
            Err(_) => {
                // `check_origin` refuses this at start-up, so it is only
                // reachable by building a config without going through it.
                tracing::warn!(origin = %o, "not a usable origin; it will not be allowed");
                None
            }
        })
        .collect();
    Some(
        CorsLayer::new()
            .allow_origin(AllowOrigin::list(list))
            .allow_methods([
                axum::http::Method::GET,
                axum::http::Method::POST,
                axum::http::Method::DELETE,
            ])
            .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
            .max_age(std::time::Duration::from_secs(3600)),
    )
}

/// Whether a string is an origin a browser would send.
///
/// An origin is a scheme and a host, with a port when there is one, and
/// nothing after: `https://console.example`, not `https://console.example/`.
/// Checked at start-up so a typo is a refusal to start with the flag named,
/// rather than an interface that can never connect and a daemon that says
/// nothing about why.
pub fn check_origin(origin: &str) -> Result<(), String> {
    if origin == "*" {
        return Ok(());
    }
    let Some((scheme, host)) = origin.split_once("://") else {
        return Err(format!(
            "{origin:?} is not an origin; an origin is a scheme and a host, such as \
             https://console.example, or * for any origin"
        ));
    };
    if scheme != "http" && scheme != "https" {
        return Err(format!("{origin:?} is not an origin; the scheme must be http or https"));
    }
    if host.is_empty() || host.contains('/') || host.contains('?') || host.contains('#') {
        return Err(format!(
            "{origin:?} is not an origin; an origin has no path, so drop everything after \
             the host and port"
        ));
    }
    // A host is letters, digits, dots and dashes, a port after a colon, and
    // brackets around an IPv6 address. Anything else, a space or a user name
    // before an `@`, is a value no browser will ever send as its origin.
    let host_char = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | ':' | '[' | ']');
    if !host.chars().all(host_char) || HeaderValue::from_str(origin).is_err() {
        return Err(format!(
            "{origin:?} is not an origin; the host holds a character a host name cannot"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_origin_is_a_scheme_and_a_host_and_nothing_after() {
        assert!(check_origin("https://console.example").is_ok());
        assert!(check_origin("http://localhost:4321").is_ok());
        assert!(check_origin("*").is_ok());
        for bad in
            ["console.example", "https://console.example/", "ftp://x", "https://", "https://a b"]
        {
            let err = check_origin(bad).expect_err(bad);
            assert!(err.contains(bad), "{err}");
        }
    }

    #[test]
    fn no_origins_means_no_layer_and_a_star_means_every_origin() {
        assert!(cors_for(&[]).is_none());
        assert!(cors_for(&["*".to_owned()]).is_some());
        assert!(cors_for(&["https://console.example".to_owned()]).is_some());
    }

    #[test]
    fn the_oversize_message_names_the_size_when_it_was_declared() {
        assert!(oversize_message(Some(3_000), 1_024).contains("3000 bytes"));
        assert!(oversize_message(Some(3_000), 1_024).contains("1024 bytes"));
        assert!(oversize_message(None, 1_024).contains("larger than the 1024 bytes"));
    }
}
