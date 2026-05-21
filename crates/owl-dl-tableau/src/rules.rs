//! ALC expansion rules.
//!
//! Each rule is a function from `(&mut TableauContext, NodeId)` to
//! `RuleOutcome`. The driver in [`crate::saturate`] picks a non-blocked
//! node, asks each rule whether it applies, and stops when no rule
//! adds anything (saturation) or a clash appears.
//!
//! ## Phase 2 commit 3 scope
//!
//! Two deterministic rules: `⊓` decomposition at a single node, and
//! `∀` propagation along role edges. Subsequent commits add `⊑`
//! (apply absorbed rules) and then the non-deterministic `⊔` and
//! `∃` rules.

use crate::TableauContext;
use crate::graph::NodeId;
use owl_dl_core::{ConceptExpr, ConceptId, RoleId};

/// What happened when a rule was asked to apply at a node.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RuleOutcome {
    /// The rule added at least one label or edge. Saturation must
    /// continue.
    Applied,
    /// The rule had nothing new to add at this node.
    NoChange,
}

/// ⊓-rule: for every `And([c1, …, cn])` in `L(x)`, add each `ci` to
/// `L(x)`.
///
/// Returns [`RuleOutcome::Applied`] if any operand was newly inserted
/// at `node`.
///
/// Implementation note: we snapshot the relevant `ConceptId`s first to
/// release the borrow on the graph before calling `add_label` (which
/// also borrows `&mut`).
pub fn apply_and(ctx: &mut TableauContext<'_>, node: NodeId) -> RuleOutcome {
    let mut pending: Vec<ConceptId> = Vec::new();
    for &c in ctx.graph().node(node).labels() {
        if let ConceptExpr::And(args) = ctx.pool().get(c) {
            pending.extend(args.iter().copied());
        }
    }
    let mut applied = false;
    for c in pending {
        if ctx.add_label(node, c) {
            applied = true;
        }
    }
    if applied {
        RuleOutcome::Applied
    } else {
        RuleOutcome::NoChange
    }
}

/// ∀-rule: for every `All(R, C)` in `L(x)` and every R-edge
/// `x —R→ y`, add `C` to `L(y)`.
///
/// In ALC (Phase 2) every role is a [`owl_dl_core::Role::named`]
/// wrapper around a [`RoleId`], so matching reduces to equality on
/// `RoleId`. Inverse-role propagation arrives in Phase 3.
///
/// Returns [`RuleOutcome::Applied`] if any successor's label set
/// gained a new concept.
///
/// Implementation note: we snapshot every applicable
/// `(target, concept)` pair before touching `add_label` so the
/// graph-read and graph-write borrows don't overlap.
pub fn apply_forall(ctx: &mut TableauContext<'_>, node: NodeId) -> RuleOutcome {
    let mut pending: Vec<(NodeId, ConceptId)> = Vec::new();
    {
        let graph = ctx.graph();
        let pool = ctx.pool();
        let n = graph.node(node);
        for &c in n.labels() {
            if let ConceptExpr::All(role, body) = pool.get(c) {
                let want: RoleId = role.role_id();
                for &(edge_role, target) in n.edges() {
                    if edge_role == want {
                        pending.push((target, *body));
                    }
                }
            }
        }
    }
    let mut applied = false;
    for (target, body) in pending {
        if ctx.add_label(target, body) {
            applied = true;
        }
    }
    if applied {
        RuleOutcome::Applied
    } else {
        RuleOutcome::NoChange
    }
}
