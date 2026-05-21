//! Reverse conversion: [`InternalOntology`] back to a horned-owl
//! [`SetOntology`]. Pairs with [`crate::convert`] to close the bijection
//! for the axiom shapes our IR can faithfully represent.
//!
//! Lossy on the source side only where Phase 0 already errors during
//! forward conversion (inverse roles, anonymous individuals, data ranges),
//! so the reverse never has to produce those.
//!
//! Concept-level rewrites in [`crate::convert`] are *not* inverted bit-for-
//! bit. Specifically:
//!
//! - `Nominal(i)` reverses to `ObjectOneOf(vec![i])` — there's no "naked
//!   individual" `ClassExpression` in OWL.
//! - `Top` / `Bot` reverse to `ObjectIntersectionOf([])` / `ObjectUnionOf([])`.
//!
//! These choices are logically equivalent but not syntactically identical
//! to whatever the user wrote on the way in. Round-trip tests therefore
//! check axiom equality *after a second forward conversion* in the same
//! ontology — see `tests/convert_round_trip_proptest.rs`.

use horned_owl::model::{
    AnnotatedComponent, AsymmetricObjectProperty, Build, Class, ClassAssertion, ClassExpression,
    Component, DeclareClass, DeclareNamedIndividual, DeclareObjectProperty, DifferentIndividuals,
    DisjointClasses, DisjointObjectProperties, DisjointUnion, EquivalentClasses,
    EquivalentObjectProperties, FunctionalObjectProperty, Individual,
    InverseFunctionalObjectProperty, InverseObjectProperties, IrreflexiveObjectProperty,
    MutableOntology, NamedIndividual, NegativeObjectPropertyAssertion, ObjectProperty,
    ObjectPropertyAssertion, ObjectPropertyDomain, ObjectPropertyExpression, ObjectPropertyRange,
    RcStr, ReflexiveObjectProperty, SameIndividual, SubClassOf, SubObjectPropertyExpression,
    SubObjectPropertyOf, SymmetricObjectProperty, TransitiveObjectProperty,
};
use horned_owl::ontology::set::SetOntology;

use crate::Vocabulary;
use crate::ir::{ClassId, ConceptExpr, ConceptId, IndividualId, Role};
use crate::ontology::{Axiom, InternalOntology, SubRolePath};

/// Reverse a [`ConceptId`] into a horned-owl [`ClassExpression`].
pub fn concept_to_class_expression(
    cid: ConceptId,
    ontology: &InternalOntology,
    build: &Build<RcStr>,
) -> ClassExpression<RcStr> {
    match ontology.concepts.get(cid) {
        ConceptExpr::Top => ClassExpression::ObjectIntersectionOf(vec![]),
        ConceptExpr::Bot => ClassExpression::ObjectUnionOf(vec![]),
        ConceptExpr::Atomic(class_id) => {
            ClassExpression::Class(class_to_named(*class_id, &ontology.vocabulary, build))
        }
        ConceptExpr::Nominal(ind_id) => ClassExpression::ObjectOneOf(vec![Individual::Named(
            named_individual(*ind_id, &ontology.vocabulary, build),
        )]),
        ConceptExpr::SelfRestriction(role) => {
            ClassExpression::ObjectHasSelf(role_to_ope(*role, &ontology.vocabulary, build))
        }
        ConceptExpr::Not(inner) => ClassExpression::ObjectComplementOf(Box::new(
            concept_to_class_expression(*inner, ontology, build),
        )),
        ConceptExpr::And(args) => ClassExpression::ObjectIntersectionOf(
            args.iter()
                .map(|&c| concept_to_class_expression(c, ontology, build))
                .collect(),
        ),
        ConceptExpr::Or(args) => ClassExpression::ObjectUnionOf(
            args.iter()
                .map(|&c| concept_to_class_expression(c, ontology, build))
                .collect(),
        ),
        ConceptExpr::Some(role, inner) => ClassExpression::ObjectSomeValuesFrom {
            ope: role_to_ope(*role, &ontology.vocabulary, build),
            bce: Box::new(concept_to_class_expression(*inner, ontology, build)),
        },
        ConceptExpr::All(role, inner) => ClassExpression::ObjectAllValuesFrom {
            ope: role_to_ope(*role, &ontology.vocabulary, build),
            bce: Box::new(concept_to_class_expression(*inner, ontology, build)),
        },
        ConceptExpr::Min(n, role, inner) => ClassExpression::ObjectMinCardinality {
            n: *n,
            ope: role_to_ope(*role, &ontology.vocabulary, build),
            bce: Box::new(concept_to_class_expression(*inner, ontology, build)),
        },
        ConceptExpr::Max(n, role, inner) => ClassExpression::ObjectMaxCardinality {
            n: *n,
            ope: role_to_ope(*role, &ontology.vocabulary, build),
            bce: Box::new(concept_to_class_expression(*inner, ontology, build)),
        },
    }
}

