//! `mcpd config` prints every setting, and no secret.
//!
//! The command exists so an operator can read back what an invocation resolved
//! to without reading the source. Eleven settings had no line at all, two lines
//! printed a value the daemon does not run with, and four printed a credential
//! in full: a ClickHouse URL, a Postgres connection string in `key=value` form,
//! and the judge and forward endpoints.

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

fn config(args: &[&str]) -> String {
    let out = mcpd().arg("config").args(args).output().expect("run mcpd config");
    assert!(
        out.status.success(),
        "mcpd config {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf-8 output")
}

fn line<'a>(output: &'a str, label: &str) -> &'a str {
    output
        .lines()
        .find(|l| l.split("  ").next() == Some(label))
        .unwrap_or_else(|| panic!("no {label:?} line in:\n{output}"))
}

/// Write `json` to a file of its own and return the path. The directory
/// carries the pid and a stamp, because a recycled pid would otherwise read a
/// file an earlier run left behind.
fn config_file(name: &str, json: &str) -> String {
    static STAMP: std::sync::OnceLock<u128> = std::sync::OnceLock::new();
    let stamp = STAMP.get_or_init(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    });
    let dir =
        std::env::temp_dir().join(format!("mcpd-config-output-{}-{stamp}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create the test directory");
    let path = dir.join(format!("{name}.json"));
    std::fs::write(&path, json).expect("write the config file");
    path.to_str().expect("a utf-8 path").to_owned()
}

/// Every secret the invocations below carry. None may appear in the output.
const SECRETS: [&str; 9] = [
    "pg-password-one",
    "pg-password-two",
    "ch-url-password",
    "ch-flag-password",
    "judge-query-key",
    "judge-field-key",
    "forward-query-token",
    "forward-header-token",
    "project-key-value",
];

/// A file that sets every structured setting, with a secret wherever one fits.
const EVERYTHING: &str = r#"{
  "monitor_interval_seconds": 2,
  "monitors": [{
    "id": "errors", "name": "errors", "project": "checkout", "watch": "error_rate",
    "compare": "above", "threshold": 0.1, "enabled": false
  }],
  "slos": [{ "id": "up", "name": "up", "project": "checkout", "objective": 0.99 }],
  "judge": {
    "endpoint": "https://judge.example.invalid/v1?api_key=judge-query-key",
    "model": "a-model",
    "api_key": "judge-field-key",
    "criteria": { "helpful": "is it helpful" }
  },
  "forward": {
    "endpoint": "https://otlp:forward-query-token@otlp.example.invalid",
    "headers": [["Authorization", "Bearer forward-header-token"], ["X-Scope-OrgID", "a"]],
    "batch_size": 100
  },
  "projects": {
    "checkout": {
      "ingest_keys": ["project-key-value"],
      "read_keys": ["project-key-value-2", "project-key-value-3"],
      "retention_days": 7,
      "sample": 0.25
    },
    "support": {}
  },
  "runs": { "lease_ttl_ms": 45000, "max_depth": 4, "max_step_repeats": 0 },
  "derived_metrics": { "tokens": false },
  "aspects": { "enabled": true, "interval_seconds": 10, "disabled": ["sentiment"] }
}"#;

#[test]
fn no_password_appears_in_the_output_in_any_form_it_can_be_given() {
    let file = config_file("everything", EVERYTHING);
    let outputs = [
        config(&["--postgres-url", "postgres://mcpd:pg-password-one@db.invalid:5432/mcpd"]),
        config(&[
            "--postgres-url",
            "host=db.invalid user=mcpd password=pg-password-two dbname=mcpd",
        ]),
        config(&[
            "--clickhouse-url",
            "http://default:ch-url-password@clickhouse.invalid:8123",
            "--clickhouse-user",
            "mcpd",
            "--clickhouse-password",
            "ch-flag-password",
        ]),
        config(&["--config-file", &file]),
        config(&[
            "--forward-endpoint",
            "https://otlp.example.invalid?token=forward-query-token",
            "--forward-header",
            "Authorization: Bearer forward-header-token",
        ]),
    ];
    for out in &outputs {
        for secret in SECRETS {
            assert!(!out.contains(secret), "{secret} was printed:\n{out}");
        }
    }
    assert_eq!(
        line(&outputs[1], "storage"),
        "storage         postgres at host=db.invalid user=mcpd password=[redacted] dbname=mcpd"
    );
    assert_eq!(
        line(&outputs[2], "storage"),
        "storage         clickhouse at http://clickhouse.invalid:8123 (database mcpd, user mcpd, \
         password set)"
    );
}

