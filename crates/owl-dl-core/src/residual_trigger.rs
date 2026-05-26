//! Residual-GCI trigger analysis — Phase 1 scaffolding.
//!
//! See [`docs/lazy-unfolding-plan.md`](../../docs/lazy-unfolding-plan.md)
//! for the full algorithm + soundness argument. This module
//! ships the analysis (pure function from a residual body to a
//! [`ResidualTrigger`]) and a small statistics aggregator. The
//! tableau integration (deferred materialisation in
//! `apply_residual_gcis`) lands in Phase 2.
//!
//! ## Why a split shipment
//!
//! [`moms-plan.md`](../../docs/moms-plan.md) §A taught us that
//! shipping a lazy-fire optimisation without measuring the
//! workload's shape produces a no-op revert. The Phase-1
//! deliverable here is the *measurement* — how many residuals
//! across the real corpus are `DeferOr` vs `Eager`, which tells
//! us upfront whether the Phase-2 integration will move walls
//! before we write it.

use crate::ir::{ConceptExpr, ConceptId, ConceptPool, Role};

/// Classification of how a residual GCI body should be
/// materialised on tableau nodes.
///
/// Variant guidance is the §"Trigger taxonomy" of
/// [`docs/lazy-unfolding-plan.md`]. `Eager` variants behave as
/// today (materialise on every node); `Defer*` variants are
/// only materialised when the trigger fires.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResidualTrigger {
    /// `⊤`, `Atomic(C)`, `And(only atomics)`. Cheap to assert;
    /// no benefit from deferring.
    Eager,
    /// `Or(d1, ..., dn)`. Defer until either:
    /// - the node has a label forcing the Or (e.g. some
    ///   `Not(di)`), or
    /// - saturate's stable-state sweep verifies no `di` is on
    ///   the node and no derivation makes one inevitable.
    DeferOr { disjuncts: Box<[ConceptId]> },
    /// `Not(C)`. Reactive: fires when `C` becomes a label of
    /// the node, immediately producing a clash.
    DeferNot { complement: ConceptId },
    /// `∀R.D`. Reactive: fires on edge creation of role `R`
    /// (incoming for inverse-R, outgoing for named-R). Until
    /// such an edge exists the constraint is vacuous.
    DeferAll { role: Role, body: ConceptId },
    /// `∃R.D` and cardinality (`Min` / `Max`). Real model
    /// commitments — deferring would be unsound under
    /// classical semantics. Same as `Eager` but kept distinct
    /// so a future demand-driven phase can target them.
    EagerExistsOrCardinality,
}

impl ResidualTrigger {
    /// Classify a residual body. Pure function — no graph
    /// access, no side effects.
    #[must_use]
    pub fn classify(body: ConceptId, pool: &ConceptPool) -> Self {
        match pool.get(body) {
            ConceptExpr::Top
            | ConceptExpr::Bot
            | ConceptExpr::Atomic(_)
            | ConceptExpr::Nominal(_)
            | ConceptExpr::SelfRestriction(_) => Self::Eager,
            ConceptExpr::And(args) => {
                // `And(atomic, atomic, ...)` is just a bundle of
                // atomic assertions — Eager. Any non-atomic
                // conjunct could change classification, but for
                // Phase 1 we fold the whole And into Eager and
                // let the conjuncts be re-classified by the
                // saturator naturally.
                let pool_ref = pool;
                let all_eager_atomic = args.iter().all(|&c| {
                    matches!(
                        pool_ref.get(c),
                        ConceptExpr::Atomic(_) | ConceptExpr::Nominal(_) | ConceptExpr::Top
                    )
                });
                if all_eager_atomic {
                    Self::Eager
                } else {
                    // Mixed And — treat as Eager for now; Phase 3
                    // could split per-conjunct. The unsound
                    // alternative (partial deferral) is not in
                    // scope.
                    Self::Eager
                }
            }
            ConceptExpr::Or(args) => Self::DeferOr {
                disjuncts: args.to_vec().into_boxed_slice(),
            },
            ConceptExpr::Not(inner) => Self::DeferNot { complement: *inner },
            ConceptExpr::All(role, inner) => Self::DeferAll {
                role: *role,
                body: *inner,
            },
            ConceptExpr::Some(_, _) | ConceptExpr::Min(_, _, _) | ConceptExpr::Max(_, _, _) => {
                Self::EagerExistsOrCardinality
            }
        }
    }

    /// Returns `true` iff this trigger keeps the residual on the
    /// eager path (materialise on every node). Used by Phase 2's
    /// `apply_residual_gcis` decision.
    #[must_use]
    pub fn is_eager(&self) -> bool {
        matches!(self, Self::Eager | Self::EagerExistsOrCardinality)
    }
}

/// Histogram of `ResidualTrigger` variants across an absorbed
/// `TBox`'s residual GCIs. Returned by [`classify_residuals`] and
/// surfaced via the `rustdl residual-triggers` CLI.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResidualTriggerStats {
    pub total: usize,
    pub eager: usize,
    pub defer_or: usize,
    pub defer_not: usize,
    pub defer_all: usize,
    pub eager_exists_or_cardinality: usize,
}

