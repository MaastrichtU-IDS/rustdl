//! #539: conversion states that `owl:bottomObjectProperty` / `owl:bottomDataProperty`
//! are empty (`∃b.⊤ ⊑ ⊥`), and that a universal role told empty or negatively asserted
//! is a contradiction (`⊤ ⊑ ⊥`). These check the CONVERTED axioms and their built-in
//! record; the per-surface answers are pinned in `owl-dl-cli/tests/bottom_role_cli.rs`.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::ir::ConceptExpr;
use owl_dl_core::ontology::{Axiom, InternalOntology};
use owl_dl_core::top_bottom_roles::{OWL_BOTTOM_DATA_PROPERTY, OWL_BOTTOM_OBJECT_PROPERTY};
use std::io::Cursor;

fn convert(body: &str) -> InternalOntology {
    let src = format!(
        "Prefix(:=<http://ex#>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
         Prefix(rdfs:=<http://www.w3.org/2000/01/rdf-schema#>)\nOntology(\n{body}\n)"
    );
    let o: SetOntology<RcStr> = read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .unwrap()
        .0;
    owl_dl_core::convert::convert_ontology(&o).unwrap()
}

/// Is `∃<iri>.⊤ ⊑ ⊥` among the axioms, and recorded as built-in?
fn has_empty_role_axiom(o: &InternalOntology, iri: &str) -> (bool, bool) {
    let Some(id) = o.vocabulary.role_id(iri) else {
        return (false, false);
    };
    let is_it = |ax: &Axiom| {
        matches!(ax, Axiom::SubClassOf { sub, sup }
            if matches!(o.concepts.get(*sup), ConceptExpr::Bot)
            && matches!(o.concepts.get(*sub), ConceptExpr::Some(r, f)
                if r.role_id() == id && !r.is_inverse()
                && matches!(o.concepts.get(*f), ConceptExpr::Top)))
    };
    (o.axioms.iter().any(is_it), o.builtin.iter().any(is_it))
}

/// Is `⊤ ⊑ ⊥` among the axioms, and recorded as built-in?
fn has_top_bot(o: &InternalOntology) -> (bool, bool) {
    let is_it = |ax: &Axiom| {
        matches!(ax, Axiom::SubClassOf { sub, sup }
            if matches!(o.concepts.get(*sub), ConceptExpr::Top)
            && matches!(o.concepts.get(*sup), ConceptExpr::Bot))
    };
    (o.axioms.iter().any(is_it), o.builtin.iter().any(is_it))
}

const BASE: &str = "Declaration(Class(:C)) Declaration(Class(:D)) Declaration(ObjectProperty(:r)) \
     Declaration(ObjectProperty(:s)) Declaration(DataProperty(:p)) \
     Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))";

// ── Negatives first: nothing is added without the built-in IRIs ──

/// Identity: an ontology that mentions none of the four built-in properties converts
/// exactly as before (no role interned, no axiom added, nothing built-in). Fails if the
/// emission is made unconditional.
#[test]
fn a_bottom_free_ontology_gains_no_role_and_no_axiom() {
    let body = format!(
        "{BASE} SubClassOf(:C ObjectSomeValuesFrom(:r :D)) SubObjectPropertyOf(:r :s) \
         ObjectPropertyAssertion(:r :a :b) NegativeObjectPropertyAssertion(:s :b :a)"
    );
    let o = convert(&body);
    assert!(o.builtin.is_empty(), "builtin: {:?}", o.builtin);
    for iri in [
        OWL_BOTTOM_OBJECT_PROPERTY,
        OWL_BOTTOM_DATA_PROPERTY,
        owl_dl_core::top_bottom_roles::OWL_TOP_OBJECT_PROPERTY,
        owl_dl_core::top_bottom_roles::OWL_TOP_DATA_PROPERTY,
    ] {
        assert!(o.vocabulary.role_id(iri).is_none(), "{iri} interned");
    }
    assert_eq!(has_top_bot(&o), (false, false));
}

/// Uses of top other than A3/A4 add nothing: `r ⊑ owl:topObjectProperty` (the only use
/// of top in the ORE pool), a positive assertion on top (a tautology), and `∃top.D` (the
/// universal role is otherwise an ordinary role; signal: #539 PR2; semantics: #543).
#[test]
fn other_top_uses_add_nothing() {
    let body = format!(
        "{BASE} SubObjectPropertyOf(:r owl:topObjectProperty) \
         SubDataPropertyOf(:p owl:topDataProperty) \
         ObjectPropertyAssertion(owl:topObjectProperty :a :b) \
         SubClassOf(:C ObjectSomeValuesFrom(owl:topObjectProperty :D))"
    );
    let o = convert(&body);
    assert!(o.builtin.is_empty(), "builtin: {:?}", o.builtin);
}

/// A negative assertion on an ordinary role, with top elsewhere, is not a contradiction.
#[test]
fn a_negative_assertion_on_an_ordinary_role_adds_nothing() {
    let body = format!(
        "{BASE} SubObjectPropertyOf(:r owl:topObjectProperty) \
         NegativeObjectPropertyAssertion(:r :a :b)"
    );
    let o = convert(&body);
    assert_eq!(has_top_bot(&o), (false, false));
}

/// The empty-role closure goes DOWN only, and never through a chain's legs (Fable's code
/// review, finding 1). With top interned these would turn a wrong B into `⊤ ⊑ ⊥`:
/// - `top ⊑ r`, `bottom ⊑ r`: bottom below a universal role empties nothing above it;
/// - `top ⊑ r`, `r ∘ s ⊑ bottom`: only the composition is empty, not `r` (it empties `s`);
/// - `top ⊑ r`, `s ⊑ r`, `s ⊑ bottom`: `s` is empty, its super-role `r` is not.
#[test]
fn the_empty_role_closure_goes_down_only_and_skips_chain_legs() {
    for rbox in [
        "SubObjectPropertyOf(owl:topObjectProperty :r) SubObjectPropertyOf(owl:bottomObjectProperty :r)",
        "SubObjectPropertyOf(owl:topObjectProperty :r) \
         SubObjectPropertyOf(ObjectPropertyChain(:r :s) owl:bottomObjectProperty)",
        "SubObjectPropertyOf(owl:topObjectProperty :r) SubObjectPropertyOf(:s :r) \
         SubObjectPropertyOf(:s owl:bottomObjectProperty)",
    ] {
        let o = convert(&format!("{BASE} {rbox}"));
        assert_eq!(has_top_bot(&o), (false, false), "{rbox}");
    }
}

/// `bottom ⊑ r` makes nothing universal or empty except bottom itself; only A1 fires.
#[test]
fn bottom_below_an_ordinary_role_adds_only_its_own_emptiness() {
    let body = format!("{BASE} SubObjectPropertyOf(owl:bottomObjectProperty :r)");
    let o = convert(&body);
    assert_eq!(
        has_empty_role_axiom(&o, OWL_BOTTOM_OBJECT_PROPERTY),
        (true, true)
    );
    assert_eq!(has_top_bot(&o), (false, false));
    assert_eq!(o.builtin.len(), 1);
}

// ── Positives ──

#[test]
fn a_mention_of_bottom_object_property_adds_its_emptiness() {
    let o = convert(&format!(
        "{BASE} SubClassOf(:C ObjectSomeValuesFrom(owl:bottomObjectProperty :D))"
    ));
    assert_eq!(
        has_empty_role_axiom(&o, OWL_BOTTOM_OBJECT_PROPERTY),
        (true, true)
    );
}

#[test]
fn a_mention_only_in_an_assertion_adds_it() {
    let o = convert(&format!(
        "{BASE} ObjectPropertyAssertion(owl:bottomObjectProperty :a :b)"
    ));
    assert_eq!(
        has_empty_role_axiom(&o, OWL_BOTTOM_OBJECT_PROPERTY),
        (true, true)
    );
}

#[test]
fn a_mention_of_bottom_data_property_adds_its_emptiness() {
    let o = convert(&format!(
        "{BASE} SubDataPropertyOf(:p owl:bottomDataProperty) \
         SubClassOf(:C DataSomeValuesFrom(:p rdfs:Literal))"
    ));
    assert_eq!(
        has_empty_role_axiom(&o, OWL_BOTTOM_DATA_PROPERTY),
        (true, true)
    );
}

/// A source that already states the axiom keeps it as the user's: not duplicated, not
/// recorded as built-in.
#[test]
fn a_stated_emptiness_axiom_stays_the_users() {
    let o = convert(&format!(
        "{BASE} SubClassOf(ObjectSomeValuesFrom(owl:bottomObjectProperty owl:Thing) owl:Nothing)"
    ));
    assert_eq!(
        has_empty_role_axiom(&o, OWL_BOTTOM_OBJECT_PROPERTY),
        (true, false)
    );
    assert!(o.builtin.is_empty());
}

/// A3: a universal role told empty, directly, two hops, through equivalence, an inverse
/// and an all-universal chain.
#[test]
fn a_universal_role_told_empty_is_a_contradiction() {
    for rbox in [
        "SubObjectPropertyOf(owl:topObjectProperty owl:bottomObjectProperty)",
        "SubObjectPropertyOf(owl:topObjectProperty :r) SubObjectPropertyOf(:r owl:bottomObjectProperty)",
        "EquivalentObjectProperties(:r owl:topObjectProperty) SubObjectPropertyOf(:r owl:bottomObjectProperty)",
        "SubObjectPropertyOf(owl:topObjectProperty ObjectInverseOf(:r)) InverseObjectProperties(:r :s) \
         SubObjectPropertyOf(:s owl:bottomObjectProperty)",
        "SubObjectPropertyOf(ObjectPropertyChain(owl:topObjectProperty owl:topObjectProperty) :r) \
         SubObjectPropertyOf(:r owl:bottomObjectProperty)",
    ] {
        let o = convert(&format!("{BASE} {rbox}"));
        assert_eq!(has_top_bot(&o), (true, true), "{rbox}");
    }
}

/// A chain with one ordinary leg does not make its super-role universal.
#[test]
fn a_chain_with_an_ordinary_leg_is_not_universal() {
    let o = convert(&format!(
        "{BASE} SubObjectPropertyOf(ObjectPropertyChain(owl:topObjectProperty :s) :r) \
         SubObjectPropertyOf(:r owl:bottomObjectProperty)"
    ));
    assert_eq!(has_top_bot(&o), (false, false));
}

/// A4: a negative assertion on a universal role, directly and on a super-role of top.
#[test]
fn a_negated_universal_assertion_is_a_contradiction() {
    for abox in [
        "NegativeObjectPropertyAssertion(owl:topObjectProperty :a :b)",
        "SubObjectPropertyOf(owl:topObjectProperty :r) NegativeObjectPropertyAssertion(:r :a :b)",
    ] {
        let o = convert(&format!("{BASE} {abox}"));
        assert_eq!(has_top_bot(&o), (true, true), "{abox}");
    }
}

/// `owl:topDataProperty` below `owl:bottomDataProperty` is outside OWL 2 DL (topData only
/// as a super-property), and both `HermiT` and Konclude answer "consistent": no `⊤ ⊑ ⊥`.
/// Only the bottom data role's own emptiness is stated.
#[test]
fn top_data_property_told_empty_adds_no_contradiction() {
    let o = convert(&format!(
        "{BASE} SubDataPropertyOf(owl:topDataProperty owl:bottomDataProperty)"
    ));
    assert_eq!(has_top_bot(&o), (false, false));
    assert_eq!(
        has_empty_role_axiom(&o, OWL_BOTTOM_DATA_PROPERTY),
        (true, true)
    );
}
