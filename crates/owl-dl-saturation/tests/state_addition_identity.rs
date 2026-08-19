//! Retained-state addition must equal from-scratch saturation, at IRI level.
#![allow(clippy::unwrap_used)]
mod common;
use common::{closure_as_iri_pairs, load_fixture, load_fixture_pair};
use owl_dl_saturation::saturate;
use owl_dl_saturation::state::SaturationState;

#[test]
fn incremental_addition_equals_from_scratch() {
    // load_fixture_pair returns (base_internal, union_internal, added_axiom_indices)
    // where union == base + a handful of class axioms (see `synthesized_delta`).
    for fixture in ["sulo.ofn", "pizza.ofn", "mie.ofn"] {
        let (base, union, added) = load_fixture_pair(fixture);

        let mut st = SaturationState::build(&base, 64);
        let outcome = st.apply_additions(&union, &added);
        assert!(!outcome.rebuilt, "a pure addition must not force a rebuild");

        let incremental = closure_as_iri_pairs(&union, st.subsumers());
        let from_scratch = closure_as_iri_pairs(&union, &saturate(&union));
        assert_eq!(from_scratch, incremental, "fixture {fixture}");
    }
}

#[test]
fn addition_introducing_a_new_class_fits_in_slack() {
    let (base, union, added) = load_fixture_pair("sulo-new-class.ofn");
    let mut st = SaturationState::build(&base, 64);
    let outcome = st.apply_additions(&union, &added);
    assert!(
        !outcome.rebuilt,
        "a new named class must fit in slack, not force a rebuild"
    );
    assert_eq!(
        closure_as_iri_pairs(&union, &saturate(&union)),
        closure_as_iri_pairs(&union, st.subsumers())
    );
}

// ---------------------------------------------------------------------------
// Anti-vacuity + rebuild-routing coverage
// ---------------------------------------------------------------------------

/// Both identity tests above would pass trivially against a `SaturationState`
/// that ignored the delta entirely IF the delta changed nothing. It must
/// change something, on every fixture, or those tests prove nothing.
#[test]
fn every_delta_actually_changes_the_closure() {
    for fixture in ["sulo.ofn", "pizza.ofn", "mie.ofn", "sulo-new-class.ofn"] {
        let (base, union, _added) = load_fixture_pair(fixture);
        let before = closure_as_iri_pairs(&base, &saturate(&base));
        let after = closure_as_iri_pairs(&union, &saturate(&union));
        assert_ne!(
            before, after,
            "the {fixture} delta is a no-op — its identity test is vacuous"
        );
        assert!(
            after.len() > before.len(),
            "the {fixture} delta must ADD entailments (before {}, after {})",
            before.len(),
            after.len()
        );
    }
}

/// Rebuild trigger (a): the role hierarchy is frozen when the engine is built,
/// so any object-property axiom in the delta invalidates every context. ELK
/// does the same. The closure must still be right afterwards.
#[test]
fn an_object_property_axiom_forces_a_rebuild() {
    // `SymmetricObjectProperty` is deliberately in this list: the EL saturator
    // ignores it outright, so it moves neither `role_super` nor any structural
    // rule map. Only the axiom-kind check routes it to a rebuild — which is
    // what Spec §9 and ELK's own limitation require.
    for rbox in [
        "SubObjectPropertyOf(<https://w3id.org/sulo/hasPart> \
         <https://w3id.org/sulo/hasParticipant>)",
        "TransitiveObjectProperty(<https://w3id.org/sulo/hasPart>)",
        "SymmetricObjectProperty(<https://w3id.org/sulo/hasPart>)",
    ] {
        let base = load_fixture("sulo.ofn");
        let mut union = base.clone();
        let added = common::apply_delta_ofn(
            &mut union,
            &format!("Ontology(<http://rustdl.test/rbox>\n{rbox}\n)\n"),
        );
        assert!(!added.is_empty(), "{rbox} lowered to no axiom");
        let mut st = SaturationState::build(&base, 64);
        let outcome = st.apply_additions(&union, &added);
        assert!(outcome.rebuilt, "{rbox} must force a rebuild");
        assert_eq!(
            closure_as_iri_pairs(&union, &saturate(&union)),
            closure_as_iri_pairs(&union, st.subsumers()),
            "{rbox}"
        );
    }
}

