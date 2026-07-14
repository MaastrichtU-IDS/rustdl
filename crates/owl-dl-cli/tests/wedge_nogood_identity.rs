//! SP2 B3 gate: classify with `RUSTDL_WEDGE_NOGOOD` OFF vs ON must produce
//! identical hierarchies on every fixture. The no-good record+prune is a
//! SOUND early cut (a disjunction branch whose node label-set is a superset
//! of a recorded node-local UNSAT core is itself UNSAT), so on inputs that
//! fully resolve it can only ever change *how fast* a branch is refuted,
//! never the verdict. A difference here therefore means the prune dropped a
//! real subsumption — an over-prune (unsound/over-broad core or a bad
//! recomputed dep-set). This test is the durable byte-identity gate on that
//! path.
//!
//! Mirrors `incremental_fixpoint_identity.rs` in structure (spawn the bin,
//! classify each fixture, compare SORTED verdict lines with `#`-prefixed
//! banner lines stripped).
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

/// `cargo test` runs integration tests with cwd == the crate manifest dir
/// (`crates/owl-dl-cli`), NOT the workspace root, so resolve every fixture
/// off `CARGO_MANIFEST_DIR`.
fn fixture_path(rel_to_workspace_root: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel_to_workspace_root)
}

/// Runs `classify` and returns only the hierarchy-verdict lines, with
/// `#`-prefixed diagnostic/stats banner lines (which carry non-byte-stable
/// wall-clock timing) stripped and the remainder sorted. Sorting can only
/// ever mask *order*, never a dropped/extra line, so it cannot hide a real
/// verdict mismatch.
fn classify_verdict_lines(ofn: &Path, nogood: bool) -> Vec<String> {
    let bin = env!("CARGO_BIN_EXE_rustdl");
    let mut c = Command::new(bin);
    c.arg("classify")
        .arg(ofn)
        .arg("--pair-timeout-ms")
        .arg("1000");
    c.env("RUSTDL_WEDGE_NOGOOD", if nogood { "1" } else { "0" });
    let out = c.output().expect("run rustdl");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut lines: Vec<String> = stdout
        .lines()
        .filter(|l| !l.starts_with('#'))
        .map(str::to_owned)
        .collect();
    lines.sort_unstable();
    lines
}

#[test]
fn nogood_matches_baseline_on_fixtures() {
    // `funcmerge-cyclic` exercises functional/`≤1` merge; the last three
    // exercise disjunctive branching (the path the prune actually fires on)
    // and `≤n` cardinality merging. All fully resolve at the 1s per-pair
    // budget, so a sound prune must leave the hierarchy byte-identical.
    for rel in [
        "ontologies/regression/funcmerge-cyclic.ofn",
        "ontologies/real/pizza.ofn",
        "crates/owl-dl-bench/fixtures/27_eight_way_disjunction_sat.ofn",
        "crates/owl-dl-bench/fixtures/18_diamond_subsumption_unsat.ofn",
    ] {
        let path = fixture_path(rel);
        // A missing fixture must fail loudly, not be silently skipped.
        assert!(
            path.exists(),
            "fixture missing: {rel} (resolved from {path:?})"
        );

        let off = classify_verdict_lines(&path, false);
        let on = classify_verdict_lines(&path, true);
        assert_eq!(off, on, "mismatch on {rel}");
    }
}
