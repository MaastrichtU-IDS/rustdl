//! #144 — `SymmetricObjectProperty(ObjectInverseOf(:p))` must act exactly like
//! `SymmetricObjectProperty(:p)`.
//!
//! A role is symmetric iff its inverse is, so the two spellings are semantically
//! interchangeable. rustdl acted on the named one and **silently ignored** the
//! inverse one — no `dropped` entry, no warning, just a missing entailment. Two
//! guards were responsible, both `!role.is_inverse()`: `expand_role_characteristics`
//! (which lowers `SymmetricRole` to a self-inverse `InverseObjectProperties` pair)
//! and the `mark_symmetric` arm of `build_role_hierarchy`.
//!
//! The premise recorded in the first guard's own doc comment — *"converter only
//! emits named-role characteristics today"* — had stopped being true: the converter
//! passes the axiom through with its polarity intact (`convert_object_property`
//! returns `Role::inverse(..)` for `ObjectInverseOf`), and both consumers then
//! declined it.
//!
//! # The control is what makes this a test
//!
//! Each positive is paired with the SAME ontology written with the named spelling.
//! Without that pair, a regression that broke both spellings would still look like
//! a pass on "the inverse spelling behaves like the named one". The pair pins the
//! named spelling as correct AND the two as equal.
//!
//! # Not extended to `Functional` / `InverseFunctional`
//!
//! Those carry the same `!role.is_inverse()` guard but are **not** interchangeable
//! across polarity — they SWAP. `Functional(p⁻)` is inverse-functional `p`, and
//! `InverseFunctional(p⁻)` is functional `p`. Relaxing their guard the same way
//! would be unsound. That gap was closed by #149 the SWAP way: conversion
//! normalizes `InverseFunctional(p⁻)` to `FunctionalRole(p)`
//! (`convert.rs`), and `inverse_functional_on_an_inverse_role_is_applied`
//! below (flipped from the pinned-gap test, per its own instruction) plus its
//! wrong-polarity FP guard pin both directions.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

fn parse(ofn: &str) -> SetOntology<RcStr> {
    let (o, _) = read_ofn(
        &mut Cursor::new(ofn.to_string()),
        ParserConfiguration::default(),
    )
    .expect("parse ofn");
    o
}

fn holds(ofn: &str, sub: &str, sup: &str) -> bool {
    owl_dl_reasoner::is_subclass_of(
        &parse(ofn),
        &format!("http://ex.org/{sub}"),
        &format!("http://ex.org/{sup}"),
    )
    .expect("subsumption query")
}

/// `Symmetric(p)` + `p ∘ q ⊑ q` makes `A ≡ B` for
/// `A ≡ ∃p.Y ⊓ ∃q.Z` and `B ≡ ∃p.(Y ⊓ ∃q.Z)`: symmetry turns `p(x,y)` into
/// `p(y,x)`, so the chain fires at the `Y` witness. `HermiT` derives the
/// equivalence for BOTH spellings of the symmetry axiom.
fn chain_with_symmetry(symmetry_axiom: &str) -> String {
    format!(
        "Prefix(:=<http://ex.org/>)\nOntology(<http://ex.org/s>\n\
Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:Y)) Declaration(Class(:Z))\n\
Declaration(ObjectProperty(:p)) Declaration(ObjectProperty(:q))\n\
{symmetry_axiom}\n\
SubObjectPropertyOf(ObjectPropertyChain(:p :q) :q)\n\
EquivalentClasses(:A ObjectIntersectionOf(\
ObjectSomeValuesFrom(:p :Y) ObjectSomeValuesFrom(:q :Z)))\n\
EquivalentClasses(:B ObjectSomeValuesFrom(:p ObjectIntersectionOf(\
:Y ObjectSomeValuesFrom(:q :Z))))\n)\n"
    )
}

#[test]
fn symmetry_on_an_inverse_role_expression_is_honoured() {
    // THE #144 FIXTURE.
    let ofn = chain_with_symmetry("SymmetricObjectProperty(ObjectInverseOf(:p))");
    assert!(
        holds(&ofn, "A", "B"),
        "A ⊑ B is entailed: Symmetric(p⁻) is Symmetric(p), so the chain fires at \
         the Y witness. Before #144 this spelling was silently ignored"
    );
}

