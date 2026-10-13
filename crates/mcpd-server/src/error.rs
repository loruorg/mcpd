//! HTTP error mapping.
//!
//! Every failure leaves as JSON with a machine-readable `error` code and a
//! message written for whoever has to fix it. An error that only says "500"
//! costs the reader an hour.

use axum::http::header::{HeaderName, HeaderValue};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

/// The body of every non-2xx response.
#[derive(Debug, Serialize)]
pub struct ApiErrorBody {
    /// A stable, machine-readable code.
    pub error: &'static str,
    /// What went wrong, in terms the caller can act on.
    pub message: String,
    /// Which field or parameter was at fault, when there is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    /// The storage capability this deployment does not have, on a 501.
    ///
    /// Named exactly as the field carrying it in `GET /v1/api/capabilities`,
    /// so a caller that met this refusal can look the same word up rather than
    /// translate between two vocabularies. Absent on every other status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capability: Option<String>,
}

/// A refusal for a spent ceiling, and how long until it opens again.
///
/// The two travel together because they are produced together: only the code
/// holding the counter can say when it next admits a signal, and by the time
/// the refusal reaches [`IntoResponse`] that counter is out of reach. Building
/// the pair at the counter is what stops `Retry-After` being a constant chosen
/// somewhere else.
#[derive(Debug)]
pub struct QuotaRefusal {
    /// What happened, and what to do about it.
    pub message: String,
    /// Whole seconds until the ceiling would admit one more signal, which is
    /// what `Retry-After` carries. Never zero: a wait of zero is an invitation
    /// to retry into the same refusal without pausing.
    pub retry_after_seconds: u64,
}

