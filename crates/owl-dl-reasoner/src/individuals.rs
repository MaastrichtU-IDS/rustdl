//! Inferred `same_individuals` / `different_individuals` queries (issue #46).
//!
//! Sound: a same-pair is reported only from an inconsistent
//! `KB ∪ {a≠b}` verdict (never from a satisfying model), and a different-pair
//! only from a proven-unsatisfiable `{a} ⊓ {b}` (no Unique Name Assumption —
//! distinctness is genuinely PROVEN).
use crate::union_find::UnionFind;
use crate::{PreparedOntology, ReasonError};
use horned_owl::model::ForIRI;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::convert::convert_ontology;
use owl_dl_core::ir::IndividualId;
use owl_dl_core::ontology::Axiom;
use owl_dl_core::vocab::Vocabulary;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// Entailed same-individual equivalence groups, plus a completeness flag.
#[derive(Debug, Clone)]
pub struct SameIndividuals {
    groups: Vec<Vec<String>>,
    incomplete: bool,
}

impl SameIndividuals {
    /// Equivalence groups (each of size ≥ 2) of IRIs entailed to denote the
    /// same individual. Each group's members are sorted; the outer list is
    /// sorted too, so the output is deterministic.
    #[must_use]
    pub fn groups(&self) -> &[Vec<String>] {
        &self.groups
    }

    /// `true` iff a probe timed out, or ANY extension probe beyond the
    /// asserted-`SameIndividual` + derived-functional-merge seed was
    /// consulted. That seed alone is sound-complete; any further pairwise
    /// `consistent_with_extra` probe relies on a "consistent" (satisfiable)
    /// verdict, which — while sound for THIS engine call — this query
    /// conservatively treats as a trusted-Sat under-approximation, so the
    /// reported group set may be missing entailed merges whenever this is
    /// `true`.
    #[must_use]
    pub fn incomplete(&self) -> bool {
        self.incomplete
    }
}

/// Entailed different-individual pairs, plus a completeness flag.
#[derive(Debug, Clone)]
pub struct DifferentIndividuals {
    pairs: Vec<(String, String)>,
    incomplete: bool,
}

impl DifferentIndividuals {
    /// `(a, b)` pairs with `a < b`, sorted and deduplicated, of IRIs proven
    /// to denote distinct individuals.
    #[must_use]
    pub fn pairs(&self) -> &[(String, String)] {
        &self.pairs
    }

    /// `true` iff a probe timed out — the reported set may be missing
    /// entailed distinctions.
    #[must_use]
    pub fn incomplete(&self) -> bool {
        self.incomplete
    }
}

/// Named (non-anonymous) individuals from `vocabulary`, sorted by IRI for
/// deterministic pair enumeration and output.
fn named_individuals(vocabulary: &Vocabulary) -> Vec<(IndividualId, String)> {
    let mut out: Vec<(IndividualId, String)> = vocabulary
        .individuals()
        .filter(|(_, iri)| !iri.starts_with(owl_dl_core::convert::ANON_IRI_PREFIX))
        .map(|(id, iri)| (id, iri.to_string()))
        .collect();
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out
}

/// Canonical `(lo, hi)` ordering by index for a symmetric pair key.
fn canon_key(a: IndividualId, b: IndividualId) -> (u32, u32) {
    let (x, y) = (a.index(), b.index());
    if x <= y { (x, y) } else { (y, x) }
}

