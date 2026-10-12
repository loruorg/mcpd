//! What the forwarding and relay suites share: a scripted destination on a
//! port the OS assigns, temp directories named the way `tests/http.rs` names
//! them, and a way to post a batch through the real router.
//!
//! Not every suite uses every helper, so dead-code warnings are allowed here.
#![allow(dead_code)]

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::Router;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A directory of its own for this test, named from the pid and a stamp so a
/// recycled pid never reopens a previous run's files.
pub fn temp_dir(tag: &str) -> std::path::PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    static STAMP: std::sync::OnceLock<u128> = std::sync::OnceLock::new();
    let stamp = STAMP.get_or_init(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    });
    let dir = std::env::temp_dir().join(format!(
        "mcpd-{tag}-{}-{stamp}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).expect("create the test directory");
    dir
}

/// One request the destination received.
#[derive(Clone, Debug)]
pub struct Seen {
    /// The method.
    pub method: Method,
    /// The path and query.
    pub path: String,
    /// The headers as sent.
    pub headers: HeaderMap,
    /// The body, inflated when it was gzipped.
    pub body: Vec<u8>,
    /// When it arrived.
    pub at: Instant,
    /// The status it was answered with.
    pub status: u16,
}

impl Seen {
    /// A header's value.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|v| v.to_str().ok())
    }

    /// The protobuf inside a gRPC frame, or the body itself.
    pub fn payload(&self) -> &[u8] {
        if self.header("content-type") == Some("application/grpc") && self.body.len() >= 5 {
            &self.body[5..]
        } else {
            &self.body
        }
    }
}

#[derive(Default)]
struct Script {
    answers: VecDeque<(u16, Option<u64>)>,
    seen: Vec<Seen>,
}

/// A destination answering a script: one status per request, each with an
/// optional `Retry-After` in seconds, then 200 for ever.
#[derive(Clone)]
pub struct Mock {
    /// Its base URL.
    pub url: String,
    script: Arc<Mutex<Script>>,
}

async fn receive(
    State(script): State<Arc<Mutex<Script>>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let body = if headers.get("content-encoding").is_some_and(|v| v == "gzip") {
        use std::io::Read;
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(&body[..]).read_to_end(&mut out).expect("a gzip body");
        out
    } else {
        body.to_vec()
    };
    let grpc = headers.get("content-type").is_some_and(|v| v == "application/grpc");
    let mut s = script.lock().expect("the script lock");
    // A GET is a readiness probe and is answered 200 without spending the
    // script, which is written for the requests that carry data.
    let (status, retry_after) = if method == Method::GET {
        (200, None)
    } else {
        s.answers.pop_front().unwrap_or((200, None))
    };
    s.seen.push(Seen {
        method,
        path: uri.path_and_query().map_or_else(|| uri.path().to_owned(), |p| p.to_string()),
        headers,
        body,
        at: Instant::now(),
        status,
    });
    drop(s);
    if grpc {
        // A trailers-only answer: gRPC's status in the headers, no body.
        let code = if status == 200 { "0" } else { "14" };
        return (StatusCode::OK, [("content-type", "application/grpc"), ("grpc-status", code)])
            .into_response();
    }
    let mut out = HeaderMap::new();
    if let Some(secs) = retry_after {
        out.insert("retry-after", secs.to_string().parse().expect("a header value"));
    }
    (StatusCode::from_u16(status).expect("a status"), out, "{}").into_response()
}

impl Mock {
    /// Start one, on a port the OS assigns, speaking HTTP/1.1 and HTTP/2.
    pub async fn start(script: &[(u16, Option<u64>)]) -> Self {
        let shared = Arc::new(Mutex::new(Script {
            answers: script.iter().copied().collect(),
            seen: Vec::new(),
        }));
        let app = Router::new().fallback(receive).with_state(Arc::clone(&shared));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind a port");
        let addr = listener.local_addr().expect("the bound address");
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Self { url: format!("http://{addr}"), script: shared }
    }

    /// Every request so far, readiness probes left out.
    pub fn seen(&self) -> Vec<Seen> {
        let s = self.script.lock().expect("the script lock");
        s.seen.iter().filter(|r| r.method != Method::GET).cloned().collect()
    }

    /// The requests answered 2xx, probes left out.
    pub fn accepted(&self) -> Vec<Seen> {
        self.seen().into_iter().filter(|s| (200..300).contains(&s.status)).collect()
    }

    /// Wait until `n` requests were answered 2xx, or fail after `limit`.
    pub async fn wait_accepted(&self, n: usize, limit: Duration) -> Vec<Seen> {
        let started = Instant::now();
        loop {
            let got = self.accepted();
            if got.len() >= n {
                return got;
            }
            assert!(
                started.elapsed() < limit,
                "waited {limit:?} for {n} accepted requests, saw {} ({} attempts)",
                got.len(),
                self.seen().len()
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// Wait until `n` requests arrived at all.
    pub async fn wait_seen(&self, n: usize, limit: Duration) -> Vec<Seen> {
        let started = Instant::now();
        loop {
            let got = self.seen();
            if got.len() >= n {
                return got;
            }
            assert!(
                started.elapsed() < limit,
                "waited {limit:?} for {n} requests, saw {}",
                got.len()
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

/// A port nothing listens on.
pub fn dead_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .expect("a free port")
        .port()
}

/// POST a body through a router, returning the status and the body.
pub async fn post(
    app: &Router,
    path: &str,
    content_type: &str,
    headers: &[(&str, &str)],
    body: Vec<u8>,
) -> (StatusCode, String) {
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    let mut req = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", content_type)
        .header("content-length", body.len().to_string());
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let res = app
        .clone()
        .oneshot(req.body(Body::from(body)).expect("a request"))
        .await
        .expect("an answer");
    let status = res.status();
    let body = res.into_body().collect().await.expect("read the body").to_bytes();
    (status, String::from_utf8_lossy(&body).into_owned())
}

/// GET a path through a router.
pub async fn get(app: &Router, path: &str) -> (StatusCode, String) {
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    let res = app
        .clone()
        .oneshot(Request::builder().uri(path).body(Body::empty()).expect("a request"))
        .await
        .expect("an answer");
    let status = res.status();
    let body = res.into_body().collect().await.expect("read the body").to_bytes();
    (status, String::from_utf8_lossy(&body).into_owned())
}

/// One sample from a Prometheus text exposition, by its full series name.
pub fn sample(text: &str, series: &str) -> Option<f64> {
    text.lines().find_map(|l| {
        let (name, value) = l.rsplit_once(' ')?;
        (name == series).then(|| value.parse().ok())?
    })
}
