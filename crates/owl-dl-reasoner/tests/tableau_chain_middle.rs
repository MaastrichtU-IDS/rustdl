//! #213: the main tableau's role-chain rule only looked forward from a chain's
//! HEAD. An edge completing a chain further down dirties only its own
//! endpoints, so with edges created head-first (`h —r→ m`, then `m —r→ t`) the
//! composed `h —sup→ t` was never derived. `Transitive(r)` with
//! `∃r.∃r.¬D ⊓ ∀r.D` came back satisfiable on `sat` (and on classify with the
//! wedge off) with `incomplete: false`. The rule now also runs with the dirty
//! node as the chain's middle.
//!
//! These go through `is_class_satisfiable`, the deadline-free main-tableau
//! surface; default `classify` was already right via the wedge. Every verdict
//! is `HermiT`'s and Konclude's (they agree on all of them).

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

fn sat(axioms: &str) -> bool {
    let ofn = format!(
        "Prefix(:=<http://ex.org/>)\nOntology(<http://ex.org/t>\n\
         Declaration(Class(:A)) Declaration(Class(:D))\n\
         Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))\n\
         Declaration(ObjectProperty(:t)) Declaration(ObjectProperty(:ri))\n{axioms}\n)\n"
    );
    let (o, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut Cursor::new(ofn), ParserConfiguration::default()).expect("parse");
    owl_dl_reasoner::is_class_satisfiable(&o, "http://ex.org/A").expect("sat query")
}

const FORALL_R_D: &str = "SubClassOf(:A ObjectAllValuesFrom(:r :D))";

#[test]
fn forall_over_a_transitive_role_reaches_the_second_hop() {
    assert!(!sat(&format!(
        "TransitiveObjectProperty(:r) {FORALL_R_D} \
         SubClassOf(:A ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:r ObjectComplementOf(:D))))"
    )));
}

#[test]
fn forall_over_a_transitive_role_reaches_the_third_hop() {
    assert!(!sat(&format!(
        "TransitiveObjectProperty(:r) {FORALL_R_D} \
         SubClassOf(:A ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:r \
         ObjectSomeValuesFrom(:r ObjectComplementOf(:D)))))"
    )));
}

#[test]
fn transitivity_spelled_as_a_chain_and_through_sub_roles() {
    for axioms in [
        "SubObjectPropertyOf(ObjectPropertyChain(:r :r) :r) SubObjectPropertyOf(:s :r) \
         SubClassOf(:A ObjectSomeValuesFrom(:s ObjectSomeValuesFrom(:s ObjectComplementOf(:D))))",
        "TransitiveObjectProperty(:r) SubObjectPropertyOf(:s :r) \
         SubClassOf(:A ObjectSomeValuesFrom(:s ObjectSomeValuesFrom(:r ObjectComplementOf(:D))))",
        "TransitiveObjectProperty(:r) SubObjectPropertyOf(:s :r) \
         SubClassOf(:A ObjectSomeValuesFrom(:s ObjectSomeValuesFrom(:s ObjectComplementOf(:D))))",
    ] {
        assert!(
            !sat(&format!("{FORALL_R_D} {axioms}")),
            "should be unsat: {axioms}"
        );
    }
}

/// Not transitivity-specific: an ordinary `r ∘ s ⊑ t` had the same miss.
#[test]
fn a_general_chain_composes_in_order() {
    assert!(!sat("SubObjectPropertyOf(ObjectPropertyChain(:r :s) :t) \
         SubClassOf(:A ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:s ObjectComplementOf(:D)))) \
         SubClassOf(:A ObjectAllValuesFrom(:t :D))"));
}

