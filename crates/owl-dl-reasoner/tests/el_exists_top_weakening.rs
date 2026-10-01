//! Canaries for the RHS-existential ⊤-weakening in the EL saturator (#180).
//!
//! `X ⊑ ∃R.body` entails `X ⊑ ∃R.⊤` for EVERY body, so a body the EL lowering
//! cannot represent (a union filler, in these fixtures) no longer drops the
//! whole fact: the subject still gets an `ExistentialFact` to the per-role
//! ⊤-witness, and `DomainSub` — which walks `role_super` — fires the domains
//! of the role AND its told supers on the subject. This is the COSMO shape
//! behind issue #180 (`CoveringSomething ⊑ ∃hadObjectCovered.(A ⊔ B)`,
//! `hadObjectCovered ⊑ hadObjectTouched`, `Domain(hadObjectTouched) =
//! Touching`), measured on the real ontologies as MISSED 130 → 2
//! (`ore_ont_16420`) and 82 → 2 (`ore_ont_9577`) with FP=0.
//!
//! **The direction-of-risk guards are the point of this file.** The weakening
//! is sound only as a one-directional FACT: the guards pin that no trigger
//! fires the ORIGINAL (stronger) head off the weakened premise, that ranges
//! do not land on the source, and that a provably-⊥ filler keeps its
//! emptiness instead of being weakened into satisfiability.

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

const PREFIX: &str =
    "Prefix(:=<urn:tw#>) Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\nOntology(<urn:tw>\n";

fn classify(body: &str) -> owl_dl_reasoner::Classification {
    let text = format!("{PREFIX}{body}\n)");
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut Cursor::new(text), ParserConfiguration::default()).expect("parse ofn");
    owl_dl_reasoner::classify(&onto).expect("classify")
}

/// Saturation-ONLY classify: the wedge answers these small fixtures on the
/// default hybrid path regardless of the fold (the #145 shape — protection by
/// another component), so the positives must interrogate the saturator alone
/// or they are vacuous for this change. Verified by sabotage: with the
/// fallback reverted, the saturation-only positives fail while the default
/// path still answers.
fn classify_sat_only(body: &str) -> owl_dl_reasoner::Classification {
    let text = format!("{PREFIX}{body}\n)");
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut Cursor::new(text), ParserConfiguration::default()).expect("parse ofn");
    owl_dl_reasoner::classify_saturation_only(&onto).expect("classify saturation-only")
}

fn has_sub(c: &owl_dl_reasoner::Classification, sub: &str, sup: &str) -> bool {
    c.is_subclass(&format!("urn:tw#{sub}"), &format!("urn:tw#{sup}"))
}

/// The COSMO shape: a union filler used to drop the whole existential, losing
/// the super-role domain. Konclude/HermiT semantics: `A ⊑ ∃r.(B ⊔ C) ⊑ ∃r.⊤ ⊑
/// Domain(s)` since `r ⊑ s`.
#[test]
fn union_filler_still_fires_the_super_role_domain() {
    let c = classify_sat_only(
        "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))\n\
         Declaration(Class(:D)) Declaration(Class(:E))\n\
         Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))\n\
         SubClassOf(:A ObjectSomeValuesFrom(:r ObjectUnionOf(:B :C)))\n\
         SubObjectPropertyOf(:r :s)\n\
         ObjectPropertyDomain(:s :D)\n\
         SubClassOf(:D :E)",
    );
    assert!(has_sub(&c, "A", "D"), "super-role domain must fire on A");
    assert!(
        has_sub(&c, "A", "E"),
        "and propagate up the class hierarchy"
    );
}

/// The role's OWN domain fires too (reflexive closure), not just supers.
#[test]
fn union_filler_fires_the_roles_own_domain() {
    let c = classify_sat_only(
        "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))\n\
         Declaration(Class(:D))\n\
         Declaration(ObjectProperty(:r))\n\
         SubClassOf(:A ObjectSomeValuesFrom(:r ObjectUnionOf(:B :C)))\n\
         ObjectPropertyDomain(:r :D)",
    );
    assert!(has_sub(&c, "A", "D"));
}

/// `≥1 r.(B ⊔ C)` implies `∃r.⊤` exactly as `∃` does; the Min arm shares the
/// fallback.
#[test]
fn min_cardinality_union_filler_fires_the_domain() {
    let c = classify_sat_only(
        "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))\n\
         Declaration(Class(:D))\n\
         Declaration(ObjectProperty(:r))\n\
         SubClassOf(:A ObjectMinCardinality(1 :r ObjectUnionOf(:B :C)))\n\
         ObjectPropertyDomain(:r :D)",
    );
    assert!(has_sub(&c, "A", "D"));
}

/// A defined class `Q ≡ ∃r.owl:Thing` is a GENUINE consequence of the
/// weakened fact — any r-successor at all makes A a Q. Konclude agrees.
#[test]
fn defined_exists_top_is_a_genuine_consequence() {
    let c = classify_sat_only(
        "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))\n\
         Declaration(Class(:Q))\n\
         Declaration(ObjectProperty(:r))\n\
         SubClassOf(:A ObjectSomeValuesFrom(:r ObjectUnionOf(:B :C)))\n\
         EquivalentClasses(:Q ObjectSomeValuesFrom(:r owl:Thing))",
    );
    assert!(has_sub(&c, "A", "Q"), "A has an r-successor, so A is a Q");
}

