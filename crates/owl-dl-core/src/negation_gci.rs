//! Canonicalize a negated GCI right-hand side into a lowered-`⊥` GCI.
//!
//! `X ⊑ ¬Y ≡ X ⊓ Y ⊑ ⊥` is an unconditional logical equivalence, so this pass
//! cannot change the entailment set — only which engine answers. Its value is
//! that `is_el_concept` / `is_saturator_concept` reject `ConceptExpr::Not`
//! outright, so a single `A ⊑ ¬B` axiom routes an otherwise-EL ontology onto the
//! O(n²) hybrid path, while `X ⊓ Y ⊑ ⊥` is in-fragment (Lever 1b) and — since the
//! `ConjunctiveUnsat` rule landed — completely reasoned over by the saturator.
//!
//! **This must run BEFORE NNF.** `nnf_axioms` pushes negation to atomic leaves,
//! so post-NNF `X ⊑ ¬(A ⊓ B)` has already become `X ⊑ ¬A ⊔ ¬B` — an `Or`, and the
//! opportunity is gone. Pre-NNF the same axiom becomes `X ⊓ A ⊓ B ⊑ ⊥`, fully
//! EL-positive. Since `nnf_axioms` returns a fresh Vec and leaves
//! `InternalOntology.axioms` untouched, "before NNF" means "a pass over
//! `InternalOntology.axioms`".
//!
//! **Coverage.** The pass handles `Axiom::SubClassOf` only. A `Not` reachable via
//! `EquivalentClasses`, `ObjectPropertyDomain`, `ObjectPropertyRange`, or
//! `ClassAssertion` is not lowered, so those ontologies stay off the saturation
//! fast path. This is an incompleteness only — never an FP.

use crate::ir::{ConceptExpr, ConceptId, ConceptPool};
use crate::ontology::{Axiom, InternalOntology};

/// Is the `RUSTDL_NEG_TO_BOT_GCI` lever enabled? Default ON; `=0` reverts.
fn enabled() -> bool {
    std::env::var("RUSTDL_NEG_TO_BOT_GCI").map_or(true, |val| val != "0")
}

/// Rewrite every `SubClassOf { sub, sup }` whose `sup` is a negation — or an
/// `And` containing one — into the equivalent lowered-`⊥` form.
///
/// Returns the number of **input** axioms rewritten (one per `SubClassOf` whose
/// RHS carried at least one negation). A single input axiom may produce multiple
/// output `⊥`-GCIs when the RHS is `¬A ⊓ ¬B …` — each negated conjunct becomes
/// its own separate `X ⊓ A ⊑ ⊥` / `X ⊓ B ⊑ ⊥` axiom, because
/// `X ⊑ ¬A ⊓ ¬B ≡ (X ⊑ ¬A) ∧ (X ⊑ ¬B)` — NOT `X ⊓ A ⊓ B ⊑ ⊥`.
#[must_use]
pub fn rewrite_negated_supers(onto: &mut InternalOntology) -> usize {
    if !enabled() {
        return 0;
    }
    let mut rewritten = 0usize;
    let mut extra: Vec<Axiom> = Vec::new();
    for i in 0..onto.axioms.len() {
        let Axiom::SubClassOf { sub, sup } = onto.axioms[i] else {
            continue;
        };
        // Split the RHS into its negated and positive parts. A top-level `Not`
        // yields one negated part and no positive part; a top-level `And` is
        // partitioned operand-wise so `X ⊑ A ⊓ ¬B` yields `X ⊑ A` plus
        // `X ⊓ B ⊑ ⊥` (otherwise the negation survives inside the `And` and the
        // axiom stays out-of-fragment).
        let (negated, positive) = split_rhs(sup, &onto.concepts);
        if negated.is_empty() {
            continue;
        }
        let bot_id = onto.concepts.bot();
        // One `⊥`-GCI per negated part: `X ⊓ yᵢ ⊑ ⊥`.
        // CORRECTNESS: `X ⊑ ¬A ⊓ ¬B` is equivalent to BOTH `X ⊓ A ⊑ ⊥` AND
        // `X ⊓ B ⊑ ⊥` independently — merging them into `X ⊓ A ⊓ B ⊑ ⊥` is
        // strictly WEAKER and would silently delete entailments.
        let mut it = negated.into_iter();
        let first = it.next().expect("negated is non-empty");
        let and_id = onto.concepts.and(vec![sub, first]);
        onto.axioms[i] = Axiom::SubClassOf {
            sub: and_id,
            sup: bot_id,
        };
        for y in it {
            let and_id = onto.concepts.and(vec![sub, y]);
            extra.push(Axiom::SubClassOf {
                sub: and_id,
                sup: bot_id,
            });
        }
        // `X ⊑ pᵢ` for each surviving positive part.
        for p in positive {
            extra.push(Axiom::SubClassOf { sub, sup: p });
        }
        rewritten += 1;
    }
    onto.axioms.extend(extra);
    rewritten
}

