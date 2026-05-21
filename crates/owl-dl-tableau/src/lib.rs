//! Tableau engine for SROIQ.
//!
//! Phase 2 starts with ALC and grows to ALCHIQ (Phase 3), receives its
//! optimization stack in Phase 4, and adds nominals + complex role
//! hierarchies in Phase 5.
//!
//! ## Phase 2 commit 1: infrastructure only
//!
//! This commit lands the storage layer:
//!
//! - [`CompletionGraph`] with [`NodeId`]-indexed nodes carrying sorted
//!   label lists and edge lists.
//! - [`TableauTrail`] with log-and-undo backtracking via
//!   [`Checkpoint`] markers.
//! - [`TableauContext`] — the only sanctioned mutation interface;
//!   every label addition, edge addition, or node creation goes
//!   through it and is recorded on the trail.
//! - Clash detection: [`TableauContext::clash_in`] checks `Bot` in
//!   the label set or a complementary `c` / `Not(c)` pair.
//! - Stub [`TableauContext::is_satisfiable`] that handles only the
//!   trivial top-level shapes (`Top`, `Bot`, `Atomic`, `Nominal`,
//!   `Not(Atomic)`). Real ⊓ / ⊔ / ∃ / ∀ rules arrive in later
//!   Phase 2 commits.
//!
//! The crate compiles and tests pass, but it cannot yet decide
//! satisfiability of any non-trivial concept.

mod graph;
mod trail;

pub use graph::{CompletionGraph, Node, NodeId};
pub use trail::{Checkpoint, TableauTrail, TrailEntry};

use owl_dl_core::{ConceptExpr, ConceptId, ConceptPool, RoleId, is_nnf};

/// Coordinator owning the completion graph and trail for one tableau
/// run.
///
/// The context borrows the [`ConceptPool`] immutably; the pool was
/// fully populated by Phase 1 normalization and absorption and no
/// further interning happens during tableau search.
///
/// All graph mutation goes through this type so the trail stays in
/// sync.
#[derive(Debug)]
pub struct TableauContext<'pool> {
    pool: &'pool ConceptPool,
    graph: CompletionGraph,
    trail: TableauTrail,
}

impl<'pool> TableauContext<'pool> {
    #[must_use]
    pub fn new(pool: &'pool ConceptPool) -> Self {
        Self {
            pool,
            graph: CompletionGraph::new(),
            trail: TableauTrail::new(),
        }
    }

    #[must_use]
    pub fn pool(&self) -> &ConceptPool {
        self.pool
    }

    #[must_use]
    pub fn graph(&self) -> &CompletionGraph {
        &self.graph
    }

    #[must_use]
    pub fn trail(&self) -> &TableauTrail {
        &self.trail
    }

    /// Take a checkpoint that can later be passed to [`Self::rollback_to`]
    /// to undo every mutation made after this call.
    pub fn checkpoint(&mut self) -> Checkpoint {
        self.trail.checkpoint()
    }

    /// Restore the graph to the state it had when `cp` was created.
    pub fn rollback_to(&mut self, cp: Checkpoint) {
        self.trail.rollback_to(cp, &mut self.graph);
    }

    /// Allocate a fresh node and return its id. Records a
    /// [`TrailEntry::NodeCreated`] so rollback drops the node.
    pub fn new_node(&mut self) -> NodeId {
        let prior_len = self.graph.len();
        let id = self.graph.push_node();
        self.trail.record(TrailEntry::NodeCreated { prior_len });
        id
    }

    /// Add concept `c` to `node`'s label list if not already present.
    ///
    /// Returns `true` if the label was newly inserted, `false` if it
    /// was already there. Records a [`TrailEntry::LabelAdded`] on
    /// insertion.
    ///
    /// `c` must be in NNF; debug-asserted at the boundary so any rule
    /// that forgets to normalize is caught in tests but pays no cost
    /// in release.
    pub fn add_label(&mut self, node: NodeId, c: ConceptId) -> bool {
        debug_assert!(
            is_nnf(c, self.pool),
            "TableauContext::add_label received non-NNF concept"
        );
        let labels = &mut self.graph.node_mut(node).labels;
        match labels.binary_search(&c) {
            Ok(_) => false,
            Err(pos) => {
                labels.insert(pos, c);
                self.trail
                    .record(TrailEntry::LabelAdded { node, concept: c });
                true
            }
        }
    }

