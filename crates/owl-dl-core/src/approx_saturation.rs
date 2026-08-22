//! SP-A: approximated saturation — forced-disjunct precomputation.
//!
//! For a GCI `C ⊑ D₁ ⊔ … ⊔ Dₙ` with **atomic** disjuncts: a disjunct `Dᵢ` is
//! *incompatible with C* iff `C` itself, or some told-subsumer `G` of `C`, is
//! told-disjoint from `Dᵢ`. Let `K` be the compatible disjuncts.
//!   * `|K| == 1` ⟹ emit `C ⊑ Dₖ` (the survivor is forced).
//!   * `|K| == 0` ⟹ emit `C ⊑ ⊥` (every disjunct clashes; `C` unsatisfiable).
//!   * `|K| ≥ 2` ⟹ emit nothing.
//!
//! Sound by construction: the told tables are a subset of true entailment, so a
//! disjunct is dropped only when genuinely entailed-disjoint — this can only
//! *miss* a forcing, never invent one. Companion of
//! [`crate::disjunction_existential`] (common-disjunct, Rule 1), which already
//! ships. Scope: **atomic disjuncts only** — any `Nominal` disjunct ⟹ skip the
//! whole disjunction (nominal value-partition forcing is a deferred increment).

use crate::ir::{ClassId, ConceptExpr, ConceptId};
use crate::ontology::{Axiom, InternalOntology};
use crate::told::{ToldTables, build_told_tables};

/// Target of a forced disjunction: a specific surviving disjunct, or bottom.
enum Forced {
    Class(ConceptId),
    Bot,
}

/// Append derived `C ⊑ Dₖ` / `C ⊑ ⊥` forced-disjunct axioms to `onto`.
pub fn derive_forced_disjuncts(onto: &mut InternalOntology) {
    derive_forced_disjuncts_with(onto, None);
}

/// As [`derive_forced_disjuncts`], but reuses `told` when the caller can prove it
/// is still valid — i.e. the ontology has not been modified since it was built.
/// See `disjunction_existential::derive_disjunction_existentials` for why: the two
/// passes run back-to-back in `convert_ontology` and each rebuilt the tables, at
/// 3.9 s per build on a 2.1M-axiom `TBox`.
pub fn derive_forced_disjuncts_with(onto: &mut InternalOntology, told: Option<ToldTables>) {
    let told = match told {
        Some(t) => t,
        None => build_told_tables(onto),
    };
    // Phase 1 (immutable borrow): decide each atomic-disjunction GCI.
    let mut derived: Vec<(ConceptId, Forced)> = Vec::new();
    for ax in &onto.axioms {
        let Axiom::SubClassOf { sub, sup } = ax else {
            continue;
        };
        // `sub` must be atomic so its told-subsumers define the context.
        let ConceptExpr::Atomic(c) = onto.concepts.get(*sub) else {
            continue;
        };
        let ConceptExpr::Or(disjuncts) = onto.concepts.get(*sup) else {
            continue;
        };
        // Collect atomic disjuncts; bail (scope guard) on any non-atomic
        // (Nominal/compound) disjunct — no nominal value-partition forcing here.
        let mut atomic: Vec<(ConceptId, ClassId)> = Vec::with_capacity(disjuncts.len());
        let mut all_atomic = true;
        for &d in disjuncts {
            if let ConceptExpr::Atomic(did) = onto.concepts.get(d) {
                atomic.push((d, *did));
            } else {
                all_atomic = false;
                break;
            }
        }
        if !all_atomic {
            continue;
        }
        let c = *c;
        let survivors: Vec<ConceptId> = atomic
            .iter()
            .copied()
            .filter(|&(_, did)| !is_incompatible(c, did, &told))
            .map(|(cid, _)| cid)
            .collect();
        match survivors.len() {
            1 => derived.push((*sub, Forced::Class(survivors[0]))),
            0 => derived.push((*sub, Forced::Bot)),
            _ => {}
        }
    }
    if derived.is_empty() {
        return;
    }
    // Phase 2 (mutable borrow): intern Bot + push axioms.
    let bot = onto.concepts.bot();
    for (sub, target) in derived {
        let sup = match target {
            Forced::Class(cid) => cid,
            Forced::Bot => bot,
        };
        if sub != sup {
            onto.axioms.push(Axiom::SubClassOf { sub, sup });
        }
    }
}

