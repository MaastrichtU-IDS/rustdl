//! SP1 gate: classify with `incremental_fixpoint` OFF vs ON must produce
//! identical hierarchies on every fixture. A difference means the
//! incremental drain dropped or double-fired a clause (a MISS or FP).
//!
//! `RUSTDL_HYPER_INCREMENTAL_FIXPOINT` is load-bearing as of Task 1.4:
//! ON, `horn_fixpoint` no longer re-seeds the whole graph every pass but
//! drains only the per-branch delta carried across `save`/`restore`. This
//! test is the durable byte-identity gate on that path — a mismatch here is
//! a real incremental-drain bug, not diagnostic noise.
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

/// `cargo test` runs integration tests with cwd == the crate manifest dir
/// (`crates/owl-dl-cli`), NOT the workspace root. A workspace-root-relative
/// fixture path (e.g. `ontologies/regression/...`) is therefore never found
/// from here — resolve every fixture off `CARGO_MANIFEST_DIR` instead.
fn fixture_path(rel_to_workspace_root: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel_to_workspace_root)
}

/// Runs `classify` and returns only the hierarchy-verdict lines (`unsat\t...`,
/// `equiv\t...`, `direct\t...`), with `#`-prefixed diagnostic/stats banner
/// lines stripped and the remainder sorted.
///
/// Two things were verified empirically before settling on this shape:
/// - The full raw stdout (banner included) is NOT byte-stable run-to-run:
///   `# wall breakdown ms: ... tier_walk=N` carries genuine wall-clock
///   timing, so two back-to-back runs on the identical input can legitimately
///   differ (observed `tier_walk=1` vs `tier_walk=2` in this harness). That
///   is diagnostic noise, not a verdict difference, so the banner is
///   excluded rather than compared.
/// - The verdict lines themselves ARE order-stable (checked 8 repeated runs
///   at both flag settings, plus `RAYON_NUM_THREADS=1` vs 8 on the full
///   output): `classes`/`direct_subsumers` are read from already-materialized
///   `Classification` structures after the parallel pairwise loop completes,
///   not printed from inside it. The sort below is kept anyway as a cheap
///   defensive belt-and-suspenders for any fixture added later with multiple
///   direct/equiv lines: sorting can only ever mask *order*, never a
///   dropped/extra line, so it cannot hide a real mismatch.
fn classify_verdict_lines(ofn: &Path, incremental: bool) -> Vec<String> {
    let bin = env!("CARGO_BIN_EXE_rustdl");
    let mut c = Command::new(bin);
    c.arg("classify")
        .arg(ofn)
        .arg("--pair-timeout-ms")
        .arg("1000");
    c.env(
        "RUSTDL_HYPER_INCREMENTAL_FIXPOINT",
        if incremental { "1" } else { "0" },
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
fn incremental_matches_baseline_on_fixtures() {
    // Small, checked-in SROIQ fixtures. The first exercises functional/`≤1`
    // merge; the last three (added SP1 Task 1.4) exercise disjunctive
    // branching and `≤n` cardinality merging across `save`/`restore` — the
    // path where the incremental worklist drain is actually stressed. Each
    // was verified deterministic under `classify` (verdict lines identical
    // over repeated runs) before inclusion.
    // The first two live under `/ontologies`, which is `.gitignore`d and fetched
    // on demand — so they are ABSENT in a fresh CI checkout and present only for a
    // developer who has pulled the corpus. The last two are checked-in bench
    // fixtures, always present. A fixture under `/ontologies` is therefore skipped
    // when absent (not a failure); the checked-in ones must always exist. The
    // `compared` guard below still forbids a vacuous pass (author's original
    // intent) — if nothing ran, the test fails loudly.
    let mut compared = 0usize;
    for rel in [
        "ontologies/regression/funcmerge-cyclic.ofn",
        "ontologies/real/pizza.ofn",
        "crates/owl-dl-bench/fixtures/27_eight_way_disjunction_sat.ofn",
        "crates/owl-dl-bench/fixtures/18_diamond_subsumption_unsat.ofn",
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
        assert_eq!(off, on, "mismatch on {rel}");
        compared += 1;
    }
    // Never let the gate pass vacuously: at least the checked-in fixtures must
    // have been compared.
    assert!(
        compared >= 2,
        "expected ≥2 fixtures compared (the checked-in bench fixtures); got {compared} \
         — the incremental-fixpoint identity gate would be vacuous"
    );
}
