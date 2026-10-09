//! #211: classify WITHOUT Layer A (`RUSTDL_CLASSIFY_ROLE_HIERARCHY=0`, or
//! above its affordability cap) matches role ids exactly in the wedge, so the
//! derived `≤1 p⁻` never fired on an `s`-edge with `s ⊑ p⁻` and the trusted
//! `Sat` missed the clash. Conversion now also emits the entailed
//! `∃s.⊤ ⊑ ≤1 s` for a sub-role carrying `≥2`, which needs no hierarchy.
//!
//! In its own file because it sets an env var: a test binary runs its tests
//! in parallel, and a sibling test would observe the `=0`.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

const OFN: &str = "Prefix(:=<http://ex.org/>)\nOntology(<http://ex.org/w>\n\
Declaration(Class(:A)) Declaration(Class(:D))\n\
Declaration(ObjectProperty(:p)) Declaration(ObjectProperty(:s))\n\
InverseFunctionalObjectProperty(:p)\n\
SubObjectPropertyOf(ObjectInverseOf(:s) :p)\n\
SubClassOf(:A ObjectMinCardinality(2 :s :D))\n)\n";

#[test]
#[allow(unsafe_code)]
fn a_min_two_below_the_inverse_clashes_in_classify_without_layer_a() {
    // SAFETY: set_var is unsafe under edition 2024; every test in this binary
    // sets the same value, so parallel tests cannot observe a different one.
    unsafe { std::env::set_var("RUSTDL_CLASSIFY_ROLE_HIERARCHY", "0") };
    let (o, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut Cursor::new(OFN), ParserConfiguration::default()).unwrap();
    let h = owl_dl_reasoner::classify(&o).unwrap();
    assert!(
        h.unsatisfiable_classes().contains(&"http://ex.org/A"),
        "classify without Layer A must derive the clash"
    );
    // The per-query surface is right on the same input (#211's fix).
    assert!(!owl_dl_reasoner::is_class_satisfiable(&o, "http://ex.org/A").unwrap());
}

/// Plain `Functional(p)` with `s ⊑ p` has the same shape (`HermiT` and Konclude:
/// unsat).
#[test]
#[allow(unsafe_code)]
fn a_min_two_on_a_sub_role_of_a_functional_role_clashes_without_layer_a() {
    // SAFETY: as above.
    unsafe { std::env::set_var("RUSTDL_CLASSIFY_ROLE_HIERARCHY", "0") };
    let ofn = "Prefix(:=<http://ex.org/>)\nOntology(<http://ex.org/w>\n\
Declaration(Class(:A)) Declaration(Class(:D))\n\
Declaration(ObjectProperty(:p)) Declaration(ObjectProperty(:s))\n\
FunctionalObjectProperty(:p) SubObjectPropertyOf(:s :p)\n\
SubClassOf(:A ObjectMinCardinality(2 :s :D))\n)\n";
    let (o, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut Cursor::new(ofn), ParserConfiguration::default()).unwrap();
    let h = owl_dl_reasoner::classify(&o).unwrap();
    assert!(h.unsatisfiable_classes().contains(&"http://ex.org/A"));
}
