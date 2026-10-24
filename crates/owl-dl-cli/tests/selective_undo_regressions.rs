//! #288 step 4d: two selective-undo defects the ALCHIQ+O fuzz found, each
//! reduced to five axioms. Both runs have the selective-undo oracle on
//! (`RUSTDL_SELECTIVE_CHECK=1`), so a regression panics instead of quietly
//! leaving a damaged completion graph.
//!
//! - `duplicate_edge_mirror.ofn` (fuzz case p0305): a merge found an edge's
//!   mirror by `(role, node)` instead of by edge id. With two instances of one
//!   edge, which a selective backjump can leave, it removed the other
//!   instance's mirror, and a later keyed undo put different deps on the two
//!   sides. `HermiT`: `Q` satisfiable.
//! - `restored_edge_on_merged_source.ofn` (fuzz case o0138): an undone merge
//!   had moved an edge off a node that a later, kept merge then redirected.
//!   Undoing the first merge put the edge back on the redirected node, so a
//!   child was left with no edge from its parent. A kept merge now replays its
//!   edge move after the undo. `HermiT`: the ontology is inconsistent, so `Q` is
//!   unsatisfiable.
//!
//! Node cap off and a deadline, as in `merged_node_termination.rs`.

#![allow(clippy::unwrap_used)]

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Generous: the debug test profile runs these well over ten times slower
/// than release (about 1 s each there).
const DEADLINE: Duration = Duration::from_secs(120);

fn fixture(name: &str) -> String {
    format!(
        "{}/tests/fixtures/selective/{name}",
        env!("CARGO_MANIFEST_DIR")
    )
}

/// `rustdl sat FILE Q` with selective undo and its oracle on.
fn sat_q(name: &str) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rustdl"))
        .args(["sat", &fixture(name), "http://x.org/f#Q"])
        .env("RUSTDL_SELECTIVE_UNDO", "1")
        .env("RUSTDL_SELECTIVE_CHECK", "1")
        .env("RUSTDL_MAX_NODES", "0")
        .env("RAYON_NUM_THREADS", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let start = Instant::now();
    while child.try_wait().unwrap().is_none() {
        if start.elapsed() > DEADLINE {
            child.kill().ok();
            child.wait().ok();
            panic!("rustdl sat {name} did not finish within {DEADLINE:?}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "rustdl sat {name} exited {}: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    stdout.lines().last().unwrap_or_default().trim().to_owned()
}

#[test]
fn a_duplicate_edge_keeps_its_own_mirror() {
    assert_eq!(sat_q("duplicate_edge_mirror.ofn"), "sat");
}

#[test]
fn a_kept_merge_moves_an_edge_the_undo_restores() {
    assert_eq!(sat_q("restored_edge_on_merged_source.ofn"), "unsat");
}