/// Reverse a [`Role`] (named only in Phase 0) into an
/// [`ObjectPropertyExpression`].
fn role_to_ope(
    role: Role,
    vocab: &Vocabulary,
    build: &Build<RcStr>,
) -> ObjectPropertyExpression<RcStr> {
    let iri = vocab.role_iri(role.role_id());
    ObjectPropertyExpression::ObjectProperty(build.object_property(iri))
}

fn class_to_named(id: ClassId, vocab: &Vocabulary, build: &Build<RcStr>) -> Class<RcStr> {
    build.class(vocab.class_iri(id))
}

fn named_individual(
    id: IndividualId,
    vocab: &Vocabulary,
    build: &Build<RcStr>,
) -> NamedIndividual<RcStr> {
    build.named_individual(vocab.individual_iri(id))
}

fn role_to_named(role: Role, vocab: &Vocabulary, build: &Build<RcStr>) -> ObjectProperty<RcStr> {
    build.object_property(vocab.role_iri(role.role_id()))
}

fn ids_to_class_expressions(
    ids: &[ConceptId],
    ontology: &InternalOntology,
    build: &Build<RcStr>,
) -> Vec<ClassExpression<RcStr>> {
    ids.iter()
        .map(|&c| concept_to_class_expression(c, ontology, build))
        .collect()
}

fn roles_to_opes(
    roles: &[Role],
    vocab: &Vocabulary,
    build: &Build<RcStr>,
) -> Vec<ObjectPropertyExpression<RcStr>> {
    roles
        .iter()
        .map(|&r| role_to_ope(r, vocab, build))
        .collect()
}

fn individuals_to_individuals(
    ids: &[IndividualId],
    vocab: &Vocabulary,
    build: &Build<RcStr>,
) -> Vec<Individual<RcStr>> {
    ids.iter()
        .map(|&i| Individual::Named(named_individual(i, vocab, build)))
        .collect()
}

fn sub_role_path_to_sub_ope(
    path: &SubRolePath,
    vocab: &Vocabulary,
    build: &Build<RcStr>,
) -> SubObjectPropertyExpression<RcStr> {
    match path {
        SubRolePath::Role(r) => {
            SubObjectPropertyExpression::ObjectPropertyExpression(role_to_ope(*r, vocab, build))
        }
        SubRolePath::Chain(chain) => {
            SubObjectPropertyExpression::ObjectPropertyChain(roles_to_opes(chain, vocab, build))
        }
    }
}

