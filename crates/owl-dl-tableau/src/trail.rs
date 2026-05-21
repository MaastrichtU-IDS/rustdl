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
    /// checkpoints, so the last entry is the right one.
    EdgeAdded {
        from: NodeId,
        role: RoleId,
        target: NodeId,
    },
    /// A new node was allocated. `prior_len` is the node-arena length
    /// *before* this allocation; rollback truncates back to that
    /// length, dropping this node and any nodes created after it.
    NodeCreated { prior_len: usize },
    /// `a` and `b` were marked pairwise distinct. Undo removes the
    /// last entry on each node's `inequalities` list (append-only
    /// discipline between checkpoints lets us pop without scanning).
    DistinctMarked { a: NodeId, b: NodeId },
    /// An out-edge `(role, target)` was removed from `from`'s edge
    /// list at position `position`. The mirror in-edge at `target`
    /// was also removed; both halves are restored on undo. Used by
    /// `merge_into` to re-anchor edges to the merge target.
    EdgeRemoved {
        from: NodeId,
        role: RoleId,
        target: NodeId,
        position: usize,
        in_position: usize,
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
    NominalAssigned {
        individual: IndividualId,
        prior: Option<NodeId>,
    },
    /// Marker recording a position in the trail. [`TableauTrail::rollback_to`]
    /// takes the [`Checkpoint`] returned by [`TableauTrail::checkpoint`]
    /// and undoes everything after it.
    Checkpoint,
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

/// Append-only log of [`TrailEntry`]s, plus rollback.
///
/// The trail does not own the graph; rollback takes a `&mut CompletionGraph`
/// and applies the inverse of each entry in reverse order.
#[derive(Clone, Debug, Default)]
pub struct TableauTrail {
    entries: Vec<TrailEntry>,
}

impl TableauTrail {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
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
        let position = self.entries.len();
        self.entries.push(TrailEntry::Checkpoint);
        Checkpoint { position }
    }

    pub(crate) fn record(&mut self, entry: TrailEntry) {
        self.entries.push(entry);
    }

    /// Undo every entry after `cp` in reverse order and drop the
    /// checkpoint itself. The graph is restored bit-for-bit to its
    /// state at the moment [`Self::checkpoint`] returned.
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
        while self.entries.len() > cp.position {
            let entry = self.entries.pop().expect("non-empty by loop guard");
            undo(&entry, graph);
        }
    }
}

fn undo(entry: &TrailEntry, graph: &mut CompletionGraph) {
    match *entry {
        TrailEntry::LabelAdded { node, concept } => {
            let labels = &mut graph.node_mut(node).labels;
            let pos = labels
                .binary_search(&concept)
                .expect("LabelAdded undo: concept missing from sorted labels");
            labels.remove(pos);
        }
        TrailEntry::EdgeAdded { from, role, target } => {
            // Pop the outgoing edge at `from`.
            let edges = &mut graph.node_mut(from).edges;
            let last = edges.pop().expect("EdgeAdded undo: edge list empty");
            debug_assert_eq!(
                last,
                (role, target),
                "EdgeAdded undo: trail/graph edge mismatch"
            );
            // Pop the mirror in-edge at `target`. Same append-only
            // discipline applies — every EdgeAdded entry on the
            // trail corresponds to one push on each side.
            let in_edges = &mut graph.node_mut(target).in_edges;
            let last_in = in_edges
                .pop()
                .expect("EdgeAdded undo: target in-edges empty");
            debug_assert_eq!(
                last_in,
                (role, from),
                "EdgeAdded undo: trail/graph in-edge mismatch"
            );
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
            let ineq_b = &mut graph.node_mut(b).inequalities;
            let last = ineq_b.pop().expect("DistinctMarked undo: b empty");
            debug_assert_eq!(last, a, "DistinctMarked undo: b/a mismatch");
        }
        TrailEntry::EdgeRemoved {
            from,
            role,
            target,
            position,
            in_position,
        } => {
            // Re-insert the out-edge at the original position.
            graph.node_mut(from).edges.insert(position, (role, target));
            // Re-insert the mirror in-edge.
            graph
                .node_mut(target)
                .in_edges
                .insert(in_position, (role, from));
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
        }
        TrailEntry::NominalAssigned { individual, prior } => match prior {
            Some(node) => {
                graph.nominals.insert(individual, node);
            }
            None => {
                graph.nominals.remove(&individual);
            }
        },
        TrailEntry::Checkpoint => {}
    }
}
