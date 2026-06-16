//! The Sequoia context order `≻ᵥ` (S1 / ALCH) — the make-or-break construction.
//!
//! Per `docs/superpowers/specs/2026-06-16-cb-sequoia-rearchitecture-design.md`
//! §0.45 (the AUTHORITATIVE correction): completeness needs a **subsumer-
//! respecting** order, NOT an arbitrary Condition-C2 order. For a told `X ⊑ Y`
//! we must rank `X ≻ Y` (subsumee above subsumer) so derivations flow to
//! subsumers — the chain `B⊑D` is only resolved (and `A→D` derived) when `B` is
//! `≻`-above `D` and hence Hyper-eligible. The query/root atom `A` is forced
//! `≻`-MINIMAL among concept atoms (Condition C2).
//!
//! # Construction (told-subsumer depth)
//!
//! Over the atomic concepts we build a told-subsumption DAG from the unit
//! ontology clauses `{X} → {Y}` (single atom premise, single atom head — a told
//! `X ⊑ Y`). Each atom gets a `depth`:
//!
//! > `depth(X) = 1 + max over told-subsumers Y of X of depth(Y)`  (0 if none),
//!
//! computed by a fixpoint that is robust to cycles (equivalent classes form a
//! cycle; the fixpoint caps their depth and they end up at equal rank, broken by
//! `ConceptId`). A higher depth = "more subsumee-like" = `≻`-greater. So `X ⊑ Y`
//! (told) gives `depth(X) > depth(Y)` whenever the DAG is acyclic, i.e. `X ≻ Y`.
//!
//! Comparison of two **atomic** literals: by `(depth, ConceptId)` descending on
//! depth. The per-context root atom is forced minimal by overriding its rank to
//! a sentinel below every other atom (Condition C2).
//!
//! Non-atomic head literals (`∃R.B`, `∀R.B`) are discharged by Succ / R∀, NOT by
//! Hyper; for the Hyper eligibility check `Δᵢ ⊁ᵥ Aᵢ` they are treated as
//! NEVER-blocking (incomparable / minimal). This is sound: the order only gates
//! WHICH atomic resolutions fire — never soundness — and admitting more
//! resolutions is MISS-biased toward completeness, never FP.

use owl_dl_core::ir::{ConceptExpr, ConceptId, ConceptPool};
use std::collections::HashMap;

/// Global, ontology-wide atom ranking shared by every context (the a-term-style
/// comparison is global; only the per-context query-minimal override differs).
#[derive(Debug, Default)]
pub(crate) struct OrderBuilder {
    /// `depth[atom-ConceptId] = told-subsumer depth` (subsumee deeper).
    depth: HashMap<ConceptId, u32>,
}

