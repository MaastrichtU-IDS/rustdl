//! Engine-blind axiom and concept evaluation.
//!
//! Generic over `Interpretation`, resolving concepts only through `ConceptPool`
//! — DATA, not saturation logic. An evaluator sharing code with the saturator
//! could hide the very bug this crate exists to find.
//!
//! NO WILDCARD MATCH ARM over `ConceptExpr` or `Axiom`. An unhandled form is
//! `Unresolved`, never a skip: otherwise "accept" can mean "ignored every form
//! I did not recognise".

use owl_dl_core::{ConceptExpr as CE, ConceptId, ConceptPool, Role};

use crate::interp::{Element, Interpretation};

/// The three-valued outcome of judging a concept membership.
///
/// `Unresolved` is not an error path — it is the honest answer for a concept
/// form this phase cannot judge, and it must never collapse into `True` or
/// `False`: doing so would let an unhandled construct silently pass (or fail)
/// a check it was never actually evaluated against.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Judgement {
    True,
    False,
    Unresolved(&'static str),
}

/// Is element `e` a member of concept `c` under `interp`?
///
/// All 12 `ConceptExpr` variants are listed explicitly, so that adding a 13th
/// variant to the core enum breaks the build rather than silently becoming a
/// skip.
pub fn eval_concept<I: Interpretation>(
    pool: &ConceptPool,
    interp: &I,
    e: Element,
    c: ConceptId,
) -> Judgement {
    match pool.get(c) {
        CE::Top => Judgement::True,
        CE::Bot => Judgement::False,
        CE::Atomic(cls) => {
            if interp.in_concept(e, *cls) {
                Judgement::True
            } else {
                Judgement::False
            }
        }
        CE::And(ops) => {
            // A `False` operand short-circuits to `False` regardless of what
            // the other, possibly-unjudgeable, operands would say. Only if no
            // operand is `False` does an `Unresolved` operand propagate — the
            // conjunction can't be called `True` when part of it wasn't
            // actually judged.
            let mut unresolved = None;
            for op in ops {
                match eval_concept(pool, interp, e, *op) {
                    Judgement::False => return Judgement::False,
                    Judgement::Unresolved(v) => unresolved = Some(v),
                    Judgement::True => {}
                }
            }
            unresolved.map_or(Judgement::True, Judgement::Unresolved)
        }
        CE::Some(Role::Named(r), body) => {
            // A `True` witness short-circuits to `True` — one satisfying
            // successor is enough for an existential. Otherwise, an
            // `Unresolved` body on any successor makes the whole restriction
            // `Unresolved`, not `False`: absence of a confirmed witness is
            // not the same as a confirmed absence of one.
            let mut unresolved = None;
            for t in interp.successors(e, *r) {
                match eval_concept(pool, interp, t, *body) {
                    Judgement::True => return Judgement::True,
                    Judgement::Unresolved(v) => unresolved = Some(v),
                    Judgement::False => {}
                }
            }
            unresolved.map_or(Judgement::False, Judgement::Unresolved)
        }
        CE::Some(Role::Inverse(_), _) => Judgement::Unresolved("Some(Inverse)"),
        CE::Nominal(_) => Judgement::Unresolved("Nominal"),
        CE::SelfRestriction(_) => Judgement::Unresolved("SelfRestriction"),
        CE::Not(_) => Judgement::Unresolved("Not"),
        CE::Or(_) => Judgement::Unresolved("Or"),
        CE::All(_, _) => Judgement::Unresolved("All"),
        CE::Min(_, _, _) => Judgement::Unresolved("Min"),
        CE::Max(_, _, _) => Judgement::Unresolved("Max"),
    }
}
