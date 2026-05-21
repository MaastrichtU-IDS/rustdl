//! Naive saturation driver.
//!
//! Repeatedly visits every node and applies every available rule
//! until either a clash is detected or no rule adds anything new
//! (saturation). This is the deliberately-unoptimized version
//! described in strategy v2 §4 Phase 2; the optimization stack
//! (priority queues, dependency-directed backtracking, lazy unfolding
//! integration) arrives in Phase 4.
//!
//! ## Phase 2 commit 6 scope
//!
//! All deterministic ALC rules are wired into the sweep: `⊓`, `∀`,
//! the four absorbed-TBox families (`ConceptRule`, `NominalRule`,
//! `RoleRule`, residual GCI), and the generative `∃` rule with
//! naive subset blocking. The non-deterministic `⊔` rule sits one
//! level up in [`crate::search`] since it requires a backtracking
//! driver.

use crate::TableauContext;
use crate::graph::NodeId;
use crate::rules::{
    RuleOutcome, apply_and, apply_concept_rules, apply_exists, apply_forall, apply_max, apply_min,
    apply_nominal_assignment, apply_nominal_rules, apply_residual_gcis, apply_role_rules,
};

/// Verdict from one run of [`saturate`].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SaturationResult {
    /// A clash was found at some node. The expansion is closed —
    /// for a satisfiability check this means *unsatisfiable* along
    /// the current branch.
    Clash(NodeId),
    /// No rule had anything left to add and no node clashes. For
    /// the full ALC ruleset this would mean *satisfiable*. With only
    /// the `⊓` rule wired in, it just means "stable under conjunction
    /// decomposition".
    Stable,
    /// Reached the iteration cap without saturating or clashing.
    /// Used as a defensive guard while the ruleset is incomplete.
    Stalled,
}

/// Naive saturation loop.
///
/// `max_iters` caps the outer fixed-point loop so a buggy rule cannot
/// run unbounded. Each iteration sweeps every existing node and
/// applies each available rule. Stops as soon as a clash is found.
pub fn saturate(ctx: &mut TableauContext<'_, '_, '_>, max_iters: usize) -> SaturationResult {
    for _ in 0..max_iters {
        if let Some(node) = first_clash(ctx) {
            return SaturationResult::Clash(node);
        }
        let mut changed = false;
        let node_count = ctx.graph().len();
        for idx in 0..node_count {
            let node = NodeId::new(u32::try_from(idx).expect("node count exceeds u32"));
            if apply_residual_gcis(ctx, node) == RuleOutcome::Applied {
                changed = true;
            }
            if apply_and(ctx, node) == RuleOutcome::Applied {
                changed = true;
            }
            if apply_concept_rules(ctx, node) == RuleOutcome::Applied {
                changed = true;
            }
            if apply_nominal_rules(ctx, node) == RuleOutcome::Applied {
                changed = true;
            }
            if apply_forall(ctx, node) == RuleOutcome::Applied {
                changed = true;
            }
            if apply_role_rules(ctx, node) == RuleOutcome::Applied {
                changed = true;
            }
            if apply_exists(ctx, node) == RuleOutcome::Applied {
                changed = true;
            }
            if apply_min(ctx, node) == RuleOutcome::Applied {
                changed = true;
            }
            if apply_max(ctx, node) == RuleOutcome::Applied {
                changed = true;
            }
            if apply_nominal_assignment(ctx, node) == RuleOutcome::Applied {
                changed = true;
            }
        }
        if !changed {
            return if let Some(node) = first_clash(ctx) {
                SaturationResult::Clash(node)
            } else {
                SaturationResult::Stable
            };
        }
    }
    SaturationResult::Stalled
}

fn first_clash(ctx: &TableauContext<'_, '_, '_>) -> Option<NodeId> {
    for idx in 0..ctx.graph().len() {
        let node = NodeId::new(u32::try_from(idx).expect("node count exceeds u32"));
        if ctx.clash_in(node) {
            return Some(node);
        }
    }
    None
}
