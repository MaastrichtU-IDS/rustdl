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
use crate::DroppedAxioms;
use crate::Vocabulary;
use crate::ir::{ClassId, ConceptId, IndividualId, Role, RoleId};

/// A sub-role *expression* on the LHS of a `SubObjectPropertyOf` axiom.
///
/// The chain variant supports SROIQ's `R₁ ∘ ... ∘ Rₙ ⊑ S` axioms. The
/// converter accepts chains now so they survive into the IR; the reasoner
/// will error on them until Phase 5 lands the automaton machinery.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
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
#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
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
    /// Axioms dropped during conversion (issue #43), tallied by diagnostic
    /// kind. Empty for a fully-supported ontology — the refactor that
    /// introduced this field is inert (produces the identical `axioms` set)
    /// when nothing is dropped.
    pub dropped: DroppedAxioms,
    /// Axioms that conversion added because the OWL 2 Direct Semantics makes them
    /// hold in every interpretation, not because the source states them (#539,
    /// `top_bottom_roles`): `∃owl:bottomObjectProperty.⊤ ⊑ ⊥`, the same for
    /// `owl:bottomDataProperty`, and `⊤ ⊑ ⊥` when a universal role is told empty or
    /// negatively asserted. Each one is also in `axioms`. A surface that reports or
    /// cites the SOURCE's axioms must not present these as the user's: `prove` leaves
    /// them out of its axiom count and labels them, `prove --json` lists them under
    /// `builtin_axioms`, and `verify-el` labels a violation on one. (`verify-el`'s
    /// `axioms_checked` is a count of IR axioms checked against the model, built-ins
    /// included, like every other derived axiom conversion adds.) `convert_back` writes
    /// them as ordinary axioms; re-converting its output would fold them into the
    /// source's (it has no non-test caller today). Empty unless the source mentions
    /// `owl:bottomObjectProperty` / `owl:bottomDataProperty`, or tells a universal role
    /// empty, or negates an assertion on one.
    pub builtin: Vec<Axiom>,
    /// #543 (S+F): for a built-in that holds only GIVEN a source axiom (a fold of a
    /// universal-role use licensed by a `ClassAssertion`, an `ObjectPropertyDomain` /
    /// `Range`, an `IrreflexiveRole` / `AsymmetricRole` or a
    /// `DisjointObjectProperties`), the pair (built-in, licensing source axioms): the
    /// axiom itself, or a `ClassAssertion` with the told `SubClassOf` /
    /// `EquivalentClasses` path from its class. A surface citing the built-in cites those
    /// axioms with it, so a step is never credited to "OWL semantics" alone when user
    /// axioms are responsible. One licence per built-in: the first one found, though
    /// other source axioms may license it too. A built-in with no entry holds in every
    /// interpretation.
    pub builtin_licenses: Vec<(Axiom, Vec<Axiom>)>,
    /// #543 (S+F): how many axioms the universal-role fold added (`unlowered`'s
    /// `universal_role_folds`; the zero-fold check of the no-sweep argument reads it).
    pub universal_role_folds: usize,
    /// #589: data keys (`DKey` classes) that may need reasoning by cases. A key that
    /// is not a single value and that meets another key of the same value space
    /// without a seeded `⊑` edge into it: the engines treat keys as atoms, so the
    /// value-level case split this pair can need (`∃d.{1,2}` against `hasValue(d,1)`
    /// and `hasValue(d,2)`) is never made. Coarse in the safe direction: role- and
    /// polarity-blind, and "meets" is assumed wherever it is not decided. Set by
    /// `seed_dkey_subsumptions`; 0 when it did not run (a conversion cut, which voids
    /// completeness on its own).
    pub dkey_split_risk_keys: usize,
    /// #538: n-ary axioms whose repeated operands were collapsed while reading (their
    /// operands are a set). Kept apart from `dropped`: no content is lost under the set
    /// reading, so completeness is unaffected; a caller reading the operands as a list
    /// can see which answers that reading would add.
    pub collapsed_operands: crate::CollapsedOperands,
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
