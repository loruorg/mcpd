//! A stand-in mcpd, for tests that need to see what the SDK actually sent.
//!
//! It speaks just enough HTTP/1.1 to satisfy reqwest: read the headers, read
//! `Content-Length` bytes, answer, close. Every request is captured, and the
//! replies can be scripted so a test can make the server fail on purpose.

#![allow(dead_code)]

use std::collections::VecDeque;
use std::io::Read;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, MutexGuard};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// One request the SDK made.
#[derive(Debug)]
pub struct Captured {
    pub method: String,
    pub target: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Captured {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    /// The path, without the query string.
    pub fn path(&self) -> &str {
        self.target.split('?').next().unwrap_or(&self.target)
    }

    /// The query string, if any.
    pub fn query(&self) -> &str {
        self.target.split_once('?').map_or("", |(_, q)| q)
    }

    /// The body as JSON, decompressing it first when the SDK gzipped it.
    pub fn json(&self) -> serde_json::Value {
        let raw = if self.header("content-encoding") == Some("gzip") {
            let mut out = Vec::new();
            flate2::read::GzDecoder::new(&self.body[..])
                .read_to_end(&mut out)
                .expect("the SDK said gzip but did not send gzip");
            out
        } else {
            self.body.clone()
        };
        serde_json::from_slice(&raw).expect("body was not JSON")
    }

    /// The spans in an ingest batch.
    pub fn spans(&self) -> Vec<serde_json::Value> {
        self.json()["spans"].as_array().cloned().unwrap_or_default()
    }
}

/// What the server should answer next.
#[derive(Debug, Clone)]
pub enum Reply {
    /// 200, with an ingest receipt.
    Accepted,
    /// A failure status, with an optional `Retry-After` in seconds.
    Status(u16, Option<u32>),
    /// A status and a body of your own.
    Body(u16, String),
    /// A status and a body, sent after that many milliseconds: a long poll the
    /// daemon holds open before it answers.
    After(u64, u16, String),
}

/// Per-path reply queues, matched by suffix.
type Routes = Arc<Mutex<Vec<(String, VecDeque<Reply>)>>>;

/// A fake mcpd listening on a loopback port.
pub struct FakeMcpd {
    addr: SocketAddr,
    requests: Arc<Mutex<Vec<Captured>>>,
    replies: Arc<Mutex<VecDeque<Reply>>>,
    routes: Routes,
}

impl FakeMcpd {
    /// A server that accepts everything.
    pub async fn start() -> Self {
        Self::scripted(Vec::new()).await
    }

    /// A server that answers with `replies` in order, then accepts everything.
    pub async fn scripted(replies: Vec<Reply>) -> Self {
        Self::routed(replies, Vec::new()).await
    }

    /// A server that answers a path ending in a route's suffix from that
    /// route's own queue (repeating its last reply), and everything else from
    /// `replies` in order. Routes are what let several concurrent leases
    /// each get the right shape of answer.
    pub async fn routed(replies: Vec<Reply>, routes: Vec<(&str, Vec<Reply>)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("could not bind");
        let addr = listener.local_addr().expect("no local address");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let replies = Arc::new(Mutex::new(replies.into_iter().collect::<VecDeque<_>>()));
        let routes = Arc::new(Mutex::new(
            routes
                .into_iter()
                .map(|(suffix, queue)| {
                    (suffix.to_owned(), queue.into_iter().collect::<VecDeque<_>>())
                })
                .collect::<Vec<_>>(),
        ));

        let (r, s, rt) = (Arc::clone(&requests), Arc::clone(&replies), Arc::clone(&routes));
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else { return };
                let (r, s, rt) = (Arc::clone(&r), Arc::clone(&s), Arc::clone(&rt));
                tokio::spawn(async move {
                    let Some(captured) = read_request(&mut stream).await else { return };
                    let routed = {
                        let mut routes = rt.lock().unwrap();
                        routes
                            .iter_mut()
                            .find(|(suffix, _)| captured.path().ends_with(suffix.as_str()))
                            .and_then(|(_, queue)| {
                                if queue.len() > 1 {
                                    queue.pop_front()
                                } else {
                                    queue.front().cloned()
                                }
                            })
                    };
                    let reply =
                        routed.or_else(|| s.lock().unwrap().pop_front()).unwrap_or(Reply::Accepted);
                    r.lock().unwrap().push(captured);
                    let retry_after = match &reply {
                        Reply::Status(_, Some(secs)) => format!("Retry-After: {secs}\r\n"),
                        _ => String::new(),
                    };
                    let (status, body) = match reply {
                        Reply::Accepted => {
                            (200, r#"{"accepted":1,"rejected":0,"duplicate":0}"#.to_owned())
                        }
                        Reply::Status(code, _) => {
                            (code, format!(r#"{{"error":"e","message":"status {code}"}}"#))
                        }
                        Reply::Body(code, body) => (code, body),
                        Reply::After(ms, code, body) => {
                            tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
                            (code, body)
                        }
                    };
                    let response = format!(
                        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\n\
                         Content-Length: {}\r\n{retry_after}Connection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(response.as_bytes()).await;
                    let _ = stream.flush().await;
                });
            }
        });

        Self { addr, requests, replies, routes }
    }

    /// The endpoint to configure the SDK with.
    pub fn endpoint(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// Everything the SDK has sent so far.
    pub fn requests(&self) -> MutexGuard<'_, Vec<Captured>> {
        self.requests.lock().unwrap()
    }

    /// How many requests have arrived.
    pub fn count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }

    /// Every span from every batch, in the order they were sent.
    pub fn all_spans(&self) -> Vec<serde_json::Value> {
        self.requests().iter().flat_map(Captured::spans).collect()
    }

    /// Wait until at least `n` requests have arrived, or give up.
    pub async fn wait_for(&self, n: usize, within: std::time::Duration) -> bool {
        let deadline = std::time::Instant::now() + within;
        while std::time::Instant::now() < deadline {
            if self.count() >= n {
                return true;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        self.count() >= n
    }
}

/// Read one HTTP/1.1 request off the socket.
async fn read_request(stream: &mut tokio::net::TcpStream) -> Option<Captured> {
    let mut buf = Vec::new();
    let head_end = loop {
        let mut chunk = [0u8; 4096];
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(i) = find(&buf, b"\r\n\r\n") {
            break i + 4;
        }
    };

    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let mut lines = head.lines();
    let mut request_line = lines.next()?.split_whitespace();
    let method = request_line.next()?.to_owned();
    let target = request_line.next()?.to_owned();

    let mut headers = Vec::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            headers.push((k.trim().to_owned(), v.trim().to_owned()));
        }
    }

    let length: usize = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(0);

    let mut body = buf[head_end..].to_vec();
    while body.len() < length {
        let mut chunk = [0u8; 4096];
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }

    Some(Captured { method, target, headers, body })
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}
