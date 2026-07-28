//! The Sequoia context order `≻ᵥ` (S1 / ALCH) — the make-or-break construction.
//!
//! Per `docs/superpowers/specs/2026-06-16-cb-sequoia-order-extraction-and-spec.md`
//! (the PRIMARY-SOURCE extraction, PART 4) the per-context order is **three-tier**:
//!
//! 1. **Contextually-DEAD atoms — MAXIMAL.** `X` is dead in context `A` if
//!    `O ⊨ A ⊓ X ⊑ ⊥`. Two sound, statically-detectable sources, both read off
//!    the normalized clause set: **global unsat** `X ⊑ ⊥` (an empty-head clause
//!    with a single distinct premise atom `{X} → {}`) and **told-disjoint from
//!    the core** `A ⊓ X ⊑ ⊥` (an empty-head clause with two distinct premise
//!    atoms `{A, X} → {}`, used symmetrically). A dead disjunct MUST be
//!    `≻`-maximal so the empty-head clause is Hyper-eligible to resolve it OUT of
//!    any disjunction `A → … ∨ X ∨ …`, leaving the live disjuncts (extraction
//!    §3.2/§3.4 traces).
//! 2. **Live atoms — by subsumer-respecting depth (descending).** A told `X ⊑ Y`
//!    gives `depth(X) = 1 + depth(Y) > depth(Y)` ⟹ `X ≻ Y` (subsumee maximal),
//!    so the chain `X⊑Y` is Hyper-resolved and the subsumer propagates down.
//! 3. **The core class A itself — MINIMAL.** Property 1 of Def-3 (`A ≻ x`) still
//!    holds; making the core least is harmless (the core is seeded as a unit
//!    `→A` and never resolved-out) and keeps the order deterministic.
//!
//! Deadness is **per-context** (depends on the core) — correct, since orders are
//! per-context anyway, and is exactly why a single global depth map was
//! structurally wrong for the dead tier.
//!
//! # SOUNDNESS
//! An order bug is MISS-biased, never FP — the Sequoia rules are sound for ANY
//! order (Soundness Theorem, order-independent, `calculus.tex` 384–388). Tier-1
//! only promotes atoms with a GENUINE `X⊑⊥` / `A⊓X⊑⊥` entailment (empty-head
//! clause), so the dead-detection may be a sound under-approximation without
//! endangering FP=0. `∃R.B`/`∀R.B` stay never-blocking in `eligible()` (discharged
//! by Succ/All, not Hyper) — sound, MISS-biased, unchanged.

use owl_dl_core::ir::{ConceptExpr, ConceptId, ConceptPool};
use std::collections::{HashMap, HashSet};

/// Global, ontology-wide order data shared by every context. The live-tier
/// `depth` map is global (a-term-style comparison is global); the dead tier is
/// materialised per-context from `global_unsat` + `told_disjoint`.
#[derive(Debug, Default)]
pub(crate) struct OrderBuilder {
    /// `depth[atom-ConceptId] = told-subsumer depth` (subsumee deeper).
    depth: HashMap<ConceptId, u32>,
    /// Atoms `X` with a global `X ⊑ ⊥` (empty-head single-premise clause).
    global_unsat: HashSet<ConceptId>,
    /// Told-disjoint pairs `A ⊓ X ⊑ ⊥` (empty-head two-premise clause), stored
    /// SYMMETRICALLY: `told_disjoint[A] ∋ X` AND `told_disjoint[X] ∋ A`.
    told_disjoint: HashMap<ConceptId, HashSet<ConceptId>>,
}

