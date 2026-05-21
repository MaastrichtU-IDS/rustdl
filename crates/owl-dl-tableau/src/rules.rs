//! ALC expansion rules.
//!
//! Each rule is a function from `(&mut TableauContext, NodeId)` to
//! `RuleOutcome`. The driver in [`crate::saturate`] picks a non-blocked
//! node, asks each rule whether it applies, and stops when no rule
//! adds anything (saturation) or a clash appears.
//!
//! ## Phase 2 commit 2 scope
//!
//! Only the deterministic `⊓` rule lives here so far. Operands of an
//! `And` label are added to the same node. Subsequent commits add `∀`,
//! `⊑` (apply absorbed rules), then the non-deterministic `⊔` and
//! `∃` rules.

use crate::TableauContext;
use crate::graph::NodeId;
use owl_dl_core::ConceptExpr;

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
    let mut pending: Vec<owl_dl_core::ConceptId> = Vec::new();
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
