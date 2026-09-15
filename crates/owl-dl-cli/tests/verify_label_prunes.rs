//! #160 — `RUSTDL_CLASSIFY_VERIFY_REFUTATIONS` must actually reach the label-heuristic
//! PRUNES, not just the pairs that survive them.
//!
//! The Phase 7 label heuristic decides `sup ∉ labels ⟹ not subsumed` and returns `false`
//! outright. Issue #66's remedy — withdraw trust in a wedge `Sat` when the ontology is out
//! of fragment — lives inside `subsumes_via_tableau`, which a pruned pair never reaches.
//! So the flag was INERT for exactly the pairs that needed it: on `ore_ont_3258` it changed
//! nothing alone, and only `RUSTDL_LABEL_HEURISTIC=0` *plus* the flag recovered all 200
//! missed subsumptions. With the prune gated, the flag alone recovers them.
//!
//! # What this test pins, and what it does not
//!
//! It pins the MECHANISM: with the flag on, prunes become verifications. It does NOT
//! demonstrate recovery of a missed subsumption, because the corpus ontology that does so
//! (`ore_ont_3258`, 324 classes) is not checked in, and the small checked-in fixture below
//! loses `X ⊑ W` through a DIFFERENT path — every label-cache counter reads zero there, so
//! it is not this bug. That second gap is tracked separately on #160; do not "fix" this test
//! by making it assert `X ⊑ W`.
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/classify160/domain-existential.ofn")
}

/// Returns (pruned, prunes_verified) off the `# label heuristic:` banner line.
fn prune_counters(verify: bool) -> (u64, u64) {
    let out = Command::new(env!("CARGO_BIN_EXE_rustdl"))
        .arg("classify")
        .arg(fixture())
        .arg("--pair-timeout-ms")
        .arg("5000")
        .env(
            "RUSTDL_CLASSIFY_VERIFY_REFUTATIONS",
            if verify { "1" } else { "0" },
        )
        .output()
        .expect("run rustdl");
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text
        .lines()
        .find(|l| l.starts_with("# label heuristic:"))
        .expect("banner must carry the label-heuristic line");
    let field = |k: &str| -> u64 {
        line.split_whitespace()
            .find_map(|t| t.strip_prefix(k))
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(|| panic!("no `{k}` field in: {line}"))
    };
    (field("pruned="), field("prunes_verified="))
}

#[test]
fn the_flag_off_path_still_prunes() {
    // CONTROL + non-vacuity denominator. If this fixture stopped producing prunes the
    // test below would pass trivially on a build where the gate was deleted.
    let (pruned, verified) = prune_counters(false);
    assert!(
        pruned > 0,
        "fixture must exercise the label-heuristic prune; got pruned={pruned}"
    );
    assert_eq!(
        verified, 0,
        "no prune may be verified with the flag off — that is the default fast path"
    );
}

#[test]
fn the_flag_converts_prunes_into_verifications() {
    // THE FIX. Same fixture, flag on: every prune is verified instead of trusted.
    let (pruned, verified) = prune_counters(true);
    assert!(
        verified > 0,
        "RUSTDL_CLASSIFY_VERIFY_REFUTATIONS=1 must verify label-heuristic prunes on an \
         out-of-fragment ontology; got prunes_verified={verified}. A zero here means the \
         gate never fired and the flag is inert again — the #160 regression"
    );
    assert_eq!(
        pruned, 0,
        "with the flag on, prunes should have become verifications, not remained prunes"
    );
}
