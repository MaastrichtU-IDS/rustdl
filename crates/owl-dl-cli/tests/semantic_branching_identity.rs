//! Fix#2 Layer A gate: classify with `RUSTDL_SEMANTIC_BRANCHING` OFF vs ON must
//! produce identical hierarchies on every fixture. Layer A (in-search
//! disjointness pruning + unit-forcing at the `⊔` decision) is
//! **verdict-preserving** — it only drops disjuncts the reactive `horn_fixpoint`
//! would clash on anyway and forces a survivor the search was compelled to
//! take. Any difference here is a real Layer-A bug (a MISS or, worse, an FP via
//! an unsound backjump on a subset dep-set).
//!
//! Mirrors `incremental_fixpoint_identity.rs` exactly, toggling
//! `RUSTDL_SEMANTIC_BRANCHING` instead of the incremental var.
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

/// `cargo test` runs integration tests with cwd == the crate manifest dir
/// (`crates/owl-dl-cli`), NOT the workspace root. Resolve every fixture off
/// `CARGO_MANIFEST_DIR`.
fn fixture_path(rel_to_workspace_root: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel_to_workspace_root)
}

/// Runs `classify` and returns only the hierarchy-verdict lines (`unsat\t...`,
/// `equiv\t...`, `direct\t...`), with `#`-prefixed diagnostic/stats banner
/// lines stripped (they carry non-deterministic wall-clock timing) and the
/// remainder sorted. See `incremental_fixpoint_identity.rs` for the rationale.
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
    // Same fixtures as the incremental-fixpoint identity gate: a
    // functional/`≤1`-merge fixture plus three exercising disjunctive branching
    // and `≤n` merging — the paths where Layer A's `⊔`-decision hook is active.
    let mut ran = 0;
    for rel in [
        "ontologies/regression/funcmerge-cyclic.ofn",
        "ontologies/real/pizza.ofn",
        "crates/owl-dl-bench/fixtures/27_eight_way_disjunction_sat.ofn",
        "crates/owl-dl-bench/fixtures/18_diamond_subsumption_unsat.ofn",
    ] {
        let path = fixture_path(rel);
        // The regression fixture (`funcmerge-cyclic.ofn`) is gitignored and not
        // present on every machine. Skip a missing fixture loudly rather than
        // fail — the three disjunctive/checked-in fixtures cover the `⊔`-decision
        // path Layer A touches; a missing one is an environment gap, not a bug.
        if !path.exists() {
            eprintln!("SKIP: fixture not present locally: {rel}");
            continue;
        }

        let off = classify_verdict_lines(&path, false);
        let on = classify_verdict_lines(&path, true);
        assert_eq!(off, on, "mismatch on {rel}");
        ran += 1;
    }
    // Non-vacuity: at least one fixture must have actually run.
    assert!(ran >= 1, "no identity fixtures were present to compare");
}
