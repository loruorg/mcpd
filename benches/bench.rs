//! A load generator and stopwatch for a running mcpd daemon.
//!
//! This talks HTTP to a real daemon over a real socket. It does not call into
//! the crates, because the number an operator sizes a deployment with is the
//! whole path: socket, decode, sample, redact, price, quota, transaction.
//!
//! There is no dependency and no benchmarking framework. A loop, a clock and a
//! sorted vector of samples answer the question, and a zero-dependency file
//! compiles with `rustc -O` in a couple of seconds, which keeps the harness
//! cheap enough that nobody skips running it.

#![forbid(unsafe_code)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/* --------------------------------------------------------------- utility -- */

/// xorshift64*. Not cryptographic, and it does not need to be: it fills ids and
/// picks payload variants. It is here because `rand` would be a dependency and
/// this is four lines.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        // A zero state is the one seed xorshift cannot escape.
        Rng(seed | 1)
    }
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }
    /// `bytes` random bytes as lowercase hex, which is the id shape mcpd
    /// validates: 32 characters for a trace, 16 for a span.
    fn hex(&mut self, bytes: usize, out: &mut String) {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut left = bytes;
        while left > 0 {
            let mut v = self.next_u64();
            let take = left.min(8);
            for _ in 0..take {
                let b = (v & 0xff) as usize;
                out.push(HEX[b >> 4] as char);
                out.push(HEX[b & 0x0f] as char);
                v >>= 8;
            }
            left -= take;
        }
    }
}

/// Days to a civil date, Howard Hinnant's algorithm.
///
/// Needed because the harness has no date library and mcpd wants RFC 3339. A
/// fixed epoch string would be wrong: the analytics summary defaults to the
/// last 24 hours, so a bench that stamped 1970 would measure an empty window
/// and report a query cost of nothing.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as i64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn rfc3339(secs: i64, millis: u32, out: &mut String) {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, mo, d) = civil_from_days(days);
    let (h, mi, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    out.push_str(&format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}.{millis:03}Z"));
}

fn now_secs() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Nearest-rank percentile over an already-sorted slice.
fn pct(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = ((p / 100.0) * sorted.len() as f64).ceil() as usize;
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

fn ms(nanos: u64) -> f64 {
    nanos as f64 / 1_000_000.0
}

/* ------------------------------------------------------------------ http -- */

/// One keep-alive HTTP/1.1 connection.
///
/// Keep-alive matters: without it every sample would include a TCP handshake,
/// and the number would describe connection setup rather than mcpd.
struct Conn {
    w: TcpStream,
    r: BufReader<TcpStream>,
    addr: String,
}

impl Conn {
    fn connect(addr: &str) -> std::io::Result<Self> {
        let w = TcpStream::connect(addr)?;
        // Nagle would batch small request bodies and add milliseconds that
        // belong to the kernel, not to the daemon.
        w.set_nodelay(true)?;
        w.set_read_timeout(Some(Duration::from_secs(120)))?;
        let r = BufReader::new(w.try_clone()?);
        Ok(Conn { w, r, addr: addr.to_owned() })
    }

    fn send(&mut self, method: &str, path: &str, body: Option<&[u8]>) -> std::io::Result<(u16, Vec<u8>)> {
        let mut req = Vec::with_capacity(256 + body.map_or(0, <[u8]>::len));
        req.extend_from_slice(method.as_bytes());
        req.extend_from_slice(b" ");
        req.extend_from_slice(path.as_bytes());
        req.extend_from_slice(b" HTTP/1.1\r\nHost: ");
        req.extend_from_slice(self.addr.as_bytes());
        req.extend_from_slice(b"\r\nConnection: keep-alive\r\n");
        if let Some(b) = body {
            req.extend_from_slice(b"Content-Type: application/json\r\nContent-Length: ");
            req.extend_from_slice(b.len().to_string().as_bytes());
            req.extend_from_slice(b"\r\n\r\n");
            req.extend_from_slice(b);
        } else {
            req.extend_from_slice(b"\r\n");
        }
        // One write_all, so a large body is not split into syscalls that show
        // up in the latency as if the daemon had been slow.
        self.w.write_all(&req)?;
        self.w.flush()?;
        self.read_response()
    }

    fn read_response(&mut self) -> std::io::Result<(u16, Vec<u8>)> {
        let mut line = String::new();
        if self.r.read_line(&mut line)? == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "server closed"));
        }
        let code: u16 = line.split(' ').nth(1).and_then(|s| s.trim().parse().ok()).unwrap_or(0);

        let mut clen: Option<usize> = None;
        let mut chunked = false;
        loop {
            line.clear();
            if self.r.read_line(&mut line)? == 0 {
                return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "eof in headers"));
            }
            let t = line.trim_end();
            if t.is_empty() {
                break;
            }
            let lower = t.to_ascii_lowercase();
            if let Some(v) = lower.strip_prefix("content-length:") {
                clen = v.trim().parse().ok();
            } else if lower.starts_with("transfer-encoding:") && lower.contains("chunked") {
                chunked = true;
            }
        }

        let mut body = Vec::new();
        if chunked {
            loop {
                line.clear();
                self.r.read_line(&mut line)?;
                let head = line.trim_end();
                let size = usize::from_str_radix(head.split(';').next().unwrap_or("0").trim(), 16)
                    .unwrap_or(0);
                let mut crlf = [0u8; 2];
                if size == 0 {
                    let _ = self.r.read_exact(&mut crlf);
                    break;
                }
                let at = body.len();
                body.resize(at + size, 0);
                self.r.read_exact(&mut body[at..])?;
                self.r.read_exact(&mut crlf)?;
            }
        } else {
            let n = clen.unwrap_or(0);
            body.resize(n, 0);
            if n > 0 {
                self.r.read_exact(&mut body)?;
            }
        }
        Ok((code, body))
    }
}

