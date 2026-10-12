//! What the daemon checks about its own configuration before it binds.
//!
//! `mcpd doctor` has always found these mistakes. Nothing ran it. A judge block
//! with no criteria makes every judged scorer decline for ever with `is not a
//! configured criterion`, and the daemon carrying it answers its health check,
//! serves every route and reads as well from every angle, so the operator who
//! most needs the report is the one who never thinks to ask for it.
//!
//! These tests drive the built binary, because the defect was never in the
//! check. It was that `serve` did not call it.

use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// How long a refusal is given to arrive. The preflight reads a struct already
/// in memory, so a healthy one takes microseconds; this is slack for a loaded
/// machine, not a measurement.
const REFUSAL_LIMIT: Duration = Duration::from_secs(20);

/// How long a daemon that should start is watched for, to prove it did not
/// refuse. It cannot pass by being slow: still running is the answer this
/// wants, so a machine under load makes this test pass rather than flake.
const SERVING_LIMIT: Duration = Duration::from_secs(3);

/// The binary, with every `MCPD_` variable of the calling environment removed
/// so a setting exported in the shell cannot change an answer.
fn mcpd() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_mcpd"));
    for (k, _) in std::env::vars() {
        if k.starts_with("MCPD_") {
            c.env_remove(k);
        }
    }
    c
}

/// Write `json` to a config file of its own and return the path.
///
/// The directory carries this process's id and a stamp taken once, because pids
/// are recycled and a recycled one would otherwise read the file an earlier
/// suite left behind.
fn config_file(name: &str, json: &str) -> std::path::PathBuf {
    static STAMP: std::sync::OnceLock<u128> = std::sync::OnceLock::new();
    let stamp = STAMP.get_or_init(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    });
    let dir = std::env::temp_dir().join(format!("mcpd-preflight-{}-{stamp}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create the test directory");
    let path = dir.join(format!("{name}.json"));
    let mut f = std::fs::File::create(&path).expect("create the config file");
    f.write_all(json.as_bytes()).expect("write the config file");
    path
}

/// Start the daemon against `config`.
///
/// Port 0, so the operating system assigns one: a hardcoded port collides with
/// whatever else is on the machine and produces convincing false failures. The
/// store is in memory, so a refusal leaves nothing on disk to clear up.
fn serve(config: &std::path::Path) -> Child {
    mcpd()
        .args([
            "--data",
            ":memory:",
            "--runs-data",
            "off",
            "--listen",
            "127.0.0.1:0",
            "--config-file",
            config.to_str().expect("a utf-8 path"),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start mcpd")
}

/// Everything the process wrote to standard error, after it has ended.
fn stderr_of(child: Child) -> String {
    let out = child.wait_with_output().expect("read what mcpd wrote");
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// Wait for the daemon to refuse, and return what it said.
///
/// Kills it and fails at the deadline rather than waiting for ever, so a
/// regression here is a red test and not a suite that hangs.
fn refusal(config: &std::path::Path) -> String {
    let mut child = serve(config);
    let started = Instant::now();
    loop {
        match child.try_wait().expect("poll mcpd") {
            Some(status) => {
                let said = stderr_of(child);
                assert!(!status.success(), "mcpd exited 0 rather than refusing:\n{said}");
                return said;
            }
            None if started.elapsed() >= REFUSAL_LIMIT => {
                let _ = child.kill();
                let said = stderr_of(child);
                panic!("mcpd started and served this configuration rather than refusing:\n{said}");
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

/// Prove the daemon does not refuse this configuration, then stop it.
fn serves(config: &std::path::Path) {
    let mut child = serve(config);
    let started = Instant::now();
    while started.elapsed() < SERVING_LIMIT {
        if child.try_wait().expect("poll mcpd").is_some() {
            panic!("mcpd refused a configuration it should serve:\n{}", stderr_of(child));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// A judge that can never produce a score, which is the shape the defect was
/// found in.
const NO_CRITERIA: &str = r#"{
  "judge": {
    "endpoint": "https://api.example.invalid/v1",
    "model": "a-model",
    "api_key": "k",
    "criteria": {}
  }
}"#;

/// The same judge with one criterion, which is a judge that works.
const ONE_CRITERION: &str = r#"{
  "judge": {
    "endpoint": "https://api.example.invalid/v1",
    "model": "a-model",
    "api_key": "k",
    "criteria": { "helpfulness": "is the answer helpful" }
  }
}"#;

#[test]
fn serving_a_judge_that_can_never_answer_is_refused_at_startup() {
    let said = refusal(&config_file("no-criteria", NO_CRITERIA));
    assert!(said.contains("will not start"), "the refusal does not say so:\n{said}");
    assert!(said.contains("judge.criteria"), "the refusal does not name the fix:\n{said}");
}

#[test]
fn the_same_judge_with_a_criterion_starts() {
    // The control. Without it the test above would pass for a daemon that
    // refuses every config file, which is a different defect.
    serves(&config_file("one-criterion", ONE_CRITERION));
}

#[test]
fn a_sample_a_project_sets_outside_the_scale_is_refused_at_startup() {
    // `--sample 4.2` has always been refused. The same number written under a
    // project in the config file started, and the daemon then ran a fraction
    // that means nothing.
    let said =
        refusal(&config_file("bad-sample", r#"{ "projects": { "checkout": { "sample": 4.2 } } }"#));
    assert!(said.contains("outside 0.0 to 1.0"), "the refusal does not name the value:\n{said}");
    assert!(said.contains("checkout"), "the refusal does not name the project:\n{said}");
}

#[test]
fn mcpd_config_still_prints_the_settings_a_start_would_refuse() {
    // `mcpd config` and `mcpd doctor` are how you find out what is wrong. If
    // the preflight ran before them, the two commands that exist to explain a
    // bad configuration would be the two that will not run against one.
    let path = config_file("no-criteria-config", NO_CRITERIA);
    let out = mcpd()
        .arg("config")
        .args(["--config-file", path.to_str().expect("a utf-8 path")])
        .output()
        .expect("run mcpd config");
    assert!(
        out.status.success(),
        "mcpd config refused a configuration it exists to explain: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let printed = String::from_utf8_lossy(&out.stdout);
    assert!(printed.contains("judge           a-model"), "no judge line in:\n{printed}");
}
