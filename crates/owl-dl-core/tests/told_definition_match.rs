//! `RUSTDL_TOLD_DEFINITION_MATCH` (#139): `A ⊑ X`, `D ≡ X` ⟹ `A ⊑ D` by
//! interned identity, emitted as an atomic axiom at conversion.
//!
//! These check the CONVERTED axioms, not a classification: in isolation the
//! tableau proves the motivating pair on its own, so only the conversion output
//! shows whether the pass fired. Its own file because one test sets an env var;
//! every test takes `ENV`.

#![allow(clippy::unwrap_used)]
#![allow(unsafe_code)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::ir::ConceptExpr;
use owl_dl_core::ontology::{Axiom, InternalOntology};
use std::io::Cursor;
use std::sync::Mutex;

static ENV: Mutex<()> = Mutex::new(());

fn convert(body: &str) -> InternalOntology {
    let _g = ENV
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let src = format!("Prefix(:=<http://ex#>)\nOntology(\n{body}\n)");
    let o: SetOntology<RcStr> = read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .unwrap()
        .0;
    owl_dl_core::convert::convert_ontology(&o).unwrap()
}

/// Is the atomic axiom `SubClassOf(:a, :d)` in the converted ontology?
fn has_edge(o: &InternalOntology, a: &str, d: &str) -> bool {
    let (Some(a), Some(d)) = (
        o.vocabulary.class_id(&format!("http://ex#{a}")),
        o.vocabulary.class_id(&format!("http://ex#{d}")),
    ) else {
        return false;
    };
    o.axioms.iter().any(|ax| {
        matches!(ax, Axiom::SubClassOf { sub, sup }
            if matches!(o.concepts.get(*sub), ConceptExpr::Atomic(c) if *c == a)
            && matches!(o.concepts.get(*sup), ConceptExpr::Atomic(c) if *c == d))
    })
}

/// The #139 shape: an asserted `=1` restriction identical to a definition.
const MOTIVATING: &str = "SubClassOf(:S ObjectExactCardinality(1 :hasMember :P))
    EquivalentClasses(:E ObjectExactCardinality(1 :hasMember :P))";

#[test]
fn an_asserted_expression_matching_a_definition_yields_the_atomic_edge() {
    assert!(has_edge(&convert(MOTIVATING), "S", "E"));
}

/// Conjuncts may be spread over told superclasses: `A ⊑ B ⊑ X` and `A ⊑ K`
/// satisfy `D ≡ K ⊓ X`.
#[test]
fn conjuncts_are_collected_through_told_superclasses() {
    let o = convert(
        "SubClassOf(:A :B)
         SubClassOf(:B ObjectMinCardinality(2 :r :P))
         SubClassOf(:A :K)
         EquivalentClasses(:D ObjectIntersectionOf(:K ObjectMinCardinality(2 :r :P)))",
    );
    assert!(has_edge(&o, "A", "D"));
}

/// The fixpoint: `A ⊑ D1` (round 1) lets `A` inherit `D1 ⊑ Y`, and with its own
/// `X` that matches `D2 ≡ X ⊓ Y` in round 2. Neither `A` nor `D1` matches `D2`
/// alone, so the edge is not merely transitive.
#[test]
fn a_derived_edge_enables_a_further_match() {
    let o = convert(
        "SubClassOf(:A ObjectMaxCardinality(1 :r :P))
         SubClassOf(:A ObjectMinCardinality(2 :t :R))
         EquivalentClasses(:D1 ObjectMaxCardinality(1 :r :P))
         SubClassOf(:D1 ObjectMinCardinality(3 :s :Q))
         EquivalentClasses(:D2 ObjectIntersectionOf(ObjectMinCardinality(2 :t :R) ObjectMinCardinality(3 :s :Q)))",
    );
    assert!(has_edge(&o, "A", "D1"));
    assert!(!has_edge(&o, "D1", "D2"), "D1 lacks X");
    assert!(has_edge(&o, "A", "D2"));
}

/// FP guard: asserting one conjunct of a two-conjunct definition is not enough.
#[test]
fn a_partial_match_does_not_fire() {
    let o = convert(
        "SubClassOf(:A ObjectMinCardinality(2 :r :P))
         EquivalentClasses(:D ObjectIntersectionOf(ObjectMinCardinality(2 :r :P) ObjectMaxCardinality(4 :r :P)))",
    );
    assert!(!has_edge(&o, "A", "D"));
}

/// FP guard, the sharper form: `A` asserts the RAREST conjunct, so it IS a
/// candidate, and lacks the other. (In the test above `A` is never a candidate,
/// so it cannot tell `all` from `any`; this one can.)
#[test]
fn a_candidate_missing_a_conjunct_does_not_fire() {
    let o = convert(
        "SubClassOf(:A ObjectMinCardinality(2 :r :P))
         SubClassOf(:B ObjectMaxCardinality(4 :r :P))
         SubClassOf(:C ObjectMaxCardinality(4 :r :P))
         SubClassOf(:F ObjectMaxCardinality(4 :r :P))
         EquivalentClasses(:D ObjectIntersectionOf(ObjectMinCardinality(2 :r :P) ObjectMaxCardinality(4 :r :P)))",
    );
    assert!(!has_edge(&o, "A", "D"));
}