#[test]
fn every_structured_setting_is_read_back_by_a_line_of_its_own() {
    let file = config_file("everything-lines", EVERYTHING);
    let out = config(&["--config-file", &file, "--retention-days", "30", "--sample", "0.5"]);

    assert_eq!(line(&out, "config file"), format!("config file     {file}"));
    assert_eq!(line(&out, "projects"), "projects        checkout (3 keys), support (no keys)");
    assert_eq!(line(&out, "retention"), "retention       30 days, except checkout 7 days");
    assert_eq!(line(&out, "sampling"), "sampling        deterministic 0.5, except checkout 0.25");
    assert_eq!(
        line(&out, "run settings"),
        "run settings    lease_ttl_ms 45000, reclaim_window_ms 30000, stall_window_ms 300000, \
         idempotency_window_ms 86400000, park_ttl_ms 86400000, max_depth 4, max_step_repeats 0 (off)"
    );
    assert_eq!(line(&out, "derived"), "derived         duration metrics only");
    assert_eq!(
        line(&out, "aspects"),
        "aspects         on, every 30s (raised from 10s, the shortest interval) over the last \
         86400s, not clustering sentiment"
    );
    // The interval the timer runs on, not the one written under the floor.
    assert_eq!(
        line(&out, "monitors"),
        "monitors        1 (1 disabled), every 5s (raised from 2s, the shortest interval)"
    );
    assert!(line(&out, "objectives").contains("every 5s"), "{out}");
    assert_eq!(
        line(&out, "judge"),
        "judge           a-model at https://judge.example.invalid/v1?api_key=[redacted] (1 criteria, 30s per call)"
    );
    assert_eq!(
        line(&out, "forwarding"),
        "forwarding      otlp at https://otlp.example.invalid/v1/traces (json, 100 per request), \
         headers Authorization, X-Scope-OrgID; spool at ./mcpd.spool, 256 MiB each"
    );
    assert_eq!(line(&out, "relay"), "relay           off: no --upstream");
}

#[test]
fn several_destinations_and_a_relay_are_each_read_back() {
    let file = config_file(
        "forward-many",
        r#"{ "forward": [
            { "vendor": "honeycomb", "api_key": "hc-secret-key", "region": "eu" },
            { "vendor": "otlp_grpc", "name": "collector", "endpoint": "http://collector:4317" }
        ] }"#,
    );
    let out = config(&[
        "--config-file",
        &file,
        "--spool-dir",
        "/var/spool/mcpd",
        "--upstream",
        "https://a.example.invalid,https://user:up-secret@b.example.invalid",
        "--upstream-key",
        "upstream-secret",
    ]);
    for secret in ["hc-secret-key", "up-secret", "upstream-secret"] {
        assert!(!out.contains(secret), "{secret} was printed:\n{out}");
    }
    assert_eq!(
        line(&out, "forwarding"),
        "forwarding      honeycomb at https://api.eu1.honeycomb.io/v1/traces (protobuf, 256 per \
         request), headers x-honeycomb-team; collector at \
         http://collector:4317/opentelemetry.proto.collector.trace.v1.TraceService/Export (grpc, \
         256 per request); spool at /var/spool/mcpd, 256 MiB each"
    );
    assert_eq!(
        line(&out, "relay"),
        "relay           2 upstream(s) in order, https://a.example.invalid, \
         https://b.example.invalid; upstream key set; spool at /var/spool/mcpd/relay, 1024 MiB"
    );
}

#[test]
fn a_destination_naming_an_unset_variable_refuses_to_start_and_names_it() {
    let file = config_file(
        "forward-env",
        r#"{ "forward": { "vendor": "datadog", "api_key": "${MCPD_TEST_UNSET_DD_KEY}" } }"#,
    );
    let out = mcpd().args(["config", "--config-file", &file]).output().expect("run");
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("MCPD_TEST_UNSET_DD_KEY") && err.contains("not set"), "{err}");
}

#[test]
fn a_forward_block_that_is_switched_off_reads_as_off() {
    // The line printed the destination whatever `enabled` said, so a
    // deployment that had paused forwarding read as forwarding.
    let file = config_file(
        "forward-off",
        r#"{ "forward": { "endpoint": "https://otlp.example.invalid", "enabled": false } }"#,
    );
    let out = config(&["--config-file", &file]);
    assert_eq!(
        line(&out, "forwarding"),
        "forwarding      off: the forward block sets enabled to false"
    );
}

#[test]
fn the_flags_that_printed_nothing_now_print_a_line() {
    let out = config(&["--log-format", "json", "--redact-key", "a-hashing-key-of-our-own"]);
    assert_eq!(line(&out, "log format"), "log format      json");
    assert_eq!(line(&out, "redact key"), "redact key      set");
    assert!(!out.contains("a-hashing-key-of-our-own"), "{out}");

    let out = config(&["--log-format", "jsno"]);
    assert_eq!(
        line(&out, "log format"),
        "log format      text (\"jsno\" is not a format; use text or json)"
    );
    assert_eq!(
        line(&out, "redact key"),
        "redact key      built in, which is public; set --redact-key"
    );
    assert_eq!(line(&out, "config file"), "config file     none");
}