impl OrderBuilder {
    /// Build the global order data from the normalized clauses.
    ///
    /// - Unit `{X} → {Y}` (both atomic) ⟹ told `X ⊑ Y` ⟹ depth fixpoint.
    /// - Empty-head `{X} → {}` (one DISTINCT atomic premise) ⟹ `X ⊑ ⊥` ⟹
    ///   `global_unsat`.
    /// - Empty-head `{A, X} → {}` (two DISTINCT atomic premises) ⟹ `A ⊓ X ⊑ ⊥` ⟹
    ///   `told_disjoint` (both directions).
    ///
    /// We dedup the distinct premise atoms before classifying by arity, because
    /// `DisjointClasses(:X :X)` lowers to `and(X,X) ⊑ ⊥` and the pool's `and` may
    /// or may not collapse the duplicate — a self-disjoint `X` is genuinely
    /// `X ⊑ ⊥`, so it must land in `global_unsat`, not `told_disjoint`.
    #[must_use]
    pub(crate) fn build(clauses: &[crate::model::OntClause], pool: &ConceptPool) -> Self {
        let is_atomic = |c: ConceptId| matches!(pool.get(c), ConceptExpr::Atomic(_));

        // Immediate told-subsumers: subsumers_of[X] = { Y : told X ⊑ Y }.
        let mut subsumers_of: HashMap<ConceptId, Vec<ConceptId>> = HashMap::new();
        let mut atoms: Vec<ConceptId> = Vec::new();
        let mut global_unsat: HashSet<ConceptId> = HashSet::new();
        let mut told_disjoint: HashMap<ConceptId, HashSet<ConceptId>> = HashMap::new();

        for cl in clauses {
            // Live tier: told X ⊑ Y.
            if cl.premise.len() == 1 && cl.head.len() == 1 {
                let (x, y) = (cl.premise[0], cl.head[0]);
                if is_atomic(x) && is_atomic(y) && x != y {
                    subsumers_of.entry(x).or_default().push(y);
                }
            }
            // Dead tier: empty-head clauses. Classify by DISTINCT atomic premise
            // atoms (dedup defends against `and(X,X)` not collapsing).
            if cl.head.is_empty() {
                let mut distinct: Vec<ConceptId> = cl
                    .premise
                    .iter()
                    .copied()
                    .filter(|&a| is_atomic(a))
                    .collect();
                distinct.sort_unstable();
                distinct.dedup();
                match distinct.as_slice() {
                    [x] => {
                        global_unsat.insert(*x);
                    }
                    [a, x] => {
                        told_disjoint.entry(*a).or_default().insert(*x);
                        told_disjoint.entry(*x).or_default().insert(*a);
                    }
                    _ => {}
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
        // depth, broken by ConceptId in `atom_key`. (PART 4 keeps the round cap;
        // a strict total order is recovered because the ConceptId tie-break is
        // total regardless of the depth values.)
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
        Self {
            depth,
            global_unsat,
            told_disjoint,
        }
    }

    /// Materialise the per-context order for a context whose conjunctive `core`
    /// holds. The dead set is `global_unsat ∪ { X : ∃ c∈core, (c,X) told-disjoint }`
    /// — contextual deadness. Core atoms are tier-0 (minimal) automatically.
    ///
    /// `mode = PerQuery(head)` (the R1 fallback, extraction §3.0 / PART 4 item 5)
    /// additionally forces the head atom `head` `≻`-MINIMAL (C2-exact); it ranks
    /// strictly below even the core so the candidate subsumer is order-minimal.
    #[must_use]
    pub(crate) fn per_context(
        &self,
        core: &std::collections::BTreeSet<ConceptId>,
    ) -> PerContextOrder {
        self.per_context_mode(core, OrderMode::PerClass)
    }

    /// Like [`Self::per_context`] but with an explicit [`OrderMode`] — the R1
    /// `PerQuery(head)` variant forces `head` strictly minimal (C2-exact).
    #[must_use]
    pub(crate) fn per_context_mode(
        &self,
        core: &std::collections::BTreeSet<ConceptId>,
        mode: OrderMode,
    ) -> PerContextOrder {
        let mut dead: HashSet<ConceptId> = self.global_unsat.clone();
        for &c in core {
            if let Some(xs) = self.told_disjoint.get(&c) {
                dead.extend(xs.iter().copied());
            }
        }
        PerContextOrder {
            depth: self.depth.clone(),
            dead,
            core: core.iter().copied().collect(),
            query_minimal: match mode {
                OrderMode::PerClass => None,
                OrderMode::PerQuery(head) => Some(head),
            },
        }
    }
}

/// The order regime for a context (extraction PART 4 item 5).
#[derive(Clone, Copy, Debug)]
pub(crate) enum OrderMode {
    /// R2: one context per class; the three-tier dead-maximal order. Default.
    PerClass,
    /// R1: one context per `(core, head)` query pair; the head atom is forced
    /// `≻`-minimal (Condition C2, Theorem-2-exact).
    PerQuery(ConceptId),
}

/// A per-context order `≻ᵥ`. `atom_gt(a, b)` means `a ≻ᵥ b`.
#[derive(Clone, Debug, Default)]
pub(crate) struct PerContextOrder {
    depth: HashMap<ConceptId, u32>,
    /// Contextually-dead atoms (tier-2, maximal).
    dead: HashSet<ConceptId>,
    /// The context's core atoms (tier-0, minimal).
    core: HashSet<ConceptId>,
    /// R1 only: the query head atom forced strictly `≻`-minimal (Condition C2).
    query_minimal: Option<ConceptId>,
}

impl PerContextOrder {
    fn depth_of(&self, atom: ConceptId) -> u32 {
        self.depth.get(&atom).copied().unwrap_or(0)
    }

    /// Three-tier rank key of an ATOM as a `(tier, depth, ConceptId)` tuple; the
    /// natural tuple order IS `≻ᵥ` (greater tuple = `≻`-greater = more eligible).
    ///
    /// - query-minimal head (R1 only) ⟹ `(0, 0, 0)` — the unique minimum (C2);
    /// - core atom ⟹ `(1, 0, cid)` — minimal among non-query atoms;
    /// - live atom ⟹ `(2, depth, cid)` — ordered by `(depth, cid)`, subsumee
    ///   (deeper) above subsumer;
    /// - dead atom ⟹ `(3, 0, cid)` — maximal.
    ///
    /// So **dead > live > core > query-minimal**. A query-minimal atom that is
    /// ALSO dead/core stays minimal (the C2 discipline wins by construction).
    fn atom_key(&self, atom: ConceptId) -> (u8, u32, u32) {
        if self.query_minimal == Some(atom) {
            (0, 0, 0)
        } else if self.dead.contains(&atom) {
            (3, 0, atom.index())
        } else if self.core.contains(&atom) {
            (1, 0, atom.index())
        } else {
            (2, self.depth_of(atom), atom.index())
        }
    }

    /// Is `lit` an atomic literal? (`∃R.B`/`∀R.B` are structural.)
    fn is_atomic(pool: &ConceptPool, lit: ConceptId) -> bool {
        matches!(pool.get(lit), ConceptExpr::Atomic(_) | ConceptExpr::Top)
    }

    /// `a ≻ᵥ b` for two ATOMIC literals (strict total order on atoms).
    #[must_use]
    fn atom_gt(&self, a: ConceptId, b: ConceptId) -> bool {
        self.atom_key(a) > self.atom_key(b)
    }

    /// The Hyper side-condition predicate `Δ ⊁ᵥ a`: NO atomic literal in the
    /// residual `delta` is strictly `≻ᵥ` the resolved atom `a`. Non-atomic
    /// literals never block (discharged by Succ/All) — MISS-biased, never FP.
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
    /// Non-atomic literals are placed BELOW all atomics (minimal for ordering),
    /// ordered among themselves by `ConceptId` for determinism.
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
