//! #110 — a conjunctive `ObjectPropertyDomain` / `ObjectPropertyRange` filler must
//! not be dropped.
//!
//! `ObjectPropertyDomain(r, P ⊓ Q)` is `∃r.⊤ ⊑ P ⊓ Q`, which is exactly
//! `∃r.⊤ ⊑ P` and `∃r.⊤ ⊑ Q` — a logical identity, so splitting it is sound and
//! completeness-preserving by construction. The same holds on the range side.
//!
//! Before this, the saturator accepted only an ATOMIC filler and silently dropped a
//! conjunctive one: `classify` returned zero rows with **`incomplete: false`** and
//! `dropped: {}`, under a banner certifying the fragment complete — the D10 shape,
//! where the answer is wrong *and* reported complete. Both peers derive the pairs
//! (`Konclude` v0.7.0-1138 and `HermiT` 1.4.3, measured), and rustdl's own
//! `subclass` proved them, so only `classify` was affected.
//!
//! The atomic controls are what show the cause is the CONJUNCTIVE filler rather
//! than the domain/range mechanism: they derived their pair correctly throughout.
//!
//! #119 — a PARTLY-atomic filler must contribute the atomic conjuncts it CAN
//! represent, not drop whole. `#110` shipped all-or-nothing on purpose (a filler
//! mixing atomic and non-atomic parts, e.g. `P ⊓ ∃s.Z`, dropped WHOLE); that was
//! itself the residual bug — `Domain(r, P ⊓ Q)` where `Q` happens to be
//! non-atomic (`P ⊓ ∃s.Z`) still entails `∃r.⊤ ⊑ P`, a WEAKER (strictly more
//! permissive) constraint than the full axiom, so contributing `P` alone can
//! only MISS a subsumption that needs the dropped part, never assert a false
//! one. Partial DECOMPOSITION is sound; partial ADMISSION to the fragment gate
//! is not, so the gate (`analyze_fragment`) must keep refusing a partly-atomic
//! filler even though the engine now derives part of it — pinned below so a
//! future widening of the gate cannot silently admit an axiom the engine only
//! partly processed.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::classify_top_down_with_timeout;
use owl_dl_reasoner::{FragmentClassification, analyze_fragment};
use std::io::Cursor;
use std::time::Duration;

fn holds(body: &str, sub: &str, sup: &str) -> bool {
    let ofn = format!(
        "Prefix(:=<http://rustdl.test/>)\nOntology(\n\
         Declaration(Class(:P)) Declaration(Class(:Q)) Declaration(Class(:X))\n\
         Declaration(Class(:B)) Declaration(Class(:D)) Declaration(Class(:Z))\n\
         Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))\n\
         {body}\n)\n"
    );
    let mut reader = Cursor::new(ofn);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    let result = classify_top_down_with_timeout(&onto, Duration::from_secs(10)).expect("classify");
    result.is_subclass(
        &format!("http://rustdl.test/{sub}"),
        &format!("http://rustdl.test/{sup}"),
    )
}

const EXISTS: &str = "SubClassOf(:X ObjectSomeValuesFrom(:r :B))";

#[test]
fn a_conjunctive_domain_yields_every_conjunct() {
    let body = format!("{EXISTS}\nObjectPropertyDomain(:r ObjectIntersectionOf(:P :Q))");
    assert!(holds(&body, "X", "P"), "X ⊑ P (Konclude and HermiT agree)");
    assert!(holds(&body, "X", "Q"), "X ⊑ Q (Konclude and HermiT agree)");
}

#[test]
fn an_atomic_domain_still_works() {
    // The discriminating control: this derived correctly even before #110, which is
    // what isolates the conjunctive filler as the cause.
    let body = format!("{EXISTS}\nObjectPropertyDomain(:r :P)");
    assert!(holds(&body, "X", "P"));
}

#[test]
fn a_conjunctive_range_yields_every_conjunct() {
    // Measured to have the identical defect: `HermiT` derives `X ⊑ D`, rustdl
    // reported `[]` with `incomplete: false`.
    let body = format!(
        "{EXISTS}\nObjectPropertyRange(:r ObjectIntersectionOf(:P :Q))\n\
         SubClassOf(ObjectSomeValuesFrom(:r ObjectIntersectionOf(:B :P)) :D)"
    );
    assert!(holds(&body, "X", "D"), "X ⊑ D (HermiT agrees)");
}

