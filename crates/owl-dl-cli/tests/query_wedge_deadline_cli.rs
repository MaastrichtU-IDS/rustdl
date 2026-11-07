//! #457: `subclass` and `explain` did not finish on an 8-axiom inconsistent
//! `TBox` (`tests/fixtures/query_wedge_hang_457.ofn`). The named subsumption
//! query ran its H4 hypertableau wedge with no deadline, and the wedge searched
//! without end; the main tableau answers the pair in milliseconds. The wedge now
//! runs under `RUSTDL_QUERY_WEDGE_MS` (default 3000) and falls through.
//!
//! Each query runs under a deadline, so a regression fails rather than hangs.
//! The KB is inconsistent and has no individual. Since #457's second residual
//! (`RUSTDL_QUERY_NO_SEED_TAIL`), the tableau-answered query runs the bounded
//! main-tableau consistency check and errors `Inconsistent` (exit 1); before, it
//! printed the vacuous `yes` with no signal.

#![allow(clippy::unwrap_used)]

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const DEADLINE: Duration = Duration::from_secs(60);
const C0: &str = "http://gen.rustdl/o#C0";
const C1: &str = "http://gen.rustdl/o#C1";

fn fixture() -> String {
    format!(
        "{}/tests/fixtures/query_wedge_hang_457.ofn",
        env!("CARGO_MANIFEST_DIR")
    )
}

/// Run `rustdl <args…>` with no inherited `RUSTDL_*` setting plus `env`, and
/// return (exit code, stdout, stderr), or panic past the deadline.
fn run(env: &[(&str, &str)], args: &[&str]) -> (i32, String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rustdl"));
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("RUSTDL_") {
            cmd.env_remove(key);
        }
    }
    let mut child = cmd
        .envs(env.iter().copied())
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let start = Instant::now();
    while child.try_wait().unwrap().is_none() {
        if start.elapsed() > DEADLINE {
            child.kill().ok();
            child.wait().ok();
            panic!("rustdl {args:?} (env {env:?}) did not finish within {DEADLINE:?}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let out = child.wait_with_output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
    )
}

/// The query finished, and found the KB inconsistent (#457 residual 2).
fn assert_inconsistent((code, stdout, stderr): (i32, String, String)) {
    assert_eq!(code, 1, "stdout {stdout:?}, stderr {stderr}");
    assert!(stderr.contains("ontology is inconsistent"), "{stderr}");
}

#[test]
fn subclass_finishes_on_the_wedge_hang_reproducer() {
    let f = fixture();
    assert_inconsistent(run(&[], &["subclass", &f, C0, C1]));
}

#[test]
fn explain_finishes_on_the_wedge_hang_reproducer() {
    let f = fixture();
    assert_inconsistent(run(&[], &["explain", &f, C0, C1]));
}

/// A short budget ends the same way: a stall falls through to the main tableau.
#[test]
fn a_tiny_wedge_budget_still_answers() {
    let f = fixture();
    assert_inconsistent(run(
        &[("RUSTDL_QUERY_WEDGE_MS", "1")],
        &["subclass", &f, C0, C1],
    ));
}

/// The budget must leave the wedge time to prove what it proves (Fable review of
/// #467: an already-expired deadline passed every other test, since the main
/// tableau then gives the same answer). On a pair outside the EL closure, the
/// `RUSTDL_TRACE` line must report the wedge's `Subsumed`, not a stall.
#[test]
fn the_wedge_still_proves_under_the_default_budget() {
    let f = format!(
        "{}/tests/fixtures/query_wedge_proves_457.ofn",
        env!("CARGO_MANIFEST_DIR")
    );
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rustdl"));
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("RUSTDL_") {
            cmd.env_remove(key);
        }
    }
    let out = cmd
        .env("RUSTDL_TRACE", "1")
        .args([
            "subclass",
            &f,
            "http://example.org/w457#A",
            "http://example.org/w457#D",
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert_eq!(stdout.trim(), "yes", "{stderr}");
    let wedge: Vec<&str> = stderr
        .lines()
        .filter(|l| l.starts_with("query wedge (#457)"))
        .collect();
    assert_eq!(wedge.len(), 1, "one wedge call expected: {stderr}");
    assert!(
        wedge[0].starts_with("query wedge (#457): Subsumed after"),
        "{}",
        wedge[0]
    );
}
