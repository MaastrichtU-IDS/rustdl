//! Completion graph for the tableau.
//!
//! A [`CompletionGraph`] holds [`Node`]s identified by [`NodeId`]. Each
//! node carries a sorted list of [`ConceptId`] labels, an outgoing
//! edge list of `(RoleId, NodeId)`, and (starting from Phase 3
//! commit 2) an incoming edge list so inverse-aware traversal is O(1)
//! per neighbour. Labels are kept sorted so subset checks and clash
//! lookups are binary searches.
//!
//! All mutating operations are exposed through [`crate::TableauContext`]
//! and route through the [`crate::TableauTrail`] so they can be undone
//! during backtracking. The methods on this module are deliberately
//! pub(crate) where the only correct use is via the trail-mandated
//! context APIs.
//!
//! ## Why sorted labels
//!
//! The two hot operations on labels are:
//! 1. "Is `c` already in `L(x)`?" — used by every rule before adding.
//! 2. Pair blocking — `L(y) ⊆ L(x)`.
//!
//! Sorted `SmallVec` keeps inline allocation for small label sets and
//! gives O(log n) contains + O(n) subset check.

use owl_dl_core::{ConceptId, Role, RoleId};
use smallvec::SmallVec;

/// Index into the node arena of a [`CompletionGraph`].
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(u32);

impl NodeId {
    #[must_use]
    pub const fn new(idx: u32) -> Self {
        Self(idx)
    }

    #[must_use]
    pub const fn index(self) -> u32 {
        self.0
    }
}

/// A node in the completion graph.
///
/// Labels are kept sorted ascending by [`ConceptId`]; edges are stored
/// in insertion order (no semantic significance to ordering).
///
/// `parent` is the *creator* — the node from whose `∃R.C` (or
/// `∃R⁻.C`) the existential rule generated this node. `parent_role`
/// records the [`Role`] expression at the creator that produced this
/// node: `Role::Named(r)` if `∃r.C` (so an edge `parent —r→ self`
/// exists), `Role::Inverse(r)` if `∃r⁻.C` (so an edge
/// `self —r→ parent` exists). Together they drive *pair blocking*.
///
/// Nodes created directly via [`crate::TableauContext::new_node`]
/// (test scaffolding, `ABox` roots) have `parent: None` and are never
/// blocked.
#[derive(Clone, Debug, Default)]
pub struct Node {
    pub(crate) parent: Option<NodeId>,
    pub(crate) parent_role: Option<Role>,
    pub(crate) labels: SmallVec<[ConceptId; 8]>,
    pub(crate) edges: SmallVec<[(RoleId, NodeId); 4]>,
    /// Incoming forward edges — for any edge `y —r→ self` somewhere
    /// in the graph, `(r, y)` lives here. Maintained as a redundant
    /// index of [`Self::edges`] across the graph so inverse-aware
    /// rules can iterate neighbours without scanning every node.
    pub(crate) in_edges: SmallVec<[(RoleId, NodeId); 2]>,
    /// Pairwise inequality assertions: every [`NodeId`] in this list
    /// is known to denote a *different* individual than this node.
    /// Symmetric — if `a ∈ b.inequalities` then `b ∈ a.inequalities`.
    /// Populated by `apply_min` (newly-generated witnesses are marked
    /// pairwise distinct); consulted by `apply_max` (Q2) when
    /// deciding whether two witnesses can be merged.
    pub(crate) inequalities: SmallVec<[NodeId; 2]>,
}

impl Node {
    #[must_use]
    pub fn labels(&self) -> &[ConceptId] {
        &self.labels
    }

    #[must_use]
    pub fn edges(&self) -> &[(RoleId, NodeId)] {
        &self.edges
    }

    #[must_use]
    pub fn in_edges(&self) -> &[(RoleId, NodeId)] {
        &self.in_edges
    }

    #[must_use]
    pub fn parent(&self) -> Option<NodeId> {
        self.parent
    }

    #[must_use]
    pub fn inequalities(&self) -> &[NodeId] {
        &self.inequalities
    }

    #[must_use]
    pub fn parent_role(&self) -> Option<Role> {
        self.parent_role
    }

    /// True if `c` is in this node's label set. O(log n) via binary
    /// search on the sorted label list.
    #[must_use]
    pub fn has_label(&self, c: ConceptId) -> bool {
        self.labels.binary_search(&c).is_ok()
    }

    /// Iterate every role-tagged neighbour of this node, with each
    /// neighbour decorated by the [`Role`] expression *as seen from
    /// this node*:
    /// - outgoing edge `self —r→ y`  yields `(Role::Named(r), y)`
    /// - incoming edge `y —r→ self`  yields `(Role::Inverse(r), y)`
    pub fn neighbours(&self) -> impl Iterator<Item = (Role, NodeId)> + '_ {
        self.edges
            .iter()
            .map(|&(r, t)| (Role::Named(r), t))
            .chain(self.in_edges.iter().map(|&(r, s)| (Role::Inverse(r), s)))
    }
}

/// The set of nodes and edges built up during one tableau run.
///
/// The graph itself is just storage; legal mutations go through
/// [`crate::TableauContext`] which records every change on the
/// [`crate::TableauTrail`] so backtracking restores prior state.
#[derive(Clone, Debug, Default)]
pub struct CompletionGraph {
    pub(crate) nodes: Vec<Node>,
}

impl CompletionGraph {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    #[must_use]
    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.0 as usize]
    }

    pub(crate) fn node_mut(&mut self, id: NodeId) -> &mut Node {
        &mut self.nodes[id.0 as usize]
    }

    /// Allocate a fresh root-level node and return its id. Bare
    /// allocation only — callers should record the
    /// [`crate::TrailEntry::NodeCreated`] event to allow rollback.
    pub(crate) fn push_node(&mut self) -> NodeId {
        self.push_node_with_parent(None, None)
    }

    /// Allocate a fresh node with the given creator and creation
    /// role. Same trail contract as [`Self::push_node`].
    pub(crate) fn push_node_with_parent(
        &mut self,
        parent: Option<NodeId>,
        parent_role: Option<Role>,
    ) -> NodeId {
        let id = NodeId(u32::try_from(self.nodes.len()).expect("node count exceeds u32"));
        self.nodes.push(Node {
            parent,
            parent_role,
            ..Node::default()
        });
        id
    }

    /// Truncate the node arena. Used by trail rollback after
    /// [`crate::TrailEntry::NodeCreated`] entries.
    pub(crate) fn truncate_nodes(&mut self, new_len: usize) {
        self.nodes.truncate(new_len);
    }
}