/// Rebuild trigger (b): with no slack there is no room above the user
/// vocabulary, so a delta that interns a new class must rebuild — and the
/// rebuild must widen the slack so the NEXT addition of the same shape fits.
#[test]
fn slack_exhaustion_forces_a_rebuild_then_widens() {
    let (base, union, added) = load_fixture_pair("sulo-new-class.ofn");
    let mut st = SaturationState::build(&base, 0);
    let outcome = st.apply_additions(&union, &added);
    assert!(
        outcome.rebuilt,
        "new classes with slack 0 must force a rebuild"
    );
    assert_eq!(
        closure_as_iri_pairs(&union, &saturate(&union)),
        closure_as_iri_pairs(&union, st.subsumers())
    );
    assert!(
        st.slack() > 0,
        "the rebuild must widen the slack, not keep it at 0"
    );
}

/// Two revisions in a row on one retained engine: the second addition must
/// reuse the state the first one left behind and still land on the from-scratch
/// closure. This is the generation-composition property — the first addition's
/// synthetics and memoized Tseitin bodies have to survive into the second.
#[test]
fn two_consecutive_additions_equal_from_scratch() {
    let base = load_fixture("sulo.ofn");
    let mut rev1 = base.clone();
    let added1 = common::apply_delta_ofn(
        &mut rev1,
        "Ontology(<http://rustdl.test/d1>\n\
         SubClassOf(<http://rustdl.test/new#Widget> <https://w3id.org/sulo/Object>)\n\
         )\n",
    );
    let mut rev2 = rev1.clone();
    let added2 = common::apply_delta_ofn(
        &mut rev2,
        "Ontology(<http://rustdl.test/d2>\n\
         SubClassOf(<http://rustdl.test/new#Gadget> ObjectIntersectionOf(\
         <http://rustdl.test/new#Widget> \
         ObjectSomeValuesFrom(<https://w3id.org/sulo/hasPart> \
         <https://w3id.org/sulo/Quality>)))\n\
         )\n",
    );

    let mut st = SaturationState::build(&base, 64);
    assert!(!st.apply_additions(&rev1, &added1).rebuilt);
    assert!(!st.apply_additions(&rev2, &added2).rebuilt);
    assert_eq!(
        closure_as_iri_pairs(&rev2, &saturate(&rev2)),
        closure_as_iri_pairs(&rev2, st.subsumers())
    );
}

// ---------------------------------------------------------------------------
// Retrigger coverage
//
// The real fixtures above happen NOT to exercise three of the retrigger paths:
// their deltas' conjunctive trigger heads a fresh Tseitin synthetic (invisible
// in the user projection), and their existential trigger matches no
// pre-existing fact. Removing either retrigger left the suite green. These
// hand-built pairs close that hole — each one fails if its retrigger is
// dropped. The mutation transitions are recorded in the task report.
// ---------------------------------------------------------------------------

/// Base closure the deltas below fire against:
/// `X ⊑ P`, `X ⊑ Q`, `T ⊑ U`, and the existential fact `(A, r, T)`.
const RETRIGGER_BASE: &str = "Prefix(:=<http://rustdl.test/rt#>)\n\
     Ontology(<http://rustdl.test/rt>\n\
     Declaration(Class(:A)) Declaration(Class(:M)) Declaration(Class(:N))\n\
     Declaration(Class(:P)) Declaration(Class(:Q)) Declaration(Class(:T))\n\
     Declaration(Class(:U)) Declaration(Class(:V)) Declaration(Class(:W))\n\
     Declaration(Class(:X)) Declaration(Class(:Z))\n\
     Declaration(ObjectProperty(:r))\n\
     SubClassOf(:X :P)\n\
     SubClassOf(:X :Q)\n\
     SubClassOf(:T :U)\n\
     SubClassOf(:A ObjectSomeValuesFrom(:r :T))\n\
     )\n";