impl OrderBuilder {
    /// Build the global told-subsumer-depth map from the ontology clauses.
    ///
    /// A unit ontology clause `{X} → {Y}` with both `X`,`Y` atomic is read as a
    /// told `X ⊑ Y`. We accumulate the immediate told-subsumers of each atom,
    /// then run a depth fixpoint.
    #[must_use]
    pub(crate) fn build(clauses: &[crate::model::OntClause], pool: &ConceptPool) -> Self {
        // Immediate told-subsumers: subsumers_of[X] = { Y : told X ⊑ Y }.
        let mut subsumers_of: HashMap<ConceptId, Vec<ConceptId>> = HashMap::new();
        let mut atoms: Vec<ConceptId> = Vec::new();
        let is_atomic = |c: ConceptId| matches!(pool.get(c), ConceptExpr::Atomic(_));
        for cl in clauses {
            if cl.premise.len() == 1 && cl.head.len() == 1 {
                let (x, y) = (cl.premise[0], cl.head[0]);
                if is_atomic(x) && is_atomic(y) && x != y {
                    subsumers_of.entry(x).or_default().push(y);
                }
            }
        }
        // Collect every atom mentioned anywhere (so isolated atoms get depth 0).
        for cl in clauses {
            for &a in cl.premise.iter().chain(cl.head.iter()) {
                if is_atomic(a) {
                    atoms.push(a);
                }
            }
        }
        atoms.sort_unstable();
        atoms.dedup();

        let mut depth: HashMap<ConceptId, u32> = atoms.iter().map(|&a| (a, 0u32)).collect();
        // Fixpoint: depth(X) = 1 + max depth(Y) over told-subsumers Y. Cap the
        // number of rounds at |atoms| so a cyclic told-subsumption (equivalent
        // classes) terminates — the cap leaves cycle members at equal capped
        // depth, broken by ConceptId in `cmp_atoms`.
        let rounds = atoms.len();
        for _ in 0..=rounds {
            let mut changed = false;
            for &x in &atoms {
                if let Some(subs) = subsumers_of.get(&x) {
                    let best = subs
                        .iter()
                        .map(|y| depth.get(y).copied().unwrap_or(0))
                        .max()
                        .unwrap_or(0);
                    let want = best.saturating_add(1);
                    let cur = depth.get(&x).copied().unwrap_or(0);
                    if want > cur {
                        depth.insert(x, want);
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        Self { depth }
    }

    /// Materialise the per-context order for a root context cored at `query`
    /// (forced `≻`-minimal among atoms — Condition C2). For non-root contexts
    /// (successor cores) pass `query = None`: no atom is forced minimal there
    /// (C2 governs only the QUERY body atom of the read-off context).
    #[must_use]
    pub(crate) fn per_context(&self, query: Option<ConceptId>) -> PerContextOrder {
        PerContextOrder {
            // Snapshot only what `cmp` needs: the depth map is shared by Rc-free
            // copy of the small relevant entries. We clone the whole map (atom
            // counts are modest in the CB fragment); profile later if needed.
            depth: self.depth.clone(),
            query_minimal: query,
        }
    }
}

/// A per-context order `≻ᵥ`. Compares two literals; `cmp_lit(a, b) == Greater`
/// means `a ≻ᵥ b`.
#[derive(Clone, Debug, Default)]
pub(crate) struct PerContextOrder {
    depth: HashMap<ConceptId, u32>,
    /// The query/root atom forced `≻`-minimal (Condition C2), if any.
    query_minimal: Option<ConceptId>,
}

impl PerContextOrder {
    fn depth_of(&self, atom: ConceptId) -> u32 {
        self.depth.get(&atom).copied().unwrap_or(0)
    }

    /// Effective rank key of an ATOM under this context's order. The query atom
    /// is forced minimal via a sentinel `(0, _)` that loses to every other atom
    /// (we encode minimality by a leading flag: non-query atoms carry `1`, the
    /// query atom carries `0`, so the query atom is strictly least).
    fn atom_key(&self, atom: ConceptId) -> (u8, u32, u32) {
        if Some(atom) == self.query_minimal {
            (0, 0, 0)
        } else {
            (1, self.depth_of(atom), atom.index())
        }
    }

    /// Is `lit` an atomic literal? (`∃R.B`/`∀R.B` are structural.)
    fn is_atomic(pool: &ConceptPool, lit: ConceptId) -> bool {
        matches!(pool.get(lit), ConceptExpr::Atomic(_) | ConceptExpr::Top)
    }

    /// `a ≻ᵥ b` for two ATOMIC literals (total order on atoms).
    ///
    /// Defined only over atomic literals — the only literals the Hyper
    /// eligibility check `Δᵢ ⊁ᵥ Aᵢ` ranges over in S1.
    #[must_use]
    fn atom_gt(&self, a: ConceptId, b: ConceptId) -> bool {
        self.atom_key(a) > self.atom_key(b)
    }

    /// The Hyper side-condition predicate `Δ ⊁ᵥ a`: NO atomic literal in the
    /// residual `delta` is strictly `≻ᵥ` the resolved atom `a`. Non-atomic
    /// literals in `delta` never block (they are discharged by Succ/R∀, not
    /// Hyper) — admitting more resolutions is MISS-biased, never FP.
    #[must_use]
    pub(crate) fn eligible(&self, pool: &ConceptPool, delta: &[ConceptId], a: ConceptId) -> bool {
        delta.iter().all(|&l| {
            if Self::is_atomic(pool, l) {
                !self.atom_gt(l, a)
            } else {
                true
            }
        })
    }

    /// Sort a head disjunction by `≻ᵥ` ascending so the maximal literal is last.
    /// Atomic literals are ordered by their key; non-atomic literals are placed
    /// BELOW all atomics (they are minimal for ordering purposes) and ordered
    /// among themselves by `ConceptId` for determinism.
    pub(crate) fn sort_head(&self, pool: &ConceptPool, head: &mut [ConceptId]) {
        head.sort_by(|&a, &b| {
            let ka = if Self::is_atomic(pool, a) {
                (1u8, self.atom_key(a))
            } else {
                (0u8, (0, 0, a.index()))
            };
            let kb = if Self::is_atomic(pool, b) {
                (1u8, self.atom_key(b))
            } else {
                (0u8, (0, 0, b.index()))
            };
            ka.cmp(&kb)
        });
    }
}
