//! Normalization passes that prepare an ontology for reasoning.
//!
//! Phase 1 of the strategy. This module provides:
//!
//! - [`to_nnf`] — Negation Normal Form, pushing `Not` to atomic positions.
//! - [`nnf_axioms`] — apply NNF to every concept embedded in every axiom of
//!   an ontology, returning a fresh `Vec<Axiom>` with the original axiom
//!   list untouched.
//!
//! Coming next: absorption (binary, role, nominal) and the structural
//! (Tseitin-style) transformation.

use crate::ConceptPool;
use crate::ir::{ConceptExpr, ConceptId};
use crate::ontology::{Axiom, InternalOntology};

/// Rewrite a concept into Negation Normal Form: every `Not` is pushed down
/// until it wraps an atomic concept (`Atomic`, `Nominal`, or
/// `SelfRestriction`). Pure transformation — the input pool is mutated only
/// by interning the new sub-expressions.
///
/// The standard SROIQ NNF rules:
///
/// | input            | NNF                       |
/// |------------------|---------------------------|
/// | `¬⊤`             | `⊥`                       |
/// | `¬⊥`             | `⊤`                       |
/// | `¬¬C`            | `nnf(C)`                  |
/// | `¬(C ⊓ D)`       | `nnf(¬C) ⊔ nnf(¬D)`       |
/// | `¬(C ⊔ D)`       | `nnf(¬C) ⊓ nnf(¬D)`       |
/// | `¬∃R.C`          | `∀R. nnf(¬C)`             |
/// | `¬∀R.C`          | `∃R. nnf(¬C)`             |
/// | `¬(≥0 R.C)`      | `⊥` (≥0 is satisfied by everything) |
/// | `¬(≥n R.C)`, n≥1 | `≤(n-1) R. nnf(C)`        |
/// | `¬(≤n R.C)`      | `≥(n+1) R. nnf(C)`        |
///
/// In the cardinality cases the inner `C` is *not* negated: the restriction
/// itself is what's flipped. `C` is re-normalized so any deeper negations
/// surface to leaves.
#[must_use]
pub fn to_nnf(cid: ConceptId, pool: &mut ConceptPool) -> ConceptId {
    let expr = pool.get(cid).clone();
    match expr {
        ConceptExpr::Top
        | ConceptExpr::Bot
        | ConceptExpr::Atomic(_)
        | ConceptExpr::Nominal(_)
        | ConceptExpr::SelfRestriction(_) => cid,
        ConceptExpr::Not(inner) => push_negation_in(inner, pool),
        ConceptExpr::And(args) => {
            let normalized: Vec<ConceptId> = args.iter().map(|&c| to_nnf(c, pool)).collect();
            pool.and(normalized)
        }
        ConceptExpr::Or(args) => {
            let normalized: Vec<ConceptId> = args.iter().map(|&c| to_nnf(c, pool)).collect();
            pool.or(normalized)
        }
        ConceptExpr::Some(role, c) => {
            let c_nnf = to_nnf(c, pool);
            pool.some(role, c_nnf)
        }
        ConceptExpr::All(role, c) => {
            let c_nnf = to_nnf(c, pool);
            pool.all(role, c_nnf)
        }
        ConceptExpr::Min(n, role, c) => {
            let c_nnf = to_nnf(c, pool);
            pool.min(n, role, c_nnf)
        }
        ConceptExpr::Max(n, role, c) => {
            let c_nnf = to_nnf(c, pool);
            pool.max(n, role, c_nnf)
        }
    }
}

/// Compute `nnf(¬C)` given `C` (already assumed NNF or convertible).
/// Exposed for the tableau's choose rule, which needs the NNF
/// complement of a concept when branching on `≤n R.C`.
#[must_use]
pub fn nnf_complement(cid: ConceptId, pool: &mut ConceptPool) -> ConceptId {
    push_negation_in(cid, pool)
}

