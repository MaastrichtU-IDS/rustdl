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
//! search detects a clash, [`crate::saturate`] returns the
//! [`crate::SaturationResult::Clash`] variant carrying the
//! [`crate::DepSet`] of the offending complementary labels. Each
//! rule propagates this `DepSet` to its conclusions during saturation
//! (see [`crate::deps`] + the per-rule plumbing in [`crate::rules`]).
//!
//! A search frame reads the clash deps. If its own `branch_id` is *not*
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
use crate::trail::Checkpoint;
use owl_dl_core::{ConceptExpr, ConceptId, ConceptPool};

/// Hard cap on the saturation fixed-point loop within each
/// deterministic phase. Phase 2 pre-blocking has no real risk of
/// unbounded growth (labels are sub-expressions of the input,
/// bounded by [`owl_dl_core::ConceptPool`] size), so this is purely
/// defensive against rule bugs.
const SATURATE_ITERS: usize = 4096;

/// Outcome of a search, or of one decision frame within it.
///
/// Generalises the previous `Option<bool>` API: `Sat` is what
/// callers want for a model existence check; `Unsat` carries the
/// `DepSet` so a search frame can decide whether the failure depends on
/// its own decision; `DepthLimit` covers both the frame-depth cap and
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
    /// Either the frame-depth cap was reached or the cooperative
    /// deadline elapsed. Callers distinguish via
    /// [`TableauContext::deadline_reached`].
    DepthLimit,
    /// The deterministic live-node cap ([`crate::max_nodes`]) was hit.
    /// Distinct from `DepthLimit`: on the deadline-free path a `DepthLimit`
    /// verdict maps to `Err(NoVerdict)`, but a cap trip must degrade to a
    /// sound `Ok(None)` MISS instead (#35 v4 safety net). Never fold this
    /// into `DepthLimit` handling.
    NodeCap,
}

impl SearchVerdict {
    /// Bridge to the legacy `Option<bool>` shape that
    /// [`TableauContext::is_satisfiable`] still hands to its callers.
    #[must_use]
    pub fn to_option(&self) -> Option<bool> {
        match self {
            Self::Sat => Some(true),
            Self::Unsat(_) => Some(false),
            // Both a depth/deadline stall and a live-node cap trip are
            // "don't know" in this legacy bridge — callers that need to
            // tell them apart use the richer `SearchVerdict` directly.
            Self::DepthLimit | Self::NodeCap => None,
        }
    }
}

/// Single-shot env check: `RUSTDL_TRACE=1` enables one-line stderr
/// dumps in `search` and `branch`. Off path is a single atomic load.
fn trace_enabled() -> bool {
    use std::sync::atomic::{AtomicU8, Ordering};
    static STATE: AtomicU8 = AtomicU8::new(0); // 0=unknown, 1=off, 2=on
    let s = STATE.load(Ordering::Relaxed);
    if s == 0 {
        let on = std::env::var("RUSTDL_TRACE").as_deref() == Ok("1");
        STATE.store(if on { 2 } else { 1 }, Ordering::Relaxed);
        on
    } else {
        s == 2
    }
}

/// Drive deterministic saturation interleaved with `⊔` branching.
///
/// A depth-first walk over decisions (#288 step 3b): an explicit stack of
/// [`Frame`]s, each with the option it is trying and that option's checkpoint,
/// stands in for recursion. Every side effect happens in the order the
/// recursive version had.
pub fn search(ctx: &mut TableauContext<'_, '_, '_>, max_depth: usize) -> SearchVerdict {
    // Each open frame, with the option it is trying and that option's checkpoint.
    let mut stack: Vec<(Frame, usize, Checkpoint)> = Vec::new();
    let mut next = expand(ctx, max_depth);
    loop {
        // Run frames until one needs a child search or a verdict is reached.
        let verdict = loop {
            match next {
                Expanded::Done(v) => break v,
                Expanded::Open(mut frame) => match frame.start_option(ctx) {
                    Start::Exhausted => break frame.finish(ctx),
                    Start::Child(k, cp) => {
                        let depth = frame.max_depth - 1;
                        stack.push((frame, k, cp));
                        next = expand(ctx, depth);
                    }
                    Start::Decided(k, cp, v) => {
                        frame.on_child(ctx, k, cp, v);
                        next = Expanded::Open(frame);
                    }
                },
            }
        };
        // Hand the verdict to the frame that asked for it.
        let Some((mut frame, k, cp)) = stack.pop() else {
            return verdict;
        };
        frame.on_child(ctx, k, cp, verdict);
        next = Expanded::Open(frame);
    }
}

/// What [`expand`] found.
enum Expanded {
    /// A verdict for this point of the search.
    Done(SearchVerdict),
    /// A decision to branch on.
    Open(Frame),
}

/// Saturate at depth `max_depth` and find what to do next: a verdict, or the
/// next decision to branch on (#288 step 3b).
fn expand(ctx: &mut TableauContext<'_, '_, '_>, max_depth: usize) -> Expanded {
    loop {
        if max_depth == 0 {
            // Genuine depth-cap bottom-out — the harm the adaptive early-abandon
            // meters. Kept FIRST so `check_deadline`'s sticky side effect stays
            // short-circuited exactly as before.
            ctx.note_depth_cap_hit();
            if trace_enabled() {
                eprintln!("# trace search depth=0_or_deadline");
            }
            return Expanded::Done(SearchVerdict::DepthLimit);
        }
        // `early_abandoned` is a latched flag and is `false` whenever the lever is
        // unarmed, so flag-OFF this is verbatim the previous predicate.
        if ctx.check_deadline() || ctx.early_abandoned() {
            if trace_enabled() {
                eprintln!("# trace search depth=0_or_deadline");
            }
            return Expanded::Done(SearchVerdict::DepthLimit);
        }
        let verdict = match saturate(ctx, SATURATE_ITERS) {
            SaturationResult::Clash(_, deps) => {
                diag::clash(deps.len());
                if trace_enabled() {
                    eprintln!(
                        "# trace search depth={max_depth} clash=immediate deps={}",
                        deps.len()
                    );
                }
                SearchVerdict::Unsat(deps)
            }
            SaturationResult::Stalled => SearchVerdict::DepthLimit,
            SaturationResult::NodeCapped => SearchVerdict::NodeCap,
            SaturationResult::Stable => {
                // Step 0 (#293): an over-full `≤n` with more than one pair that may
                // merge. Saturation used to commit to the first pair; which pair
                // to merge is a real choice, so branch over them here.
                if let Some(open) = first_open_max(ctx) {
                    if trace_enabled() {
                        eprintln!(
                            "# trace search depth={max_depth} max pivot={} options={} graph_nodes={}",
                            open.pivot.index(),
                            open.partners.len(),
                            ctx.graph().len()
                        );
                    }
                    diag::open(ctx.pool(), 2, open.pivot.index(), 0);
                    return Expanded::Open(Frame::merge(ctx, max_depth, open));
                }
                // Step 1: ⊔ branching has priority — it's structurally
                // cheaper and keeps the search shape predictable.
                if let Some((node, or_label, disjuncts, or_deps)) = first_open_disjunction(ctx) {
                    diag::open(ctx.pool(), 0, node.index(), or_label.index());
                    if trace_enabled() {
                        eprintln!(
                            "# trace search depth={max_depth} disj node={} options={} graph_nodes={}",
                            node.index(),
                            disjuncts.len(),
                            ctx.graph().len()
                        );
                    }
                    return Expanded::Open(Frame::or(ctx, max_depth, node, &disjuncts, &or_deps));
                }
                // Step 2: choose rule for `≤n R.C` — pick a neighbour
                // that doesn't yet have `C` or `¬C` and branch.
                if let Some((node, c, c_neg)) = first_open_choose(ctx) {
                    if trace_enabled() {
                        eprintln!(
                            "# trace search depth={max_depth} choose node={} graph_nodes={}",
                            node.index(),
                            ctx.graph().len()
                        );
                    }
                    diag::open(ctx.pool(), 1, node.index(), c.index());
                    return Expanded::Open(Frame::or(
                        ctx,
                        max_depth,
                        node,
                        &[c, c_neg],
                        &DepSet::new(),
                    ));
                }
                // #294: `saturate` can stop with work left. A label added to a
                // node does not dirty its neighbours, so a rule there that reads
                // it (the `≤n` count at the parent) may not re-run, and only the
                // next call's mark-all catches it. A `Sat` leaf has no next call,
                // so run one: if it changes anything, search the new state.
                if crate::sat_leaf_resaturate_enabled() {
                    let before = ctx.trail().len();
                    match saturate(ctx, SATURATE_ITERS) {
                        SaturationResult::Stable if !ctx.trail().mutated_since(before) => {}
                        // Search the new state at the same depth.
                        SaturationResult::Stable => continue,
                        SaturationResult::Clash(_, deps) => {
                            if trace_enabled() {
                                eprintln!(
                                    "# trace search depth={max_depth} clash=sat_leaf_resaturate deps={}",
                                    deps.len()
                                );
                            }
                            return Expanded::Done(SearchVerdict::Unsat(deps));
                        }
                        SaturationResult::Stalled => {
                            return Expanded::Done(SearchVerdict::DepthLimit);
                        }
                        SaturationResult::NodeCapped => {
                            return Expanded::Done(SearchVerdict::NodeCap);
                        }
                    }
                }
                if trace_enabled() {
                    eprintln!(
                        "# trace search depth={max_depth} sat graph_nodes={}",
                        ctx.graph().len()
                    );
                }
                SearchVerdict::Sat
            }
        };
        return Expanded::Done(verdict);
    }
}

