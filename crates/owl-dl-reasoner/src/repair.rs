//! Repair suggestions: minimal sets of axioms whose removal makes an unwanted
//! entailment `η` no longer hold. Repairs are the minimal hitting sets over all
//! justifications of `η` (Reiter diagnoses). Every reported repair is VERIFIED by
//! removing it and confirming `η` no longer holds — sound even when the
//! justification set is incomplete. Read-only; never mutates the ontology.

use std::collections::BTreeSet;

use horned_owl::model::{Component, ForIRI};
use horned_owl::ontology::set::SetOntology;

use crate::ReasonError;
use crate::justify::{Entailment, entails, find_all_justifications, logical_axioms, ontology_from};

/// Cap on justifications discovered for repair (independent of the user-facing
/// `max` on repairs). Generous so the hitting sets are computed over as complete a
/// justification set as the fragment allows; on EL/Horn this finds them all.
const REPAIR_JUSTIFICATION_CAP: usize = 100;

/// A single repair: the axioms to remove to break the entailment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repair<A: ForIRI> {
    /// Axioms to remove (sorted, minimal).
    pub remove: Vec<Component<A>>,
}

/// The result of a repair query.
#[derive(Debug, Clone)]
pub struct Repairs<A: ForIRI> {
    /// Whether `η` was entailed at all (`false` → nothing to repair).
    pub entailed: bool,
    /// Verified minimal repairs, smallest first, capped by the user `max`.
    pub repairs: Vec<Repair<A>>,
    /// Whether the repair set is complete (all minimal repairs found) — true iff
    /// the underlying justification set is complete (EL/Horn).
    pub complete: bool,
    /// Candidate hitting sets discarded because they failed verification (an
    /// unfound justification survived). >0 signals the reported set may be partial.
    pub dropped_unverified: usize,
}

/// Compute repairs for `q` in `onto`. Filled in by Task 3.
pub fn find_repairs<A: ForIRI>(
    onto: &SetOntology<A>,
    q: &Entailment,
    max: usize,
) -> Result<Repairs<A>, ReasonError> {
    let _ = (onto, q, max, REPAIR_JUSTIFICATION_CAP);
    Ok(Repairs {
        entailed: false,
        repairs: Vec::new(),
        complete: true,
        dropped_unverified: 0,
    })
}