/// Helper: compute `nnf(¬C)` given the `C` (not its negation).
fn push_negation_in(cid: ConceptId, pool: &mut ConceptPool) -> ConceptId {
    let expr = pool.get(cid).clone();
    match expr {
        ConceptExpr::Top => pool.bot(),
        ConceptExpr::Bot => pool.top(),
        ConceptExpr::Atomic(_) | ConceptExpr::Nominal(_) | ConceptExpr::SelfRestriction(_) => {
            pool.not(cid)
        }
        // ¬¬C = nnf(C)
        ConceptExpr::Not(inner) => to_nnf(inner, pool),
        // ¬(C ⊓ D) = nnf(¬C) ⊔ nnf(¬D)
        ConceptExpr::And(args) => {
            let negated: Vec<ConceptId> = args.iter().map(|&c| push_negation_in(c, pool)).collect();
            pool.or(negated)
        }
        // ¬(C ⊔ D) = nnf(¬C) ⊓ nnf(¬D)
        ConceptExpr::Or(args) => {
            let negated: Vec<ConceptId> = args.iter().map(|&c| push_negation_in(c, pool)).collect();
            pool.and(negated)
        }
        // ¬∃R.C = ∀R. nnf(¬C)
        ConceptExpr::Some(role, c) => {
            let c_neg = push_negation_in(c, pool);
            pool.all(role, c_neg)
        }
        // ¬∀R.C = ∃R. nnf(¬C)
        ConceptExpr::All(role, c) => {
            let c_neg = push_negation_in(c, pool);
            pool.some(role, c_neg)
        }
        // ¬(≥0 R.C) = ⊥  (any individual has at least 0 R-successors)
        // ¬(≥n R.C) = ≤(n-1) R. nnf(C),  for n ≥ 1
        ConceptExpr::Min(n, role, c) => {
            if n == 0 {
                pool.bot()
            } else {
                let c_nnf = to_nnf(c, pool);
                pool.max(n - 1, role, c_nnf)
            }
        }
        // ¬(≤n R.C) = ≥(n+1) R. nnf(C)
        ConceptExpr::Max(n, role, c) => {
            let c_nnf = to_nnf(c, pool);
            pool.min(n + 1, role, c_nnf)
        }
    }
}

/// Check the NNF invariant: in a properly normalized concept tree, every
/// `Not` directly wraps an `Atomic`, `Nominal`, or `SelfRestriction`.
#[must_use]
pub fn is_nnf(cid: ConceptId, pool: &ConceptPool) -> bool {
    match pool.get(cid) {
        ConceptExpr::Top
        | ConceptExpr::Bot
        | ConceptExpr::Atomic(_)
        | ConceptExpr::Nominal(_)
        | ConceptExpr::SelfRestriction(_) => true,
        ConceptExpr::Not(inner) => matches!(
            pool.get(*inner),
            ConceptExpr::Atomic(_) | ConceptExpr::Nominal(_) | ConceptExpr::SelfRestriction(_)
        ),
        ConceptExpr::And(args) | ConceptExpr::Or(args) => args.iter().all(|&c| is_nnf(c, pool)),
        ConceptExpr::Some(_, c)
        | ConceptExpr::All(_, c)
        | ConceptExpr::Min(_, _, c)
        | ConceptExpr::Max(_, _, c) => is_nnf(*c, pool),
    }
}

/// Apply NNF to every concept embedded in the ontology's axioms, producing
/// a fresh `Vec<Axiom>`. The original `ontology.axioms` is left untouched;
/// only `ontology.concepts` may grow with newly interned sub-expressions.
///
/// The pipeline pattern matches told/definitions: source ontology in,
/// derived view out. Absorption and structural transformation downstream
/// consume the result.
pub fn nnf_axioms(ontology: &mut InternalOntology) -> Vec<Axiom> {
    // Disjoint field access: read `axioms`, mutate `concepts`.
    let axioms = &ontology.axioms;
    let pool = &mut ontology.concepts;
    let mut out = Vec::with_capacity(axioms.len());
    for ax in axioms {
        out.push(nnf_axiom(ax, pool));
    }
    out
}

