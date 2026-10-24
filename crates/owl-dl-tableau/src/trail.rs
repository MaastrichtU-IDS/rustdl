//! Log-and-undo trail for tableau backtracking.
//!
//! Every mutation of the [`crate::CompletionGraph`] that happens after
//! a [`Checkpoint`] entry must push a matching [`TrailEntry`] so that
//! [`TableauTrail::rollback_to`] can restore the graph to the
//! checkpointed state.
//!
//! [`Checkpoint`]: TrailEntry::Checkpoint
//!
//! ## Why a single trail (not per-node snapshots)
//!
//! ALC tableau backtracking visits exponentially many branches in the
//! worst case. Snapshotting the entire graph at every ⊔ choice point
//! is O(graph) per branch — fatal. A flat trail records O(1)
//! information per mutation and rolls back by replaying entries in
//! reverse; cost is proportional to *changes since the checkpoint*,
//! not graph size.
//!
//! ## Why `Checkpoint` is just a marker
//!
//! A checkpoint records a position in the trail and the node count at
//! that position. `rollback_to(cp)` truncates the trail back to that
//! position, undoing every later entry in reverse order. The
//! [`TrailEntry::NodeCreated`] variant carries no payload because
//! "the node count before this entry" is reconstructible from the
//! position of the entry in the trail — but we store the *prior* node
//! count anyway so rollback doesn't have to count.

use crate::graph::{CompletionGraph, NodeId};
use owl_dl_core::{ConceptId, IndividualId, Role, RoleId};

/// A single recorded mutation of the completion graph, or a checkpoint
/// marker.
///
/// Entries are appended in the order the mutations happen and undone
/// in reverse order on rollback.
#[derive(Clone, Debug)]
pub enum TrailEntry {
    /// `c` was added to `node`'s label list. Undo: remove it.
    LabelAdded { node: NodeId, concept: ConceptId },
    /// `(role, target)` was appended to `from`'s edge list. Undo: pop
    /// the last edge from `from`. Edges are append-only between
    /// checkpoints, so the last entry is the right one. `id` is the edge's
    /// instance id (#288 step 4b), which a keyed undo removes by.
    EdgeAdded {
        from: NodeId,
        role: RoleId,
        target: NodeId,
        id: u32,
    },
    /// A new node was allocated. `prior_len` is the node-arena length
    /// *before* this allocation; rollback truncates back to that
    /// length, dropping this node and any nodes created after it.
    /// The node's id is `prior_len`; its deps (#288 step 1c) are the
    /// node's `exist_deps`, stored on the node rather than here.
    NodeCreated { prior_len: usize },
    /// `a` and `b` were marked pairwise distinct. Undo removes the
    /// last entry on each node's `inequalities` list (append-only
    /// discipline between checkpoints lets us pop without scanning),
    /// with its `inequality_deps` entry.
    DistinctMarked { a: NodeId, b: NodeId },
    /// An out-edge `(role, target)` was removed from `from`'s edge
    /// list at position `position`. The mirror in-edge at `target`
    /// was also removed; both halves are restored on undo. Used by
    /// `merge_into` to re-anchor edges to the merge target.
    ///
    /// `prior_edge_deps` and `prior_in_edge_deps` carry the
    /// [`crate::DepSet`]s that were attached to those edge slots
    /// before removal, so undo can re-insert them in lockstep with
    /// the edges themselves. Phase 4 commit 1 invariant.
    EdgeRemoved {
        from: NodeId,
        role: RoleId,
        target: NodeId,
        position: usize,
        in_position: usize,
        /// The removed edge's instance id (#288 step 4b).
        id: u32,
        prior_edge_deps: crate::graph::DepSet,
        prior_in_edge_deps: crate::graph::DepSet,
    },
    /// `node` was marked as merged into `new_target`. Undo restores
    /// the prior `merged_into` value (usually `None`).
    MergedRedirect {
        node: NodeId,
        new_target: NodeId,
        prior_redirect: Option<NodeId>,
    },
    /// `node`'s `parent` / `parent_role` was rewritten (typically
    /// during merge, when a child's parent was the source side and
    /// needs to point at the target). Undo restores the prior
    /// `(parent, parent_role)` pair.
    ParentRewritten {
        node: NodeId,
        prior_parent: Option<NodeId>,
        prior_parent_role: Option<Role>,
    },
    /// The canonical node for `individual` (the `O` axis of `SROIQ`)
    /// was changed. Stored on the [`crate::TableauContext`], outside
    /// the graph itself; the trail entry holds the prior mapping so
    /// undo restores it.
    ///
    /// `node` is the new canonical node and `deps` what the assignment
    /// depends on (#288 step 1c): the `{a}` label's deps ∪ the node's
    /// existence deps (∪ the merge's, after a nominal merge). Recorded
    /// only; undo does not read them.
    NominalAssigned {
        individual: IndividualId,
        prior: Option<NodeId>,
        node: NodeId,
        deps: crate::graph::DepSet,
    },
    /// Start of a merge unit (#288 step 1c): every entry up to the matching
    /// [`TrailEntry::MergeEnd`] was pushed by merging `source` into `target`
    /// and depends on `deps`, the merge's deps. Units never nest. Undo is a
    /// no-op; the entries inside undo themselves.
    MergeBegin {
        source: NodeId,
        target: NodeId,
        deps: crate::graph::DepSet,
    },
    /// End of the merge unit opened by the last [`TrailEntry::MergeBegin`].
    MergeEnd,
    /// Marker recording a position in the trail. [`TableauTrail::rollback_to`]
    /// takes the [`Checkpoint`] returned by [`TableauTrail::checkpoint`]
    /// and undoes everything after it.
    Checkpoint,
    /// `saturate` reached a fixpoint here (#288). Not a mutation; undo is a
    /// no-op. `pending` lists the nodes a full pass would visit again on the
    /// next call. See [`TableauTrail::labels_since_stable`].
    StableMark { pending: Vec<NodeId> },
    /// A selective undo (#288 step 4) rebuilt the trail from here. Not a
    /// mutation; undo is a no-op. The `StableMark`s above it were dropped, and
    /// [`TableauTrail::labels_since_stable`] stops here, so the next `saturate`
    /// takes the full path.
    SelectiveBarrier,
}