/// FP GUARD (the two-way-leak direction): a defined class over an ATOMIC
/// filler must NOT fire off the weakened fact — `∃r.(B ⊔ C)` does not entail
/// `∃r.K`. If this fails, the ⊤-witness leaked into a trigger keyed on a
/// stronger body.
#[test]
fn fp_guard_defined_exists_atomic_does_not_fire() {
    let c = classify(
        "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))\n\
         Declaration(Class(:K)) Declaration(Class(:Q))\n\
         Declaration(ObjectProperty(:r))\n\
         SubClassOf(:A ObjectSomeValuesFrom(:r ObjectUnionOf(:B :C)))\n\
         EquivalentClasses(:Q ObjectSomeValuesFrom(:r :K))",
    );
    assert!(
        !has_sub(&c, "A", "Q"),
        "FALSE POSITIVE: ∃r.(B ⊔ C) must not satisfy the ∃r.K definition"
    );
}

/// FP GUARD (the polarity direction): `Range(s)` constrains r's TARGETS, and
/// the weakened fact must not put it on the SOURCE. If this fails, a
/// range/domain table got crossed — the #125 defect shape.
#[test]
fn fp_guard_super_role_range_does_not_land_on_the_source() {
    let c = classify(
        "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))\n\
         Declaration(Class(:G))\n\
         Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))\n\
         SubClassOf(:A ObjectSomeValuesFrom(:r ObjectUnionOf(:B :C)))\n\
         SubObjectPropertyOf(:r :s)\n\
         ObjectPropertyRange(:s :G)",
    );
    assert!(
        !has_sub(&c, "A", "G"),
        "FALSE POSITIVE: Range(s) types r-successors, never the source A"
    );
}

/// FP GUARD (the inverse direction): `X ⊑ ∃r⁻.(B ⊔ C)` gives X an
/// r-PREDECESSOR, so `Domain(r)` types that predecessor, never X (X gets
/// `Range(r)`, which the saturator does not model here — a sound miss). If
/// this fails, the fallback treated an inverse existential as a forward fact
/// — the mutation that removes its `is_inverse` refusal.
#[test]
fn fp_guard_inverse_existential_is_not_weakened_forward() {
    let c = classify(
        "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))\n\
         Declaration(Class(:D))\n\
         Declaration(ObjectProperty(:r))\n\
         SubClassOf(:A ObjectSomeValuesFrom(ObjectInverseOf(:r) ObjectUnionOf(:B :C)))\n\
         ObjectPropertyDomain(:r :D)",
    );
    assert!(
        !has_sub(&c, "A", "D"),
        "FALSE POSITIVE: Domain(r) types r-SOURCES; A is an r-target here"
    );
}

/// FP GUARD (no successor at all): the domain must fire only on classes that
/// actually HAVE the existential — a sibling class with no r-edge stays out.
#[test]
fn fp_guard_domain_needs_the_existential() {
    let c = classify(
        "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))\n\
         Declaration(Class(:D)) Declaration(Class(:Z))\n\
         Declaration(ObjectProperty(:r))\n\
         SubClassOf(:A ObjectSomeValuesFrom(:r ObjectUnionOf(:B :C)))\n\
         ObjectPropertyDomain(:r :D)\n\
         SubClassOf(:Z :B)",
    );
    assert!(!has_sub(&c, "Z", "D"), "Z has no r-successor");
}

/// A provably-⊥ filler keeps its emptiness: `A ⊑ ∃r.⊥` makes A unsatisfiable
/// (the `RUSTDL_EL_BOT_FILLER` path), and the weakening must not pre-empt it
/// into a satisfiable ⊤-fact.
#[test]
fn bot_filler_is_not_weakened_into_satisfiability() {
    let c = classify(
        "Declaration(Class(:A))\n\
         Declaration(ObjectProperty(:r))\n\
         SubClassOf(:A ObjectSomeValuesFrom(:r owl:Nothing))",
    );
    assert!(
        c.unsatisfiable_classes().contains(&"urn:tw#A"),
        "A ⊑ ∃r.⊥ must stay unsatisfiable"
    );
}

/// KNOWN SATURATION RESIDUAL, pinned at the right surface: the And-LHS arm
/// (`(X ⊓ Y) ⊑ ∃r.<union>`) still drops the body IN THE SATURATOR, because
/// its lowering mints a TWO-WAY marker where the ⊤-weakening would be unsound
/// (the trigger direction would assert the stronger head off the weakened
/// premise). A future fix should route it through a ONE-WAY marker and then
/// FLIP the saturation-only assertion rather than delete it.
///
/// The DEFAULT pipeline already answers this shape — the wedge clausifies the
/// union natively — so the end-to-end assertion is a positive control, and
/// the residual is observable only saturation-only. (First draft pinned the
/// miss at the classify level and was refuted by its own run: the #145
/// protection-by-another-component shape.)
#[test]
fn and_lhs_union_filler_saturation_residual_but_wedge_covers() {
    const BODY: &str = "Declaration(Class(:X)) Declaration(Class(:Y)) Declaration(Class(:W))\n\
         Declaration(Class(:B)) Declaration(Class(:C)) Declaration(Class(:D))\n\
         Declaration(ObjectProperty(:r))\n\
         SubClassOf(ObjectIntersectionOf(:X :Y) ObjectSomeValuesFrom(:r ObjectUnionOf(:B :C)))\n\
         ObjectPropertyDomain(:r :D)\n\
         SubClassOf(:W :X) SubClassOf(:W :Y)";
    let full = classify(BODY);
    assert!(
        has_sub(&full, "W", "D"),
        "the hybrid path (wedge) must keep covering the And-LHS shape"
    );
    let sat = classify_sat_only(BODY);
    assert!(
        !has_sub(&sat, "W", "D"),
        "the saturator now derives the And-LHS shape — FLIP this assertion, per the doc"
    );
}
