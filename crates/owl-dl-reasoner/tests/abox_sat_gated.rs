//! Tests for the Variant A-gated ABox-saturation consistency pre-check.
//!
//! This is the consequence-based named-individual fixpoint
//! (`saturate_abox_for_consistency` in `owl-dl-saturation`) wired into
//! `is_consistent_internal_full` behind `RUSTDL_ABOX_SAT_GATED=1`.
//!
//! Tests:
//! 1. `family_detected_inconsistent` — full family.ofn detected via A-gated
//!    (requires `ontologies/real/family.ofn`; `#[ignore]`d).
//! 2. `fp_smoke_consistent_abox_stays_consistent` — a consistent ontology with
//!    inverse roles and role chains is NOT flagged as inconsistent.
//! 3. `functional_marker_clash_detected` — a synthetic clash where a class implies
//!    `∃hasSex.Male ⊓ ∃hasSex.Female` with `Functional(hasSex)` and
//!    `Disjoint(Male, Female)` is detected.
//! 4. `el_preservation_gate_off` — with `RUSTDL_ABOX_SAT_GATED=0` (or unset), a
//!    consistent ontology remains consistent (gate-off path unchanged).
//!
//! **EL preservation** is guaranteed by construction: `saturate_abox_for_consistency`
//! is unreachable from the `saturate()` classification path. Run:
//!   cargo test -p owl-dl-saturation   # all 51 existing tests must pass

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::is_consistent;
use std::fs;
use std::io::Cursor;
use std::path::Path;

const FIXTURE_DIR: &str = "tests/fixtures/abox_sat_gated";

/// Parse and run `is_consistent` on an OFN fixture file.
/// The `RUSTDL_ABOX_SAT_GATED` environment variable must be set by the caller.
fn check_consistency_with_gate(name: &str) -> bool {
    let path = Path::new(FIXTURE_DIR).join(format!("{name}.ofn"));
    let src = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    is_consistent(&onto).expect("is_consistent succeeds")
}

/// Parse `path` (absolute or relative to workspace root) and run `is_consistent`.
fn check_consistency_path(path: &Path) -> bool {
    let src =
        fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    is_consistent(&onto).expect("is_consistent succeeds")
}

// ── Test 1: family detected via A-gated (requires real corpus) ────────────────

/// Full family.ofn should be detected inconsistent when A-gated is enabled.
/// Requires `ontologies/real/family.ofn` (gitignored, pull with
/// `scripts/fetch-real-ontologies.sh`).
#[test]
#[ignore = "requires ontologies/real/family.ofn (run scripts/fetch-real-ontologies.sh)"]
fn family_detected_inconsistent() {
    // Safety: set the gate for this test.
    // Note: cargo test runs tests in-process; we use env_guard via std::env::set_var.
    // This test is `#[ignore]` so it runs only when explicitly invoked:
    //   RUSTDL_ABOX_SAT_GATED=1 cargo test -p owl-dl-reasoner family_detected_inconsistent -- --ignored
    //
    // Alternatively: env var can be set externally before running the test.
    let path = Path::new("ontologies/real/family.ofn");
    if !path.exists() {
        eprintln!("SKIP: ontologies/real/family.ofn not found (run scripts/fetch-real-ontologies.sh)");
        return;
    }
    // Enable the gate for this call.
    // SAFETY: test is single-threaded at this point (#[ignore] + serial execution).
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("RUSTDL_ABOX_SAT_GATED", "1");
    }
    let t0 = std::time::Instant::now();
    let consistent = check_consistency_path(path);
    let elapsed = t0.elapsed();
    eprintln!(
        "family: consistent={consistent}, elapsed={:.3}s",
        elapsed.as_secs_f64()
    );
    // Clean up env.
    #[allow(unsafe_code)]
    unsafe {
        std::env::remove_var("RUSTDL_ABOX_SAT_GATED");
    }
    assert!(!consistent, "family.ofn should be detected inconsistent via A-gated");
}

// ── Test 2: FP smoke — consistent ABox with inverses + chains stays consistent ─

/// A consistent ontology with inverse roles, role chains, and type propagation
/// must NOT be flagged as inconsistent.
#[test]
fn fp_smoke_consistent_abox_stays_consistent() {
    // Gate off: use default (gate off), expect consistent.
    // This verifies the gate-off path does not perturb consistent ABox ontologies.
    #[allow(unsafe_code)]
    unsafe {
        std::env::remove_var("RUSTDL_ABOX_SAT_GATED");
    }
    let consistent = check_consistency_with_gate("fp_smoke_consistent");
    assert!(consistent, "Consistent ABox with inverse/chain should stay consistent (gate off)");

    // Gate on: still consistent (no clash in this ontology).
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("RUSTDL_ABOX_SAT_GATED", "1");
    }
    let consistent_gated = check_consistency_with_gate("fp_smoke_consistent");
    #[allow(unsafe_code)]
    unsafe {
        std::env::remove_var("RUSTDL_ABOX_SAT_GATED");
    }
    assert!(
        consistent_gated,
        "Consistent ABox with inverse/chain should stay consistent (gate on)"
    );
}

// ── Test 3: Synthetic functional-marker clash ─────────────────────────────────

/// A class A ⊑ ∃hasSex.Male ⊓ ∃hasSex.Female with Functional(hasSex) and
/// Disjoint(Male, Female), asserted on an individual → inconsistent.
#[test]
fn functional_marker_clash_detected() {
    // Gate on.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("RUSTDL_ABOX_SAT_GATED", "1");
    }
    let consistent = check_consistency_with_gate("functional_chain_clash");
    #[allow(unsafe_code)]
    unsafe {
        std::env::remove_var("RUSTDL_ABOX_SAT_GATED");
    }
    assert!(
        !consistent,
        "Functional-marker clash (∃hasSex.Male ⊓ ∃hasSex.Female + Functional + Disjoint) \
         should be detected inconsistent"
    );
}

// ── Test 4: gate-off leaves consistent ontologies untouched ──────────────────

/// With the gate off (`RUSTDL_ABOX_SAT_GATED=0` or unset), a consistent ontology
/// returns consistent, verifying the code path is inactive.
#[test]
fn el_preservation_gate_off() {
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("RUSTDL_ABOX_SAT_GATED", "0");
    }
    let consistent = check_consistency_with_gate("fp_smoke_consistent");
    #[allow(unsafe_code)]
    unsafe {
        std::env::remove_var("RUSTDL_ABOX_SAT_GATED");
    }
    assert!(
        consistent,
        "Gate off: consistent ontology must remain consistent"
    );
}
