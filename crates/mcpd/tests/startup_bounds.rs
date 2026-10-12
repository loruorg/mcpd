//! Settings that used to be accepted and then took the daemon down, or did
//! the opposite of what they said, refused at start-up through the built
//! binary.
//!
//! Each case runs `mcpd config`, which resolves every setting the way `serve`
//! does and then prints instead of serving. A refusal there is a refusal to
//! start, and it is checked here by its message, because the message is the
//! whole of the fix: it has to name the setting and say what to write instead.
//!
//! Its own file rather than an addition to `config.rs` or `limits.rs`, so the
//! suites can be worked on at once. The helpers are the ones those files use,
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

/// A config file holding `body`, written where only this test can find it.
/// Named from the pid and a per-process stamp, because pids are recycled and
/// a recycled one reopens another run's file.
fn config_file(body: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    static STAMP: std::sync::OnceLock<u128> = std::sync::OnceLock::new();
    let stamp = STAMP.get_or_init(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    });
    let dir = std::env::temp_dir().join(format!("mcpd-bounds-test-{}-{stamp}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create the test directory");
    let path = dir.join(format!("{}.json", N.fetch_add(1, Ordering::Relaxed)));
    std::fs::write(&path, body).expect("write the config file");
    path.to_string_lossy().into_owned()
}

// ------------------------------------------------------------- retention ---

/// Zero reads like "no retention" and meant a cutoff of now: the pass that
/// runs at boot deleted everything the project held.
#[test]
fn a_retention_of_zero_days_is_refused_with_the_way_to_keep_everything() {
    let err = refusal(&["--retention-days", "0"]);
    assert!(err.contains("--retention-days"), "the flag is not named: {err}");
    assert!(err.contains("at least 1"), "{err}");
    assert!(err.contains("omit it"), "the way to keep everything is not said: {err}");

    let out =
        mcpd().arg("config").env("MCPD_RETENTION_DAYS", "0").output().expect("run mcpd config");
    assert!(!out.status.success(), "the environment is the other way in to the same flag");
}

#[test]
fn a_project_retention_of_zero_days_is_refused_naming_the_project() {
    let file = config_file(r#"{ "projects": { "checkout": { "retention_days": 0 } } }"#);
    let err = refusal(&["--config-file", &file]);
    assert!(err.contains("project checkout retention_days"), "{err}");
    assert!(err.contains("omit it"), "{err}");
}

/// The cutoff is now minus the window, and a window past the calendar's range
/// panicked the first pass.
#[test]
fn a_retention_past_the_bound_is_refused_naming_the_bound() {
    let err = refusal(&["--retention-days", "4294967295"]);
    assert!(err.contains("at most 36500"), "the bound is not named: {err}");
    let file = config_file(r#"{ "projects": { "support": { "retention_days": 36501 } } }"#);
    let err = refusal(&["--config-file", &file]);
    assert!(err.contains("project support retention_days"), "{err}");
    assert!(err.contains("at most 36500"), "{err}");
}

#[test]
fn a_retention_at_the_bound_is_accepted() {
    let out = config_output(&["--retention-days", "36500"]);
    assert!(out.contains("retention       36500 days"), "{out}");
}

// ------------------------------------------------------------- run timings ---

/// A duration past the run store's own bound was accepted, and then every
/// claim failed "out of range" with nothing in the start-up log to say why.
#[test]
fn a_run_timing_past_the_run_store_bound_is_refused_naming_the_key_and_the_bound() {
    for key in [
        "lease_ttl_ms",
        "reclaim_window_ms",
        "stall_window_ms",
        "idempotency_window_ms",
        "park_ttl_ms",
    ] {
        let file = config_file(&format!(r#"{{ "runs": {{ "{key}": 300000000000000 }} }}"#));
        let err = refusal(&["--config-file", &file]);
        assert!(err.contains(&format!("runs.{key}")), "the key is not named: {err}");
        assert!(err.contains("at most 315360000000"), "the bound is not named: {err}");
        assert!(err.contains("got 300000000000000"), "the value is not named: {err}");
    }
    // At the bound it is accepted.
    let file = config_file(r#"{ "runs": { "lease_ttl_ms": 315360000000 } }"#);
    config_output(&["--config-file", &file]);
}

// ------------------------------------------------------ objectives, aspects ---

fn objective(fields: &str) -> String {
    config_file(&format!(
        r#"{{ "slos": [ {{ "id": "checkout-success", "name": "Checkout succeeds",
             "project": "checkout", {fields} }} ] }}"#
    ))
}