#[test]
fn an_atomic_range_still_works() {
    let body = format!(
        "{EXISTS}\nObjectPropertyRange(:r :P)\n\
         SubClassOf(ObjectSomeValuesFrom(:r ObjectIntersectionOf(:B :P)) :D)"
    );
    assert!(holds(&body, "X", "D"));
}

#[test]
fn a_nested_conjunction_is_flattened() {
    // `P ⊓ (Q ⊓ Z)` — the recursion, not just one level.
    let body = format!(
        "{EXISTS}\n\
         ObjectPropertyDomain(:r ObjectIntersectionOf(:P ObjectIntersectionOf(:Q :Z)))"
    );
    assert!(holds(&body, "X", "P"));
    assert!(holds(&body, "X", "Q"));
    assert!(holds(&body, "X", "Z"));
}

#[test]
fn a_filler_mixing_atomic_and_non_atomic_parts_contributes_its_atomic_conjuncts() {
    // #119: PARTIAL decomposition, not all-or-nothing. This FLIPS the pre-#119
    // assertion (`!holds`) — see [[tests-that-pin-the-bug]]: a fixture chosen to
    // characterise a defect becomes a bug-pin once the defect closes, and this
    // one was explicitly flagged for exactly that in #110's own comment ("If a
    // future change teaches the gate about conjunctive fillers, this becomes the
    // wrong assertion and should be FLIPPED rather than deleted" — the engine
    // side changed, not the gate, but the effect on this fixture is the same).
    //
    // `Domain(r, P ⊓ ∃s.Z)` means `∃r.⊤ ⊑ P` AND `∃r.⊤ ⊑ ∃s.Z`; contributing only
    // the atomic `P` conjunct asserts a WEAKER (more permissive) constraint than
    // the full axiom, so it can only MISS a subsumption that needs the dropped
    // `∃s.Z` half, never assert a false one — the opposite direction of risk
    // from a `DataUnionOf` in a `∀`/range position, where keeping half narrows
    // the range and manufactures a clash.
    let body = format!(
        "{EXISTS}\n\
         ObjectPropertyDomain(:r ObjectIntersectionOf(:P ObjectSomeValuesFrom(:s :Z)))"
    );
    assert!(
        holds(&body, "X", "P"),
        "a partly-atomic filler must contribute the atomic conjuncts it CAN represent (P)"
    );
}

#[test]
fn a_partly_atomic_domain_filler_is_not_admitted_to_the_pure_el_fragment() {
    // #119's load-bearing constraint: partial DECOMPOSITION in the engine is
    // sound, but partial ADMISSION to the fragment gate is NOT — a conjunct the
    // engine never processed, inside a completeness-certified fragment, is
    // exactly the D10 bug class #110 exists to close. `is_atomic_or_trivial_concept`
    // (the gate `classify.rs` uses for `ObjectPropertyDomain`/`Range` fillers)
    // must keep refusing this axiom outright even though the engine now derives
    // PART of it, so a future widening of the gate cannot silently admit an
    // axiom the engine only partly processed.
    let ofn = format!(
        "Prefix(:=<http://rustdl.test/>)\nOntology(\n\
         Declaration(Class(:P)) Declaration(Class(:Q)) Declaration(Class(:X))\n\
         Declaration(Class(:B)) Declaration(Class(:D)) Declaration(Class(:Z))\n\
         Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))\n\
         {EXISTS}\n\
         ObjectPropertyDomain(:r ObjectIntersectionOf(:P ObjectSomeValuesFrom(:s :Z)))\n)\n"
    );
    let mut reader = Cursor::new(ofn);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    let internal = owl_dl_core::convert::convert_ontology(&onto).expect("convert");
    assert_ne!(
        analyze_fragment(&internal),
        FragmentClassification::PureEl,
        "a partly-atomic domain filler must NOT be certified PureEl — the engine only \
         partly processes it, so the gate must keep routing it to the sound+complete \
         hybrid path"
    );
}
