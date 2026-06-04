//! Sound ABox-driven ontology-level inconsistency pre-check.
//!
//! Runs after `collect_abox` and the EL saturator (so both the
//! `Abox` struct and `Subsumers` closure are available). Returns
//! [`AboxVerdict::Inconsistent`] on a detected clash;
//! [`AboxVerdict::Unknown`] otherwise. The caller falls through to
//! the existing tableau path on `Unknown`.
//!
//! Sound under-approximation: every positive verdict is a direct
//! semantic clash on the ABox; no inferred subsumption is created.
//!
//! Seven clash patterns implemented incrementally (P1 direct-Bot
//! assertion, P2 disjoint types per individual, P3 NegOPA-vs-OPA,
//! P4 SameAs∩DifferentFrom, P5 Functional+two-distinct-witnesses,
//! P6 Asymmetric/Irreflexive violations, P7 domain/range as a
//! stretch).
//!
//! Spec: `docs/superpowers/specs/2026-06-04-abox-consistency-check-design.md`

use owl_dl_core::ir::{ClassId, IndividualId, RoleId};

/// Verdict from the ABox consistency check.
///
/// Sound under-approximation: `Inconsistent` is unconditional;
/// `Unknown` means "we couldn't catch a clash with the cheap
/// patterns" — caller should fall through to the full tableau.
#[derive(Debug, Clone)]
pub(crate) enum AboxVerdict {
    Inconsistent { reason: ClashReason },
    Unknown,
}

/// The specific clash the check detected. Surfaced in `RUSTDL_TRACE`
/// output and intended for a future `consistent --explain` extension
/// (not part of this project's scope).
#[derive(Debug, Clone)]
pub(crate) enum ClashReason {
    /// P1: `ClassAssertion(C, a)` with `Subsumers::is_unsatisfiable(C)`.
    AssertedBot { individual: IndividualId, class: ClassId },
    /// P2 / P7: individual `a` has both `c` and `d` in its asserted-
    /// or-derived type set, and `(c, d)` is in `told_disjoint_pairs`.
    DisjointTypes { individual: IndividualId, c: ClassId, d: ClassId },
    /// P3: positive `R(a, b)` and `NegativeObjectPropertyAssertion(R, a, b)`.
    NegOpaConflict { from: IndividualId, role: RoleId, to: IndividualId },
    /// P4 / P5: `(a, b)` in `DifferentIndividuals` and union-find
    /// (post-`SameIndividual` and post-functional-merges) finds them equal.
    SameDifferent { a: IndividualId, b: IndividualId },
    /// P5 detail: `Functional(R) ∧ R(a, b1) ∧ R(a, b2)` forced a
    /// merge of `b1` and `b2` that subsequently clashed with a
    /// `DifferentIndividuals` declaration.
    FunctionalDiff { role: RoleId, a: IndividualId, b1: IndividualId, b2: IndividualId },
    /// P6: `Asymmetric(R) ∧ R(a, b) ∧ R(b, a)`.
    AsymmetricViolation { role: RoleId, a: IndividualId, b: IndividualId },
    /// P6: `Irreflexive(R) ∧ R(a, a)` (or `R(a, b)` with `a ≡ b` after merge).
    IrreflexiveViolation { role: RoleId, a: IndividualId },
}

/// Entry point. Runs all implemented clash patterns and returns the first
/// detected clash, or [`AboxVerdict::Unknown`] if none fire.
pub(crate) fn check(prepared: &crate::PreparedOntology) -> AboxVerdict {
    // Early return: no individuals → no ABox → no clash possible.
    if prepared.abox.individuals.is_empty() {
        return AboxVerdict::Unknown;
    }
    let closure = &prepared.closure;
    let pool = &prepared.pool;
    // P1: direct-⊥ assertion. For each ClassAssertion(C, a), if
    // C = Atomic(c) and the EL saturator deems `c` unsatisfiable,
    // the ABox is inconsistent.
    for &(individual, class_concept) in &prepared.abox.class_assertions {
        if let owl_dl_core::ir::ConceptExpr::Atomic(c) = pool.get(class_concept) {
            if closure.is_unsatisfiable(*c) {
                return AboxVerdict::Inconsistent {
                    reason: ClashReason::AssertedBot { individual, class: *c },
                };
            }
        }
    }
    AboxVerdict::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;
    use owl_dl_core::ir::ConceptPool;
    use owl_dl_core::ontology::InternalOntology;
    use owl_dl_core::vocab::Vocabulary;

    #[test]
    fn skeleton_returns_unknown_for_empty_abox() {
        // Build the tiniest InternalOntology (no axioms), wrap in
        // PreparedOntology, and confirm the skeleton check returns
        // Unknown. This guards the entry-point signature; pattern
        // tests live in tests/abox_consistency.rs.
        let internal = InternalOntology {
            vocabulary: Vocabulary::default(),
            concepts: ConceptPool::default(),
            axioms: Vec::new(),
        };
        let prepared = crate::PreparedOntology::from_internal(internal)
            .expect("empty ontology prepares");
        assert!(matches!(check(&prepared), AboxVerdict::Unknown));
    }
}
