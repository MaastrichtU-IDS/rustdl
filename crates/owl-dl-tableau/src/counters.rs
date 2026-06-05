//! Per-rule call counters for the tableau engine.
//!
//! Behind the `counters` Cargo feature. Each `apply_*` rule and key
//! mutator (`add_label_with_deps`, `add_edge_with_deps`,
//! `is_blocked`) bumps its slot; on `TableauContext::drop` the totals
//! are dumped to stderr when the `RUSTDL_COUNTERS=1` environment
//! variable is set.
//!
//! Intended use: identify the call-count histogram of a workload that
//! does not finish (e.g. `pizza.ofn`). Pairs with the pprof
//! flamegraphs in `docs/flamegraphs/` — the flamegraph names the hot
//! frames, the counters say how often they fire.
//!
//! Off-feature builds carry zero overhead — the field is omitted
//! from `TableauContext` and every `bump_*` call site is a no-op.

use std::cell::Cell;

/// Lightweight per-context counter bag. All counters are `Cell<u64>`
/// to allow bumping from `&self` callers (`is_blocked`).
#[derive(Debug, Default)]
pub(crate) struct RuleCounters {
    pub(crate) apply_and: Cell<u64>,
    pub(crate) apply_forall: Cell<u64>,
    pub(crate) apply_concept_rules: Cell<u64>,
    pub(crate) apply_nominal_rules: Cell<u64>,
    pub(crate) apply_role_rules: Cell<u64>,
    pub(crate) apply_residual_gcis: Cell<u64>,
    pub(crate) apply_exists: Cell<u64>,
    pub(crate) apply_min: Cell<u64>,
    pub(crate) apply_max: Cell<u64>,
    pub(crate) apply_nominal_assignment: Cell<u64>,
    pub(crate) apply_role_chains: Cell<u64>,
    pub(crate) apply_role_chains_body_iters: Cell<u64>,
    pub(crate) apply_self_restriction: Cell<u64>,
    pub(crate) apply_role_axioms: Cell<u64>,

    pub(crate) add_label_calls: Cell<u64>,
    pub(crate) add_label_inserted: Cell<u64>,
    pub(crate) add_edge_calls: Cell<u64>,

    pub(crate) is_blocked_calls: Cell<u64>,
    pub(crate) is_blocked_true: Cell<u64>,
    pub(crate) is_blocked_prefilter_rejects: Cell<u64>,
    pub(crate) is_blocked_subset_scans: Cell<u64>,

    /// Phase 3: needs_deferred_or's bloom prefilter rejected the call
    /// (the deferred OR's concept or any of its disjuncts couldn't be
    /// in the label set per the bloom). Each reject skips the
    /// binary_search + per-disjunct iteration. See
    /// `docs/phase3-fix-target.md`.
    pub(crate) needs_deferred_or_bloom_rejects: Cell<u64>,

    /// Phase 3b: each call to `are_declared_inverses` that consulted
    /// the O(1) `hashbrown::HashSet` (i.e. `inverse_pairs_set` was
    /// non-empty). Bumped regardless of whether the pair was found, so
    /// it counts "fast-path consultations." Used by the Phase 3b
    /// structural canary to confirm the new lookup path is actually
    /// wired in. See `docs/phase3b-fix-target.md`.
    pub(crate) inverse_pair_fast_hits: Cell<u64>,

    /// Phase 3d: each time `apply_deferred_concept_or_rules`'s indexed
    /// branch encounters a trigger with no entry in
    /// `concept_rules_by_trigger` and skips with `continue` (instead of
    /// the legacy per-trigger linear scan over `&tbox.concept_rules`).
    /// Each bump represents an O(R) scan saved on a finalized `TBox`.
    /// See `docs/phase3d-fix-target.md`.
    pub(crate) apply_deferred_concept_or_skip_missing_trigger: Cell<u64>,
}

/// Increment by 1. Internal helper; macro callers go through the
/// `bump!` macro instead of touching this directly.
#[inline]
pub(crate) fn inc(c: &Cell<u64>) {
    c.set(c.get().wrapping_add(1));
}

/// Add `n` (used for batch counters like body-loop iterations).
/// Kept available for callers that want to count by something other
/// than 1; currently no in-tree callsite. Allow `dead_code` so adding
/// counters elsewhere doesn't require a separate cleanup pass.
#[allow(dead_code)]
#[inline]
pub(crate) fn add(c: &Cell<u64>, n: u64) {
    c.set(c.get().wrapping_add(n));
}

impl RuleCounters {
    /// Dump non-zero counters to stderr. Called from
    /// `TableauContext::drop` when `RUSTDL_COUNTERS=1`. Format:
    /// one counter per line, `name=value`. Each tableau context dumps
    /// independently — parallel classify will interleave multiple
    /// blocks, one per worker thread. Use `>2 counters.log` and grep
    /// in the analysis script.
    pub(crate) fn dump(&self, label: &str) {
        let entries: &[(&str, u64)] = &[
            ("apply_and", self.apply_and.get()),
            ("apply_forall", self.apply_forall.get()),
            ("apply_concept_rules", self.apply_concept_rules.get()),
            ("apply_nominal_rules", self.apply_nominal_rules.get()),
            ("apply_role_rules", self.apply_role_rules.get()),
            ("apply_residual_gcis", self.apply_residual_gcis.get()),
            ("apply_exists", self.apply_exists.get()),
            ("apply_min", self.apply_min.get()),
            ("apply_max", self.apply_max.get()),
            (
                "apply_nominal_assignment",
                self.apply_nominal_assignment.get(),
            ),
            ("apply_role_chains", self.apply_role_chains.get()),
            (
                "apply_role_chains_body_iters",
                self.apply_role_chains_body_iters.get(),
            ),
            ("apply_self_restriction", self.apply_self_restriction.get()),
            ("apply_role_axioms", self.apply_role_axioms.get()),
            ("add_label_calls", self.add_label_calls.get()),
            ("add_label_inserted", self.add_label_inserted.get()),
            ("add_edge_calls", self.add_edge_calls.get()),
            ("is_blocked_calls", self.is_blocked_calls.get()),
            ("is_blocked_true", self.is_blocked_true.get()),
            (
                "is_blocked_prefilter_rejects",
                self.is_blocked_prefilter_rejects.get(),
            ),
            (
                "is_blocked_subset_scans",
                self.is_blocked_subset_scans.get(),
            ),
            (
                "needs_deferred_or_bloom_rejects",
                self.needs_deferred_or_bloom_rejects.get(),
            ),
            ("inverse_pair_fast_hits", self.inverse_pair_fast_hits.get()),
            (
                "apply_deferred_concept_or_skip_missing_trigger",
                self.apply_deferred_concept_or_skip_missing_trigger.get(),
            ),
        ];
        let total: u64 = entries.iter().map(|(_, v)| *v).sum();
        if total == 0 {
            return;
        }
        eprintln!("# rustdl counters ({label}):");
        for (name, value) in entries {
            if *value > 0 {
                eprintln!("  {name}={value}");
            }
        }
    }
}