/// Pull an integer field out of a JSON response without a JSON parser.
///
/// The harness reads exactly three numbers back from the daemon, all of them
/// flat integers in a small object. A parser would be more code than this.
fn json_u64(body: &[u8], key: &str) -> Option<u64> {
    let s = std::str::from_utf8(body).ok()?;
    let pat = format!("\"{key}\":");
    let at = s.find(&pat)? + pat.len();
    let rest = s[at..].trim_start();
    let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
    rest[..end].parse().ok()
}

fn json_str(body: &[u8], key: &str) -> Option<String> {
    let s = std::str::from_utf8(body).ok()?;
    let pat = format!("\"{key}\":\"");
    let at = s.find(&pat)? + pat.len();
    let rest = &s[at..];
    let end = rest.find('"')?;
    Some(rest[..end].to_owned())
}

/* --------------------------------------------------------------- payload -- */

const PROMPT: &str = "The customer asks why invoice INV-40122 was charged twice \
in March. Check the billing ledger, then the refund queue, then answer in two \
sentences. Do not promise a refund you have not confirmed.";

const COMPLETION: &str = "Invoice INV-40122 was authorised once and captured \
twice because the retry ran before the first capture settled. The duplicate \
capture has been queued for refund and clears in three working days.";

/// One ingest batch, as the daemon's native shape.
///
/// The payload is deliberately not a toy. It carries a prompt, a completion,
/// nested attributes and correlation fields, including keys the default
/// redactor matches (`api_key`, `email`, `user_id`), so the redaction column of
/// the report is measuring redaction actually happening rather than a walk over
/// an empty object.
fn build_batch(rng: &mut Rng, batch: usize, project: &str, now: i64) -> String {
    let mut trace = String::with_capacity(32);
    rng.hex(16, &mut trace);

    let mut start = String::new();
    // Spread the run over the last six hours so a query has a realistic range
    // to sort and filter, not one instant.
    let offset = rng.below(6 * 3600) as i64;
    rfc3339(now - offset, (rng.below(1000)) as u32, &mut start);
    let mut end = String::new();
    rfc3339(now - offset + 2, 400, &mut end);
    let mut received = String::new();
    rfc3339(now, 0, &mut received);

    let mut out = String::with_capacity(batch * 1200 + 64);
    out.push_str("{\"spans\":[");

    let mut root = String::with_capacity(32);
    rng.hex(8, &mut root);

    for i in 0..batch {
        if i > 0 {
            out.push(',');
        }
        let is_root = i == 0;
        let mut span = String::with_capacity(16);
        if is_root {
            span.push_str(&root);
        } else {
            rng.hex(8, &mut span);
        }
        let (name, kind) = if is_root {
            ("support_agent", "agent_run")
        } else if i % 3 == 0 {
            ("lookup_ledger", "tool_call")
        } else if i % 3 == 1 {
            ("answer", "model_inference")
        } else {
            ("rank_documents", "model_step")
        };
        let status = if rng.below(100) < 4 { "error" } else { "ok" };

        out.push_str("{\"trace_id\":\"");
        out.push_str(&trace);
        out.push_str("\",\"span_id\":\"");
        out.push_str(&span);
        out.push('"');
        if !is_root {
            out.push_str(",\"parent_span_id\":\"");
            out.push_str(&root);
            out.push('"');
        }
        out.push_str(",\"name\":\"");
        out.push_str(name);
        out.push_str("\",\"kind\":\"");
        out.push_str(kind);
        out.push_str("\",\"status\":\"");
        out.push_str(status);
        out.push_str("\",\"start_time\":\"");
        out.push_str(&start);
        out.push_str("\",\"end_time\":\"");
        out.push_str(&end);
        out.push_str("\",\"duration_ns\":");
        out.push_str(&(1_000_000 + rng.below(4_000_000_000)).to_string());
        out.push_str(",\"is_root\":");
        out.push_str(if is_root { "true" } else { "false" });
        out.push_str(",\"is_event\":false");
        out.push_str(",\"input\":");
        out.push('"');
        out.push_str(PROMPT);
        out.push_str("\",\"output\":\"");
        out.push_str(COMPLETION);
        out.push_str("\",\"model\":\"claude-sonnet-4-5\",\"provider\":\"anthropic\"");
        out.push_str(",\"usage\":{\"input_tokens\":");
        out.push_str(&(400 + rng.below(3000)).to_string());
        out.push_str(",\"output_tokens\":");
        out.push_str(&(50 + rng.below(700)).to_string());
        out.push_str("}");
        out.push_str(",\"attributes\":{\"tool\":{\"name\":\"billing_ledger\",\
\"api_key\":\"sk-live-9f3a2b7c4d5e6f708192a3b4c5d6e7f8\",\
\"endpoint\":\"https://ledger.internal/v2/invoices\"},\
\"contact\":{\"email\":\"claimant@example.com\",\"phone\":\"+44 7700 900412\"},\
\"retries\":2,\"region\":\"eu-west-1\"}");
        out.push_str(",\"metadata\":{\"deploy\":\"canary\",\"build\":\"2026.8.14\"}");
        out.push_str(",\"correlation\":{\"project\":\"");
        out.push_str(project);
        out.push_str("\",\"environment\":\"production\",\"service_name\":\"support-api\",\
\"service_version\":\"1.14.2\",\"session_id\":\"sess-");
        out.push_str(&rng.below(10_000).to_string());
        out.push_str("\",\"user_id\":\"cust-");
        out.push_str(&rng.below(50_000).to_string());
        out.push_str("\"}");
        out.push_str(",\"received_at\":\"");
        out.push_str(&received);
        out.push_str("\"}");
    }
    out.push_str("]}");
    out
}