fn retrigger_case(delta: &str) -> (owl_dl_core::InternalOntology, SaturationState) {
    let base = common::load_ofn_str(RETRIGGER_BASE);
    let mut union = base.clone();
    let added = common::apply_delta_ofn(&mut union, delta);
    let mut st = SaturationState::build(&base, 64);
    let outcome = st.apply_additions(&union, &added);
    assert!(!outcome.rebuilt, "class-only delta must not rebuild");
    (union, st)
}

fn assert_entails(
    union: &owl_dl_core::InternalOntology,
    st: &SaturationState,
    sub: &str,
    sup: &str,
) {
    let pairs = closure_as_iri_pairs(union, st.subsumers());
    let want = (
        format!("http://rustdl.test/rt#{sub}"),
        format!("http://rustdl.test/rt#{sup}"),
    );
    assert!(
        pairs.contains(&want),
        "resumed engine lost {sub} ⊑ {sup}; from-scratch has it"
    );
    assert_eq!(
        closure_as_iri_pairs(union, &saturate(union)),
        pairs,
        "resumed closure diverged from from-scratch"
    );
}

/// A new conjunctive trigger `P ⊓ Q ⊑ Z` must fire on `X`, which already had
/// both bodies before the delta and gains no other new subsumer — so nothing
/// re-enqueues `X` on its own.
#[test]
fn new_conjunctive_trigger_fires_on_already_derived_subsumers() {
    let (union, st) = retrigger_case(
        "Prefix(:=<http://rustdl.test/rt#>)\n\
         Ontology(<http://rustdl.test/d>\n\
         SubClassOf(ObjectIntersectionOf(:P :Q) :Z)\n\
         )\n",
    );
    assert_entails(&union, &st, "X", "Z");
}

/// A new existential trigger `∃r.U ⊑ W` must fire on the pre-existing fact
/// `(A, r, T)` with `T ⊑ U` — a fact derived in the previous revision and long
/// since drained off the worklist.
#[test]
fn new_existential_trigger_fires_on_an_already_derived_fact() {
    let (union, st) = retrigger_case(
        "Prefix(:=<http://rustdl.test/rt#>)\n\
         Ontology(<http://rustdl.test/d>\n\
         SubClassOf(ObjectSomeValuesFrom(:r :U) :W)\n\
         )\n",
    );
    assert_entails(&union, &st, "A", "W");
}

/// A delta that introduces a Tseitin synthetic `F ≡ M ⊓ N` on BOTH sides needs
/// `F ⊑ F` seeded, exactly as `WorklistEngine::seed` does for the static
/// synthetics — otherwise `F ∉ subsumers(F)` and the `∃r.F ⊑ V` trigger never
/// matches the `(A, r, F)` fact.
#[test]
fn a_new_tseitin_synthetic_gets_its_reflexive_row() {
    let (union, st) = retrigger_case(
        "Prefix(:=<http://rustdl.test/rt#>)\n\
         Ontology(<http://rustdl.test/d>\n\
         SubClassOf(:A ObjectSomeValuesFrom(:r ObjectIntersectionOf(:M :N)))\n\
         SubClassOf(ObjectSomeValuesFrom(:r ObjectIntersectionOf(:M :N)) :V)\n\
         )\n",
    );
    assert_entails(&union, &st, "A", "V");
}

/// A new `DisjointClasses(P, Q)` must clash on `X`, which already carries both.
#[test]
fn a_new_disjoint_pair_clashes_on_an_already_derived_class() {
    let (union, st) = retrigger_case(
        "Prefix(:=<http://rustdl.test/rt#>)\n\
         Ontology(<http://rustdl.test/d>\n\
         DisjointClasses(:P :Q)\n\
         )\n",
    );
    let x = union
        .vocabulary
        .class_id("http://rustdl.test/rt#X")
        .unwrap();
    assert!(
        st.subsumers().is_unsatisfiable(x),
        "X ⊑ P ⊓ Q with P,Q disjoint must be unsat"
    );
    assert_eq!(
        closure_as_iri_pairs(&union, &saturate(&union)),
        closure_as_iri_pairs(&union, st.subsumers())
    );
}

