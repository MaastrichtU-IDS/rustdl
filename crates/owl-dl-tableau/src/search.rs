//! Backtracking driver for the non-deterministic `⊔` rule.
//!
//! The deterministic rules in [`crate::rules`] cannot handle a label
//! of shape `Or([d1, …, dn])` — they would have to *choose* which
//! disjunct to add. This module implements the choice via depth-first
//! search with trail-based undo:
//!
//! 1. Run deterministic saturation. If it clashes, the branch is dead.
//! 2. Otherwise, find any `Or` label in any node whose disjuncts are
//!    not yet present in that node's labels — an *open disjunction*.
//! 3. For each disjunct `di`:
//!    - take a [`crate::Checkpoint`];
//!    - add `di` to the node;
//!    - recurse;
//!    - on a `false` return, [`crate::TableauContext::rollback_to`]
//!      and try the next disjunct.
//! 4. If every disjunct fails, the branch is unsatisfiable.
//! 5. If there are no open disjunctions and no clash, the branch is
//!    satisfiable.
//!
//! ## Why open-disjunction detection is non-trivial
//!
//! A naive `for label in L(x): if Or(_) then branch` would loop
//! forever: after we add `d1` to satisfy the Or, the Or is still in
//! `L(x)`. We have to check that *no* disjunct is already present
//! before branching. This makes the rule re-entrant under
//! deterministic saturation: a later `⊓` may add `d1` as a side
//! effect, closing the disjunction without an explicit choice.

use crate::TableauContext;
use crate::graph::NodeId;
use crate::saturate::{SaturationResult, saturate};
use owl_dl_core::{ConceptExpr, ConceptId};

/// Hard cap on the saturation fixed-point loop within each
/// deterministic phase. Phase 2 pre-blocking has no real risk of
/// unbounded growth (labels are sub-expressions of the input,
/// bounded by [`owl_dl_core::ConceptPool`] size), so this is purely
/// defensive against rule bugs.
const SATURATE_ITERS: usize = 4096;

/// Drive deterministic saturation interleaved with `⊔` branching.
///
/// Returns:
/// - `Some(true)` if some branch reaches a saturated, clash-free
///   completion graph with no open disjunctions;
/// - `Some(false)` if every branch clashes;
/// - `None` if the recursion depth cap is hit (defensive — should
///   never happen for well-formed input until subset blocking is
///   added in commit 6 and the search becomes potentially
///   non-terminating without it).
pub fn search(ctx: &mut TableauContext<'_, '_>, max_depth: usize) -> Option<bool> {
    if max_depth == 0 {
        return None;
    }
    match saturate(ctx, SATURATE_ITERS) {
        SaturationResult::Clash(_) => Some(false),
        SaturationResult::Stalled => None,
        SaturationResult::Stable => {
            let Some((node, disjuncts)) = first_open_disjunction(ctx) else {
                return Some(true);
            };
            let mut depth_limited = false;
            for d in disjuncts {
                let cp = ctx.checkpoint();
                ctx.add_label(node, d);
                match search(ctx, max_depth - 1) {
                    Some(true) => return Some(true),
                    Some(false) => {
                        ctx.rollback_to(cp);
                    }
                    None => {
                        ctx.rollback_to(cp);
                        depth_limited = true;
                    }
                }
            }
            if depth_limited { None } else { Some(false) }
        }
    }
}

/// Find the first `Or` label in any node such that none of its
/// disjuncts is already in that node's label set.
///
/// "First" is well-defined: nodes are visited in arena order, labels
/// in sorted order. Stable choice keeps the search deterministic for
/// reproducible tests; smarter heuristics arrive in Phase 4.
fn first_open_disjunction(ctx: &TableauContext<'_, '_>) -> Option<(NodeId, Vec<ConceptId>)> {
    let pool = ctx.pool();
    let graph = ctx.graph();
    for idx in 0..graph.len() {
        let node = NodeId::new(u32::try_from(idx).expect("node count exceeds u32"));
        let labels = graph.node(node).labels();
        for &c in labels {
            if let ConceptExpr::Or(args) = pool.get(c)
                && !args.iter().any(|d| labels.binary_search(d).is_ok())
            {
                return Some((node, args.to_vec()));
            }
        }
    }
    None
}