/* ----------------------------------------------------------------- modes -- */

struct Totals {
    requests: AtomicU64,
    accepted: AtomicU64,
    rejected: AtomicU64,
    errors: AtomicU64,
    samples: Mutex<Vec<u64>>,
}

impl Totals {
    fn new() -> Self {
        Totals {
            requests: AtomicU64::new(0),
            accepted: AtomicU64::new(0),
            rejected: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            samples: Mutex::new(Vec::new()),
        }
    }
}

/// Drive ingest at a fixed concurrency for a fixed wall time.
///
/// Closed-loop by design: every connection sends the next batch as soon as the
/// last reply lands. That reports what the daemon will absorb when the sender
/// keeps pushing, which is the question an operator sizing a deployment is
/// asking. It is not an open-loop arrival test, so the latency here is service
/// time under that load, not queue time under a fixed arrival rate.
fn run_ingest(
    addr: &str,
    batch: usize,
    conns: usize,
    seconds: f64,
    warmup: f64,
    project: &str,
    target: Option<u64>,
) {
    let totals = Totals::new();
    let start_wall = Instant::now();
    // Fixed-work mode takes no warmup. A warmup would put rows in the store
    // before the clock started, and because per-span cost rises with the row
    // count, each batch size would then be measured over a different part of
    // the curve and the sizes would not be comparable.
    let warmup = if target.is_some() { 0.0 } else { warmup };
    let warm_until = start_wall + Duration::from_secs_f64(warmup);
    let deadline = if target.is_some() {
        // A ceiling, not the stop condition: it only stops a run that has
        // stalled, so a hung daemon fails the bench instead of hanging it.
        start_wall + Duration::from_secs_f64(1800.0)
    } else {
        warm_until + Duration::from_secs_f64(seconds)
    };

    std::thread::scope(|scope| {
        for t in 0..conns {
            let totals = &totals;
            scope.spawn(move || {
                let seed = now_secs() as u64 ^ ((t as u64 + 1) << 32) ^ (std::process::id() as u64) << 7;
                let mut rng = Rng::new(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15));
                let mut conn = match Conn::connect(addr) {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("connect failed: {e}");
                        return;
                    }
                };
                let mut local: Vec<u64> = Vec::with_capacity(4096);
                loop {
                    let now = Instant::now();
                    if now >= deadline {
                        break;
                    }
                    if let Some(t) = target {
                        if totals.accepted.load(Ordering::Relaxed) >= t {
                            break;
                        }
                    }
                    let measuring = now >= warm_until;
                    let body = build_batch(&mut rng, batch, project, now_secs());
                    let t0 = Instant::now();
                    let res = conn.send("POST", "/v1/ingest", Some(body.as_bytes()));
                    let dt = t0.elapsed().as_nanos() as u64;
                    match res {
                        Ok((200, b)) | Ok((202, b)) => {
                            let acc = json_u64(&b, "accepted").unwrap_or(0);
                            let rej = json_u64(&b, "rejected").unwrap_or(0);
                            if measuring {
                                totals.requests.fetch_add(1, Ordering::Relaxed);
                                totals.accepted.fetch_add(acc, Ordering::Relaxed);
                                totals.rejected.fetch_add(rej, Ordering::Relaxed);
                                local.push(dt);
                            }
                        }
                        Ok((code, b)) => {
                            if measuring {
                                totals.errors.fetch_add(1, Ordering::Relaxed);
                            }
                            if totals.errors.load(Ordering::Relaxed) < 3 {
                                eprintln!(
                                    "http {code}: {}",
                                    String::from_utf8_lossy(&b[..b.len().min(300)])
                                );
                            }
                        }
                        Err(e) => {
                            if measuring {
                                totals.errors.fetch_add(1, Ordering::Relaxed);
                            }
                            if totals.errors.load(Ordering::Relaxed) < 3 {
                                eprintln!("io: {e}");
                            }
                            // A dropped keep-alive is not a measurement failure
                            // on its own, so reconnect and carry on.
                            match Conn::connect(addr) {
                                Ok(c) => conn = c,
                                Err(_) => return,
                            }
                        }
                    }
                }
                totals.samples.lock().unwrap().extend_from_slice(&local);
            });
        }
    });

    let elapsed = start_wall.elapsed().as_secs_f64() - warmup;
    let mut s = totals.samples.lock().unwrap().clone();
    s.sort_unstable();
    let accepted = totals.accepted.load(Ordering::Relaxed);
    let mean = if s.is_empty() { 0.0 } else { s.iter().sum::<u64>() as f64 / s.len() as f64 };
    println!(
        "{{\"mode\":\"ingest\",\"batch\":{batch},\"conns\":{conns},\"seconds\":{:.3},\
\"requests\":{},\"spans\":{accepted},\"rejected\":{},\"errors\":{},\
\"spans_per_sec\":{:.1},\"requests_per_sec\":{:.1},\
\"p50_ms\":{:.3},\"p90_ms\":{:.3},\"p99_ms\":{:.3},\"max_ms\":{:.3},\"mean_ms\":{:.3}}}",
        elapsed,
        totals.requests.load(Ordering::Relaxed),
        totals.rejected.load(Ordering::Relaxed),
        totals.errors.load(Ordering::Relaxed),
        accepted as f64 / elapsed,
        totals.requests.load(Ordering::Relaxed) as f64 / elapsed,
        ms(pct(&s, 50.0)),
        ms(pct(&s, 90.0)),
        ms(pct(&s, 99.0)),
        ms(s.last().copied().unwrap_or(0)),
        ms(mean as u64),
    );
}

