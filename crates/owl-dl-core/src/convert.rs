//! Conversion from `horned-owl`'s model into our [`InternalOntology`].
//!
//! - Day 10: concept-level conversion ([`convert_class_expression`],
//!   [`convert_object_property`], [`convert_individual`]).
//! - Day 11: axiom-level conversion ([`convert_component`],
//!   [`convert_ontology`]) — this file.
//! - Day 12: reverse conversion + round-trip proptest (still to come).

use horned_owl::model::{AnnotatedComponent, Class, SubObjectPropertyExpression};
use horned_owl::model::{ClassExpression, Component, ForIRI, Individual, ObjectPropertyExpression};
use horned_owl::ontology::set::SetOntology;
use thiserror::Error;

use crate::ConceptPool;
use crate::Vocabulary;
use crate::ir::{ClassId, ConceptId, IndividualId, Role};
use crate::ontology::{Axiom, InternalOntology, SubRolePath};

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
            // OWL 2 built-in vocabulary: owl:Thing ≡ ⊤, owl:Nothing ≡ ⊥.
            // The IRI form is the only legal way to refer to them in
            // ClassExpression (horned-owl has no dedicated Top/Bottom
            // variants), so we intercept here and lower to the IR's
            // structural Top/Bot rather than interning the IRI as if
            // it were an arbitrary user class.
            match iri {
                "http://www.w3.org/2002/07/owl#Thing" => Ok(pool.top()),
                "http://www.w3.org/2002/07/owl#Nothing" => Ok(pool.bot()),
                _ => {
                    let class_id = vocab.intern_class(iri);
                    Ok(pool.atomic(class_id))
                }
            }
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
/// `InverseObjectProperty` lowers to [`Role::Inverse`] as of Phase 3
/// commit 2. The named property inside the inversion is interned in
/// the role vocabulary just like a forward use; downstream rules
/// decide direction by inspecting [`Role::is_inverse`].
pub fn convert_object_property<A: ForIRI>(
    ope: &ObjectPropertyExpression<A>,
    vocab: &mut Vocabulary,
) -> Result<Role, ConversionError> {
    match ope {
        ObjectPropertyExpression::ObjectProperty(op) => {
            let iri: &str = op.0.as_ref();
            Ok(Role::named(vocab.intern_role(iri)))
        }
        ObjectPropertyExpression::InverseObjectProperty(op) => {
            let iri: &str = op.0.as_ref();
            Ok(Role::inverse(vocab.intern_role(iri)))
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

fn intern_class_decl<A: ForIRI>(c: &Class<A>, vocab: &mut Vocabulary) -> ClassId {
    let iri: &str = c.0.as_ref();
    vocab.intern_class(iri)
}

fn convert_sub_role_path<A: ForIRI>(
    sub: &SubObjectPropertyExpression<A>,
    vocab: &mut Vocabulary,
) -> Result<SubRolePath, ConversionError> {
    match sub {
        SubObjectPropertyExpression::ObjectPropertyExpression(ope) => {
            Ok(SubRolePath::Role(convert_object_property(ope, vocab)?))
        }
        SubObjectPropertyExpression::ObjectPropertyChain(chain) => {
            let mut roles = Vec::with_capacity(chain.len());
            for link in chain {
                roles.push(convert_object_property(link, vocab)?);
            }
            Ok(SubRolePath::Chain(roles))
        }
    }
}

fn convert_individuals<A: ForIRI>(
    inds: &[Individual<A>],
    vocab: &mut Vocabulary,
) -> Result<Vec<IndividualId>, ConversionError> {
    let mut out = Vec::with_capacity(inds.len());
    for i in inds {
        out.push(convert_individual(i, vocab)?);
    }
    Ok(out)
}

fn convert_roles<A: ForIRI>(
    opes: &[ObjectPropertyExpression<A>],
    vocab: &mut Vocabulary,
) -> Result<Vec<Role>, ConversionError> {
    let mut out = Vec::with_capacity(opes.len());
    for o in opes {
        out.push(convert_object_property(o, vocab)?);
    }
    Ok(out)
}

/// Axiom-site helper: convert a `ClassExpression`, but if the
/// expression contains a data-range constructor
/// ([`ConversionError::UnsupportedDataRange`]), return `Ok(None)` for
/// the enclosing axiom (drops it silently — sound under-approximation,
/// see Phase D1 notes in the data-property arms). Other errors
/// propagate via `?`.
macro_rules! ce_or_skip {
    ($expr:expr) => {
        match $expr {
            Ok(c) => c,
            Err(ConversionError::UnsupportedDataRange) => return Ok(None),
            Err(e) => return Err(e),
        }
    };
}

/// Convert a single horned-owl [`Component`] to one of our axioms.
///
/// Returns:
/// - `Ok(Some(axiom))` when the component maps to an axiom in our IR.
/// - `Ok(None)` when the component is metadata or annotation-related and
///   has no representation in our IR (silently dropped — see the module
///   docs for the rationale). Also returned for axioms dropped under
///   Phase D1 data-axiom sound-under-approximation (see the data property
///   arms below + the `ce_or_skip!` macro above).
/// - `Err(_)` when the component is semantically meaningful but
///   unsupported in this phase (data ranges, datatypes, SWRL rules,
///   inverse-property expressions, anonymous individuals, etc.).
#[allow(clippy::too_many_lines)] // intrinsic to the breadth of horned-owl's Component enum
pub fn convert_component<A: ForIRI>(
    c: &Component<A>,
    vocab: &mut Vocabulary,
    pool: &mut ConceptPool,
) -> Result<Option<Axiom>, ConversionError> {
    use Component as C;
    match c {
        // ── Silently dropped: metadata + annotation axioms ──────────────
        // None of these carry reasoning-load-bearing content.
        #[allow(clippy::match_same_arms)]
        C::OntologyID(_)
        | C::DocIRI(_)
        | C::OntologyAnnotation(_)
        | C::Import(_)
        | C::DeclareAnnotationProperty(_)
        | C::AnnotationAssertion(_)
        | C::SubAnnotationPropertyOf(_)
        | C::AnnotationPropertyDomain(_)
        | C::AnnotationPropertyRange(_) => Ok(None),

        // ── Declarations ────────────────────────────────────────────────
        C::DeclareClass(d) => Ok(Some(Axiom::DeclareClass(intern_class_decl(&d.0, vocab)))),
        C::DeclareObjectProperty(d) => {
            let iri: &str = d.0.0.as_ref();
            Ok(Some(Axiom::DeclareObjectProperty(vocab.intern_role(iri))))
        }
        C::DeclareNamedIndividual(d) => {
            let iri: &str = d.0.0.as_ref();
            Ok(Some(Axiom::DeclareNamedIndividual(
                vocab.intern_individual(iri),
            )))
        }
        // ── Data properties + datatypes: sound under-approximation ──────
        // Phase D1 (2026-06-03): silently drop data-related declarations
        // and axioms. Class subsumption inferences that DEPEND on data
        // axioms (e.g., disjointness derivable from
        // DataMaxCardinality(1, dp) + DataMinCardinality(2, dp)) are
        // missed; no false positives are introduced. Class expressions
        // containing data-range constructors cause the enclosing axiom
        // to be dropped via the `ce_or_skip!` macro at axiom sites
        // (see `convert_class_expression`'s UnsupportedDataRange returns).
        // Phase D2 measurement decides whether real data-cardinality
        // reasoning (Tier B) is needed; Phase D3+ would add datatype
        // ranges (Tier C).
        C::DeclareDataProperty(_) | C::DeclareDatatype(_) => Ok(None),

        // ── TBox ────────────────────────────────────────────────────────
        C::SubClassOf(ax) => {
            let sub = ce_or_skip!(convert_class_expression(&ax.sub, vocab, pool));
            let sup = ce_or_skip!(convert_class_expression(&ax.sup, vocab, pool));
            Ok(Some(Axiom::SubClassOf { sub, sup }))
        }
        C::EquivalentClasses(ax) => {
            let mut ids = Vec::with_capacity(ax.0.len());
            for ce in &ax.0 {
                ids.push(ce_or_skip!(convert_class_expression(ce, vocab, pool)));
            }
            Ok(Some(Axiom::EquivalentClasses(ids)))
        }
        C::DisjointClasses(ax) => {
            let mut ids = Vec::with_capacity(ax.0.len());
            for ce in &ax.0 {
                ids.push(ce_or_skip!(convert_class_expression(ce, vocab, pool)));
            }
            Ok(Some(Axiom::DisjointClasses(ids)))
        }
        C::DisjointUnion(ax) => {
            let class = intern_class_decl(&ax.0, vocab);
            let mut members = Vec::with_capacity(ax.1.len());
            for ce in &ax.1 {
                members.push(ce_or_skip!(convert_class_expression(ce, vocab, pool)));
            }
            Ok(Some(Axiom::DisjointUnion { class, members }))
        }

        // ── RBox ────────────────────────────────────────────────────────
        C::SubObjectPropertyOf(ax) => {
            let sub = convert_sub_role_path(&ax.sub, vocab)?;
            let sup = convert_object_property(&ax.sup, vocab)?;
            Ok(Some(Axiom::SubObjectPropertyOf { sub, sup }))
        }
        C::EquivalentObjectProperties(ax) => {
            let roles = convert_roles(&ax.0, vocab)?;
            Ok(Some(Axiom::EquivalentObjectProperties(roles)))
        }
        C::DisjointObjectProperties(ax) => {
            let roles = convert_roles(&ax.0, vocab)?;
            Ok(Some(Axiom::DisjointObjectProperties(roles)))
        }
        C::InverseObjectProperties(ax) => {
            let a = Role::named(vocab.intern_role(ax.0.0.as_ref()));
            let b = Role::named(vocab.intern_role(ax.1.0.as_ref()));
            Ok(Some(Axiom::InverseObjectProperties(a, b)))
        }
        C::ObjectPropertyDomain(ax) => {
            let role = convert_object_property(&ax.ope, vocab)?;
            let domain = ce_or_skip!(convert_class_expression(&ax.ce, vocab, pool));
            Ok(Some(Axiom::ObjectPropertyDomain { role, domain }))
        }
        C::ObjectPropertyRange(ax) => {
            let role = convert_object_property(&ax.ope, vocab)?;
            let range = ce_or_skip!(convert_class_expression(&ax.ce, vocab, pool));
            Ok(Some(Axiom::ObjectPropertyRange { role, range }))
        }
        C::FunctionalObjectProperty(ax) => Ok(Some(Axiom::FunctionalRole(
            convert_object_property(&ax.0, vocab)?,
        ))),
        C::InverseFunctionalObjectProperty(ax) => Ok(Some(Axiom::InverseFunctionalRole(
            convert_object_property(&ax.0, vocab)?,
        ))),
        C::ReflexiveObjectProperty(ax) => Ok(Some(Axiom::ReflexiveRole(convert_object_property(
            &ax.0, vocab,
        )?))),
        C::IrreflexiveObjectProperty(ax) => Ok(Some(Axiom::IrreflexiveRole(
            convert_object_property(&ax.0, vocab)?,
        ))),
        C::SymmetricObjectProperty(ax) => Ok(Some(Axiom::SymmetricRole(convert_object_property(
            &ax.0, vocab,
        )?))),
        C::AsymmetricObjectProperty(ax) => Ok(Some(Axiom::AsymmetricRole(
            convert_object_property(&ax.0, vocab)?,
        ))),
        C::TransitiveObjectProperty(ax) => Ok(Some(Axiom::TransitiveRole(
            convert_object_property(&ax.0, vocab)?,
        ))),

        // ── ABox ────────────────────────────────────────────────────────
        C::ClassAssertion(ax) => {
            let class = ce_or_skip!(convert_class_expression(&ax.ce, vocab, pool));
            let individual = convert_individual(&ax.i, vocab)?;
            Ok(Some(Axiom::ClassAssertion { class, individual }))
        }
        C::ObjectPropertyAssertion(ax) => {
            let role = convert_object_property(&ax.ope, vocab)?;
            let subject = convert_individual(&ax.from, vocab)?;
            let object = convert_individual(&ax.to, vocab)?;
            Ok(Some(Axiom::ObjectPropertyAssertion {
                role,
                subject,
                object,
            }))
        }
        C::NegativeObjectPropertyAssertion(ax) => {
            let role = convert_object_property(&ax.ope, vocab)?;
            let subject = convert_individual(&ax.from, vocab)?;
            let object = convert_individual(&ax.to, vocab)?;
            Ok(Some(Axiom::NegativeObjectPropertyAssertion {
                role,
                subject,
                object,
            }))
        }
        C::SameIndividual(ax) => Ok(Some(Axiom::SameIndividual(convert_individuals(
            &ax.0, vocab,
        )?))),
        C::DifferentIndividuals(ax) => Ok(Some(Axiom::DifferentIndividuals(convert_individuals(
            &ax.0, vocab,
        )?))),

        // ── Data property / datatype: silently dropped per Phase D1 ─────
        // See the DeclareDataProperty / DeclareDatatype block above for
        // the sound-under-approximation rationale.
        #[allow(clippy::match_same_arms)]
        C::SubDataPropertyOf(_)
        | C::EquivalentDataProperties(_)
        | C::DisjointDataProperties(_)
        | C::DataPropertyDomain(_)
        | C::DataPropertyRange(_)
        | C::FunctionalDataProperty(_)
        | C::DatatypeDefinition(_)
        | C::DataPropertyAssertion(_)
        | C::NegativeDataPropertyAssertion(_) => Ok(None),

        // ── HasKey: advanced feature, deferred ──────────────────────────
        C::HasKey(_) => Err(ConversionError::UnsupportedAxiom { kind: "HasKey" }),

        // ── SWRL rules: silently skipped ────────────────────────────────
        // DL-safe `Rule` axioms are FOL-style entailment rules over
        // individuals; on real workloads (e.g. RO with 25 such rules)
        // they encode ABox-level inferences (`if x has property P
        // and y holds, then ...`). They don't enter class-side
        // classification — no class definition references their head
        // predicates — so silently dropping them is sound for the
        // `classify` use case. A future `swrl` feature gate could
        // materialise them via tableau extensions if needed.
        #[allow(clippy::match_same_arms)]
        C::Rule(_) => Ok(None),
    }
}

/// Convert an entire horned-owl [`SetOntology`] into an [`InternalOntology`].
///
/// Returns the first error encountered. horned-owl iterates a `HashSet`, so
/// the components arrive in HashMap-iteration order (different between
/// processes). Two stabilizations make every downstream pass — vocabulary
/// interning, absorption, saturation, the tableau search — deterministic
/// across runs:
///
/// 1. Sort components by their derived `Ord` *before* lowering, so the
///    sequence of `intern_class` / `intern_role` / `intern_individual`
///    calls is reproducible. This pins `ClassId` / `RoleId` /
///    `IndividualId` assignment (and therefore every `ConceptId` derived
///    from them) to a single canonical order across runs.
/// 2. Sort the lowered axiom list afterwards. Step 1 already guarantees
///    a deterministic sequence given a stable component order, but
///    sorting the output too keeps the contract explicit and survives
///    any future change to lowering that might shuffle ordering.
///
/// Same input → same axiom vector → reproducible reasoning behaviour and
/// timings.
pub fn convert_ontology<A: ForIRI>(
    src: &SetOntology<A>,
) -> Result<InternalOntology, ConversionError> {
    let mut components: Vec<&AnnotatedComponent<A>> = src.iter().collect();
    components.sort();
    let mut out = InternalOntology::new();
    for ac in components {
        if let Some(axiom) =
            convert_component(&ac.component, &mut out.vocabulary, &mut out.concepts)?
        {
            out.axioms.push(axiom);
        }
    }
    // Phase D4 (2026-06-03): scan for data-axiom patterns the main
    // conversion dropped (DeclareDataProperty, DataMin/Max, Functional,
    // DataPropertyDomain, SubDataPropertyOf, DataSome) and emit derived
    // class-subsumption / unsat axioms. The vocabulary is now fully
    // populated so class IRIs resolve. Sound under-approximation:
    // patterns we don't recognize stay dropped; recognized patterns
    // contribute additional axioms that close specific completeness
    // gaps without changing any other behavior. See
    // crates/owl-dl-core/src/data_axioms.rs for the pattern docs +
    // crates/owl-dl-reasoner/tests/datatype_completeness.rs for the
    // TDD harness.
    let bot_id = out.concepts.bot();
    // We intern atomic concept lookups inside the closure so the pool
    // gets all referenced atomic classes (some may not have been
    // referenced by any axiom that survived ce_or_skip!).
    // RefCell scoped tightly so its borrow on out.concepts ends before
    // out.axioms.extend (which doesn't need it but reads cleaner).
    let derived = {
        let concepts_cell = std::cell::RefCell::new(&mut out.concepts);
        crate::data_axioms::derive_data_axioms(src, &out.vocabulary, bot_id, |cid| {
            concepts_cell.borrow_mut().atomic(cid)
        })
    };
    out.axioms.extend(derived);
    // Derive `X ⊑ ∃R.C` from `X ⊑ ∃R.(D₁ ⊔ … ⊔ Dₙ)` when the disjuncts
    // share a told-subsumer C (sound under-approximation; feeds the EL
    // saturator a case-split it otherwise drops). Runs on the fully
    // populated IR.
    crate::disjunction_existential::derive_disjunction_existentials(&mut out);
    out.axioms.sort();
    Ok(out)
}

impl<A: ForIRI> TryFrom<&SetOntology<A>> for InternalOntology {
    type Error = ConversionError;

    fn try_from(src: &SetOntology<A>) -> Result<Self, Self::Error> {
        convert_ontology(src)
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
    fn inverse_object_property_lowers_to_inverse_role() {
        let mut v = Vocabulary::new();
        let ope =
            ObjectPropertyExpression::<RcStr>::InverseObjectProperty(b().object_property("r"));
        let role = convert_object_property(&ope, &mut v).unwrap();
        assert!(role.is_inverse());
        // The named id should match the forward use's id.
        let forward = b().object_property("r");
        let forward_ope = ObjectPropertyExpression::<RcStr>::ObjectProperty(forward);
        let forward_role = convert_object_property(&forward_ope, &mut v).unwrap();
        assert_eq!(role.role_id(), forward_role.role_id());
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

    // ──────────────────────────────────────────────────────────────────
    // Day 11: per-Component axiom conversion tests
    // ──────────────────────────────────────────────────────────────────

    use horned_owl::model as ho;
    use horned_owl::model::MutableOntology;

    fn ce_class(name: &str) -> ClassExpression<RcStr> {
        ClassExpression::Class(b().class(name))
    }

    fn ope(name: &str) -> ObjectPropertyExpression<RcStr> {
        ObjectPropertyExpression::ObjectProperty(b().object_property(name))
    }

    fn named_ind(name: &str) -> Individual<RcStr> {
        Individual::Named(b().named_individual(name))
    }

    fn convert_one(c: &Component<RcStr>) -> (InternalOntology, Option<Axiom>) {
        let mut o = InternalOntology::new();
        let ax = convert_component(c, &mut o.vocabulary, &mut o.concepts).unwrap();
        (o, ax)
    }

    #[test]
    fn sub_class_of_axiom() {
        let c = Component::SubClassOf(ho::SubClassOf {
            sub: ce_class("A"),
            sup: ce_class("B"),
        });
        let (_, ax) = convert_one(&c);
        assert!(matches!(ax, Some(Axiom::SubClassOf { .. })));
    }

    #[test]
    fn equivalent_classes_axiom_keeps_vec() {
        let c = Component::EquivalentClasses(ho::EquivalentClasses(vec![
            ce_class("A"),
            ce_class("B"),
            ce_class("C"),
        ]));
        let (_, ax) = convert_one(&c);
        let Some(Axiom::EquivalentClasses(v)) = ax else {
            panic!()
        };
        assert_eq!(v.len(), 3);
    }

    #[test]
    fn disjoint_classes_axiom() {
        let c = Component::DisjointClasses(ho::DisjointClasses(vec![ce_class("A"), ce_class("B")]));
        let (_, ax) = convert_one(&c);
        assert!(matches!(ax, Some(Axiom::DisjointClasses(_))));
    }

    #[test]
    fn disjoint_union_axiom() {
        let c = Component::DisjointUnion(ho::DisjointUnion(
            b().class("Parent"),
            vec![ce_class("Child1"), ce_class("Child2")],
        ));
        let (_, ax) = convert_one(&c);
        let Some(Axiom::DisjointUnion { members, .. }) = ax else {
            panic!()
        };
        assert_eq!(members.len(), 2);
    }

    #[test]
    fn sub_object_property_of_single() {
        let c = Component::SubObjectPropertyOf(ho::SubObjectPropertyOf {
            sub: SubObjectPropertyExpression::ObjectPropertyExpression(ope("r")),
            sup: ope("s"),
        });
        let (_, ax) = convert_one(&c);
        let Some(Axiom::SubObjectPropertyOf { sub, .. }) = ax else {
            panic!()
        };
        assert!(matches!(sub, SubRolePath::Role(_)));
    }

    #[test]
    fn sub_object_property_of_chain() {
        let c = Component::SubObjectPropertyOf(ho::SubObjectPropertyOf {
            sub: SubObjectPropertyExpression::ObjectPropertyChain(vec![ope("r"), ope("s")]),
            sup: ope("t"),
        });
        let (_, ax) = convert_one(&c);
        let Some(Axiom::SubObjectPropertyOf { sub, .. }) = ax else {
            panic!()
        };
        let SubRolePath::Chain(chain) = sub else {
            panic!()
        };
        assert_eq!(chain.len(), 2);
    }

    #[test]
    fn equivalent_object_properties() {
        let c = Component::EquivalentObjectProperties(ho::EquivalentObjectProperties(vec![
            ope("r"),
            ope("s"),
        ]));
        let (_, ax) = convert_one(&c);
        assert!(matches!(ax, Some(Axiom::EquivalentObjectProperties(_))));
    }

    #[test]
    fn inverse_object_properties_axiom() {
        let c = Component::InverseObjectProperties(ho::InverseObjectProperties(
            b().object_property("r"),
            b().object_property("s"),
        ));
        let (_, ax) = convert_one(&c);
        assert!(matches!(ax, Some(Axiom::InverseObjectProperties(_, _))));
    }

    #[test]
    fn object_property_domain_and_range() {
        let domain_c = Component::ObjectPropertyDomain(ho::ObjectPropertyDomain {
            ope: ope("r"),
            ce: ce_class("A"),
        });
        let range_c = Component::ObjectPropertyRange(ho::ObjectPropertyRange {
            ope: ope("r"),
            ce: ce_class("B"),
        });
        assert!(matches!(
            convert_one(&domain_c).1,
            Some(Axiom::ObjectPropertyDomain { .. })
        ));
        assert!(matches!(
            convert_one(&range_c).1,
            Some(Axiom::ObjectPropertyRange { .. })
        ));
    }

    #[test]
    fn role_characteristics() {
        type AxiomCheck = (Component<RcStr>, fn(&Axiom) -> bool);
        let cases: Vec<AxiomCheck> = vec![
            (
                Component::TransitiveObjectProperty(ho::TransitiveObjectProperty(ope("r"))),
                |a| matches!(a, Axiom::TransitiveRole(_)),
            ),
            (
                Component::FunctionalObjectProperty(ho::FunctionalObjectProperty(ope("r"))),
                |a| matches!(a, Axiom::FunctionalRole(_)),
            ),
            (
                Component::InverseFunctionalObjectProperty(ho::InverseFunctionalObjectProperty(
                    ope("r"),
                )),
                |a| matches!(a, Axiom::InverseFunctionalRole(_)),
            ),
            (
                Component::ReflexiveObjectProperty(ho::ReflexiveObjectProperty(ope("r"))),
                |a| matches!(a, Axiom::ReflexiveRole(_)),
            ),
            (
                Component::IrreflexiveObjectProperty(ho::IrreflexiveObjectProperty(ope("r"))),
                |a| matches!(a, Axiom::IrreflexiveRole(_)),
            ),
            (
                Component::SymmetricObjectProperty(ho::SymmetricObjectProperty(ope("r"))),
                |a| matches!(a, Axiom::SymmetricRole(_)),
            ),
            (
                Component::AsymmetricObjectProperty(ho::AsymmetricObjectProperty(ope("r"))),
                |a| matches!(a, Axiom::AsymmetricRole(_)),
            ),
        ];
        for (c, pred) in cases {
            let (_, ax) = convert_one(&c);
            let ax = ax.expect("expected an axiom");
            assert!(pred(&ax), "wrong axiom: {ax:?}");
        }
    }

    #[test]
    fn class_assertion() {
        let c = Component::ClassAssertion(ho::ClassAssertion {
            ce: ce_class("A"),
            i: named_ind("a"),
        });
        let (_, ax) = convert_one(&c);
        assert!(matches!(ax, Some(Axiom::ClassAssertion { .. })));
    }

    #[test]
    fn object_property_assertion_positive_and_negative() {
        let pos = Component::ObjectPropertyAssertion(ho::ObjectPropertyAssertion {
            ope: ope("r"),
            from: named_ind("a"),
            to: named_ind("b"),
        });
        let neg = Component::NegativeObjectPropertyAssertion(ho::NegativeObjectPropertyAssertion {
            ope: ope("r"),
            from: named_ind("a"),
            to: named_ind("b"),
        });
        assert!(matches!(
            convert_one(&pos).1,
            Some(Axiom::ObjectPropertyAssertion { .. })
        ));
        assert!(matches!(
            convert_one(&neg).1,
            Some(Axiom::NegativeObjectPropertyAssertion { .. })
        ));
    }

    #[test]
    fn same_and_different_individuals() {
        let same =
            Component::SameIndividual(ho::SameIndividual(vec![named_ind("a"), named_ind("b")]));
        let diff = Component::DifferentIndividuals(ho::DifferentIndividuals(vec![
            named_ind("a"),
            named_ind("c"),
        ]));
        assert!(matches!(
            convert_one(&same).1,
            Some(Axiom::SameIndividual(_))
        ));
        assert!(matches!(
            convert_one(&diff).1,
            Some(Axiom::DifferentIndividuals(_))
        ));
    }

    #[test]
    fn declarations() {
        assert!(matches!(
            convert_one(&Component::DeclareClass(ho::DeclareClass(b().class("A")))).1,
            Some(Axiom::DeclareClass(_))
        ));
        assert!(matches!(
            convert_one(&Component::DeclareObjectProperty(
                ho::DeclareObjectProperty(b().object_property("r"))
            ))
            .1,
            Some(Axiom::DeclareObjectProperty(_))
        ));
        assert!(matches!(
            convert_one(&Component::DeclareNamedIndividual(
                ho::DeclareNamedIndividual(b().named_individual("a"))
            ))
            .1,
            Some(Axiom::DeclareNamedIndividual(_))
        ));
    }

    #[test]
    fn metadata_and_annotations_silently_skipped() {
        // OntologyID with no IRIs is the default.
        let id = ho::OntologyID::default();
        let (_, ax) = convert_one(&Component::<RcStr>::OntologyID(id));
        assert!(ax.is_none());
        // AnnotationProperty declaration is dropped (not reasoning-load-bearing).
        let ap = Component::<RcStr>::DeclareAnnotationProperty(ho::DeclareAnnotationProperty(
            b().annotation_property("p"),
        ));
        assert!(convert_one(&ap).1.is_none());
    }

    /// Phase D1 (2026-06-03): data-axiom declarations no longer hard-
    /// error — they're silently dropped as sound under-approximation
    /// so the 4 erroring fixtures (family, ro, sio, shoiq-knowledge)
    /// parse + classify. Phase D2 measures FP/MISSED vs Konclude to
    /// decide if real cardinality reasoning (Tier B) is needed.
    #[test]
    fn data_axiom_declarations_silently_dropped() {
        let c = Component::<RcStr>::DeclareDataProperty(ho::DeclareDataProperty(
            b().data_property("dp"),
        ));
        let mut o = InternalOntology::new();
        let result = convert_component(&c, &mut o.vocabulary, &mut o.concepts).unwrap();
        assert!(
            result.is_none(),
            "Phase D1: data-property declarations drop silently (Ok(None))"
        );
    }

    /// Phase D1: a `SubClassOf` where the SUP contains a data-range
    /// constructor (e.g., `DataMaxCardinality`) is silently dropped —
    /// the `ce_or_skip!` macro maps `UnsupportedDataRange` to `Ok(None)`
    /// for the enclosing axiom. Sound under-approximation: we lose the
    /// constraint, never invent a wrong one.
    #[test]
    fn subclass_with_data_range_silently_dropped() {
        use horned_owl::model::DataProperty;
        let dp = DataProperty::<RcStr>(b().iri("http://t/dp"));
        let c = Component::<RcStr>::SubClassOf(ho::SubClassOf {
            sub: ce_class("A"),
            sup: ClassExpression::DataMaxCardinality {
                n: 1,
                dp,
                dr: horned_owl::model::DataRange::Datatype(horned_owl::model::Datatype(
                    b().iri("http://www.w3.org/2001/XMLSchema#integer"),
                )),
            },
        });
        let mut o = InternalOntology::new();
        let result = convert_component(&c, &mut o.vocabulary, &mut o.concepts).unwrap();
        assert!(
            result.is_none(),
            "Phase D1: SubClassOf containing a data-range SUP drops silently"
        );
    }

    #[test]
    fn convert_ontology_smoke() {
        let mut o = SetOntology::<RcStr>::new();
        o.insert(ho::AnnotatedComponent::from(Component::SubClassOf(
            ho::SubClassOf {
                sub: ce_class("A"),
                sup: ce_class("B"),
            },
        )));
        o.insert(ho::AnnotatedComponent::from(Component::DeclareClass(
            ho::DeclareClass(b().class("A")),
        )));
        let internal = convert_ontology(&o).unwrap();
        assert_eq!(internal.num_axioms(), 2);
        assert_eq!(internal.vocabulary.num_classes(), 2); // A, B
    }

    #[test]
    fn try_from_set_ontology() {
        let mut o = SetOntology::<RcStr>::new();
        o.insert(ho::AnnotatedComponent::from(Component::SubClassOf(
            ho::SubClassOf {
                sub: ce_class("A"),
                sup: ce_class("B"),
            },
        )));
        let internal = InternalOntology::try_from(&o).unwrap();
        assert_eq!(internal.num_axioms(), 1);
    }
}