/// Entailed same-individual equivalence groups. Seeds a union-find from
/// asserted `SameIndividual` axioms and the `ABox` saturator's
/// `derived_same` (functional/inverse-functional-forced merges — Task 0.1),
/// then extends: for each not-yet-same candidate pair `(a, b)`, `a = b` is
/// entailed iff `KB ∪ {a ≠ b}` is inconsistent
/// (`PreparedOntology::consistent_with_extra`). `pair_deadline` bounds each
/// such extension probe; `None` = unbounded.
///
/// # Errors
/// [`ReasonError::Inconsistent`] if the ontology is inconsistent;
/// [`ReasonError::Conversion`] if the input can't be lowered to the internal
/// IR.
pub fn same_individuals<A: ForIRI>(
    onto: &SetOntology<A>,
    pair_deadline: Option<Duration>,
) -> Result<SameIndividuals, ReasonError> {
    let internal = convert_ontology(onto)?;
    let saturation = crate::abox_saturation::saturate_abox_consistency(&internal);
    if saturation.clash {
        return Err(ReasonError::Inconsistent);
    }

    // Seed the union-find from told `SameIndividual` axioms + the `ABox`
    // saturator's derived-same (functional-role-forced) pairs. This seed
    // alone is sound-complete.
    let n = internal.vocabulary.num_individuals();
    let mut uf = UnionFind::new(n);
    for ax in &internal.axioms {
        if let Axiom::SameIndividual(inds) = ax {
            for w in inds.windows(2) {
                uf.union(w[0].index(), w[1].index());
            }
        }
    }
    for &(a, b) in &saturation.derived_same {
        uf.union(a.index(), b.index());
    }

    // `from_internal` clones `internal.vocabulary` before consuming
    // `internal`, so `prepared.vocabulary` resolves the same IRI ↔ id
    // mapping used above.
    let prepared = PreparedOntology::from_internal(internal)?;
    let names = named_individuals(&prepared.vocabulary);

    let mut incomplete = false;
    for i in 0..names.len() {
        for j in (i + 1)..names.len() {
            let (a, _) = &names[i];
            let (b, _) = &names[j];
            if uf.same(a.index(), b.index()) {
                continue;
            }
            // Any pair not already resolved by the seed requires an
            // extension probe — the seed is the only sound-complete source,
            // so consulting a probe at all makes the result potentially
            // incomplete (see `SameIndividuals::incomplete`).
            incomplete = true;
            let deadline = pair_deadline.map(|d| Instant::now() + d);
            if prepared.consistent_with_extra(&[(*a, *b)], &[], deadline)? == Some(false) {
                uf.union(a.index(), b.index());
            }
        }
    }

    // Emit equivalence groups of size ≥ 2, each sorted, outer list sorted.
    let mut by_root: HashMap<u32, Vec<String>> = HashMap::new();
    for (id, iri) in &names {
        let root = uf.find(id.index());
        by_root.entry(root).or_default().push(iri.clone());
    }
    let mut groups: Vec<Vec<String>> = by_root
        .into_values()
        .filter(|g| g.len() >= 2)
        .map(|mut g| {
            g.sort();
            g
        })
        .collect();
    groups.sort();

    Ok(SameIndividuals { groups, incomplete })
}

/// Entailed different-individual pairs. Seeds from told
/// `DifferentIndividuals` axioms (horned-owl folds the `AllDifferent`
/// construct into the same axiom shape — pairwise over its member list),
/// then extends: for each remaining candidate pair, `a ≠ b` is entailed iff
/// `{a} ⊓ {b}` is unsatisfiable
/// (`PreparedOntology::pair_individuals_disjoint_with_deadline`).
/// `pair_deadline` bounds each such probe; `None` = unbounded.
///
/// # Errors
/// [`ReasonError::Inconsistent`] if the ontology is inconsistent;
/// [`ReasonError::Conversion`] if the input can't be lowered to the internal
/// IR.
pub fn different_individuals<A: ForIRI>(
    onto: &SetOntology<A>,
    pair_deadline: Option<Duration>,
) -> Result<DifferentIndividuals, ReasonError> {
    let internal = convert_ontology(onto)?;
    if crate::abox_saturation::saturate_abox_consistency(&internal).clash {
        return Err(ReasonError::Inconsistent);
    }

    // Seed told-different pairs (asserted `DifferentIndividuals`/`AllDifferent`).
    let mut told_different: HashSet<(u32, u32)> = HashSet::new();
    for ax in &internal.axioms {
        if let Axiom::DifferentIndividuals(inds) = ax {
            for i in 0..inds.len() {
                for j in (i + 1)..inds.len() {
                    told_different.insert(canon_key(inds[i], inds[j]));
                }
            }
        }
    }

    let prepared = PreparedOntology::from_internal(internal)?;
    let names = named_individuals(&prepared.vocabulary);

    let mut pairs: Vec<(String, String)> = Vec::new();
    let mut incomplete = false;
    for i in 0..names.len() {
        for j in (i + 1)..names.len() {
            let (a, a_iri) = &names[i];
            let (b, b_iri) = &names[j];
            let is_different = if told_different.contains(&canon_key(*a, *b)) {
                true
            } else {
                let deadline = pair_deadline.map(|d| Instant::now() + d);
                match prepared.pair_individuals_disjoint_with_deadline(*a, *b, deadline)? {
                    Some(true) => true,
                    Some(false) => false,
                    None => {
                        incomplete = true;
                        false
                    }
                }
            };
            if is_different {
                let (lo, hi) = if a_iri <= b_iri {
                    (a_iri, b_iri)
                } else {
                    (b_iri, a_iri)
                };
                pairs.push((lo.clone(), hi.clone()));
            }
        }
    }
    pairs.sort();
    pairs.dedup();

    Ok(DifferentIndividuals { pairs, incomplete })
}
