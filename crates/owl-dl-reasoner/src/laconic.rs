//! Laconic (fine-grained) justifications: weaken each axiom of a regular
//! justification to its responsible fragment, then re-minimize. Sound by
//! construction — every emitted fragment is *entailed by* an original axiom, so a
//! laconic justification is a set of genuine consequences of the ontology that
//! explains the entailment. Read-only; FP=0 untouched.

use std::collections::BTreeSet;
// Imports below are consumed by the driver wired up in Task 3.
#[allow(unused_imports)] // wired into the driver in Task 3; allow removed there
use std::collections::HashSet;

use horned_owl::model::{ClassExpression, Component, ForIRI, SubClassOf};
use horned_owl::ontology::set::SetOntology;

use crate::ReasonError;
use crate::justify::{Entailment, Justification};
#[allow(unused_imports)] // wired into the driver in Task 3; allow removed there
use crate::justify::{
    find_all_justifications, find_one_justification, logical_axioms, quickxplain,
};

/// Decompose a superclass expression into top-level fragments, each of which the
/// original superclass is subsumed by (so `C ⊑ sup` entails `C ⊑ fragment`).
/// Splits conjunctions and recurses into existential fillers; everything else is
/// atomic (returned as-is).
#[allow(dead_code)] // wired into the driver in Task 3; allow removed there
fn split_sup<A: ForIRI>(sup: &ClassExpression<A>) -> Vec<ClassExpression<A>> {
    use ClassExpression as CE;
    match sup {
        CE::ObjectIntersectionOf(cs) => cs.iter().flat_map(split_sup).collect(),
        CE::ObjectSomeValuesFrom { ope, bce } => split_sup(bce)
            .into_iter()
            .map(|f| CE::ObjectSomeValuesFrom {
                ope: ope.clone(),
                bce: Box::new(f),
            })
            .collect(),
        other => vec![other.clone()],
    }
}

/// Weaken a single axiom into a set of fragments, each ENTAILED BY the axiom.
/// An axiom with no applicable operator returns `vec![axiom.clone()]` (passes
/// through unchanged).
#[allow(dead_code)] // wired into the driver in Task 3; allow removed there
fn weaken<A: ForIRI>(axiom: &Component<A>) -> Vec<Component<A>> {
    match axiom {
        // C ⊑ sup  →  one fragment per split of sup (LHS kept whole — splitting it
        // would strengthen the axiom, which is not entailed).
        Component::SubClassOf(sc) => {
            let frags = split_sup(&sc.sup);
            if frags.len() == 1 && frags[0] == sc.sup {
                vec![axiom.clone()]
            } else {
                frags
                    .into_iter()
                    .map(|f| Component::SubClassOf(SubClassOf { sub: sc.sub.clone(), sup: f }))
                    .collect()
            }
        }
        // C₁ ≡ … ≡ Cₙ  →  all ordered pairs Cᵢ ⊑ (each split fragment of Cⱼ).
        Component::EquivalentClasses(eq) => {
            let members = &eq.0;
            if members.len() < 2 {
                return vec![axiom.clone()];
            }
            let mut out = Vec::new();
            for (i, mi) in members.iter().enumerate() {
                for (j, mj) in members.iter().enumerate() {
                    if i == j {
                        continue;
                    }
                    for f in split_sup(mj) {
                        out.push(Component::SubClassOf(SubClassOf {
                            sub: mi.clone(),
                            sup: f,
                        }));
                    }
                }
            }
            out
        }
        // DisjointClasses(C₁ … Cₙ), n>2  →  pairwise DisjointClasses(Cᵢ, Cⱼ).
        Component::DisjointClasses(dc) => {
            let members = &dc.0;
            if members.len() <= 2 {
                return vec![axiom.clone()];
            }
            let mut out = Vec::new();
            for i in 0..members.len() {
                for j in (i + 1)..members.len() {
                    out.push(Component::DisjointClasses(horned_owl::model::DisjointClasses(vec![
                        members[i].clone(),
                        members[j].clone(),
                    ])));
                }
            }
            out
        }
        // Everything else passes through unchanged.
        _ => vec![axiom.clone()],
    }
    .into_iter()
    .collect::<BTreeSet<_>>() // dedup, deterministic order
    .into_iter()
    .collect()
}

