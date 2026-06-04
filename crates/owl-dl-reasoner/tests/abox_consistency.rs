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
