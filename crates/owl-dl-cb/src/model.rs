//! Consequence-based context/clause data model (FROZEN interface — B1).
//!
//! See `docs/superpowers/specs/2026-06-15-cb-engine-b1-alch-design.md`.
//! Do not change these types without re-syncing the parallel implementer tasks.

use owl_dl_core::ir::{ConceptId, Role};
use std::collections::BTreeSet;

/// A normalized clause body atom: an atomic concept that must hold.
///
/// Invariant: `pool.get(_)` is `Atomic` (or `Top`, used as the empty premise).
pub type Atom = ConceptId;

/// A normalized clause head literal: atomic `B`, `∃R.B`, or `∀R.B` (B atomic).
/// Represented as the interned `ConceptId` of that literal.
pub type Literal = ConceptId;

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
    /// Sorted, deduped (empty = `⊥`).
    pub head: Vec<Literal>,
}

pub type ContextId = usize;

/// A context: reasoning about an element whose `core` conjunction holds.
#[derive(Clone, Debug, Default)]
pub struct Context {
    /// The conjunction of atoms defining this context.
    pub core: BTreeSet<Atom>,
    /// Derived sequents (membership-guarded by `seen`).
    pub clauses: Vec<DerivedClause>,
    /// Membership guard for `clauses` (dedup).
    pub seen: BTreeSet<DerivedClause>,
    /// `∃`-generated successor edges.
    pub succ: Vec<(Role, ContextId)>,
}

/// The saturated context graph. Contexts are reused by `core` (termination key).
#[derive(Default)]
pub struct ContextGraph {
    pub contexts: Vec<Context>,
    pub by_core: hashbrown::HashMap<BTreeSet<Atom>, ContextId>,
}
