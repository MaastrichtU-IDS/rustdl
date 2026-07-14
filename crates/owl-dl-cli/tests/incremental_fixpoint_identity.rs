//! SP1 gate: classify with `incremental_fixpoint` OFF vs ON must produce
//! identical hierarchies on every fixture. A difference means the
//! incremental drain dropped or double-fired a clause (a MISS or FP).
//!
//! `RUSTDL_HYPER_INCREMENTAL_FIXPOINT` is inert as of Task 1.2 (SP1's
//! later tasks make it load-bearing), so this test currently passes
//! trivially — it exists to catch a regression the moment the flag
//! starts doing real work.
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
// The fixture list is a single entry today (SP1 Task 1.2); later SP1 tasks
// grow it, at which point this stops being a single-element loop. Keep the
// loop shape rather than de-looping and re-looping later.
#[allow(clippy::single_element_loop)]
fn incremental_matches_baseline_on_fixtures() {
    // Small, checked-in SROIQ fixtures that exercise disjunction + <=n.
    for rel in ["ontologies/regression/funcmerge-cyclic.ofn"] {
        let path = fixture_path(rel);
        // A missing fixture must fail loudly, not be silently skipped: a
        // vacuous pass here would defeat the whole point of this gate.
        assert!(
            path.exists(),
            "fixture missing: {rel} (resolved from {path:?})"
        );

        let off = classify_verdict_lines(&path, false);
        let on = classify_verdict_lines(&path, true);
        assert_eq!(off, on, "mismatch on {rel}");
    }
}