/// An open `≤n R.C` (#293), ready to branch on.
struct OpenMax {
    /// The `≤n` label's deps.
    max_deps: DepSet,
    /// `n`.
    bound: usize,
    /// The first witness (in node order) that may merge with an earlier one.
    pivot: NodeId,
    /// The earlier witnesses `pivot` may merge with, each with its
    /// merge-condition deps: every witness's deps, not only the pair's, since
    /// the merge is forced because all of them exist (#288).
    partners: Vec<(NodeId, DepSet)>,
    /// How many witnesses precede `pivot`. They are pairwise distinct, because
    /// `pivot` is the first witness with a mergeable earlier one.
    earlier: usize,
    /// Every witness's deps (the same set each partner carries).
    witness_deps: DepSet,
    /// The deps of the distinctness marks among the witnesses up to and
    /// including `pivot` (#370): the marks that removed the other merges and,
    /// when `earlier >= bound`, the distinct option. Empty unless
    /// `RUSTDL_MERGE_EXACT_DEPS` is on.
    distinct_deps: DepSet,
}

/// Find an over-full `≤n R.C` with at least two pairs that may merge (#293),
/// which `apply_max` leaves to the search when
/// [`crate::max_choice_point_enabled`].
///
/// Branching over pairs reaches the same final merge in every order, which is
/// exponential: `≤1 r.C` over k witnesses explored every merge sequence. So the
/// witnesses are taken in node order and only the first one with a mergeable
/// earlier witness is decided: it merges into one of those, or is distinct from
/// all of them. Each way of partitioning the witnesses is then reached once.
fn first_open_max(ctx: &TableauContext<'_, '_, '_>) -> Option<OpenMax> {
    if !crate::max_choice_point_enabled() {
        return None;
    }
    let pool = ctx.pool();
    let graph = ctx.graph();
    for idx in 0..graph.len() {
        let node_id = NodeId::new(u32::try_from(idx).expect("node count exceeds u32"));
        let node = graph.node(node_id);
        if node.is_redirected() {
            continue;
        }
        for (pos, &c) in node.labels().iter().enumerate() {
            let ConceptExpr::Max(n, role, body) = pool.get(c) else {
                continue;
            };
            if ctx.dkey_range_of(*body).is_some() || ctx.is_blocked(node_id) {
                continue;
            }
            let mut witnesses = crate::rules::max_witnesses(ctx, node_id, *role, *body);
            if witnesses.len() <= *n as usize
                || crate::rules::mergeable_pairs(ctx, &witnesses).len() < 2
            {
                continue;
            }
            witnesses.sort_unstable();
            let max_deps = node.label_deps[pos].clone();
            // #288: the `≤n` is over-full because every witness exists, so a
            // merge depends on all of them, as in `apply_max`. Pairwise deps
            // were enough only because `Frame::merge` reports exhaustion
            // under every active decision; selective undo needs them exact.
            let mut witness_deps = max_deps.clone();
            for &w in &witnesses {
                witness_deps = crate::deps::union(
                    &witness_deps,
                    &crate::rules::compute_max_merge_deps(
                        ctx, node_id, *role, *body, w, w, &max_deps,
                    ),
                );
            }
            for (j, &pivot) in witnesses.iter().enumerate() {
                let partners: Vec<(NodeId, DepSet)> = witnesses[..j]
                    .iter()
                    .filter(|&&w| !ctx.are_distinct(w, pivot))
                    // A pair's own merge deps are a subset of `witness_deps`
                    // (the same per-witness edge and body-label deps).
                    .map(|&w| (w, witness_deps.clone()))
                    .collect();
                if !partners.is_empty() {
                    let distinct_deps = if ctx.merge_exact_deps() {
                        crate::rules::distinct_marks_deps(ctx, &witnesses, j + 1)
                    } else {
                        DepSet::new()
                    };
                    return Some(OpenMax {
                        max_deps,
                        bound: *n as usize,
                        pivot,
                        partners,
                        earlier: j,
                        witness_deps,
                        distinct_deps,
                    });
                }
            }
        }
    }
    None
}

/// What a frame chooses between (#288 step 3).
enum FrameKind {
    /// A `⊔` label, or the choose rule's `C ⊔ ¬C`, at `node`.
    Or {
        node: NodeId,
        /// The options in the order they are tried ([`reorder_disjuncts`]).
        ordered: Vec<ConceptId>,
        /// The `⊔` label's deps (empty for the choose rule).
        parent_deps: DepSet,
        /// Restricted semantic branching companion. When option `d_j`
        /// failed and `¬d_j` is registered as a cheap literal complement,
        /// assert `¬d_j` in every subsequent branch so any rule that
        /// tries to re-derive `d_j` clashes immediately. Compound
        /// complements (Or, quantified) are *not* carried forward — they
        /// would inflate the label set without back-jumping enough subtree
        /// to pay for themselves (see `docs/phase4-backjumping-plan.md`).
        /// Each with the clash deps of the disjunct whose failure justifies it
        /// (#288): `¬d_j` holds because `d_j` failed, so it depends on exactly what
        /// that failure depended on.
        literal_complements: Vec<(ConceptId, DepSet)>,
    },
    /// An over-full `≤n` (#293): merge the pivot into each partner, then
    /// (if the bound allows) keep it distinct from all of them.
    Merge { open: OpenMax },
}

/// One decision of the search: its options, which one is being tried, and
/// what the refuted ones said (#288 step 3).
struct Frame {
    kind: FrameKind,
    my_id: u32,
    max_depth: usize,
    total: usize,
    /// The option `start_option` tries next.
    next: usize,
    /// Deps the frame reports on exhaustion besides its refuted options'.
    initial: DepSet,
    /// Clash deps of each refuted option; every one contains `my_id`.
    refuted: Vec<DepSet>,
    depth_limited: bool,
    early_return: Option<SearchVerdict>,
}

/// What [`Frame::start_option`] did.
enum Start {
    /// No option left to try, or the frame has already concluded.
    Exhausted,
    /// Option `k` is asserted; search below it.
    Child(usize, Checkpoint),
    /// Option `k` concluded without a search.
    Decided(usize, Checkpoint, SearchVerdict),
}

impl Frame {
    /// Branch on an open `≤n` (#293): merge the pivot into each mergeable
    /// earlier witness in turn, then try it distinct from all of them. A merge
    /// carries this frame's id, a clash that does not depend on it back-jumps,
    /// and if every option fails the `≤n` is unsatisfiable under every active
    /// decision (the distinctness that rules out other merges is not tracked
    /// precisely, so this stays conservative, as `apply_max`'s `⊥` arm does).
    /// Each merge's deps cover every witness (see [`first_open_max`]).
    fn merge(ctx: &mut TableauContext<'_, '_, '_>, max_depth: usize, open: OpenMax) -> Self {
        // Read before this frame's own id is pushed.
        let initial = if ctx.merge_exact_deps() {
            // #370: every option failing means the `≤n` cannot hold given its
            // label, the witnesses, and the marks that removed the other merges
            // (and the distinct option, when `earlier >= bound`).
            let exact = crate::deps::union(
                &crate::deps::union(&open.max_deps, &open.witness_deps),
                &open.distinct_deps,
            );
            crate::bump_counter!(ctx, merge_exact_p4);
            #[cfg(feature = "counters")]
            if ctx
                .active_branches()
                .iter()
                .any(|b| exact.binary_search(b).is_err())
            {
                crate::bump_counter!(ctx, merge_exact_p4_narrower);
            }
            exact
        } else {
            crate::deps::union(&open.max_deps, &DepSet::from_slice(ctx.active_branches()))
        };
        let my_id = ctx.push_branch();
        // The distinct option leaves `earlier + 1` pairwise-distinct witnesses,
        // which a bound of `earlier` or less cannot accommodate.
        let try_distinct = open.earlier < open.bound;
        let total = open.partners.len() + usize::from(try_distinct);
        Self {
            kind: FrameKind::Merge { open },
            my_id,
            max_depth,
            total,
            next: 0,
            initial,
            refuted: Vec::new(),
            depth_limited: false,
            early_return: None,
        }
    }

