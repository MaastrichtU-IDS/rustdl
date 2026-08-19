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
    /// Bit `i` set iff `axioms[i]` is active. NEVER shrink `axioms` —
    /// `ProofTrace`'s provenance vectors and `justify`/`repair` key on
    /// these indices. Removal clears a bit; the slot stays addressable.
    pub live: fixedbitset::FixedBitSet,
    /// Bit `i` set iff `axioms[i]` was produced by one of the whole-ontology
    /// derivation passes rather than by lowering a source component.
    ///
    /// SOUNDNESS: the derivation passes are fixpoints over the ENTIRE axiom
    /// set, so a derived axiom retained across a retraction is a false
    /// positive (e.g. a `C ⊑ ⊥` that outlives the `Functional(dp)` that
    /// produced it). [`crate::delta::refresh_derived`] recomputes the whole
    /// derived overlay at every commit and retracts what no longer follows;
    /// this bitset is what tells it which axioms it owns. Never set for an
    /// axiom that came from the user's ontology — those are retracted only
    /// by an explicit delta.
    pub derived: fixedbitset::FixedBitSet,
    /// The lowered user axioms **as they were BEFORE** the derivation passes
    /// ran — the exact input [`crate::delta::refresh_derived`] must re-run
    /// them over.
    ///
    /// This cannot be reconstructed from `axioms`. Two passes CONSUME their
    /// input: `split_disjunctive_antecedents` replaces `(A ⊔ B) ⊑ C` with
    /// `A ⊑ C`, `B ⊑ C`, and `decompose_long_chains` replaces an n-leg chain
    /// with a 2-leg cascade. The original is then absent from `axioms`
    /// altogether, so `live ∧ ¬derived` is strictly smaller than the real
    /// baseline and re-running the passes over it would fail to reproduce the
    /// replacements — which are marked `derived`, and so would be retracted.
    /// That deletes real axiom content on the first commit of any session.
    ///
    /// Kept in sync by `push_live_axiom`'s callers: `convert_ontology` seeds
    /// it, `delta::convert_delta` appends to it, and [`Self::kill_axiom`]
    /// removes from it. Its ORDER is stable across commits, which also keeps
    /// `decompose_long_chains`' index-derived auxiliary role IRIs stable.
    pub user_axioms: Vec<Axiom>,
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

    /// Bring `live` up to `axioms.len()`, marking any un-tracked tail live.
    /// Call after code paths that push straight into `axioms`.
    pub fn sync_liveness(&mut self) {
        let n = self.axioms.len();
        let old_len = self.live.len();
        if old_len < n {
            self.live.grow(n);
            for i in old_len..n {
                self.live.insert(i);
            }
        }
    }

    pub fn push_live_axiom(&mut self, ax: Axiom) -> usize {
        let idx = self.axioms.len();
        self.axioms.push(ax);
        self.live.grow(idx + 1);
        self.live.insert(idx);
        idx
    }

    /// Returns true iff this call transitioned the axiom live -> dead.
    ///
    /// Killing a USER axiom also drops one occurrence of it from
    /// [`Self::user_axioms`]. That is not bookkeeping — it is the soundness
    /// step: `refresh_derived` re-runs the derivation passes over that
    /// baseline, so an entry left behind after its axiom was retracted would
    /// re-derive the very consequences the retraction was meant to remove
    /// (delete `Functional(dp)`, get `C ⊑ ⊥` back). One occurrence, not all:
    /// the baseline is a multiset and the caller killed one slot.
    pub fn kill_axiom(&mut self, idx: usize) -> bool {
        if idx < self.live.len() && self.live.contains(idx) {
            self.live.set(idx, false);
            if !self.derived.contains(idx) {
                let ax = &self.axioms[idx];
                if let Some(pos) = self.user_axioms.iter().position(|u| u == ax) {
                    self.user_axioms.remove(pos);
                }
            }
            true
        } else {
            false
        }
    }

    pub fn live_axiom_indices(&self) -> impl Iterator<Item = usize> + '_ {
        self.live.ones()
    }

    pub fn live_axioms(&self) -> impl Iterator<Item = (usize, &Axiom)> + '_ {
        self.live.ones().map(move |i| (i, &self.axioms[i]))
    }

    #[must_use]
    pub fn num_live_axioms(&self) -> usize {
        self.live.count_ones(..)
    }

    /// Append a USER axiom: live, not derived, and recorded in the
    /// [`Self::user_axioms`] baseline the derivation passes re-run over.
    pub fn push_user_axiom(&mut self, ax: Axiom) -> usize {
        self.user_axioms.push(ax.clone());
        self.push_live_axiom(ax)
    }

    /// Append an axiom owned by the derivation overlay (live + `derived`).
    pub fn push_derived_axiom(&mut self, ax: Axiom) -> usize {
        let idx = self.push_live_axiom(ax);
        self.derived.grow(idx + 1);
        self.derived.insert(idx);
        idx
    }

    /// True iff `axioms[idx]` is owned by the derivation overlay.
    #[must_use]
    pub fn is_derived(&self, idx: usize) -> bool {
        self.derived.contains(idx)
    }
}