/// Laconic justification for `q` (one). Filled in by Task 3.
pub fn find_laconic_justification<A: ForIRI>(
    onto: &SetOntology<A>,
    q: &Entailment,
) -> Result<Option<Justification<A>>, ReasonError> {
    let _ = (onto, q);
    Ok(None)
}

/// All laconic justifications for `q` (capped). Filled in by Task 3.
pub fn find_all_laconic_justifications<A: ForIRI>(
    onto: &SetOntology<A>,
    q: &Entailment,
    max: usize,
) -> Result<Vec<Justification<A>>, ReasonError> {
    let _ = (onto, q, max);
    Ok(Vec::new())
}

#[cfg(test)]
mod weaken_tests {
    use super::*;
    use horned_owl::model::Build;

    type Rc = std::rc::Rc<str>;
    fn b() -> Build<Rc> {
        Build::new_rc()
    }
    fn cls(b: &Build<Rc>, iri: &str) -> ClassExpression<Rc> {
        ClassExpression::Class(b.class(iri))
    }
    fn sc(sub: ClassExpression<Rc>, sup: ClassExpression<Rc>) -> Component<Rc> {
        Component::SubClassOf(SubClassOf { sub, sup })
    }

    // C ⊑ D ⊓ E  →  {C ⊑ D, C ⊑ E}
    #[test]
    fn rhs_conjunction_splits() {
        let b = b();
        let ax = sc(
            cls(&b, "urn:C"),
            ClassExpression::ObjectIntersectionOf(vec![cls(&b, "urn:D"), cls(&b, "urn:E")]),
        );
        let got: BTreeSet<Component<Rc>> = weaken(&ax).into_iter().collect();
        let want: BTreeSet<Component<Rc>> = [
            sc(cls(&b, "urn:C"), cls(&b, "urn:D")),
            sc(cls(&b, "urn:C"), cls(&b, "urn:E")),
        ]
        .into_iter()
        .collect();
        assert_eq!(got, want);
    }

    // C ⊑ ∃r.(D ⊓ E)  →  {C ⊑ ∃r.D, C ⊑ ∃r.E}
    #[test]
    fn existential_filler_splits() {
        let b = b();
        let some = |f: ClassExpression<Rc>| ClassExpression::ObjectSomeValuesFrom {
            ope: b.object_property("urn:r").into(),
            bce: Box::new(f),
        };
        let ax = sc(
            cls(&b, "urn:C"),
            some(ClassExpression::ObjectIntersectionOf(vec![
                cls(&b, "urn:D"),
                cls(&b, "urn:E"),
            ])),
        );
        let got: BTreeSet<Component<Rc>> = weaken(&ax).into_iter().collect();
        let want: BTreeSet<Component<Rc>> = [
            sc(cls(&b, "urn:C"), some(cls(&b, "urn:D"))),
            sc(cls(&b, "urn:C"), some(cls(&b, "urn:E"))),
        ]
        .into_iter()
        .collect();
        assert_eq!(got, want);
    }

