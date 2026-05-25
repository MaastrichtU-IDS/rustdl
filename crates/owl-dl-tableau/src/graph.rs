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

use std::collections::HashMap;

use owl_dl_core::{ConceptId, IndividualId, Role, RoleId};
use smallvec::SmallVec;

/// The set of `branch_id`s whose `⊔`- or choose-rule decisions a
/// particular label or edge derivation depended on. Empty means the
/// label/edge was added by a deterministic rule with no upstream
/// branch decisions (e.g. a residual GCI, an absorbed `ConceptRule`
/// triggered by an axiom-direct atomic).
///
/// Used by Phase 4's dependency-directed back-jumping (see
/// `docs/phase4-backjumping-plan.md`). The list is kept sorted +
/// dedup'd so set union / membership tests run in O(n).
///
/// Backed by `SmallVec<[u32; 1]>`: the single-branch case (one
/// `branch_id` in the set) stays inline, no heap alloc. Profiling on
/// pizza.ofn (see `docs/flamegraphs/pizza-2026-05-24.svg`) showed
/// per-edge `DepSet::clone` dominating `apply_role_chains`; inlining
/// the common case removes that allocation.
pub type DepSet = SmallVec<[u32; 1]>;

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
    /// Per-label [`DepSet`], parallel to [`Self::labels`] (same index,
    /// same length). Populated by `add_label_with_deps`; the legacy
    /// `add_label` writes an empty `DepSet`. Tracks which branch
    /// decisions a label derivation depended on; used by Phase 4 DDB.
    pub(crate) label_deps: Vec<DepSet>,
    pub(crate) edges: SmallVec<[(RoleId, NodeId); 4]>,
    /// Per-out-edge `DepSet`, parallel to [`Self::edges`].
    pub(crate) edge_deps: Vec<DepSet>,
    /// Incoming forward edges — for any edge `y —r→ self` somewhere
    /// in the graph, `(r, y)` lives here. Maintained as a redundant
    /// index of [`Self::edges`] across the graph so inverse-aware
    /// rules can iterate neighbours without scanning every node.
    pub(crate) in_edges: SmallVec<[(RoleId, NodeId); 2]>,
    /// Per-in-edge `DepSet`, parallel to [`Self::in_edges`]. Mirrors
    /// the matching `edge_deps` entry on the source node.
    pub(crate) in_edge_deps: Vec<DepSet>,
    /// Pairwise inequality assertions: every [`NodeId`] in this list
    /// is known to denote a *different* individual than this node.
    /// Symmetric — if `a ∈ b.inequalities` then `b ∈ a.inequalities`.
    pub(crate) inequalities: SmallVec<[NodeId; 2]>,
    /// `Some(t)` if this node has been merged into `t` by
    /// `apply_max`. All meaningful state has been transferred to
    /// `t`; this node is a redirect. Resolved through chains via
    /// [`crate::TableauContext::resolve`].
    pub(crate) merged_into: Option<NodeId>,
}

/// Bit position contributed by `c` to a [`Node::label_sig`] bloom
/// signature. Uses Knuth's multiplicative hash on the raw u32 index
/// so adjacent IDs spread across the 64-bit word rather than packing
/// into one byte (which would defeat the prefilter on small label
/// pools).
#[inline]
#[must_use]
pub(crate) fn label_sig_bit(c: ConceptId) -> u64 {
    // 0x9E37_79B9 ≈ 2^32 / φ. Top 6 bits of the mixed product index
    // the 64 bits of the signature.
    let h = c.index().wrapping_mul(0x9E37_79B9);
    1u64 << (h >> 26)
}

