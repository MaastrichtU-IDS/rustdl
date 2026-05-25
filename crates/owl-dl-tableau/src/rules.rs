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
use crate::deps::union;
use crate::graph::{DepSet, NodeId};
use owl_dl_core::{ConceptExpr, ConceptId, Role, RoleId};
use smallvec::SmallVec;

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
    crate::bump_counter!(ctx, apply_and);
    // Snapshot (operand, deps) pairs first to release the graph
    // borrow before any `add_label_with_deps` mutates the node. The
    // conclusion's deps inherit from the triggering `And` label.
    //
    // Skip conjuncts that are already labels — they'd round-trip
    // through `add_label_with_deps` and return `false` after paying
    // the deps clone. The current label set is sorted by
    // construction, so the presence check is one binary search per
    // conjunct.
    let pending: Vec<(ConceptId, DepSet)> = {
        let n = ctx.graph().node(node);
        let pool = ctx.pool();
        let labels = n.labels();
        let mut out = Vec::new();
        for (pos, &c) in labels.iter().enumerate() {
            if let ConceptExpr::And(args) = pool.get(c) {
                let deps = &n.label_deps[pos];
                for &arg in args {
                    if labels.binary_search(&arg).is_err() {
                        out.push((arg, deps.clone()));
                    }
                }
            }
        }
        out
    };
    let mut applied = false;
    for (c, deps) in pending {
        if ctx.add_label_with_deps(node, c, &deps) {
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
    crate::bump_counter!(ctx, apply_forall);
    // `(target, body, deps)` triples. Conclusion deps = deps of the
    // `All`-label ∪ deps of the matching edge (outgoing or incoming).
    let pending: Vec<(NodeId, ConceptId, DepSet)> = {
        let n = ctx.graph().node(node);
        let pool = ctx.pool();
        let mut out = Vec::new();
        for (pos, &c) in n.labels().iter().enumerate() {
            if let ConceptExpr::All(role, body) = pool.get(c) {
                let wanted: Role = *role;
                let all_deps = &n.label_deps[pos];
                // Outgoing edges first, in `edges` order — index into
                // `edge_deps` matches.
                for (epos, &(edge_role, neighbour)) in n.edges.iter().enumerate() {
                    if ctx.edge_satisfies(Role::Named(edge_role), wanted) {
                        let combined = union(all_deps, &n.edge_deps[epos]);
                        out.push((neighbour, *body, combined));
                    }
                }
                for (epos, &(edge_role, neighbour)) in n.in_edges.iter().enumerate() {
                    if ctx.edge_satisfies(Role::Inverse(edge_role), wanted) {
                        let combined = union(all_deps, &n.in_edge_deps[epos]);
                        out.push((neighbour, *body, combined));
                    }
                }
            }
        }
        out
    };
    let mut applied = false;
    for (target, body, deps) in pending {
        if ctx.add_label_with_deps(target, body, &deps) {
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
    crate::bump_counter!(ctx, apply_concept_rules);
    let Some(tbox) = ctx.tbox() else {
        return RuleOutcome::NoChange;
    };
    if tbox.concept_rules.is_empty() {
        return RuleOutcome::NoChange;
    }
    // Trigger class + the deps of the `Atomic(trigger)` label that
    // licenses each rule firing. Conclusion deps inherit from the
    // triggering atomic label.
    //
    // We also snapshot the current label set so the pending-list
    // construction below can skip conclusions that are already
    // present — those would round-trip through `add_label_with_deps`
    // and return `false` anyway, but a deps clone has already been
    // paid. Filtering early saves the clone on every duplicate; on
    // pizza this rule's clone chain is the top exclusive-time frame.
    let (label_snapshot, triggers) = {
        let n = ctx.graph().node(node);
        let pool = ctx.pool();
        let triggers: Vec<(owl_dl_core::ClassId, DepSet)> = n
            .labels()
            .iter()
            .enumerate()
            .filter_map(|(pos, &c)| match pool.get(c) {
                ConceptExpr::Atomic(cls) => Some((*cls, n.label_deps[pos].clone())),
                _ => None,
            })
            .collect();
        let label_snapshot: SmallVec<[ConceptId; 8]> = n.labels().iter().copied().collect();
        (label_snapshot, triggers)
    };
    if triggers.is_empty() {
        return RuleOutcome::NoChange;
    }
    // (conclusion, deps) pairs. Index lookup is O(triggers + hits).
    // Fall back to the linear scan only when callers built the TBox
    // by hand without calling `finalize()` — e.g., tableau unit tests.
    let pending: Vec<(ConceptId, DepSet)> = if tbox.concept_rules_by_trigger.is_empty() {
        let mut out = Vec::new();
        for (trigger, deps) in &triggers {
            for rule in &tbox.concept_rules {
                if rule.trigger == *trigger
                    && label_snapshot.binary_search(&rule.conclusion).is_err()
                {
                    out.push((rule.conclusion, deps.clone()));
                }
            }
        }
        out
    } else {
        let mut out = Vec::new();
        for (trigger, deps) in &triggers {
            if let Some(conclusions) = tbox.concept_rules_by_trigger.get(trigger) {
                for &c in conclusions {
                    if label_snapshot.binary_search(&c).is_err() {
                        out.push((c, deps.clone()));
                    }
                }
            }
        }
        out
    };
    let mut applied = false;
    for (c, deps) in pending {
        if ctx.add_label_with_deps(node, c, &deps) {
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
    crate::bump_counter!(ctx, apply_nominal_rules);
    let Some(tbox) = ctx.tbox() else {
        return RuleOutcome::NoChange;
    };
    if tbox.nominal_rules.is_empty() {
        return RuleOutcome::NoChange;
    }
    // Nominal trigger + the deps of its `Nominal(_)` label.
    let individuals: Vec<(owl_dl_core::IndividualId, DepSet)> = {
        let n = ctx.graph().node(node);
        let pool = ctx.pool();
        n.labels()
            .iter()
            .enumerate()
            .filter_map(|(pos, &c)| match pool.get(c) {
                ConceptExpr::Nominal(i) => Some((*i, n.label_deps[pos].clone())),
                _ => None,
            })
            .collect()
    };
    if individuals.is_empty() {
        return RuleOutcome::NoChange;
    }
    let pending: Vec<(ConceptId, DepSet)> = if tbox.nominal_rules_by_individual.is_empty() {
        let mut out = Vec::new();
        for (ind, deps) in &individuals {
            for rule in &tbox.nominal_rules {
                if rule.individual == *ind {
                    out.push((rule.conclusion, deps.clone()));
                }
            }
        }
        out
    } else {
        let mut out = Vec::new();
        for (ind, deps) in &individuals {
            if let Some(conclusions) = tbox.nominal_rules_by_individual.get(ind) {
                for &c in conclusions {
                    out.push((c, deps.clone()));
                }
            }
        }
        out
    };
    let mut applied = false;
    for (c, deps) in pending {
        if ctx.add_label_with_deps(node, c, &deps) {
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
    crate::bump_counter!(ctx, apply_role_rules);
    let Some(tbox) = ctx.tbox() else {
        return RuleOutcome::NoChange;
    };
    if tbox.role_rules.is_empty() {
        return RuleOutcome::NoChange;
    }
    // Fall back to the linear scan when the partitioned indices have
    // not been finalized — guarded by both partitions being empty
    // *and* `role_rules` being non-empty (i.e. a hand-built TBox that
    // skipped `finalize`).
    let use_index =
        !(tbox.unguarded_role_rules.is_empty() && tbox.guarded_role_rules_by_guard.is_empty());
    // `(target_node, conclusion_label, deps)`. Unguarded rules
    // inherit only the matching edge's deps. Guarded rules also
    // include the deps of the guard atomic on `node`.
    let pending: Vec<(NodeId, ConceptId, DepSet)> = {
        let pool = ctx.pool();
        let n = ctx.graph().node(node);
        // guard class → deps of its Atomic label on `node`
        let guards_present: std::collections::HashMap<owl_dl_core::ClassId, DepSet> = n
            .labels()
            .iter()
            .enumerate()
            .filter_map(|(pos, &c)| match pool.get(c) {
                ConceptExpr::Atomic(cls) => Some((*cls, n.label_deps[pos].clone())),
                _ => None,
            })
            .collect();
        let mut out = Vec::new();
        // Helper closure: yield matching (edge_role, neighbour, edge_deps)
        // triples for a wanted role.
        let matching_edges = |rule_role: Role| {
            let mut triples: Vec<(Role, NodeId, DepSet)> = Vec::new();
            for (pos, &(role, neighbour)) in n.edges.iter().enumerate() {
                if ctx.edge_satisfies(Role::Named(role), rule_role) {
                    triples.push((Role::Named(role), neighbour, n.edge_deps[pos].clone()));
                }
            }
            for (pos, &(role, neighbour)) in n.in_edges.iter().enumerate() {
                if ctx.edge_satisfies(Role::Inverse(role), rule_role) {
                    triples.push((Role::Inverse(role), neighbour, n.in_edge_deps[pos].clone()));
                }
            }
            triples
        };
        if use_index {
            for rule in &tbox.unguarded_role_rules {
                for (_, neighbour, edge_deps) in matching_edges(rule.role) {
                    out.push((neighbour, rule.target_label, edge_deps));
                }
            }
            for (g, guard_deps) in &guards_present {
                if let Some(rules) = tbox.guarded_role_rules_by_guard.get(g) {
                    for rule in rules {
                        for (_, neighbour, edge_deps) in matching_edges(rule.role) {
                            let combined = union(guard_deps, &edge_deps);
                            out.push((neighbour, rule.target_label, combined));
                        }
                    }
                }
            }
        } else {
            for rule in &tbox.role_rules {
                let guard_deps_opt: Option<&DepSet> = match rule.guard {
                    None => None,
                    Some(g) => guards_present.get(&g),
                };
                if rule.guard.is_some() && guard_deps_opt.is_none() {
                    continue;
                }
                for (_, neighbour, edge_deps) in matching_edges(rule.role) {
                    let combined = match guard_deps_opt {
                        None => edge_deps,
                        Some(gd) => union(gd, &edge_deps),
                    };
                    out.push((neighbour, rule.target_label, combined));
                }
            }
        }
        out
    };
    let mut applied = false;
    for (target, c, deps) in pending {
        if ctx.add_label_with_deps(target, c, &deps) {
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
    crate::bump_counter!(ctx, apply_residual_gcis);
    let Some(tbox) = ctx.tbox() else {
        return RuleOutcome::NoChange;
    };
    if tbox.residual_gcis.is_empty() {
        return RuleOutcome::NoChange;
    }
    // Per-node memo: once every residual GCI has been materialized
    // on `node`, subsequent calls are deterministic no-ops. Avoids
    // the ~10 M wasted `add_label` binary-search probes per 15 s
    // that pizza counters showed (see
    // `docs/perf-2026-05-24-new-server.md` Phase A.1). Cleared on
    // any `LabelAdded` rollback for this node — conservative but
    // trivially correct.
    if ctx.graph().residuals_saturated(node) {
        return RuleOutcome::NoChange;
    }
    let pending: Vec<ConceptId> = tbox.residual_gcis.clone();
    let mut applied = false;
    for c in pending {
        if ctx.add_label(node, c) {
            applied = true;
        }
    }
    ctx.mark_residuals_saturated(node);
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
    crate::bump_counter!(ctx, apply_exists);
    if ctx.is_blocked(node) {
        return RuleOutcome::NoChange;
    }
    // `(role, body, deps_of_the_some_label)` triples.
    let pending: Vec<(Role, ConceptId, DepSet)> = {
        let n = ctx.graph().node(node);
        let pool = ctx.pool();
        n.labels()
            .iter()
            .enumerate()
            .filter_map(|(pos, &c)| match pool.get(c) {
                ConceptExpr::Some(role, body) => Some((*role, *body, n.label_deps[pos].clone())),
                _ => None,
            })
            .collect()
    };
    if pending.is_empty() {
        return RuleOutcome::NoChange;
    }
    let mut applied = false;
    for (role, body, exists_deps) in pending {
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
        // a successor; an inverse role grows a predecessor. The new
        // edge inherits the deps of the licensing ∃ label; the seed
        // label on the fresh node inherits the same.
        let fresh = match role {
            Role::Named(r) => ctx.new_successor_with_deps(node, r, &exists_deps),
            Role::Inverse(r) => ctx.new_predecessor_with_deps(node, r, &exists_deps),
        };
        if ctx.add_label_with_deps(fresh, body, &exists_deps) {
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
    crate::bump_counter!(ctx, apply_min);
    if ctx.is_blocked(node) {
        return RuleOutcome::NoChange;
    }
    let mins: Vec<(u32, Role, ConceptId, DepSet)> = {
        let n = ctx.graph().node(node);
        let pool = ctx.pool();
        n.labels()
            .iter()
            .enumerate()
            .filter_map(|(pos, &c)| match pool.get(c) {
                ConceptExpr::Min(count, role, body) => {
                    Some((*count, *role, *body, n.label_deps[pos].clone()))
                }
                _ => None,
            })
            .collect()
    };
    if mins.is_empty() {
        return RuleOutcome::NoChange;
    }
    let mut applied = false;
    for (n, role, body, min_deps) in mins {
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
            // Generated witnesses inherit the deps of the `Min` label
            // that triggered their creation, both on the generative
            // edge and on the seed body label.
            let fresh = match role {
                Role::Named(r) => ctx.new_successor_with_deps(node, r, &min_deps),
                Role::Inverse(r) => ctx.new_predecessor_with_deps(node, r, &min_deps),
            };
            ctx.add_label_with_deps(fresh, body, &min_deps);
            all_witnesses.push(fresh);
            applied = true;
        }
        // Pairwise-mark *up to* n witnesses distinct — only the n
        // we commit to as the Min(n) constraint's satisfying set,
        // not every R-witness with body that happens to be at the
        // node. Over-asserting distinctness when existing R-witnesses
        // already exceed n (e.g. a concept-rule chain like
        // `:X508 ⊑ :X532` added :X532 to a node that's now showing
        // up as an extra :X532-witness for `Min(2, :r, :X532)`)
        // poisons downstream `Max(k, :r, :X532)` merges by marking
        // pairs distinct that the search needs to be free to merge.
        // SIO_000450 et al. tripped exactly this. See
        // `docs/perf-2026-05-24-new-server.md` §5.
        let commit_count = (n as usize).min(all_witnesses.len());
        for i in 0..commit_count {
            for j in (i + 1)..commit_count {
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
    crate::bump_counter!(ctx, apply_max);
    if ctx.is_blocked(node) {
        return RuleOutcome::NoChange;
    }
    let maxes: Vec<(u32, Role, ConceptId, DepSet)> = {
        let n = ctx.graph().node(node);
        let pool = ctx.pool();
        n.labels()
            .iter()
            .enumerate()
            .filter_map(|(pos, &c)| match pool.get(c) {
                ConceptExpr::Max(count, role, body) => {
                    Some((*count, *role, *body, n.label_deps[pos].clone()))
                }
                _ => None,
            })
            .collect()
    };
    if maxes.is_empty() {
        return RuleOutcome::NoChange;
    }
    let mut applied = false;
    for (n, role, body, max_deps) in maxes {
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
        // The merge is conditional on: (a) the `≤n R.C` label being
        // on this node (max_deps) and (b) both neighbours existing as
        // R-witnesses carrying C (their edge deps + body-label deps).
        // Pass that union as merge_deps so moved labels carry it.
        let mut merged = false;
        'pairs: for i in 0..c_neighbours.len() {
            for j in (i + 1)..c_neighbours.len() {
                let a = c_neighbours[i];
                let b = c_neighbours[j];
                if !ctx.are_distinct(a, b) {
                    // Compute precise merge-condition deps for this pair.
                    let merge_deps: DepSet =
                        compute_max_merge_deps(ctx, node, role, body, a, b, &max_deps);
                    if ctx.merge_into_with_deps(b, a, merge_deps.as_slice()) {
                        applied = true;
                        merged = true;
                        break 'pairs;
                    }
                }
            }
        }
        if !merged && let Some(bot) = ctx.pool().bot_id() {
            // Conservative deps for the clash: union of the Max
            // label's deps and every active branch decision. The
            // contributing neighbour edges + their `body` labels also
            // matter, but `active_branches()` is a strict
            // over-approximation that subsumes them all and keeps the
            // soundness invariant — back-jumping may miss prune
            // opportunities here but will never wrongly propagate.
            let mut deps = max_deps.clone();
            for &b in ctx.active_branches() {
                if let Err(pos) = deps.binary_search(&b) {
                    deps.insert(pos, b);
                }
            }
            if ctx.add_label_with_deps(node, bot, &deps) {
                applied = true;
            }
        }
    }
    if applied {
        RuleOutcome::Applied
    } else {
        RuleOutcome::NoChange
    }
}

/// Compute the precise [`DepSet`] of branch decisions that licensed
/// merging neighbours `a` and `b` of `node` under a `≤n R.C`
/// constraint. The merge depends on: the cardinality label
/// (`max_deps`), the two R-edges that put `a`/`b` in the witness set,
/// and the body labels on each side. Each of those was added with a
/// `DepSet`; the union is the precise reason the search now needs to
/// collapse the two witnesses, and it must flow into every label /
/// edge moved by [`TableauContext::merge_into_with_deps`] so a
/// post-merge clash carries the branch ids back to back-jumping.
fn compute_max_merge_deps(
    ctx: &TableauContext<'_, '_, '_>,
    node: NodeId,
    role: Role,
    body: ConceptId,
    a: NodeId,
    b: NodeId,
    max_deps: &DepSet,
) -> DepSet {
    let mut deps = max_deps.clone();
    // Edge deps for both witnesses + body-label deps on both sides.
    let n = ctx.graph().node(node);
    let pool_role = role;
    for w in [a, b] {
        // First matching edge in either direction satisfying `role`.
        for (pos, &(er, t)) in n.edges.iter().enumerate() {
            if t == w && ctx.edge_satisfies(Role::Named(er), pool_role) {
                deps = union(&deps, &n.edge_deps[pos]);
                break;
            }
        }
        for (pos, &(er, src)) in n.in_edges.iter().enumerate() {
            if src == w && ctx.edge_satisfies(Role::Inverse(er), pool_role) {
                deps = union(&deps, &n.in_edge_deps[pos]);
                break;
            }
        }
        // Body label deps on the witness.
        let wn = ctx.graph().node(w);
        if let Ok(p) = wn.labels.binary_search(&body) {
            deps = union(&deps, &wn.label_deps[p]);
        }
    }
    deps
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
    crate::bump_counter!(ctx, apply_nominal_assignment);
    // Collect (individual, deps of its Nominal label) pairs. The deps
    // matter for the merge-condition: when this nominal label collides
    // with the same nominal on a different node, the resulting merge
    // depends on *both* nominal labels' branch decisions. Passing
    // those to `merge_into_with_deps` so the moved labels/edges carry
    // them is the soundness invariant the search relies on.
    let individuals: Vec<(owl_dl_core::IndividualId, DepSet)> = {
        let n = ctx.graph().node(node);
        let pool = ctx.pool();
        n.labels()
            .iter()
            .enumerate()
            .filter_map(|(pos, &c)| match pool.get(c) {
                ConceptExpr::Nominal(i) => Some((*i, n.label_deps[pos].clone())),
                _ => None,
            })
            .collect()
    };
    if individuals.is_empty() {
        return RuleOutcome::NoChange;
    }
    let mut applied = false;
    let resolved_node = ctx.resolve(node);
    for (ind, here_nom_deps) in individuals {
        match ctx.graph().nominal_node(ind) {
            None => {
                ctx.assign_nominal(ind, resolved_node);
                applied = true;
            }
            Some(other) => {
                let other_res = ctx.resolve(other);
                if other_res != resolved_node {
                    // Find the matching nominal label's deps on the
                    // other node. The merge-condition deps = union of
                    // both sides' nominal-label deps.
                    let other_nom_deps: DepSet = {
                        let other_node = ctx.graph().node(other_res);
                        let pool = ctx.pool();
                        other_node
                            .labels()
                            .iter()
                            .enumerate()
                            .find_map(|(p, &c)| match pool.get(c) {
                                ConceptExpr::Nominal(i) if *i == ind => {
                                    Some(other_node.label_deps[p].clone())
                                }
                                _ => None,
                            })
                            .unwrap_or_default()
                    };
                    let merge_deps: DepSet = union(&here_nom_deps, &other_nom_deps);
                    // Merge other_res into resolved_node, threading
                    // the merge-condition deps so moved labels/edges
                    // carry them.
                    if !ctx.merge_into_with_deps(other_res, resolved_node, merge_deps.as_slice())
                        && let Some(bot) = ctx.pool().bot_id()
                    {
                        // Conservative deps for the failed-merge clash:
                        // active branches ⊇ the precise merge-cond deps.
                        let deps: DepSet = DepSet::from_slice(ctx.active_branches());
                        ctx.add_label_with_deps(resolved_node, bot, &deps);
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

/// Length-2 role-chain rule with per-position polarity.
///
/// For every registered chain axiom `r₁ ∘ r₂ ⊑ sup` (each role may
/// be named or inverse), walk the polarity-correct edge at each
/// position. With `r` ranging over named edges:
/// - `Named(r)` reads an outgoing edge `x —r→ y` and contributes
///   the source `x` / target `y` as the chain's left / right
///   endpoint.
/// - `Inverse(r)` reads an incoming edge `y —r→ x` and contributes
///   the target `y` / source `x` as the chain's left / right
///   endpoint.
///
/// The combined effect is: a length-2 walk `node → mid → tail`
/// (where the arrows respect each position's polarity) implies an
/// edge of `sup`'s polarity between `node` and `tail`.
///
/// Skipped at blocked nodes — the blocking ancestor already
/// witnesses any chain-derived edge by label inclusion.
#[allow(clippy::too_many_lines)]
pub fn apply_role_chains(ctx: &mut TableauContext<'_, '_, '_>, node: NodeId) -> RuleOutcome {
    crate::bump_counter!(ctx, apply_role_chains);
    if ctx.check_deadline() {
        return RuleOutcome::NoChange;
    }
    if ctx.is_blocked(node) {
        return RuleOutcome::NoChange;
    }
    if ctx.chains().is_empty() {
        return RuleOutcome::NoChange;
    }
    let chains: Vec<(Role, Role, Role)> = ctx.chains().to_vec();
    let outgoing: Vec<(RoleId, NodeId, DepSet)> = {
        let n = ctx.graph().node(node);
        n.edges
            .iter()
            .enumerate()
            .map(|(pos, &(r, t))| (r, t, n.edge_deps[pos].clone()))
            .collect()
    };
    let incoming: Vec<(RoleId, NodeId, DepSet)> = {
        let n = ctx.graph().node(node);
        n.in_edges
            .iter()
            .enumerate()
            .map(|(pos, &(r, t))| (r, t, n.in_edge_deps[pos].clone()))
            .collect()
    };
    // Pending chain-derived edges keyed by `(sup, tail_res)`. The
    // earlier Vec + linear `iter_mut().find()` was O(P) per tail and
    // the outer (mid, tail) iteration is O(K²), making the
    // structure O(K² · P) per call. HashMap brings the find to O(1)
    // and the call to O(K² + P). Showed up at ~17 % of CPU on the
    // pizza `:NamedPizza` flamegraph (post-reorder).
    let mut pending: std::collections::HashMap<(Role, NodeId), DepSet> =
        std::collections::HashMap::new();
    for (r1, r2, sup) in chains {
        // Step 1: find every `mid` reachable from `node` via the
        // first chain position, together with the edge deps.
        let mids: Vec<(NodeId, DepSet)> = match r1 {
            Role::Named(r) => outgoing
                .iter()
                .filter_map(|(role, n, d)| {
                    if *role == r {
                        Some((*n, d.clone()))
                    } else {
                        None
                    }
                })
                .collect(),
            Role::Inverse(r) => incoming
                .iter()
                .filter_map(|(role, n, d)| {
                    if *role == r {
                        Some((*n, d.clone()))
                    } else {
                        None
                    }
                })
                .collect(),
        };
        for (mid, head_deps) in mids {
            if ctx.check_deadline() {
                return RuleOutcome::NoChange;
            }
            let mid_res = ctx.resolve(mid);
            // Step 2: tail walk through `mid_res` carrying that edge's
            // deps too.
            let tails: Vec<(NodeId, DepSet)> = {
                let mid_node = ctx.graph().node(mid_res);
                match r2 {
                    Role::Named(r) => mid_node
                        .edges
                        .iter()
                        .enumerate()
                        .filter_map(|(pos, &(role, n))| {
                            if role == r {
                                Some((n, mid_node.edge_deps[pos].clone()))
                            } else {
                                None
                            }
                        })
                        .collect(),
                    Role::Inverse(r) => mid_node
                        .in_edges
                        .iter()
                        .enumerate()
                        .filter_map(|(pos, &(role, n))| {
                            if role == r {
                                Some((n, mid_node.in_edge_deps[pos].clone()))
                            } else {
                                None
                            }
                        })
                        .collect(),
                }
            };
            for (tail, tail_deps) in tails {
                crate::bump_counter!(ctx, apply_role_chains_body_iters);
                if ctx.check_deadline() {
                    return RuleOutcome::NoChange;
                }
                let tail_res = ctx.resolve(tail);
                if chain_edge_already_present(ctx, node, sup, tail_res) {
                    continue;
                }
                let combined = union(&head_deps, &tail_deps);
                pending
                    .entry((sup, tail_res))
                    .and_modify(|d| *d = union(d, &combined))
                    .or_insert(combined);
            }
        }
    }
    if pending.is_empty() {
        return RuleOutcome::NoChange;
    }
    for ((sup, tail), deps) in pending {
        // Polarity of `sup` chooses which direction we materialise:
        // Named(r)  ⇒ outgoing r-edge from node to tail.
        // Inverse(r) ⇒ outgoing r-edge from tail to node (which
        //               looks like an incoming r-edge at node).
        match sup {
            Role::Named(r) => ctx.add_edge_with_deps(node, r, tail, &deps),
            Role::Inverse(r) => ctx.add_edge_with_deps(tail, r, node, &deps),
        }
    }
    RuleOutcome::Applied
}

/// True iff the implied chain edge `node —sup→ tail` (where polarity
/// of `sup` determines direction) is already present.
fn chain_edge_already_present(
    ctx: &TableauContext<'_, '_, '_>,
    node: NodeId,
    sup: Role,
    tail: NodeId,
) -> bool {
    let r = sup.role_id();
    match sup {
        Role::Named(_) => ctx
            .graph()
            .node(node)
            .edges()
            .iter()
            .any(|&(role, t)| role == r && ctx.resolve(t) == tail),
        Role::Inverse(_) => ctx
            .graph()
            .node(tail)
            .edges()
            .iter()
            .any(|&(role, t)| role == r && ctx.resolve(t) == node),
    }
}

/// Self-restriction rule for SROIQ's `ObjectHasSelf` concept.
///
/// Two halves at one node:
/// - **Positive:** for every `SelfRestriction(r)` in `L(node)`, ensure
///   a self-edge `node —r→ node` (or any sub-role / inverse-equivalent
///   self-edge already witnessing it). Since `r(x, x) ⇔ r⁻(x, x)` for
///   any self pair, an inverse-role wanted is satisfied by the named
///   self-edge through [`TableauContext::edge_satisfies`].
/// - **Negative:** for every `Not(SelfRestriction(r))` in `L(node)`,
///   if any existing self-edge `node —s→ node` satisfies `r`, the
///   model would have to both contain and forbid `r(node, node)` —
///   add `⊥` to flag the clash. (The `Bot` label is what
///   [`TableauContext::clash_in`] looks for; adding it surfaces the
///   clash on the next sweep iteration.)
///
/// Skipped at blocked nodes: the blocking ancestor witnesses any
/// self-restriction by label inclusion plus the (well-known)
/// self-loop-respecting pair-blocking discipline.
pub fn apply_self_restriction(ctx: &mut TableauContext<'_, '_, '_>, node: NodeId) -> RuleOutcome {
    crate::bump_counter!(ctx, apply_self_restriction);
    if ctx.is_blocked(node) {
        return RuleOutcome::NoChange;
    }
    // Snapshot the relevant label data; release the immutable borrow
    // before mutating.
    let mut positives: Vec<(Role, DepSet)> = Vec::new();
    let mut negatives: Vec<(Role, DepSet)> = Vec::new();
    {
        let n = ctx.graph().node(node);
        let pool = ctx.pool();
        for (pos, &c) in n.labels().iter().enumerate() {
            let deps = &n.label_deps[pos];
            match pool.get(c) {
                ConceptExpr::SelfRestriction(role) => positives.push((*role, deps.clone())),
                ConceptExpr::Not(inner) => {
                    if let ConceptExpr::SelfRestriction(role) = pool.get(*inner) {
                        negatives.push((*role, deps.clone()));
                    }
                }
                _ => {}
            }
        }
    }
    if positives.is_empty() && negatives.is_empty() {
        return RuleOutcome::NoChange;
    }
    // Helper: does any outgoing self-edge of `node` satisfy `wanted`?
    let self_edges: Vec<RoleId> = ctx
        .graph()
        .node(node)
        .edges()
        .iter()
        .filter_map(|&(r, t)| if t == node { Some(r) } else { None })
        .collect();
    let satisfies_any =
        |wanted: Role, edges: &[RoleId], ctx: &TableauContext<'_, '_, '_>| -> bool {
            edges
                .iter()
                .any(|&r| ctx.edge_satisfies(Role::Named(r), wanted))
        };
    let mut applied = false;
    // Negatives first — a fresh positive added below could shadow an
    // existing self-edge into clash, but the standard pattern is to
    // check before mutating. We flag `⊥` when an existing self-edge
    // already satisfies the negated role.
    let bot_id = ctx.pool().bot_id();
    for (wanted, neg_deps) in &negatives {
        if satisfies_any(*wanted, &self_edges, ctx)
            && let Some(bot) = bot_id
            && ctx.add_label_with_deps(node, bot, neg_deps)
        {
            applied = true;
        }
    }
    for (wanted, pos_deps) in positives {
        if satisfies_any(wanted, &self_edges, ctx) {
            continue;
        }
        // Self-edge polarity is irrelevant for the model (the same
        // pair is its own inverse), so always materialize as a named
        // forward edge on the underlying role id. The negative-self
        // check above will catch any clash introduced this sweep on
        // the next pass.
        ctx.add_edge_with_deps(node, wanted.role_id(), node, &pos_deps);
        applied = true;
    }
    if applied {
        RuleOutcome::Applied
    } else {
        RuleOutcome::NoChange
    }
}

/// Cross-edge role-axiom clash checks for SROIQ's role characteristics
/// that don't reduce to concept axioms:
/// - `AsymmetricObjectProperty(r)`: at every node `x`, if both
///   `x —r→ y` and `y —r→ x` exist for some `y`, then `r(x,y)` and
///   `r(y,x)` together violate asymmetry — flag `⊥`.
/// - `DisjointObjectProperties(r, s)`: at every node `x`, if both
///   `x —r→ y` and `x —s→ y` exist, the two pairs collapse — flag `⊥`.
///
/// Sub-role propagation is left to the role hierarchy: the axiom
/// declarations name *atomic* roles, so the asymmetry / disjointness
/// holds for the literal `RoleId`. (Sub-roles inherit the constraint
/// upstream from the SROIQ legality conditions; we don't enforce that
/// here, the input is assumed regular.)
///
/// Skipped at blocked nodes — the blocking ancestor witnesses any
/// edge configuration by structural inclusion.
pub fn apply_role_axioms(ctx: &mut TableauContext<'_, '_, '_>, node: NodeId) -> RuleOutcome {
    crate::bump_counter!(ctx, apply_role_axioms);
    if ctx.is_blocked(node) {
        return RuleOutcome::NoChange;
    }
    if ctx.asymmetric_roles().is_empty() && ctx.disjoint_role_pairs().is_empty() {
        return RuleOutcome::NoChange;
    }
    let Some(bot) = ctx.pool().bot_id() else {
        return RuleOutcome::NoChange;
    };
    if ctx.graph().node(node).has_label(bot) {
        return RuleOutcome::NoChange;
    }
    let outgoing: Vec<(RoleId, NodeId)> = ctx.graph().node(node).edges().to_vec();
    let mut violated = false;
    // Asymmetric: for each outgoing r-edge to y, look for an r-edge
    // back from y to node.
    for &r in ctx.asymmetric_roles() {
        for &(role, y) in &outgoing {
            if role != r || y == node {
                continue;
            }
            let back_exists = ctx
                .graph()
                .node(y)
                .edges()
                .iter()
                .any(|&(rr, tt)| rr == r && tt == node);
            if back_exists {
                violated = true;
                break;
            }
        }
        if violated {
            break;
        }
    }
    if !violated {
        // Disjoint: for each unordered pair (r, s), look for outgoing
        // edges of both roles to the same target.
        for &(r, s) in ctx.disjoint_role_pairs() {
            let mut r_targets: Vec<NodeId> = outgoing
                .iter()
                .filter_map(|&(rr, t)| if rr == r { Some(t) } else { None })
                .collect();
            let s_targets: Vec<NodeId> = outgoing
                .iter()
                .filter_map(|&(rr, t)| if rr == s { Some(t) } else { None })
                .collect();
            r_targets.sort();
            r_targets.dedup();
            if s_targets.iter().any(|t| r_targets.binary_search(t).is_ok()) {
                violated = true;
                break;
            }
        }
    }
    if violated {
        // Conservative deps: the violation depends on whichever
        // branch decisions placed the two clashing edges. Computing
        // it precisely would require threading per-edge deps through
        // the `outgoing` snapshot above; `active_branches()` is a
        // sound over-approximation that keeps the soundness
        // invariant.
        let deps: DepSet = DepSet::from_slice(ctx.active_branches());
        if ctx.add_label_with_deps(node, bot, &deps) {
            return RuleOutcome::Applied;
        }
    }
    RuleOutcome::NoChange
}