/// Fill a store with a known number of spans, as fast as the daemon will take
/// them. Used to build the 100k-span store the query numbers are measured on.
fn run_seed(addr: &str, total: u64, batch: usize, conns: usize, project: &str) {
    let done = AtomicU64::new(0);
    let errors = AtomicU64::new(0);
    let t0 = Instant::now();
    std::thread::scope(|scope| {
        for t in 0..conns {
            let done = &done;
            let errors = &errors;
            scope.spawn(move || {
                let seed = now_secs() as u64 ^ ((t as u64 + 1) << 40) ^ (std::process::id() as u64);
                let mut rng = Rng::new(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15));
                let mut conn = match Conn::connect(addr) {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("connect failed: {e}");
                        return;
                    }
                };
                loop {
                    if done.load(Ordering::Relaxed) >= total {
                        break;
                    }
                    let body = build_batch(&mut rng, batch, project, now_secs());
                    match conn.send("POST", "/v1/ingest", Some(body.as_bytes())) {
                        Ok((200, b)) | Ok((202, b)) => {
                            done.fetch_add(json_u64(&b, "accepted").unwrap_or(0), Ordering::Relaxed);
                        }
                        Ok((code, b)) => {
                            errors.fetch_add(1, Ordering::Relaxed);
                            if errors.load(Ordering::Relaxed) < 3 {
                                eprintln!(
                                    "seed http {code}: {}",
                                    String::from_utf8_lossy(&b[..b.len().min(300)])
                                );
                            }
                        }
                        Err(e) => {
                            let n = errors.fetch_add(1, Ordering::Relaxed) + 1;
                            // Always say why. A seed that stopped early and
                            // said nothing is how a short store gets reported
                            // as a full one.
                            eprintln!("seed io error {n} at {} spans: {e}", done.load(Ordering::Relaxed));
                            if n > 200 {
                                eprintln!("seed giving up after {n} errors");
                                return;
                            }
                            match Conn::connect(addr) {
                                Ok(c) => conn = c,
                                Err(e2) => {
                                    eprintln!(
                                        "seed reconnect to {addr} failed, this thread stops \
                                         at {} spans: {e2}",
                                        done.load(Ordering::Relaxed)
                                    );
                                    return;
                                }
                            }
                        }
                    }
                }
            });
        }
    });
    let el = t0.elapsed().as_secs_f64();
    let got = done.load(Ordering::Relaxed);
    if got < total {
        eprintln!("seed reached {got} of {total} spans; the store is short and the read numbers are measured against what is actually there");
    }
    println!(
        "{{\"mode\":\"seed\",\"spans\":{got},\"requested\":{total},\"complete\":{},\"seconds\":{el:.3},\"spans_per_sec\":{:.1},\"errors\":{}}}",
        got >= total,
        got as f64 / el,
        errors.load(Ordering::Relaxed)
    );
}

