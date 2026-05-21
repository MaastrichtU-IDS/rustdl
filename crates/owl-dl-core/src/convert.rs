//! Conversion from `horned-owl`'s model into our [`InternalOntology`].
//!
//! Concept-level conversion is here (Day 10). Axiom-level conversion lands
//! in Day 11; the reverse direction and round-trip proptest in Day 12.

use horned_owl::model::{ClassExpression, ForIRI, Individual, ObjectPropertyExpression};
use thiserror::Error;

use crate::ConceptPool;
use crate::Vocabulary;
use crate::ir::{ConceptId, IndividualId, Role};

/// Errors produced by conversion from `horned-owl` to our IR.
#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum ConversionError {
    /// A class expression variant our IR cannot represent in this phase.
    /// The `kind` field names the offending constructor.
    #[error("unsupported class expression variant: {kind}")]
    UnsupportedConcept { kind: &'static str },

    /// An axiom variant our IR cannot represent in this phase.
    #[error("unsupported axiom kind: {kind}")]
    UnsupportedAxiom { kind: &'static str },

    /// Anonymous individuals are not part of our IR in Phase 0; they are
    /// scheduled for the `ABox` work in Phase 7.
    #[error("anonymous individuals are not supported (planned for Phase 7)")]
    AnonymousIndividual,

    /// Data ranges (everything `xsd:*`-like) wait until Phase 3 minimal
    /// datatype support and Phase 7 full concrete domains.
    #[error("data ranges and data properties are not supported until Phase 3")]
    UnsupportedDataRange,
}

/// Convert a horned-owl [`ClassExpression`] to a [`ConceptId`] in `pool`,
/// interning any encountered class IRIs into `vocab`.
///
/// Concept-level rewriting is performed here because our IR has no direct
/// counterpart for some horned-owl constructors:
///
/// | horned-owl                  | IR encoding              |
/// |-----------------------------|--------------------------|
/// | `ObjectHasValue { r, i }`   | `Some(r, Nominal(i))`    |
/// | `ObjectExactCardinality`    | `And(Min(n,r,c), Max(n,r,c))` |
/// | `ObjectOneOf([a, b, ...])`  | `Or(Nominal(a), Nominal(b), ...)` |
/// | `ObjectIntersectionOf([])`  | `Top`                    |
/// | `ObjectUnionOf([])`         | `Bot`                    |
///
/// These rewrites are logically lossless — our IR's `And/Or/Some/Max/Min`
/// already canonicalize internally.
pub fn convert_class_expression<A: ForIRI>(
    ce: &ClassExpression<A>,
    vocab: &mut Vocabulary,
    pool: &mut ConceptPool,
) -> Result<ConceptId, ConversionError> {
    match ce {
        ClassExpression::Class(c) => {
            let iri: &str = c.0.as_ref();
            let class_id = vocab.intern_class(iri);
            Ok(pool.atomic(class_id))
        }
        ClassExpression::ObjectIntersectionOf(xs) => {
            let ids = convert_many(xs, vocab, pool)?;
            if ids.is_empty() {
                Ok(pool.top())
            } else {
                Ok(pool.and(ids))
            }
        }
        ClassExpression::ObjectUnionOf(xs) => {
            let ids = convert_many(xs, vocab, pool)?;
            if ids.is_empty() {
                Ok(pool.bot())
            } else {
                Ok(pool.or(ids))
            }
        }
        ClassExpression::ObjectComplementOf(inner) => {
            let inner_id = convert_class_expression(inner, vocab, pool)?;
            Ok(pool.not(inner_id))
        }
        ClassExpression::ObjectOneOf(xs) => {
            let mut ids = Vec::with_capacity(xs.len());
            for ind in xs {
                let id = convert_individual(ind, vocab)?;
                ids.push(pool.nominal(id));
            }
            if ids.is_empty() {
                Ok(pool.bot())
            } else {
                Ok(pool.or(ids))
            }
        }
        ClassExpression::ObjectSomeValuesFrom { ope, bce } => {
            let role = convert_object_property(ope, vocab)?;
            let inner = convert_class_expression(bce, vocab, pool)?;
            Ok(pool.some(role, inner))
        }
        ClassExpression::ObjectAllValuesFrom { ope, bce } => {
            let role = convert_object_property(ope, vocab)?;
            let inner = convert_class_expression(bce, vocab, pool)?;
            Ok(pool.all(role, inner))
        }
        ClassExpression::ObjectHasValue { ope, i } => {
            let role = convert_object_property(ope, vocab)?;
            let ind = convert_individual(i, vocab)?;
            let nom = pool.nominal(ind);
            Ok(pool.some(role, nom))
        }
        ClassExpression::ObjectHasSelf(ope) => {
            let role = convert_object_property(ope, vocab)?;
            Ok(pool.self_restriction(role))
        }
        ClassExpression::ObjectMinCardinality { n, ope, bce } => {
            let role = convert_object_property(ope, vocab)?;
            let inner = convert_class_expression(bce, vocab, pool)?;
            Ok(pool.min(*n, role, inner))
        }
        ClassExpression::ObjectMaxCardinality { n, ope, bce } => {
            let role = convert_object_property(ope, vocab)?;
            let inner = convert_class_expression(bce, vocab, pool)?;
            Ok(pool.max(*n, role, inner))
        }
        ClassExpression::ObjectExactCardinality { n, ope, bce } => {
            let role = convert_object_property(ope, vocab)?;
            let inner = convert_class_expression(bce, vocab, pool)?;
            let lo = pool.min(*n, role, inner);
            let hi = pool.max(*n, role, inner);
            Ok(pool.and([lo, hi]))
        }
        ClassExpression::DataSomeValuesFrom { .. }
        | ClassExpression::DataAllValuesFrom { .. }
        | ClassExpression::DataHasValue { .. }
        | ClassExpression::DataMinCardinality { .. }
        | ClassExpression::DataMaxCardinality { .. }
        | ClassExpression::DataExactCardinality { .. } => {
            Err(ConversionError::UnsupportedDataRange)
        }
    }
}

fn convert_many<A: ForIRI>(
    xs: &[ClassExpression<A>],
    vocab: &mut Vocabulary,
    pool: &mut ConceptPool,
) -> Result<Vec<ConceptId>, ConversionError> {
    let mut out = Vec::with_capacity(xs.len());
    for ce in xs {
        out.push(convert_class_expression(ce, vocab, pool)?);
    }
    Ok(out)
}

/// Convert a horned-owl [`ObjectPropertyExpression`] to a [`Role`].
///
/// `InverseObjectProperty` is rejected in Phase 0: our [`Role`] type does
/// not yet carry an `inverted: bool` field (Phase 3 adds it). Returning an
/// error here, rather than silently dropping the inversion, surfaces the
/// gap explicitly.
pub fn convert_object_property<A: ForIRI>(
    ope: &ObjectPropertyExpression<A>,
    vocab: &mut Vocabulary,
) -> Result<Role, ConversionError> {
    match ope {
        ObjectPropertyExpression::ObjectProperty(op) => {
            let iri: &str = op.0.as_ref();
            Ok(Role::named(vocab.intern_role(iri)))
        }
        ObjectPropertyExpression::InverseObjectProperty(_) => {
            Err(ConversionError::UnsupportedConcept {
                kind: "InverseObjectProperty",
            })
        }
    }
}

/// Convert a horned-owl [`Individual`] (named only — anonymous is rejected).
pub fn convert_individual<A: ForIRI>(
    i: &Individual<A>,
    vocab: &mut Vocabulary,
) -> Result<IndividualId, ConversionError> {
    match i {
        Individual::Named(ni) => {
            let iri: &str = ni.0.as_ref();
            Ok(vocab.intern_individual(iri))
        }
        Individual::Anonymous(_) => Err(ConversionError::AnonymousIndividual),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::ir::ConceptExpr;
    use horned_owl::model::{Build, RcStr};

    fn b() -> Build<RcStr> {
        Build::new_rc()
    }

    fn ctx() -> (Vocabulary, ConceptPool) {
        (Vocabulary::new(), ConceptPool::new())
    }

    #[test]
    fn class() {
        let (mut v, mut p) = ctx();
        let ce = ClassExpression::Class(b().class("http://example.org/A"));
        let id = convert_class_expression(&ce, &mut v, &mut p).unwrap();
        let ConceptExpr::Atomic(c) = p.get(id) else {
            panic!("expected Atomic")
        };
        assert_eq!(v.class_iri(*c), "http://example.org/A");
    }

    #[test]
    fn intersection() {
        let (mut v, mut p) = ctx();
        let ce = ClassExpression::ObjectIntersectionOf(vec![
            ClassExpression::Class(b().class("A")),
            ClassExpression::Class(b().class("B")),
        ]);
        let id = convert_class_expression(&ce, &mut v, &mut p).unwrap();
        let ConceptExpr::And(args) = p.get(id) else {
            panic!("expected And")
        };
        assert_eq!(args.len(), 2);
    }

    #[test]
    fn empty_intersection_is_top() {
        let (mut v, mut p) = ctx();
        let ce: ClassExpression<RcStr> = ClassExpression::ObjectIntersectionOf(vec![]);
        let id = convert_class_expression(&ce, &mut v, &mut p).unwrap();
        assert!(matches!(p.get(id), ConceptExpr::Top));
    }

    #[test]
    fn union() {
        let (mut v, mut p) = ctx();
        let ce = ClassExpression::ObjectUnionOf(vec![
            ClassExpression::Class(b().class("A")),
            ClassExpression::Class(b().class("B")),
        ]);
        let id = convert_class_expression(&ce, &mut v, &mut p).unwrap();
        assert!(matches!(p.get(id), ConceptExpr::Or(_)));
    }

    #[test]
    fn empty_union_is_bot() {
        let (mut v, mut p) = ctx();
        let ce: ClassExpression<RcStr> = ClassExpression::ObjectUnionOf(vec![]);
        let id = convert_class_expression(&ce, &mut v, &mut p).unwrap();
        assert!(matches!(p.get(id), ConceptExpr::Bot));
    }

    #[test]
    fn complement() {
        let (mut v, mut p) = ctx();
        let ce =
            ClassExpression::ObjectComplementOf(Box::new(ClassExpression::Class(b().class("A"))));
        let id = convert_class_expression(&ce, &mut v, &mut p).unwrap();
        assert!(matches!(p.get(id), ConceptExpr::Not(_)));
    }

    #[test]
    fn some_values_from() {
        let (mut v, mut p) = ctx();
        let ce = ClassExpression::ObjectSomeValuesFrom {
            ope: ObjectPropertyExpression::ObjectProperty(b().object_property("r")),
            bce: Box::new(ClassExpression::Class(b().class("A"))),
        };
        let id = convert_class_expression(&ce, &mut v, &mut p).unwrap();
        assert!(matches!(p.get(id), ConceptExpr::Some(_, _)));
    }

    #[test]
    fn all_values_from() {
        let (mut v, mut p) = ctx();
        let ce = ClassExpression::ObjectAllValuesFrom {
            ope: ObjectPropertyExpression::ObjectProperty(b().object_property("r")),
            bce: Box::new(ClassExpression::Class(b().class("A"))),
        };
        let id = convert_class_expression(&ce, &mut v, &mut p).unwrap();
        assert!(matches!(p.get(id), ConceptExpr::All(_, _)));
    }

    #[test]
    fn has_value_encodes_as_some_of_nominal() {
        let (mut v, mut p) = ctx();
        let ce = ClassExpression::ObjectHasValue {
            ope: ObjectPropertyExpression::ObjectProperty(b().object_property("r")),
            i: Individual::Named(b().named_individual("a")),
        };
        let id = convert_class_expression(&ce, &mut v, &mut p).unwrap();
        let ConceptExpr::Some(_, inner) = p.get(id) else {
            panic!("expected Some(_, _)")
        };
        assert!(matches!(p.get(*inner), ConceptExpr::Nominal(_)));
    }

    #[test]
    fn has_self() {
        let (mut v, mut p) = ctx();
        let ce = ClassExpression::ObjectHasSelf(ObjectPropertyExpression::ObjectProperty(
            b().object_property("r"),
        ));
        let id = convert_class_expression(&ce, &mut v, &mut p).unwrap();
        assert!(matches!(p.get(id), ConceptExpr::SelfRestriction(_)));
    }

    #[test]
    fn min_cardinality() {
        let (mut v, mut p) = ctx();
        let ce = ClassExpression::ObjectMinCardinality {
            n: 3,
            ope: ObjectPropertyExpression::ObjectProperty(b().object_property("r")),
            bce: Box::new(ClassExpression::Class(b().class("A"))),
        };
        let id = convert_class_expression(&ce, &mut v, &mut p).unwrap();
        let ConceptExpr::Min(n, _, _) = p.get(id) else {
            panic!("expected Min")
        };
        assert_eq!(*n, 3);
    }

    #[test]
    fn max_cardinality() {
        let (mut v, mut p) = ctx();
        let ce = ClassExpression::ObjectMaxCardinality {
            n: 5,
            ope: ObjectPropertyExpression::ObjectProperty(b().object_property("r")),
            bce: Box::new(ClassExpression::Class(b().class("A"))),
        };
        let id = convert_class_expression(&ce, &mut v, &mut p).unwrap();
        let ConceptExpr::Max(n, _, _) = p.get(id) else {
            panic!("expected Max")
        };
        assert_eq!(*n, 5);
    }

    #[test]
    fn exact_cardinality_encodes_as_and_of_min_max() {
        let (mut v, mut p) = ctx();
        let ce = ClassExpression::ObjectExactCardinality {
            n: 2,
            ope: ObjectPropertyExpression::ObjectProperty(b().object_property("r")),
            bce: Box::new(ClassExpression::Class(b().class("A"))),
        };
        let id = convert_class_expression(&ce, &mut v, &mut p).unwrap();
        let ConceptExpr::And(args) = p.get(id) else {
            panic!("expected And(Min, Max)")
        };
        assert_eq!(args.len(), 2);
        // One of the conjuncts is Min, the other Max.
        let kinds: Vec<&'static str> = args
            .iter()
            .map(|a| match p.get(*a) {
                ConceptExpr::Min(..) => "Min",
                ConceptExpr::Max(..) => "Max",
                _ => "other",
            })
            .collect();
        assert!(kinds.contains(&"Min"));
        assert!(kinds.contains(&"Max"));
    }

    #[test]
    fn one_of_encodes_as_or_of_nominals() {
        let (mut v, mut p) = ctx();
        let ce = ClassExpression::ObjectOneOf(vec![
            Individual::Named(b().named_individual("a")),
            Individual::Named(b().named_individual("b")),
        ]);
        let id = convert_class_expression(&ce, &mut v, &mut p).unwrap();
        let ConceptExpr::Or(args) = p.get(id) else {
            panic!("expected Or")
        };
        assert_eq!(args.len(), 2);
        for a in args {
            assert!(matches!(p.get(*a), ConceptExpr::Nominal(_)));
        }
    }

    #[test]
    fn inverse_object_property_rejected() {
        let mut v = Vocabulary::new();
        let ope =
            ObjectPropertyExpression::<RcStr>::InverseObjectProperty(b().object_property("r"));
        let err = convert_object_property(&ope, &mut v).unwrap_err();
        assert_eq!(
            err,
            ConversionError::UnsupportedConcept {
                kind: "InverseObjectProperty"
            }
        );
    }

    #[test]
    fn anonymous_individual_rejected() {
        use horned_owl::model::AnonymousIndividual;
        use std::rc::Rc;

        let mut v = Vocabulary::new();
        let i = Individual::<RcStr>::Anonymous(AnonymousIndividual(Rc::from("blank-0")));
        let err = convert_individual(&i, &mut v).unwrap_err();
        assert_eq!(err, ConversionError::AnonymousIndividual);
    }

    #[test]
    fn data_some_values_rejected() {
        let (mut v, mut p) = ctx();
        let ce: ClassExpression<RcStr> = ClassExpression::DataSomeValuesFrom {
            dp: b().data_property("dp"),
            dr: horned_owl::model::DataRange::Datatype(b().datatype("dt")),
        };
        let err = convert_class_expression(&ce, &mut v, &mut p).unwrap_err();
        assert_eq!(err, ConversionError::UnsupportedDataRange);
    }

    #[test]
    fn shared_subexpressions_share_ids() {
        let (mut v, mut p) = ctx();
        let ce1 = ClassExpression::Class(b().class("A"));
        let ce2 = ClassExpression::Class(b().class("A"));
        let id1 = convert_class_expression(&ce1, &mut v, &mut p).unwrap();
        let id2 = convert_class_expression(&ce2, &mut v, &mut p).unwrap();
        assert_eq!(id1, id2);
        assert_eq!(p.len(), 1);
        assert_eq!(v.num_classes(), 1);
    }
}
