//! Pattern unit tests for the ABox consistency pre-check.
//!
//! Each pattern (P1–P7) has a positive fixture (asserts
//! `is_consistent → false`) and a negative near-miss (asserts
//! `is_consistent → true`). Sound-positive AND sound-negative
//! coverage.
//!
//! Spec: `docs/superpowers/specs/2026-06-04-abox-consistency-check-design.md`

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::is_consistent;
use std::fs;
use std::io::Cursor;
use std::path::Path;

const FIXTURE_DIR: &str = "tests/fixtures/abox";

fn check_consistency(name: &str) -> bool {
    let path = Path::new(FIXTURE_DIR).join(format!("{name}.ofn"));
    let src = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    is_consistent(&onto).expect("is_consistent succeeds")
}

// ── P1: Direct ⊥ assertion ──────────────────────────────────────────

#[test]
fn p1_direct_bot_is_inconsistent() {
    assert!(!check_consistency("p1_direct_bot"),
        "P1: ClassAssertion(C, a) + C ⊑ ⊥ should be inconsistent");
}

#[test]
fn p1_no_bot_assertion_is_consistent() {
    assert!(check_consistency("p1_no_bot"),
        "P1 negative: Unsat class with no asserted member should stay consistent");
}

// ── P2: Disjoint types on the same individual ───────────────────────

#[test]
fn p2_disjoint_types_is_inconsistent() {
    assert!(!check_consistency("p2_disjoint_types"),
        "P2: same individual asserted in two disjoint classes should be inconsistent");
}

#[test]
fn p2_disjoint_different_individuals_is_consistent() {
    assert!(check_consistency("p2_disjoint_different_individuals"),
        "P2 negative: disjoint classes asserted on DIFFERENT individuals should stay consistent");
}

// ── P3: NegativeObjectPropertyAssertion vs ObjectPropertyAssertion ──

#[test]
fn p3_neg_opa_is_inconsistent() {
    assert!(!check_consistency("p3_neg_opa"),
        "P3: positive OPA + NegOPA on same (a, R, b) should be inconsistent");
}

#[test]
fn p3_neg_opa_no_clash_is_consistent() {
    assert!(check_consistency("p3_neg_opa_no_clash"),
        "P3 negative: NegOPA to a DIFFERENT target should stay consistent");
}

// ── P4: SameAs ∩ DifferentFrom (transitive) ─────────────────────────

#[test]
fn p4_same_then_different_is_inconsistent() {
    assert!(!check_consistency("p4_same_different"),
        "P4: SameIndividual(a,b) + SameIndividual(b,c) + DifferentIndividuals(a,c) inconsistent");
}

#[test]
fn p4_same_without_different_is_consistent() {
    assert!(check_consistency("p4_same_without_different"),
        "P4 negative: SameAs(a,b) + DifferentFrom(c,d) over disjoint pairs is consistent");
}

// ── P5: Functional role + two distinct witnesses ────────────────────

#[test]
fn p5_functional_distinct_witnesses_is_inconsistent() {
    assert!(!check_consistency("p5_functional_diff"),
        "P5: Functional(R) + R(a,b1) + R(a,b2) + Different(b1,b2) should be inconsistent");
}

#[test]
fn p5_functional_no_different_is_consistent() {
    assert!(check_consistency("p5_functional_same_target"),
        "P5 negative: same two facts WITHOUT DifferentIndividuals stay consistent (OWA)");
}

// ── P6: Asymmetric / Irreflexive violations ─────────────────────────

#[test]
fn p6_asymmetric_two_way_is_inconsistent() {
    assert!(!check_consistency("p6_asymmetric"),
        "P6: Asymmetric(R) + R(a,b) + R(b,a) should be inconsistent");
}

#[test]
fn p6_asymmetric_one_way_is_consistent() {
    assert!(check_consistency("p6_asymmetric_one_way"),
        "P6 negative: Asymmetric(R) + R(a,b) alone should stay consistent");
}

#[test]
fn p6_irreflexive_self_loop_is_inconsistent() {
    assert!(!check_consistency("p6_irreflexive"),
        "P6: Irreflexive(R) + R(a,a) should be inconsistent");
}

#[test]
fn p6_irreflexive_distinct_pair_is_consistent() {
    assert!(check_consistency("p6_irreflexive_distinct"),
        "P6 negative: Irreflexive(R) + R(a,b) with distinct a,b should stay consistent");
}

// ── P7: Domain/range disjointness propagation (stretch) ─────────────

#[test]
fn p7_range_clashes_with_assertion_is_inconsistent() {
    assert!(!check_consistency("p7_range_disjoint"),
        "P7: range(R)=Female + R(c,m) + ClassAssertion(Male,m) + Male/Female disjoint inconsistent");
}

#[test]
fn p7_range_compatible_is_consistent() {
    assert!(check_consistency("p7_range_compatible"),
        "P7 negative: range and explicit class agree → consistent");
}
