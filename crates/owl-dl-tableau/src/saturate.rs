//! Worklist-driven saturation driver.
//!
//! Each saturator pass processes only nodes whose `dirty` bit is set —
//! see [`crate::graph::CompletionGraph::dirty`]. The bit is raised by
//! every mutation that could enable a rule to fire (label add, edge
//! add or remove, merge target side, ...) and cleared by this loop
//! before the per-node rule block runs. Rules that re-mutate the node
//! re-raise the bit, so intra-pass convergence is automatic.
//!
//! Why bother: the old "every iter, every node, every rule" loop was
//! the structural perf bottleneck — counters on pizza showed 99 % of
//! rule invocations were no-ops, and the new deps-tracking work
//! (2026-05-25) made each no-op call more expensive (more
//! [`DepSet`] cloning and unioning). The dirty bit caps total work
//! at O(deltas) per saturate() call instead of O(passes × nodes ×
//! rules). See `docs/perf-2026-05-24-new-server.md` §5.
//!
//! ## Rule coverage
//!
//! All deterministic ALC rules are wired into the sweep: `⊓`, `∀`,
//! the four absorbed-TBox families (`ConceptRule`, `NominalRule`,
//! `RoleRule`, residual GCI), and the generative `∃` rule with
//! pair blocking. The non-deterministic `⊔` rule sits one level up
//! in [`crate::search`] since it requires a backtracking driver.

use crate::TableauContext;
use crate::graph::{DepSet, NodeId};
use crate::rules::{
    RuleOutcome, apply_and, apply_concept_rules, apply_exists, apply_forall, apply_max, apply_min,
    apply_nominal_assignment, apply_nominal_rules, apply_residual_gcis, apply_role_axioms,
    apply_role_chains, apply_role_rules, apply_self_restriction,
};

/// Verdict from one run of [`saturate`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SaturationResult {
    /// A clash was found at some node, with the [`DepSet`] of the
    /// offending complementary labels. The expansion is closed —
    /// for a satisfiability check this means *unsatisfiable* along
    /// the current branch. The deps tell `search::branch` which
    /// branch decisions the clash actually depended on, enabling
    /// dependency-directed back-jumping.
    Clash(NodeId, DepSet),
    /// No rule had anything left to add and no node clashes. For
    /// the full ALC ruleset this would mean *satisfiable*. With only
    /// the `⊓` rule wired in, it just means "stable under conjunction
    /// decomposition".
    Stable,
    /// Reached the iteration cap without saturating or clashing.
    /// Used as a defensive guard while the ruleset is incomplete.
    Stalled,
}

/// Worklist-driven saturation loop.
///
/// On entry, every node is conservatively marked dirty (the search
/// added or rolled back labels between this call and the previous
/// one, so some — typically a small subset — of the rules will
/// re-fire). Each outer iteration scans nodes once and runs the
/// rule block only on dirty nodes, clearing the bit before doing so;
/// mutations performed by rules re-raise the bit on affected nodes.
/// The loop exits when a full scan finds nothing dirty (stable) or
/// a clash surfaces, with `max_iters` as a defensive cap.
pub fn saturate(ctx: &mut TableauContext<'_, '_, '_>, max_iters: usize) -> SaturationResult {
    // Conservatively mark everything dirty: between saturate() calls
    // the search has added or rolled back labels, and some state
    // (e.g. `residuals_saturated` memo, nominal-map shape, edge
    // counts that feed apply_max's threshold check) isn't fully
    // covered by the per-mutation dirty hooks alone. A persistent-
    // dirty variant was tried 2026-05-25 and broke three merge
    // fixtures (48/65/66 — functional and inverse-functional role
    // merges) by failing to re-fire rules on nodes affected
    // *indirectly* by a merge. The fine-grained worklist inside this
    // saturate() call still saves the bulk of the work; the entry
    // reset is a single boolean write per node.
    ctx.graph_mut().mark_all_dirty();
    for _ in 0..max_iters {
        // Cooperative deadline check. A single saturate() call can
        // generate many nodes (e.g. via chain rule expansion under
        // inverse roles) and would otherwise run far past a
        // caller-imposed wall-clock budget. Returning `Stalled` lets
        // search.rs propagate `None` up to the reasoner facade.
        if ctx.check_deadline() {
            return SaturationResult::Stalled;
        }
        if let Some((node, deps)) = first_clash(ctx) {
            return SaturationResult::Clash(node, deps);
        }
        let mut changed = false;
        let node_count = ctx.graph().len();
        for idx in 0..node_count {
            if ctx.check_deadline() {
                return SaturationResult::Stalled;
            }
            let node = NodeId::new(u32::try_from(idx).expect("node count exceeds u32"));
            if !ctx.graph().is_dirty(node) {
                continue;
            }
            // Clear *before* running rules so that a rule mutating
            // this node (e.g. apply_and adding decomposed children to
            // the same node) re-raises the bit and we revisit on the
            // next outer iteration.
            ctx.graph_mut().set_dirty(node, false);
            // Inter-rule deadline checks: one apply_* call can do
            // enough work that batching the check to once-per-node
            // wouldn't yield within the caller's budget. The check is
            // a cheap Instant comparison, dwarfed by rule bodies.
            macro_rules! step {
                ($apply:expr) => {{
                    if ctx.check_deadline() {
                        return SaturationResult::Stalled;
                    }
                    if $apply == RuleOutcome::Applied {
                        changed = true;
                    }
                }};
            }
            step!(apply_residual_gcis(ctx, node));
            step!(apply_and(ctx, node));
            step!(apply_concept_rules(ctx, node));
            step!(apply_nominal_rules(ctx, node));
            step!(apply_forall(ctx, node));
            step!(apply_role_rules(ctx, node));
            step!(apply_role_chains(ctx, node));
            step!(apply_self_restriction(ctx, node));
            step!(apply_role_axioms(ctx, node));
            step!(apply_exists(ctx, node));
            step!(apply_min(ctx, node));
            if apply_max(ctx, node) == RuleOutcome::Applied {
                changed = true;
            }
            if apply_nominal_assignment(ctx, node) == RuleOutcome::Applied {
                changed = true;
            }
        }
        if !changed {
            return if let Some((node, deps)) = first_clash(ctx) {
                SaturationResult::Clash(node, deps)
            } else {
                SaturationResult::Stable
            };
        }
    }
    SaturationResult::Stalled
}

fn first_clash(ctx: &TableauContext<'_, '_, '_>) -> Option<(NodeId, DepSet)> {
    for idx in 0..ctx.graph().len() {
        let node = NodeId::new(u32::try_from(idx).expect("node count exceeds u32"));
        if let Some(deps) = ctx.clash_deps_at(node) {
            return Some((node, deps));
        }
    }
    None
}
