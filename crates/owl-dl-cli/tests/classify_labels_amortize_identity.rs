//! `RUSTDL_CLASSIFY_LABELS_AMORTIZE` gate: classify with the per-CLASS
//! clause-index amortization OFF vs ON must produce identical hierarchies.
//!
//! The flag replaces the per-class full `ClauseIndexes` rebuild in
//! `HyperCache::classify_labels` with the O(#extras) sparse
//! `build_clause_index_delta` already used by the per-PAIR sibling
//! `decide_with_stats` (v0.3.39). A difference here means the delta dropped,
//! mis-numbered, or double-fired a per-class seed clause.
//!
//! `--pair-timeout-ms 1000` is deliberate and load-bearing. At a TRUNCATING
//! per-pair budget the hierarchy is not run-to-run deterministic on hard
//! ontologies: measured on `ore_ont_1508` at `--pair-timeout-ms 20`, the
//! timed-out-pair count varied 57–68 across five runs of the SAME binary and
//! two runs differed by four `direct` rows — noise from racing a wall-clock
//! deadline, reproduced WITHIN one flag setting. At a budget generous enough
//! that no pair times out, the arms are byte-identical. So this gate must run
//! at a non-truncating budget or it will flake.
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

/// `cargo test` runs integration tests with cwd == the crate manifest dir
/// (`crates/owl-dl-cli`), not the workspace root — resolve fixtures off
/// `CARGO_MANIFEST_DIR`.
fn fixture_path(rel_to_workspace_root: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel_to_workspace_root)
}

/// Hierarchy verdict lines only (`unsat`/`equiv`/`direct`), sorted, with the
/// `#`-prefixed banner stripped. The banner carries genuine wall-clock timing
/// (`# wall breakdown ms: … tier_walk=N`) and a nondeterministic
/// `# wedge-cost-histogram`, so it is excluded rather than compared.
fn classify_verdict_lines(ofn: &Path, amortize: bool) -> Vec<String> {
    let bin = env!("CARGO_BIN_EXE_rustdl");
    let mut c = Command::new(bin);
    c.arg("classify")
        .arg(ofn)
        .arg("--pair-timeout-ms")
        .arg("1000");
    if amortize {
        c.env("RUSTDL_CLASSIFY_LABELS_AMORTIZE", "1");
    } else {
        c.env_remove("RUSTDL_CLASSIFY_LABELS_AMORTIZE");
    }
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
fn amortized_label_cache_matches_full_rebuild_on_fixtures() {
    // Checked-in bench fixtures (always present) plus two gitignored corpus
    // fixtures that are compared when the corpus has been fetched. The
    // disjunctive / cardinality fixtures are the ones that actually build a
    // label cache through the wedge, which is the path the flag changes.
    let mut compared = 0usize;
    for rel in [
        "crates/owl-dl-bench/fixtures/27_eight_way_disjunction_sat.ofn",
        "crates/owl-dl-bench/fixtures/18_diamond_subsumption_unsat.ofn",
        "ontologies/real/pizza.ofn",
        "ontologies/real/ro.ofn",
    ] {
        let path = fixture_path(rel);
        if !path.exists() {
            if rel.starts_with("ontologies/") {
                // gitignored corpus fixture, not fetched — skip, don't fail.
                continue;
            }
            panic!("checked-in fixture missing: {rel} (resolved from {path:?})");
        }
        let off = classify_verdict_lines(&path, false);
        let on = classify_verdict_lines(&path, true);
        assert_eq!(off, on, "hierarchy mismatch on {rel}");
        compared += 1;
    }
    assert!(
        compared >= 2,
        "expected ≥2 fixtures compared (the checked-in bench fixtures); got {compared} \
         — this identity gate would be vacuous"
    );
}
