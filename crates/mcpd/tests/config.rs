//! `mcpd config`, through the built binary.
//!
//! The four flags this covers each wire a setting into the daemon, and the
//! only place an operator can read back what an invocation resolved to is
//! this output. A flag that parses and prints nothing is a flag nobody can
//! confirm is set, so each one is checked here by the line it prints, and
//! each refusal by the message it exits with.

use std::process::Command;

/// The binary, with every `MCPD_` variable of the calling environment
/// removed so a setting exported in the shell cannot change an answer.
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

#[test]
fn the_defaults_are_same_origin_thirty_seconds_and_redaction_on() {
    let out = config_output(&[]);
    assert_eq!(line(&out, "cors"), "cors            same-origin only");
    assert_eq!(line(&out, "timeout"), "timeout         30s per request");
    assert_eq!(line(&out, "redaction"), "redaction       on");
}

#[test]
fn each_flag_is_read_back_by_the_line_it_prints() {
    let out = config_output(&[
        "--cors-origin",
        "https://Console.Example",
        "--cors-origin",
        "http://localhost:4321",
        "--request-timeout-seconds",
        "5",
        "--redact-key-contains",
        "ticket,Case_Ref",
        "--redact-max-string",
        "4096",
    ]);
    // Lowercased, because a browser sends its origin lowercased and the
    // match is exact.
    assert_eq!(
        line(&out, "cors"),
        "cors            https://console.example, http://localhost:4321"
    );
    assert_eq!(line(&out, "timeout"), "timeout         5s per request");
    assert_eq!(
        line(&out, "redaction"),
        "redaction       on, also masking keys containing ticket, case_ref, strings cut at 4096 bytes"
    );
}

#[test]
fn the_variables_are_read_the_same_way() {
    let out = mcpd()
        .arg("config")
        .env("MCPD_CORS_ORIGINS", "https://a.example,https://b.example")
        .env("MCPD_REQUEST_TIMEOUT_SECONDS", "90")
        .env("MCPD_REDACT_KEY_CONTAINS", "account")
        .env("MCPD_REDACT_MAX_STRING", "2048")
        .output()
        .expect("run mcpd config");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let out = String::from_utf8(out.stdout).expect("utf-8 output");
    assert_eq!(line(&out, "cors"), "cors            https://a.example, https://b.example");
    assert_eq!(line(&out, "timeout"), "timeout         90s per request");
    assert_eq!(
        line(&out, "redaction"),
        "redaction       on, also masking keys containing account, strings cut at 2048 bytes"
    );
}

#[test]
fn a_star_is_read_back_as_any_origin() {
    let out = config_output(&["--cors-origin", "*"]);
    assert_eq!(line(&out, "cors"), "cors            any origin");
}

#[test]
fn a_string_that_is_not_an_origin_is_refused_with_the_flag_named() {
    let err = refusal(&["--cors-origin", "console.example"]);
    assert!(err.contains("--cors-origin"), "{err}");
    assert!(err.contains("console.example"), "{err}");
    let err = refusal(&["--cors-origin", "https://console.example/"]);
    assert!(err.contains("drop everything after the host"), "{err}");
}

#[test]
fn a_deadline_of_zero_is_refused_rather_than_read_as_no_deadline() {
    let err = refusal(&["--request-timeout-seconds", "0"]);
    assert!(err.contains("--request-timeout-seconds must be at least 1"), "{err}");
}

#[test]
fn a_redaction_addition_beside_no_redact_is_a_contradiction_not_a_silent_no_op() {
    let err = refusal(&["--no-redact", "--redact-key-contains", "ticket"]);
    assert!(err.contains("--no-redact"), "{err}");
    assert!(err.contains("--redact-key-contains"), "{err}");
    let err = refusal(&["--no-redact", "--redact-max-string", "10"]);
    assert!(err.contains("--redact-max-string"), "{err}");
    let err = refusal(&["--redact-max-string", "0"]);
    assert!(err.contains("--redact-max-string must be at least 1"), "{err}");
}
