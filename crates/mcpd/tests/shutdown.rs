//! A stop is prompt even with a long-poll open.
//!
//! The shutdown token was cancelled only after `axum::serve` returned, and
//! `serve` returns only when every open request has finished. A worker's claim
//! long-poll finishes when that token is cancelled or its wait runs out, so a
//! SIGTERM with one open waited out the whole claim, up to a minute, and the
//! five second drain then ran after the orchestrator's SIGKILL.
//!
//! Unix only, because the test sends the signal the way an orchestrator does.
#![cfg(unix)]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How long the daemon is given to stop once signalled. The claim below waits
/// for sixty seconds, so a daemon that waits it out cannot pass, and a healthy
/// stop takes well under a second.
const STOP_LIMIT: Duration = Duration::from_secs(15);

/// A port the operating system just handed out, released for the daemon to
/// bind. A hardcoded port collides with whatever else is on the machine.
fn free_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind a free port");
    l.local_addr().expect("the bound address").port()
}

/// One HTTP/1.1 exchange over a fresh connection, returning everything read.
fn exchange(port: u16, request: &str, timeout: Duration) -> std::io::Result<String> {
    let mut s = TcpStream::connect(("127.0.0.1", port))?;
    s.set_read_timeout(Some(timeout))?;
    s.write_all(request.as_bytes())?;
    let mut out = String::new();
    s.read_to_string(&mut out)?;
    Ok(out)
}

#[test]
fn sigterm_with_a_claim_long_poll_open_stops_promptly_and_answers_the_claim() {
    let port = free_port();
    let mut child = Command::new(env!("CARGO_BIN_EXE_mcpd"))
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .args(["--data", ":memory:", "--listen", &format!("127.0.0.1:{port}")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start mcpd");

    // Wait for the daemon to answer before opening the long-poll.
    let started = Instant::now();
    loop {
        let health = "GET /healthz HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n";
        if exchange(port, health, Duration::from_secs(2)).is_ok_and(|r| r.contains(" 200 ")) {
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(20), "mcpd never answered /healthz");
        std::thread::sleep(Duration::from_millis(50));
    }

    let body = r#"{"worker_id":"shutdown-test","max":1,"wait_ms":60000}"#;
    let claim = format!(
        "POST /v1/api/steps/claim HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let poll = std::thread::spawn(move || {
        exchange(port, &claim, Duration::from_secs(90)).unwrap_or_default()
    });
    // Long enough for the request to be inside the handler's wait.
    std::thread::sleep(Duration::from_millis(500));

    let signalled = Instant::now();
    let sent = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .expect("send SIGTERM");
    assert!(sent.success(), "kill -TERM failed");

    let status = loop {
        if let Some(status) = child.try_wait().expect("poll mcpd") {
            break status;
        }
        if signalled.elapsed() > STOP_LIMIT {
            let _ = child.kill();
            let _ = child.wait();
            panic!("mcpd was still running {STOP_LIMIT:?} after SIGTERM with a claim open");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(status.success(), "mcpd exited with {status} after SIGTERM");

    // The worker is answered, with no lease, rather than having its
    // connection cut: it reconnects to the next replica and claims again.
    let answer = poll.join().expect("the claim thread");
    assert!(answer.starts_with("HTTP/1.1 200"), "the claim was not answered: {answer:?}");
    assert!(answer.contains(r#""leases":[]"#), "{answer}");
}
