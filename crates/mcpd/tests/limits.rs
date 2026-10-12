//! The two per-request bounds, through the built binary.
//!
//! Both existed on `ServerConfig` and neither was reachable: no flag set
//! either and the config file declared neither key, so an operator whose
//! batches were being refused could only change the number by rebuilding. The
//! tests here drive the whole path a setting takes, from the command line, the
//! environment and the file, to the line `mcpd config` prints back.
//!
//! Its own file rather than an addition to `config.rs`, so the two suites can
//! be worked on at once. The three helpers below are the ones that file uses,
//! for the same reasons.

use std::process::Command;

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

fn config_output(args: &[&str]) -> String {
    let out = mcpd().arg("config").args(args).output().expect("run mcpd config");
    assert!(
        out.status.success(),
        "mcpd config {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf-8 output")
}

fn refusal(args: &[&str]) -> String {
    let out = mcpd().arg("config").args(args).output().expect("run mcpd config");
    assert!(
        !out.status.success(),
        "mcpd config {} was accepted and should have been refused:\n{}",
        args.join(" "),
        String::from_utf8_lossy(&out.stdout)
    );
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn line<'a>(output: &'a str, label: &str) -> &'a str {
    output
        .lines()
        .find(|l| l.starts_with(label))
        .unwrap_or_else(|| panic!("no {label:?} line in:\n{output}"))
}

/// A config file holding `body` bytes and `signals` per batch, written where
/// only this test can find it. Named from the pid and a per-process stamp,
/// because pids are recycled and a recycled one reopens another run's file.
fn config_file(body: Option<usize>, signals: Option<usize>) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    static STAMP: std::sync::OnceLock<u128> = std::sync::OnceLock::new();
    let stamp = STAMP.get_or_init(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    });
    let dir = std::env::temp_dir().join(format!("mcpd-limits-test-{}-{stamp}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create the test directory");
    let path = dir.join(format!("{}.json", N.fetch_add(1, Ordering::Relaxed)));
    let mut keys: Vec<String> = Vec::new();
    if let Some(n) = body {
        keys.push(format!("\"max_body_bytes\": {n}"));
    }
    if let Some(n) = signals {
        keys.push(format!("\"max_batch_signals\": {n}"));
    }
    std::fs::write(&path, format!("{{ {} }}", keys.join(", "))).expect("write the config file");
    path
}

#[test]
fn the_defaults_are_read_back_in_the_units_they_were_written_in() {
    let out = config_output(&[]);
    assert_eq!(line(&out, "limits"), "limits          32 MiB per request, 10000 signals per batch");
}

#[test]
fn each_cap_is_read_back_by_the_line_it_prints() {
    let out = config_output(&["--max-body-bytes", "1048576", "--max-batch-signals", "250"]);
    assert_eq!(line(&out, "limits"), "limits          1 MiB per request, 250 signals per batch");
}

/// A number that is not a whole unit is printed as itself rather than rounded
/// into one, or the line would report a bound the daemon is not enforcing.
#[test]
fn a_cap_that_is_not_a_whole_unit_is_printed_as_bytes() {
    let out = config_output(&["--max-body-bytes", "1500000"]);
    assert_eq!(
        line(&out, "limits"),
        "limits          1500000 bytes per request, 10000 signals per batch"
    );
}

#[test]
fn the_variables_are_read_the_same_way() {
    let out = mcpd()
        .arg("config")
        .env("MCPD_MAX_BODY_BYTES", "65536")
        .env("MCPD_MAX_BATCH_SIGNALS", "40")
        .output()
        .expect("run mcpd config");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let out = String::from_utf8(out.stdout).expect("utf-8 output");
    assert_eq!(line(&out, "limits"), "limits          64 KiB per request, 40 signals per batch");
}

#[test]
fn the_config_file_sets_both_of_them_too() {
    let path = config_file(Some(2 * 1024 * 1024), Some(500));
    let out = config_output(&["--config-file", path.to_str().expect("a utf-8 path")]);
    assert_eq!(line(&out, "limits"), "limits          2 MiB per request, 500 signals per batch");
}

/// A flag beats the file, so one instance can be given a different bound
/// without editing the ConfigMap every other instance shares.
#[test]
fn a_flag_beats_the_file() {
    let path = config_file(Some(2 * 1024 * 1024), Some(500));
    let out = config_output(&[
        "--config-file",
        path.to_str().expect("a utf-8 path"),
        "--max-batch-signals",
        "7",
    ]);
    assert_eq!(
        line(&out, "limits"),
        "limits          2 MiB per request, 7 signals per batch",
        "the flag should have replaced the file's signal cap and left its body cap alone"
    );
}

/// Zero is a plausible way to write "no limit" and it means the opposite, so
/// it is refused with the name of the thing to change.
#[test]
fn a_cap_of_zero_is_refused_from_either_way_in() {
    let err = refusal(&["--max-body-bytes", "0"]);
    assert!(err.contains("--max-body-bytes must be at least 1"), "{err}");
    let err = refusal(&["--max-batch-signals", "0"]);
    assert!(err.contains("--max-batch-signals must be at least 1"), "{err}");

    let path = config_file(None, Some(0));
    let err = refusal(&["--config-file", path.to_str().expect("a utf-8 path")]);
    assert!(err.contains("--max-batch-signals must be at least 1"), "{err}");
}