/// A new told `U ⊑ ⊥` must propagate to `T ⊑ U` and on to `A`, whose only
/// `r`-witness is a `T`.
#[test]
fn a_new_directly_unsat_rule_propagates_to_existing_subclasses() {
    let (union, st) = retrigger_case(
        "Prefix(:=<http://rustdl.test/rt#>)\n\
         Ontology(<http://rustdl.test/d>\n\
         SubClassOf(:U <http://www.w3.org/2002/07/owl#Nothing>)\n\
         )\n",
    );
    for name in ["U", "T", "A"] {
        let c = union
            .vocabulary
            .class_id(&format!("http://rustdl.test/rt#{name}"))
            .unwrap();
        assert!(
            st.subsumers().is_unsatisfiable(c),
            "{name} must be unsatisfiable"
        );
    }
    assert_eq!(
        closure_as_iri_pairs(&union, &saturate(&union)),
        closure_as_iri_pairs(&union, st.subsumers())
    );
}

/// `⊤ ⊑ C` is broadcast to every named class at seed time. A class interned by
/// the delta was not there to receive it, so `apply_additions` has to replay
/// the broadcast for the new ids.
#[test]
fn a_class_added_by_the_delta_receives_the_top_broadcast() {
    let base = common::load_ofn_str(
        "Prefix(:=<http://rustdl.test/tp#>)\n\
         Ontology(<http://rustdl.test/tp>\n\
         Declaration(Class(:Everything))\n\
         Declaration(Class(:Seed))\n\
         SubClassOf(<http://www.w3.org/2002/07/owl#Thing> :Everything)\n\
         )\n",
    );
    let mut union = base.clone();
    let added = common::apply_delta_ofn(
        &mut union,
        "Prefix(:=<http://rustdl.test/tp#>)\n\
         Ontology(<http://rustdl.test/d>\n\
         Declaration(Class(:Latecomer))\n\
         )\n",
    );
    let mut st = SaturationState::build(&base, 64);
    assert!(!st.apply_additions(&union, &added).rebuilt);
    let pairs = closure_as_iri_pairs(&union, st.subsumers());
    assert!(
        pairs.contains(&(
            "http://rustdl.test/tp#Latecomer".to_string(),
            "http://rustdl.test/tp#Everything".to_string(),
        )),
        "a class added after the `⊤ ⊑ Everything` broadcast must still get it"
    );
    assert_eq!(closure_as_iri_pairs(&union, &saturate(&union)), pairs);
}

/// Rebuild trigger (c): the compatibility gate. `nominal_to_ind`,
/// `role_ranges`, `functional_roles`, … are read directly by the worklist
/// rules and have no incremental re-trigger path, so a delta that grows one
/// must rebuild rather than splice. A delta introducing the ontology's first
/// nominal filler grows `nominal_to_ind`.
#[test]
fn a_delta_that_grows_a_structural_rule_map_forces_a_rebuild() {
    let base = common::load_ofn_str(
        "Prefix(:=<http://rustdl.test/nm#>)\n\
         Ontology(<http://rustdl.test/nm>\n\
         Declaration(Class(:A)) Declaration(Class(:V))\n\
         Declaration(ObjectProperty(:r))\n\
         Declaration(NamedIndividual(:ind))\n\
         SubClassOf(:A <http://www.w3.org/2002/07/owl#Thing>)\n\
         )\n",
    );
    let mut union = base.clone();
    let added = common::apply_delta_ofn(
        &mut union,
        "Prefix(:=<http://rustdl.test/nm#>)\n\
         Ontology(<http://rustdl.test/d>\n\
         SubClassOf(:A ObjectSomeValuesFrom(:r ObjectOneOf(:ind)))\n\
         SubClassOf(ObjectSomeValuesFrom(:r ObjectOneOf(:ind)) :V)\n\
         )\n",
    );
    let mut st = SaturationState::build(&base, 64);
    assert!(
        st.apply_additions(&union, &added).rebuilt,
        "a delta that grows `nominal_to_ind` must rebuild, not splice"
    );
    let pairs = closure_as_iri_pairs(&union, st.subsumers());
    assert_eq!(closure_as_iri_pairs(&union, &saturate(&union)), pairs);
    assert!(
        pairs.contains(&(
            "http://rustdl.test/nm#A".to_string(),
            "http://rustdl.test/nm#V".to_string(),
        )),
        "the rebuild must still land on the full closure"
    );
}