impl ResidualTriggerStats {
    /// Total count of residuals classified as deferred (any
    /// `Defer*` variant). The win from Phase-2 integration is
    /// bounded above by `deferred / total`.
    #[must_use]
    pub fn deferred(&self) -> usize {
        self.defer_or + self.defer_not + self.defer_all
    }
}

/// Classify every body in `residuals` and return both the
/// per-residual triggers (in input order) and the aggregate
/// histogram.
#[must_use]
pub fn classify_residuals(
    residuals: &[ConceptId],
    pool: &ConceptPool,
) -> (Vec<ResidualTrigger>, ResidualTriggerStats) {
    let mut triggers = Vec::with_capacity(residuals.len());
    let mut stats = ResidualTriggerStats {
        total: residuals.len(),
        ..ResidualTriggerStats::default()
    };
    for &r in residuals {
        let t = ResidualTrigger::classify(r, pool);
        match &t {
            ResidualTrigger::Eager => stats.eager += 1,
            ResidualTrigger::DeferOr { .. } => stats.defer_or += 1,
            ResidualTrigger::DeferNot { .. } => stats.defer_not += 1,
            ResidualTrigger::DeferAll { .. } => stats.defer_all += 1,
            ResidualTrigger::EagerExistsOrCardinality => {
                stats.eager_exists_or_cardinality += 1;
            }
        }
        triggers.push(t);
    }
    (triggers, stats)
}

#[cfg(test)]
#[allow(clippy::many_single_char_names)]
mod tests {
    use super::*;
    use crate::ir::{ClassId, ConceptPool, Role, RoleId};

    #[test]
    fn atomic_body_classifies_as_eager() {
        let mut pool = ConceptPool::new();
        let c = pool.atomic(ClassId::new(0));
        assert_eq!(ResidualTrigger::classify(c, &pool), ResidualTrigger::Eager);
    }

    #[test]
    fn or_body_classifies_as_defer_or() {
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let b = pool.atomic(ClassId::new(1));
        let or_ab = pool.or([a, b]);
        match ResidualTrigger::classify(or_ab, &pool) {
            ResidualTrigger::DeferOr { disjuncts } => {
                assert_eq!(disjuncts.as_ref(), &[a, b] as &[_]);
            }
            other => panic!("expected DeferOr, got {other:?}"),
        }
    }

    #[test]
    fn not_body_classifies_as_defer_not() {
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let not_a = pool.not(a);
        match ResidualTrigger::classify(not_a, &pool) {
            ResidualTrigger::DeferNot { complement } => assert_eq!(complement, a),
            other => panic!("expected DeferNot, got {other:?}"),
        }
    }

    #[test]
    fn all_body_classifies_as_defer_all() {
        let mut pool = ConceptPool::new();
        let r = Role::Named(RoleId::new(0));
        let body = pool.atomic(ClassId::new(0));
        let all = pool.all(r, body);
        match ResidualTrigger::classify(all, &pool) {
            ResidualTrigger::DeferAll { role, body: b } => {
                assert_eq!(role, r);
                assert_eq!(b, body);
            }
            other => panic!("expected DeferAll, got {other:?}"),
        }
    }

    #[test]
    fn exists_body_classifies_as_eager_exists() {
        let mut pool = ConceptPool::new();
        let r = Role::Named(RoleId::new(0));
        let body = pool.atomic(ClassId::new(0));
        let some = pool.some(r, body);
        assert_eq!(
            ResidualTrigger::classify(some, &pool),
            ResidualTrigger::EagerExistsOrCardinality
        );
    }

    #[test]
    fn and_of_atomics_classifies_as_eager() {
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let b = pool.atomic(ClassId::new(1));
        let and_ab = pool.and([a, b]);
        assert_eq!(
            ResidualTrigger::classify(and_ab, &pool),
            ResidualTrigger::Eager
        );
    }

    #[test]
    fn and_with_or_classifies_as_eager_for_now() {
        // Phase 1 treats mixed And as Eager. Phase 3 may split
        // per-conjunct; this test pins the current behaviour
        // so the deferred refactor lands as an intentional
        // change, not a regression.
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let b = pool.atomic(ClassId::new(1));
        let c = pool.atomic(ClassId::new(2));
        let or_bc = pool.or([b, c]);
        let and_a_orbc = pool.and([a, or_bc]);
        assert_eq!(
            ResidualTrigger::classify(and_a_orbc, &pool),
            ResidualTrigger::Eager
        );
    }

    #[test]
    fn classify_residuals_aggregates_correctly() {
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let b = pool.atomic(ClassId::new(1));
        let or_ab = pool.or([a, b]);
        let not_a = pool.not(a);
        let r = Role::Named(RoleId::new(0));
        let all = pool.all(r, a);
        let residuals = vec![a, or_ab, not_a, all];
        let (_triggers, stats) = classify_residuals(&residuals, &pool);
        assert_eq!(stats.total, 4);
        assert_eq!(stats.eager, 1);
        assert_eq!(stats.defer_or, 1);
        assert_eq!(stats.defer_not, 1);
        assert_eq!(stats.defer_all, 1);
        assert_eq!(stats.eager_exists_or_cardinality, 0);
        assert_eq!(stats.deferred(), 3);
    }
}
