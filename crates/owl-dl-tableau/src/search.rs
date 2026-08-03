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
pub fn search(ctx: &mut TableauContext<'_, '_, '_>, max_depth: usize) -> SearchVerdict {
    if max_depth == 0 {
        // Genuine depth-cap bottom-out — the harm the adaptive early-abandon
        // meters. Kept FIRST so `check_deadline`'s sticky side effect stays
        // short-circuited exactly as before.
        ctx.note_depth_cap_hit();
        if trace_enabled() {
            eprintln!("# trace search depth=0_or_deadline");
        }
        return SearchVerdict::DepthLimit;
    }
    // `early_abandoned` is a latched flag and is `false` whenever the lever is
    // unarmed, so flag-OFF this is verbatim the previous predicate.
    if ctx.check_deadline() || ctx.early_abandoned() {
        if trace_enabled() {
            eprintln!("# trace search depth=0_or_deadline");
        }
        return SearchVerdict::DepthLimit;
    }
    match saturate(ctx, SATURATE_ITERS) {
        SaturationResult::Clash(_, deps) => {
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
            // Step 1: ⊔ branching has priority — it's structurally
            // cheaper and keeps the search shape predictable.
            if let Some((node, _or_label, disjuncts, or_deps)) = first_open_disjunction(ctx) {
                if trace_enabled() {
                    eprintln!(
                        "# trace search depth={max_depth} disj node={} options={} graph_nodes={}",
                        node.index(),
                        disjuncts.len(),
                        ctx.graph().len()
                    );
                }
                return branch(ctx, max_depth, node, &disjuncts, &or_deps);
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
                return branch(ctx, max_depth, node, &[c, c_neg], &DepSet::new());
            }
            if trace_enabled() {
                eprintln!(
                    "# trace search depth={max_depth} sat graph_nodes={}",
                    ctx.graph().len()
                );
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

    let total_opts = ordered.len();
    for (opt_idx, d) in ordered.iter().enumerate() {
        if early_return.is_some() {
            break;
        }
        if trace_enabled() {
            eprintln!(
                "# trace branch depth={max_depth} my_id={my_id} pick={}/{total_opts} disj={}",
                opt_idx + 1,
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
        // CDBL Phase 1 (docs/cdbl-plan.md): record which disjunct
        // concept this branch is asserting, so a downstream clash's
        // DepSet can be translated to the structural set of disjunct
        // concepts that caused it. Overwrites the entry for `my_id`
        // each iteration — at clash time it reflects the disjunct
        // currently under trial, which is exactly the one in scope.
        // Phase-1 bookkeeping only; no lookup acts on it yet.
        ctx.record_decision(my_id, node, *d);
        // The labelled disjunct depends on *this* branch decision and
        // every reason the parent disjunction was at this node.
        ctx.add_label_with_deps(node, *d, combined_deps.as_slice());
        let verdict = search(ctx, max_depth - 1);
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
            SearchVerdict::NodeCap => {
                // Hard early-return: a global node-cap trip means the
                // whole search is too expensive to continue — abandon
                // the remaining sibling disjuncts rather than trying
                // them (Task 1's soft handling let an exploding search
                // re-grow the graph per sibling, which was slow). This
                // is sound: NodeCap maps to `Ok(None)` in `decide` (a
                // sound MISS / consistent-under-approx), and giving up
                // earlier only ever yields a MISS, never a false
                // positive.
                ctx.rollback_to(cp);
                early_return = Some(SearchVerdict::NodeCap);
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
        if ctx.note_branch_trial(definite) && early_return.is_none() {
            early_return = Some(SearchVerdict::DepthLimit);
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