#[test]
fn the_named_spelling_control_still_holds() {
    // THE CONTROL. Same ontology, named spelling. If this ever fails, the test
    // above proves nothing — both spellings would be agreeing on the wrong answer.
    let ofn = chain_with_symmetry("SymmetricObjectProperty(:p)");
    assert!(
        holds(&ofn, "A", "B"),
        "A ⊑ B is entailed with the named spelling; this is the control that makes \
         the inverse-spelling assertion meaningful"
    );
}

#[test]
fn both_spellings_agree() {
    // States the actual contract directly, rather than leaving it implied by two
    // separate assertions: one syntactic change must not change the answer.
    let named = chain_with_symmetry("SymmetricObjectProperty(:p)");
    let inverse = chain_with_symmetry("SymmetricObjectProperty(ObjectInverseOf(:p))");
    assert_eq!(
        holds(&named, "A", "B"),
        holds(&inverse, "A", "B"),
        "SymmetricObjectProperty(:p) and SymmetricObjectProperty(ObjectInverseOf(:p)) \
         are semantically interchangeable and must give the same answer"
    );
}

#[test]
fn symmetry_is_not_invented_where_it_was_not_declared() {
    // FP GUARD. The same ontology with NO symmetry axiom: `p(x,y)` does not give
    // `p(y,x)`, the chain cannot fire at the witness, and `A ⊑ B` must NOT hold.
    // A fix that marked roles symmetric too eagerly would derive it.
    let ofn = chain_with_symmetry("");
    assert!(
        !holds(&ofn, "A", "B"),
        "A ⊑ B is NOT entailed without a symmetry declaration on p"
    );
}

#[test]
fn inverse_functional_on_an_inverse_role_is_applied() {
    // FLIPPED (#149), as the pinned-gap version of this test instructed.
    // `InverseFunctional(p⁻)` IS `Functional(p)`; conversion now normalizes the
    // axiom to `FunctionalRole(p)` (`convert.rs`, the
    // `InverseFunctionalObjectProperty` arm), so the unconditional `≤1 p` GCI,
    // the saturator's functional bitset and `abox_check` P5 all see it. With
    // `C`/`D` disjoint, `A ≡ ∃p.C ⊓ ∃p.D` is UNSATISFIABLE — the two
    // `p`-successors merge and clash.
    let ofn = "Prefix(:=<http://ex.org/>)\nOntology(<http://ex.org/f>\n\
Declaration(Class(:A)) Declaration(Class(:C)) Declaration(Class(:D))\n\
Declaration(ObjectProperty(:p))\n\
InverseFunctionalObjectProperty(ObjectInverseOf(:p))\n\
DisjointClasses(:C :D)\n\
EquivalentClasses(:A ObjectIntersectionOf(\
ObjectSomeValuesFrom(:p :C) ObjectSomeValuesFrom(:p :D)))\n)\n";
    let sat = owl_dl_reasoner::is_class_satisfiable(&parse(ofn), "http://ex.org/A")
        .expect("satisfiability query");
    assert!(
        !sat,
        "InverseFunctional(p⁻) is Functional(p): A must be unsatisfiable (#149)"
    );
}

#[test]
fn inverse_functional_normalization_does_not_constrain_the_wrong_role() {
    // FP GUARD for the #149 normalization: flipping the characteristic the WRONG
    // way (`InverseFunctional(p⁻)` read as `InverseFunctional(p)`, i.e. a `≤1`
    // on `p⁻`) would merge the SOURCES of `p`-edges instead of the targets.
    // Here two ∃p⁻ successors of `A` must NOT merge: `InverseFunctional(p⁻)` =
    // `Functional(p)` says nothing about p⁻-successor multiplicity.
    let ofn = "Prefix(:=<http://ex.org/>)\nOntology(<http://ex.org/f2>\n\
Declaration(Class(:A)) Declaration(Class(:C)) Declaration(Class(:D))\n\
Declaration(ObjectProperty(:p))\n\
InverseFunctionalObjectProperty(ObjectInverseOf(:p))\n\
DisjointClasses(:C :D)\n\
EquivalentClasses(:A ObjectIntersectionOf(\
ObjectSomeValuesFrom(ObjectInverseOf(:p) :C) \
ObjectSomeValuesFrom(ObjectInverseOf(:p) :D)))\n)\n";
    let sat = owl_dl_reasoner::is_class_satisfiable(&parse(ofn), "http://ex.org/A")
        .expect("satisfiability query");
    assert!(
        sat,
        "Functional(p) does not bound p⁻-successors — deriving unsat here is the \
         wrong-polarity normalization (a false positive)"
    );
}