    /// Append `(role, target)` to `from`'s edge list and record the
    /// addition on the trail.
    ///
    /// Edges are not deduplicated here — distinct role assertions
    /// between the same nodes can be meaningful for cardinality
    /// reasoning later. Higher-level rules can check before adding.
    pub fn add_edge(&mut self, from: NodeId, role: RoleId, target: NodeId) {
        self.graph.node_mut(from).edges.push((role, target));
        self.trail
            .record(TrailEntry::EdgeAdded { from, role, target });
    }

    /// Return true if `node` contains a clash:
    /// 1. `Bot` is in its label set, or
    /// 2. some concept `c` and its negation `Not(c)` are both in
    ///    its label set.
    ///
    /// This is the local clash check; later commits may add global
    /// clashes (e.g., individual identity for nominals).
    #[must_use]
    pub fn clash_in(&self, node: NodeId) -> bool {
        let labels = self.graph.node(node).labels();
        for &c in labels {
            match self.pool.get(c) {
                ConceptExpr::Bot => return true,
                ConceptExpr::Not(inner) if labels.binary_search(inner).is_ok() => return true,
                _ => {}
            }
        }
        false
    }

    /// Stub satisfiability check for trivial top-level shapes only.
    ///
    /// Returns:
    /// - `Some(true)` for `Top`, `Atomic`, `Nominal`, `Not(Atomic)`,
    ///   `Not(Nominal)`, `SelfRestriction` (any single-node-consistent
    ///   shape with no further obligations);
    /// - `Some(false)` for `Bot`;
    /// - `None` for everything else — the real ⊓/⊔/∃/∀/⊑ rules will
    ///   answer those in subsequent Phase 2 commits.
    ///
    /// Implemented as a free-standing decision over the pool, but
    /// kept as a method so later commits can replace it with a full
    /// completion-graph driver without changing call sites.
    #[must_use]
    pub fn is_satisfiable(&self, c: ConceptId) -> Option<bool> {
        match self.pool.get(c) {
            ConceptExpr::Top
            | ConceptExpr::Atomic(_)
            | ConceptExpr::Nominal(_)
            | ConceptExpr::SelfRestriction(_) => Some(true),
            ConceptExpr::Bot => Some(false),
            ConceptExpr::Not(inner) => match self.pool.get(*inner) {
                ConceptExpr::Top => Some(false),
                ConceptExpr::Atomic(_) | ConceptExpr::Nominal(_) | ConceptExpr::Bot => Some(true),
                _ => None,
            },
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use owl_dl_core::{ClassId, Role, RoleId};

    fn pool_with_a_and_not_a() -> (ConceptPool, ConceptId, ConceptId) {
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let not_a = pool.not(a);
        (pool, a, not_a)
    }

    #[test]
    fn new_node_creates_empty_node() {
        let pool = ConceptPool::new();
        let mut ctx = TableauContext::new(&pool);
        let n = ctx.new_node();
        assert_eq!(n.index(), 0);
        assert!(ctx.graph().node(n).labels().is_empty());
        assert!(ctx.graph().node(n).edges().is_empty());
    }

    #[test]
    fn add_label_is_idempotent_and_records_once() {
        let (pool, a, _) = pool_with_a_and_not_a();
        let mut ctx = TableauContext::new(&pool);
        let n = ctx.new_node();
        let trail_len_before = ctx.trail().len();
        assert!(ctx.add_label(n, a));
        assert!(!ctx.add_label(n, a));
        assert_eq!(ctx.graph().node(n).labels(), &[a]);
        assert_eq!(ctx.trail().len(), trail_len_before + 1);
    }

    #[test]
    fn labels_stay_sorted() {
        let mut pool = ConceptPool::new();
        let c0 = pool.atomic(ClassId::new(0));
        let c1 = pool.atomic(ClassId::new(1));
        let c2 = pool.atomic(ClassId::new(2));
        let mut ctx = TableauContext::new(&pool);
        let n = ctx.new_node();
        ctx.add_label(n, c2);
        ctx.add_label(n, c0);
        ctx.add_label(n, c1);
        let labels = ctx.graph().node(n).labels();
        let mut sorted = labels.to_vec();
        sorted.sort();
        assert_eq!(labels, sorted.as_slice());
    }

    #[test]
    fn clash_on_bot() {
        let mut pool = ConceptPool::new();
        let bot = pool.bot();
        let mut ctx = TableauContext::new(&pool);
        let n = ctx.new_node();
        assert!(!ctx.clash_in(n));
        ctx.add_label(n, bot);
        assert!(ctx.clash_in(n));
    }

    #[test]
    fn clash_on_complementary_pair() {
        let (pool, a, not_a) = pool_with_a_and_not_a();
        let mut ctx = TableauContext::new(&pool);
        let n = ctx.new_node();
        ctx.add_label(n, a);
        assert!(!ctx.clash_in(n));
        ctx.add_label(n, not_a);
        assert!(ctx.clash_in(n));
    }

    #[test]
    fn trail_round_trip_undoes_label_and_clears_clash() {
        let (pool, a, not_a) = pool_with_a_and_not_a();
        let mut ctx = TableauContext::new(&pool);
        let n = ctx.new_node();
        ctx.add_label(n, a);
        let cp = ctx.checkpoint();
        ctx.add_label(n, not_a);
        assert!(ctx.clash_in(n));
        ctx.rollback_to(cp);
        assert!(!ctx.clash_in(n));
        assert_eq!(ctx.graph().node(n).labels(), &[a]);
    }

    #[test]
    fn rollback_drops_nodes_created_after_checkpoint() {
        let pool = ConceptPool::new();
        let mut ctx = TableauContext::new(&pool);
        let n0 = ctx.new_node();
        let cp = ctx.checkpoint();
        let _n1 = ctx.new_node();
        let _n2 = ctx.new_node();
        assert_eq!(ctx.graph().len(), 3);
        ctx.rollback_to(cp);
        assert_eq!(ctx.graph().len(), 1);
        assert_eq!(n0.index(), 0);
    }

    #[test]
    fn rollback_undoes_edge_addition() {
        let pool = ConceptPool::new();
        let mut ctx = TableauContext::new(&pool);
        let from = ctx.new_node();
        let to = ctx.new_node();
        let cp = ctx.checkpoint();
        ctx.add_edge(from, RoleId::new(0), to);
        assert_eq!(ctx.graph().node(from).edges().len(), 1);
        ctx.rollback_to(cp);
        assert!(ctx.graph().node(from).edges().is_empty());
    }

    #[test]
    fn is_satisfiable_handles_trivial_shapes() {
        let mut pool = ConceptPool::new();
        let top = pool.top();
        let bot = pool.bot();
        let a = pool.atomic(ClassId::new(0));
        let not_a = pool.not(a);
        let self_r = pool.self_restriction(Role::named(RoleId::new(0)));
        let some_r_a = pool.some(Role::named(RoleId::new(0)), a);
        let ctx = TableauContext::new(&pool);
        assert_eq!(ctx.is_satisfiable(top), Some(true));
        assert_eq!(ctx.is_satisfiable(bot), Some(false));
        assert_eq!(ctx.is_satisfiable(a), Some(true));
        assert_eq!(ctx.is_satisfiable(not_a), Some(true));
        assert_eq!(ctx.is_satisfiable(self_r), Some(true));
        assert_eq!(ctx.is_satisfiable(some_r_a), None);
    }
}
