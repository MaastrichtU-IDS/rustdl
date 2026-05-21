//! Structural transformation (Tseitin-style fresh-name introduction).
//!
//! ## Scope in Phase 1 v0
//!
//! This module is a **deliberate stub**. It exposes a stable
//! [`transform`] function that the Phase 1 pipeline (`nnf -> told ->
//! definitions -> absorb -> transform`) can call today, even though the
//! current body is the identity. Phase 4 will fill it in.
//!
//! ## Why deferred
//!
//! Tseitin-style transformation introduces fresh atomic class names for
//! complex sub-concepts that recur frequently — converting axioms like
//! `A ⊑ ∃R.(C ⊓ D ⊓ E)` into `A ⊑ ∃R.X` plus `X ≡ C ⊓ D ⊓ E`. The
//! tableau then labels nodes with the fresh `X` instead of carrying the
//! conjunction.
//!
//! For the **standard tableau** we picked in strategy v2 §3.2, two
//! pre-existing pieces of work already do most of what Tseitin buys:
//!
//! 1. [`crate::ConceptPool`] interns every distinct sub-expression to
//!    a single [`crate::ConceptId`]. Logical equality is O(1) integer
//!    comparison; sub-concept sharing is automatic.
//! 2. [`crate::absorb`] converts most `⊤ ⊑ φ` axioms into triggers —
//!    `ConceptRule` / `NominalRule` / `RoleRule` — that fire only when
//!    needed, regardless of the structural complexity of φ.
//!
//! [`crate::Definitions`] (lazy unfolding) handles the remaining
//! case where keeping a named atom in labels is preferable to expanding
//! it.
//!
//! ## Phase 4 plan
//!
//! When this is revisited:
//!
//! - Walk every `ConceptId` reachable from the absorbed `TBox`.
//! - Identify sub-expressions appearing in `N ≥ k` distinct contexts.
//! - Synthesize a fresh class IRI in a `_TS_n` namespace, add to the
//!   vocabulary, and emit a [`crate::Definitions`] entry binding it to
//!   the body.
//! - Rewrite the absorbed `TBox` to use the fresh name in place of the
//!   original sub-expression.
//!
//! Implementation will live here, with the existing [`transform`] entry
//! point evolving to do the work (or returning a richer struct that
//! also carries the new vocabulary / definition entries).

use crate::absorb::AbsorbedTBox;

/// Phase 1 v0: identity. See module-level docs for what Phase 4 will
/// add.
#[must_use]
pub fn transform(tbox: AbsorbedTBox) -> AbsorbedTBox {
    tbox
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ConceptPool;
    use crate::absorb::{AbsorbedTBox, ConceptRule};
    use crate::ir::ClassId;

    #[test]
    fn identity_preserves_concept_rules() {
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let input = AbsorbedTBox {
            concept_rules: vec![ConceptRule {
                trigger: ClassId::new(1),
                conclusion: a,
            }],
            ..AbsorbedTBox::default()
        };
        let cloned = input.clone();
        let output = transform(input);
        assert_eq!(output, cloned);
    }

    #[test]
    fn identity_preserves_empty_tbox() {
        let output = transform(AbsorbedTBox::default());
        assert_eq!(output, AbsorbedTBox::default());
    }
}
