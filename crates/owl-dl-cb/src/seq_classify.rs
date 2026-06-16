//! Sequoia ordered-calculus read-off (S1 / ALCH) — the positive `∈̂` check.
//!
//! After [`crate::seq_engine::saturate`], each reportable class `A` has a root
//! context cored `{A}`. The Sequoia read-off (Theorem 2, specialised to a
//! singleton body): `A ⊑ B` iff `A(x) → B(x) ∈̂ S_{q_A}` — the clause `A → B` is
//! contained **up to redundancy** (Def. 4) in the root's clause set. In S1 every
//! derived clause has an empty body, so the witness is a head `Δ' ⊆ {B}`:
//!
//! - a derived UNIT `{B}` (or a clause subsuming it — but in S1 the only head
//!   `⊆ {B}` is the unit itself or the empty `⊥`), or
//! - the empty clause `→ ⊥` (`A ⊑ ⊥`, unsatisfiable).
//!
//! This is direct and positive — no negation, no `A ⊓ ¬B`, no refutation.
//! Mirrors the unordered engine's `classify.rs` read-off (same `top_equiv` and
//! transitive-closure handling) so the differential against B1 is apples-to-
//! apples.

use crate::CbHierarchy;
use crate::normalize::Normalized;
use crate::seq_model::SeqGraph;
use owl_dl_core::ir::{ClassId, ConceptExpr, ConceptId};
use std::collections::{BTreeMap, BTreeSet};

/// Read the (transitively closed) atomic-class subsumption relation + the
/// unsatisfiable set from the saturated ordered graph.
#[must_use]
pub(crate) fn read_hierarchy(norm: &Normalized, graph: &SeqGraph) -> CbHierarchy {
    let mut out = CbHierarchy::default();

    let class_atom: BTreeMap<ClassId, ConceptId> = {
        let mut m = BTreeMap::new();
        for (id, e) in norm.pool.iter_with_ids() {
            if let ConceptExpr::Atomic(c) = e {
                m.insert(*c, id);
            }
        }
        m
    };
    let reportable: BTreeSet<ClassId> = norm.classes.iter().copied().collect();
    let atom_class: BTreeMap<ConceptId, ClassId> = class_atom
        .iter()
        .filter(|(c, _)| reportable.contains(c))
        .map(|(c, id)| (*id, *c))
        .collect();

    // ⊤-equivalent classes (mirrors classify.rs): a class appearing as the sole
    // atomic head of an empty-premise ontology clause is ≡ owl:Thing; excluded
    // from the superclass position (the hybrid folds it into Thing). Sound —
    // dropping `X ⊑ C` here only ever MISSes, never FPs.
    let mut top_equiv: BTreeSet<ClassId> = BTreeSet::new();
    for cl in &norm.clauses {
        if cl.premise.is_empty()
            && cl.head.len() == 1
            && let ConceptExpr::Atomic(c) = norm.pool.get(cl.head[0])
            && reportable.contains(c)
        {
            top_equiv.insert(*c);
        }
    }

    // Direct subsumptions + unsat via the `∈̂` read-off (Def-4): head ⊆ {sup}.
    //
    // Two regimes (mirror `seq_engine::order_regime`):
    // - R2 (`per_class`, default): one root context per class, cored `{A}`,
    //   reused via `by_core`. Harvest ALL unit/empty heads from it.
    // - R1 (`per_query`): one root QUERY context per `(A,B)` pair, cored `{A}`
    //   with `B` head-minimal, in `by_query`. Read the specific pair from its
    //   own context (`A → B` iff `→ B` or `→ ⊥` is in `S_{q_(A,B)}`).
    //
    // CAVEAT (R1 only): this OVER-reports an unsat sub `A` — `→ ⊥ ∈ S_{q_(A,B)}`
    // emits `A ⊑ B` for EVERY `B` (logically sound, `⊥ ⊑ X`), whereas B1 / the R2
    // read-off suppress subsumers of an unsat class. R1 is opt-in and unused on
    // the default path (R2 clears the gate); were it ever invoked, cb-diff would
    // show spurious `only_in_cb` for unsat subjects. R1 is the C2-EXACT order
    // *oracle for ordering*, not a B1-output-identical engine — do not treat it
    // as a validated drop-in. Tighten the unsat handling here if R1 is promoted.
    let per_query = matches!(std::env::var("RUSTDL_CB_ORDER").as_deref(), Ok("per_query"));
    let mut direct: BTreeMap<ClassId, BTreeSet<ClassId>> = BTreeMap::new();

    if per_query {
        for ((core, head), &cid) in &graph.by_query {
            // core is the singleton `{A}` here.
            let Some(&sub_atom) = core.iter().next() else {
                continue;
            };
            let Some(&sub) = atom_class.get(&sub_atom) else {
                continue;
            };
            let ctx = &graph.contexts[cid];
            let has_bot = ctx.clauses.iter().any(|c| c.head.is_empty());
            if has_bot {
                out.unsat.insert(sub);
            }
            if let Some(&sup) = atom_class.get(head)
                && sup != sub
                && ctx
                    .clauses
                    .iter()
                    .any(|c| c.head.is_empty() || (c.head.len() == 1 && c.head[0] == *head))
            {
                direct.entry(sub).or_default().insert(sup);
            }
        }
    } else {
        // R2: root context per reportable class (core == {A-atom}).
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
        for (&cls, &cid) in &root_of {
            let ctx = &graph.contexts[cid];
            let has_bot = ctx.clauses.iter().any(|c| c.head.is_empty());
            if has_bot {
                out.unsat.insert(cls);
            }
            for c in &ctx.clauses {
                // `Δ' ⊆ {sup}`: a unit head `{sup}` witnesses `A → sup`.
                if c.head.len() == 1
                    && let Some(&sup) = atom_class.get(&c.head[0])
                    && sup != cls
                {
                    direct.entry(cls).or_default().insert(sup);
                }
            }
        }
    }

    // Transitive closure over the direct relation.
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

    // Transitively close ⊤-equivalence under the derived hierarchy.
    {
        let base: Vec<ClassId> = top_equiv.iter().copied().collect();
        for x in base {
            if let Some(sups) = reach.get(&x) {
                for &y in sups {
                    top_equiv.insert(y);
                }
            }
        }
    }

    for (&sub, sups) in &reach {
        for &sup in sups {
            if sub != sup && !top_equiv.contains(&sup) {
                out.subsumptions.insert((sub, sup));
            }
        }
    }

    out
}