/// What [`TableauTrail::selective_undo`] does with one entry (#288 step 4b).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EntryAction {
    /// Stays on the trail, effect kept.
    Keep,
    /// Undone by key, then removed from the trail.
    Undo,
    /// Removed from the trail without undoing: markers, and an
    /// `EdgeRemoved` whose edge's `EdgeAdded` is undone. A `StableMark` is
    /// always removed, whatever its action: the fixpoint it records predates
    /// the undo (plan B4).
    Drop,
}

/// Where the entries kept by a [`TableauTrail::selective_undo`] moved to.
#[derive(Clone, Debug)]
pub struct TrailRemap {
    start: usize,
    /// For entry `start + i`, its new index, or `None` if it was removed.
    moved: Vec<Option<usize>>,
}

/// How a keyed undo treats [`TrailEntry::NodeCreated`].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum NodeUndo {
    /// Rollback: truncate the arena back to `prior_len`.
    Truncate,
    /// Selective undo: tombstone the node, keeping ids stable.
    Tombstone,
}

/// Opaque handle into the trail returned by [`TableauTrail::checkpoint`].
///
/// Pass back to [`TableauTrail::rollback_to`] to restore the graph to
/// the state it had when the checkpoint was created.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Checkpoint {
    /// Index of the [`TrailEntry::Checkpoint`] entry in the trail.
    position: usize,
}

impl Checkpoint {
    /// This checkpoint after `remap`, or `None` if its entry was removed.
    /// `self` must predate the selective undo.
    #[must_use]
    pub fn remap(self, remap: &TrailRemap) -> Option<Self> {
        if self.position < remap.start {
            return Some(self);
        }
        debug_assert!(
            self.position < remap.start + remap.moved.len(),
            "Checkpoint::remap: checkpoint taken after the selective undo"
        );
        remap
            .moved
            .get(self.position - remap.start)
            .copied()
            .flatten()
            .map(|position| Self { position })
    }
}

/// Append-only log of [`TrailEntry`]s, plus rollback.
///
/// The trail does not own the graph; rollback takes a `&mut CompletionGraph`
/// and applies the inverse of each entry in reverse order.
#[derive(Clone, Debug, Default)]
pub struct TableauTrail {
    entries: Vec<TrailEntry>,
    /// Bumped on every record, checkpoint and rollback, so it changes whenever
    /// the graph may have. Unlike `len`, it never repeats after a rollback, which
    /// makes it a sound cache key for derived state (#288).
    generation: u64,
    /// Roll back with keyed undo (#288 step 4b; set under
    /// `RUSTDL_SELECTIVE_UNDO=1`). After a selective undo, entries recorded
    /// before it no longer match the graph positionally, so a later rollback
    /// over them must find each item by key rather than pop the last one.
    keyed: bool,
}

