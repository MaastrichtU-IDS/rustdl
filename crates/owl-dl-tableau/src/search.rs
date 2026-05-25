//! Backtracking driver for the non-deterministic `⊔` rule, with
//! dependency-directed back-jumping (Phase 4 commits 4 + 5).
//!
//! The deterministic rules in [`crate::rules`] cannot handle a label
//! of shape `Or([d1, …, dn])` — they would have to *choose* which
//! disjunct to add. This module implements the choice via depth-first
//! search with trail-based undo.
//!
//! Each `⊔`-branching decision is identified by a unique `branch_id`
//! allocated by [`crate::TableauContext::push_branch`]. When the
//! recursive search detects a clash, [`crate::saturate`] returns the
//! [`crate::SaturationResult::Clash`] variant carrying the
//! [`crate::DepSet`] of the offending complementary labels. Each
//! rule propagates this `DepSet` to its conclusions during saturation
//! (see [`crate::deps`] + the per-rule plumbing in [`crate::rules`]).
//!
//! [`branch`] reads the clash deps. If its own `branch_id` is *not*
//! in there, this disjunction's choice didn't contribute to the clash
//! — every sibling disjunct would clash for the same upstream
//! reason, so we propagate the [`SearchVerdict::Unsat`] (with the
//! original deps) straight up without trying them. This is the
//! dependency-directed back-jumping that the chronological version
//! couldn't do.
//!
//! When all disjuncts *did* clash with this branch's id in their
//! deps, we conclude that the disjunction itself is unsat under the
//! ancestor branches' deps — return `Unsat(combined ∖ {my_id})`
//! where combined unions each child's clash deps.

use crate::TableauContext;
use crate::graph::{DepSet, NodeId};
use crate::saturate::{SaturationResult, saturate};
use owl_dl_core::{ConceptExpr, ConceptId, ConceptPool};

/// Hard cap on the saturation fixed-point loop within each
/// deterministic phase. Phase 2 pre-blocking has no real risk of
/// unbounded growth (labels are sub-expressions of the input,
/// bounded by [`owl_dl_core::ConceptPool`] size), so this is purely
/// defensive against rule bugs.
const SATURATE_ITERS: usize = 4096;

/// Outcome of one call to [`search`] or [`branch`].
///
/// Generalises the previous `Option<bool>` API: `Sat` is what
/// callers want for a model existence check; `Unsat` carries the
/// `DepSet` so [`branch`] can decide whether the failure depends on
/// its own decision; `DepthLimit` covers both the recursion cap and
/// the cooperative deadline (callers disambiguate via
/// [`TableauContext::deadline_reached`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SearchVerdict {
    /// A clash-free saturated completion exists — concept is
    /// satisfiable along the current branch.
    Sat,
    /// Every continuation clashed. The [`DepSet`] is the union of
    /// every clash's deps minus any branch decisions made *inside*
    /// this subtree — what remains is the set of ancestor branches
    /// the failure depends on. Empty `DepSet` ⇒ the failure is
    /// independent of any branch (unsat under the root context).
    Unsat(DepSet),
    /// Either the recursion depth cap was reached or the cooperative
    /// deadline elapsed. Callers distinguish via
    /// [`TableauContext::deadline_reached`].
    DepthLimit,
}

impl SearchVerdict {
    /// Bridge to the legacy `Option<bool>` shape that
    /// [`TableauContext::is_satisfiable`] still hands to its callers.
    #[must_use]
    pub fn to_option(&self) -> Option<bool> {
        match self {
            Self::Sat => Some(true),
            Self::Unsat(_) => Some(false),
            Self::DepthLimit => None,
        }
    }
}