/// Reverse an [`Axiom`] into a horned-owl [`Component`].
#[allow(clippy::too_many_lines)] // intrinsic to the breadth of the Axiom enum
pub fn axiom_to_component(
    axiom: &Axiom,
    ontology: &InternalOntology,
    build: &Build<RcStr>,
) -> Component<RcStr> {
    let vocab = &ontology.vocabulary;
    match axiom {
        // ── TBox ────────────────────────────────────────────────────────
        Axiom::SubClassOf { sub, sup } => Component::SubClassOf(SubClassOf {
            sub: concept_to_class_expression(*sub, ontology, build),
            sup: concept_to_class_expression(*sup, ontology, build),
        }),
        Axiom::EquivalentClasses(ids) => Component::EquivalentClasses(EquivalentClasses(
            ids_to_class_expressions(ids, ontology, build),
        )),
        Axiom::DisjointClasses(ids) => Component::DisjointClasses(DisjointClasses(
            ids_to_class_expressions(ids, ontology, build),
        )),
        Axiom::DisjointUnion { class, members } => Component::DisjointUnion(DisjointUnion(
            class_to_named(*class, vocab, build),
            ids_to_class_expressions(members, ontology, build),
        )),

        // ── RBox ────────────────────────────────────────────────────────
        Axiom::SubObjectPropertyOf { sub, sup } => {
            Component::SubObjectPropertyOf(SubObjectPropertyOf {
                sub: sub_role_path_to_sub_ope(sub, vocab, build),
                sup: role_to_ope(*sup, vocab, build),
            })
        }
        Axiom::EquivalentObjectProperties(roles) => Component::EquivalentObjectProperties(
            EquivalentObjectProperties(roles_to_opes(roles, vocab, build)),
        ),
        Axiom::DisjointObjectProperties(roles) => Component::DisjointObjectProperties(
            DisjointObjectProperties(roles_to_opes(roles, vocab, build)),
        ),
        Axiom::InverseObjectProperties(a, b) => {
            Component::InverseObjectProperties(InverseObjectProperties(
                role_to_named(*a, vocab, build),
                role_to_named(*b, vocab, build),
            ))
        }
        Axiom::ObjectPropertyDomain { role, domain } => {
            Component::ObjectPropertyDomain(ObjectPropertyDomain {
                ope: role_to_ope(*role, vocab, build),
                ce: concept_to_class_expression(*domain, ontology, build),
            })
        }
        Axiom::ObjectPropertyRange { role, range } => {
            Component::ObjectPropertyRange(ObjectPropertyRange {
                ope: role_to_ope(*role, vocab, build),
                ce: concept_to_class_expression(*range, ontology, build),
            })
        }
        Axiom::TransitiveRole(role) => Component::TransitiveObjectProperty(
            TransitiveObjectProperty(role_to_ope(*role, vocab, build)),
        ),
        Axiom::SymmetricRole(role) => Component::SymmetricObjectProperty(SymmetricObjectProperty(
            role_to_ope(*role, vocab, build),
        )),
        Axiom::AsymmetricRole(role) => Component::AsymmetricObjectProperty(
            AsymmetricObjectProperty(role_to_ope(*role, vocab, build)),
        ),
        Axiom::ReflexiveRole(role) => Component::ReflexiveObjectProperty(ReflexiveObjectProperty(
            role_to_ope(*role, vocab, build),
        )),
        Axiom::IrreflexiveRole(role) => Component::IrreflexiveObjectProperty(
            IrreflexiveObjectProperty(role_to_ope(*role, vocab, build)),
        ),
        Axiom::FunctionalRole(role) => Component::FunctionalObjectProperty(
            FunctionalObjectProperty(role_to_ope(*role, vocab, build)),
        ),
        Axiom::InverseFunctionalRole(role) => Component::InverseFunctionalObjectProperty(
            InverseFunctionalObjectProperty(role_to_ope(*role, vocab, build)),
        ),

        // ── ABox ────────────────────────────────────────────────────────
        Axiom::ClassAssertion { class, individual } => Component::ClassAssertion(ClassAssertion {
            ce: concept_to_class_expression(*class, ontology, build),
            i: Individual::Named(named_individual(*individual, vocab, build)),
        }),
        Axiom::ObjectPropertyAssertion {
            role,
            subject,
            object,
        } => Component::ObjectPropertyAssertion(ObjectPropertyAssertion {
            ope: role_to_ope(*role, vocab, build),
            from: Individual::Named(named_individual(*subject, vocab, build)),
            to: Individual::Named(named_individual(*object, vocab, build)),
        }),
        Axiom::NegativeObjectPropertyAssertion {
            role,
            subject,
            object,
        } => Component::NegativeObjectPropertyAssertion(NegativeObjectPropertyAssertion {
            ope: role_to_ope(*role, vocab, build),
            from: Individual::Named(named_individual(*subject, vocab, build)),
            to: Individual::Named(named_individual(*object, vocab, build)),
        }),
        Axiom::SameIndividual(ids) => Component::SameIndividual(SameIndividual(
            individuals_to_individuals(ids, vocab, build),
        )),
        Axiom::DifferentIndividuals(ids) => Component::DifferentIndividuals(DifferentIndividuals(
            individuals_to_individuals(ids, vocab, build),
        )),

        // ── Declarations ────────────────────────────────────────────────
        Axiom::DeclareClass(id) => {
            Component::DeclareClass(DeclareClass(class_to_named(*id, vocab, build)))
        }
        Axiom::DeclareObjectProperty(id) => Component::DeclareObjectProperty(
            DeclareObjectProperty(build.object_property(vocab.role_iri(*id))),
        ),
        Axiom::DeclareNamedIndividual(id) => Component::DeclareNamedIndividual(
            DeclareNamedIndividual(named_individual(*id, vocab, build)),
        ),
    }
}