/// `d` is incompatible with class `c` iff `c` itself or any told-subsumer of `c`
/// is told-disjoint from `d`.
fn is_incompatible(c: ClassId, d: ClassId, told: &ToldTables) -> bool {
    if told.are_told_disjoint(c, d) {
        return true;
    }
    told.super_classes(c)
        .iter()
        .any(|&g| told.are_told_disjoint(g, d))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::ConceptExpr;
    use crate::ontology::Axiom;

    /// Build a tiny `InternalOntology` with the given class IRIs interned.
    /// Returns the ontology and a map from name → `ClassId`.
    fn build(
        class_iris: &[&str],
    ) -> (InternalOntology, std::collections::HashMap<String, ClassId>) {
        let mut o = InternalOntology::new();
        let mut ids = std::collections::HashMap::new();
        for &iri in class_iris {
            let id = o.vocabulary.intern_class(iri);
            ids.insert(iri.to_owned(), id);
        }
        (o, ids)
    }

    /// Push `C ⊑ D` (both atomic).
    fn push_sub(onto: &mut InternalOntology, c: ClassId, d: ClassId) {
        let sub = onto.concepts.atomic(c);
        let sup = onto.concepts.atomic(d);
        onto.axioms.push(Axiom::SubClassOf { sub, sup });
    }

    /// Push `C ⊑ D₁ ⊔ … ⊔ Dₙ` (atomic sub, atomic-disjunct Or sup).
    fn push_sub_or(onto: &mut InternalOntology, c: ClassId, ds: &[ClassId]) {
        let sub = onto.concepts.atomic(c);
        let disjuncts: Vec<ConceptId> = ds.iter().map(|&d| onto.concepts.atomic(d)).collect();
        let sup = onto.concepts.or(disjuncts);
        onto.axioms.push(Axiom::SubClassOf { sub, sup });
    }

    /// Push `DisjointClasses(A, B)`.
    fn push_disjoint(onto: &mut InternalOntology, a: ClassId, b: ClassId) {
        let ca = onto.concepts.atomic(a);
        let cb = onto.concepts.atomic(b);
        onto.axioms.push(Axiom::DisjointClasses(vec![ca, cb]));
    }

    /// True iff `onto.axioms` contains `SubClassOf(Atomic(c), Atomic(d))`.
    fn has_atomic_sub(onto: &InternalOntology, c: ClassId, d: ClassId) -> bool {
        onto.axioms.iter().any(|ax| {
            if let Axiom::SubClassOf { sub, sup } = ax {
                matches!(onto.concepts.get(*sub), ConceptExpr::Atomic(x) if *x == c)
                    && matches!(onto.concepts.get(*sup), ConceptExpr::Atomic(y) if *y == d)
            } else {
                false
            }
        })
    }

    /// True iff `onto.axioms` contains `SubClassOf(Atomic(c), Bot)`.
    fn has_sub_bot(onto: &InternalOntology, c: ClassId) -> bool {
        onto.axioms.iter().any(|ax| {
            if let Axiom::SubClassOf { sub, sup } = ax {
                matches!(onto.concepts.get(*sub), ConceptExpr::Atomic(x) if *x == c)
                    && matches!(onto.concepts.get(*sup), ConceptExpr::Bot)
            } else {
                false
            }
        })
    }

    /// Intern a fresh individual and return its `IndividualId`.
    fn new_individual(onto: &mut InternalOntology, iri: &str) -> crate::ir::IndividualId {
        onto.vocabulary.intern_individual(iri)
    }

    #[test]
    fn forced_disjunct_fires() {
        // C ⊑ A ⊔ B, C ⊑ G, Disjoint(G, A) ⟹ derive C ⊑ B.
        let (mut onto, ids) = build(&["C", "A", "B", "G"]);
        let (c, a, b, g) = (ids["C"], ids["A"], ids["B"], ids["G"]);
        push_sub_or(&mut onto, c, &[a, b]);
        push_sub(&mut onto, c, g);
        push_disjoint(&mut onto, g, a);
        derive_forced_disjuncts(&mut onto);
        assert!(has_atomic_sub(&onto, c, b), "expected derived C ⊑ B");
        assert!(!has_atomic_sub(&onto, c, a), "must NOT derive C ⊑ A");
    }

    #[test]
    fn forced_to_bot() {
        // C ⊑ A ⊔ B, C ⊑ G, Disjoint(G,A), Disjoint(G,B) ⟹ C ⊑ ⊥.
        let (mut onto, ids) = build(&["C", "A", "B", "G"]);
        let (c, a, b, g) = (ids["C"], ids["A"], ids["B"], ids["G"]);
        push_sub_or(&mut onto, c, &[a, b]);
        push_sub(&mut onto, c, g);
        push_disjoint(&mut onto, g, a);
        push_disjoint(&mut onto, g, b);
        derive_forced_disjuncts(&mut onto);
        assert!(has_sub_bot(&onto, c), "expected derived C ⊑ ⊥");
    }

    #[test]
    fn undetermined_emits_nothing() {
        // C ⊑ A ⊔ B with no disjointness ⟹ nothing derived (no spurious C⊑A/C⊑B).
        let (mut onto, ids) = build(&["C", "A", "B"]);
        let (c, a, b) = (ids["C"], ids["A"], ids["B"]);
        let before = onto.axioms.len();
        push_sub_or(&mut onto, c, &[a, b]);
        let after_push = onto.axioms.len();
        derive_forced_disjuncts(&mut onto);
        assert_eq!(onto.axioms.len(), after_push, "no axiom should be derived");
        assert!(!has_atomic_sub(&onto, c, a) && !has_atomic_sub(&onto, c, b));
        let _ = before;
    }

    #[test]
    fn nominal_disjunction_not_touched() {
        // C ⊑ {x} ⊔ {y} (nominal disjuncts) ⟹ nothing derived (scope guard).
        let (mut onto, ids) = build(&["C"]);
        let c = ids["C"];
        let ix = new_individual(&mut onto, "x");
        let iy = new_individual(&mut onto, "y");
        let x = onto.concepts.nominal(ix);
        let y = onto.concepts.nominal(iy);
        let or = onto.concepts.or(vec![x, y]);
        let csub = onto.concepts.atomic(c);
        onto.axioms.push(Axiom::SubClassOf { sub: csub, sup: or });
        let before = onto.axioms.len();
        derive_forced_disjuncts(&mut onto);
        assert_eq!(
            onto.axioms.len(),
            before,
            "nominal disjunction must be skipped"
        );
    }
}
