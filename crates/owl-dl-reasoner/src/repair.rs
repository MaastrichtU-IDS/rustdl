//! Repair suggestions: minimal sets of axioms whose removal makes an unwanted
//! entailment `η` no longer hold. Repairs are the minimal hitting sets over all
//! justifications of `η` (Reiter diagnoses). Every reported repair is VERIFIED by
//! removing it and confirming `η` no longer holds — sound even when the
//! justification set is incomplete. Read-only; never mutates the ontology.

use std::collections::BTreeSet;

use horned_owl::model::{Component, ForIRI};
use horned_owl::ontology::set::SetOntology;

use crate::ReasonError;
use crate::justify::{Entailment, PreparedJustifier, entails, ontology_from};

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

/// Compute verified minimal repairs for `q` in `onto`.
///
/// Repairing several entailments of the SAME ontology? Prepare once and call
/// [`find_repairs_prepared`] instead — this entry point derives the per-ontology
/// state (axiom split + fragment) on every call.
pub fn find_repairs<A: ForIRI>(
    onto: &SetOntology<A>,
    q: &Entailment,
    max: usize,
) -> Result<Repairs<A>, ReasonError> {
    find_repairs_prepared(&PreparedJustifier::prepare(onto), q, max)
}

/// [`find_repairs`] over prepared per-ontology state, reused across queries.
///
/// Also cheaper for a single query than [`find_repairs`] used to be: the
/// logical-axiom split happened TWICE per call (once inside
/// `find_all_justifications`, once here for repair verification) and is now
/// computed once, by [`PreparedJustifier::prepare`].
///
/// # Errors
/// Propagates [`ReasonError`].
pub fn find_repairs_prepared<A: ForIRI>(
    prepared: &PreparedJustifier<A>,
    q: &Entailment,
    max: usize,
) -> Result<Repairs<A>, ReasonError> {
    // All justifications (generous internal cap, independent of the repair `max`).
    let justifications = prepared.find_all(q, REPAIR_JUSTIFICATION_CAP)?;
    if justifications.is_empty() {
        return Ok(Repairs {
            entailed: false,
            repairs: Vec::new(),
            complete: true,
            dropped_unverified: 0,
        });
    }
    let complete = justifications.iter().all(|j| j.minimal_guaranteed);

    // Hitting sets over the justification axiom-sets.
    let j_sets: Vec<BTreeSet<Component<A>>> = justifications
        .iter()
        .map(|j| j.axioms.iter().cloned().collect())
        .collect();
    let mut candidates = minimal_hitting_sets(&j_sets);
    // Smallest repairs first, deterministic.
    candidates.sort_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)));

    // Verify each candidate by removing it and re-checking the entailment.
    let (fixed, logical) = (prepared.background(), prepared.logical());
    let mut repairs = Vec::new();
    let mut dropped_unverified = 0usize;
    for h in candidates {
        if repairs.len() >= max {
            break;
        }
        let kept: Vec<Component<A>> = logical.iter().filter(|a| !h.contains(a)).cloned().collect();
        let reduced = ontology_from(fixed, &kept);
        if entails(&reduced, q)? {
            // An unfound justification survives — not a real repair.
            dropped_unverified += 1;
            continue;
        }
        repairs.push(Repair {
            remove: h.into_iter().collect(),
        });
    }

    Ok(Repairs {
        entailed: true,
        repairs,
        complete,
        dropped_unverified,
    })
}

