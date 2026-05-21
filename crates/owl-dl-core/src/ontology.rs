//! [`InternalOntology`] — the workspace's in-memory representation of an OWL
//! ontology after conversion from `horned-owl`.
//!
//! Design choice (advisor-reviewed Phase 0): the container is **faithful**, not
//! pre-normalized. `EquivalentClasses`, `DisjointUnion`, and other multi-way
//! axioms are kept as first-class variants here; decomposition to atomic
//! `SubClassOf` form is a Phase 1 normalization pass, not a parse-time
//! concern. This preserves source-axiom semantics and lets the normalizer
//! own the choice of how to break them apart.

use crate::ConceptPool;
use crate::Vocabulary;
use crate::ir::{ClassId, ConceptId, IndividualId, Role, RoleId};

/// A sub-role *expression* on the LHS of a `SubObjectPropertyOf` axiom.
///
/// The chain variant supports SROIQ's `R₁ ∘ ... ∘ Rₙ ⊑ S` axioms. The
/// converter accepts chains now so they survive into the IR; the reasoner
/// will error on them until Phase 5 lands the automaton machinery.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum SubRolePath {
    Role(Role),
    Chain(Vec<Role>),
}

/// An OWL axiom in our internal representation.
///
/// Variants are kept faithful to the source: multi-way axioms like
/// `EquivalentClasses` are not exploded into pairwise `SubClassOf` here —
/// that's normalization (Phase 1). Concept-level isomorphic encodings
/// (`ObjectHasValue` → `Some-of-Nominal`, `ObjectExactCardinality` →
/// `Min ⊓ Max`) happen during conversion because our IR has no direct
/// counterpart for those source constructors.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum Axiom {
    // --- TBox ---
    SubClassOf {
        sub: ConceptId,
        sup: ConceptId,
    },
    EquivalentClasses(Vec<ConceptId>),
    DisjointClasses(Vec<ConceptId>),
    DisjointUnion {
        class: ClassId,
        members: Vec<ConceptId>,
    },

    // --- RBox ---
    SubObjectPropertyOf {
        sub: SubRolePath,
        sup: Role,
    },
    EquivalentObjectProperties(Vec<Role>),
    DisjointObjectProperties(Vec<Role>),
    InverseObjectProperties(Role, Role),
    ObjectPropertyDomain {
        role: Role,
        domain: ConceptId,
    },
    ObjectPropertyRange {
        role: Role,
        range: ConceptId,
    },
    TransitiveRole(Role),
    SymmetricRole(Role),
    AsymmetricRole(Role),
    ReflexiveRole(Role),
    IrreflexiveRole(Role),
    FunctionalRole(Role),
    InverseFunctionalRole(Role),

    // --- ABox ---
    ClassAssertion {
        class: ConceptId,
        individual: IndividualId,
    },
    ObjectPropertyAssertion {
        role: Role,
        subject: IndividualId,
        object: IndividualId,
    },
    NegativeObjectPropertyAssertion {
        role: Role,
        subject: IndividualId,
        object: IndividualId,
    },
    SameIndividual(Vec<IndividualId>),
    DifferentIndividuals(Vec<IndividualId>),

    // --- Declarations ---
    DeclareClass(ClassId),
    DeclareObjectProperty(RoleId),
    DeclareNamedIndividual(IndividualId),
}

/// The in-memory ontology produced by conversion.
///
/// Holds the IRI vocabulary, the concept pool (so all `ConceptId`s in
/// `axioms` are valid in `concepts`), and the axiom list in source order.
/// The role hierarchy and other derived structures are computed in Phase 1.
#[derive(Default, Clone, Debug)]
pub struct InternalOntology {
    pub vocabulary: Vocabulary,
    pub concepts: ConceptPool,
    pub axioms: Vec<Axiom>,
}

impl InternalOntology {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn num_axioms(&self) -> usize {
        self.axioms.len()
    }
}