/// `mcpd doctor` grades an objective of one or more as a fault, and the
/// daemon started with it anyway and burned on every run.
#[test]
fn an_objective_that_is_not_a_fraction_below_one_is_refused() {
    for value in ["1.0", "99.5", "-0.1"] {
        let file = objective(&format!(r#""objective": {value}"#));
        let err = refusal(&["--config-file", &file]);
        assert!(err.contains("objective checkout-success"), "{value}: {err}");
        assert!(err.contains("0.995"), "{value}: the way to write it is not shown: {err}");
    }
}

/// The window's start is now minus the window, and a window past the
/// calendar's range panicked the monitor timer.
#[test]
fn an_objective_window_outside_its_range_is_refused_naming_the_range() {
    for days in ["0", "366", "4294967295"] {
        let file = objective(&format!(r#""objective": 0.995, "window_days": {days}"#));
        let err = refusal(&["--config-file", &file]);
        assert!(err.contains("window_days"), "{days}: {err}");
        assert!(err.contains("from 1 to 365"), "{days}: the range is not named: {err}");
    }
    let file = objective(r#""objective": 0.995, "window_days": 365"#);
    let out = config_output(&["--config-file", &file]);
    assert!(out.contains("objectives      1, multi-window burn"), "{out}");
}

#[test]
fn a_monitor_interval_past_a_day_is_refused() {
    let file = config_file(r#"{ "monitor_interval_seconds": 18446744073709551615 }"#);
    let err = refusal(&["--config-file", &file]);
    assert!(err.contains("monitor_interval_seconds must be at most 86400"), "{err}");
}

/// The same subtraction, in the aspect pass.
#[test]
fn an_aspect_window_outside_its_range_is_refused_naming_the_range() {
    for seconds in ["0", "31536001", "18446744073709551615"] {
        let file = config_file(&format!(
            r#"{{ "aspects": {{ "enabled": true, "window_seconds": {seconds} }} }}"#
        ));
        let err = refusal(&["--config-file", &file]);
        assert!(err.contains("aspects.window_seconds"), "{seconds}: {err}");
        assert!(err.contains("from 1 to 31536000"), "{seconds}: {err}");
    }
    let file = config_file(r#"{ "aspects": { "enabled": true, "interval_seconds": 604801 } }"#);
    let err = refusal(&["--config-file", &file]);
    assert!(err.contains("aspects.interval_seconds must be at most 604800"), "{err}");
}

// ----------------------------------------------------------------- judge ---

fn judged(timeout: &str) -> String {
    config_file(&format!(
        r#"{{ "judge": {{ "endpoint": "http://localhost:11434/v1", "model": "a-judge",
             "criteria": {{ "relevance": "Is it relevant?" }}{timeout} }} }}"#
    ))
}

#[test]
fn a_judge_timeout_outside_its_range_is_refused_naming_the_range() {
    for seconds in ["0", "301"] {
        let file = judged(&format!(r#", "timeout_seconds": {seconds}"#));
        let err = refusal(&["--config-file", &file]);
        assert!(err.contains("judge.timeout_seconds must be from 1 to 300"), "{seconds}: {err}");
    }
}

#[test]
fn the_judge_timeout_defaults_to_thirty_seconds_and_is_read_back() {
    let out = config_output(&["--config-file", &judged("")]);
    assert!(out.contains("30s per call"), "{out}");
    let out = config_output(&["--config-file", &judged(r#", "timeout_seconds": 120"#)]);
    assert!(out.contains("120s per call"), "{out}");
}

// ------------------------------------------------------- forward headers ---

/// A header with no colon was dropped without a word, so the OTel form
/// `key=value` never reached the destination and every batch it was meant to
/// authorise was refused there.
#[test]
fn a_forward_header_with_no_colon_is_refused_with_the_form_that_works() {
    let err = refusal(&[
        "--forward-endpoint",
        "http://collector.example",
        "--forward-header",
        "Authorization=Bearer sk-1",
    ]);
    assert!(err.contains("--forward-header"), "the flag is not named: {err}");
    assert!(err.contains("Name: value"), "the expected form is not shown: {err}");
    assert!(
        err.contains("Authorization: Bearer sk-1"),
        "the corrected header is not offered: {err}"
    );
}

#[test]
fn a_forward_header_with_a_colon_is_still_accepted() {
    let out = config_output(&[
        "--forward-endpoint",
        "http://collector.example",
        "--forward-header",
        "Authorization: Bearer sk-1",
    ]);
    assert!(out.contains("forwarding      http://collector.example/v1/traces"), "{out}");
}

#[test]
fn a_forward_header_name_no_client_can_send_is_refused() {
    let err = refusal(&[
        "--forward-endpoint",
        "http://collector.example",
        "--forward-header",
        "X Api Key: abc",
    ]);
    assert!(err.contains("not a valid header name"), "{err}");
}