/// The base saturation of a functional role mints RUNTIME synthetics above the
/// static Tseitin universe. Two things depend on that here:
///
/// * the delta's own Tseitin synthetics must be allocated ABOVE them (the
///   allocator is seeded from the engine's live one), or a fresh static
///   synthetic aliases a live merged witness; and
/// * `atomic_content_of` must be registered for the delta's synthetics, or the
///   Phase-2a witness merge flattens `F` to `{F}` instead of `{M, N}`.
#[test]
fn a_delta_over_a_functional_role_keeps_runtime_synthetics_distinct() {
    let base = common::load_ofn_str(
        "Prefix(:=<http://rustdl.test/fn#>)\n\
         Ontology(<http://rustdl.test/fn>\n\
         Declaration(ObjectProperty(:f))\n\
         FunctionalObjectProperty(:f)\n\
         Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))\n\
         Declaration(Class(:D)) Declaration(Class(:M)) Declaration(Class(:N))\n\
         Declaration(Class(:V)) Declaration(Class(:W))\n\
         SubClassOf(:A ObjectSomeValuesFrom(:f :B))\n\
         SubClassOf(:A ObjectSomeValuesFrom(:f :C))\n\
         SubClassOf(ObjectIntersectionOf(:B :C) :D)\n\
         SubClassOf(ObjectSomeValuesFrom(:f :D) :W)\n\
         )\n",
    );
    let mut union = base.clone();
    let added = common::apply_delta_ofn(
        &mut union,
        "Prefix(:=<http://rustdl.test/fn#>)\n\
         Ontology(<http://rustdl.test/d>\n\
         SubClassOf(:A ObjectSomeValuesFrom(:f ObjectIntersectionOf(:M :N)))\n\
         SubClassOf(ObjectSomeValuesFrom(:f ObjectIntersectionOf(:M :N)) :V)\n\
         )\n",
    );
    let mut st = SaturationState::build(&base, 64);
    assert!(!st.apply_additions(&union, &added).rebuilt);
    assert_eq!(
        closure_as_iri_pairs(&union, &saturate(&union)),
        closure_as_iri_pairs(&union, st.subsumers())
    );
}

/// The mirror of the test above: a `⊤ ⊑ C` axiom in the DELTA must reach the
/// classes that were already there when the engine was seeded. Nothing else
/// re-enqueues them — `Existing` gains no other subsumer.
#[test]
fn a_top_subsumer_added_by_the_delta_reaches_the_existing_classes() {
    let base = common::load_ofn_str(
        "Prefix(:=<http://rustdl.test/tp2#>)\n\
         Ontology(<http://rustdl.test/tp2>\n\
         Declaration(Class(:Existing))\n\
         Declaration(Class(:Everything))\n\
         )\n",
    );
    let mut union = base.clone();
    let added = common::apply_delta_ofn(
        &mut union,
        "Prefix(:=<http://rustdl.test/tp2#>)\n\
         Ontology(<http://rustdl.test/d>\n\
         SubClassOf(<http://www.w3.org/2002/07/owl#Thing> :Everything)\n\
         )\n",
    );
    let mut st = SaturationState::build(&base, 64);
    assert!(!st.apply_additions(&union, &added).rebuilt);
    let pairs = closure_as_iri_pairs(&union, st.subsumers());
    assert!(
        pairs.contains(&(
            "http://rustdl.test/tp2#Existing".to_string(),
            "http://rustdl.test/tp2#Everything".to_string(),
        )),
        "a `⊤ ⊑ Everything` in the delta must broadcast to classes seeded earlier"
    );
    assert_eq!(closure_as_iri_pairs(&union, &saturate(&union)), pairs);
}