/// Time one read endpoint, serially, on one connection.
///
/// Serial and single-connection on purpose: a read latency is what one operator
/// waits for when they open a trace, so contention from a second reader would
/// describe a different question.
fn run_query(addr: &str, label: &str, path: &str, repeats: usize) {
    let mut conn = match Conn::connect(addr) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("connect failed: {e}");
            return;
        }
    };
    let mut samples = Vec::with_capacity(repeats);
    let mut errors = 0u64;
    let mut bytes = 0usize;
    for _ in 0..repeats {
        let t0 = Instant::now();
        match conn.send("GET", path, None) {
            Ok((200, b)) => {
                samples.push(t0.elapsed().as_nanos() as u64);
                bytes = b.len();
            }
            Ok((code, b)) => {
                errors += 1;
                if errors < 3 {
                    eprintln!(
                        "{label} http {code}: {}",
                        String::from_utf8_lossy(&b[..b.len().min(300)])
                    );
                }
            }
            Err(e) => {
                errors += 1;
                if errors < 3 {
                    eprintln!("{label} io: {e}");
                }
                match Conn::connect(addr) {
                    Ok(c) => conn = c,
                    Err(_) => break,
                }
            }
        }
    }
    samples.sort_unstable();
    let mean =
        if samples.is_empty() { 0.0 } else { samples.iter().sum::<u64>() as f64 / samples.len() as f64 };
    println!(
        "{{\"mode\":\"query\",\"label\":\"{label}\",\"repeats\":{},\"errors\":{errors},\
\"bytes\":{bytes},\"p50_ms\":{:.3},\"p90_ms\":{:.3},\"p99_ms\":{:.3},\"max_ms\":{:.3},\"mean_ms\":{:.3}}}",
        samples.len(),
        ms(pct(&samples, 50.0)),
        ms(pct(&samples, 90.0)),
        ms(pct(&samples, 99.0)),
        ms(samples.last().copied().unwrap_or(0)),
        mean / 1_000_000.0
    );
}