/// An atomic conjunct satisfied only through an atom asserted inside an `And`
/// on a told superclass: `A ⊑ B`, `B ⊑ X ⊓ M`, `M ⊑ K` covers `K` in
/// `D ≡ X ⊓ K`. `K` is not a told superclass of `A` (told tables do not split
/// an asserted `And`), so only the second clause of `has_atom` decides it.
#[test]
fn an_atomic_conjunct_is_reached_through_an_asserted_conjunction() {
    let o = convert(
        "SubClassOf(:A :B)
         SubClassOf(:B ObjectIntersectionOf(ObjectMinCardinality(2 :r :P) :M))
         SubClassOf(:M :K)
         EquivalentClasses(:D ObjectIntersectionOf(ObjectMinCardinality(2 :r :P) :K))",
    );
    assert!(has_edge(&o, "A", "D"));
}

/// A class in a three-member equivalence asserts each complex member, and a
/// defined class satisfies a WEAKER definition: `D1 ⊑ D2`, not the converse.
#[test]
fn equivalence_members_and_weaker_definitions_are_matched() {
    let o = convert(
        "EquivalentClasses(:E1 :E2 ObjectMinCardinality(2 :r :P))
         SubClassOf(:A ObjectMinCardinality(2 :r :P))
         EquivalentClasses(:D1 ObjectIntersectionOf(ObjectMinCardinality(2 :r :P) ObjectMaxCardinality(3 :r :P)))
         EquivalentClasses(:D2 ObjectMinCardinality(2 :r :P))",
    );
    assert!(has_edge(&o, "A", "E1") && has_edge(&o, "A", "E2"));
    assert!(has_edge(&o, "D1", "D2"));
    assert!(!has_edge(&o, "D2", "D1"), "the converse is not entailed");
}

/// FP guard: a different expression (even an entailing one) is not matched, and
/// a `SubClassOf` on the other side is not a definition.
#[test]
fn non_identical_or_non_defining_axioms_do_not_fire() {
    let o = convert(
        "SubClassOf(:A ObjectMinCardinality(3 :r :P))
         EquivalentClasses(:D ObjectMinCardinality(2 :r :P))
         SubClassOf(:B ObjectMaxCardinality(1 :r :P))
         SubClassOf(:F ObjectMaxCardinality(1 :r :P))",
    );
    assert!(!has_edge(&o, "A", "D"), "different interned ids");
    assert!(
        !has_edge(&o, "B", "F"),
        "F is not defined by the expression"
    );
}

/// An all-EL definition is left to the saturator, which derives it anyway:
/// matching those cost tens of seconds on the PRO-derived ORE ontologies.
#[test]
fn an_all_el_definition_is_left_to_the_saturator() {
    let o = convert(
        "SubClassOf(:A ObjectSomeValuesFrom(:r :P))
         EquivalentClasses(:D ObjectSomeValuesFrom(:r :P))",
    );
    assert!(!has_edge(&o, "A", "D"));
}

/// One non-EL conjunct is enough to qualify, and the EL conjuncts must still
/// match: the filter selects definitions, it does not drop conjuncts.
#[test]
fn a_definition_with_one_non_el_conjunct_is_matched_in_full() {
    let o = convert(
        "SubClassOf(:A ObjectIntersectionOf(ObjectSomeValuesFrom(:r :P) ObjectMaxCardinality(1 :s :Q)))
         SubClassOf(:B ObjectMaxCardinality(1 :s :Q))
         EquivalentClasses(:D ObjectIntersectionOf(ObjectSomeValuesFrom(:r :P) ObjectMaxCardinality(1 :s :Q)))",
    );
    assert!(has_edge(&o, "A", "D"));
    assert!(!has_edge(&o, "B", "D"), "B lacks the EL conjunct");
}

#[test]
fn the_flag_switches_the_pass_off() {
    let _g = ENV
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // SAFETY: every env access in this binary holds `ENV`.
    unsafe { std::env::set_var("RUSTDL_TOLD_DEFINITION_MATCH", "0") };
    let src = format!("Prefix(:=<http://ex#>)\nOntology(\n{MOTIVATING}\n)");
    let o: SetOntology<RcStr> = read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .unwrap()
        .0;
    let conv = owl_dl_core::convert::convert_ontology(&o).unwrap();
    // SAFETY: as above.
    unsafe { std::env::remove_var("RUSTDL_TOLD_DEFINITION_MATCH") };
    assert!(!has_edge(&conv, "S", "E"));
}
