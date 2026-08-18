//! Laconic (fine-grained) justifications: weaken each axiom of a regular
//! justification to its responsible fragment, then re-minimize. Sound by
//! construction — every emitted fragment is *entailed by* an original axiom, so a
//! laconic justification is a set of genuine consequences of the ontology that
//! explains the entailment. Read-only; FP=0 untouched.

use std::collections::{BTreeSet, HashSet};

use horned_owl::model::{ClassExpression, Component, ForIRI, SubClassOf};
use horned_owl::ontology::set::SetOntology;

use crate::ReasonError;
use crate::justify::{Entailment, Justification, PreparedJustifier, quickxplain};

/// Decompose a superclass expression into top-level fragments whose CONJUNCTION
/// is EQUIVALENT to the original (so the fragment set, as a whole, preserves every
/// entailment of `C ⊑ sup`, and each `C ⊑ fragment` is individually entailed).
///
/// Only conjunctions are split: `X ⊑ D₁ ⊓ … ⊓ Dₙ` iff `X ⊑ Dᵢ` for all `i`, so
/// `{C ⊑ D₁, …, C ⊑ Dₙ}` is set-equivalent to `C ⊑ D₁⊓…⊓Dₙ`. Existential-filler
/// narrowing (`∃r.(D⊓E) → ∃r.D`) is deliberately NOT done: it is a strict
/// weakening (the split successors need not coincide), so it does NOT preserve the
/// original's entailment and would let the candidate set fail `QuickXplain`'s
/// precondition. Everything else is atomic (returned as-is).
fn split_sup<A: ForIRI>(sup: &ClassExpression<A>) -> Vec<ClassExpression<A>> {
    use ClassExpression as CE;
    match sup {
        CE::ObjectIntersectionOf(cs) => cs.iter().flat_map(split_sup).collect(),
        other => vec![other.clone()],
    }
}

/// Weaken a single axiom into a set of fragments, each ENTAILED BY the axiom.
/// An axiom with no applicable operator returns `vec![axiom.clone()]` (passes
/// through unchanged).
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

/// Build the laconic version of ONE regular justification: weaken that
/// justification's axioms and re-minimize over the weakenings via `QuickXplain`.
///
/// The background is the **non-logical** axioms only (declarations) — NOT the rest
/// of the ontology's logical axioms. The regular justification `J` is minimal and
/// `J` alone entails `q`, so we explain `q` using only (weakenings of) `J`'s
/// axioms. Including other logical axioms would let an alternative derivation in
/// the background entail `q` on its own, collapsing the result to `∅`. This mirrors
/// how `find_one_justification` itself calls `quickxplain` (fixed = non-logical,
/// candidates = the axioms under consideration).
///
/// `background` comes from the caller's [`PreparedJustifier`] rather than a fresh
/// `logical_axioms(onto)` — this runs once per regular justification, so deriving
/// the split here made `find_all_laconic_justifications` re-split the whole
/// ontology N times.
fn laconic_from<A: ForIRI>(
    background: &[Component<A>],
    q: &Entailment,
    j_axioms: &[Component<A>],
    fragment: crate::classify::FragmentClassification,
    minimal_guaranteed: bool,
) -> Result<Justification<A>, ReasonError> {
    // candidates = the union of the weakenings of the justification's axioms.
    let candidates: Vec<Component<A>> = j_axioms
        .iter()
        .flat_map(weaken)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();

    // Belt-and-suspenders: every supported weakening operator is
    // entailment-preserving (the fragment set is set-equivalent to `J`), so the
    // candidate set must still entail `q`. With the ∃-filler narrowing dropped this
    // can never fire; assert it in debug builds so a future non-preserving operator
    // is caught immediately rather than silently producing a non-entailing result.
    #[cfg(debug_assertions)]
    {
        let still_entails =
            crate::justify::entails(&crate::justify::ontology_from(background, &candidates), q)?;
        debug_assert!(
            still_entails,
            "laconic candidate set must still entail q — a weakening operator is not \
             entailment-preserving"
        );
    }

    let laconic = quickxplain(background, &candidates, q)?;
    Ok(Justification {
        axioms: laconic,
        fragment,
        minimal_guaranteed,
    })
}

/// Laconic justification for `q` (one), or `None` if `q` is not entailed.
///
/// # Errors
/// Propagates [`ReasonError`].
pub fn find_laconic_justification<A: ForIRI>(
    onto: &SetOntology<A>,
    q: &Entailment,
) -> Result<Option<Justification<A>>, ReasonError> {
    let prepared = PreparedJustifier::prepare(onto);
    let Some(j) = prepared.find_one(q)? else {
        return Ok(None);
    };
    Ok(Some(laconic_from(
        prepared.background(),
        q,
        &j.axioms,
        j.fragment,
        j.minimal_guaranteed,
    )?))
}

/// All laconic justifications for `q` (one per regular justification, capped by
/// `max`), de-duplicated by fragment set.
///
/// # Errors
/// Propagates [`ReasonError`].
pub fn find_all_laconic_justifications<A: ForIRI>(
    onto: &SetOntology<A>,
    q: &Entailment,
    max: usize,
) -> Result<Vec<Justification<A>>, ReasonError> {
    let prepared = PreparedJustifier::prepare(onto);
    let regular = prepared.find_all(q, max)?;
    let mut out = Vec::new();
    let mut seen: HashSet<BTreeSet<Component<A>>> = HashSet::new();
    for j in regular {
        let lac = laconic_from(
            prepared.background(),
            q,
            &j.axioms,
            j.fragment,
            j.minimal_guaranteed,
        )?;
        let key: BTreeSet<Component<A>> = lac.axioms.iter().cloned().collect();
        if seen.insert(key) {
            out.push(lac);
        }
    }
    Ok(out)
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

    // C ⊑ ∃r.(D ⊓ E)  →  PASSES THROUGH unchanged (∃-filler narrowing is NOT
    // entailment-preserving, so it is deliberately not performed).
    #[test]
    fn existential_filler_not_split() {
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
        assert_eq!(weaken(&ax), vec![ax.clone()]);
    }

    // Nested: C ⊑ F ⊓ ∃r.(G ⊓ H)  →  {C⊑F, C⊑∃r.(G⊓H)}  (top-level conjuncts split;
    // the existential conjunct is kept WHOLE, not narrowed).
    #[test]
    fn nested_splits_conjuncts_only() {
        let b = b();
        let some = |f: ClassExpression<Rc>| ClassExpression::ObjectSomeValuesFrom {
            ope: b.object_property("urn:r").into(),
            bce: Box::new(f),
        };
        let inner = ClassExpression::ObjectIntersectionOf(vec![cls(&b, "urn:G"), cls(&b, "urn:H")]);
        let ax = sc(
            cls(&b, "urn:C"),
            ClassExpression::ObjectIntersectionOf(vec![cls(&b, "urn:F"), some(inner.clone())]),
        );
        let got: BTreeSet<Component<Rc>> = weaken(&ax).into_iter().collect();
        let want: BTreeSet<Component<Rc>> = [
            sc(cls(&b, "urn:C"), cls(&b, "urn:F")),
            sc(cls(&b, "urn:C"), some(inner)),
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
