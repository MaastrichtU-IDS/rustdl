//! Single-trigger binary absorption.
//!
//! Given an NNF'd `Vec<Axiom>` (produced by [`crate::nnf_axioms`]), extract
//! per-named-class trigger rules of the shape `A ⊑ φ` and leave everything
//! else as a residual GCI `⊤ ⊑ ψ`.
//!
//! ## Why
//!
//! A naive tableau must apply every GCI `⊤ ⊑ φ` universally — to every
//! node, adding `φ` to its label. Disjunctive `φ` then causes branching
//! everywhere. Absorption finds patterns of the form `⊤ ⊑ ¬A ⊔ ψ` and
//! converts them to "when `A` shows up in a label, add `ψ`" — fires only
//! when needed.
//!
//! ## Algorithm (single-trigger v0)
//!
//! For each input axiom:
//!
//! 1. Encode as `⊤ ⊑ φ`:
//!    - `SubClassOf { sub, sup }` → `φ = nnf(¬sub) ⊔ sup`.
//!    - `EquivalentClasses(ids)` → decompose into pairwise `SubClassOf`.
//!    - `DisjointClasses(ids)` → decompose into pairwise `SubClassOf(Ci, ¬Cj)`.
//!    - `DisjointUnion { class, members }` → emit the equivalence and
//!      pairwise disjointness sub-axioms.
//!    - `ObjectPropertyDomain { role, domain }` → `∃role.⊤ ⊑ domain`.
//!    - `ObjectPropertyRange { role, range }`  → `⊤ ⊑ ∀role.range`.
//! 2. Read off the top-level disjuncts of `φ` (relying on the Or-flattening
//!    and Top/Bot normalizations in `ConceptPool::or`).
//! 3. If any disjunct is `Not(Atomic(A))`, extract `A` as a trigger and
//!    emit `ConceptRule { trigger: A, conclusion: Or(other disjuncts) }`.
//! 4. Otherwise stash `φ` in `residual_gcis`.
//!
//! Multi-trigger absorption (`A ⊓ B ⊑ C`) is a Phase 4 refinement. Role
//! and nominal absorption ride on top of this pass — they scan the
//! resulting concept-rule conclusions and residual GCIs for further
//! patterns.

use crate::ConceptPool;
use crate::ir::{ClassId, ConceptExpr, ConceptId};
use crate::normalize::to_nnf;
use crate::ontology::Axiom;

/// The output of binary absorption.
#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub struct AbsorbedTBox {
    /// Concept rules: when the trigger class appears in a node label,
    /// add the conclusion concept.
    pub concept_rules: Vec<ConceptRule>,
    /// Residual GCIs: `⊤ ⊑ φᵢ`, applied universally by the tableau.
    pub residual_gcis: Vec<ConceptId>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct ConceptRule {
    pub trigger: ClassId,
    pub conclusion: ConceptId,
}

/// Run single-trigger binary absorption over the NNF axiom list.
#[must_use]
pub fn absorb(axioms_nnf: &[Axiom], pool: &mut ConceptPool) -> AbsorbedTBox {
    let mut tbox = AbsorbedTBox::default();
    for ax in axioms_nnf {
        absorb_one(ax, pool, &mut tbox);
    }
    tbox
}

fn absorb_one(ax: &Axiom, pool: &mut ConceptPool, tbox: &mut AbsorbedTBox) {
    match ax {
        Axiom::SubClassOf { sub, sup } => absorb_sub_sup(*sub, *sup, pool, tbox),
        Axiom::EquivalentClasses(ids) => {
            for i in 0..ids.len() {
                for j in 0..ids.len() {
                    if i != j {
                        absorb_sub_sup(ids[i], ids[j], pool, tbox);
                    }
                }
            }
        }
        Axiom::DisjointClasses(ids) => {
            emit_pairwise_disjoint(ids, pool, tbox);
        }
        Axiom::DisjointUnion { class, members } => {
            let class_concept = pool.atomic(*class);
            let union_concept = pool.or(members.iter().copied());
            // The equivalence half.
            absorb_sub_sup(class_concept, union_concept, pool, tbox);
            absorb_sub_sup(union_concept, class_concept, pool, tbox);
            // Pairwise-disjoint half.
            emit_pairwise_disjoint(members, pool, tbox);
        }
        Axiom::ObjectPropertyDomain { role, domain } => {
            // ∃role.⊤ ⊑ domain
            let top = pool.top();
            let some_r_top = pool.some(*role, top);
            absorb_sub_sup(some_r_top, *domain, pool, tbox);
        }
        Axiom::ObjectPropertyRange { role, range } => {
            // ⊤ ⊑ ∀role.range — a clean residual GCI.
            let all_r = pool.all(*role, *range);
            tbox.residual_gcis.push(all_r);
        }
        _ => {
            // Role characteristics, ABox, declarations — not TBox content.
            // They flow through to the reasoner via separate paths.
        }
    }
}

