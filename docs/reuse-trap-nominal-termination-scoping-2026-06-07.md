# Unified scoping: reuse-trap + nominal-termination

Drafted 2026-06-07 as a **fresh-session seed**. This is a multi-session
undertaking; this doc is the entry point so a clean session can start without
re-deriving the analysis below. Format mirrors `model-caching-plan.md` /
`moms-plan.md`.

> ## ⚠ MEASUREMENT UPDATE 2026-06-07 — the "nominal-termination" half of this
> ## thesis is REFUTED. Read this before the rest of the doc.
>
> A worktree measurement (instrumented the wedge on 4 distinct hard wine
> sub-classes, depth-256, 5 s cap) found the wedge's non-termination on wine is
> **NOT** model growth, a blocking gap, or construction cost. It is a
> **backtracking search explosion**:
> - **node count stays at 10** (the per-branch model is finite and terminates in
>   0.0 ms) while **`branches_taken` climbs to 13k–21k**, **`restores ==
>   branches`** (every branch fails and is undone), **~20M `match_attempts`**.
> - `is_blocked` fired **0** times — nothing to block (9 non-root nodes). So
>   nominal-aware blocking would do **nothing** here.
> - The branching is **disjunction + `≤n`-merge**, not nominals — and in fact
>   nominals aren't even wired into the production wine wedge path
>   (`HyperCache::decide` at `lib.rs:909` sets `with_double_blocking` +
>   `with_precise_card_deps` only, NOT `with_nominals`).
>
> **Consequence — re-scope:** the wine *wall* is a **search-pruning problem**
> (disjunction + `≤n`-merge branching, ~20M match attempts driven partly by the
> full `save()` graph-clone-per-branch), NOT terminating model construction.
>
> **But this lever is largely already explored — and mostly NEGATIVE. Read
> before assuming "do CDCL".** Prior work (2026-06-06, memory
> `conflict-learning-simple-is-weak`; branches `feat/conflict-learning` (PR #19
> foundation, ~490-line `hyper.rs` nogood store + decision stack, behind
> `RUSTDL_HYPER_LEARNING`, default OFF) and `feat/1uip-spike`;
> `docs/conflict-learning-inc1-results-2026-06-06.md`):
> - **Simple dep-set nogood learning gave −13.5% branches but 0 wine classes
>   un-stalled** — the recurring clashes are *leaves*, so each prune saves ~1
>   branch, never a subtree. Do NOT re-attempt the simple form.
> - Wine's stalls were measured as **`≤n` cardinality** clashes; the cardinality
>   half was addressed (completeness, not wall) by the shipped
>   `RUSTDL_PRECISE_CARD_DEPS` (34→31).
> - The note's standing guidance: **1-UIP may still matter ONLY for
>   non-cardinality *disjunction* stalls — measure the conflict structure first.**
>
> **The worktree measurement supplies exactly that structure** and it is the one
> piece of *new, positive* evidence: across the 4 hard subs, `disj_branches` vs
> `merge_branches` ranges from balanced (8833/4890) to **disjunction-dominated
> (18631/2704)**. So a disjunction-dominated subset *does* exist — the only place
> 1-UIP (asserting clauses → subtree pruning, where simple dep-set nogoods
> couldn't) has a chance.
>
> **The two halves therefore SPLIT (they are more separable than this doc
> claimed), not unify:**
> 1. **Wine wall / search-explosion** → the ONLY unexplored sliver is **1-UIP
>    asserting-clause learning on the disjunction-dominated stalls** (simple
>    dep-set learning already failed; cardinality half already shipped). Start
>    from `feat/1uip-spike` / `feat/conflict-learning` (both STALE — rebase onto
>    current main first). This is approach (C) re-scoped — nothing to do with
>    nominals or blocking. **Treat as speculative**: the prior note is skeptical,
>    and even 1-UIP only helps if the disjunction-dominated subtrees are deep
>    enough that an asserting clause prunes more than a leaf.
> 2. **Model-reuse generalization (reuse-trap)** → snapshot-replay soundness
>    under back-propagation (approach (A) below) — still stands as written, an
>    independent problem.
>
> **Open sub-question — RESOLVED by existing data.** The hard stalled pairs are
> **non-subsumptions** (wine classify MISSED=0 vs HermiT ⟹ every stalled pair
> that defaults to "not subsumed" is genuinely not subsumed ⟹ `C⊓¬D` is
> **satisfiable**). So it's **Sat-pathological**, not Unsat-grind: the wedge is
> failing to *find an existing model*. Residual nuance for the fresh session
> (cheap to check): is that failure pathological search *order* (→ 1-UIP/ordering
> helps) or wedge *incompleteness* — note nominals are NOT wired into
> `HyperCache::decide`, so on a genuinely nominal-dependent model the wedge may
> never close regardless of search effort, which would point at wiring
> `with_nominals` into that path rather than (or before) 1-UIP.
>
> The original thesis and "approach (C) = terminating model construction" framing
> below are kept as the historical reasoning trail, but treat them as superseded
> by this block. Diagnostic instrumentation that produced these numbers lives in
> worktree branch `worktree-agent-aa2bcc7a5e964341c` (`SearchStats.stall_site`,
> `diag_block_analysis`, the extended `wine_wedge_construct_vs_solve_probe`, plus
> `wine_wedge_long_deadline_nominals_probe`).
>
> ## GO/NO-GO RESULT 2026-06-08 — DEFERRED (not built), 1-UIP still OPEN. One firm sub-result (nominals refuted).
>
> *(An earlier draft of this block over-claimed "NO-GO / accept the gap /
> architectural"; corrected after advisor review + an oracle check. What's
> below is the corrected, evidence-bounded verdict.)*
>
> The measurement (`wine_wedge_long_deadline_nominals_probe`, 60 s each config):
> - **(A) production config (no nominals), 60 s:** `Stalled`, **168 246 branches**
>   (vs ~14 k at 5 s → linear ~2800/s climb), `restores==branches`, `nodes` flat
>   at 10, **disjunction-dominated** (107 834 disj / 60 412 merge).
> - **(B) nominals + sub-roles wired, 60 s:** `Stalled`, **166 206 branches** —
>   essentially identical. **Wiring `with_nominals` does NOT help** — this sub-
>   result is FIRM (refutes the incompleteness hypothesis; nominals are not the
>   wine-wall lever).
>
> **Sat/Unsat of the hard pairs — VERIFIED.** The hard pairs found are
> `food#BlandFishCourse ⊑? {BlandFish, CheeseNutsDessert, DarkMeatFowl, Dessert,
> …}` — a *course* vs unrelated *food/dish* classes. The HermiT oracle
> (`wine-classified.owx`) has **no** such `SubClassOf` → they are genuine
> **non-subsumptions (Sat)**, i.e. representative wall pairs. So `C⊓¬D` is
> **satisfiable with a ~10-node model that the wedge never reaches in 168k+
> branches**.
>
> **Corrected verdict — DEFERRED, not NO-GO. 1-UIP remains the OPEN candidate.**
> The earlier "leaves ⟹ 1-UIP can't help" was **backwards**: resolving leaf-level
> dep-set conflicts into short asserting clauses that fire high and prune subtrees
> is exactly 1-UIP's *purpose*; the leaf-ness of the simple nogoods is its
> *motivation*. And the wall pair is **disjunction-dominated** — the regime the
> prior note (`conflict-learning-simple-is-weak`) flagged as "1-UIP may matter."
> A confirmed-Sat tiny (10-node) model that backtracking can't find in 168k
> branches is **itself a lever to chase**, not a dead end. Candidates that remain
> open: (i) 1-UIP asserting-clause learning on disjunction-dominated stalls;
> (ii) model-guided / better branch ordering (distinct from MOMS's reverted static
> heuristic). The architectural alternative (§2 global model construction) is the
> bigger fallback, not the only option.
>
> **Before building 1-UIP, the fresh session must first establish** whether the
> 168k disjunction-level clashes share structure an asserting clause can exploit
> (do conflict dep-sets span shallow decision levels a 1-UIP backjump would reach,
> or are they all deep/independent?). That's the real go/no-go, and it is *not yet
> measured*.
>
> **Practical answer (already shipped):** `--pair-timeout-ms 25` — these pairs
> find nothing regardless, so cutting the budget is free (wine 7.5×, MISSED=0).
> **Session-management note:** don't build 1-UIP now (defer to a fresh session);
> this is "not built," NOT "proven impossible." The reuse-trap half (approach (A))
> also remains open and independent.

## The thesis: two threads are one problem

Two long-standing levers turn out to share a single prerequisite:

- **(Reuse-trap)** Make HermiT/Konclude-style **global model reuse** sound. The
  snapshot cache (Phase 1b/1c) reuses a per-class satisfiability model to answer
  many `(C, *)` subsumption probes cheaply — but only on the
  `BackPropRisk::Safe` (inverse/nominal/cardinality-free) fragment, because
  reuse is unsound when `¬D` **back-propagates** into the cached graph.
- **(Nominal-termination)** Make the hypertableau **wedge terminate** on the
  nominal+cardinality fragment. Today it *Stalls* (the wine wall); HermiT builds
  the same model in seconds.

**They are the same problem because both need a *terminating, reusable model on
the inverse/nominal/cardinality fragment*.** You cannot reuse a model you cannot
finish building (nominal-termination is the prerequisite); and once you can
build it, sound reuse on that fragment is exactly the reuse-trap. Solving the
pair buys **both** the wine/nominal completeness *and* the generic
model-reuse orchestration win (kills the O(n²) per-pair redundancy that makes
wine 412 s where HermiT takes seconds).

## What is already known (do not re-derive)

**The reuse infrastructure exists and is sound on Horn.**
- Snapshot capture: `HyperEngine::satisfiability_snapshot` (`hyper.rs:1069`).
- Lazy replay: `replay::replay_with_neg_sup` (`replay.rs:69`) seeds an engine
  `from_snapshot_lazy`, injects `¬sup`, re-fires only *new* triggers
  (fingerprint skip).
- Gate: `SnapshotCache::build` stamps `BackPropRisk::classify_ontology`
  (`lib.rs:1053`); `try_replay` (`lib.rs:1122`) returns `None` unless
  `is_safe()`. Verdicts: `ReplayVerdict::{Subsumed, NotSubsumed,
  BackPropAborted, Stalled}` (`replay.rs:29`).
- Runtime sentinel: `snapshot_backprop_aborted` (`hyper.rs:434`, set at
  `hyper.rs:729` when a label reaches a `snapshot_origin` node).

**The structural gate is LOAD-BEARING for soundness — do NOT loosen it.**
`hypertableau-dead-ends.md` §19 (recon) + §20 (impl), re-confirmed by a
throwaway spike 2026-06-07: forcing `risk=Safe` unconditionally (sentinel-only)
gives **pizza FP=100, ro FP=275, sio FP=225** vs HermiT. The sentinel is **not
a complete guard** — unsound reuse surfaces as a false `Subsumed` (FP), not
merely MISSED. Per-class gating is also unsound (§20 Failure 1: the *`sup`*'s
definition embeds cardinality the snapshot wasn't built for) and regresses
SROIQ wall (§20 Failure 2). See [[snapshot-gate-loosening-dead-end]].

**The wedge does not terminate on wine; construction is cheap, search isn't.**
Probe `wine_wedge_construct_vs_solve_probe` (`reasoner/src/lib.rs` tests):
`clauses.clone()`=0.1 ms, `HyperEngine::new`=0.0 ms, **`solve` (5 s cap) =
Stalled** on every hard wine pair (661 clauses). So the cost is the *search*,
not setup. The label oracle (Phase 7) — itself a cheap global-reuse-for-
non-subsumptions — terminates for *all* pizza classes (`misses=0`) but stalls
on ~1875 wine pairs (`label_cache_misses`), exactly the nominal classes.

**HermiT returns on wine in seconds** (it produced our MISSED=0 oracle), so this
is not intrinsic hardness — it's our nominal handling + per-pair orchestration.
HermiT reuses *through* back-prop (re-fires correctly); we *abort* on it.

**The reuse-trap triggers** (spec
`docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md`
§4.2, Inv-1): `¬D` back-propagation via inverse (`∀R⁻.X` reaching root), nominal
coupling (`{a}` merging cached nodes), cardinality merges (`≤n R`).

## Approaches (for the fresh session to weigh)

1. **(C) Terminating nominal model construction — the prerequisite.** Fix the
   wedge so it returns a *finite* `Sat` model on wine instead of Stalling.
   Blocking machinery: `is_blocked` (`hyper.rs:780`), `with_double_blocking`
   (`hyper.rs:571`, required for Sat-soundness with inverse roles),
   `apply_nn_rule` (nominal merge). Likely root: blocking doesn't terminate the
   nominal+cardinality model (nominal nodes can't be subset-blocked like
   anonymous ones; need nominal-aware / pairwise blocking). **Start here** — it
   gates everything else and is the more self-contained sub-problem.

2. **(A) Sound replay under back-propagation (the reuse-trap proper).** Once a
   model terminates, make lazy replay correct when `¬D` back-propagates —
   i.e. re-fire affected snapshot rules correctly instead of `BackPropAborted`.
   This is HermiT's reuse-through-back-prop. Gate EVERY increment on FP=0 (the
   §20 spike proved this path produces FP if done wrong).

3. **(B) Konclude-style sub-tableau caching (`dead-ends §2`).** A structurally
   different engine/cost-profile than the snapshot path. Larger rewrite;
   consider only if (A)+(C) prove intractable.

**Out of scope / known not-this-lever:** loosening the gate (§19/§20); the
ore-15672 hard-class search-budget cluster (§18 — needs multi-class search, not
reuse); model-validation as a Sat-soundness path (§9).

## Recommended first step (fresh session)

Instrument **why the wedge doesn't terminate on a wine class**, before any
build. Extend `wine_wedge_construct_vs_solve_probe` to dump, at the 5 s stall:
graph node count over time, `is_blocked` fire rate, `SearchStats`
(`max_branch_depth`, `node_clones`, `restores`), and whether it hits the
`depth==0` disjunction bound (`hyper.rs:1284`) vs just unbounded growth. That
single measurement decides whether termination is a **blocking-completeness**
fix (blocking should fire but doesn't on nominal cycles) or a **genuine
model-infinity** that needs nominal-aware blocking — and tells you if (C) is
days or weeks.

## Soundness guardrails (non-negotiable)

FP=0 is the cardinal invariant. Every increment in (A) and (C) must pass the
full corpus closure-diff vs HermiT/Konclude — **FP=0 AND MISSED=0** — across
pizza, ore-10908, ore-15672, sio, wine, ro/sulo-stripped, shoiq-knowledge,
alehif (harness: `tests/konclude_closure_diff.rs`, now sweepable via
`RUSTDL_TEST_PAIR_MS`). The §20 spike is the cautionary tale: a "sound by
sentinel" assumption produced FP=100. Prove reuse soundness; never assume it.

## Pointers

- Code: `crates/owl-dl-tableau/src/{hyper.rs,snapshot.rs,replay.rs}`;
  `crates/owl-dl-reasoner/src/lib.rs` (`SnapshotCache`, `classify.rs`
  `subsumes_via_tableau`).
- Docs: `model-caching-plan.md`, `hypertableau-dead-ends.md` §2/§9/§18/§19/§20,
  the snapshot spec (`…/2026-06-03-konclude-style-global-classification-design.md`),
  `handoff-2026-06-03-snapshot-cache-project-complete.md`,
  `perf-attribution-2026-06-07.md` (the wine wedge-stall measurement).
- Memory: [[snapshot-gate-loosening-dead-end]],
  [[sub-tableau-caching-already-shipped]], [[perf-frontier-attributed]],
  [[corpus-parity-achieved]].
