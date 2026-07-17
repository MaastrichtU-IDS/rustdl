//! Preprocessing pass: split a **union on the left** of a subclass axiom into
//! one axiom per disjunct — `(D₁ ⊔ … ⊔ Dₙ) ⊑ C ≡ ⋀ᵢ (Dᵢ ⊑ C)`.
//!
//! ## Why
//!
//! This is a logical equivalence (sound in both directions). The EL saturator
//! reads `SubClassOf` axioms directly and handles atomic and *intersection*
//! LHS, but **drops a union LHS** (it is not an EL body it recognises). The
//! tableau's binary absorption likewise files `(D₁ ⊔ … ⊔ Dₙ) ⊑ C` — which
//! internalises to `⊤ ⊑ (¬D₁ ⊓ … ⊓ ¬Dₙ) ⊔ C`, an `Or` with no `Not(Atomic)`
//! trigger — as a residual `Or`-GCI. Net effect: the entailed subsumptions
//! `Dᵢ ⊑ C` are missed by classify's forward closure (found on
//! `ore_ont_13077`, whose `SubClassOf(ObjectUnionOf(Osteuropaeer,
//! Lateinamerikaner), Auslaender)` entails `Lateinamerikaner ⊑ Auslaender`).
//!
//! Splitting the axiom up front feeds both engines the atomic-LHS form
//! `Dᵢ ⊑ C`, which the saturator turns into told subsumptions and the tableau
//! absorbs into `ConceptRule`s. **Sound by construction** (each emitted axiom
//! is entailed by the original) and completeness-improving (never removes a
//! subsumption). Runs before `derive_disjunction_existentials` and the engine
//! builds so every consumer sees the split form.

use crate::ir::{ConceptExpr, ConceptId};
use crate::ontology::{Axiom, InternalOntology};

/// Replace every `SubClassOf(ObjectUnionOf(D₁, …, Dₙ), C)` with the `n`
/// axioms `SubClassOf(Dᵢ, C)`. Nested unions cannot occur (the pool flattens
/// `Or`), so a single pass suffices.
pub fn split_disjunctive_antecedents(onto: &mut InternalOntology) {
    let mut rewritten: Vec<Axiom> = Vec::with_capacity(onto.axioms.len());
    for ax in std::mem::take(&mut onto.axioms) {
        match &ax {
            Axiom::SubClassOf { sub, sup } => {
                let disjuncts: Option<Vec<ConceptId>> = match onto.concepts.get(*sub) {
                    ConceptExpr::Or(args) => Some(args.to_vec()),
                    _ => None,
                };
                if let Some(disjuncts) = disjuncts {
                    let sup = *sup;
                    for disjunct in disjuncts {
                        rewritten.push(Axiom::SubClassOf { sub: disjunct, sup });
                    }
                } else {
                    rewritten.push(ax);
                }
            }
            _ => rewritten.push(ax),
        }
    }
    onto.axioms = rewritten;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::ClassId;

    fn fresh(names: &[&str]) -> InternalOntology {
        let mut o = InternalOntology::new();
        for n in names {
            o.vocabulary.intern_class(n);
        }
        o
    }
    fn atom(o: &mut InternalOntology, name: &str) -> crate::ir::ConceptId {
        let c = o.vocabulary.class_id(name).expect("class missing");
        o.concepts.atomic(c)
    }
    fn cid(o: &InternalOntology, name: &str) -> ClassId {
        o.vocabulary.class_id(name).expect("class missing")
    }

    #[test]
    fn union_lhs_subclass_splits_into_one_axiom_per_disjunct() {
        // (A ⊔ B) ⊑ C  →  A ⊑ C  and  B ⊑ C.
        let mut o = fresh(&["A", "B", "C"]);
        let a = atom(&mut o, "A");
        let b = atom(&mut o, "B");
        let c = atom(&mut o, "C");
        let or_ab = o.concepts.or([a, b]);
        o.axioms.push(Axiom::SubClassOf { sub: or_ab, sup: c });
        split_disjunctive_antecedents(&mut o);
        assert_eq!(o.axioms.len(), 2);
        let subs: std::collections::HashSet<_> = o
            .axioms
            .iter()
            .map(|ax| match ax {
                Axiom::SubClassOf { sub, sup } => {
                    assert_eq!(*sup, c);
                    *sub
                }
                _ => panic!("expected SubClassOf"),
            })
            .collect();
        assert!(subs.contains(&a));
        assert!(subs.contains(&b));
        // no residual Or-LHS axiom left
        for ax in &o.axioms {
            if let Axiom::SubClassOf { sub, .. } = ax {
                assert!(!matches!(o.concepts.get(*sub), ConceptExpr::Or(_)));
            }
        }
        let _ = cid(&o, "A");
    }

    #[test]
    fn atomic_lhs_subclass_is_left_unchanged() {
        // A ⊑ C stays a single axiom.
        let mut o = fresh(&["A", "C"]);
        let a = atom(&mut o, "A");
        let c = atom(&mut o, "C");
        o.axioms.push(Axiom::SubClassOf { sub: a, sup: c });
        split_disjunctive_antecedents(&mut o);
        assert_eq!(o.axioms.len(), 1);
    }
}