fn emit_pairwise_disjoint(ids: &[ConceptId], pool: &mut ConceptPool, tbox: &mut AbsorbedTBox) {
    for i in 0..ids.len() {
        for j in (i + 1)..ids.len() {
            let not_cj = pool.not(ids[j]);
            let not_cj_nnf = to_nnf(not_cj, pool);
            absorb_sub_sup(ids[i], not_cj_nnf, pool, tbox);
        }
    }
}

/// Encode `sub ⊑ sup` as `⊤ ⊑ nnf(¬sub) ⊔ sup` and try to extract a trigger.
fn absorb_sub_sup(sub: ConceptId, sup: ConceptId, pool: &mut ConceptPool, tbox: &mut AbsorbedTBox) {
    let neg_sub = pool.not(sub);
    let neg_sub_nnf = to_nnf(neg_sub, pool);
    let disjunction = pool.or([neg_sub_nnf, sup]);
    absorb_gci(disjunction, pool, tbox);
}

/// Process a `⊤ ⊑ φ` GCI: extract a `Not(Atomic)` disjunct as trigger if
/// any, otherwise add `φ` to the residual list.
fn absorb_gci(phi: ConceptId, pool: &mut ConceptPool, tbox: &mut AbsorbedTBox) {
    let disjuncts: Vec<ConceptId> = match pool.get(phi) {
        ConceptExpr::Or(args) => args.to_vec(),
        _ => vec![phi],
    };

    // Find first disjunct of the form Not(Atomic(A)).
    let trigger_pos = disjuncts
        .iter()
        .position(|&d| as_not_atomic(d, pool).is_some());

    if let Some(pos) = trigger_pos {
        let trigger =
            as_not_atomic(disjuncts[pos], pool).expect("trigger position established above");
        let rest: Vec<ConceptId> = disjuncts
            .iter()
            .enumerate()
            .filter_map(|(i, &c)| (i != pos).then_some(c))
            .collect();
        // Or normalizations handle empty (→ Bot), single (→ operand), or
        // multi-operand cases.
        let conclusion = pool.or(rest);
        tbox.concept_rules.push(ConceptRule {
            trigger,
            conclusion,
        });
    } else {
        tbox.residual_gcis.push(phi);
    }
}

fn as_not_atomic(cid: ConceptId, pool: &ConceptPool) -> Option<ClassId> {
    if let ConceptExpr::Not(inner) = pool.get(cid)
        && let ConceptExpr::Atomic(c) = pool.get(*inner)
    {
        return Some(*c);
    }
    None
}

#[cfg(test)]
mod tests {
    #![allow(clippy::many_single_char_names)]

    use super::*;
    use crate::Vocabulary;
    use crate::ir::{Role, RoleId};
    use crate::nnf_axioms;
    use crate::ontology::InternalOntology;

    fn fresh(class_names: &[&str]) -> InternalOntology {
        let mut o = InternalOntology::new();
        for n in class_names {
            o.vocabulary.intern_class(n);
        }
        o
    }

    fn cid(o: &InternalOntology, name: &str) -> ClassId {
        o.vocabulary.class_id(name).expect("class missing")
    }

    fn atom(o: &mut InternalOntology, name: &str) -> ConceptId {
        let c = cid(o, name);
        o.concepts.atomic(c)
    }

    /// NNF the ontology's axioms and run absorption. Returns the absorbed
    /// tbox and the NNF'd axioms (for inspection in tests).
    fn run(o: &mut InternalOntology) -> AbsorbedTBox {
        let nnf = nnf_axioms(o);
        absorb(&nnf, &mut o.concepts)
    }

