//! #128 gate: classify with `RUSTDL_HEAD_PRESENT_SKIP` OFF vs ON must produce
//! identical hierarchies on every fixture.
//!
//! The skip short-circuits a Horn clause's body match when its single head atom is
//! `Class(c, X)` and the node already carries `c`. That is an identity on fixpoint
//! semantics (`HyperNode::add` is keep-first), and the unit tests in
//! `owl-dl-tableau/tests/head_present_skip.rs` pin the mechanism plus its soundness
//! discriminator. This is the corpus-level gate: a difference here means the skip
//! dropped a derivation on real input.
//!
//! Mirrors `incremental_fixpoint_identity.rs` exactly — same banner-stripping
//! rationale (the `# wall breakdown ms:` line carries genuine wall-clock and is not
//! byte-stable), same gitignored-fixture skip policy, same anti-vacuity counter.
//!
//! NOTE ON A NON-IDENTITY THAT IS EXPECTED: the skip changes match-deadline PACING,
//! so under a tight `--pair-timeout-ms` the set of pairs that complete within budget
//! can differ, and with it the `# timed-out pairs:` counter. That is why this test
//! uses a generous per-pair budget and compares verdict lines only. It is an identity
//! on fixpoint semantics, NOT on run behaviour under a deadline.
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

fn fixture_path(rel_to_workspace_root: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel_to_workspace_root)
}

fn classify_verdict_lines(ofn: &Path, skip: bool) -> Vec<String> {
    let bin = env!("CARGO_BIN_EXE_rustdl");
    let mut c = Command::new(bin);
    c.arg("classify")
        .arg(ofn)
        .arg("--pair-timeout-ms")
        .arg("1000");
    c.env("RUSTDL_HEAD_PRESENT_SKIP", if skip { "1" } else { "0" });
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
fn head_present_skip_matches_baseline_on_fixtures() {
    let mut compared = 0usize;
    for rel in [
        // gitignored corpus fixtures — present only for a developer who fetched them
        "ontologies/real/pizza.ofn",
        "ontologies/regression/funcmerge-cyclic.ofn",
        // checked-in, always present
        "crates/owl-dl-bench/fixtures/27_eight_way_disjunction_sat.ofn",
        "crates/owl-dl-bench/fixtures/18_diamond_subsumption_unsat.ofn",
        "crates/owl-dl-cli/tests/fixtures/incremental/qualified-card-forall-union.ofn",
    ] {
        let path = fixture_path(rel);
        if !path.exists() {
            if rel.starts_with("ontologies/") {
                continue;
            }
            panic!("checked-in fixture missing: {rel} (resolved from {path:?})");
        }
        let off = classify_verdict_lines(&path, false);
        let on = classify_verdict_lines(&path, true);
        assert_eq!(off, on, "mismatch on {rel}");
        compared += 1;
    }
    assert!(
        compared >= 3,
        "vacuous gate: only {compared} fixture(s) compared — the checked-in ones \
         must always run"
    );
}