impl TableauTrail {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A trail that rolls back with keyed undo iff `keyed` (#288 step 4b).
    #[must_use]
    pub fn keyed_if(keyed: bool) -> Self {
        Self {
            keyed,
            ..Self::default()
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Record a checkpoint. The returned handle is the only legal
    /// argument to [`Self::rollback_to`].
    pub fn checkpoint(&mut self) -> Checkpoint {
        self.generation += 1;
        let position = self.entries.len();
        self.entries.push(TrailEntry::Checkpoint);
        Checkpoint { position }
    }

    pub(crate) fn record(&mut self, entry: TrailEntry) {
        self.generation += 1;
        self.entries.push(entry);
    }

    /// If the graph is a saturation fixpoint plus local label additions only,
    /// the labelled nodes and the last [`TrailEntry::StableMark`]'s deferred
    /// set: scanning back from the end meets nothing but local
    /// [`TrailEntry::LabelAdded`]s and [`TrailEntry::Checkpoint`]s before a
    /// mark. A rollback pops entries, so after one the scan sees the restored
    /// state. `local(c)` must say whether a label `c` only matters to its own
    /// node and that node's neighbours; any other label also disqualifies.
    #[must_use]
    pub fn labels_since_stable(
        &self,
        local: impl Fn(ConceptId) -> bool,
    ) -> Option<(Vec<NodeId>, &[NodeId])> {
        let mut labelled = Vec::new();
        for entry in self.entries.iter().rev() {
            match entry {
                TrailEntry::StableMark { pending } => return Some((labelled, pending)),
                TrailEntry::LabelAdded { node, concept } if local(*concept) => {
                    labelled.push(*node);
                }
                TrailEntry::Checkpoint => {}
                _ => return None,
            }
        }
        None
    }

    /// Whether any entry past `len` mutates the graph: a
    /// [`TrailEntry::StableMark`] records a fixpoint, not a change (#288).
    #[must_use]
    pub fn mutated_since(&self, len: usize) -> bool {
        self.entries[len.min(self.entries.len())..]
            .iter()
            .any(|e| !matches!(e, TrailEntry::StableMark { .. }))
    }

    /// A counter that changes whenever the trail does; see `generation`.
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Undo every entry after `cp` in reverse order and drop the
    /// checkpoint itself. The graph is restored bit-for-bit to its
    /// state at the moment [`Self::checkpoint`] returned — except after a
    /// selective undo (#288 step 4), where nodes tombstoned since may stay in
    /// the arena as dead slots and kept edges may sit at other positions.
    ///
    /// # Panics
    ///
    /// Panics if `cp` does not point at a [`TrailEntry::Checkpoint`]
    /// — for example if the trail was already rolled back past it.
    pub fn rollback_to(&mut self, cp: Checkpoint, graph: &mut CompletionGraph) {
        assert!(
            cp.position < self.entries.len(),
            "checkpoint already rolled back"
        );
        assert!(
            matches!(self.entries[cp.position], TrailEntry::Checkpoint),
            "checkpoint handle does not point at a Checkpoint entry"
        );
        self.generation += 1;
        while self.entries.len() > cp.position {
            let entry = self.entries.pop().expect("non-empty by loop guard");
            if self.keyed {
                undo_keyed(&entry, graph, NodeUndo::Truncate);
            } else {
                undo(&entry, graph);
            }
        }
    }

    /// Roll back with keyed undo from now on (#288 step 4b).
    pub fn set_keyed(&mut self, keyed: bool) {
        self.keyed = keyed;
    }

    /// Bump the generation without recording anything: state the trail does
    /// not describe changed (#288 step 4b).
    pub(crate) fn bump_generation(&mut self) {
        self.generation += 1;
    }

    /// The entries from `start` on (#288 step 4b).
    #[must_use]
    pub fn entries_from(&self, start: usize) -> &[TrailEntry] {
        &self.entries[start..]
    }

    /// The trail position of `cp` (#288 step 4b).
    #[must_use]
    pub fn position(cp: Checkpoint) -> usize {
        cp.position
    }

    /// Turn `Keep`/`Undo` on an [`TrailEntry::EdgeRemoved`] into `Drop` when
    /// the same edge instance's [`TrailEntry::EdgeAdded`] (from `start` on) is
    /// undone (plan B3): the edge it removed never existed.
    pub fn drop_removals_of_undone_edges(&self, start: usize, actions: &mut [EntryAction]) {
        let undone: std::collections::HashSet<u32> = self.entries[start..]
            .iter()
            .zip(actions.iter())
            .filter_map(|(e, a)| match (e, a) {
                (TrailEntry::EdgeAdded { id, .. }, EntryAction::Undo) => Some(*id),
                _ => None,
            })
            .collect();
        if undone.is_empty() {
            return;
        }
        for (e, a) in self.entries[start..].iter().zip(actions.iter_mut()) {
            if let TrailEntry::EdgeRemoved { id, .. } = e
                && undone.contains(id)
            {
                *a = EntryAction::Drop;
            }
        }
    }

    /// Extend `Undo` along prior-value chains (#288 step 4b, review B-1).
    /// `ParentRewritten` and `MergedRedirect` (per node) and `NominalAssigned`
    /// (per individual) undo by restoring the value the previous entry for the
    /// same key wrote. If an earlier entry for a key is undone and a later one
    /// kept, a later rollback through the kept one would restore a value the
    /// kept history never had. So every entry after an undone one, for the
    /// same key, is undone too; walking backwards then restores the right
    /// value.
    pub fn undo_later_writes(&self, start: usize, actions: &mut [EntryAction]) {
        let mut undone: std::collections::HashSet<WriteKey> = std::collections::HashSet::new();
        for (e, a) in self.entries[start..].iter().zip(actions.iter_mut()) {
            let Some(key) = write_key(e) else { continue };
            if *a == EntryAction::Undo {
                undone.insert(key);
            } else if undone.contains(&key) {
                *a = EntryAction::Undo;
            }
        }
    }

    /// Selective undo (#288 step 4b). `actions[i]` says what to do with entry
    /// `start + i`. Undoes the `Undo` entries by key in reverse order
    /// (tombstoning created nodes), then rebuilds the trail from `start` as a
    /// [`TrailEntry::SelectiveBarrier`] followed by the `Keep` entries in their
    /// order. Returns where kept entries moved, and the nodes the undone
    /// entries touched.
    ///
    /// # Panics
    ///
    /// Panics if `actions` does not cover exactly the entries from `start`.
    pub fn selective_undo(
        &mut self,
        start: usize,
        actions: &[EntryAction],
        graph: &mut CompletionGraph,
    ) -> (TrailRemap, Vec<NodeId>) {
        assert_eq!(
            actions.len(),
            self.entries.len() - start,
            "selective_undo: one action per entry from start"
        );
        debug_assert!(
            {
                let mut a = actions.to_vec();
                self.undo_later_writes(start, &mut a);
                a == actions
            },
            "selective_undo: a kept prior-value write follows an undone one"
        );
        debug_assert!(
            {
                let mut a = actions.to_vec();
                self.drop_removals_of_undone_edges(start, &mut a);
                a == actions
            },
            "selective_undo: a removal of an undone edge is not dropped"
        );
        debug_assert!(
            self.entries[start..]
                .iter()
                .zip(actions)
                .all(|(e, a)| *a != EntryAction::Drop || droppable(e)),
            "selective_undo: Drop on an entry that changed the graph"
        );
        self.generation += 1;
        let mut touched = Vec::new();
        for (entry, action) in self.entries[start..].iter().zip(actions).rev() {
            if *action == EntryAction::Undo {
                entry_nodes(entry, &mut touched);
                undo_keyed(entry, graph, NodeUndo::Tombstone);
            }
        }
        let tail = self.entries.split_off(start);
        self.entries.push(TrailEntry::SelectiveBarrier);
        let mut moved = Vec::with_capacity(tail.len());
        for (entry, action) in tail.into_iter().zip(actions) {
            if *action == EntryAction::Keep && !matches!(entry, TrailEntry::StableMark { .. }) {
                moved.push(Some(self.entries.len()));
                self.entries.push(entry);
            } else {
                moved.push(None);
            }
        }
        touched.sort_unstable();
        touched.dedup();
        (TrailRemap { start, moved }, touched)
    }
}

impl TableauTrail {
    /// Rebuild the trail from `start` with `inserts` placed just before the
    /// entry at their position (in list order for one position) and without
    /// the entries at `removes` (#288 step 4c, plan Revision 2 B-A: the merge
    /// re-copy). The graph is the caller's to update. Returns where the
    /// surviving entries moved; `Checkpoint` entries are never inserted or
    /// removed, so every checkpoint survives.
    ///
    /// # Panics
    ///
    /// Panics if an insert position is before `start` or past the end.
    pub(crate) fn splice(
        &mut self,
        start: usize,
        mut inserts: Vec<(usize, TrailEntry)>,
        removes: &std::collections::HashSet<usize>,
    ) -> TrailRemap {
        debug_assert!(
            removes.iter().all(|&p| p >= start
                && matches!(
                    self.entries[p],
                    TrailEntry::LabelAdded { .. } | TrailEntry::DistinctMarked { .. }
                )),
            "splice: removes only label and mark entries from start on"
        );
        inserts.sort_by_key(|(p, _)| *p);
        self.generation += 1;
        let tail = self.entries.split_off(start);
        let mut moved = Vec::with_capacity(tail.len());
        let mut ins = inserts.into_iter().peekable();
        for (i, entry) in tail.into_iter().enumerate() {
            let old = start + i;
            while let Some((_, e)) = ins.next_if(|(p, _)| *p == old) {
                self.entries.push(e);
            }
            if removes.contains(&old) {
                moved.push(None);
            } else {
                moved.push(Some(self.entries.len()));
                self.entries.push(entry);
            }
        }
        // Inserts at the end of the trail go last.
        let end = start + moved.len();
        for (p, e) in ins {
            assert_eq!(p, end, "splice: insert position past the trail");
            self.entries.push(e);
        }
        TrailRemap { start, moved }
    }
}

/// Whether `entry` may be dropped without undoing it: it records no graph
/// change, or it is an `EdgeRemoved` (whose `EdgeAdded` must be undone, which
/// the removal-rule assertion checks).
fn droppable(entry: &TrailEntry) -> bool {
    matches!(
        entry,
        TrailEntry::Checkpoint
            | TrailEntry::StableMark { .. }
            | TrailEntry::MergeBegin { .. }
            | TrailEntry::MergeEnd
            | TrailEntry::SelectiveBarrier
            | TrailEntry::EdgeRemoved { .. }
    )
}

/// The key a prior-value entry writes, for [`TableauTrail::undo_later_writes`].
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
enum WriteKey {
    Parent(NodeId),
    Redirect(NodeId),
    Nominal(IndividualId),
}

fn write_key(entry: &TrailEntry) -> Option<WriteKey> {
    match *entry {
        TrailEntry::ParentRewritten { node, .. } => Some(WriteKey::Parent(node)),
        TrailEntry::MergedRedirect { node, .. } => Some(WriteKey::Redirect(node)),
        TrailEntry::NominalAssigned { individual, .. } => Some(WriteKey::Nominal(individual)),
        _ => None,
    }
}

/// The nodes `entry` mutates.
fn entry_nodes(entry: &TrailEntry, out: &mut Vec<NodeId>) {
    match *entry {
        TrailEntry::LabelAdded { node, .. }
        | TrailEntry::ParentRewritten { node, .. }
        | TrailEntry::MergedRedirect { node, .. } => out.push(node),
        TrailEntry::EdgeAdded { from, target, .. }
        | TrailEntry::EdgeRemoved { from, target, .. } => {
            out.extend([from, target]);
        }
        TrailEntry::DistinctMarked { a, b } => out.extend([a, b]),
        TrailEntry::NodeCreated { prior_len } => {
            out.push(NodeId::new(
                u32::try_from(prior_len).expect("node count fits u32"),
            ));
        }
        TrailEntry::NominalAssigned { node, prior, .. } => {
            out.push(node);
            out.extend(prior);
        }
        TrailEntry::MergeBegin { .. }
        | TrailEntry::MergeEnd
        | TrailEntry::Checkpoint
        | TrailEntry::StableMark { .. }
        | TrailEntry::SelectiveBarrier => {}
    }
}

/// Undo `entry` by key (#288 step 4b): find what it added wherever it now is,
/// rather than assume it is the last item. On a purely chronological history
/// this gives the same graph as [`undo`].
fn undo_keyed(entry: &TrailEntry, graph: &mut CompletionGraph, nodes: NodeUndo) {
    match *entry {
        TrailEntry::EdgeAdded {
            from, target, id, ..
        } => {
            let from_node = graph.node_mut(from);
            let Some(pos) = from_node.edge_ids.iter().position(|&e| e == id) else {
                // Already removed (its removal was dropped, plan B3).
                debug_assert!(
                    !graph.node(target).in_edge_ids.contains(&id),
                    "EdgeAdded keyed undo: in-edge mirror without its out-edge"
                );
                return;
            };
            from_node.edges.remove(pos);
            from_node.edge_deps.remove(pos);
            from_node.edge_ids.remove(pos);
            let target_node = graph.node_mut(target);
            let in_pos = target_node
                .in_edge_ids
                .iter()
                .position(|&e| e == id)
                .expect("EdgeAdded keyed undo: in-edge mirror missing");
            target_node.in_edges.remove(in_pos);
            target_node.in_edge_deps.remove(in_pos);
            target_node.in_edge_ids.remove(in_pos);
        }
        TrailEntry::EdgeRemoved {
            from,
            role,
            target,
            position,
            in_position,
            id,
            ref prior_edge_deps,
            ref prior_in_edge_deps,
        } => {
            let from_node = graph.node_mut(from);
            let pos = position.min(from_node.edges.len());
            from_node.edges.insert(pos, (role, target));
            from_node.edge_deps.insert(pos, prior_edge_deps.clone());
            from_node.edge_ids.insert(pos, id);
            let target_node = graph.node_mut(target);
            let in_pos = in_position.min(target_node.in_edges.len());
            target_node.in_edges.insert(in_pos, (role, from));
            target_node
                .in_edge_deps
                .insert(in_pos, prior_in_edge_deps.clone());
            target_node.in_edge_ids.insert(in_pos, id);
        }
        TrailEntry::DistinctMarked { a, b } => {
            for (x, y) in [(a, b), (b, a)] {
                let n = graph.node_mut(x);
                let pos = n
                    .inequalities
                    .iter()
                    .position(|&m| m == y)
                    .expect("DistinctMarked keyed undo: mark missing");
                n.inequalities.remove(pos);
                n.inequality_deps.remove(pos);
            }
        }
        TrailEntry::NodeCreated { prior_len } => match nodes {
            NodeUndo::Truncate => graph.truncate_nodes(prior_len),
            NodeUndo::Tombstone => graph.tombstone(NodeId::new(
                u32::try_from(prior_len).expect("node count fits u32"),
            )),
        },
        TrailEntry::LabelAdded { .. }
        | TrailEntry::MergedRedirect { .. }
        | TrailEntry::ParentRewritten { .. }
        | TrailEntry::NominalAssigned { .. }
        | TrailEntry::MergeBegin { .. }
        | TrailEntry::MergeEnd
        | TrailEntry::Checkpoint
        | TrailEntry::StableMark { .. }
        | TrailEntry::SelectiveBarrier => undo(entry, graph),
    }
}

#[allow(clippy::too_many_lines)]
fn undo(entry: &TrailEntry, graph: &mut CompletionGraph) {
    match *entry {
        TrailEntry::LabelAdded { node, concept } => {
            let n = graph.node_mut(node);
            let pos = n
                .labels
                .binary_search(&concept)
                .expect("LabelAdded undo: concept missing from sorted labels");
            n.labels.remove(pos);
            // The parallel `label_deps` always tracks `labels` in
            // lockstep — `add_label_with_deps` pushes both, undo pops
            // both. Phase 4 commit 1 invariant: every `LabelAdded`
            // entry corresponds to one push on each.
            debug_assert!(
                pos < n.label_deps.len(),
                "LabelAdded undo: label_deps shorter than labels"
            );
            n.label_deps.remove(pos);
            // `label_sig` is the OR of `label_sig_bit` over `labels`.
            // Removing one concept may or may not clear its bit (if no
            // other label hashes to the same position). Recompute from
            // scratch — O(|labels|), same order as the binary_search
            // above, so no asymptotic cost on rollback.
            let new_sig = crate::graph::label_sig_of(&n.labels);
            graph.blocking_mut(node).label_sig = new_sig;
            // Conservatively clear the residual-saturation memo:
            // any label removal might have removed a residual. The
            // next `apply_residual_gcis` call will re-materialize
            // (binary-search-confirmed no-op for residuals still
            // present, real insert for the one(s) just rolled back).
            graph.set_residuals_saturated(node, false);
        }
        TrailEntry::EdgeAdded {
            from, role, target, ..
        } => {
            // Pop the outgoing edge at `from`.
            let from_node = graph.node_mut(from);
            let last = from_node
                .edges
                .pop()
                .expect("EdgeAdded undo: edge list empty");
            debug_assert_eq!(
                last,
                (role, target),
                "EdgeAdded undo: trail/graph edge mismatch"
            );
            // Same append-only discipline for `edge_deps`. Always
            // pushed in lockstep with `edges`; same length invariant.
            from_node
                .edge_deps
                .pop()
                .expect("EdgeAdded undo: edge_deps empty");
            from_node.edge_ids.pop();
            // Pop the mirror in-edge at `target` plus its dep
            // mirror. Every EdgeAdded entry on the trail corresponds
            // to one push on each side.
            let target_node = graph.node_mut(target);
            let last_in = target_node
                .in_edges
                .pop()
                .expect("EdgeAdded undo: target in-edges empty");
            debug_assert_eq!(
                last_in,
                (role, from),
                "EdgeAdded undo: trail/graph in-edge mismatch"
            );
            target_node
                .in_edge_deps
                .pop()
                .expect("EdgeAdded undo: target in_edge_deps empty");
            target_node.in_edge_ids.pop();
        }
        TrailEntry::NodeCreated { prior_len } => {
            graph.truncate_nodes(prior_len);
        }
        TrailEntry::DistinctMarked { a, b } => {
            // Pop the trailing peer from each side; the mark is
            // symmetric and append-only between checkpoints.
            let ineq_a = &mut graph.node_mut(a).inequalities;
            let last = ineq_a.pop().expect("DistinctMarked undo: a empty");
            debug_assert_eq!(last, b, "DistinctMarked undo: a/b mismatch");
            graph.node_mut(a).inequality_deps.pop();
            let ineq_b = &mut graph.node_mut(b).inequalities;
            let last = ineq_b.pop().expect("DistinctMarked undo: b empty");
            debug_assert_eq!(last, a, "DistinctMarked undo: b/a mismatch");
            graph.node_mut(b).inequality_deps.pop();
            debug_assert_eq!(
                graph.node(a).inequalities.len(),
                graph.node(a).inequality_deps.len()
            );
        }
        TrailEntry::EdgeRemoved {
            from,
            role,
            target,
            position,
            in_position,
            id,
            ref prior_edge_deps,
            ref prior_in_edge_deps,
        } => {
            // Re-insert the out-edge at the original position.
            let from_node = graph.node_mut(from);
            from_node.edges.insert(position, (role, target));
            from_node
                .edge_deps
                .insert(position, prior_edge_deps.clone());
            from_node.edge_ids.insert(position, id);
            // Re-insert the mirror in-edge.
            let target_node = graph.node_mut(target);
            target_node.in_edges.insert(in_position, (role, from));
            target_node
                .in_edge_deps
                .insert(in_position, prior_in_edge_deps.clone());
            target_node.in_edge_ids.insert(in_position, id);
        }
        TrailEntry::MergedRedirect {
            node,
            new_target: _,
            prior_redirect,
        } => {
            graph.node_mut(node).merged_into = prior_redirect;
        }
        TrailEntry::ParentRewritten {
            node,
            prior_parent,
            prior_parent_role,
        } => {
            let n = graph.node_mut(node);
            n.parent = prior_parent;
            n.parent_role = prior_parent_role;
            // Mirror into the cache-dense blocking summary.
            let b = graph.blocking_mut(node);
            b.parent = prior_parent;
            b.parent_role = prior_parent_role;
        }
        TrailEntry::NominalAssigned {
            individual, prior, ..
        } => match prior {
            Some(node) => {
                graph.nominals.insert(individual, node);
            }
            None => {
                graph.nominals.remove(&individual);
            }
        },
        TrailEntry::Checkpoint
        | TrailEntry::StableMark { .. }
        | TrailEntry::SelectiveBarrier
        | TrailEntry::MergeBegin { .. }
        | TrailEntry::MergeEnd => {}
    }
}
