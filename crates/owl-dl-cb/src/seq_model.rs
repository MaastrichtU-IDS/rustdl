//! Sequoia ordered-calculus context/clause data model (S1, ALCH only).
//!
//! Fresh module set for the ordered consequence-based engine
//! (`docs/superpowers/specs/2026-06-16-cb-sequoia-rearchitecture-design.md`,
//! §2). Deliberately separate from the unordered B1/B2 `model.rs`/`engine.rs`,
//! which are retained as the differential oracle.
//!
//! S1 scope (ALCH): atomic concept literals + `∃R.B` + `∀R.B` head literals,
//! role hierarchy, disjunction. NO equality / terms / cardinality / inverse /
//! nominal (those are S2+). A context's clause set holds **ordered DL-clauses**
//! `Γ → Δ` where the body `Γ` is empty (the core is held implicitly via Core
//! seeding) and the head `Δ` is a disjunction of [`SeqLit`] literals kept sorted
//! by the per-context order `≻ᵥ` (so the maximal literal is `head.last()`).

use crate::seq_order::PerContextOrder;
use owl_dl_core::ir::ConceptId;
use std::collections::BTreeSet;

/// A clause body atom: an atomic concept (interned `ConceptId`, `pool.get(_)`
/// is `Atomic` or `Top`).
pub(crate) type Atom = ConceptId;

/// A head literal in the ordered calculus (S1 / ALCH).
///
/// Every literal is an interned `ConceptId`. We do not introduce a distinct
/// equality literal kind in S1 (no Eq/Ineq/Fact). The literal is one of:
/// atomic `B`, `∃R.B`, `∀R.B`.
pub(crate) type SeqLit = ConceptId;

/// An ordered context clause `Γ → Δ`.
///
/// In S1 the body `Γ` is always empty for derived clauses — the context core is
/// seeded as units by Core and never re-introduced into a body. The head is a
/// disjunction of literals, **sorted by `≻ᵥ`** (maximal last) and deduped.
/// An empty head means `⊥` (`Γ → ⊥`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct SeqClause {
    /// Head disjunction, sorted by the owning context's order (maximal last),
    /// deduped. Empty = `⊥`.
    pub(crate) head: Vec<SeqLit>,
}

pub(crate) type ContextId = usize;

/// A successor edge `parent —R→ child` carrying the spawning clause residual
/// `N` (the disjunction minus the `∃R.B` literal that spawned the edge).
#[derive(Clone, Debug)]
pub(crate) struct SeqEdge {
    /// The child (successor) context, cored at the filler.
    pub(crate) child: ContextId,
    /// The role on the edge.
    pub(crate) role: owl_dl_core::ir::Role,
    /// Residual disjunction `N`: the rest of the spawning clause's head. The
    /// successor's `⊥` (or other Pred-eligible conclusion) is reflected to the
    /// parent under this residual. Sorted+deduped (by the PARENT's order).
    pub(crate) residual: Vec<SeqLit>,
}

/// A Sequoia context: reasoning about an element whose `core` conjunction holds.
#[derive(Clone, Debug, Default)]
pub(crate) struct SeqContext {
    /// The conjunction of atoms defining this context.
    pub(crate) core: BTreeSet<Atom>,
    /// Derived ordered clauses (membership-guarded by `seen`).
    pub(crate) clauses: Vec<SeqClause>,
    /// Membership guard for `clauses` (dedup / Elim subsumption gate).
    pub(crate) seen: BTreeSet<SeqClause>,
    /// Per-context order `≻ᵥ` (subsumer-respecting; query atom minimal at root).
    pub(crate) order: PerContextOrder,
    /// Outgoing successor edges `v —R→ child` (Succ / R∀).
    pub(crate) succ: Vec<SeqEdge>,
}

/// The saturated context graph. Contexts are reused by `core` (termination key).
#[derive(Default)]
pub(crate) struct SeqGraph {
    pub(crate) contexts: Vec<SeqContext>,
    pub(crate) by_core: hashbrown::HashMap<BTreeSet<Atom>, ContextId>,
    /// R1 (`RUSTDL_CB_ORDER=per_query`) only: root query contexts keyed by
    /// `(core, head)` — NOT by `by_core` alone, which would wrongly merge
    /// `(A,B1)` and `(A,B2)` (identical core, different head-minimal orders).
    pub(crate) by_query: hashbrown::HashMap<(BTreeSet<Atom>, ConceptId), ContextId>,
}
