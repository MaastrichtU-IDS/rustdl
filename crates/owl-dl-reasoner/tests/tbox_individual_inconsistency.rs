//! A TBox-only KB made inconsistent through a named individual (#194 item 2).
//!
//! Every named individual denotes a domain element, so `{a} ⊑ ⊥` has no model,
//! and neither does `{a} ⊑ ∃r.{a}` with `r` irreflexive. The wedge consistency
//! route seeds one node per individual and catches all of these, but it was
//! only built when the KB had an `ABox` axiom: with none, `classify` and
//! `is_consistent` both reported consistent. Adding any unrelated assertion
//! made both surfaces correct, which is what located the gate.
//!
//! Every inconsistent case is confirmed by HermiT. Konclude agrees on the
//! `⊥`/`ObjectHasValue` cases and reports the cycle cases consistent, its usual
//! under-report.

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
         Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
         Ontology(\n\
         Declaration(Class(:A)) Declaration(ObjectProperty(:r))\n\
         Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
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
fn an_unsatisfiable_individual_is_inconsistent() {
    assert_eq!(
        verdicts("SubClassOf(ObjectOneOf(:a) owl:Nothing)"),
        (false, false)
    );
}

#[test]
fn an_individual_in_an_unsatisfiable_class_is_inconsistent() {
    assert_eq!(
        verdicts("SubClassOf(ObjectOneOf(:a) :A)\nSubClassOf(:A owl:Nothing)"),
        (false, false)
    );
}

#[test]
fn a_self_cycle_through_an_individual_breaks_irreflexivity() {
    assert_eq!(
        verdicts(
            "IrreflexiveObjectProperty(:r)\n\
             SubClassOf(ObjectOneOf(:a) ObjectSomeValuesFrom(:r ObjectOneOf(:a)))"
        ),
        (false, false)
    );
}

#[test]
fn a_has_value_self_cycle_breaks_irreflexivity() {
    assert_eq!(
        verdicts(
            "IrreflexiveObjectProperty(:r)\nSubClassOf(ObjectOneOf(:a) ObjectHasValue(:r :a))"
        ),
        (false, false)
    );
}

#[test]
fn a_two_cycle_through_individuals_breaks_asymmetry() {
    assert_eq!(
        verdicts(
            "AsymmetricObjectProperty(:r)\n\
             SubClassOf(ObjectOneOf(:a) ObjectSomeValuesFrom(:r ObjectOneOf(:b)))\n\
             SubClassOf(ObjectOneOf(:b) ObjectSomeValuesFrom(:r ObjectOneOf(:a)))"
        ),
        (false, false)
    );
}

/// The individual reaches the clash only through a class.
#[test]
fn a_self_cycle_through_a_class_breaks_irreflexivity() {
    assert_eq!(
        verdicts(
            "IrreflexiveObjectProperty(:r)\n\
             SubClassOf(:A ObjectHasValue(:r :a))\nSubClassOf(ObjectOneOf(:a) :A)"
        ),
        (false, false)
    );
}

/// FP guard: the self-cycle is fine without irreflexivity.
#[test]
fn a_self_cycle_alone_is_consistent() {
    assert_eq!(
        verdicts("SubClassOf(ObjectOneOf(:a) ObjectSomeValuesFrom(:r ObjectOneOf(:a)))"),
        (true, true)
    );
}

/// FP guard: a one-way edge between two individuals respects asymmetry.
#[test]
fn a_one_way_edge_respects_asymmetry() {
    assert_eq!(
        verdicts(
            "AsymmetricObjectProperty(:r)\n\
             SubClassOf(ObjectOneOf(:a) ObjectSomeValuesFrom(:r ObjectOneOf(:b)))"
        ),
        (true, true)
    );
}

/// FP guard: an edge to a different individual respects irreflexivity, since
/// `a` and `b` are not forced to be the same.
#[test]
fn an_edge_to_another_individual_respects_irreflexivity() {
    assert_eq!(
        verdicts(
            "IrreflexiveObjectProperty(:r)\n\
             SubClassOf(ObjectOneOf(:a) ObjectSomeValuesFrom(:r ObjectOneOf(:b)))"
        ),
        (true, true)
    );
}

/// FP guard: an unsatisfiable class with no individual in it is not an
/// inconsistency.
#[test]
fn an_unsatisfiable_class_without_individuals_is_consistent() {
    assert_eq!(
        verdicts("SubClassOf(:A owl:Nothing)\nSubClassOf(:A ObjectHasValue(:r :a))"),
        (true, true)
    );
}
