//! Read the atomic-class hierarchy off the saturated context graph (Task B).
//!
//! After [`crate::engine::saturate`], each reportable class `A` has a root
//! context (core `{A}`). The subsumption read-off (SKH §4.2 / Theorem 1
//! specialized to a singleton context core):
//!
//! - `A ⊑ B` iff `A`'s root context derives the **singleton unit clause**
//!   `{B}` for an atomic `B` (the closure entails `core ⊑ B`). A multi-literal
//!   head like `{B, C}` does **not** give `A ⊑ B` (that would be unsound — it
//!   only says `A ⊑ B ⊔ C`).
//! - `A` is **unsatisfiable** iff its root context derives the empty clause
//!   (`A ⊑ ⊥`).
//!
//! An unsatisfiable class subsumes (and is subsumed by) everything under `⊥`
//! semantics; the [`CbHierarchy`] keeps the unsat set separate (mirroring the
//! reasoner's `Classification`, which reports unsatisfiable classes apart from
//! the subsumption relation), so we do **not** flood `subsumptions` with the
//! `A ⊑ everything` pairs of an unsat `A`. We do still report the genuine
//! atomic subsumptions derived in its (collapsed) context.
//!
//! The collected relation is transitively closed; reflexive pairs and
//! `owl:Thing`/`owl:Nothing` (`Top`/`Bot`) on either side are excluded.

use crate::CbHierarchy;
use crate::model::ContextGraph;
use crate::normalize::Normalized;
use owl_dl_core::ir::{ClassId, ConceptExpr, ConceptId};
use std::collections::{BTreeMap, BTreeSet};

/// Read the (transitively closed) atomic-class subsumption relation + the
/// unsatisfiable set from the saturated graph.
#[must_use]
pub fn read_hierarchy(norm: &Normalized, graph: &ContextGraph) -> CbHierarchy {
    let mut out = CbHierarchy::default();

    // Map each reportable class to its `Atomic` ConceptId.
    let class_atom: BTreeMap<ClassId, ConceptId> = {
        let mut m = BTreeMap::new();
        for (id, e) in norm.pool.iter_with_ids() {
            if let ConceptExpr::Atomic(c) = e {
                m.insert(*c, id);
            }
        }
        m
    };
    // Reverse: ConceptId → reportable ClassId (only for reportable classes).
    let reportable: BTreeSet<ClassId> = norm.classes.iter().copied().collect();
    let atom_class: BTreeMap<ConceptId, ClassId> = class_atom
        .iter()
        .filter(|(c, _)| reportable.contains(c))
        .map(|(c, id)| (*id, *c))
        .collect();

    // For each reportable class, find its root context (core == {A-atom}).
    let mut root_of: BTreeMap<ClassId, usize> = BTreeMap::new();
    for &cls in &norm.classes {
        let Some(&atom) = class_atom.get(&cls) else {
            continue;
        };
        let mut core = BTreeSet::new();
        core.insert(atom);
        if let Some(&cid) = graph.by_core.get(&core) {
            root_of.insert(cls, cid);
        }
    }

    // Collect direct (atomic, singleton-unit) subsumptions + unsat.
    // `direct[(sub)] = {sup, ...}`.
    let mut direct: BTreeMap<ClassId, BTreeSet<ClassId>> = BTreeMap::new();
    for (&cls, &cid) in &root_of {
        let ctx = &graph.contexts[cid];
        let has_bot = ctx.clauses.iter().any(|dc| dc.head.is_empty());
        if has_bot {
            out.unsat.insert(cls);
        }
        for dc in &ctx.clauses {
            if dc.head.len() == 1 {
                let lit = dc.head[0];
                if let Some(&sup) = atom_class.get(&lit)
                    && sup != cls
                {
                    direct.entry(cls).or_default().insert(sup);
                }
            }
        }
    }

    // Transitive closure over the direct relation.
    // (Floyd-style fixpoint; small class counts in B1.)
    let classes: Vec<ClassId> = norm.classes.clone();
    let mut reach: BTreeMap<ClassId, BTreeSet<ClassId>> = direct.clone();
    let mut changed = true;
    while changed {
        changed = false;
        let snapshot = reach.clone();
        for &sub in &classes {
            let Some(mids) = snapshot.get(&sub) else {
                continue;
            };
            let mids: Vec<ClassId> = mids.iter().copied().collect();
            for mid in mids {
                if let Some(sups) = snapshot.get(&mid) {
                    let entry = reach.entry(sub).or_default();
                    for &s in sups {
                        if s != sub && entry.insert(s) {
                            changed = true;
                        }
                    }
                }
            }
        }
    }

    for (&sub, sups) in &reach {
        for &sup in sups {
            if sub != sup {
                out.subsumptions.insert((sub, sup));
            }
        }
    }

    out
}
