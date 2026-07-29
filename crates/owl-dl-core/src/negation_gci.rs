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

use crate::ir::{ConceptExpr, ConceptId};
use crate::ontology::{Axiom, InternalOntology};

/// Is the `RUSTDL_NEG_TO_BOT_GCI` lever enabled? Default ON; `=0` reverts.
fn enabled() -> bool {
    std::env::var("RUSTDL_NEG_TO_BOT_GCI").map_or(true, |v| v != "0")
}

/// Rewrite every `SubClassOf { sub, sup }` whose `sup` is a negation — or an
/// `And` containing one — into the equivalent lowered-`⊥` form. Returns the
/// number of axioms rewritten.
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
        // `X ⊓ y₁ ⊓ … ⊓ yₙ ⊑ ⊥` for the negated parts.
        let mut conj = vec![sub];
        conj.extend(negated);
        let and_id = onto.concepts.and(conj);
        let bot_id = onto.concepts.bot();
        onto.axioms[i] = Axiom::SubClassOf {
            sub: and_id,
            sup: bot_id,
        };
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
fn split_rhs(sup: ConceptId, pool: &crate::ir::ConceptPool) -> (Vec<ConceptId>, Vec<ConceptId>) {
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

    /// The flag reverts the pass. Serialised because it mutates the process env.
    #[test]
    #[allow(unsafe_code, clippy::many_single_char_names)]
    fn flag_off_reverts() {
        let _g = NEG_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let prev = std::env::var_os("RUSTDL_NEG_TO_BOT_GCI");
        // SAFETY: set_var is unsafe under edition 2024; serialised by NEG_ENV_LOCK
        // and restored below.
        unsafe { std::env::set_var("RUSTDL_NEG_TO_BOT_GCI", "0") };

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
        let n = rewrite_negated_supers(&mut o);

        // SAFETY: see above.
        unsafe {
            match &prev {
                Some(v) => std::env::set_var("RUSTDL_NEG_TO_BOT_GCI", v),
                None => std::env::remove_var("RUSTDL_NEG_TO_BOT_GCI"),
            }
        }
        assert_eq!(n, 0);
        assert_eq!(o.axioms, before);
    }
}