    // Nested: C ⊑ F ⊓ ∃r.(G ⊓ H)  →  {C⊑F, C⊑∃r.G, C⊑∃r.H}
    #[test]
    fn nested_splits() {
        let b = b();
        let some = |f: ClassExpression<Rc>| ClassExpression::ObjectSomeValuesFrom {
            ope: b.object_property("urn:r").into(),
            bce: Box::new(f),
        };
        let ax = sc(
            cls(&b, "urn:C"),
            ClassExpression::ObjectIntersectionOf(vec![
                cls(&b, "urn:F"),
                some(ClassExpression::ObjectIntersectionOf(vec![
                    cls(&b, "urn:G"),
                    cls(&b, "urn:H"),
                ])),
            ]),
        );
        let got: BTreeSet<Component<Rc>> = weaken(&ax).into_iter().collect();
        let want: BTreeSet<Component<Rc>> = [
            sc(cls(&b, "urn:C"), cls(&b, "urn:F")),
            sc(cls(&b, "urn:C"), some(cls(&b, "urn:G"))),
            sc(cls(&b, "urn:C"), some(cls(&b, "urn:H"))),
        ]
        .into_iter()
        .collect();
        assert_eq!(got, want);
    }

    // C ≡ D ⊓ E  →  {C⊑D, C⊑E, (D⊓E)⊑C}
    #[test]
    fn equivalence_splits_to_subsumptions() {
        let b = b();
        let inter = ClassExpression::ObjectIntersectionOf(vec![cls(&b, "urn:D"), cls(&b, "urn:E")]);
        let ax = Component::EquivalentClasses(horned_owl::model::EquivalentClasses(vec![
            cls(&b, "urn:C"),
            inter.clone(),
        ]));
        let got: BTreeSet<Component<Rc>> = weaken(&ax).into_iter().collect();
        let want: BTreeSet<Component<Rc>> = [
            sc(cls(&b, "urn:C"), cls(&b, "urn:D")),
            sc(cls(&b, "urn:C"), cls(&b, "urn:E")),
            sc(inter, cls(&b, "urn:C")),
        ]
        .into_iter()
        .collect();
        assert_eq!(got, want);
    }

    // DisjointClasses(C,D,E) → pairwise {DC(C,D), DC(C,E), DC(D,E)}
    #[test]
    fn disjoint_splits_pairwise() {
        let b = b();
        let dc = |x: &str, y: &str| {
            Component::DisjointClasses(horned_owl::model::DisjointClasses(vec![
                cls(&b, x),
                cls(&b, y),
            ]))
        };
        let ax = Component::DisjointClasses(horned_owl::model::DisjointClasses(vec![
            cls(&b, "urn:C"),
            cls(&b, "urn:D"),
            cls(&b, "urn:E"),
        ]));
        let got: BTreeSet<Component<Rc>> = weaken(&ax).into_iter().collect();
        let want: BTreeSet<Component<Rc>> = [
            dc("urn:C", "urn:D"),
            dc("urn:C", "urn:E"),
            dc("urn:D", "urn:E"),
        ]
        .into_iter()
        .collect();
        assert_eq!(got, want);
    }

    // NEGATIVE: plain C ⊑ D passes through unchanged.
    #[test]
    fn plain_subsumption_unchanged() {
        let b = b();
        let ax = sc(cls(&b, "urn:C"), cls(&b, "urn:D"));
        assert_eq!(weaken(&ax), vec![ax.clone()]);
    }

    // NEGATIVE: LHS conjunction C₁⊓C₂ ⊑ D is NOT split (would strengthen → unsound).
    #[test]
    fn lhs_conjunction_not_split() {
        let b = b();
        let ax = sc(
            ClassExpression::ObjectIntersectionOf(vec![cls(&b, "urn:C1"), cls(&b, "urn:C2")]),
            cls(&b, "urn:D"),
        );
        assert_eq!(weaken(&ax), vec![ax.clone()]);
    }

    // NEGATIVE: cardinality filler is NOT weakened.
    #[test]
    fn cardinality_not_weakened() {
        let b = b();
        let ax = sc(
            cls(&b, "urn:C"),
            ClassExpression::ObjectMinCardinality {
                n: 3,
                ope: b.object_property("urn:r").into(),
                bce: Box::new(cls(&b, "urn:D")),
            },
        );
        assert_eq!(weaken(&ax), vec![ax.clone()]);
    }
}