/// Reverse an entire [`InternalOntology`] into a horned-owl [`SetOntology`].
pub fn convert_back(ontology: &InternalOntology) -> SetOntology<RcStr> {
    let build = Build::new_rc();
    let mut out = SetOntology::<RcStr>::new();
    for axiom in &ontology.axioms {
        let component = axiom_to_component(axiom, ontology, &build);
        out.insert(AnnotatedComponent::from(component));
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::convert::{convert_component, convert_ontology};
    use horned_owl::model as ho;

    fn b() -> Build<RcStr> {
        Build::new_rc()
    }

    fn ce_class(name: &str) -> ClassExpression<RcStr> {
        ClassExpression::Class(b().class(name))
    }

    fn ope(name: &str) -> ObjectPropertyExpression<RcStr> {
        ObjectPropertyExpression::ObjectProperty(b().object_property(name))
    }

    fn ind(name: &str) -> Individual<RcStr> {
        Individual::Named(b().named_individual(name))
    }

    /// Round-trip a single component through the ONE pool/vocab pair.
    /// Returns `(first_forward_axiom, second_forward_axiom)` — they should be
    /// equal whenever the round-trip is faithful.
    fn round_trip(c: &Component<RcStr>) -> (Axiom, Axiom) {
        let mut o = InternalOntology::new();
        let ax_1 = convert_component(c, &mut o.vocabulary, &mut o.concepts)
            .unwrap()
            .unwrap();
        let build = Build::new_rc();
        let c_back = axiom_to_component(&ax_1, &o, &build);
        let ax_2 = convert_component(&c_back, &mut o.vocabulary, &mut o.concepts)
            .unwrap()
            .unwrap();
        (ax_1, ax_2)
    }

    #[test]
    fn sub_class_of_round_trips() {
        let (ax1, ax2) = round_trip(&Component::SubClassOf(ho::SubClassOf {
            sub: ce_class("A"),
            sup: ClassExpression::ObjectIntersectionOf(vec![ce_class("B"), ce_class("C")]),
        }));
        assert_eq!(ax1, ax2);
    }

    #[test]
    fn equivalent_classes_round_trips() {
        let (ax1, ax2) = round_trip(&Component::EquivalentClasses(ho::EquivalentClasses(vec![
            ce_class("A"),
            ce_class("B"),
        ])));
        assert_eq!(ax1, ax2);
    }

    #[test]
    fn disjoint_classes_round_trips() {
        let (ax1, ax2) = round_trip(&Component::DisjointClasses(ho::DisjointClasses(vec![
            ce_class("A"),
            ce_class("B"),
        ])));
        assert_eq!(ax1, ax2);
    }

    #[test]
    fn disjoint_union_round_trips() {
        let (ax1, ax2) = round_trip(&Component::DisjointUnion(ho::DisjointUnion(
            b().class("Parent"),
            vec![ce_class("C1"), ce_class("C2")],
        )));
        assert_eq!(ax1, ax2);
    }

    #[test]
    fn sub_object_property_of_chain_round_trips() {
        let (ax1, ax2) = round_trip(&Component::SubObjectPropertyOf(ho::SubObjectPropertyOf {
            sub: SubObjectPropertyExpression::ObjectPropertyChain(vec![ope("r"), ope("s")]),
            sup: ope("t"),
        }));
        assert_eq!(ax1, ax2);
    }

    #[test]
    fn role_characteristics_round_trip() {
        let cases: Vec<Component<RcStr>> = vec![
            Component::TransitiveObjectProperty(ho::TransitiveObjectProperty(ope("r"))),
            Component::SymmetricObjectProperty(ho::SymmetricObjectProperty(ope("r"))),
            Component::AsymmetricObjectProperty(ho::AsymmetricObjectProperty(ope("r"))),
            Component::ReflexiveObjectProperty(ho::ReflexiveObjectProperty(ope("r"))),
            Component::IrreflexiveObjectProperty(ho::IrreflexiveObjectProperty(ope("r"))),
            Component::FunctionalObjectProperty(ho::FunctionalObjectProperty(ope("r"))),
            Component::InverseFunctionalObjectProperty(ho::InverseFunctionalObjectProperty(ope(
                "r",
            ))),
        ];
        for c in cases {
            let (ax1, ax2) = round_trip(&c);
            assert_eq!(ax1, ax2);
        }
    }

    #[test]
    fn inverse_object_properties_round_trips() {
        let (ax1, ax2) = round_trip(&Component::InverseObjectProperties(
            ho::InverseObjectProperties(b().object_property("r"), b().object_property("s")),
        ));
        assert_eq!(ax1, ax2);
    }

    #[test]
    fn object_property_domain_range_round_trip() {
        let (ax1, ax2) = round_trip(&Component::ObjectPropertyDomain(ho::ObjectPropertyDomain {
            ope: ope("r"),
            ce: ce_class("A"),
        }));
        assert_eq!(ax1, ax2);
        let (ax1, ax2) = round_trip(&Component::ObjectPropertyRange(ho::ObjectPropertyRange {
            ope: ope("r"),
            ce: ce_class("A"),
        }));
        assert_eq!(ax1, ax2);
    }

    #[test]
    fn class_assertion_round_trips() {
        let (ax1, ax2) = round_trip(&Component::ClassAssertion(ho::ClassAssertion {
            ce: ce_class("A"),
            i: ind("a"),
        }));
        assert_eq!(ax1, ax2);
    }

    #[test]
    fn object_property_assertions_round_trip() {
        let (ax1, ax2) = round_trip(&Component::ObjectPropertyAssertion(
            ho::ObjectPropertyAssertion {
                ope: ope("r"),
                from: ind("a"),
                to: ind("b"),
            },
        ));
        assert_eq!(ax1, ax2);
        let (ax1, ax2) = round_trip(&Component::NegativeObjectPropertyAssertion(
            ho::NegativeObjectPropertyAssertion {
                ope: ope("r"),
                from: ind("a"),
                to: ind("b"),
            },
        ));
        assert_eq!(ax1, ax2);
    }

    #[test]
    fn same_and_different_individuals_round_trip() {
        let (ax1, ax2) = round_trip(&Component::SameIndividual(ho::SameIndividual(vec![
            ind("a"),
            ind("b"),
        ])));
        assert_eq!(ax1, ax2);
        let (ax1, ax2) = round_trip(&Component::DifferentIndividuals(ho::DifferentIndividuals(
            vec![ind("a"), ind("c")],
        )));
        assert_eq!(ax1, ax2);
    }

    #[test]
    fn declarations_round_trip() {
        let (ax1, ax2) = round_trip(&Component::DeclareClass(ho::DeclareClass(b().class("A"))));
        assert_eq!(ax1, ax2);
        let (ax1, ax2) = round_trip(&Component::DeclareObjectProperty(
            ho::DeclareObjectProperty(b().object_property("r")),
        ));
        assert_eq!(ax1, ax2);
        let (ax1, ax2) = round_trip(&Component::DeclareNamedIndividual(
            ho::DeclareNamedIndividual(b().named_individual("a")),
        ));
        assert_eq!(ax1, ax2);
    }

    #[test]
    fn has_value_then_back_uses_one_of_encoding() {
        // ObjectHasValue → IR Some(r, Nominal(i)) → back to
        // ObjectSomeValuesFrom { ope, bce: ObjectOneOf([i]) }.
        // The two forms are logically equivalent, so a second forward
        // conversion in the SAME pool reaches the SAME ConceptId pair —
        // hence the axioms compare equal.
        let (ax1, ax2) = round_trip(&Component::SubClassOf(ho::SubClassOf {
            sub: ce_class("A"),
            sup: ClassExpression::ObjectHasValue {
                ope: ope("r"),
                i: ind("a"),
            },
        }));
        assert_eq!(ax1, ax2);
    }

    #[test]
    fn convert_back_smoke() {
        let mut so = SetOntology::<RcStr>::new();
        so.insert(AnnotatedComponent::from(Component::SubClassOf(
            ho::SubClassOf {
                sub: ce_class("A"),
                sup: ce_class("B"),
            },
        )));
        so.insert(AnnotatedComponent::from(Component::DeclareClass(
            ho::DeclareClass(b().class("A")),
        )));
        let internal = convert_ontology(&so).unwrap();
        let so2 = convert_back(&internal);
        assert_eq!(so2.iter().count(), 2);
        // Re-converting must give the same number of axioms.
        let internal2 = convert_ontology(&so2).unwrap();
        assert_eq!(internal2.num_axioms(), internal.num_axioms());
    }
}
