//! gRPC ingest, on the same listener as everything else.
//!
//! Four services are served:
//!
//! * `opentelemetry.proto.collector.trace.v1.TraceService/Export`
//! * `opentelemetry.proto.collector.logs.v1.LogsService/Export`
//! * `opentelemetry.proto.collector.metrics.v1.MetricsService/Export`
//! * `jaeger.api_v2.CollectorService/PostSpans`
//!
//! A gRPC call is an HTTP/2 `POST` to a path named after the service and the
//! method, so these are ordinary routes in the one router, behind the same
//! layers as every HTTP door. The listener speaks HTTP/1.1 and cleartext
//! HTTP/2 on the same port and tells them apart by the connection preface, so
//! an OpenTelemetry exporter set to gRPC and one set to HTTP both point at
//! the same address. Behind a proxy that terminates TLS, the proxy forwards
//! HTTP/2 to the daemon in the clear.
//!
//! Every call is unary, so the framing is a five-byte prefix on one message
//! each way and the status in the trailers. It is written here, around the
//! same hand-written protobuf readers the HTTP doors use, rather than through
//! generated service code: the message bytes are the bytes `POST /v1/traces`
//! takes as `application/x-protobuf`, and decoding them a second way is how
//! the two transports would drift apart. The key comes from the call's
//! metadata, which is its HTTP/2 headers, so `authorization: Bearer <key>` and
//! `x-mcpd-key` work as they do over HTTP. From there a call takes exactly the
//! road of the matching HTTP door: project pinning for an account's key,
//! ceilings, idempotency, sampling and redaction.

use crate::auth::{authorize, Grant, Scope};
use crate::error::ApiError;
use crate::ingest::{self, Signal};
use crate::state::AppState;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use http_body_util::BodyExt;
use std::sync::atomic::Ordering;
use std::sync::Arc;

/// The OTLP trace service's one method.
pub const OTLP_TRACES: &str = "/opentelemetry.proto.collector.trace.v1.TraceService/Export";
/// The OTLP logs service's one method.
pub const OTLP_LOGS: &str = "/opentelemetry.proto.collector.logs.v1.LogsService/Export";
/// The OTLP metrics service's one method.
pub const OTLP_METRICS: &str = "/opentelemetry.proto.collector.metrics.v1.MetricsService/Export";
/// The Jaeger collector's span method.
pub const JAEGER_POST_SPANS: &str = "/jaeger.api_v2.CollectorService/PostSpans";

/// gRPC status codes this module answers with.
mod code {
    pub const OK: u8 = 0;
    pub const INVALID_ARGUMENT: u8 = 3;
    pub const NOT_FOUND: u8 = 5;
    pub const ALREADY_EXISTS: u8 = 6;
    pub const PERMISSION_DENIED: u8 = 7;
    pub const RESOURCE_EXHAUSTED: u8 = 8;
    pub const FAILED_PRECONDITION: u8 = 9;
    pub const UNIMPLEMENTED: u8 = 12;
    pub const INTERNAL: u8 = 13;
    pub const UNAVAILABLE: u8 = 14;
    pub const UNAUTHENTICATED: u8 = 16;
}

/// Whether a request is a gRPC call, from its content type.
fn is_grpc(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("application/grpc"))
}