/// Sum-OR of [`label_sig_bit`] over a label slice.
#[inline]
#[must_use]
pub(crate) fn label_sig_of(labels: &[ConceptId]) -> u64 {
    let mut s = 0u64;
    for &c in labels {
        s |= label_sig_bit(c);
    }
    s
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
    pub fn merged_into(&self) -> Option<NodeId> {
        self.merged_into
    }

    #[must_use]
    pub fn is_redirected(&self) -> bool {
        self.merged_into.is_some()
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

    /// Read the [`DepSet`] of label `c` on this node, if present.
    /// Returns `None` for labels that aren't in `L(node)`. Empty
    /// `DepSet` means the label was added by a deterministic rule
    /// with no upstream branch decisions.
    #[must_use]
    pub fn deps_of_label(&self, c: ConceptId) -> Option<&DepSet> {
        let pos = self.labels.binary_search(&c).ok()?;
        Some(&self.label_deps[pos])
    }

    /// Read the [`DepSet`] of the *first* edge from this node with
    /// the given `(role, target)`. Returns `None` if no such edge
    /// exists.
    #[must_use]
    pub fn deps_of_edge(&self, role: RoleId, target: NodeId) -> Option<&DepSet> {
        let pos = self.edges.iter().position(|&e| e == (role, target))?;
        Some(&self.edge_deps[pos])
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

/// Compact summary of one node's pair-blocking inputs, laid out in a
/// dense parallel array (`CompletionGraph::blocking`) so that the
/// ancestor walk in [`crate::TableauContext::is_blocked`] stays in
/// cache. The full [`Node`] is ~200 bytes — pulling it in just to
/// read `parent`, `parent_role`, and `label_sig` was the dominant
/// cost after the B.4 prefilter landed
/// (`docs/flamegraphs/pizza-2026-05-24-post-b4.svg`).
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct BlockingSummary {
    pub(crate) parent: Option<NodeId>,
    pub(crate) parent_role: Option<Role>,
    pub(crate) label_sig: u64,
}

/// The set of nodes and edges built up during one tableau run.
///
/// The graph itself is just storage; legal mutations go through
/// [`crate::TableauContext`] which records every change on the
/// [`crate::TableauTrail`] so backtracking restores prior state.
#[derive(Clone, Debug, Default)]
pub struct CompletionGraph {
    pub(crate) nodes: Vec<Node>,
    /// Cache-dense mirror of each node's pair-blocking inputs;
    /// indexed by `NodeId`. Maintained in lockstep with `nodes` in
    /// `push_node_with_parent`, `truncate_nodes`, label add, and
    /// `LabelAdded` rollback. See [`BlockingSummary`].
    pub(crate) blocking: Vec<BlockingSummary>,
    /// Per-node memo: `true` once
    /// [`crate::rules::apply_residual_gcis`] has materialized every
    /// residual GCI on this node. Subsequent calls short-circuit.
    /// Cleared in the `LabelAdded` rollback on any node whose label
    /// list changed — conservative (re-running the rule is a no-op
    /// if the labels are still present) but trivially correct.
    ///
    /// Counter data on `pizza.ofn` showed 99.3 % of `add_label`
    /// invocations are no-ops; nearly all of them come from this
    /// rule eagerly re-asserting the same residuals on every sweep.
    /// See `docs/perf-2026-05-24-new-server.md` Phase A.1.
    pub(crate) residuals_saturated: Vec<bool>,
    /// Per-node worklist bit for the saturation driver. `true` means
    /// "some rule's input changed on this node since the saturator
    /// last processed it" — labels or edges added or removed. Set by
    /// the mutation accessors ([`crate::TableauContext::add_label_with_deps`],
    /// edge add / remove, merge). Cleared by
    /// [`crate::saturate::saturate`] before it runs the per-node rule
    /// block. Without this bit the saturator's outer loop re-runs all
    /// 12 rules on all nodes every pass, even when nothing relevant
    /// changed since the previous pass; the bit caps work at "rules
    /// re-fire only when a relevant input changed."
    pub(crate) dirty: Vec<bool>,
    /// Canonical node for each nominal individual seen during this
    /// tableau run. Phase 5 (N): when a node is labelled `{a}` and
    /// another already represents `a`, they must denote the same
    /// individual — the rule merges them.
    pub(crate) nominals: HashMap<IndividualId, NodeId>,
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
        self.blocking.push(BlockingSummary {
            parent,
            parent_role,
            label_sig: 0,
        });
        self.residuals_saturated.push(false);
        // New nodes start dirty: the saturator must visit them at
        // least once to apply rules over their initial labels/edges.
        self.dirty.push(true);
        id
    }

    /// Truncate the node arena. Used by trail rollback after
    /// [`crate::TrailEntry::NodeCreated`] entries. The
    /// [`Self::blocking`] mirror, the residual-saturation memo, and
    /// the worklist dirty bit are truncated in lockstep.
    pub(crate) fn truncate_nodes(&mut self, new_len: usize) {
        self.nodes.truncate(new_len);
        self.blocking.truncate(new_len);
        self.residuals_saturated.truncate(new_len);
        self.dirty.truncate(new_len);
    }

    /// Worklist accessor: `true` iff this node has had a relevant
    /// input change since the saturator last processed it.
    #[must_use]
    pub(crate) fn is_dirty(&self, id: NodeId) -> bool {
        self.dirty[id.0 as usize]
    }

    /// Mark `id` as needing re-saturation (or clean again).
    pub(crate) fn set_dirty(&mut self, id: NodeId, value: bool) {
        self.dirty[id.0 as usize] = value;
    }

    /// Mark *every* node as needing re-saturation. Used by
    /// [`crate::saturate::saturate`] at entry: the search adds a
    /// disjunct's label (or rollback removes labels) between calls,
    /// so we conservatively re-process everything once. Most rules
    /// short-circuit (residual-GCI memo, no new atomic triggers,
    /// edges unchanged) and intra-saturate iterations honour the
    /// fine-grained worklist.
    pub(crate) fn mark_all_dirty(&mut self) {
        self.dirty.iter_mut().for_each(|d| *d = true);
    }

    /// Read accessor for the residual-saturation memo.
    #[must_use]
    pub(crate) fn residuals_saturated(&self, id: NodeId) -> bool {
        self.residuals_saturated[id.0 as usize]
    }

    /// Set the residual-saturation memo for `id`.
    pub(crate) fn set_residuals_saturated(&mut self, id: NodeId, value: bool) {
        self.residuals_saturated[id.0 as usize] = value;
    }

    /// Read accessor for the cache-dense blocking summary of `id`.
    /// Used by [`crate::TableauContext::is_blocked`].
    #[must_use]
    pub(crate) fn blocking(&self, id: NodeId) -> &BlockingSummary {
        &self.blocking[id.0 as usize]
    }

    /// Mutable accessor for the blocking summary, used by label-add /
    /// label-undo paths to keep the mirror in lockstep with `Node`.
    pub(crate) fn blocking_mut(&mut self, id: NodeId) -> &mut BlockingSummary {
        &mut self.blocking[id.0 as usize]
    }

    /// Look up the canonical node currently assigned to `individual`,
    /// if any. The result may itself be a redirected node; callers
    /// should pass it through [`crate::TableauContext::resolve`].
    #[must_use]
    pub fn nominal_node(&self, individual: IndividualId) -> Option<NodeId> {
        self.nominals.get(&individual).copied()
    }
}
