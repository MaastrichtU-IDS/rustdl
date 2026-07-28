//! Consequence-based context/clause data model.
//!
//! B1 (ALCH): `docs/superpowers/specs/2026-06-15-cb-engine-b1-alch-design.md`.
//! B2 Tier-2 (ALCHQ, equality): `docs/.../2026-06-16-cb-b2-tier2-equality-design.md`
//! (the freeze-break: `TermId`/`Term`, `HeadLit`, per-context `terms`/`at_most`,
//! union-find `merged_into`, and the `Succ`/`Merge` edge representation, §1.2/§1.4/§9.7).
//!
//! Do not change these types without re-syncing the parallel implementer tasks.

use owl_dl_core::ir::{ConceptId, Role};
use std::collections::BTreeSet;

/// A normalized clause body atom: an atomic concept that must hold.
///
/// Invariant: `pool.get(_)` is `Atomic` (or `Top`, used as the empty premise).
pub type Atom = ConceptId;

/// A normalized clause head literal: atomic `B`, `∃R.B`, or `∀R.B` (B atomic).
/// Represented as the interned `ConceptId` of that literal.
///
/// This is the **ontology-clause** literal kind — ontology clauses never carry
/// equalities, so `OntClause.head` stays `Vec<Literal>`. Derived clauses, which
/// can carry `Eq`/`Neq`, use [`HeadLit`].
pub type Literal = ConceptId;

/// A successor witness (term): identity is decoupled from the context that types
/// it, so `≥2 R.A` can mint **two distinct** terms both pointing at core `{A}`
/// (§1.1). Owned by a parent context.
pub type TermId = usize;

/// A derived-clause head literal in the B2 model. B1's `Literal = ConceptId`
/// (atomic / ∃R.B / ∀R.B) becomes the [`HeadLit::Concept`] arm. Equalities are a
/// DISTINCT variant so the term ordering can never leak into the concept read-off
/// (the Slice-0 hazard — §3.2). `apply_hyper` filters to `Concept(_)` and never
/// indexes `Eq`/`Neq`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HeadLit {
    /// B1 literal: atomic `B`, `∃R.B`, or `∀R.B` (interned `ConceptId`).
    Concept(ConceptId),
    /// `s ≈ t` — the two successor terms denote the same element.
    Eq(TermId, TermId),
    /// `s ≉ t` — the two successor terms are distinct.
    Neq(TermId, TermId),
}

impl HeadLit {
    /// The wrapped concept id, if this is a `Concept` literal.
    #[must_use]
    pub fn as_concept(self) -> Option<ConceptId> {
        match self {
            HeadLit::Concept(c) => Some(c),
            _ => None,
        }
    }
}

/// A successor witness owned by a *parent* context. It points at a context
/// (shareable by core, for type-reasoning) but has its own identity.
#[derive(Clone, Debug)]
pub struct Term {
    /// The context whose core types this witness (find-or-create by core).
    pub ctx: ContextId,
    /// The role on the edge `parent —R→ this`.
    pub role: Role,
    /// The clause residual `N` of the spawning `Succ` edge (B1 edge-residual
    /// semantics) — the term's *signature* together with `(ctx, role)`. Sorted,
    /// deduped. Carried so witness-minting is idempotent (termination key).
    pub residual: Vec<HeadLit>,
    /// Set to `Some(other)` when this term has been merged INTO `other`
    /// (union-find parent). Merge never mutates a shared ctx — it repoints.
    pub merged_into: Option<TermId>,
}

/// A normalized ontology clause `⊓ premise ⊑ ⊔ head`.
///
/// An empty `head` is `⊑ ⊥`; an empty `premise` is `⊤ ⊑ …`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OntClause {
    pub premise: Vec<Atom>,
    pub head: Vec<Literal>,
}

/// A clause derived *within a context* — `premise → ⊔ head`, where the premise
/// atoms hold in the context's core/derived set. Empty `head` = `⊥`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DerivedClause {
    /// Sorted, deduped.
    pub premise: Vec<Atom>,
    /// Sorted, deduped (empty = `⊥`). May carry `Eq`/`Neq` (B2).
    pub head: Vec<HeadLit>,
}

pub type ContextId = usize;

/// The kind of an edge into a context (§9.7): a `Succ` term-edge carries the role
/// `R` of `parent —R→ this`; a `Merge` edge comes from a speculative term merge
/// (it has no single role). Both carry a residual disjunction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeKind {
    /// `parent —R→ this` (an `∃`/`≥n`/`∀`-augmented successor edge).
    Succ(Role),
    /// A speculative merge of two of `parent`'s terms (§9.2). ⊥-back-prop only.
    Merge,
}

/// A context: reasoning about an element whose `core` conjunction holds.
#[derive(Clone, Debug, Default)]
pub struct Context {
    /// The conjunction of atoms defining this context.
    pub core: BTreeSet<Atom>,
    /// Derived sequents (membership-guarded by `seen`).
    pub clauses: Vec<DerivedClause>,
    /// Membership guard for `clauses` (dedup).
    pub seen: BTreeSet<DerivedClause>,
    /// This context's successor witnesses (B2 — replaces B1's `succ`).
    pub terms: Vec<Term>,
    /// Active `≤n R.C` constraints (Tier-2): `(n, R, C)`.
    pub at_most: Vec<(u32, Role, Atom)>,
}

/// The saturated context graph. Contexts are reused by `core` (termination key).
#[derive(Default)]
pub struct ContextGraph {
    pub contexts: Vec<Context>,
    pub by_core: hashbrown::HashMap<BTreeSet<Atom>, ContextId>,
}
