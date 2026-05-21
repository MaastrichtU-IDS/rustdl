//! ALC expansion rules.
//!
//! Each rule is a function from `(&mut TableauContext, NodeId)` to
//! `RuleOutcome`. The driver in [`crate::saturate`] picks a non-blocked
//! node, asks each rule whether it applies, and stops when no rule
//! adds anything (saturation) or a clash appears.
//!
//! ## Phase 2 commit 6 scope
//!
//! Deterministic rules covered:
//!
//! - `⊓` decomposition at a single node ([`apply_and`])
//! - `∀` propagation along role edges ([`apply_forall`])
//! - `⊑` via the four absorbed-TBox families:
//!   [`apply_concept_rules`], [`apply_nominal_rules`],
//!   [`apply_role_rules`], [`apply_residual_gcis`]
//! - `∃` generation with naive subset blocking ([`apply_exists`])
//!
//! The non-deterministic `⊔` rule lives in [`crate::search`] since it
//! requires a backtracking driver rather than a fixed-point sweep.

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
pub fn apply_and(ctx: &mut TableauContext<'_, '_, '_>, node: NodeId) -> RuleOutcome {
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
pub fn apply_forall(ctx: &mut TableauContext<'_, '_, '_>, node: NodeId) -> RuleOutcome {
    let mut pending: Vec<(NodeId, ConceptId)> = Vec::new();
    {
        let n = ctx.graph().node(node);
        for &c in n.labels() {
            if let ConceptExpr::All(role, body) = ctx.pool().get(c) {
                let want: RoleId = role.role_id();
                for &(edge_role, target) in n.edges() {
                    if ctx.edge_satisfies(edge_role, want) {
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

/// `ConceptRule` family: for every absorbed
/// `ConceptRule { trigger, conclusion }` whose `trigger` (as
/// [`ConceptExpr::Atomic`]) appears in `L(node)`, add `conclusion` to
/// `L(node)`.
///
/// Returns [`RuleOutcome::NoChange`] when the context has no `TBox`.
pub fn apply_concept_rules(ctx: &mut TableauContext<'_, '_, '_>, node: NodeId) -> RuleOutcome {
    let Some(tbox) = ctx.tbox() else {
        return RuleOutcome::NoChange;
    };
    if tbox.concept_rules.is_empty() {
        return RuleOutcome::NoChange;
    }
    let triggers: Vec<owl_dl_core::ClassId> = ctx
        .graph()
        .node(node)
        .labels()
        .iter()
        .filter_map(|&c| match ctx.pool().get(c) {
            ConceptExpr::Atomic(cls) => Some(*cls),
            _ => None,
        })
        .collect();
    if triggers.is_empty() {
        return RuleOutcome::NoChange;
    }
    let pending: Vec<ConceptId> = tbox
        .concept_rules
        .iter()
        .filter(|r| triggers.contains(&r.trigger))
        .map(|r| r.conclusion)
        .collect();
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

/// `NominalRule` family: for every absorbed
/// `NominalRule { individual, conclusion }` whose
/// [`ConceptExpr::Nominal`] form appears in `L(node)`, add
/// `conclusion` to `L(node)`.
///
/// Phase 2 ALC does not yet handle individual-identity merges; this
/// rule is wired but only fires when a nominal literal happens to
/// label some node (e.g., from a `ClassAssertion` lowering not yet
/// implemented here). Kept in the driver so the integration point is
/// stable for Phase 5.
pub fn apply_nominal_rules(ctx: &mut TableauContext<'_, '_, '_>, node: NodeId) -> RuleOutcome {
    let Some(tbox) = ctx.tbox() else {
        return RuleOutcome::NoChange;
    };
    if tbox.nominal_rules.is_empty() {
        return RuleOutcome::NoChange;
    }
    let individuals: Vec<owl_dl_core::IndividualId> = ctx
        .graph()
        .node(node)
        .labels()
        .iter()
        .filter_map(|&c| match ctx.pool().get(c) {
            ConceptExpr::Nominal(i) => Some(*i),
            _ => None,
        })
        .collect();
    if individuals.is_empty() {
        return RuleOutcome::NoChange;
    }
    let pending: Vec<ConceptId> = tbox
        .nominal_rules
        .iter()
        .filter(|r| individuals.contains(&r.individual))
        .map(|r| r.conclusion)
        .collect();
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

/// `RoleRule` family: for every absorbed
/// `RoleRule { role, guard, target_label }` and every edge
/// `node —role→ y`, add `target_label` to `L(y)` if either
/// `guard` is `None` or [`ConceptExpr::Atomic(guard)`] is in
/// `L(node)`.
pub fn apply_role_rules(ctx: &mut TableauContext<'_, '_, '_>, node: NodeId) -> RuleOutcome {
    let Some(tbox) = ctx.tbox() else {
        return RuleOutcome::NoChange;
    };
    if tbox.role_rules.is_empty() {
        return RuleOutcome::NoChange;
    }
    let mut pending: Vec<(NodeId, ConceptId)> = Vec::new();
    {
        let pool = ctx.pool();
        let n = ctx.graph().node(node);
        let guards_present: Vec<owl_dl_core::ClassId> = n
            .labels()
            .iter()
            .filter_map(|&c| match pool.get(c) {
                ConceptExpr::Atomic(cls) => Some(*cls),
                _ => None,
            })
            .collect();
        for rule in &tbox.role_rules {
            let guard_ok = match rule.guard {
                None => true,
                Some(g) => guards_present.contains(&g),
            };
            if !guard_ok {
                continue;
            }
            for &(edge_role, target) in n.edges() {
                if ctx.edge_satisfies(edge_role, rule.role) {
                    pending.push((target, rule.target_label));
                }
            }
        }
    }
    let mut applied = false;
    for (target, c) in pending {
        if ctx.add_label(target, c) {
            applied = true;
        }
    }
    if applied {
        RuleOutcome::Applied
    } else {
        RuleOutcome::NoChange
    }
}

/// Residual-GCI family: add every `⊤ ⊑ φ` body that survived
/// absorption to every node's label set.
///
/// Idempotent: subsequent passes are O(|residuals|) lookups with no
/// graph mutation once each node already carries the residuals.
pub fn apply_residual_gcis(ctx: &mut TableauContext<'_, '_, '_>, node: NodeId) -> RuleOutcome {
    let Some(tbox) = ctx.tbox() else {
        return RuleOutcome::NoChange;
    };
    if tbox.residual_gcis.is_empty() {
        return RuleOutcome::NoChange;
    }
    let pending: Vec<ConceptId> = tbox.residual_gcis.clone();
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

/// `∃`-rule: for every `Some(R, C)` in `L(x)`, ensure x has an
/// R-successor whose label set contains `C`. If no existing R-edge
/// from `x` reaches a node already carrying `C`, allocate a fresh
/// successor via [`TableauContext::new_successor`] and seed it with
/// `C`.
///
/// Skipped entirely when `x` is subset-blocked by an ancestor (see
/// [`TableauContext::is_blocked`]): the ancestor already witnesses
/// every existential `x` would generate.
///
/// Returns [`RuleOutcome::Applied`] if any successor was created or
/// re-used by adding a new label.
pub fn apply_exists(ctx: &mut TableauContext<'_, '_, '_>, node: NodeId) -> RuleOutcome {
    if ctx.is_blocked(node) {
        return RuleOutcome::NoChange;
    }
    let pending: Vec<(RoleId, ConceptId)> = ctx
        .graph()
        .node(node)
        .labels()
        .iter()
        .filter_map(|&c| match ctx.pool().get(c) {
            ConceptExpr::Some(role, body) => Some((role.role_id(), *body)),
            _ => None,
        })
        .collect();
    if pending.is_empty() {
        return RuleOutcome::NoChange;
    }
    let mut applied = false;
    for (role, body) in pending {
        // Witness check honours the role hierarchy: an edge via any
        // sub-role of `role` to a node already carrying `body`
        // discharges the existential. Generation, however, always
        // uses the exact `role` — using a sub-role would be
        // unsound (a sub-role assertion implies the super-role, not
        // vice versa).
        let witness = ctx
            .graph()
            .node(node)
            .edges()
            .iter()
            .find(|&&(edge_role, target)| {
                ctx.edge_satisfies(edge_role, role) && ctx.graph().node(target).has_label(body)
            })
            .map(|&(_, target)| target);
        if witness.is_some() {
            continue;
        }
        let succ = ctx.new_successor(node, role);
        if ctx.add_label(succ, body) {
            applied = true;
        }
    }
    if applied {
        RuleOutcome::Applied
    } else {
        RuleOutcome::NoChange
    }
}