impl std::fmt::Display for QuotaRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// Anything the API can refuse to do.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// The request was malformed.
    #[error("{0}")]
    BadRequest(String),

    /// A specific parameter was malformed.
    #[error("{message}")]
    BadParam {
        /// Which parameter.
        field: String,
        /// What was wrong with it.
        message: String,
    },

    /// No credential, or one no configured key list contains.
    ///
    /// Kept distinct from [`ApiError::Forbidden`] because the two mean
    /// different things to whoever is holding the key: a 401 says "you have not
    /// identified yourself", and retrying with the same key will fail the same
    /// way, while a 403 says "the key works, it is not permitted here", which is
    /// a configuration question rather than a credential one.
    #[error("{0}")]
    Unauthorized(String),

    /// A valid key, without the scope or the project reach this endpoint needs.
    #[error("{0}")]
    Forbidden(String),

    /// The thing asked for does not exist.
    #[error("{0}")]
    NotFound(String),

    /// The payload was larger than the configured limit.
    #[error("{0}")]
    TooLarge(String),

    /// The project has spent its ingest ceiling for this window.
    ///
    /// A 429 rather than a 403, because the key is permitted here and the
    /// answer changes on its own once the window rolls. A sender that reads it
    /// as a permission problem would stop retrying for good, which is the
    /// opposite of what a ceiling wants.
    ///
    /// Carries the wait as well as the sentence, so the answer says *when* the
    /// ceiling opens rather than leaving the sender to guess. See
    /// [`QuotaRefusal`].
    #[error("{0}")]
    TooManyRequests(QuotaRefusal),

    /// Too many sign-up or log-in attempts from one address or for one email.
    /// A 429 with `Retry-After`, under its own code so a client does not read
    /// it as a project's signal ceiling.
    #[error("{0}")]
    RateLimited(QuotaRefusal),

    /// An account with that email already exists.
    #[error("{0}")]
    AccountExists(String),

    /// The password was right and the account has two-step log-in on, but no
    /// code, or a wrong one, came with it. A 401 under its own code, so a client
    /// knows to ask for the code rather than the password again.
    #[error("{0}")]
    TotpRequired(String),

    /// The password was right, and an organisation the account belongs to
    /// requires single sign-on for its address.
    #[error("{0}")]
    SsoRequired(String),

    /// The password was right, and the address is not confirmed yet.
    #[error("{0}")]
    EmailUnverified(String),

    /// The request is valid and conflicts with what is stored: the last owner
    /// of an organisation, a domain another organisation claims.
    #[error("{0}")]
    Conflict(String),

    /// The request ran past the configured deadline.
    ///
    /// A 503 rather than a 408: the deadline covers the handler as well as the
    /// upload, so the client usually did nothing slow, and a 503 is the status
    /// the SDKs retry. See `layers::answer_for_timeout`.
    #[error("{0}")]
    Timeout(String),

    /// A worker call refused with one of the four closed reasons of the run
    /// contract. The whole body is the answer: a worker that cannot tell a
    /// fence from its own landed call either abandons finished work or reports
    /// a failure for a success.
    #[error("{}", .0.message)]
    StepConflict(Box<mcpd_core::StepConflict>),

    /// A named definition version is already held by a different body.
    #[error("{0}")]
    VersionTaken(String),

    /// A single record was refused because the trace it names is not recorded
    /// here: the sampler discarded that trace, so the record has nothing to
    /// attach to.
    ///
    /// A 422 rather than a 404, because the route and the record are both
    /// well formed, and rather than a silent success, because a door that
    /// answers "stored" for a row it dropped is the dishonest option. Storing
    /// it anyway is the other dishonest option: it would leave a score
    /// pointing at a trace no reader can open, which is the inconsistency the
    /// sampling pass exists to prevent.
    #[error("{0}")]
    SampledOut(String),

    /// The daemon is not serving this surface. A 503 rather than a 404,
    /// because the route exists and the operator can turn the surface on.
    #[error("{0}")]
    Unavailable(String),

    /// The storage backend behind this daemon does not implement what the
    /// endpoint needs.
    ///
    /// A 501 rather than a 503, and the difference is who can act on it.
    /// [`ApiError::Unavailable`] is a surface the operator can turn on, so
    /// retrying later can succeed. This one cannot change without changing the
    /// backend, so a client that retries it retries for ever.
    ///
    /// It carries the capability's name, which is the same word
    /// [`mcpd_store::StoreCapabilities`] uses for it and the same word
    /// `GET /v1/api/capabilities` publishes. That is what makes the refusal
    /// answerable: the caller reads the capability set once and stops asking,
    /// instead of learning the difference between two backends from an empty
    /// list.
    #[error("{message}")]
    NotImplemented {
        /// The capability, named as its field in the capability set.
        capability: String,
        /// What this backend cannot do, and why.
        message: String,
    },

    /// Storage failed.
    #[error("{0}")]
    Storage(#[from] mcpd_store::Error),

    /// Anything else.
    #[error("{0}")]
    Internal(String),
}