    fn or(
        ctx: &mut TableauContext<'_, '_, '_>,
        max_depth: usize,
        node: NodeId,
        options: &[ConceptId],
        parent_deps: &[u32],
    ) -> Self {
        let my_id = ctx.push_branch();
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
        let total = ordered.len();
        Self {
            kind: FrameKind::Or {
                node,
                ordered,
                parent_deps: DepSet::from_slice(parent_deps),
                literal_complements: Vec::new(),
            },
            my_id,
            max_depth,
            total,
            next: 0,
            initial: DepSet::new(),
            refuted: Vec::new(),
            depth_limited: false,
            early_return: None,
        }
    }

    /// Assert the next option behind a fresh checkpoint.
    fn start_option(&mut self, ctx: &mut TableauContext<'_, '_, '_>) -> Start {
        if self.early_return.is_some() || self.next >= self.total {
            return Start::Exhausted;
        }
        let k = self.next;
        self.next += 1;
        let (my_id, max_depth, total) = (self.my_id, self.max_depth, self.total);
        match &self.kind {
            FrameKind::Merge { open } => {
                let merge_into = open.partners.get(k);
                if trace_enabled() {
                    match merge_into {
                        Some((w, _)) => eprintln!(
                            "# trace branch depth={max_depth} my_id={my_id} merge={}/{total} n{} into n{}",
                            k + 1,
                            open.pivot.index(),
                            w.index()
                        ),
                        None => eprintln!(
                            "# trace branch depth={max_depth} my_id={my_id} merge={}/{total} n{} distinct",
                            k + 1,
                            open.pivot.index()
                        ),
                    }
                }
                let cp = ctx.checkpoint();
                if let Some((w, pair_deps)) = merge_into {
                    let mut deps = pair_deps.clone();
                    if let Err(pos) = deps.binary_search(&my_id) {
                        deps.insert(pos, my_id);
                    }
                    // #362: drop the pivot's blockable subtree instead of letting
                    // the merge reparent it (behind `cp`, so the next option
                    // starts from the unpruned graph).
                    let _ = ctx.prune_subtree(open.pivot, *w);
                    if ctx.merge_into_with_deps(open.pivot, *w, deps.as_slice()) {
                        Start::Child(k, cp)
                    } else {
                        // Unreachable: the pair was mergeable when listed and
                        // nothing has run since the last rollback. With exact
                        // deps (#370) this `{my_id}` would under-report.
                        debug_assert!(!ctx.merge_exact_deps(), "merge frame: refused merge");
                        Start::Decided(k, cp, SearchVerdict::Unsat(DepSet::from_slice(&[my_id])))
                    }
                } else {
                    // The marks are this decision's; the witnesses' deps keep
                    // the record self-contained for any reader (#370).
                    let mut deps = open.witness_deps.clone();
                    if let Err(pos) = deps.binary_search(&my_id) {
                        deps.insert(pos, my_id);
                    }
                    for (w, _) in &open.partners {
                        ctx.mark_distinct_with_deps(open.pivot, *w, &deps);
                    }
                    Start::Child(k, cp)
                }
            }
            FrameKind::Or {
                node,
                ordered,
                parent_deps,
                literal_complements,
            } => {
                let d = ordered[k];
                if trace_enabled() {
                    eprintln!(
                        "# trace branch depth={max_depth} my_id={my_id} pick={}/{total} disj={}",
                        k + 1,
                        d.index()
                    );
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
                let mut combined_deps = parent_deps.clone();
                if let Err(pos) = combined_deps.binary_search(&my_id) {
                    combined_deps.insert(pos, my_id);
                }
                // Assert prior failed disjuncts' literal complements.
                for (comp, comp_deps) in literal_complements {
                    // The failed disjunct carried `combined_deps`, and every
                    // set that contains `my_id` got it from there, so its clash
                    // deps contain them. If this ever fails, the tags would be
                    // narrower than before and backjumping could change.
                    debug_assert!(
                        combined_deps
                            .iter()
                            .all(|x| comp_deps.binary_search(x).is_ok()),
                        "complement deps {comp_deps:?} miss part of {combined_deps:?}"
                    );
                    ctx.add_label_with_deps(*node, *comp, comp_deps.as_slice());
                }
                // CDBL Phase 1 (docs/cdbl-plan.md): record which disjunct
                // concept this branch is asserting, so a downstream clash's
                // DepSet can be translated to the structural set of disjunct
                // concepts that caused it. Overwrites the entry for `my_id`
                // each iteration — at clash time it reflects the disjunct
                // currently under trial, which is exactly the one in scope.
                // Phase-1 bookkeeping only; no lookup acts on it yet.
                ctx.record_decision(my_id, *node, d);
                // The labelled disjunct depends on *this* branch decision and
                // every reason the parent disjunction was at this node.
                ctx.add_label_with_deps(*node, d, combined_deps.as_slice());
                Start::Child(k, cp)
            }
        }
    }

    /// Take the verdict of option `k`, the one `start_option` last asserted.
    fn on_child(
        &mut self,
        ctx: &mut TableauContext<'_, '_, '_>,
        k: usize,
        cp: Checkpoint,
        verdict: SearchVerdict,
    ) {
        // Whether the child concluded. Feeds the early-abandon telemetry (the
        // criterion itself is the cumulative depth-cap-hit count, latched in
        // `note_depth_cap_hit`). Flag-OFF `note_branch_trial` is a single
        // `Option` discriminant test returning `false`.
        let definite = matches!(
            verdict,
            SearchVerdict::Sat | SearchVerdict::Unsat(_) | SearchVerdict::NodeCap
        );
        match verdict {
            SearchVerdict::Sat => {
                // Found a model; keep state, exit early. State is
                // left as-is — the model labels are real.
                self.early_return = Some(SearchVerdict::Sat);
            }
            SearchVerdict::Unsat(clash_deps) => {
                ctx.rollback_to(cp);
                if clash_deps.binary_search(&self.my_id).is_err() {
                    // Back-jump: this branch decision didn't
                    // contribute to the clash. Every sibling option
                    // would clash for the same upstream reason —
                    // propagate the failure straight up.
                    {
                        let frame_node = match &self.kind {
                            FrameKind::Or { node, .. } => *node,
                            FrameKind::Merge { open } => open.pivot,
                        };
                        let related = |a: NodeId, b: NodeId| {
                            // a is b or an ancestor of b
                            let mut n = Some(b);
                            while let Some(x) = n {
                                if x == a {
                                    return true;
                                }
                                n = ctx.graph().node(x).parent();
                            }
                            false
                        };
                        let independent = !clash_deps
                            .iter()
                            .filter_map(|&d| ctx.diag_decision_node(d))
                            .any(|d| related(frame_node, d) || related(d, frame_node));
                        diag::skip_kind(independent);
                    }
                    self.early_return = Some(SearchVerdict::Unsat(clash_deps));
                } else {
                    // This decision mattered. Keep its deps for the "all
                    // options exhausted" case. (Recording side of
                    // conflict-driven learning is wired but the lookup is
                    // unsound, so the recording would be free-allocated
                    // garbage. See the comment on the lookup side in
                    // `start_option`.) Carry forward the failed
                    // disjunct's literal complement (if it has one registered)
                    // so the next iteration short-circuits any rebirth of
                    // `d` in the model.
                    if let FrameKind::Or {
                        ordered,
                        literal_complements,
                        ..
                    } = &mut self.kind
                        && let Some(comp) = ctx.complement_of(ordered[k])
                        && is_literal(ctx, comp)
                    {
                        literal_complements.push((comp, clash_deps.clone()));
                    }
                    self.refuted.push(clash_deps);
                }
            }
            SearchVerdict::DepthLimit => {
                ctx.rollback_to(cp);
                self.depth_limited = true;
            }
            SearchVerdict::NodeCap => {
                // Hard early-return: a global node-cap trip means the
                // whole search is too expensive to continue — abandon
                // the remaining sibling options rather than trying
                // them (Task 1's soft handling let an exploding search
                // re-grow the graph per sibling, which was slow). This
                // is sound: NodeCap maps to `Ok(None)` in `decide` (a
                // sound MISS / consistent-under-approx), and giving up
                // earlier only ever yields a MISS, never a false
                // positive.
                ctx.rollback_to(cp);
                self.early_return = Some(SearchVerdict::NodeCap);
            }
        }
        // Meter the trial. `note_branch_trial` returns `true` only when the cut
        // fires, which requires `!definite` — and a `!definite` trial is exactly
        // the `DepthLimit` arm, which sets no `early_return`, so this can never
        // clobber a `Sat` / back-jumped `Unsat` / `NodeCap` verdict.
        // Stop trying siblings once the probe is abandoned. No rollback is owed:
        // the abandoning arm is `DepthLimit`, which has already rolled back to
        // `cp`. `early_return.is_none()` is belt-and-braces — a latched probe can
        // no longer produce a `Sat` or a back-jumped `Unsat`, because every
        // `search` entry returns `DepthLimit` from the latch onwards — so this can
        // never downgrade a definite verdict to a non-verdict.
        if ctx.note_branch_trial(definite) && self.early_return.is_none() {
            self.early_return = Some(SearchVerdict::DepthLimit);
        }
    }

    /// The frame's verdict once no option is left to try.
    fn finish(self, ctx: &mut TableauContext<'_, '_, '_>) -> SearchVerdict {
        ctx.pop_branch();
        if let Some(v) = self.early_return {
            v
        } else if self.depth_limited {
            SearchVerdict::DepthLimit
        } else {
            // Every option clashed and every clash depended on `my_id`.
            // The decision itself is therefore unsat under the union of
            // the other deps of its refuted options (plus, for a merge,
            // every active decision).
            let mut combined = self.initial;
            for deps in &self.refuted {
                for &x in deps {
                    if x != self.my_id
                        && let Err(pos) = combined.binary_search(&x)
                    {
                        combined.insert(pos, x);
                    }
                }
            }
            diag::exhaust(self.refuted.len());
            SearchVerdict::Unsat(combined)
        }
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

/// Most `R`-successors a node may have for a `∀R.C` disjunct to be tried first
/// (#288). Measured on PMD core and on a node with hundreds of role assertions:
/// allowing 0 or 1, `PMD_0020213` stops finishing within 90 s; allowing 3, it
/// takes 39 CPU-s, and allowing 7 gains nothing more. With no bound, a
/// 600-edge individual's `consistent` fell from 1.6 s to a budget `unknown`.
const FORALL_FIRST_MAX_SUCCESSORS: usize = 3;

/// Stable secondary key on original index keeps the
/// literal-complements optimisation downstream deterministic.
fn reorder_disjuncts(
    ctx: &TableauContext<'_, '_, '_>,
    node: NodeId,
    options: &[ConceptId],
) -> Vec<ConceptId> {
    let pool = ctx.pool();
    let labels = ctx.graph().node(node).labels();
    let forall_first = ctx.forall_first_enabled();

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
        // DIAGNOSTIC (#382): a `≤n R.C` the node already meets by count generates
        // nothing, so try it first.
        if std::env::var_os("RUSTDL_DIAG_MAX_FIRST").is_some_and(|v| v == "1")
            && let ConceptExpr::Max(n, role, _) = pool.get(d)
            && ctx
                .graph()
                .node(node)
                .neighbours()
                .filter(|(seen, _)| ctx.edge_satisfies(*seen, *role))
                .nth(*n as usize)
                .is_none()
        {
            return 0;
        }
        // #288: `∀R.C` generates nothing, so at a node with few `R`-successors
        // try it before an atomic disjunct whose definition may generate (see
        // `forall_first_enabled`). Its cost is pushing `C` into each
        // `R`-successor, so past `FORALL_FIRST_MAX_SUCCESSORS` it keeps its old
        // score.
        if forall_first
            && let ConceptExpr::All(role, _) = pool.get(d)
            && ctx
                .graph()
                .node(node)
                .neighbours()
                .filter(|(seen, _)| ctx.edge_satisfies(*seen, *role))
                .nth(FORALL_FIRST_MAX_SUCCESSORS)
                .is_none()
        {
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
/// self-restriction, or `Not(_)` of one. Used by `Frame::or` to
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
        // #284: a merged-away node's `≤n` is enforced on its representative.
        if graph.node(node_id).is_redirected() {
            continue;
        }
        for &c in graph.node(node_id).labels() {
            let ConceptExpr::Max(_, role, body) = pool.get(c) else {
                continue;
            };
            // DIAGNOSTIC (#382): every node is in `⊤`, so `⊤ ⊔ ¬⊤` decides nothing.
            if std::env::var_os("RUSTDL_DIAG_CHOOSE_TOP").is_some_and(|v| v == "1")
                && matches!(pool.get(*body), ConceptExpr::Top)
            {
                continue;
            }
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
    // #35 v4: with nominals-first scheduling on, prefer an open `Or`
    // carrying a `Nominal` disjunct — resolving the nominal-covering
    // disjunction first lets the o-rule merge the deferred node
    // (rules.rs Task 3) before ∃/≥ generation resumes. Flag off (or
    // no nominal-bearing Or open): identical to the historical
    // first-open choice.
    let prefer_nominal = crate::nominal_first_enabled();
    let mut first_any: Option<(NodeId, ConceptId, Vec<ConceptId>, DepSet)> = None;
    for idx in 0..graph.len() {
        let node_id = NodeId::new(u32::try_from(idx).expect("node count exceeds u32"));
        let node = graph.node(node_id);
        // #284: a merged-away node's labels are enforced on its representative.
        if node.is_redirected() {
            continue;
        }
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
                let hit = (node_id, c, args.to_vec(), node.label_deps[pos].clone());
                let has_nominal = prefer_nominal
                    && args
                        .iter()
                        .any(|&d| matches!(pool.get(d), ConceptExpr::Nominal(_)));
                if has_nominal {
                    return Some(hit);
                }
                if first_any.is_none() {
                    first_any = Some(hit);
                }
            }
        }
    }
    first_any
}

#[cfg(test)]
#[allow(clippy::many_single_char_names)]
mod tests {
    use crate::TableauContext;
    use owl_dl_core::{ClassId, ConceptExpr, ConceptPool, IndividualId};

    #[test]
    #[ignore = "nominal-first deferred (A redesign); opt-in RUSTDL_NOMINAL_FIRST=1, run with --ignored"]
    fn first_open_disjunction_prefers_nominal_bearing() {
        // #35 v4 Task 4: with nominals-first scheduling on, the search
        // driver must resolve a nominal-covering disjunction (an `Or`
        // with a `Nominal` disjunct) BEFORE any plain disjunction, so
        // the o-rule can merge the deferred node (Task 3) before
        // generation resumes.
        //
        // NOTE (deferred, 2026-07-23): `RUSTDL_NOMINAL_FIRST` now defaults
        // OFF (nominal-first scheduling did not bound the issue #35 target
        // bug; the validated fix is the realize pair-timeout + hard NodeCap
        // safety net, independent of this flag). This test documents the
        // deferred-A priority behaviour and only passes with
        // `RUSTDL_NOMINAL_FIRST=1` set process-wide before the `OnceLock`
        // initializes (run with `--ignored` in a dedicated process, since
        // `nominal_first_enabled` is OnceLock-cached and shared with other
        // tests in this binary).
        let mut pool = ConceptPool::new();
        let p = pool.atomic(ClassId::new(0));
        let q = pool.atomic(ClassId::new(1));
        // Interned FIRST -> smaller ConceptId -> earlier in the node's
        // sorted label order than the nominal-bearing Or below.
        let plain_or = pool.or([p, q]);
        let x = pool.nominal(IndividualId::new(0));
        let y = pool.nominal(IndividualId::new(1));
        let nominal_or = pool.or([x, y]);
        assert!(
            plain_or < nominal_or,
            "plain Or must precede in label order"
        );

        let mut ctx = TableauContext::new(&pool);
        let n = ctx.new_node();
        ctx.add_label(n, plain_or);
        ctx.add_label(n, nominal_or);

        let (_, chosen, _, _) = super::first_open_disjunction(&ctx).expect("an open Or");
        assert!(
            matches!(ctx.pool().get(chosen), ConceptExpr::Or(args)
                if args
                    .iter()
                    .any(|&d| matches!(ctx.pool().get(d), ConceptExpr::Nominal(_)))),
            "nominal-bearing Or must win despite being second"
        );
    }
}

/// Canaries for the adaptive early-abandon
/// (`RUSTDL_TABLEAU_EARLY_ABANDON`; see `TableauContext::note_depth_cap_hit` and
/// `docs/2026-08-03-tableau-early-abandon.md`).
///
/// These live in the tableau crate on purpose: the lever's two hooks are *in*
/// `search`/`branch`, and the iterative-deepening write-up recorded an uncaught
/// sabotage precisely because its canaries pinned a `TableauContext` API without
/// pinning the call. Every test below goes through the real `search` driver.
///
/// The env flag is NOT read here — the limit is passed explicitly to
/// `enable_early_abandon`, so these tests are immune to env-ordering flakiness
/// (`OnceLock`-cached reads elsewhere in the workspace have caused exactly that).
/// The flag's own default-OFF idiom is canaried on the reasoner side, which owns it.
#[cfg(test)]
mod early_abandon_tests {
    use super::{SearchVerdict, search};
    use crate::TableauContext;
    use owl_dl_core::{ClassId, ConceptId, ConceptPool};

    /// `⊔`-chain depth. Level 0 is `⊔(a ⊓ ¬a, b ⊓ ¬b)`; level k is
    /// `⊔(level_{k-1}, c_k ⊓ ¬c_k)`. So every level has TWO live options: the
    /// nested one recurses, and the conjunction clashes only *after* the ⊓-rule
    /// expands it — a DEFINITE verdict. One branch level per link, so a cap below
    /// `CHAIN_DEPTH` bottoms out and a cap above it refutes.
    ///
    /// **The obvious fixture does NOT work, and finding that out is why this is a
    /// negatives-first suite.** A first attempt labelled `¬a, ¬b, ¬c_k` at the
    /// node and used bare atomics as the second option. Every such disjunct is
    /// pruned by the deterministic `⊔`-rule's literal-complement check, leaving a
    /// single live disjunct per level, so the whole chain unit-propagated to a
    /// clash with **zero** branch decisions and **zero** depth-cap hits — the
    /// three controls passed and five assertions were vacuous. A clash that needs
    /// *expansion* is what forces a real branch.
    const CHAIN_DEPTH: usize = 12;

    /// A cap comfortably above `CHAIN_DEPTH` (plus the frames `search` itself
    /// consumes) — the arm where the depth cap is never reached.
    const DEEP_CAP: usize = 40;

    /// A cap comfortably below `CHAIN_DEPTH` — the arm that bottoms out.
    const SHALLOW_CAP: usize = 8;

    /// `(trials, definite, depth0, max_stall_run, abandoned)` — the per-probe
    /// early-abandon telemetry, as `TableauContext::early_abandon_stats` returns it.
    type Stats = (u64, u64, u64, u64, bool);

    /// `x ⊓ ¬x` — unsatisfiable, but only once the ⊓-rule has expanded it, so it
    /// survives the `⊔`-rule's cheap literal prune and forces a real branch.
    fn clashing_conjunction(pool: &mut ConceptPool, id: u32) -> ConceptId {
        let x = pool.atomic(ClassId::new(id));
        let nx = pool.not(x);
        pool.and([x, nx])
    }

    /// Build one clashing `⊔`-chain over class ids `base..`, returning its top
    /// `Or`.
    ///
    /// The nested level is wrapped in `⊓(level, filler)` because
    /// [`ConceptPool::or`] **flattens** a nested `Or`: the naive
    /// `or([level_{k-1}, c_k])` collapsed the whole chain into ONE flat 14-ary
    /// disjunction, which the driver resolved in a single frame — measured as
    /// `trials = 14, definite = 14, depth0 = 0` at every cap from 2 to 40, i.e.
    /// the fixture had no depth at all. A conjunction is not flattened into a
    /// disjunction, so the `⊓`-rule re-exposes the inner `Or` one level down and
    /// each link costs exactly one branch level.
    fn chain(pool: &mut ConceptPool, base: u32, depth: usize) -> ConceptId {
        let leaf_a = clashing_conjunction(pool, base);
        let leaf_b = clashing_conjunction(pool, base + 1);
        let mut top = pool.or([leaf_a, leaf_b]);
        for k in 0..depth {
            let off = base + 2 + 2 * u32::try_from(k).expect("small");
            let c = clashing_conjunction(pool, off);
            let filler = pool.atomic(ClassId::new(off + 1));
            let nested = pool.and([top, filler]);
            top = pool.or([nested, c]);
        }
        top
    }

    /// One node labelled a single clashing chain: **unsatisfiable**, and its
    /// refutation needs `CHAIN_DEPTH`-ish branch levels.
    fn one_chain() -> (ConceptPool, ConceptId) {
        let mut pool = ConceptPool::new();
        let top = chain(&mut pool, 0, CHAIN_DEPTH);
        (pool, top)
    }

    /// `⊔(chainA, chainB)` over disjoint class ids: **two** independent paths to
    /// the cap, with decisive clashes in between. The fixture that shows a
    /// *definite verdict does not reset the criterion*.
    fn two_chains() -> (ConceptPool, ConceptId) {
        let mut pool = ConceptPool::new();
        let a_top = chain(&mut pool, 0, CHAIN_DEPTH);
        let b_top = chain(&mut pool, 500, CHAIN_DEPTH);
        let root = pool.or([a_top, b_top]);
        (pool, root)
    }

    /// `⊔(⊓(chain, f), ⊓(s, g))` — a deep unsatisfiable-at-this-cap option
    /// followed by a plainly satisfiable one. Flag-OFF this fixture is
    /// **satisfiable** (the chain bottoms out, then the second option models),
    /// which is the shape that makes a verdict *change* observable rather than
    /// only a counter.
    ///
    /// **Both sides must be wrapped in `⊓`.** The naive
    /// `or([chain_top, or([s1, s2])])` is FLATTENED into one disjunction, and
    /// `reorder_disjuncts` then scores the bare atomics 1 against the
    /// conjunctions' 2 — so the satisfiable atomic was tried FIRST, the chain was
    /// never entered, and the cut never fired. Measured, not assumed: that
    /// version failed this test with "the cut must have fired".
    fn sat_behind_a_deep_option() -> (ConceptPool, ConceptId) {
        let mut pool = ConceptPool::new();
        let c_top = chain(&mut pool, 0, CHAIN_DEPTH);
        let f = pool.atomic(ClassId::new(900));
        let deep_side = pool.and([c_top, f]);
        let s = pool.atomic(ClassId::new(901));
        let g = pool.atomic(ClassId::new(902));
        let sat_side = pool.and([s, g]);
        let root = pool.or([deep_side, sat_side]);
        (pool, root)
    }

    /// Run one `search` over `fixture` at `cap`, with the early-abandon armed at
    /// `limit` (`None` = unarmed, i.e. the flag-OFF path).
    /// Returns `(verdict, stats)`.
    fn run(
        fixture: (ConceptPool, ConceptId),
        cap: usize,
        limit: Option<u64>,
    ) -> (SearchVerdict, Option<Stats>) {
        let (pool, root) = fixture;
        let mut ctx = TableauContext::new(&pool);
        if let Some(l) = limit {
            ctx.enable_early_abandon(l);
        }
        let n = ctx.new_node();
        ctx.add_label(n, root);
        let v = search(&mut ctx, cap);
        let s = ctx.early_abandon_stats();
        (v, s)
    }

    // ---------------------------------------------------------- negatives first

    /// **Control.** Unarmed, at a cap ABOVE the chain depth, the fixture is
    /// refuted. If this ever stopped being `Unsat`, every "the cut lost a
    /// verdict" assertion below would be measuring a fixture that had no verdict
    /// to lose.
    #[test]
    fn unarmed_deep_cap_refutes_the_chain() {
        let (v, s) = run(one_chain(), DEEP_CAP, None);
        assert!(matches!(v, SearchVerdict::Unsat(_)), "got {v:?}");
        assert!(s.is_none(), "unarmed ⇒ no accounting at all");
    }

    /// **Control.** Armed at a cap ABOVE the chain depth, the depth cap is never
    /// reached — `depth0 == 0` — so the criterion is INERT and the verdict is
    /// unchanged even at the most aggressive limit of 1. This is the
    /// completeness-preservation property the whole design rests on: a search
    /// that does not bottom out cannot be cut.
    #[test]
    fn a_search_that_never_bottoms_out_is_never_cut() {
        let (v, s) = run(one_chain(), DEEP_CAP, Some(1));
        let (_, _, depth0, _, abandoned) = s.expect("armed");
        assert_eq!(depth0, 0, "the cap must not be reached at DEEP_CAP");
        assert!(!abandoned, "nothing to abandon");
        assert!(
            matches!(v, SearchVerdict::Unsat(_)),
            "verdict unchanged: {v:?}"
        );
    }

    /// **Control.** Armed at a cap BELOW the chain depth with the limit
    /// DISABLED (`0`), the accounting is live and the cap IS reached — so the
    /// fixture really does exercise the criterion's input, and
    /// `the_cut_fires_at_the_limit` is not vacuous.
    #[test]
    fn limit_zero_keeps_accounting_but_never_cuts() {
        let (v, s) = run(one_chain(), SHALLOW_CAP, Some(0));
        let (trials, _, depth0, _, abandoned) = s.expect("armed");
        assert!(depth0 >= 1, "the cap must be reached at SHALLOW_CAP");
        assert!(trials >= 1, "branch trials must have happened");
        assert!(!abandoned, "limit 0 must never cut");
        assert!(matches!(v, SearchVerdict::DepthLimit), "got {v:?}");
    }

    // ------------------------------------------------------------- the criterion

    /// The cut fires once the cap has been hit `limit` times, and reports
    /// `DepthLimit`.
    #[test]
    fn the_cut_fires_at_the_limit() {
        let (v, s) = run(one_chain(), SHALLOW_CAP, Some(1));
        let (_, _, depth0, _, abandoned) = s.expect("armed");
        assert!(abandoned, "limit 1 must cut on the first bottom-out");
        assert_eq!(depth0, 1, "and must cut AT the limit, not later");
        assert!(matches!(v, SearchVerdict::DepthLimit), "got {v:?}");
    }

    /// **The refutation of the "no progress" shape, pinned in code.** On
    /// `two_chains` the search returns DEFINITE verdicts (decisive atomic
    /// clashes) *between* the two paths to the cap, so a criterion that reset on
    /// progress would never reach 2. The shipped cumulative criterion does.
    #[test]
    fn a_definite_verdict_does_not_reset_the_criterion() {
        // Arm A (limit 0, accounting only) establishes that this fixture really
        // does interleave progress with cap hits — `max_stall_run < depth0` is the
        // discriminating fact, and without it arm B would prove nothing.
        let (_, a) = run(two_chains(), SHALLOW_CAP, Some(0));
        let (_, definite, depth0, max_stall_run, _) = a.expect("armed");
        assert!(definite >= 1, "the fixture must produce definite verdicts");
        assert!(depth0 >= 2, "and must reach the cap more than once");
        assert!(
            max_stall_run < depth0,
            "a definite verdict must have interrupted the run \
             (max_stall_run {max_stall_run} vs depth0 {depth0}) — otherwise this \
             fixture does not distinguish the two criteria"
        );
        // Arm B: the limit is set STRICTLY ABOVE the longest run but at or below
        // the cumulative total, so it is reachable ONLY by the cumulative
        // criterion. Choosing `max_stall_run` itself here would leave the refuted
        // variant passing — measured: with the limit at 2 (== max_stall_run) a
        // sabotage that switches the criterion back to `stall_run` kept all 8
        // canaries green.
        let limit = max_stall_run + 1;
        assert!(
            limit <= depth0,
            "limit {limit} must still be reachable cumulatively"
        );
        let (_, b) = run(two_chains(), SHALLOW_CAP, Some(limit));
        assert!(
            b.expect("armed").4,
            "the cumulative criterion must fire at a limit ({limit}) the \
             reset-on-progress variant can never reach (longest run {max_stall_run})"
        );
    }

    /// **A DEADLINE cut is not a depth-cap hit.** The two exits share a
    /// `DepthLimit` verdict, and the constant audit had to split its
    /// `search_depth0`/`search_deadline0` counters for exactly this reason — a
    /// merged counter reads a deadline as a cap hit and the lever then fires on
    /// budget pressure rather than on depth. With an already-elapsed deadline the
    /// accounting must record ZERO cap hits and never abandon.
    #[test]
    fn a_deadline_cut_is_not_counted_as_a_depth_cap_hit() {
        let (pool, root) = one_chain();
        let mut ctx = TableauContext::new(&pool);
        ctx.enable_early_abandon(1);
        ctx.set_deadline(
            std::time::Instant::now()
                .checked_sub(std::time::Duration::from_secs(1))
                .expect("an instant one second in the past exists"),
        );
        let n = ctx.new_node();
        ctx.add_label(n, root);
        let v = search(&mut ctx, DEEP_CAP);
        let (_, _, depth0, _, abandoned) = ctx.early_abandon_stats().expect("armed");
        assert!(matches!(v, SearchVerdict::DepthLimit), "got {v:?}");
        assert!(ctx.deadline_reached(), "the deadline must be what cut this");
        assert_eq!(depth0, 0, "a deadline cut must NOT count as a cap hit");
        assert!(!abandoned, "and must never trip the early abandon");
    }

    /// The cut does LESS work: fewer branch trials than the same search with the
    /// cut disabled. This is the wall claim, at unit scale.
    #[test]
    fn the_cut_does_strictly_less_work() {
        let (_, off) = run(two_chains(), SHALLOW_CAP, Some(0));
        let (_, on) = run(two_chains(), SHALLOW_CAP, Some(1));
        let (off_trials, _, _, _, _) = off.expect("armed");
        let (on_trials, _, _, _, on_abandoned) = on.expect("armed");
        assert!(on_abandoned, "the ON arm must actually cut");
        assert!(
            on_trials < off_trials,
            "cut must reduce trials ({on_trials} vs {off_trials})"
        );
    }

    // ------------------------------------------------------------- soundness

    /// **FP=0, in the only direction that matters.** The cut may only ever turn
    /// a verdict INTO a non-verdict. On a fixture that is genuinely SATISFIABLE
    /// behind a deep first option, flag-OFF returns `Sat`; the cut turns that
    /// into `DepthLimit` — a MISS — and must NEVER return `Unsat`.
    #[test]
    fn the_cut_never_manufactures_an_unsat() {
        let (off, _) = run(sat_behind_a_deep_option(), SHALLOW_CAP, None);
        assert_eq!(
            off,
            SearchVerdict::Sat,
            "control: the fixture IS satisfiable"
        );
        let (on, s) = run(sat_behind_a_deep_option(), SHALLOW_CAP, Some(1));
        assert!(s.expect("armed").4, "the cut must have fired");
        assert!(
            !matches!(on, SearchVerdict::Unsat(_)),
            "an early abandon must never yield Unsat, got {on:?}"
        );
        assert_eq!(
            on,
            SearchVerdict::DepthLimit,
            "and it degrades to a non-verdict"
        );
    }

    /// Armed-with-limit-0 is verdict-identical to unarmed on every fixture, so
    /// the accounting itself never perturbs the search.
    #[test]
    fn accounting_alone_is_verdict_neutral() {
        for cap in [SHALLOW_CAP, DEEP_CAP] {
            let (unarmed, _) = run(one_chain(), cap, None);
            let (armed, _) = run(one_chain(), cap, Some(0));
            assert_eq!(unarmed, armed, "cap {cap}");
            let (unarmed2, _) = run(sat_behind_a_deep_option(), cap, None);
            let (armed2, _) = run(sat_behind_a_deep_option(), cap, Some(0));
            assert_eq!(unarmed2, armed2, "cap {cap} (sat fixture)");
        }
    }
}

/// #288 step 1: the deps a merge and a failed-disjunct complement carry.
///
/// Today both widenings are trace-identical (every clash that reaches the
/// frame is absorbed into its `combined`), so only these tests see them. They
/// pin the dep content selective undo will rely on.
#[cfg(test)]
mod dep_tests {
    use super::{SearchVerdict, first_open_max, search};
    use crate::TableauContext;
    use owl_dl_core::{AbsorbedTBox, ClassId, ConceptPool, ConceptRule, Role, RoleId};

    /// Three witnesses of `≤1 r.C`, each created under its own decision.
    /// Every merge must depend on all three, not only the merged pair.
    #[test]
    #[allow(clippy::many_single_char_names)]
    fn a_choice_point_merge_depends_on_every_witness() {
        let mut pool = ConceptPool::new();
        let r = RoleId::new(0);
        let c = pool.atomic(ClassId::new(0));
        let d = pool.atomic(ClassId::new(1));
        let at_most_one = pool.max(1, Role::Named(r), c);

        let mut ctx = TableauContext::new(&pool);
        let n = ctx.new_node();
        ctx.add_label_with_deps(n, at_most_one, &[]);
        let ids: Vec<u32> = (0..3).map(|_| ctx.push_branch()).collect();
        let witnesses: Vec<_> = ids
            .iter()
            .map(|&b| {
                let w = ctx.new_successor_with_deps(n, r, &[b]);
                ctx.add_label_with_deps(w, c, &[b]);
                w
            })
            .collect();
        // A marker on the pivot (the second witness), moved by the merge.
        ctx.add_label_with_deps(witnesses[1], d, &[]);

        let open = first_open_max(&ctx).expect("an open ≤1 with three mergeable pairs");
        for (_, deps) in &open.partners {
            for b in &ids {
                assert!(
                    deps.contains(b),
                    "merge deps {deps:?} miss witness decision {b}"
                );
            }
        }

        assert!(matches!(search(&mut ctx, 64), SearchVerdict::Sat));
        let holder = (0..ctx.graph().len())
            .map(|i| crate::NodeId::new(u32::try_from(i).expect("node count fits u32")))
            .find(|&x| {
                !ctx.graph().node(x).is_redirected()
                    && ctx.graph().node(x).labels().binary_search(&d).is_ok()
            })
            .expect("the marker survives the merges");
        let deps = ctx.label_deps_of(holder, d).expect("marker deps");
        for b in &ids {
            assert!(
                deps.contains(b),
                "moved label deps {deps:?} miss witness decision {b}"
            );
        }
    }

    /// `A ⊔ B` at a node holding `E`, with `A ⊑ ¬E`. `A` fails on the rule, so
    /// `¬A`, asserted for `B`, must carry `A`'s clash deps. With `outer`, a
    /// decision placed `E`: the complement must then carry it too, where
    /// tagging with the branch's own deps alone would lose it. Returns the
    /// complement's deps and the outer decision's id.
    fn complement_deps(outer: bool) -> (Vec<u32>, Option<u32>) {
        let mut pool = ConceptPool::new();
        let a_cls = ClassId::new(0);
        let a = pool.atomic(a_cls);
        let b = pool.atomic(ClassId::new(1));
        let e = pool.atomic(ClassId::new(2));
        let not_a = pool.not(a);
        let not_e = pool.not(e);
        let a_or_b = pool.or([a, b]);
        let mut tbox = AbsorbedTBox {
            concept_rules: vec![ConceptRule {
                trigger: a_cls,
                conclusion: not_e,
            }],
            ..AbsorbedTBox::default()
        };
        tbox.finalize();

        let mut ctx = TableauContext::with_tbox(&pool, &tbox);
        ctx.set_complement(a, not_a);
        let n = ctx.new_node();
        let outer_id = outer.then(|| ctx.push_branch());
        ctx.add_label_with_deps(n, e, outer_id.as_slice());
        ctx.add_label_with_deps(n, a_or_b, &[]);
        assert!(matches!(search(&mut ctx, 64), SearchVerdict::Sat));
        assert!(
            ctx.graph().node(n).labels().binary_search(&b).is_ok(),
            "the search settles on B"
        );
        let deps = ctx.label_deps_of(n, not_a).expect("¬A is asserted for B");
        (deps.to_vec(), outer_id)
    }

    #[test]
    fn a_failed_disjuncts_complement_carries_its_clash_deps() {
        // Control: only the branch itself caused the clash.
        let (deps, _) = complement_deps(false);
        assert_eq!(deps.len(), 1, "control complement deps {deps:?}");

        let (deps, outer) = complement_deps(true);
        let outer = outer.expect("outer decision");
        assert!(
            deps.contains(&outer),
            "complement deps {deps:?} miss decision {outer}"
        );
        assert_eq!(deps.len(), 2, "complement deps {deps:?}");
    }
}

/// #370: exact `≤n` merge deps (`RUSTDL_MERGE_EXACT_DEPS`).
#[cfg(test)]
#[allow(clippy::many_single_char_names)]
mod exact_dep_tests {
    use super::{Frame, first_open_max};
    use crate::TableauContext;
    use owl_dl_core::{ClassId, ConceptPool, Role, RoleId};

    #[test]
    fn a_mark_keeps_its_deps_and_undo_restores_both_vectors() {
        let pool = ConceptPool::new();
        let mut ctx = TableauContext::new(&pool);
        ctx.set_merge_exact_deps(true);
        let a = ctx.new_node();
        let b = ctx.new_node();
        let cp = ctx.checkpoint();
        ctx.mark_distinct_with_deps(a, b, &[3]);
        assert_eq!(ctx.distinct_deps(a, b).as_slice(), &[3]);
        assert_eq!(ctx.distinct_deps(b, a).as_slice(), &[3]);
        // A second reason keeps the first mark's deps.
        ctx.mark_distinct_with_deps(a, b, &[5]);
        assert_eq!(ctx.distinct_deps(a, b).as_slice(), &[3]);
        ctx.rollback_to(cp);
        for x in [a, b] {
            assert!(ctx.graph().node(x).inequalities.is_empty());
            assert!(ctx.graph().node(x).inequality_deps.is_empty());
        }
    }

    #[test]
    fn flag_off_records_no_deps() {
        let pool = ConceptPool::new();
        let mut ctx = TableauContext::new(&pool);
        ctx.set_merge_exact_deps(false);
        let a = ctx.new_node();
        let b = ctx.new_node();
        ctx.mark_distinct_with_deps(a, b, &[3]);
        assert!(ctx.are_distinct(a, b));
        assert!(ctx.distinct_deps(a, b).is_empty());
        assert_eq!(ctx.graph().node(a).inequality_deps.len(), 1);
    }

    #[test]
    fn a_carried_mark_depends_on_the_mark_and_the_merge() {
        let pool = ConceptPool::new();
        let mut ctx = TableauContext::new(&pool);
        ctx.set_merge_exact_deps(true);
        let a = ctx.new_node();
        let b = ctx.new_node();
        let c = ctx.new_node();
        ctx.mark_distinct_with_deps(a, c, &[1]);
        assert!(ctx.merge_into_with_deps(a, b, &[2]));
        assert!(ctx.are_distinct(b, c));
        assert_eq!(ctx.distinct_deps(b, c).as_slice(), &[1, 2]);
    }

    #[test]
    fn fresh_min_witnesses_are_distinct_under_the_min_label() {
        let mut pool = ConceptPool::new();
        let r = RoleId::new(0);
        let c = pool.atomic(ClassId::new(0));
        let at_least_two = pool.min(2, Role::Named(r), c);
        let mut ctx = TableauContext::new(&pool);
        ctx.set_merge_exact_deps(true);
        let n = ctx.new_node();
        let d = ctx.push_branch();
        let _unrelated = ctx.push_branch();
        ctx.add_label_with_deps(n, at_least_two, &[d]);
        crate::rules::apply_min(&mut ctx, n);
        let ws = crate::rules::max_witnesses(&ctx, n, Role::Named(r), c);
        assert_eq!(ws.len(), 2);
        assert_eq!(ctx.distinct_deps(ws[0], ws[1]).as_slice(), &[d]);
    }

    /// `≤1 r.C` over three witnesses where the first two are distinct under
    /// decision `d`; `u` is an unrelated open decision. The pivot is the third
    /// witness with no distinct option (`earlier = 2 ≥ 1`), so exhaustion rests
    /// on the label, the witnesses and the mark: `d` and the pivot's own
    /// decision `e`, but not `u`.
    fn p4_initial(exact: bool) -> (Vec<u32>, u32, u32, u32) {
        let mut pool = ConceptPool::new();
        let r = RoleId::new(0);
        let c = pool.atomic(ClassId::new(0));
        let at_most_one = pool.max(1, Role::Named(r), c);
        let mut ctx = TableauContext::new(&pool);
        ctx.set_merge_exact_deps(exact);
        let n = ctx.new_node();
        ctx.add_label_with_deps(n, at_most_one, &[]);
        let u = ctx.push_branch();
        let d = ctx.push_branch();
        let e = ctx.push_branch();
        let ws: Vec<_> = [&[][..], &[], &[e]]
            .into_iter()
            .map(|deps| {
                let w = ctx.new_successor_with_deps(n, r, deps);
                ctx.add_label_with_deps(w, c, deps);
                w
            })
            .collect();
        ctx.mark_distinct_with_deps(ws[0], ws[1], &[d]);
        let open = first_open_max(&ctx).expect("an open ≤1 with two mergeable pairs");
        assert_eq!(open.pivot, ws[2]);
        let frame = Frame::merge(&mut ctx, 64, open);
        (frame.initial.to_vec(), u, d, e)
    }

    /// The frame's distinct option marks the pivot distinct from each partner
    /// under the frame's own decision and the witnesses' deps.
    #[test]
    fn the_distinct_option_marks_under_the_frame_decision() {
        let mut pool = ConceptPool::new();
        let r = RoleId::new(0);
        let c = pool.atomic(ClassId::new(0));
        let at_most_two = pool.max(2, Role::Named(r), c);
        let mut ctx = TableauContext::new(&pool);
        ctx.set_merge_exact_deps(true);
        let n = ctx.new_node();
        ctx.add_label_with_deps(n, at_most_two, &[]);
        let w_dec = ctx.push_branch();
        for _ in 0..3 {
            let w = ctx.new_successor_with_deps(n, r, &[w_dec]);
            ctx.add_label_with_deps(w, c, &[w_dec]);
        }
        let open = first_open_max(&ctx).expect("an open ≤2 over three witnesses");
        let (pivot, partners): (_, Vec<_>) =
            (open.pivot, open.partners.iter().map(|(w, _)| *w).collect());
        assert!(open.earlier < open.bound, "the distinct option is offered");
        let mut frame = Frame::merge(&mut ctx, 64, open);
        // Skip the merge options: start the distinct one directly.
        frame.next = partners.len();
        assert!(matches!(
            frame.start_option(&mut ctx),
            super::Start::Child(..)
        ));
        for w in partners {
            let deps = ctx.distinct_deps(pivot, w);
            assert!(deps.contains(&frame.my_id), "{deps:?} misses the frame");
            assert!(deps.contains(&w_dec), "{deps:?} misses the witnesses");
        }
    }

    #[test]
    fn p4_exhaustion_depends_on_the_marks_not_on_unrelated_decisions() {
        let (initial, u, d, e) = p4_initial(true);
        assert!(
            initial.contains(&d),
            "{initial:?} misses the mark's decision"
        );
        assert!(
            initial.contains(&e),
            "{initial:?} misses the pivot's existence"
        );
        assert!(
            !initial.contains(&u),
            "{initial:?} carries an unrelated decision"
        );
        let (initial, u, d, _) = p4_initial(false);
        assert!(initial.contains(&d) && initial.contains(&u), "{initial:?}");
    }

    /// `≤1 r.C` over three witnesses, `w0 ≠ w1` under `d1` and `w0 ≠ w2` under
    /// `d2`: one mergeable pair, so `apply_max` forces `w2` into `w1`. The moved
    /// marker must depend on both marks, and (exact) not on the unrelated `u`.
    fn p7_moved_deps(exact: bool) -> (Vec<u32>, [u32; 3]) {
        let mut pool = ConceptPool::new();
        let r = RoleId::new(0);
        let c = pool.atomic(ClassId::new(0));
        let marker = pool.atomic(ClassId::new(1));
        let at_most_one = pool.max(1, Role::Named(r), c);
        let mut ctx = TableauContext::new(&pool);
        ctx.set_merge_exact_deps(exact);
        let n = ctx.new_node();
        ctx.add_label_with_deps(n, at_most_one, &[]);
        let u = ctx.push_branch();
        let d1 = ctx.push_branch();
        let d2 = ctx.push_branch();
        let ws: Vec<_> = (0..3)
            .map(|_| {
                let w = ctx.new_successor_with_deps(n, r, &[]);
                ctx.add_label_with_deps(w, c, &[]);
                w
            })
            .collect();
        ctx.add_label_with_deps(ws[2], marker, &[]);
        ctx.mark_distinct_with_deps(ws[0], ws[1], &[d1]);
        ctx.mark_distinct_with_deps(ws[0], ws[2], &[d2]);
        crate::rules::apply_max(&mut ctx, n);
        let holder = ctx.resolve(ws[2]);
        assert_eq!(holder, ws[1], "the one mergeable pair was merged");
        let deps = ctx
            .label_deps_of(holder, marker)
            .expect("moved marker")
            .to_vec();
        (deps, [u, d1, d2])
    }

    #[test]
    fn p7_forced_merge_depends_on_the_excluding_marks_only() {
        if !crate::max_choice_point_enabled() {
            return;
        }
        let (deps, [u, d1, d2]) = p7_moved_deps(true);
        assert!(deps.contains(&d1) && deps.contains(&d2), "{deps:?}");
        assert!(!deps.contains(&u), "{deps:?} carries an unrelated decision");
        let (deps, [u, ..]) = p7_moved_deps(false);
        assert!(deps.contains(&u), "{deps:?}");
    }
}

/// DIAGNOSTIC (#382, not for merge): counters over one process, printed every
/// 20,000 decisions when `RUSTDL_DIAG382` is set.
mod diag {
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

    static OPENS: [AtomicU64; 3] = [const { AtomicU64::new(0) }; 3];
    static SKIPS: AtomicU64 = AtomicU64::new(0);
    static EXHAUSTS: AtomicU64 = AtomicU64::new(0);
    static EXHAUST_OPTS: AtomicU64 = AtomicU64::new(0);
    static CLASHES: AtomicU64 = AtomicU64::new(0);
    static CLASH_DEPS: [AtomicU64; 6] = [const { AtomicU64::new(0) }; 6];
    static SITES: Mutex<Option<HashMap<u64, u32>>> = Mutex::new(None);

    fn on() -> bool {
        std::env::var_os("RUSTDL_DIAG382").is_some()
    }

    fn show(pool: &owl_dl_core::ConceptPool, id: owl_dl_core::ConceptId, d: usize) -> String {
        use owl_dl_core::{ConceptExpr as E, Role};
        if d > 4 {
            return "…".into();
        }
        let r = |r: &Role| match r {
            Role::Named(x) => format!("r{}", x.index()),
            Role::Inverse(x) => format!("r{}⁻", x.index()),
        };
        let list = |xs: &[owl_dl_core::ConceptId], sep: &str| {
            xs.iter().map(|&x| show(pool, x, d + 1)).collect::<Vec<_>>().join(sep)
        };
        match pool.get(id) {
            E::Top => "⊤".into(),
            E::Bot => "⊥".into(),
            E::Atomic(c) => format!("C{}", c.index()),
            E::Nominal(i) => format!("{{i{}}}", i.index()),
            E::SelfRestriction(x) => format!("∃{}.Self", r(x)),
            E::Not(x) => format!("¬{}", show(pool, *x, d + 1)),
            E::And(xs) => format!("({})", list(xs, " ⊓ ")),
            E::Or(xs) => format!("({})", list(xs, " ⊔ ")),
            E::Some(x, c) => format!("∃{}.{}", r(x), show(pool, *c, d + 1)),
            E::All(x, c) => format!("∀{}.{}", r(x), show(pool, *c, d + 1)),
            E::Min(n, x, c) => format!("≥{n} {}.{}", r(x), show(pool, *c, d + 1)),
            E::Max(n, x, c) => format!("≤{n} {}.{}", r(x), show(pool, *c, d + 1)),
        }
    }

    pub(super) fn open(pool: &owl_dl_core::ConceptPool, kind: usize, node: u32, concept: u32) {
        if !on() {
            return;
        }
        OPENS[kind].fetch_add(1, Relaxed);
        let key = ((kind as u64) << 60) | (u64::from(node) << 32) | u64::from(concept);
        let mut g = SITES.lock().expect("diag lock");
        *g.get_or_insert_with(HashMap::new).entry(key).or_insert(0) += 1;
        let total: u64 = OPENS.iter().map(|a| a.load(Relaxed)).sum();
        if total % 20_000 == 0 {
            let m = g.as_ref().expect("map");
            let mut counts: Vec<u32> = m.values().copied().collect();
            counts.sort_unstable_by(|a, b| b.cmp(a));
            let top: Vec<u32> = counts.iter().take(8).copied().collect();
            // Opens by decision concept (kind, concept), summed over nodes.
            let mut by_c: HashMap<(u64, u64), u64> = HashMap::new();
            for (k, v) in m {
                *by_c.entry((k >> 60, k & 0xffff_ffff)).or_insert(0) += u64::from(*v);
            }
            let mut by_c: Vec<_> = by_c.into_iter().collect();
            by_c.sort_by(|a, b| b.1.cmp(&a.1));
            for ((kind, c), v) in by_c.iter().take(8) {
                let c = owl_dl_core::ConceptId::new(u32::try_from(*c).expect("id"));
                eprintln!("#   kind={kind} opens={v} {}", show(pool, c, 0));
            }
            let o: Vec<u64> = OPENS.iter().map(|a| a.load(Relaxed)).collect();
            let cd: Vec<u64> = CLASH_DEPS.iter().map(|a| a.load(Relaxed)).collect();
            let ex = EXHAUSTS.load(Relaxed);
            eprintln!(
                "# diag382 opens[or,choose,max]={o:?} sites={} top_site_opens={top:?} skips={} indep_skips={} exhausts={ex} avg_opts_at_exhaust={:.2} clashes={} clash_deps[0-1,2-4,5-9,10-19,20-39,40+]={cd:?}",
                m.len(),
                SKIPS.load(Relaxed),
                INDEP_SKIPS.load(Relaxed),
                EXHAUST_OPTS.load(Relaxed) as f64 / ex.max(1) as f64,
                CLASHES.load(Relaxed),
            );
        }
    }

    static INDEP_SKIPS: AtomicU64 = AtomicU64::new(0);

    pub(super) fn skip_kind(independent: bool) {
        if on() {
            SKIPS.fetch_add(1, Relaxed);
            if independent {
                INDEP_SKIPS.fetch_add(1, Relaxed);
            }
        }
    }

    pub(super) fn exhaust(options: usize) {
        if on() {
            EXHAUSTS.fetch_add(1, Relaxed);
            EXHAUST_OPTS.fetch_add(options as u64, Relaxed);
        }
    }

    pub(super) fn clash(deps: usize) {
        if on() {
            CLASHES.fetch_add(1, Relaxed);
            let b = match deps {
                0..=1 => 0,
                2..=4 => 1,
                5..=9 => 2,
                10..=19 => 3,
                20..=39 => 4,
                _ => 5,
            };
            CLASH_DEPS[b].fetch_add(1, Relaxed);
        }
    }
}
