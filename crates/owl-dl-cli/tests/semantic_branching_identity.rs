//! Fix#2 Layer A gate: classify with `RUSTDL_SEMANTIC_BRANCHING` OFF vs ON
//! must produce byte-identical hierarchies on every fixture. Layer A
//! (in-search disjoint-pruning + unit-forcing at the `⊔` decision) is
//! verdict-preserving by construction — it only drops disjuncts that would
//! clash on the next `horn_fixpoint` pass and only forces a disjunct the
//! search was obliged to take. A difference here is a real soundness bug
//! (a wrong dep-set → unsound backjump → FP, or a dropped survivor → MISS).
//!
//! Mirrors `incremental_fixpoint_identity.rs`. `RUSTDL_SAT_LOOKAHEAD` is
//! pinned OFF in both runs: lookahead-dropped disjuncts carry no
//! reason-deps, a known incompatibility with Layer A's dep-precision — the
//! two flags are kept separate.
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

/// `cargo test` runs integration tests with cwd == the crate manifest dir
/// (`crates/owl-dl-cli`), NOT the workspace root — resolve every fixture off
/// `CARGO_MANIFEST_DIR`.
fn fixture_path(rel_to_workspace_root: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel_to_workspace_root)
}

/// Runs `classify` and returns only the hierarchy-verdict lines (`unsat\t...`,
/// `equiv\t...`, `direct\t...`), with `#`-prefixed diagnostic/stats banner
/// lines (which carry non-deterministic wall-clock timing) stripped and the
/// remainder sorted (defensive: sorting can mask order, never a
/// dropped/extra line, so it cannot hide a real mismatch).
fn classify_verdict_lines(ofn: &Path, semantic: bool) -> Vec<String> {
    let bin = env!("CARGO_BIN_EXE_rustdl");
    let mut c = Command::new(bin);
    c.arg("classify")
        .arg(ofn)
        .arg("--pair-timeout-ms")
        .arg("1000");
    c.env(
        "RUSTDL_SEMANTIC_BRANCHING",
        if semantic { "1" } else { "0" },
    );
    // Keep the two flags separate (see module docs).
    c.env("RUSTDL_SAT_LOOKAHEAD", "0");
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
fn semantic_branching_matches_baseline_on_fixtures() {
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
