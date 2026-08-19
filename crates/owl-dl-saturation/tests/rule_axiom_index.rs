//! Always-on rule→axiom provenance: every compiled EL rule carries the index
//! of the source axiom it was lowered from, or `u32::MAX` when it has no single
//! source axiom (Tseitin/synthetic definitional clauses).
//!
//! The index must be populated *without* proof recording — a later incremental
//! phase uses it to drop a deleted axiom's compiled rules, and `ProofTrace`
//! (the only prior source of this mapping) is too expensive at SNOMED scale.
#![allow(clippy::unwrap_used)]
mod common;
use common::load_fixture;

#[test]
fn every_compiled_rule_maps_to_a_live_source_axiom_or_the_synthetic_sentinel() {
    let internal = load_fixture("pizza.ofn");
    let rules = owl_dl_saturation::collect_el_rules_for_test(&internal);

    assert_eq!(
        rules.atomic_subsumptions.len(),
        rules.axiom_of_atomic_sub.len()
    );
    assert_eq!(
        rules.conjunctive_triggers.len(),
        rules.axiom_of_conjunctive_trigger.len()
    );
    assert_eq!(
        rules.existential_facts.len(),
        rules.axiom_of_existential_fact.len()
    );
    assert_eq!(
        rules.existential_triggers.len(),
        rules.axiom_of_existential_trigger.len()
    );
    assert_eq!(
        rules.disjoint_pairs.len(),
        rules.axiom_of_disjoint_pair.len()
    );

    for &a in &rules.axiom_of_atomic_sub {
        if a != u32::MAX {
            let idx = a as usize;
            assert!(idx < internal.axioms.len(), "axiom index out of range");
            assert!(internal.live.contains(idx), "rule points at a dead axiom");
        }
    }
}

#[test]
fn index_is_populated_without_proof_recording() {
    // The whole point: this must NOT require RUSTDL_PROOF=1.
    assert!(std::env::var("RUSTDL_PROOF").is_err());
    let internal = load_fixture("sulo.ofn");
    let rules = owl_dl_saturation::collect_el_rules_for_test(&internal);
    assert!(
        rules.axiom_of_atomic_sub.iter().any(|&a| a != u32::MAX),
        "at least one rule must carry real axiom provenance"
    );
}