/// Partition a GCI right-hand side into (inner concepts of negated parts,
/// positive parts). `¬Y` contributes `Y` to the first list; an `And` is
/// partitioned operand-wise; anything else is a single positive part.
///
/// **Invariant:** `ConceptExpr::And` is only constructed by [`ConceptPool::and`]
/// (`crates/owl-dl-core/src/ir.rs`), which FLATTENS nested `And`s, so a non-flat
/// `And` cannot exist in the pool and the `And` arm here sees only a flat list of
/// operands. This means `Not(A ⊓ B)` is a `Not` whose inner is an `And` — it
/// arrives at the `Not` arm and returns `(vec![inner_id], vec![])` (one negated
/// part), NOT as an `And` of `Not`s. The two input shapes (`¬A ⊓ ¬B` vs
/// `¬(A ⊓ B)`) therefore produce different output: two axioms vs one, correctly.
///
/// **`Not(Top)` identity:** `and([sub, top_id])` drops `Top` (it is the identity
/// of `And`), so `X ⊑ ¬⊤` → `and([X, ⊤])` = `X` → `X ⊑ ⊥`. No explicit code
/// needed for this case.
fn split_rhs(sup: ConceptId, pool: &ConceptPool) -> (Vec<ConceptId>, Vec<ConceptId>) {
    match pool.get(sup) {
        ConceptExpr::Not(inner) => (vec![*inner], Vec::new()),
        ConceptExpr::And(ops) => {
            let mut neg = Vec::new();
            let mut pos = Vec::new();
            for &op in ops {
                if let ConceptExpr::Not(inner) = pool.get(op) {
                    neg.push(*inner);
                } else {
                    pos.push(op);
                }
            }
            (neg, pos)
        }
        _ => (Vec::new(), Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::ConceptExpr;

    // All tests that call `rewrite_negated_supers` (or check `enabled()`) must
    // hold this lock so `flag_off_reverts` cannot race them.
    static NEG_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// RAII guard that sets `RUSTDL_NEG_TO_BOT_GCI` to a value on construction
    /// and restores the prior value (or removes the var) on drop.
    ///
    /// SAFETY: `set_var`/`remove_var` is `unsafe` under edition 2024; serialised by
    /// `NEG_ENV_LOCK` (this module's tests only — other modules mutate the
    /// process env under their own locks such as `DP_ENV_MUTEX` in
    /// `convert.rs`; this guard does NOT serialise against those).
    struct NegGuard {
        prior: Option<std::ffi::OsString>,
    }
    impl NegGuard {
        #[allow(unsafe_code)]
        fn off() -> Self {
            let prior = std::env::var_os("RUSTDL_NEG_TO_BOT_GCI");
            // SAFETY: serialised by NEG_ENV_LOCK; restored on Drop.
            unsafe { std::env::set_var("RUSTDL_NEG_TO_BOT_GCI", "0") };
            Self { prior }
        }
    }
    impl Drop for NegGuard {
        #[allow(unsafe_code)]
        fn drop(&mut self) {
            // SAFETY: see NegGuard::off.
            unsafe {
                match &self.prior {
                    Some(v) => std::env::set_var("RUSTDL_NEG_TO_BOT_GCI", v),
                    None => std::env::remove_var("RUSTDL_NEG_TO_BOT_GCI"),
                }
            }
        }
    }

    /// `A ⊑ ¬B` becomes `A ⊓ B ⊑ ⊥`.
    #[test]
    fn atomic_negation_becomes_bot_gci() {
        let _g = NEG_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut o = InternalOntology::new();
        let a = o.vocabulary.intern_class("http://t/A");
        let b = o.vocabulary.intern_class("http://t/B");
        let a_c = o.concepts.atomic(a);
        let b_c = o.concepts.atomic(b);
        let not_b = o.concepts.not(b_c);
        o.axioms.push(Axiom::SubClassOf {
            sub: a_c,
            sup: not_b,
        });

        assert_eq!(rewrite_negated_supers(&mut o), 1);
        assert_eq!(o.axioms.len(), 1);
        let Axiom::SubClassOf { sub, sup } = o.axioms[0] else {
            panic!("expected SubClassOf");
        };
        assert!(matches!(o.concepts.get(sup), ConceptExpr::Bot));
        let ConceptExpr::And(ops) = o.concepts.get(sub) else {
            panic!("expected And LHS, got {:?}", o.concepts.get(sub));
        };
        let mut got: Vec<ConceptId> = ops.to_vec();
        got.sort();
        let mut want = vec![a_c, b_c];
        want.sort();
        assert_eq!(got, want, "LHS must be A ⊓ B");
    }

    /// `X ⊑ A ⊓ ¬B` becomes `X ⊓ B ⊑ ⊥` PLUS `X ⊑ A` — otherwise the negation
    /// survives inside the `And` and the axiom stays out-of-fragment.
    #[test]
    fn conjunctive_rhs_splits_positive_and_negated() {
        let _g = NEG_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut o = InternalOntology::new();
        let x = o.vocabulary.intern_class("http://t/X");
        let a = o.vocabulary.intern_class("http://t/A");
        let b = o.vocabulary.intern_class("http://t/B");
        let x_c = o.concepts.atomic(x);
        let a_c = o.concepts.atomic(a);
        let b_c = o.concepts.atomic(b);
        let not_b = o.concepts.not(b_c);
        let rhs = o.concepts.and(vec![a_c, not_b]);
        o.axioms.push(Axiom::SubClassOf { sub: x_c, sup: rhs });

        assert_eq!(rewrite_negated_supers(&mut o), 1);
        assert_eq!(o.axioms.len(), 2, "one ⊥-GCI plus one positive GCI");
        let has_positive = o
            .axioms
            .iter()
            .any(|ax| matches!(ax, Axiom::SubClassOf { sub, sup } if *sub == x_c && *sup == a_c));
        assert!(has_positive, "X ⊑ A must survive as its own axiom");
        // The ⊥-GCI's LHS must be exactly {X, B} — not {X, A, B}.
        let bot = o.concepts.bot();
        let bot_gci = o
            .axioms
            .iter()
            .find(|ax| matches!(ax, Axiom::SubClassOf { sup, .. } if *sup == bot))
            .expect("must have a ⊥-GCI");
        let Axiom::SubClassOf { sub, .. } = bot_gci else {
            unreachable!()
        };
        let ConceptExpr::And(ops) = o.concepts.get(*sub) else {
            panic!("⊥-GCI LHS must be And, got {:?}", o.concepts.get(*sub));
        };
        let mut got: Vec<ConceptId> = ops.to_vec();
        got.sort();
        let mut want = vec![x_c, b_c];
        want.sort();
        assert_eq!(got, want, "⊥-GCI LHS must be exactly {{X, B}}");
    }

    /// A negation-free RHS is untouched.
    #[test]
    fn positive_axioms_are_inert() {
        let _g = NEG_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut o = InternalOntology::new();
        let a = o.vocabulary.intern_class("http://t/A");
        let b = o.vocabulary.intern_class("http://t/B");
        let a_c = o.concepts.atomic(a);
        let b_c = o.concepts.atomic(b);
        o.axioms.push(Axiom::SubClassOf { sub: a_c, sup: b_c });
        let before = o.axioms.clone();

        assert_eq!(rewrite_negated_supers(&mut o), 0);
        assert_eq!(o.axioms, before, "no negation ⇒ no change");
    }

    /// A negation on the LEFT is NOT touched: `¬A ⊑ B` is a covering axiom
    /// (`⊤ ⊑ A ⊔ B`), not a disjointness, and rewriting it would be wrong.
    #[test]
    fn left_hand_negation_is_untouched() {
        let _g = NEG_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut o = InternalOntology::new();
        let a = o.vocabulary.intern_class("http://t/A");
        let b = o.vocabulary.intern_class("http://t/B");
        let a_c = o.concepts.atomic(a);
        let b_c = o.concepts.atomic(b);
        let not_a = o.concepts.not(a_c);
        o.axioms.push(Axiom::SubClassOf {
            sub: not_a,
            sup: b_c,
        });
        let before = o.axioms.clone();

        assert_eq!(rewrite_negated_supers(&mut o), 0);
        assert_eq!(o.axioms, before, "LHS negation is a covering axiom");
    }

    /// `X ⊑ ¬A ⊓ ¬B` ⟹ TWO `⊥`-GCIs: `{X,A} ⊑ ⊥` and `{X,B} ⊑ ⊥`.
    ///
    /// This is the direct regression guard for the Critical fix. The WRONG
    /// pre-fix behaviour emits ONE `⊥`-GCI `{X,A,B} ⊑ ⊥`, which is strictly
    /// weaker.
    #[test]
    fn two_negated_conjuncts_yields_two_bot_gcis() {
        let _g = NEG_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut o = InternalOntology::new();
        let x = o.vocabulary.intern_class("http://t/X");
        let a = o.vocabulary.intern_class("http://t/A");
        let b = o.vocabulary.intern_class("http://t/B");
        let x_c = o.concepts.atomic(x);
        let a_c = o.concepts.atomic(a);
        let b_c = o.concepts.atomic(b);
        let not_a = o.concepts.not(a_c);
        let not_b = o.concepts.not(b_c);
        let rhs = o.concepts.and(vec![not_a, not_b]);
        o.axioms.push(Axiom::SubClassOf { sub: x_c, sup: rhs });

        assert_eq!(
            rewrite_negated_supers(&mut o),
            1,
            "one INPUT axiom rewritten"
        );
        assert_eq!(o.axioms.len(), 2, "must produce TWO ⊥-GCIs");
        let bot = o.concepts.bot();
        for ax in &o.axioms {
            let Axiom::SubClassOf { sup, .. } = ax else {
                panic!("expected SubClassOf");
            };
            assert_eq!(*sup, bot, "all output axioms must have ⊥ RHS");
        }
        // Each ⊥-GCI's LHS must be exactly one of {X,A} or {X,B}.
        let find_lhs = |needle_a: ConceptId, needle_b: ConceptId| {
            o.axioms.iter().any(|ax| {
                let Axiom::SubClassOf { sub, .. } = ax else {
                    return false;
                };
                let ConceptExpr::And(ops) = o.concepts.get(*sub) else {
                    return false;
                };
                let mut got: Vec<ConceptId> = ops.to_vec();
                got.sort();
                let mut want = vec![needle_a, needle_b];
                want.sort();
                got == want
            })
        };
        assert!(find_lhs(x_c, a_c), "must have ⊥-GCI with LHS {{X,A}}");
        assert!(find_lhs(x_c, b_c), "must have ⊥-GCI with LHS {{X,B}}");
    }

    /// `X ⊑ ¬(A ⊓ B)` ⟹ ONE `⊥`-GCI whose LHS is `{X,A,B}`.
    ///
    /// This pins the two shapes `¬A ⊓ ¬B` and `¬(A ⊓ B)` APART: pre-NNF the
    /// inner `A ⊓ B` is still an `And`, so `and([X, A⊓B])` flattens to
    /// `{X,A,B}` via `ConceptPool::and`'s flatten rule. One axiom, not two.
    #[test]
    fn not_conjunction_produces_single_three_way_bot_gci() {
        let _g = NEG_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut o = InternalOntology::new();
        let x = o.vocabulary.intern_class("http://t/X");
        let a = o.vocabulary.intern_class("http://t/A");
        let b = o.vocabulary.intern_class("http://t/B");
        let x_c = o.concepts.atomic(x);
        let a_c = o.concepts.atomic(a);
        let b_c = o.concepts.atomic(b);
        let a_and_b = o.concepts.and(vec![a_c, b_c]);
        let not_a_and_b = o.concepts.not(a_and_b);
        o.axioms.push(Axiom::SubClassOf {
            sub: x_c,
            sup: not_a_and_b,
        });

        assert_eq!(
            rewrite_negated_supers(&mut o),
            1,
            "one INPUT axiom rewritten"
        );
        assert_eq!(o.axioms.len(), 1, "must produce exactly ONE ⊥-GCI");
        let Axiom::SubClassOf { sub, sup } = o.axioms[0] else {
            panic!("expected SubClassOf");
        };
        assert!(matches!(o.concepts.get(sup), ConceptExpr::Bot));
        let ConceptExpr::And(ops) = o.concepts.get(sub) else {
            panic!("LHS must be And, got {:?}", o.concepts.get(sub));
        };
        let mut got: Vec<ConceptId> = ops.to_vec();
        got.sort();
        let mut want = vec![x_c, a_c, b_c];
        want.sort();
        assert_eq!(got, want, "LHS must be exactly {{X,A,B}}");
    }

    /// `X ⊑ A ⊓ ¬B ⊓ ¬C` ⟹ two `⊥`-GCIs (`{X,B}`, `{X,C}`) plus `X ⊑ A`.
    #[test]
    fn mixed_three_conjuncts_yields_two_bot_gcis_and_one_positive() {
        let _g = NEG_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut onto = InternalOntology::new();
        let ix = onto.vocabulary.intern_class("http://t/X");
        let ia = onto.vocabulary.intern_class("http://t/A");
        let ib = onto.vocabulary.intern_class("http://t/B");
        let ic = onto.vocabulary.intern_class("http://t/C");
        let x_c = onto.concepts.atomic(ix);
        let a_c = onto.concepts.atomic(ia);
        let b_c = onto.concepts.atomic(ib);
        let c_c = onto.concepts.atomic(ic);
        let not_b = onto.concepts.not(b_c);
        let not_c = onto.concepts.not(c_c);
        let rhs = onto.concepts.and(vec![a_c, not_b, not_c]);
        onto.axioms.push(Axiom::SubClassOf { sub: x_c, sup: rhs });

        assert_eq!(
            rewrite_negated_supers(&mut onto),
            1,
            "one INPUT axiom rewritten"
        );
        assert_eq!(onto.axioms.len(), 3, "two ⊥-GCIs plus one positive GCI");
        // X ⊑ A must be present.
        let has_positive = onto
            .axioms
            .iter()
            .any(|ax| matches!(ax, Axiom::SubClassOf { sub, sup } if *sub == x_c && *sup == a_c));
        assert!(has_positive, "X ⊑ A must survive");
        // {X,B} ⊑ ⊥ and {X,C} ⊑ ⊥ must each be present.
        let find_lhs = |needle_a: ConceptId, needle_b: ConceptId| {
            onto.axioms.iter().any(|ax| {
                let Axiom::SubClassOf { sub, sup } = ax else {
                    return false;
                };
                if !matches!(onto.concepts.get(*sup), ConceptExpr::Bot) {
                    return false;
                }
                let ConceptExpr::And(ops) = onto.concepts.get(*sub) else {
                    return false;
                };
                let mut got: Vec<ConceptId> = ops.to_vec();
                got.sort();
                let mut want = vec![needle_a, needle_b];
                want.sort();
                got == want
            })
        };
        assert!(find_lhs(x_c, b_c), "must have ⊥-GCI with LHS {{X,B}}");
        assert!(find_lhs(x_c, c_c), "must have ⊥-GCI with LHS {{X,C}}");
    }

    /// `X ⊑ ¬⊤` ⟹ `X ⊑ ⊥` (Top-identity collapse via `ConceptPool::and`).
    #[test]
    fn negated_top_becomes_x_subsumes_bot() {
        let _g = NEG_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut o = InternalOntology::new();
        let x = o.vocabulary.intern_class("http://t/X");
        let x_c = o.concepts.atomic(x);
        let top = o.concepts.top();
        let not_top = o.concepts.not(top);
        o.axioms.push(Axiom::SubClassOf {
            sub: x_c,
            sup: not_top,
        });

        assert_eq!(rewrite_negated_supers(&mut o), 1);
        assert_eq!(o.axioms.len(), 1);
        let Axiom::SubClassOf { sub, sup } = o.axioms[0] else {
            panic!("expected SubClassOf");
        };
        // and([X, Top]) drops Top (identity) → X alone → sub == x_c.
        assert_eq!(sub, x_c, "LHS must collapse to just X (Top identity)");
        assert!(
            matches!(o.concepts.get(sup), ConceptExpr::Bot),
            "RHS must be ⊥"
        );
    }

    /// The flag reverts the pass. Serialised because it mutates the process env.
    #[test]
    fn flag_off_reverts() {
        let _g = NEG_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _env = NegGuard::off();

        let mut o = InternalOntology::new();
        let a = o.vocabulary.intern_class("http://t/A");
        let b = o.vocabulary.intern_class("http://t/B");
        let a_c = o.concepts.atomic(a);
        let b_c = o.concepts.atomic(b);
        let not_b = o.concepts.not(b_c);
        o.axioms.push(Axiom::SubClassOf {
            sub: a_c,
            sup: not_b,
        });
        let before = o.axioms.clone();
        let rewritten = rewrite_negated_supers(&mut o);
        assert_eq!(rewritten, 0);
        assert_eq!(o.axioms, before);
    }
}
