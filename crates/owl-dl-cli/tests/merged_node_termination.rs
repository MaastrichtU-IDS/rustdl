//! #284: `sat` / `subclass` did not terminate on a 5-axiom ontology.
//!
//! `X ⊑ ∃R.T`, `T ⊑ ∃S.B`, `R⁻ ⊑ F`, `S ⊑ F`, `F` functional. At the T node the
//! `≤1 F` merge folds its two F-neighbours (X, via `R⁻`, and B, via `S`) into one.
//! It merged X into B, and the saturator kept firing rules on the merged-away X:
//! its `∃R.T` looked unsatisfied once its edges had moved, so it rebuilt the
//! T→B chain, which was merged again, forever. Merged-away nodes are now skipped.
//!
//! Each query runs under a deadline so a regression fails rather than hangs.

#![allow(clippy::unwrap_used)]

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const DEADLINE: Duration = Duration::from_secs(30);

fn fixture() -> String {
    format!(
        "{}/tests/fixtures/json/prove_inverse_functional_merge.ofn",
        env!("CARGO_MANIFEST_DIR")
    )
}

/// Run `rustdl <args…>` and return its last stdout line, or panic past the deadline.
fn answer(args: &[&str]) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rustdl"))
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let start = Instant::now();
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if start.elapsed() > DEADLINE {
            child.kill().ok();
            child.wait().ok();
            panic!("rustdl {args:?} did not finish within {DEADLINE:?}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "rustdl {args:?} exited {}",
        out.status
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    stdout.lines().last().unwrap_or_default().trim().to_owned()
}

#[test]
fn x_is_satisfiable() {
    assert_eq!(answer(&["sat", &fixture(), "http://t/X"]), "sat");
}

#[test]
fn x_is_not_subsumed_by_t() {
    assert_eq!(
        answer(&["subclass", &fixture(), "http://t/X", "http://t/T"]),
        "no"
    );
}

/// Control: the merge's real consequence still holds.
#[test]
fn x_is_subsumed_by_b() {
    assert_eq!(
        answer(&["subclass", &fixture(), "http://t/X", "http://t/B"]),
        "yes"
    );
}