/// Enumerate the minimal hitting sets (minimal transversals) over `justifications`:
/// the ⊆-minimal sets that intersect every justification. These are the minimal
/// repairs. Cheap for the small justification sets seen in practice; the
/// dominated-branch prune below bounds the search.
fn minimal_hitting_sets<A: ForIRI>(
    justifications: &[BTreeSet<Component<A>>],
) -> Vec<BTreeSet<Component<A>>> {
    let mut results: Vec<BTreeSet<Component<A>>> = Vec::new();
    if justifications.is_empty() {
        return results;
    }
    let mut seen: std::collections::HashSet<BTreeSet<Component<A>>> =
        std::collections::HashSet::new();
    let mut worklist: Vec<BTreeSet<Component<A>>> = vec![BTreeSet::new()];

    while let Some(h) = worklist.pop() {
        if !seen.insert(h.clone()) {
            continue;
        }
        // Prune: if some known minimal repair is already ⊆ h, h can't be minimal.
        if results.iter().any(|r| r.is_subset(&h)) {
            continue;
        }
        // First justification not hit by h.
        match justifications.iter().find(|j| j.is_disjoint(&h)) {
            None => {
                // h hits all → it is a minimal hitting set (prune above guaranteed
                // no subset already present). Drop any existing superset of h.
                results.retain(|r| !h.is_subset(r));
                results.push(h);
            }
            Some(ju) => {
                for a in ju {
                    let mut next = h.clone();
                    next.insert(a.clone());
                    worklist.push(next);
                }
            }
        }
    }
    results
}

#[cfg(test)]
#[allow(clippy::cloned_ref_to_slice_refs)] // verbatim Task-2 test bodies; single-elem sets
mod mhs_tests {
    use super::*;
    use horned_owl::model::ClassExpression as CE;
    use horned_owl::model::{Build, SubClassOf};

    type Rc = std::rc::Rc<str>;

    // Build a distinct dummy axiom per label so sets compare by content.
    fn ax(b: &Build<Rc>, name: &str) -> Component<Rc> {
        Component::SubClassOf(SubClassOf {
            sub: CE::Class(b.class(format!("urn:{name}sub").as_str())),
            sup: CE::Class(b.class(format!("urn:{name}sup").as_str())),
        })
    }
    fn set(items: &[Component<Rc>]) -> BTreeSet<Component<Rc>> {
        items.iter().cloned().collect()
    }

    // One justification {a, b}: minimal hitting sets are {a} and {b}.
    #[test]
    fn single_justification_each_axiom_is_a_repair() {
        let b = Build::new_rc();
        let (a, c) = (ax(&b, "a"), ax(&b, "b"));
        let js = vec![set(&[a.clone(), c.clone()])];
        let mhs = minimal_hitting_sets(&js);
        let got: BTreeSet<BTreeSet<Component<Rc>>> = mhs.into_iter().collect();
        let want: BTreeSet<BTreeSet<Component<Rc>>> = [set(&[a]), set(&[c])].into_iter().collect();
        assert_eq!(got, want);
    }

    // Two disjoint justifications {a},{b}: the only hitting set is {a,b}.
    #[test]
    fn disjoint_justifications_need_both() {
        let b = Build::new_rc();
        let (a, c) = (ax(&b, "a"), ax(&b, "b"));
        let js = vec![set(&[a.clone()]), set(&[c.clone()])];
        let mhs = minimal_hitting_sets(&js);
        assert_eq!(mhs, vec![set(&[a, c])]);
    }

    // Overlapping {a,b},{a,c}: shared {a} and {b,c} are both minimal transversals;
    // neither is a subset of the other.
    #[test]
    fn overlapping_justifications_share_repair() {
        let b = Build::new_rc();
        let (a, c, d) = (ax(&b, "a"), ax(&b, "b"), ax(&b, "c"));
        let js = vec![set(&[a.clone(), c.clone()]), set(&[a.clone(), d.clone()])];
        let mhs: BTreeSet<BTreeSet<Component<Rc>>> =
            minimal_hitting_sets(&js).into_iter().collect();
        assert!(
            mhs.contains(&set(&[a.clone()])),
            "shared axiom {{a}} must be a repair"
        );
        assert!(
            mhs.contains(&set(&[c, d])),
            "{{b,c}} is also a minimal transversal"
        );
        for x in &mhs {
            for y in &mhs {
                if x != y {
                    assert!(!x.is_subset(y), "no repair may be a superset of another");
                }
            }
        }
    }

    // No justifications → no hitting sets.
    #[test]
    fn empty_justifications_no_repairs() {
        let js: Vec<BTreeSet<Component<Rc>>> = Vec::new();
        assert!(minimal_hitting_sets(&js).is_empty());
    }
}
