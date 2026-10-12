//! What the daemon makes of the keys it is given, through the built binary.
//!
//! Four defects, each one in how a list is read rather than how a key is
//! compared. An empty environment variable became an empty key that an empty
//! header matched. Dropping that entry then left the list unset, so the same
//! variable opened the scope it was set to close. A placeholder copied from an example file started a daemon
//! whose key is published in this repository. And an ingest key that also
//! opened the run surface was reported in the tone of a closed scope.

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How long a refusal or a start is given to show itself. Slack for a loaded
/// machine, not a measurement.
const LIMIT: Duration = Duration::from_secs(20);

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

fn line<'a>(output: &'a str, label: &str) -> &'a str {
    output
        .lines()
        .find(|l| l.starts_with(label))
        .unwrap_or_else(|| panic!("no {label:?} line in:\n{output}"))
}

/// `mcpd config` with these arguments and these variables.
fn config(args: &[&str], env: &[(&str, &str)]) -> String {
    let out = mcpd().arg("config").args(args).envs(env.iter().copied()).output().expect("run");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).expect("utf-8 output")
}

/// Start a daemon on an in-memory store and port 0, and return what it wrote
/// once it said it was listening, or once it exited.
///
/// Port 0, so the operating system assigns one and nothing else on the machine
/// can collide with it. The store is in memory, so nothing is left behind.
fn start(args: &[&str]) -> (Option<std::process::ExitStatus>, String) {
    start_with(args, &[])
}

/// [`start`], with these variables set as well.
fn start_with(args: &[&str], env: &[(&str, &str)]) -> (Option<std::process::ExitStatus>, String) {
    let mut child = mcpd()
        .args(["--data", ":memory:", "--listen", "127.0.0.1:0"])
        .args(args)
        .envs(env.iter().copied())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start mcpd");
    let stdout = child.stdout.take().expect("stdout");
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for l in BufReader::new(stdout).lines().map_while(Result::ok) {
            if tx.send(l).is_err() {
                return;
            }
        }
    });
    let started = Instant::now();
    let mut said = String::new();
    loop {
        if let Some(status) = child.try_wait().expect("poll mcpd") {
            while let Ok(l) = rx.try_recv() {
                said.push_str(&l);
                said.push('\n');
            }
            let err = child.wait_with_output().expect("collect mcpd").stderr;
            said.push_str(&String::from_utf8_lossy(&err));
            return (Some(status), said);
        }
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(l) => {
                let listening = l.contains("listening on");
                said.push_str(&l);
                said.push('\n');
                if listening {
                    let _ = child.kill();
                    let _ = child.wait();
                    return (None, said);
                }
            }
            Err(_) if started.elapsed() > LIMIT => {
                let _ = child.kill();
                panic!("mcpd neither started nor refused within {LIMIT:?}:\n{said}");
            }
            Err(_) => {}
        }
    }
}

// ------------------------------------------------------------- blank keys --

#[test]
fn mcpd_config_prints_an_empty_variable_as_the_unset_list_the_server_would_enforce() {
    // clap reads `MCPD_EXECUTE_KEYS=` as one empty key, and `mcpd config`
    // printed `key required` for a scope an empty header could open. The
    // start refuses this now (below); `mcpd config` still shows what the lists
    // resolve to, because it exists to show the settings you got wrong.
    let out = config(
        &["--ingest-key", "the-fleets-ingest-key-0123"],
        &[("MCPD_EXECUTE_KEYS", ""), ("MCPD_WORKER_KEYS", ""), ("MCPD_WRITE_KEYS", "")],
    );
    assert_eq!(line(&out, "execute auth"), "execute auth    key required (ingest keys)");
    assert_eq!(line(&out, "worker auth"), "worker auth     key required (ingest keys)");
    assert_eq!(line(&out, "write auth"), "write auth      key required (ingest keys)");
}

#[test]
fn a_key_variable_that_holds_no_key_refuses_the_start_and_names_the_variable() {
    // A Kubernetes Secret with an empty value sets the variable to "". The
    // blank entry was dropped, so the list read as unset and the read API was
    // open with only a warning in the log.
    for (flag, variable) in [
        ("--ingest-key", "MCPD_INGEST_KEYS"),
        ("--read-key", "MCPD_READ_KEYS"),
        ("--write-key", "MCPD_WRITE_KEYS"),
        ("--admin-key", "MCPD_ADMIN_KEYS"),
        ("--execute-key", "MCPD_EXECUTE_KEYS"),
        ("--worker-key", "MCPD_WORKER_KEYS"),
    ] {
        for value in ["", ",", " ", " , "] {
            let (status, said) = start_with(&["--runs-data", "off"], &[(variable, value)]);
            let status = status.unwrap_or_else(|| panic!("{variable}={value:?} started:\n{said}"));
            assert!(!status.success(), "{said}");
            assert!(said.contains("will not start"), "{said}");
            assert!(
                said.contains(&format!("{flag} (or {variable}) is set but holds no key")),
                "{variable}={value:?} is not named:\n{said}"
            );
            assert!(said.contains(&format!("remove {variable}")), "no recovery given:\n{said}");
        }
    }
    // The flag is the other way in, and is refused the same way.
    let (status, said) = start(&["--read-key", "", "--runs-data", "off"]);
    assert!(status.is_some_and(|s| !s.success()), "an empty --read-key started:\n{said}");
    assert!(said.contains("--read-key (or MCPD_READ_KEYS)"), "{said}");
}