    #[test]
    fn atomic_sub_class_of_yields_one_rule() {
        // A ⊑ B  →  rule (A, B).
        let mut o = fresh(&["A", "B"]);
        let a = atom(&mut o, "A");
        let b = atom(&mut o, "B");
        o.axioms.push(Axiom::SubClassOf { sub: a, sup: b });
        let t = run(&mut o);
        assert_eq!(t.concept_rules.len(), 1);
        assert!(t.residual_gcis.is_empty());
        assert_eq!(t.concept_rules[0].trigger, cid(&o, "A"));
        assert_eq!(t.concept_rules[0].conclusion, b);
    }

    #[test]
    fn sub_class_of_with_conjunctive_conclusion() {
        // A ⊑ B ⊓ C  →  rule (A, And([B, C])).
        let mut o = fresh(&["A", "B", "C"]);
        let a = atom(&mut o, "A");
        let b = atom(&mut o, "B");
        let cc = atom(&mut o, "C");
        let and_bc = o.concepts.and([b, cc]);
        o.axioms.push(Axiom::SubClassOf {
            sub: a,
            sup: and_bc,
        });
        let t = run(&mut o);
        assert_eq!(t.concept_rules.len(), 1);
        assert_eq!(t.concept_rules[0].trigger, cid(&o, "A"));
        assert_eq!(t.concept_rules[0].conclusion, and_bc);
    }

    #[test]
    fn complex_lhs_with_atomic_rhs_absorbs_via_double_negation() {
        // (B ⊓ C) ⊑ A  →  ⊤ ⊑ ¬(B⊓C) ⊔ A  →  ⊤ ⊑ ¬B ⊔ ¬C ⊔ A.
        // Top-level disjuncts include Not(B) and Not(C); pick one (first
        // by id) as trigger, conclusion = Or of the rest.
        let mut o = fresh(&["A", "B", "C"]);
        let a = atom(&mut o, "A");
        let b = atom(&mut o, "B");
        let cc = atom(&mut o, "C");
        let and_bc = o.concepts.and([b, cc]);
        o.axioms.push(Axiom::SubClassOf {
            sub: and_bc,
            sup: a,
        });
        let t = run(&mut o);
        assert_eq!(t.concept_rules.len(), 1);
        assert!(t.residual_gcis.is_empty());
        // The trigger must be one of B or C (whichever Not is sorted first).
        let trigger = t.concept_rules[0].trigger;
        assert!(trigger == cid(&o, "B") || trigger == cid(&o, "C"));
    }

    #[test]
    fn pure_existential_gci_is_residual() {
        // ⊤ ⊑ ∃R.A  has no Not(Atomic) top-level disjunct → residual.
        let mut o = fresh(&["A"]);
        let a = atom(&mut o, "A");
        let r = Role::named(RoleId::new(0));
        let some_a = o.concepts.some(r, a);
        let top = o.concepts.top();
        o.axioms.push(Axiom::SubClassOf {
            sub: top,
            sup: some_a,
        });
        let t = run(&mut o);
        assert!(t.concept_rules.is_empty());
        assert_eq!(t.residual_gcis.len(), 1);
        assert_eq!(t.residual_gcis[0], some_a);
    }

    #[test]
    fn equivalent_classes_creates_pairwise_rules() {
        // A ≡ B  →  rules (A, B) and (B, A).
        let mut o = fresh(&["A", "B"]);
        let a = atom(&mut o, "A");
        let b = atom(&mut o, "B");
        o.axioms.push(Axiom::EquivalentClasses(vec![a, b]));
        let t = run(&mut o);
        assert_eq!(t.concept_rules.len(), 2);
        // One rule for each direction.
        let triggers: Vec<ClassId> = t.concept_rules.iter().map(|r| r.trigger).collect();
        assert!(triggers.contains(&cid(&o, "A")));
        assert!(triggers.contains(&cid(&o, "B")));
    }