/// gRPC percent-encodes `grpc-message`: every byte outside printable ASCII,
/// and `%` itself.
fn percent_encode(message: &str) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(message.len());
    for b in message.bytes() {
        if (0x20..=0x7e).contains(&b) && b != b'%' {
            out.push(b as char);
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

/// A call that ends without a message: the status in the headers, which gRPC
/// calls a trailers-only response.
fn status(code: u8, message: &str) -> Response {
    let mut res = Response::new(Body::empty());
    let h = res.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/grpc"));
    h.insert("grpc-accept-encoding", HeaderValue::from_static("gzip"));
    h.insert("grpc-status", HeaderValue::from(u16::from(code)));
    if let Ok(v) = HeaderValue::from_str(&percent_encode(message)) {
        h.insert("grpc-message", v);
    }
    res
}

/// A call that succeeded: one uncompressed message, then `grpc-status: 0` in
/// the trailers.
fn reply(message: Vec<u8>) -> Response {
    let mut frame = Vec::with_capacity(5 + message.len());
    frame.push(0);
    frame.extend_from_slice(&u32::try_from(message.len()).unwrap_or(u32::MAX).to_be_bytes());
    frame.extend_from_slice(&message);
    let mut trailers = HeaderMap::new();
    trailers.insert("grpc-status", HeaderValue::from(u16::from(code::OK)));
    let body = http_body_util::Full::new(Bytes::from(frame))
        .with_trailers(async move { Some(Ok(trailers)) });
    let mut res = Response::new(Body::new(body));
    let h = res.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/grpc"));
    h.insert("grpc-accept-encoding", HeaderValue::from_static("gzip"));
    res
}

/// The gRPC status for a refusal the HTTP doors would give.
///
/// Read from the HTTP status the same error carries, so a new variant of
/// [`ApiError`] gets a code without this list having to learn it. The codes
/// are the ones OpenTelemetry's exporters act on: `UNAVAILABLE` is retried,
/// `INVALID_ARGUMENT` and `UNAUTHENTICATED` are not.
fn status_for(state: &AppState, e: ApiError) -> Response {
    let message = e.to_string();
    let http = e.into_response().status();
    let code = match http {
        StatusCode::BAD_REQUEST | StatusCode::UNSUPPORTED_MEDIA_TYPE => code::INVALID_ARGUMENT,
        StatusCode::UNAUTHORIZED => code::UNAUTHENTICATED,
        StatusCode::FORBIDDEN => code::PERMISSION_DENIED,
        StatusCode::NOT_FOUND => code::NOT_FOUND,
        StatusCode::CONFLICT => code::ALREADY_EXISTS,
        StatusCode::PAYLOAD_TOO_LARGE | StatusCode::TOO_MANY_REQUESTS => code::RESOURCE_EXHAUSTED,
        StatusCode::UNPROCESSABLE_ENTITY => code::FAILED_PRECONDITION,
        StatusCode::NOT_IMPLEMENTED => code::UNIMPLEMENTED,
        StatusCode::SERVICE_UNAVAILABLE => code::UNAVAILABLE,
        _ => code::INTERNAL,
    };
    // The answer leaves as HTTP 200, so the layer that counts server errors
    // never sees one. Counted here instead, by the same rule.
    if http.is_server_error() {
        state.metrics.errors.fetch_add(1, Ordering::Relaxed);
    }
    status(code, &message)
}

/// Read the one message of a unary call, decompressed.
async fn read_message(
    state: &AppState,
    headers: &HeaderMap,
    body: Body,
) -> Result<Bytes, Response> {
    let max = state.config.max_body_bytes;
    let raw = axum::body::to_bytes(body, max.saturating_add(5)).await.map_err(|_| {
        status(
            code::RESOURCE_EXHAUSTED,
            &format!("the message is larger than the {max} bytes this server accepts per call"),
        )
    })?;
    if raw.len() < 5 {
        return Err(status(code::INVALID_ARGUMENT, "the call carries no gRPC message"));
    }
    let compressed = raw[0] == 1;
    let len = u32::from_be_bytes([raw[1], raw[2], raw[3], raw[4]]) as usize;
    if raw.len() - 5 != len {
        return Err(status(
            code::INVALID_ARGUMENT,
            "a unary call carries exactly one message, and this one's length prefix does not \
             match the bytes that follow it",
        ));
    }
    let message = raw.slice(5..);
    if !compressed {
        return Ok(message);
    }
    let encoding = headers.get("grpc-encoding").and_then(|v| v.to_str().ok()).unwrap_or("");
    if !encoding.eq_ignore_ascii_case("gzip") {
        return Err(status(
            code::UNIMPLEMENTED,
            &format!("grpc-encoding {encoding:?} is not supported; use gzip or none"),
        ));
    }
    // The same bounded inflate as the HTTP doors: the decompressed size is held
    // to the body cap, so a small message cannot expand without limit.
    let mut gzip = HeaderMap::new();
    gzip.insert(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"));
    ingest::decompress(&gzip, message, max).map_err(|e| status_for(state, e))
}

/// What a call is for.
#[derive(Clone, Copy)]
enum Call {
    Otlp(Signal),
    Jaeger,
}

async fn serve(
    state: Arc<AppState>,
    headers: HeaderMap,
    body: Body,
    grant: Result<Grant, ApiError>,
    call: Call,
) -> Response {
    if !is_grpc(&headers) {
        return ApiError::BadRequest(
            "this path is a gRPC method; call it with a gRPC client, content-type \
             application/grpc, over HTTP/2"
                .to_owned(),
        )
        .into_response();
    }
    if let Some(ct) = headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()) {
        if ct != "application/grpc" && !ct.starts_with("application/grpc+proto") {
            return status(code::UNIMPLEMENTED, "only the protobuf codec is served");
        }
    }
    // The key is checked before a byte of the body is read, as on the HTTP
    // doors, so a caller with no key cannot make the daemon hold a message.
    let grant = match grant {
        Ok(g) => g,
        Err(e) => return status_for(&state, e),
    };
    state.metrics.ingest_requests.fetch_add(1, Ordering::Relaxed);
    let message = match read_message(&state, &headers, body).await {
        Ok(m) => m,
        Err(res) => return res,
    };
    let outcome = match call {
        Call::Otlp(signal) => ingest::ingest_otlp(&state, &headers, &grant, signal, &message, true)
            .await
            .map(|result| ingest::otlp_response_protobuf(&result)),
        Call::Jaeger => match mcpd_core::jaeger::decode_post_spans(&message) {
            Ok(batch) => ingest::ingest_spans(&state, &headers, &grant, &batch.lower())
                .await
                // PostSpansResponse has no fields.
                .map(|_| Vec::new()),
            Err(e) => Err(ApiError::BadRequest(format!("not a valid PostSpansRequest: {e}"))),
        },
    };
    match outcome {
        Ok(response) => reply(response),
        Err(e) => status_for(&state, e),
    }
}

/// `opentelemetry.proto.collector.trace.v1.TraceService/Export`
pub async fn export_traces(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let grant = authorize(&state, &headers, Scope::Ingest);
    serve(state, headers, body, grant, Call::Otlp(Signal::Traces)).await
}

/// `opentelemetry.proto.collector.logs.v1.LogsService/Export`
pub async fn export_logs(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let grant = authorize(&state, &headers, Scope::Ingest);
    serve(state, headers, body, grant, Call::Otlp(Signal::Logs)).await
}

/// `opentelemetry.proto.collector.metrics.v1.MetricsService/Export`
pub async fn export_metrics(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let grant = authorize(&state, &headers, Scope::Ingest);
    serve(state, headers, body, grant, Call::Otlp(Signal::Metrics)).await
}

/// `jaeger.api_v2.CollectorService/PostSpans`
pub async fn post_spans(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let grant = authorize(&state, &headers, Scope::Ingest);
    serve(state, headers, body, grant, Call::Jaeger).await
}

/// Answer a gRPC call to a method this daemon does not serve with
/// `UNIMPLEMENTED`, which is what a gRPC client understands, rather than with
/// the HTML or the `405` an HTTP route would give it.
pub async fn refuse_unknown(req: Request, next: Next) -> Response {
    let path = req.uri().path();
    let known = [OTLP_TRACES, OTLP_LOGS, OTLP_METRICS, JAEGER_POST_SPANS].contains(&path);
    if !known && is_grpc(req.headers()) {
        return status(
            code::UNIMPLEMENTED,
            &format!(
                "mcpd does not serve the gRPC method {path}; it serves the three OTLP collector \
                 services and jaeger.api_v2.CollectorService"
            ),
        );
    }
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grpc_message_is_percent_encoded() {
        assert_eq!(percent_encode("plain text"), "plain text");
        assert_eq!(percent_encode("100% é"), "100%25 %C3%A9");
    }

    #[test]
    fn a_status_has_no_body_and_names_the_code() {
        let res = status(code::UNAUTHENTICATED, "no key");
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers()["grpc-status"], "16");
        assert_eq!(res.headers()["grpc-message"], "no key");
    }
}
