//! #180: `RUSTDL_SALVAGE_DROPPED_CONJUNCTS` (default ON). A class axiom dropped
//! because one conjunct cannot be converted keeps its NECESSARY-side consequences
//! for the conjuncts that do convert.
//!
//! The unconvertible conjunct here is `DataAllValuesFrom(:dp xsd:boolean)`
//! (`xsd:boolean` has no `DKey` bucket), the `ore_ont_14379` shape.
//!
//! **Direction of risk is a false positive**: keeping a conjunct on a LEFT side
//! would strengthen the axiom. The negatives pin that nothing is kept from a
//! subclass position or from the sufficient direction of an equivalence.

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

const BOOL: &str = "DataAllValuesFrom(:dp xsd:boolean)";

fn convert_unlocked(body: &str) -> InternalOntology {
    let src = format!(
        "Prefix(:=<http://ex#>)\nPrefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
         Ontology(\nDeclaration(DataProperty(:dp))\n\
         Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) Declaration(Class(:D))\n\
         {body}\n)"
    );
    let o: SetOntology<RcStr> = read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .unwrap()
        .0;
    owl_dl_core::convert::convert_ontology(&o).unwrap()
}

fn convert(body: &str) -> InternalOntology {
    let _g = ENV
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    convert_unlocked(body)
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

// ---------------------------------------------------------------- negatives

/// FP guard: an unconvertible conjunct on the LEFT keeps nothing.
/// `B ⊓ U ⊑ A` does not give `B ⊑ A`.
#[test]
fn nothing_is_kept_from_a_subclass_position() {
    let o = convert(&format!("SubClassOf(ObjectIntersectionOf(:B {BOOL}) :A)"));
    assert_eq!(o.dropped.total(), 1, "precondition: the axiom is dropped");
    assert!(!has_edge(&o, "B", "A"));
}

/// FP guard: the sufficient direction of an equivalence is not kept.
/// `A ≡ B ⊓ U` gives `A ⊑ B`, never `B ⊑ A`.
#[test]
fn the_sufficient_direction_of_an_equivalence_is_not_kept() {
    let o = convert(&format!(
        "EquivalentClasses(:A ObjectIntersectionOf(:B {BOOL}))"
    ));
    assert_eq!(o.dropped.total(), 1, "precondition: the axiom is dropped");
    assert!(!has_edge(&o, "B", "A"));
}

/// The axiom is still reported as dropped: part of it is still lost.
#[test]
fn a_salvaged_axiom_is_still_counted_as_dropped() {
    let o = convert(&format!("SubClassOf(:A ObjectIntersectionOf(:B {BOOL}))"));
    assert_eq!(o.dropped.total(), 1);
}

// ---------------------------------------------------------------- positives

#[test]
fn a_subclass_keeps_its_convertible_conjuncts() {
    let o = convert(&format!("SubClassOf(:A ObjectIntersectionOf(:B {BOOL}))"));
    assert!(has_edge(&o, "A", "B"));
}

/// The `ore_ont_14379` shape: `ObservationEvent ≡ Event ⊓ … ⊓ ∀p.xsd:boolean`.
#[test]
fn an_equivalence_keeps_its_necessary_conjuncts() {
    let o = convert(&format!(
        "EquivalentClasses(:A ObjectIntersectionOf(:B {BOOL}))"
    ));
    assert!(has_edge(&o, "A", "B"));
}

/// Nested intersections are flattened.
#[test]
fn nested_intersections_are_flattened() {
    let o = convert(&format!(
        "SubClassOf(:A ObjectIntersectionOf(:B ObjectIntersectionOf(:C {BOOL})))"
    ));
    assert!(has_edge(&o, "A", "B") && has_edge(&o, "A", "C"));
}

/// With three members, the two convertible ones stay equivalent and both get
/// the partial member's convertible conjunct.
#[test]
fn a_three_member_equivalence_keeps_the_rest() {
    let o = convert(&format!(
        "EquivalentClasses(:A :D ObjectIntersectionOf(:B {BOOL}))"
    ));
    assert!(has_edge(&o, "A", "B") && has_edge(&o, "D", "B"));
    assert!(
        o.axioms
            .iter()
            .any(|ax| matches!(ax, Axiom::EquivalentClasses(ids) if ids.len() == 2))
    );
}

/// `RUSTDL_SALVAGE_DROPPED_CONJUNCTS=0` drops the whole axiom as before.
#[test]
fn the_flag_restores_the_whole_drop() {
    let _g = ENV
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // SAFETY: every env access in this binary holds `ENV`.
    unsafe { std::env::set_var("RUSTDL_SALVAGE_DROPPED_CONJUNCTS", "0") };
    let o = convert_unlocked(&format!("SubClassOf(:A ObjectIntersectionOf(:B {BOOL}))"));
    // SAFETY: as above.
    unsafe { std::env::remove_var("RUSTDL_SALVAGE_DROPPED_CONJUNCTS") };
    assert!(!has_edge(&o, "A", "B"));
}
