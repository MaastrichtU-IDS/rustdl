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
            out.push(internal.push_live_axiom(axiom));
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
    // 1. The live USER axioms - the only input the passes may see. Feeding a
    //    previous revision's derived axioms back in would let them bootstrap
    //    themselves and survive the retraction of their premises.
    let user: Vec<Axiom> = internal
        .live_user_axiom_indices()
        .map(|i| internal.axioms[i].clone())
        .collect();

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
        .filter(|i| internal.derived.contains(*i))
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
