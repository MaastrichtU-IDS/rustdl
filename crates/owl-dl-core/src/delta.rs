//! Incremental lowering. See spec §3.
//!
//! `convert_ontology` cannot be reused for a delta: it sorts components before
//! interning and sorts the axiom list again afterwards, so ids and indices are
//! a function of the WHOLE axiom set. `convert_delta` interns into the existing
//! vocabulary and appends without sorting.

use std::collections::{HashMap, HashSet};

use horned_owl::model::{AnnotatedComponent, ForIRI};
use horned_owl::ontology::set::SetOntology;

use crate::InternalOntology;
use crate::convert::{ConversionError, convert_component, run_derivation_passes};
use crate::ontology::Axiom;

/// Lower `added` into `internal`, interning into the EXISTING vocabulary.
/// Returns the new axiom indices. Does not touch derived axioms - call
/// [`refresh_derived`] afterwards, in the same commit.
///
/// `_mirror` is unused: lowering a component needs only the vocabulary and the
/// concept pool. It is accepted for signature symmetry with
/// [`refresh_derived`] (which genuinely needs it) so a caller threads one
/// mirror through the whole commit and cannot pass a different ontology to the
/// two halves.
///
/// # Errors
/// Propagates the lowering error for any unsupported component.
pub fn convert_delta<A: ForIRI>(
    internal: &mut InternalOntology,
    _mirror: &SetOntology<A>,
    added: &[AnnotatedComponent<A>],
) -> Result<Vec<usize>, ConversionError> {
    let mut out = Vec::new();
    for ac in added {
        if let Some(axiom) = convert_component(
            &ac.component,
            &mut internal.vocabulary,
            &mut internal.concepts,
        )? {
            // push_USER_axiom: also records it in the pre-pass baseline the
            // derivation passes re-run over.
            out.push(internal.push_user_axiom(axiom));
        }
    }
    Ok(out)
}

/// What one [`refresh_derived`] commit did to the derived overlay.
#[derive(Debug, Default)]
pub struct DerivedDiff {
    /// Indices of derived axioms newly appended by this commit.
    pub added: Vec<usize>,
    /// Indices of derived axioms retracted (tombstoned) by this commit.
    pub killed: Vec<usize>,
}

/// Recompute ALL derived axioms over the live user axioms and reconcile.
///
/// SOUNDNESS: the derivation passes are whole-ontology fixpoints whose output
/// depends on the entire axiom set, so a stale derived axiom retained across a
/// delete is a FALSE POSITIVE — delete `Functional(dp)` and the `C ⊑ ⊥` it
/// produced keeps `C` unsatisfiable forever. Additions diverge too: a
/// from-scratch run may derive a TIGHTER common subsumer than the retained one.
/// So the whole overlay is recomputed and diffed at every commit. Cost is
/// ~7.6 % of a saturation-only classify on galen and the share falls with size
/// - see `docs/2026-08-19-incremental-lowering-floor-findings.md`.
///
/// `mirror` must be the horned-owl ontology AFTER the delta: two of the passes
/// (`derive_data_axioms`, `derive_data_domain_unions`) read source components
/// directly rather than the lowered IR.
///
/// Only axioms this overlay owns (`InternalOntology::derived`) are ever
/// retracted here; retracting a user axiom is the caller's job.
pub fn refresh_derived<A: ForIRI>(
    internal: &mut InternalOntology,
    mirror: &SetOntology<A>,
) -> DerivedDiff {
    debug_assert_baseline_matches_mirror(internal, mirror);

    // 1. The PRE-pass user baseline - the only input the passes may see.
    //    Feeding a previous revision's derived axioms back in would let them
    //    bootstrap themselves and survive the retraction of their premises.
    //
    //    NOT `live ∧ ¬derived`: `split_disjunctive_antecedents` and
    //    `decompose_long_chains` CONSUME their input, so an axiom they
    //    rewrote is in neither set, and the passes could not reproduce the
    //    replacements they emitted - which are marked `derived`, so step 3
    //    would retract them and silently delete real axiom content.
    let user: Vec<Axiom> = internal.user_axioms.clone();

    // 2. Re-run the passes over exactly those. `run_derivation_passes` returns
    //    the FULL post-pass list and restores `internal.axioms`, so everything
    //    the passes interned (concepts, auxiliary roles) stays in `internal`
    //    while the axiom vector - whose indices are load-bearing - does not
    //    move.
    let parked = std::mem::replace(&mut internal.axioms, user.clone());
    let full = run_derivation_passes(internal, mirror);
    internal.axioms = parked;
    let fresh = multiset_difference(full, &user);

    // 3. Reconcile BY VALUE so unchanged derived axioms keep their indices
    //    (proof provenance and rule indices stay valid across the commit).
    //
    //    The value -> index buckets are rebuilt from the live+derived bits on
    //    every call rather than cached: a cache would still name axioms this
    //    or an earlier commit tombstoned, and matching a re-derived axiom
    //    against a dead index would silently drop it.
    let stale: Vec<usize> = internal
        .live
        .ones()
        .filter(|i| internal.is_derived(*i))
        .collect();
    let mut buckets: HashMap<&Axiom, Vec<usize>> = HashMap::new();
    for &i in &stale {
        buckets.entry(&internal.axioms[i]).or_default().push(i);
    }
    // Detach the retained indices from the borrow before mutating `internal`.
    let mut retained: HashSet<usize> = HashSet::new();
    let mut to_push: Vec<Axiom> = Vec::new();
    for ax in fresh {
        match buckets.get_mut(&ax).and_then(Vec::pop) {
            Some(idx) => {
                retained.insert(idx);
            }
            None => to_push.push(ax),
        }
    }

    let mut diff = DerivedDiff::default();
    for ax in to_push {
        diff.added.push(internal.push_derived_axiom(ax));
    }
    for i in stale {
        if !retained.contains(&i) && internal.kill_axiom(i) {
            diff.killed.push(i);
        }
    }
    diff
}