/// Apply NNF to every `ConceptId` field of a single [`Axiom`]. Axioms with
/// no concept fields (role characteristics, `ABox` assertions, declarations,
/// etc.) are returned unchanged.
fn nnf_axiom(ax: &Axiom, pool: &mut ConceptPool) -> Axiom {
    match ax {
        Axiom::SubClassOf { sub, sup } => Axiom::SubClassOf {
            sub: to_nnf(*sub, pool),
            sup: to_nnf(*sup, pool),
        },
        Axiom::EquivalentClasses(ids) => {
            Axiom::EquivalentClasses(ids.iter().map(|&c| to_nnf(c, pool)).collect())
        }
        Axiom::DisjointClasses(ids) => {
            Axiom::DisjointClasses(ids.iter().map(|&c| to_nnf(c, pool)).collect())
        }
        Axiom::DisjointUnion { class, members } => Axiom::DisjointUnion {
            class: *class,
            members: members.iter().map(|&c| to_nnf(c, pool)).collect(),
        },
        Axiom::ObjectPropertyDomain { role, domain } => Axiom::ObjectPropertyDomain {
            role: *role,
            domain: to_nnf(*domain, pool),
        },
        Axiom::ObjectPropertyRange { role, range } => Axiom::ObjectPropertyRange {
            role: *role,
            range: to_nnf(*range, pool),
        },
        Axiom::ClassAssertion { class, individual } => Axiom::ClassAssertion {
            class: to_nnf(*class, pool),
            individual: *individual,
        },
        // All other variants — RBox characteristics, role hierarchies, ABox
        // assertions, declarations — have no embedded ConceptId.
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{ClassId, IndividualId, Role, RoleId};

    fn pool() -> ConceptPool {
        ConceptPool::new()
    }

    #[test]
    fn nnf_of_atomic_is_identity() {
        let mut p = pool();
        let a = p.atomic(ClassId::new(0));
        assert_eq!(to_nnf(a, &mut p), a);
    }

    #[test]
    fn nnf_of_not_top_is_bot() {
        let mut p = pool();
        let t = p.top();
        let not_top = p.not(t);
        let nnf = to_nnf(not_top, &mut p);
        assert_eq!(nnf, p.bot());
    }

    #[test]
    fn nnf_of_not_bot_is_top() {
        let mut p = pool();
        let b = p.bot();
        let not_bot = p.not(b);
        let nnf = to_nnf(not_bot, &mut p);
        assert_eq!(nnf, p.top());
    }

    #[test]
    fn nnf_of_double_negation_is_inner() {
        let mut p = pool();
        let a = p.atomic(ClassId::new(0));
        let not_a = p.not(a);
        let not_not_a = p.not(not_a);
        assert_eq!(to_nnf(not_not_a, &mut p), a);
    }

    #[test]
    fn nnf_of_not_atomic_keeps_one_not() {
        let mut p = pool();
        let a = p.atomic(ClassId::new(0));
        let not_a = p.not(a);
        assert_eq!(to_nnf(not_a, &mut p), not_a);
    }

    #[test]
    fn nnf_pushes_through_and_via_de_morgan() {
        // ¬(A ⊓ B) ≡ ¬A ⊔ ¬B
        let mut p = pool();
        let a = p.atomic(ClassId::new(0));
        let b = p.atomic(ClassId::new(1));
        let and_ab = p.and([a, b]);
        let not_and = p.not(and_ab);
        let result = to_nnf(not_and, &mut p);
        let na = p.not(a);
        let nb = p.not(b);
        let expected = p.or([na, nb]);
        assert_eq!(result, expected);
    }

    #[test]
    fn nnf_pushes_through_or_via_de_morgan() {
        // ¬(A ⊔ B) ≡ ¬A ⊓ ¬B
        let mut p = pool();
        let a = p.atomic(ClassId::new(0));
        let b = p.atomic(ClassId::new(1));
        let or_ab = p.or([a, b]);
        let not_or = p.not(or_ab);
        let result = to_nnf(not_or, &mut p);
        let na = p.not(a);
        let nb = p.not(b);
        let expected = p.and([na, nb]);
        assert_eq!(result, expected);
    }

    #[test]
    fn nnf_pushes_through_some_to_all_with_negated_inner() {
        // ¬∃R.A ≡ ∀R. ¬A
        let mut p = pool();
        let r = Role::named(RoleId::new(0));
        let a = p.atomic(ClassId::new(0));
        let some_a = p.some(r, a);
        let not_some = p.not(some_a);
        let result = to_nnf(not_some, &mut p);
        let na = p.not(a);
        let expected = p.all(r, na);
        assert_eq!(result, expected);
    }

    #[test]
    fn nnf_pushes_through_all_to_some_with_negated_inner() {
        // ¬∀R.A ≡ ∃R. ¬A
        let mut p = pool();
        let r = Role::named(RoleId::new(0));
        let a = p.atomic(ClassId::new(0));
        let all_a = p.all(r, a);
        let not_all = p.not(all_a);
        let result = to_nnf(not_all, &mut p);
        let na = p.not(a);
        let expected = p.some(r, na);
        assert_eq!(result, expected);
    }

    #[test]
    fn nnf_of_not_min_positive_n_becomes_max_n_minus_one() {
        // ¬(≥3 R.A) ≡ ≤2 R.A     (inner A stays positive)
        let mut p = pool();
        let r = Role::named(RoleId::new(0));
        let a = p.atomic(ClassId::new(0));
        let min3 = p.min(3, r, a);
        let not_min3 = p.not(min3);
        let result = to_nnf(not_min3, &mut p);
        let expected = p.max(2, r, a);
        assert_eq!(result, expected);
    }

    #[test]
    fn nnf_of_not_min_zero_is_bot() {
        // ¬(≥0 R.A) ≡ ⊥
        let mut p = pool();
        let r = Role::named(RoleId::new(0));
        let a = p.atomic(ClassId::new(0));
        let min0 = p.min(0, r, a);
        let not_min0 = p.not(min0);
        let result = to_nnf(not_min0, &mut p);
        assert_eq!(result, p.bot());
    }

    #[test]
    fn nnf_of_not_max_becomes_min_n_plus_one() {
        // ¬(≤2 R.A) ≡ ≥3 R.A
        let mut p = pool();
        let r = Role::named(RoleId::new(0));
        let a = p.atomic(ClassId::new(0));
        let max2 = p.max(2, r, a);
        let not_max2 = p.not(max2);
        let result = to_nnf(not_max2, &mut p);
        let expected = p.min(3, r, a);
        assert_eq!(result, expected);
    }

    #[test]
    fn nnf_of_not_nominal_keeps_one_not() {
        // ¬{a} stays as Not(Nominal) — there's no equivalent positive form.
        let mut p = pool();
        let n = p.nominal(IndividualId::new(0));
        let not_n = p.not(n);
        assert_eq!(to_nnf(not_n, &mut p), not_n);
    }

    #[test]
    fn nnf_of_not_self_restriction_keeps_one_not() {
        let mut p = pool();
        let r = Role::named(RoleId::new(0));
        let s = p.self_restriction(r);
        let not_s = p.not(s);
        assert_eq!(to_nnf(not_s, &mut p), not_s);
    }

    #[test]
    fn nested_negation_through_nested_structure() {
        // ¬(A ⊓ ∃R.B) ≡ ¬A ⊔ ∀R.¬B
        let mut p = pool();
        let r = Role::named(RoleId::new(0));
        let a = p.atomic(ClassId::new(0));
        let b = p.atomic(ClassId::new(1));
        let some_b = p.some(r, b);
        let conj = p.and([a, some_b]);
        let neg = p.not(conj);
        let result = to_nnf(neg, &mut p);
        let na = p.not(a);
        let nb = p.not(b);
        let all_nb = p.all(r, nb);
        let expected = p.or([na, all_nb]);
        assert_eq!(result, expected);
    }

    #[test]
    fn is_nnf_recognizes_nnf_forms() {
        let mut p = pool();
        let r = Role::named(RoleId::new(0));
        let a = p.atomic(ClassId::new(0));
        let na = p.not(a);
        let conj = p.and([a, na]);
        assert!(is_nnf(conj, &p));
        let all_na = p.all(r, na);
        assert!(is_nnf(all_na, &p));
    }

    #[test]
    fn is_nnf_rejects_non_atomic_negation() {
        // Not(And(...)) is NOT in NNF.
        let mut p = pool();
        let a = p.atomic(ClassId::new(0));
        let b = p.atomic(ClassId::new(1));
        let conj = p.and([a, b]);
        let neg = p.not(conj);
        assert!(!is_nnf(neg, &p));
    }

    // ── nnf_axioms tests ───────────────────────────────────────────────

    use crate::ontology::{Axiom, InternalOntology};

    fn fresh_ontology() -> InternalOntology {
        let mut o = InternalOntology::new();
        o.vocabulary.intern_class("A");
        o.vocabulary.intern_class("B");
        o.vocabulary.intern_class("C");
        o
    }

    #[test]
    fn nnf_axioms_rewrites_sub_class_of() {
        // SubClassOf(A, Not(And(B, C)))  →  SubClassOf(A, Or(Not(B), Not(C)))
        let mut o = fresh_ontology();
        let a = o.concepts.atomic(ClassId::new(0));
        let b = o.concepts.atomic(ClassId::new(1));
        let c = o.concepts.atomic(ClassId::new(2));
        let and_bc = o.concepts.and([b, c]);
        let not_and = o.concepts.not(and_bc);
        o.axioms.push(Axiom::SubClassOf {
            sub: a,
            sup: not_and,
        });
        let normalized = nnf_axioms(&mut o);
        let Axiom::SubClassOf { sub, sup } = &normalized[0] else {
            panic!()
        };
        assert_eq!(*sub, a);
        let nb = o.concepts.not(b);
        let nc = o.concepts.not(c);
        let expected_sup = o.concepts.or([nb, nc]);
        assert_eq!(*sup, expected_sup);
        assert!(is_nnf(*sup, &o.concepts));
    }

    #[test]
    fn nnf_axioms_leaves_original_axioms_unchanged() {
        // Verify the source list survives unaltered.
        let mut o = fresh_ontology();
        let a = o.concepts.atomic(ClassId::new(0));
        let b = o.concepts.atomic(ClassId::new(1));
        let and_ab = o.concepts.and([a, b]);
        let not_and = o.concepts.not(and_ab);
        o.axioms.push(Axiom::SubClassOf {
            sub: a,
            sup: not_and,
        });
        let original_count = o.axioms.len();
        let _ = nnf_axioms(&mut o);
        assert_eq!(o.axioms.len(), original_count);
        // The original axiom still holds the un-NNF'd super-concept.
        let Axiom::SubClassOf { sup, .. } = &o.axioms[0] else {
            panic!()
        };
        assert_eq!(*sup, not_and);
    }

    #[test]
    fn nnf_axioms_handles_multi_concept_axioms() {
        // EquivalentClasses normalizes every member.
        let mut o = fresh_ontology();
        let a = o.concepts.atomic(ClassId::new(0));
        let b = o.concepts.atomic(ClassId::new(1));
        let not_a = o.concepts.not(a);
        let not_not_a = o.concepts.not(not_a);
        o.axioms.push(Axiom::EquivalentClasses(vec![not_not_a, b]));
        let normalized = nnf_axioms(&mut o);
        let Axiom::EquivalentClasses(ids) = &normalized[0] else {
            panic!()
        };
        // Double-negated A collapses to A in NNF.
        assert_eq!(ids[0], a);
        assert_eq!(ids[1], b);
    }

    #[test]
    fn nnf_axioms_passes_through_role_axioms_unchanged() {
        let mut o = fresh_ontology();
        let r = Role::named(RoleId::new(0));
        o.axioms.push(Axiom::TransitiveRole(r));
        let normalized = nnf_axioms(&mut o);
        assert!(matches!(normalized[0], Axiom::TransitiveRole(_)));
    }

    #[test]
    fn nnf_axioms_normalizes_class_assertion_concept() {
        let mut o = fresh_ontology();
        let i = o.vocabulary.intern_individual("a");
        let a = o.concepts.atomic(ClassId::new(0));
        let not_a = o.concepts.not(a);
        let not_not_a = o.concepts.not(not_a);
        o.axioms.push(Axiom::ClassAssertion {
            class: not_not_a,
            individual: i,
        });
        let normalized = nnf_axioms(&mut o);
        let Axiom::ClassAssertion { class, .. } = normalized[0] else {
            panic!()
        };
        assert_eq!(class, a);
    }
}
