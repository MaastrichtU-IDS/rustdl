//! Regression test for the issue-#35 tableau **depth-cap** hang.
//!
//! Distinct from the 2026-07-21 realize saturation fast-path fix: that one
//! keeps `realize` off the tableau on the EL/Horn fragment. This one targets
//! the *tableau itself*, exercised by `is_consistent` / `is_class_satisfiable`.
//!
//! The ontology's completion graph is bounded by pair-blocking (~200–330
//! nodes), but the two `EquivalentClasses` reverse-directions plus the
//! `Man ⊔ Woman` union absorb into three open `⊔`s on every `Person` node.
//! A clash-free model needs ~650 sequential ⊔/choose decisions — far past the
//! old `MAX_SEARCH_DEPTH = 256`. Because a depth-cutoff yields `DepthLimit`
//! (no clash deps ⇒ no back-jumping), the search could not prune and instead
//! enumerated the exponential ⊔-space forever: a >300 s hang on the
//! deadline-free query paths (`sat` / `consistent`).
//!
//! With the fix (deep recursion budget on a large-stack thread for the
//! deadline-free paths, so termination rests on blocking, not the cap) these
//! return the correct verdict quickly. The satisfiable model exists, so the
//! ontology is consistent and `Person` is satisfiable.

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{is_class_satisfiable, is_consistent};
use std::fs;
use std::io::Cursor;
use std::path::Path;

fn load() -> SetOntology<RcStr> {
    let path = Path::new("tests/fixtures/regression/issue35_tableau_depth_hang.ofn");
    let src = fs::read_to_string(path).unwrap_or_else(|e| panic!("read fixture: {e}"));
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    onto
}

#[test]
fn issue35_is_consistent_terminates() {
    let onto = load();
    assert!(
        is_consistent(&onto).expect("is_consistent must return a verdict, not hang"),
        "the 5-axiom core has a (small) satisfying model ⇒ consistent"
    );
}

#[test]
fn issue35_person_is_satisfiable() {
    let onto = load();
    assert!(
        is_class_satisfiable(&onto, "http://example.org/hang2#Person")
            .expect("is_class_satisfiable must return a verdict, not hang"),
        "Person is satisfiable (pick Man; maternal chain blocks)"
    );
}