/// Drive deterministic saturation interleaved with `⊔` branching.
pub fn search(ctx: &mut TableauContext<'_, '_, '_>, max_depth: usize) -> SearchVerdict {
    if max_depth == 0 || ctx.check_deadline() {
        return SearchVerdict::DepthLimit;
    }
    match saturate(ctx, SATURATE_ITERS) {
        SaturationResult::Clash(_, deps) => SearchVerdict::Unsat(deps),
        SaturationResult::Stalled => SearchVerdict::DepthLimit,
        SaturationResult::Stable => {
            // Step 1: ⊔ branching has priority — it's structurally
            // cheaper and keeps the search shape predictable.
            if let Some((node, _or_label, disjuncts, or_deps)) = first_open_disjunction(ctx) {
                return branch(ctx, max_depth, node, &disjuncts, &or_deps);
            }
            // Step 2: choose rule for `≤n R.C` — pick a neighbour
            // that doesn't yet have `C` or `¬C` and branch.
            if let Some((node, c, c_neg)) = first_open_choose(ctx) {
                return branch(ctx, max_depth, node, &[c, c_neg], &DepSet::new());
            }
            SearchVerdict::Sat
        }
    }
}

fn branch(
    ctx: &mut TableauContext<'_, '_, '_>,
    max_depth: usize,
    node: NodeId,
    options: &[ConceptId],
    parent_deps: &[u32],
) -> SearchVerdict {
    let my_id = ctx.push_branch();
    let mut combined: DepSet = DepSet::new();
    let mut depth_limited = false;
    let mut early_return: Option<SearchVerdict> = None;
    // Restricted semantic branching companion. When option `d_j`
    // failed and `¬d_j` is registered as a cheap literal complement,
    // assert `¬d_j` in every subsequent branch so any rule that
    // tries to re-derive `d_j` clashes immediately. Compound
    // complements (Or, quantified) are *not* carried forward — they
    // would inflate the label set without back-jumping enough subtree
    // to pay for themselves (see `docs/phase4-backjumping-plan.md`).
    let mut literal_complements: Vec<ConceptId> = Vec::new();

    // Reorder disjuncts: try first those that don't *obviously* clash
    // with an existing label at `node`. A disjunct is "obvious clash"
    // when asserting it produces a contradictory `(C, ¬C)` pair with
    // a label already present. Doing the cheap-sat branch first cuts
    // the search tree on workloads with absorbed disjunctions where
    // one branch is structurally satisfiable and the other generates
    // expensive downstream work — notably the Country / nominal
    // pattern on pizza, where the `(¬{a} ⊓ … ⊓ ¬{e})` disjunct is a
    // direct sat while the `:Country` disjunct fans out into nominal
    // assignment and merging.
    let ordered = reorder_disjuncts(ctx, node, options);

    for d in &ordered {
        if early_return.is_some() {
            break;
        }
        // (CDBL lookup intentionally not wired here — see the
        // `learned_nogoods` doc on [`crate::TableauContext`] and
        // `docs/perf-2026-05-24-new-server.md` §5. The naive
        // "precond ⊆ active ⇒ skip" rule is unsound on pizza —
        // verdict went from 2 unsat to 0 unsat — because the
        // preconditions don't fully capture *which* node labels
        // produced the clash; in particular, two no-goods recorded
        // in different sub-trees can fire jointly at a node that's
        // actually sat. A correct implementation needs to key
        // no-goods on a richer fingerprint than just `(node,
        // or_label, disjunct, precond)` — the smallest unsat-
        // explaining label sub-set is the principled choice but
        // requires deps on labels-as-evidence the current trail
        // doesn't track.)
        let cp = ctx.checkpoint();
        // Each disjunct carries: (a) the parent disjunction's deps —
        // without them an inner clash returns `clash_deps` missing
        // the outer branch's id and back-jumping skips past it, and
        // (b) this branch's `my_id` so the inner search can attribute
        // any clash to this specific disjunct choice.
        let combined_deps: DepSet = {
            let mut d = DepSet::from_slice(parent_deps);
            if d.binary_search(&my_id).is_err() {
                let pos = d.binary_search(&my_id).unwrap_or_else(|p| p);
                d.insert(pos, my_id);
            }
            d
        };
        // Assert prior failed disjuncts' literal complements.
        for &comp in &literal_complements {
            ctx.add_label_with_deps(node, comp, combined_deps.as_slice());
        }
        // The labelled disjunct depends on *this* branch decision and
        // every reason the parent disjunction was at this node.
        ctx.add_label_with_deps(node, *d, combined_deps.as_slice());
        match search(ctx, max_depth - 1) {
            SearchVerdict::Sat => {
                // Found a model; keep state, exit early. State is
                // left as-is — the model labels are real.
                early_return = Some(SearchVerdict::Sat);
            }
            SearchVerdict::Unsat(clash_deps) => {
                ctx.rollback_to(cp);
                if clash_deps.binary_search(&my_id).is_err() {
                    // Back-jump: this branch decision didn't
                    // contribute to the clash. Every sibling disjunct
                    // would clash for the same upstream reason —
                    // propagate the failure straight up.
                    early_return = Some(SearchVerdict::Unsat(clash_deps));
                } else {
                    // This decision mattered. Accumulate the rest of
                    // the deps for the "all options exhausted" case.
                    for &x in &clash_deps {
                        if x != my_id
                            && let Err(pos) = combined.binary_search(&x)
                        {
                            combined.insert(pos, x);
                        }
                    }
                    // (Recording side of conflict-driven learning
                    // is wired but the lookup is unsound, so the
                    // recording would be free-allocated garbage. See
                    // the corresponding comment on the lookup side.)
                    // Carry forward the failed disjunct's literal
                    // complement (if it has one registered) so the
                    // next iteration short-circuits any rebirth of
                    // `d` in the model.
                    if let Some(comp) = ctx.complement_of(*d)
                        && is_literal(ctx, comp)
                    {
                        literal_complements.push(comp);
                    }
                }
            }
            SearchVerdict::DepthLimit => {
                ctx.rollback_to(cp);
                depth_limited = true;
            }
        }
    }
    ctx.pop_branch();

    if let Some(v) = early_return {
        v
    } else if depth_limited {
        SearchVerdict::DepthLimit
    } else {
        // Every option clashed and every clash depended on `my_id`.
        // The disjunction itself is therefore unsat under the union
        // of ancestor deps in `combined`.
        SearchVerdict::Unsat(combined)
    }
}

