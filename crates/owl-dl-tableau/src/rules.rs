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
use owl_dl_core::{ConceptExpr, ConceptId, Role, RoleId};

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
                let wanted: Role = *role;
                for (seen, neighbour) in n.neighbours() {
                    if ctx.edge_satisfies(seen, wanted) {
                        pending.push((neighbour, *body));
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
            for (seen, neighbour) in n.neighbours() {
                if ctx.edge_satisfies(seen, rule.role) {
                    pending.push((neighbour, rule.target_label));
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
    let pending: Vec<(Role, ConceptId)> = ctx
        .graph()
        .node(node)
        .labels()
        .iter()
        .filter_map(|&c| match ctx.pool().get(c) {
            ConceptExpr::Some(role, body) => Some((*role, *body)),
            _ => None,
        })
        .collect();
    if pending.is_empty() {
        return RuleOutcome::NoChange;
    }
    let mut applied = false;
    for (role, body) in pending {
        // Witness check honours the role hierarchy and inverse
        // polarity: any neighbour reachable via a sub-role of `role`
        // (same polarity) that already carries `body` discharges the
        // existential.
        let witness = ctx
            .graph()
            .node(node)
            .neighbours()
            .find(|&(seen, neighbour)| {
                ctx.edge_satisfies(seen, role) && ctx.graph().node(neighbour).has_label(body)
            })
            .map(|(_, neighbour)| neighbour);
        if witness.is_some() {
            continue;
        }
        // Generation: use the exact role (no sub-role substitution —
        // unsound). Polarity dictates direction: a named role grows
        // a successor; an inverse role grows a predecessor.
        let fresh = match role {
            Role::Named(r) => ctx.new_successor(node, r),
            Role::Inverse(r) => ctx.new_predecessor(node, r),
        };
        if ctx.add_label(fresh, body) {
            applied = true;
        }
    }
    if applied {
        RuleOutcome::Applied
    } else {
        RuleOutcome::NoChange
    }
}

/// `≥n R.C` rule: for every `Min(n, R, C)` in `L(node)`, ensure
/// `node` has at least `n` pairwise-distinct R-successors carrying
/// `C`.
///
/// Skipped at blocked nodes (the blocking ancestor already witnesses
/// any cardinality assertion via label inclusion).
///
/// Algorithm:
/// 1. Collect existing R-witnesses (via inverse-aware traversal,
///    sub-role-aware match) that already carry `C`.
/// 2. If at least `n` exist, no generation is needed but we still
///    pairwise-mark them distinct so a future `≤m R.C` (Q2) can see
///    the existing constraint.
/// 3. Otherwise, generate fresh successors via the exact role (named
///    polarity → `new_successor`; inverse polarity → `new_predecessor`)
///    until the count reaches `n`. All witnesses (existing ∪ fresh)
///    are then marked pairwise distinct.
pub fn apply_min(ctx: &mut TableauContext<'_, '_, '_>, node: NodeId) -> RuleOutcome {
    if ctx.is_blocked(node) {
        return RuleOutcome::NoChange;
    }
    let mins: Vec<(u32, Role, ConceptId)> = ctx
        .graph()
        .node(node)
        .labels()
        .iter()
        .filter_map(|&c| match ctx.pool().get(c) {
            ConceptExpr::Min(n, role, body) => Some((*n, *role, *body)),
            _ => None,
        })
        .collect();
    if mins.is_empty() {
        return RuleOutcome::NoChange;
    }
    let mut applied = false;
    for (n, role, body) in mins {
        if n == 0 {
            continue;
        }
        // Existing R-witnesses carrying `body`. Collect into a Vec
        // (deduped: an edge that loops or that we'd otherwise count
        // twice for some reason gets counted once).
        let mut existing: Vec<NodeId> = Vec::new();
        for (seen, neighbour) in ctx.graph().node(node).neighbours() {
            if ctx.edge_satisfies(seen, role)
                && ctx.graph().node(neighbour).has_label(body)
                && !existing.contains(&neighbour)
            {
                existing.push(neighbour);
            }
        }
        let need = (n as usize).saturating_sub(existing.len());
        let mut all_witnesses = existing;
        for _ in 0..need {
            let fresh = match role {
                Role::Named(r) => ctx.new_successor(node, r),
                Role::Inverse(r) => ctx.new_predecessor(node, r),
            };
            ctx.add_label(fresh, body);
            all_witnesses.push(fresh);
            applied = true;
        }
        // Pairwise-mark all witnesses distinct. mark_distinct is
        // idempotent and a no-op when a == b, so this is safe.
        for i in 0..all_witnesses.len() {
            for j in (i + 1)..all_witnesses.len() {
                if !ctx.are_distinct(all_witnesses[i], all_witnesses[j]) {
                    ctx.mark_distinct(all_witnesses[i], all_witnesses[j]);
                    applied = true;
                }
            }
        }
    }
    if applied {
        RuleOutcome::Applied
    } else {
        RuleOutcome::NoChange
    }
}

/// `≤n R.C` rule: for every `Max(n, R, C)` in `L(node)`, ensure at
/// most `n` distinct R-neighbours of `node` carry `C`.
///
/// Algorithm:
/// 1. Skip blocked nodes.
/// 2. Collect distinct R-neighbours where `C ∈ L(neighbour)`.
/// 3. If `count <= n`, no action.
/// 4. Otherwise find a pair `(a, b)` not yet known distinct and
///    merge `b` into `a` via [`TableauContext::merge_into`].
/// 5. If every pair in the over-count set is pairwise distinct,
///    the constraint cannot be satisfied — flag the node with `Bot`
///    so clash detection fires.
///
/// The choose rule is in [`crate::search`]: this rule assumes the
/// neighbours' `C`/`¬C` labelling is already decided.
pub fn apply_max(ctx: &mut TableauContext<'_, '_, '_>, node: NodeId) -> RuleOutcome {
    if ctx.is_blocked(node) {
        return RuleOutcome::NoChange;
    }
    let maxes: Vec<(u32, Role, ConceptId)> = ctx
        .graph()
        .node(node)
        .labels()
        .iter()
        .filter_map(|&c| match ctx.pool().get(c) {
            ConceptExpr::Max(n, role, body) => Some((*n, *role, *body)),
            _ => None,
        })
        .collect();
    if maxes.is_empty() {
        return RuleOutcome::NoChange;
    }
    let mut applied = false;
    for (n, role, body) in maxes {
        // Distinct R-neighbours carrying body (deduped: edges to
        // the same NodeId count once).
        let mut c_neighbours: Vec<NodeId> = Vec::new();
        for (seen, w) in ctx.graph().node(node).neighbours() {
            if ctx.edge_satisfies(seen, role)
                && ctx.graph().node(w).has_label(body)
                && !c_neighbours.contains(&w)
            {
                c_neighbours.push(w);
            }
        }
        if c_neighbours.len() <= n as usize {
            continue;
        }
        // Find a mergeable pair (not already known distinct).
        let mut merged = false;
        'pairs: for i in 0..c_neighbours.len() {
            for j in (i + 1)..c_neighbours.len() {
                let a = c_neighbours[i];
                let b = c_neighbours[j];
                if !ctx.are_distinct(a, b) && ctx.merge_into(b, a) {
                    applied = true;
                    merged = true;
                    break 'pairs;
                }
            }
        }
        if !merged
            && let Some(bot) = ctx.pool().bot_id()
            && ctx.add_label(node, bot)
        {
            applied = true;
        }
    }
    if applied {
        RuleOutcome::Applied
    } else {
        RuleOutcome::NoChange
    }
}

/// Nominal-assignment rule: when `node` is labelled `Nominal(a)`,
/// either claim `node` as the canonical witness of individual `a`
/// (if no prior claim exists) or merge `node` with the existing
/// canonical node.
///
/// OWL 2 has no Unique Name Assumption: two `Nominal({a})` and
/// `Nominal({b})` labels on the same node don't clash on their
/// own; they jointly force `SameIndividual(a, b)` to hold in the
/// model. The merge semantics drop out naturally — both
/// individuals end up pointing at the same canonical node.
///
/// Distinctness comes from `DifferentIndividuals` axioms (not yet
/// processed by the facade) which would `mark_distinct` the
/// corresponding nominal nodes; a forced merge between distinct
/// nodes then returns `false` and the caller flags the clash.
pub fn apply_nominal_assignment(ctx: &mut TableauContext<'_, '_, '_>, node: NodeId) -> RuleOutcome {
    // Collect Nominal(a) individuals from this node's labels.
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
    let mut applied = false;
    let resolved_node = ctx.resolve(node);
    for ind in individuals {
        match ctx.graph().nominal_node(ind) {
            None => {
                ctx.assign_nominal(ind, resolved_node);
                applied = true;
            }
            Some(other) => {
                let other_res = ctx.resolve(other);
                if other_res != resolved_node {
                    // Merge other_res into resolved_node. If the
                    // merge is rejected (declared distinct), the
                    // caller surfaces the clash via the next
                    // iteration's clash_in check — we still need
                    // to flag the node with ⊥.
                    if !ctx.merge_into(other_res, resolved_node)
                        && let Some(bot) = ctx.pool().bot_id()
                    {
                        ctx.add_label(resolved_node, bot);
                    }
                    // After merging, update the nominal map so
                    // subsequent lookups skip the resolve chain.
                    ctx.assign_nominal(ind, resolved_node);
                    applied = true;
                }
            }
        }
    }
    if applied {
        RuleOutcome::Applied
    } else {
        RuleOutcome::NoChange
    }
}

/// Length-2 role-chain rule: for every registered chain axiom
/// `r₁ ∘ r₂ ⊑ sup` and every pair of outgoing edges
/// `node —r₁→ mid —r₂→ tail`, ensure the implied edge
/// `node —sup→ tail` exists. Deduplicated against existing outgoing
/// `sup`-edges so transitivity (`r ∘ r ⊑ r`) does not loop.
///
/// Phase 5 (R): restricted to **named** roles end-to-end. Inverse roles
/// in chain axioms are rejected upstream by the reasoner facade with
/// `ReasonError::RoleChainUnsupported`.
///
/// Skipped at blocked nodes — the blocking ancestor already witnesses
/// any chain-derived edge by label inclusion.
pub fn apply_role_chains(ctx: &mut TableauContext<'_, '_, '_>, node: NodeId) -> RuleOutcome {
    if ctx.is_blocked(node) {
        return RuleOutcome::NoChange;
    }
    if ctx.chains().is_empty() {
        return RuleOutcome::NoChange;
    }
    let chains: Vec<(RoleId, RoleId, RoleId)> = ctx.chains().to_vec();
    let outgoing: Vec<(RoleId, NodeId)> = ctx.graph().node(node).edges().to_vec();
    let mut pending: Vec<(RoleId, NodeId)> = Vec::new();
    for (r1, r2, sup) in chains {
        for &(role_a, mid) in &outgoing {
            if role_a != r1 {
                continue;
            }
            let mid_res = ctx.resolve(mid);
            let mid_edges: Vec<(RoleId, NodeId)> = ctx.graph().node(mid_res).edges().to_vec();
            for (role_b, tail) in mid_edges {
                if role_b != r2 {
                    continue;
                }
                let tail_res = ctx.resolve(tail);
                let already = ctx
                    .graph()
                    .node(node)
                    .edges()
                    .iter()
                    .any(|&(r, t)| r == sup && ctx.resolve(t) == tail_res)
                    || pending.iter().any(|&(r, t)| r == sup && t == tail_res);
                if !already {
                    pending.push((sup, tail_res));
                }
            }
        }
    }
    if pending.is_empty() {
        return RuleOutcome::NoChange;
    }
    for (sup, tail) in pending {
        ctx.add_edge(node, sup, tail);
    }
    RuleOutcome::Applied
}