/// Ask the daemon for one trace id, so the single-trace read has a real target
/// rather than an id that would take the not-found path.
fn pick_trace(addr: &str, project: &str) -> Option<String> {
    let mut conn = Conn::connect(addr).ok()?;
    let path = format!("/v1/api/traces?project={project}&limit=1");
    let (code, body) = conn.send("GET", &path, None).ok()?;
    if code != 200 {
        eprintln!("pick_trace http {code}: {}", String::from_utf8_lossy(&body[..body.len().min(300)]));
        return None;
    }
    json_str(&body, "trace_id")
}

/* ------------------------------------------------------------------ main -- */

fn flag(name: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    let key = format!("--{name}");
    args.iter().position(|a| a == &key).and_then(|i| args.get(i + 1).cloned())
}

fn flag_or<T: std::str::FromStr>(name: &str, default: T) -> T {
    flag(name).and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_default();
    let addr = flag("url").unwrap_or_else(|| "127.0.0.1:4317".to_owned());
    let project = flag("project").unwrap_or_else(|| "default".to_owned());

    match mode.as_str() {
        "ingest" => run_ingest(
            &addr,
            flag_or("batch", 1usize),
            flag_or("conns", 8usize),
            flag_or("seconds", 4.0f64),
            flag_or("warmup", 1.0f64),
            &project,
            flag("spans").and_then(|v| v.parse().ok()),
        ),
        "seed" => run_seed(
            &addr,
            flag_or("total", 100_000u64),
            flag_or("batch", 1000usize),
            flag_or("conns", 4usize),
            &project,
        ),
        "query" => {
            let repeats = flag_or("repeats", 50usize);
            let trace = flag("trace-id").or_else(|| pick_trace(&addr, &project));
            run_query(&addr, "list_traces", &format!("/v1/api/traces?project={project}&limit=50"), repeats);
            match trace {
                Some(id) => run_query(
                    &addr,
                    "get_trace",
                    &format!("/v1/api/traces/{id}?project={project}"),
                    repeats,
                ),
                None => eprintln!("no trace id available; skipping get_trace"),
            }
            run_query(
                &addr,
                "search",
                &format!("/v1/api/traces?project={project}&q=invoice&limit=50"),
                repeats,
            );
            run_query(
                &addr,
                "analytics_summary",
                &format!("/v1/api/analytics/summary?project={project}"),
                repeats,
            );
        }
        "trace-id" => match pick_trace(&addr, &project) {
            Some(id) => println!("{id}"),
            None => std::process::exit(1),
        },
        _ => {
            eprintln!(
                "usage:\n  \
                 bench ingest --url HOST:PORT [--batch N] [--conns N] [--seconds S | --spans N] [--warmup S] [--project P]\n  \
                 bench seed   --url HOST:PORT [--total N] [--batch N] [--conns N] [--project P]\n  \
                 bench query  --url HOST:PORT [--repeats N] [--trace-id ID] [--project P]\n  \
                 bench trace-id --url HOST:PORT [--project P]"
            );
            std::process::exit(2);
        }
    }
}