    #[test]
    fn disjoint_classes_yields_not_atom_conclusion() {
        // DisjointClasses(A, B)  →  rule (A, Not(B)).
        let mut o = fresh(&["A", "B"]);
        let a = atom(&mut o, "A");
        let b = atom(&mut o, "B");
        o.axioms.push(Axiom::DisjointClasses(vec![a, b]));
        let t = run(&mut o);
        assert_eq!(t.concept_rules.len(), 1);
        let rule = &t.concept_rules[0];
        // The trigger is whichever Not gets matched first — but both
        // operands are atoms, so the trigger and conclusion partition them.
        let trigger = rule.trigger;
        assert!(trigger == cid(&o, "A") || trigger == cid(&o, "B"));
        // Conclusion must be Not(Atomic(other)).
        let other = if trigger == cid(&o, "A") {
            cid(&o, "B")
        } else {
            cid(&o, "A")
        };
        let expected_other_atom = o.concepts.atomic(other);
        let expected_conclusion = o.concepts.not(expected_other_atom);
        assert_eq!(rule.conclusion, expected_conclusion);
    }

    #[test]
    fn disjoint_classes_three_way_yields_three_pairwise_rules() {
        // DisjointClasses(A, B, C) — pairs are (A,B), (A,C), (B,C).
        let mut o = fresh(&["A", "B", "C"]);
        let a = atom(&mut o, "A");
        let b = atom(&mut o, "B");
        let cc = atom(&mut o, "C");
        o.axioms.push(Axiom::DisjointClasses(vec![a, b, cc]));
        let t = run(&mut o);
        assert_eq!(t.concept_rules.len(), 3);
    }

    #[test]
    fn disjoint_union_emits_subsumption_and_pairwise_disjoint_rules() {
        // DisjointUnion(P, [C1, C2]):
        //   1. P ⊑ C1 ⊔ C2     →  one rule (P, Or([C1, C2]))
        //   2. C1 ⊔ C2 ⊑ P    →  residual (multi-trigger required)
        //   3. C1 ⌖ C2         →  one rule (C1, Not(C2))
        let mut o = fresh(&["P", "C1", "C2"]);
        let c1 = atom(&mut o, "C1");
        let c2 = atom(&mut o, "C2");
        o.axioms.push(Axiom::DisjointUnion {
            class: cid(&o, "P"),
            members: vec![c1, c2],
        });
        let t = run(&mut o);
        // One concept rule from P ⊑ C1 ⊔ C2.
        // One concept rule from pairwise disjoint.
        // C1 ⊔ C2 ⊑ P drops to residual.
        assert_eq!(t.concept_rules.len(), 2);
        assert_eq!(t.residual_gcis.len(), 1);
    }

    #[test]
    fn object_property_range_yields_residual_for_all_pattern() {
        // ObjectPropertyRange(r, A)  ≡  ⊤ ⊑ ∀r.A
        let mut o = fresh(&["A"]);
        let a = atom(&mut o, "A");
        let r = Role::named(RoleId::new(0));
        o.axioms
            .push(Axiom::ObjectPropertyRange { role: r, range: a });
        let t = run(&mut o);
        assert!(t.concept_rules.is_empty());
        assert_eq!(t.residual_gcis.len(), 1);
        let all_r_a = o.concepts.all(r, a);
        assert_eq!(t.residual_gcis[0], all_r_a);
    }

    #[test]
    fn unrelated_axioms_pass_through_without_contribution() {
        // Role characteristics and ABox don't show up in AbsorbedTBox.
        let mut o = fresh(&["A"]);
        let _ = atom(&mut o, "A");
        let r = Role::named(RoleId::new(0));
        let _ = Vocabulary::new(); // placate clippy::dead_code
        o.axioms.push(Axiom::TransitiveRole(r));
        let i = o.vocabulary.intern_individual("a");
        let a = o.concepts.atomic(cid(&o, "A"));
        o.axioms.push(Axiom::ClassAssertion {
            class: a,
            individual: i,
        });
        let t = run(&mut o);
        assert!(t.concept_rules.is_empty());
        assert!(t.residual_gcis.is_empty());
    }

    #[test]
    fn sub_class_of_top_to_atom_is_residual() {
        // ⊤ ⊑ A — no Not(Atomic) anywhere; just `A` is the residual GCI.
        let mut o = fresh(&["A"]);
        let a = atom(&mut o, "A");
        let top = o.concepts.top();
        o.axioms.push(Axiom::SubClassOf { sub: top, sup: a });
        let t = run(&mut o);
        assert!(t.concept_rules.is_empty());
        assert_eq!(t.residual_gcis.len(), 1);
        assert_eq!(t.residual_gcis[0], a);
    }
}