#[test]
fn a_project_key_list_that_holds_no_key_refuses_the_start() {
    let dir = std::env::temp_dir().join(format!(
        "mcpd-blank-keys-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    ));
    std::fs::create_dir_all(&dir).expect("create the test directory");
    for scope in ["ingest", "read", "write", "admin", "execute", "worker"] {
        let path = dir.join(format!("{scope}.json"));
        std::fs::write(
            &path,
            format!(r#"{{ "projects": {{ "checkout": {{ "{scope}_keys": ["", "  "] }} }} }}"#),
        )
        .expect("write the config file");
        let (status, said) =
            start(&["--config-file", path.to_str().expect("a utf-8 path"), "--runs-data", "off"]);
        assert!(status.is_some_and(|s| !s.success()), "{scope} started:\n{said}");
        assert!(
            said.contains(&format!("projects.checkout.{scope}_keys in the config file is set")),
            "{said}"
        );
    }
}

#[test]
fn a_trailing_comma_beside_a_real_key_still_starts() {
    // The control for the two tests above: dropping a blank entry next to a
    // real key is still right, and a refusal there would break every list
    // written with a trailing comma.
    let (status, said) = start_with(
        &["--redact-key", "a0c55ad0159f86d081884c7d659a2fea", "--runs-data", "off"],
        &[("MCPD_INGEST_KEYS", "9f86d081884c7d659a2feaa0c55ad015,")],
    );
    assert!(status.is_none(), "a key with a trailing comma was refused:\n{said}");
}

#[test]
fn a_trailing_comma_leaves_the_real_key_and_nothing_else() {
    let out = config(&[], &[("MCPD_READ_KEYS", ""), ("MCPD_ADMIN_KEYS", "the-admin-key-0123456,")]);
    assert_eq!(line(&out, "read auth"), "read auth       open");
    assert_eq!(line(&out, "admin auth"), "admin auth      key required");
}

// ------------------------------------------------------- placeholder keys --

#[test]
fn a_placeholder_from_the_example_files_refuses_the_start_and_is_never_printed() {
    for (flag, placeholder) in [
        ("--ingest-key", "REPLACE_WITH_A_RANDOM_KEY"),
        ("--read-key", "replace-me-read"),
        ("--execute-key", "REPLACE_WITH_A_FIFTH_RANDOM_KEY"),
        ("--worker-key", "Replace-Me-Worker"),
    ] {
        let (status, said) = start(&[flag, placeholder, "--runs-data", "off"]);
        let status = status.unwrap_or_else(|| panic!("{flag} {placeholder} started:\n{said}"));
        assert!(!status.success(), "{said}");
        assert!(said.contains("will not start"), "{said}");
        assert!(said.contains(&format!("entry 1 of {flag}")), "the scope is not named:\n{said}");
        assert!(said.contains("openssl rand -hex 32"), "no way to make a key is given:\n{said}");
        assert!(!said.contains(placeholder), "the key itself was printed:\n{said}");
    }
}

#[test]
fn a_placeholder_under_a_project_refuses_the_start_too() {
    let dir = std::env::temp_dir().join(format!(
        "mcpd-keys-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    ));
    std::fs::create_dir_all(&dir).expect("create the test directory");
    let path = dir.join("projects.json");
    std::fs::write(
        &path,
        r#"{ "projects": { "checkout": { "worker_keys": ["REPLACE_WITH_A_SIXTH_RANDOM_KEY"] } } }"#,
    )
    .expect("write the config file");
    let (status, said) = start(&["--config-file", path.to_str().expect("a utf-8 path")]);
    assert!(status.is_some_and(|s| !s.success()), "it started:\n{said}");
    assert!(said.contains("projects.checkout.worker_keys"), "{said}");
    assert!(!said.contains("REPLACE_WITH_A_SIXTH_RANDOM_KEY"), "{said}");
}

#[test]
fn a_placeholder_hashing_key_refuses_the_start() {
    let (status, said) = start(&["--redact-key", "replace-me-redact", "--runs-data", "off"]);
    assert!(status.is_some_and(|s| !s.success()), "it started:\n{said}");
    assert!(said.contains("--redact-key is the placeholder"), "{said}");
    assert!(!said.contains("replace-me-redact"), "{said}");
}

#[test]
fn a_generated_key_starts() {
    // The control: without it, the tests above pass for a daemon that refuses
    // every key.
    let (status, said) = start(&[
        "--ingest-key",
        "9f86d081884c7d659a2feaa0c55ad015",
        "--redact-key",
        "a0c55ad0159f86d081884c7d659a2fea",
        "--runs-data",
        "off",
    ]);
    assert!(status.is_none(), "a generated key was refused:\n{said}");
}

// --------------------------------------------------- the run scopes' reach --

#[test]
fn an_ingest_key_that_also_starts_runs_is_warned_about_at_start() {
    let (status, said) = start(&[
        "--ingest-key",
        "9f86d081884c7d659a2feaa0c55ad015",
        "--read-key",
        "a0c55ad0159f86d081884c7d659a2fea",
    ]);
    assert!(status.is_none(), "it refused:\n{said}");
    assert!(
        said.contains("every ingest key can also start runs and claim steps"),
        "nothing said the fleet's key reaches the run surface:\n{said}"
    );
    assert!(said.contains("--execute-key and --worker-key"), "{said}");

    // With the two flags set there is nothing to say.
    let (_, said) = start(&[
        "--ingest-key",
        "9f86d081884c7d659a2feaa0c55ad015",
        "--read-key",
        "a0c55ad0159f86d081884c7d659a2fea",
        "--execute-key",
        "c7d659a2fea0c55ad0159f86d081884a",
        "--worker-key",
        "d0159f86d081884c7d659a2fea0c55ab",
    ]);
    assert!(!said.contains("claim steps"), "{said}");
}
