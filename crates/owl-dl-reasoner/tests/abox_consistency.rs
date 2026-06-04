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