/// `full` minus `baseline`, as multisets. Both may contain duplicates and
/// `baseline` is NOT necessarily a sub-multiset of `full`: two of the passes
/// rewrite entries rather than appending, so a baseline axiom can be missing
/// from `full`.
fn multiset_difference(mut full: Vec<Axiom>, baseline: &[Axiom]) -> Vec<Axiom> {
    full.sort();
    let mut base: Vec<&Axiom> = baseline.iter().collect();
    base.sort();
    let mut out = Vec::new();
    let mut j = 0usize;
    for ax in full {
        while j < base.len() && *base[j] < ax {
            j += 1;
        }
        if j < base.len() && *base[j] == ax {
            j += 1;
        } else {
            out.push(ax);
        }
    }
    out
}

/// Debug-only guard on the invariant every soundness claim here rests on: the
/// `mirror` handed to [`refresh_derived`] describes the SAME ontology as
/// `internal.user_axioms`.
///
/// Two of the passes (`derive_data_axioms`, `derive_data_domain_unions`) read
/// the mirror's source components rather than the IR. So a caller that kills
/// `Functional(dp)` in the IR but passes a mirror still containing the
/// component gets the stale `C ⊑ ⊥` RE-DERIVED - the exact false positive this
/// module exists to prevent, reintroduced from the caller side, and invisible
/// to every test that does not compare against a from-scratch run.
///
/// Re-lowers the mirror into a scratch copy of the vocabulary/pool (so no ids
/// are allocated in `internal`) and compares axiom multisets. O(|mirror|) and
/// compiled out of release builds.
#[allow(clippy::needless_pass_by_ref_mut)] // signature parity with the cfg'd-in arm
fn debug_assert_baseline_matches_mirror<A: ForIRI>(
    internal: &InternalOntology,
    mirror: &SetOntology<A>,
) {
    #[cfg(debug_assertions)]
    {
        let mut vocab = internal.vocabulary.clone();
        let mut pool = internal.concepts.clone();
        let mut from_mirror: Vec<Axiom> = Vec::new();
        for ac in mirror {
            match convert_component(&ac.component, &mut vocab, &mut pool) {
                Ok(Some(ax)) => from_mirror.push(ax),
                Ok(None) => {}
                // A component the mirror cannot lower would have failed
                // `convert_ontology` too; nothing to compare against.
                Err(_) => return,
            }
        }
        from_mirror.sort();
        let mut baseline = internal.user_axioms.clone();
        baseline.sort();
        assert_eq!(
            from_mirror.len(),
            baseline.len(),
            "refresh_derived: mirror and IR baseline disagree ({} lowered from the mirror vs {} \
             in user_axioms). The mirror must be the ontology AFTER the delta - passing a stale \
             one re-derives axioms whose premises were retracted.",
            from_mirror.len(),
            baseline.len()
        );
        assert!(
            from_mirror == baseline,
            "refresh_derived: mirror and IR baseline describe different ontologies"
        );
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = (internal, mirror);
    }
}