impl ApiError {
    /// The headers this refusal carries beyond its body.
    ///
    /// Here rather than at each handler that raises the error, so a refusal
    /// cannot leave without its header because one door forgot. `Retry-After`
    /// is part of what a 429 means to every SDK, and two doors produce one. A
    /// variant that needs a header of its own adds an arm.
    fn headers(&self) -> Vec<(HeaderName, HeaderValue)> {
        match self {
            Self::TooManyRequests(refusal) | Self::RateLimited(refusal) => {
                vec![(header::RETRY_AFTER, HeaderValue::from(refusal.retry_after_seconds))]
            }
            // One second: the queue it was refused from drains in less than
            // that when the writer is healthy, and a client that waits longer
            // only delays its own data.
            Self::Storage(mcpd_store::Error::Busy(_)) => {
                vec![(header::RETRY_AFTER, HeaderValue::from(1_u32))]
            }
            _ => Vec::new(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        if let Self::StepConflict(conflict) = self {
            return (StatusCode::CONFLICT, Json(*conflict)).into_response();
        }
        let (status, code, field, capability) = match &self {
            Self::BadRequest(_) => (StatusCode::BAD_REQUEST, "bad_request", None, None),
            Self::BadParam { field, .. } => {
                (StatusCode::BAD_REQUEST, "bad_parameter", Some(field.clone()), None)
            }
            Self::Unauthorized(_) => (StatusCode::UNAUTHORIZED, "unauthorized", None, None),
            Self::Forbidden(_) => (StatusCode::FORBIDDEN, "forbidden", None, None),
            Self::NotFound(_) => (StatusCode::NOT_FOUND, "not_found", None, None),
            Self::TooLarge(_) => (StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large", None, None),
            Self::TooManyRequests(_) => (StatusCode::TOO_MANY_REQUESTS, "over_quota", None, None),
            Self::RateLimited(_) => (StatusCode::TOO_MANY_REQUESTS, "rate_limited", None, None),
            Self::AccountExists(_) => (StatusCode::CONFLICT, "account_exists", None, None),
            Self::TotpRequired(_) => (StatusCode::UNAUTHORIZED, "totp_required", None, None),
            Self::SsoRequired(_) => (StatusCode::FORBIDDEN, "sso_required", None, None),
            Self::EmailUnverified(_) => (StatusCode::FORBIDDEN, "email_unverified", None, None),
            Self::Conflict(_) => (StatusCode::CONFLICT, "conflict", None, None),
            Self::Timeout(_) => (StatusCode::SERVICE_UNAVAILABLE, "timeout", None, None),
            Self::StepConflict(_) => (StatusCode::CONFLICT, "conflict", None, None),
            Self::VersionTaken(_) => (StatusCode::CONFLICT, "version_taken", None, None),
            Self::SampledOut(_) => (StatusCode::UNPROCESSABLE_ENTITY, "sampled_out", None, None),
            Self::Unavailable(_) => (StatusCode::SERVICE_UNAVAILABLE, "unavailable", None, None),
            Self::NotImplemented { capability, .. } => {
                (StatusCode::NOT_IMPLEMENTED, "not_implemented", None, Some(capability.clone()))
            }
            Self::Storage(e) => match e {
                mcpd_store::Error::NotFound { .. } => {
                    (StatusCode::NOT_FOUND, "not_found", None, None)
                }
                mcpd_store::Error::InvalidQuery(_) => {
                    (StatusCode::BAD_REQUEST, "invalid_query", None, None)
                }
                mcpd_store::Error::Conflict(_) => (StatusCode::CONFLICT, "id_taken", None, None),
                mcpd_store::Error::Busy(_) => {
                    (StatusCode::SERVICE_UNAVAILABLE, "store_busy", None, None)
                }
                // The same 501 as the variant above, because it is the same
                // fact arriving by the other route: a handler that checked the
                // capability first raises `NotImplemented`, and one that asked
                // the store anyway gets this. Mapping it to a 500 would report
                // a healthy backend answering a well-formed request as a fault
                // in the daemon.
                mcpd_store::Error::Unsupported { capability, .. } => (
                    StatusCode::NOT_IMPLEMENTED,
                    "not_implemented",
                    None,
                    Some((*capability).to_owned()),
                ),
                _ => (StatusCode::INTERNAL_SERVER_ERROR, "storage_error", None, None),
            },
            Self::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error", None, None),
        };

        // A 5xx is our fault and belongs in the log; a 4xx is the caller's and
        // does not, or a scanner fills the disk.
        if status.is_server_error() {
            tracing::error!(error = %self, "request failed");
        }

        let headers = self.headers();
        let mut response = (
            status,
            Json(ApiErrorBody { error: code, message: self.to_string(), field, capability }),
        )
            .into_response();
        for (name, value) in headers {
            response.headers_mut().insert(name, value);
        }
        response
    }
}

/// Convenience alias for handler results.
pub type ApiResult<T> = std::result::Result<T, ApiError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_write_queue_is_a_503_that_says_when_to_retry() {
        let r = ApiError::Storage(mcpd_store::Error::Busy("queue full".into())).into_response();
        assert_eq!(r.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(r.headers().get(header::RETRY_AFTER).and_then(|v| v.to_str().ok()), Some("1"));
    }

    #[test]
    fn an_id_another_owner_holds_is_a_409() {
        let r = ApiError::Storage(mcpd_store::Error::Conflict("taken".into())).into_response();
        assert_eq!(r.status(), StatusCode::CONFLICT);
    }
}
