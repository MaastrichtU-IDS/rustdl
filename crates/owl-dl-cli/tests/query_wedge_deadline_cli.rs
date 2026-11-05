//! #457: `subclass` and `explain` did not finish on an 8-axiom inconsistent
//! `TBox` (`tests/fixtures/query_wedge_hang_457.ofn`). The named subsumption
//! query ran its H4 hypertableau wedge with no deadline, and the wedge searched
//! without end; the main tableau answers the pair in milliseconds. The wedge now
//! runs under `RUSTDL_QUERY_WEDGE_MS` (default 3000) and falls through.
//!
//! Each query runs under a deadline, so a regression fails rather than hangs.
//! The answer is the vacuous `yes` of an inconsistent KB: the #428 guard does not
//! see this inconsistency (no `ABox`), which is #457's second residual, tracked
//! apart. If that lands, `Inconsistent` (exit 1) is the expected answer instead.

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
/// return (exit code, stdout), or panic past the deadline.
fn run(env: &[(&str, &str)], args: &[&str]) -> (i32, String) {
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
        .stderr(Stdio::null())
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
    )
}

#[test]
fn subclass_finishes_on_the_wedge_hang_reproducer() {
    let f = fixture();
    let (code, stdout) = run(&[], &["subclass", &f, C0, C1]);
    assert_eq!(code, 0, "{stdout}");
    assert_eq!(stdout.trim(), "yes");
}

#[test]
fn explain_finishes_on_the_wedge_hang_reproducer() {
    let f = fixture();
    let (code, stdout) = run(&[], &["explain", &f, C0, C1]);
    assert_eq!(code, 0, "{stdout}");
    assert!(stdout.contains("yes"), "{stdout}");
}

/// A short budget answers the same: a stall falls through to the main tableau.
#[test]
fn a_tiny_wedge_budget_still_answers() {
    let f = fixture();
    let (code, stdout) = run(&[("RUSTDL_QUERY_WEDGE_MS", "1")], &["subclass", &f, C0, C1]);
    assert_eq!(code, 0, "{stdout}");
    assert_eq!(stdout.trim(), "yes");
}
