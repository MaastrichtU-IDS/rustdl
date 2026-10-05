//! A TBox-only KB made inconsistent by a reflexive role (#194).
//!
//! `Reflexive(r)` + `Asymmetric(r)` has no model: every element has an `r` self-loop,
//! which asymmetry forbids. The wedge clausifier derives this as an empty clause
//! (`r(X,y) ∧ r(y,X) → ⊥` contracted by reflexivity to `→ ⊥`), but neither surface
//! read it: `classify` reported every class unsatisfiable alongside
//! `consistent: true`, and `is_consistent` said consistent. HermiT calls it
//! inconsistent.
//!
//! The negative controls are the load-bearing half: a spurious verdict here marks
//! every class unsatisfiable.

#![allow(clippy::unwrap_used)]
#![allow(clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

fn onto(body: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<http://ex#>)\n\
         Ontology(\n\
         Declaration(Class(:A)) Declaration(Class(:B))\n\
         Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s)) Declaration(ObjectProperty(:t))\n\
         SubClassOf(:A :B)\n\
         {body}\n)"
    );
    let mut reader = Cursor::new(src);
    read_ofn(&mut reader, ParserConfiguration::default())
        .expect("parse")
        .0
}

/// `(classify says consistent, is_consistent)`.
fn verdicts(body: &str) -> (bool, bool) {
    let o = onto(body);
    (
        !owl_dl_reasoner::classify(&o).unwrap().stats().inconsistent,
        owl_dl_reasoner::is_consistent(&o).unwrap(),
    )
}

#[test]
fn reflexive_and_asymmetric_role_is_inconsistent_on_both_surfaces() {
    assert_eq!(
        verdicts("ReflexiveObjectProperty(:r)\nAsymmetricObjectProperty(:r)"),
        (false, false)
    );
}

/// Reflexivity reaches a super-role, so an asymmetric super-role clashes too.
#[test]
fn asymmetric_super_role_of_a_reflexive_role_is_inconsistent() {
    assert_eq!(
        verdicts(
            "ReflexiveObjectProperty(:r)\nSubObjectPropertyOf(:r :s)\nAsymmetricObjectProperty(:s)"
        ),
        (false, false)
    );
}

#[test]
fn reflexive_and_irreflexive_role_is_inconsistent_on_both_surfaces() {
    assert_eq!(
        verdicts("ReflexiveObjectProperty(:r)\nIrreflexiveObjectProperty(:r)"),
        (false, false)
    );
}

/// Control: the characteristics sit on different roles.
#[test]
fn asymmetry_on_an_unrelated_role_is_consistent() {
    assert_eq!(
        verdicts("ReflexiveObjectProperty(:r)\nAsymmetricObjectProperty(:t)"),
        (true, true)
    );
}

/// Control: reflexivity is closed UPWARD only. An asymmetric SUB-role of a reflexive
/// role need not be reflexive, so this is consistent.
#[test]
fn asymmetric_sub_role_of_a_reflexive_role_is_consistent() {
    assert_eq!(
        verdicts(
            "ReflexiveObjectProperty(:r)\nSubObjectPropertyOf(:t :r)\nAsymmetricObjectProperty(:t)"
        ),
        (true, true)
    );
}

/// Control: reflexive alone.
#[test]
fn reflexive_role_alone_is_consistent() {
    assert_eq!(verdicts("ReflexiveObjectProperty(:r)"), (true, true));
}
