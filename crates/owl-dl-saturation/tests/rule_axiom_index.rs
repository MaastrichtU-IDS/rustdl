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
fn every_compiled_rule_maps_to_a_source_axiom_or_the_synthetic_sentinel() {
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
    assert_eq!(
        rules.directly_unsat.len(),
        rules.axiom_of_directly_unsat.len()
    );

    // NOTE there is deliberately no `internal.live.contains(idx)` here. It read
    // as a liveness check but could not fail — nothing in this test kills an
    // axiom — and the invariant it looked like it was pinning is the OPPOSITE
    // of the truth, which the next test pins for real.
    for &a in &rules.axiom_of_atomic_sub {
        if a != u32::MAX {
            let idx = a as usize;
            assert!(idx < internal.axioms.len(), "axiom index out of range");
        }
    }
}

/// **`collect_el_rules` does not consult `InternalOntology::live`.**
///
/// This is the fact `IncrementalSession` is built on, stated from the
/// saturation side: a retracted axiom that is merely TOMBSTONED (live bit
/// cleared, still in `axioms`) keeps compiling rules and keeps firing — a
/// silent false positive in release, where the session's `debug_assert` is
/// gone. It is why every retraction re-lowers the whole mirror through
/// `convert_ontology_seeded` instead of clearing a bit, and why the rule→axiom
/// provenance above cannot be read as "this rule is live".
///
/// Asserted as an equality in BOTH directions on purpose: if `collect_el_rules`
/// ever learns to skip dead axioms, this test goes red and the re-lower-on-
/// retraction machinery can be revisited — an over-caution, not a defect.
#[test]
fn a_tombstoned_axiom_still_compiles_its_rules() {
    let mut internal = load_fixture("pizza.ofn");
    let before = owl_dl_saturation::collect_el_rules_for_test(&internal);
    assert!(
        !before.atomic_subsumptions.is_empty(),
        "fixture must compile at least one atomic-subsumption rule"
    );

    let victim = before
        .axiom_of_atomic_sub
        .iter()
        .copied()
        .find(|&a| a != u32::MAX)
        .expect("at least one rule must carry real axiom provenance") as usize;
    assert!(
        internal.kill_axiom(victim),
        "the victim must have been live before this test killed it"
    );
    assert!(!internal.live.contains(victim));

    let after = owl_dl_saturation::collect_el_rules_for_test(&internal);
    assert_eq!(
        before.atomic_subsumptions.len(),
        after.atomic_subsumptions.len(),
        "killing an axiom changed the compiled rule set — saturation now reads \
         `live`, and the session no longer has to re-lower on a retraction"
    );
    assert_eq!(
        before.axiom_of_atomic_sub, after.axiom_of_atomic_sub,
        "the provenance table moved even though `axioms` did not"
    );
}

/// A `C ⊑ ⊥` rule that outlives its axiom keeps `C` permanently flagged
/// unsatisfiable — a false positive, the one failure mode this project treats as
/// unacceptable. So `directly_unsat` must be attributable too, and the attributed
/// axiom must really be the `SubClassOf(C, owl:Nothing)` that produced it.
#[test]
fn directly_unsat_rules_name_the_subclass_of_bot_axiom_they_came_from() {
    let internal = load_fixture("unsat_bot.ofn");
    let rules = owl_dl_saturation::collect_el_rules_for_test(&internal);

    assert_eq!(
        rules.directly_unsat.len(),
        rules.axiom_of_directly_unsat.len()
    );
    assert!(
        !rules.directly_unsat.is_empty(),
        "fixture must exercise the `C ⊑ ⊥` lowering"
    );

    for (&c, &a) in rules
        .directly_unsat
        .iter()
        .zip(rules.axiom_of_directly_unsat.iter())
    {
        assert_ne!(
            a,
            u32::MAX,
            "a told `C ⊑ ⊥` always has a source axiom — never the synthetic sentinel"
        );
        let idx = a as usize;
        assert!(idx < internal.axioms.len(), "axiom index out of range");
        // The named axiom must be the very `SubClassOf(sub, Bot)` that produced
        // this rule — provenance pointing at the WRONG axiom is worse than none.
        match &internal.axioms[idx] {
            owl_dl_core::Axiom::SubClassOf { sub, sup } => {
                assert!(
                    matches!(internal.concepts.get(*sup), owl_dl_core::ConceptExpr::Bot),
                    "attributed axiom is not a `... ⊑ ⊥`"
                );
                assert_eq!(
                    *internal.concepts.get(*sub),
                    owl_dl_core::ConceptExpr::Atomic(c),
                    "attributed axiom names a different subclass"
                );
            }
            other => panic!("attributed axiom is not a SubClassOf: {other:?}"),
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