/// Reorder the disjuncts of an open `Or` to try the *cheapest* one
/// first — the branch most likely to satisfy with the least
/// downstream work. The score (lower is better) classifies each
/// disjunct by how much rule activity its assertion is expected to
/// trigger:
///
/// - `0` — leaf-class: a `Not(_)` or an `And` whose conjuncts are
///   all leaf-class. Adding them just inserts inert labels (no
///   concept-rule trigger, no existential to expand, no merge).
///   The pizza Country reverse-equiv disjunction has one such
///   conjunction — `(¬{a} ⊓ ¬{b} ⊓ … ⊓ ¬{e})` — and trying it first
///   discovers the SAT model immediately instead of exploring the
///   `:Country → :DC ⊓ OneOf(…)` cascade of the sibling disjunct.
/// - `1` — atomic that *doesn't* obviously clash. Triggers concept-
///   rules but is otherwise simple. Most pizza-shaped disjunctions.
/// - `2` — compound (`Some`/`Min`/`Max`/etc.) likely to generate
///   nodes or fire merges. Most expensive in practice.
/// - `3` — obvious immediate clash: the disjunct's complement is
///   already labelled. Try last; the branch will UNSAT quickly via
///   the trivial label-pair clash.
///
/// A "leaf" disjunct decomposes only into `Not(_)` labels — no
/// atomic-class triggers, no existentials, no merging. Used by the
/// score-0 case in `reorder_disjuncts`.
fn is_leaf_compound(pool: &ConceptPool, c: ConceptId) -> bool {
    match pool.get(c) {
        ConceptExpr::Not(_) => true,
        ConceptExpr::And(args) => args.iter().all(|&a| is_leaf_compound(pool, a)),
        _ => false,
    }
}

