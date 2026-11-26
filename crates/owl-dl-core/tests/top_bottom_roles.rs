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

/// #538 meets #539 (the merge conflict sat where `builtin` and `collapsed_operands` meet):
/// a built-in axiom is never counted as a collapsed n-ary axiom. Only the source's
/// repeated-operand axiom is counted, with or without the bottom role.
#[test]
fn a_builtin_axiom_is_never_a_collapsed_operand_axiom() {
    let o = convert(&format!(
        "{BASE} SubClassOf(:C ObjectSomeValuesFrom(owl:bottomObjectProperty :D))"
    ));
    assert_eq!(o.builtin.len(), 1);
    assert!(
        o.collapsed_operands.is_empty(),
        "{:?}",
        o.collapsed_operands
    );
    let o = convert(&format!(
        "{BASE} SubClassOf(:C ObjectSomeValuesFrom(owl:bottomObjectProperty :D)) \
         EquivalentClasses(:C :C :D)"
    ));
    assert_eq!(o.builtin.len(), 1);
    assert_eq!(o.collapsed_operands.axioms, 1, "{:?}", o.collapsed_operands);
}

// ── #543: where a universal role is read as an ordinary one ──

fn flagged(body: &str) -> bool {
    owl_dl_core::top_bottom_roles::universal_role_ordinary(&convert(&format!("{BASE} {body}")))
}

/// Inert occurrences: each holds in the model that reinterprets the universal role as
/// `Δ × Δ`, so no class-level entailment depends on it.
#[test]
fn inert_universal_role_occurrences_are_not_flagged() {
    for body in [
        // No top at all (and the O(1) exit).
        "SubClassOf(:C ObjectSomeValuesFrom(:r :D))",
        // `x ⊑ top`, the only ORE use.
        "SubObjectPropertyOf(:r owl:topObjectProperty) SubClassOf(:C ObjectSomeValuesFrom(:r :D))",
        "SubDataPropertyOf(:p owl:topDataProperty)",
        // A positive assertion, and characteristics `Δ × Δ` has.
        "ObjectPropertyAssertion(owl:topObjectProperty :a :b)",
        "TransitiveObjectProperty(owl:topObjectProperty) \
         ReflexiveObjectProperty(owl:topObjectProperty) \
         SymmetricObjectProperty(owl:topObjectProperty)",
        // Equivalence and inverse within U, and a chain INTO U.
        "EquivalentObjectProperties(owl:topObjectProperty :r)",
        "InverseObjectProperties(owl:topObjectProperty :r)",
        "SubObjectPropertyOf(ObjectPropertyChain(:r :s) owl:topObjectProperty)",
        // A chain with a universal leg INTO U: `top ∘ s ⊑ top` always holds.
        "SubObjectPropertyOf(ObjectPropertyChain(owl:topObjectProperty :s) owl:topObjectProperty)",
        "SubObjectPropertyOf(owl:topObjectProperty :r)",
        // A negated assertion: #539's A4 already makes it `⊤ ⊑ ⊥`.
        "NegativeObjectPropertyAssertion(owl:topObjectProperty :a :b)",
        // #539's A3: `top ⊑ r ⊑ bottom` puts bottom in U, and the built-in
        // `∃bottom.⊤ ⊑ ⊥` must not count as a use.
        "SubObjectPropertyOf(owl:topObjectProperty :r) \
         SubObjectPropertyOf(:r owl:bottomObjectProperty)",
    ] {
        assert!(!flagged(body), "inert, not flagged: {body}");
    }
}

/// Semantic uses: each can lose an entailment under the ordinary reading.
#[test]
fn semantic_universal_role_uses_are_flagged() {
    for body in [
        "SubClassOf(ObjectSomeValuesFrom(owl:topObjectProperty :D) :C)",
        "SubClassOf(:C ObjectSomeValuesFrom(owl:topObjectProperty :D))",
        "SubClassOf(:C ObjectAllValuesFrom(owl:topObjectProperty :D))",
        "SubClassOf(:C ObjectMinCardinality(2 owl:topObjectProperty :D))",
        "SubClassOf(:C ObjectMaxCardinality(1 owl:topObjectProperty :D))",
        "SubClassOf(:C ObjectHasSelf(owl:topObjectProperty))",
        "SubClassOf(:C ObjectHasValue(owl:topObjectProperty :a))",
        "SubClassOf(:C ObjectSomeValuesFrom(ObjectInverseOf(owl:topObjectProperty) :D))",
        "ClassAssertion(ObjectAllValuesFrom(owl:topObjectProperty :D) :a)",
        "ObjectPropertyDomain(owl:topObjectProperty :C)",
        "ObjectPropertyRange(owl:topObjectProperty :C)",
        "FunctionalObjectProperty(owl:topObjectProperty)",
        "InverseFunctionalObjectProperty(owl:topObjectProperty)",
        "IrreflexiveObjectProperty(owl:topObjectProperty)",
        "AsymmetricObjectProperty(owl:topObjectProperty)",
        "DisjointObjectProperties(owl:topObjectProperty :r)",
        // A chain with a universal leg into an ordinary role.
        "SubObjectPropertyOf(ObjectPropertyChain(:s owl:topObjectProperty) :r)",
        // A role above top, used: `top ⊑ r` makes `r` universal.
        "SubObjectPropertyOf(owl:topObjectProperty :r) \
         SubClassOf(ObjectSomeValuesFrom(:r :D) :C)",
        // `owl:topDataProperty` outside the super-property position.
        "SubClassOf(:C DataSomeValuesFrom(owl:topDataProperty rdfs:Literal))",
    ] {
        assert!(flagged(body), "a semantic use, flagged: {body}");
    }
}

/// The role-level companion: `top ⊑ r` is inert for classes but makes `r` relate every
/// pair, which the property-assertion surfaces do not list.
#[test]
fn a_role_above_top_is_a_universal_super_role() {
    let o = convert(&format!(
        "{BASE} SubObjectPropertyOf(owl:topObjectProperty :r)"
    ));
    assert!(!owl_dl_core::top_bottom_roles::universal_role_ordinary(&o));
    let r = o.vocabulary.role_id("http://ex#r").unwrap();
    assert!(owl_dl_core::top_bottom_roles::universal_super_roles(&o).contains(&r));
    let plain = convert(&format!(
        "{BASE} SubObjectPropertyOf(:r owl:topObjectProperty)"
    ));
    assert!(owl_dl_core::top_bottom_roles::universal_super_roles(&plain).is_empty());
}