/// The middle pass reads each leg with its polarity (`chain_leg_targets` at
/// `r1⁻` for the head side). Each of these is unsat with the fix and was sat
/// without it; each has a mirror guard below.
#[test]
fn chains_with_inverse_legs_or_an_inverse_super_role_compose() {
    for axioms in [
        // inverse first leg
        "SubObjectPropertyOf(ObjectPropertyChain(ObjectInverseOf(:r) :s) :t) \
         SubClassOf(:A ObjectAllValuesFrom(:t :D)) \
         SubClassOf(:A ObjectSomeValuesFrom(ObjectInverseOf(:r) \
         ObjectSomeValuesFrom(:s ObjectComplementOf(:D))))",
        // inverse second leg
        "SubObjectPropertyOf(ObjectPropertyChain(:r ObjectInverseOf(:s)) :t) \
         SubClassOf(:A ObjectAllValuesFrom(:t :D)) \
         SubClassOf(:A ObjectSomeValuesFrom(:r \
         ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectComplementOf(:D))))",
        // inverse super-role
        "SubObjectPropertyOf(ObjectPropertyChain(:r :s) ObjectInverseOf(:t)) \
         SubClassOf(:A ObjectAllValuesFrom(ObjectInverseOf(:t) :D)) \
         SubClassOf(:A ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:s ObjectComplementOf(:D))))",
        // everything inverse
        "SubObjectPropertyOf(ObjectPropertyChain(ObjectInverseOf(:r) ObjectInverseOf(:s)) \
         ObjectInverseOf(:t)) SubClassOf(:A ObjectAllValuesFrom(ObjectInverseOf(:t) :D)) \
         SubClassOf(:A ObjectSomeValuesFrom(ObjectInverseOf(:r) \
         ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectComplementOf(:D))))",
        // first leg reached through a declared inverse (`ri⁻ = r`)
        "InverseObjectProperties(:r :ri) SubObjectPropertyOf(ObjectPropertyChain(:r :s) :t) \
         SubClassOf(:A ObjectAllValuesFrom(:t :D)) \
         SubClassOf(:A ObjectSomeValuesFrom(ObjectInverseOf(:ri) \
         ObjectSomeValuesFrom(:s ObjectComplementOf(:D))))",
    ] {
        assert!(!sat(axioms), "should be unsat: {axioms}");
    }
}

#[test]
fn chains_do_not_compose_against_a_legs_polarity() {
    for axioms in [
        // `r⁻` where the chain wants `r`
        "SubObjectPropertyOf(ObjectPropertyChain(:r :s) :t) \
         SubClassOf(:A ObjectAllValuesFrom(:t :D)) \
         SubClassOf(:A ObjectSomeValuesFrom(ObjectInverseOf(:r) \
         ObjectSomeValuesFrom(:s ObjectComplementOf(:D))))",
        // `s⁻` where the chain wants `s`
        "SubObjectPropertyOf(ObjectPropertyChain(:r :s) :t) \
         SubClassOf(:A ObjectAllValuesFrom(:t :D)) \
         SubClassOf(:A ObjectSomeValuesFrom(:r \
         ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectComplementOf(:D))))",
        // `∀t` where the chain derives `t⁻`
        "SubObjectPropertyOf(ObjectPropertyChain(:r :s) ObjectInverseOf(:t)) \
         SubClassOf(:A ObjectAllValuesFrom(:t :D)) \
         SubClassOf(:A ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:s ObjectComplementOf(:D))))",
        // `ri` (= `r⁻`) where the chain wants `r`
        "InverseObjectProperties(:r :ri) SubObjectPropertyOf(ObjectPropertyChain(:r :s) :t) \
         SubClassOf(:A ObjectAllValuesFrom(:t :D)) \
         SubClassOf(:A ObjectSomeValuesFrom(:ri ObjectSomeValuesFrom(:s ObjectComplementOf(:D))))",
    ] {
        assert!(sat(axioms), "unsat here is an FP: {axioms}");
    }
}

/// CONTROL, not a fix positive: it passes with `RUSTDL_TABLEAU_CHAIN_MIDDLE=0`
/// too (here the edges happen to arrive in an order the head pass sees).
#[test]
fn the_inverse_of_a_transitive_role_is_transitive() {
    assert!(!sat("TransitiveObjectProperty(:r) \
         SubClassOf(:A ObjectSomeValuesFrom(ObjectInverseOf(:r) \
         ObjectSomeValuesFrom(ObjectInverseOf(:r) ObjectComplementOf(:D)))) \
         SubClassOf(:A ObjectAllValuesFrom(ObjectInverseOf(:r) :D))"));
}

// FP guards: the middle pass must compose only what the chain licenses.

#[test]
fn no_chain_no_composition() {
    assert!(sat(&format!(
        "{FORALL_R_D} \
         SubClassOf(:A ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:r ObjectComplementOf(:D))))"
    )));
}

#[test]
fn a_chain_does_not_compose_in_the_wrong_order() {
    assert!(sat("SubObjectPropertyOf(ObjectPropertyChain(:r :s) :t) \
         SubClassOf(:A ObjectSomeValuesFrom(:s ObjectSomeValuesFrom(:r ObjectComplementOf(:D)))) \
         SubClassOf(:A ObjectAllValuesFrom(:t :D))"));
}

#[test]
fn a_transitive_role_does_not_compose_across_polarity() {
    assert!(sat(&format!(
        "TransitiveObjectProperty(:r) {FORALL_R_D} \
         SubClassOf(:A ObjectSomeValuesFrom(:r \
         ObjectSomeValuesFrom(ObjectInverseOf(:r) ObjectComplementOf(:D))))"
    )));
}