/// Stable secondary key on original index keeps the
/// literal-complements optimisation downstream deterministic.
fn reorder_disjuncts(
    ctx: &TableauContext<'_, '_, '_>,
    node: NodeId,
    options: &[ConceptId],
) -> Vec<ConceptId> {
    let pool = ctx.pool();
    let labels = ctx.graph().node(node).labels();

    let score = |d: ConceptId| -> u8 {
        // 3: would clash immediately.
        match pool.get(d) {
            ConceptExpr::Atomic(_) | ConceptExpr::Nominal(_) => {
                if let Some(neg) = ctx.complement_of(d)
                    && labels.binary_search(&neg).is_ok()
                {
                    return 3;
                }
            }
            ConceptExpr::Not(inner) if labels.binary_search(inner).is_ok() => {
                return 3;
            }
            _ => {}
        }
        if is_leaf_compound(pool, d) {
            return 0;
        }
        match pool.get(d) {
            ConceptExpr::Atomic(_) | ConceptExpr::Nominal(_) => 1,
            _ => 2,
        }
    };

    let mut indexed: Vec<(u8, usize, ConceptId)> = options
        .iter()
        .enumerate()
        .map(|(i, &d)| (score(d), i, d))
        .collect();
    indexed.sort_by_key(|&(s, i, _)| (s, i));
    indexed.into_iter().map(|(_, _, d)| d).collect()
}

/// True iff `c` is a cheap literal — atomic, nominal,
/// self-restriction, or `Not(_)` of one. Used by `branch()` to
/// decide whether to carry a disjunct's complement forward in
/// restricted semantic branching.
fn is_literal(ctx: &TableauContext<'_, '_, '_>, c: ConceptId) -> bool {
    matches!(
        ctx.pool().get(c),
        ConceptExpr::Atomic(_)
            | ConceptExpr::Nominal(_)
            | ConceptExpr::SelfRestriction(_)
            | ConceptExpr::Not(_)
    )
}

/// Find the first `Max(n, R, C)` label whose R-neighbour at the
/// owning node is unlabelled for both `C` and `¬C`. Returns
/// `(neighbour, C, ¬C)` — the two labels the search will branch on.
fn first_open_choose(ctx: &TableauContext<'_, '_, '_>) -> Option<(NodeId, ConceptId, ConceptId)> {
    let pool = ctx.pool();
    let graph = ctx.graph();
    for idx in 0..graph.len() {
        let node_id = NodeId::new(u32::try_from(idx).expect("node count exceeds u32"));
        for &c in graph.node(node_id).labels() {
            let ConceptExpr::Max(_, role, body) = pool.get(c) else {
                continue;
            };
            let Some(complement) = ctx.complement_of(*body) else {
                // No complement registered — the reasoner facade
                // should have set this for every Max body. Skip
                // rather than panic; a missing complement results
                // in incompleteness, not unsoundness.
                continue;
            };
            for (seen, neighbour) in graph.node(node_id).neighbours() {
                if !ctx.edge_satisfies(seen, *role) {
                    continue;
                }
                let nlabels = graph.node(neighbour).labels();
                let has_body = nlabels.binary_search(body).is_ok();
                let has_comp = nlabels.binary_search(&complement).is_ok();
                if !has_body && !has_comp {
                    return Some((neighbour, *body, complement));
                }
            }
        }
    }
    None
}

/// Find the first `Or` label in any node such that none of its
/// disjuncts is already in that node's label set.
///
/// "First" is well-defined: nodes are visited in arena order, labels
/// in sorted order. Stable choice keeps the search deterministic for
/// reproducible tests; smarter heuristics arrive in Phase 4.
fn first_open_disjunction(
    ctx: &TableauContext<'_, '_, '_>,
) -> Option<(NodeId, ConceptId, Vec<ConceptId>, DepSet)> {
    let pool = ctx.pool();
    let graph = ctx.graph();
    for idx in 0..graph.len() {
        let node_id = NodeId::new(u32::try_from(idx).expect("node count exceeds u32"));
        let node = graph.node(node_id);
        let labels = node.labels();
        for (pos, &c) in labels.iter().enumerate() {
            if let ConceptExpr::Or(args) = pool.get(c)
                && !args.iter().any(|d| labels.binary_search(d).is_ok())
            {
                // Return the parent Or's label id (for conflict-
                // driven learning keyed by `(node, or_label,
                // disjunct)`) and its `DepSet` so the search can
                // attach the parent's deps to each disjunct it
                // asserts. Without the deps, a clash deep inside a
                // chosen disjunct returns `clash_deps` missing the
                // dependency on "this disjunction was at this node
                // in the first place" and back-jumping skips past
                // it — the soundness gap chased on pizza (2026-05-25).
                let or_deps = node.label_deps[pos].clone();
                return Some((node_id, c, args.to_vec(), or_deps));
            }
        }
    }
    None
}
