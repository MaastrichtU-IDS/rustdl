# Within-search state caching (Lever #2) — design

**Date:** 2026-06-19
**Status:** approved (brainstorming session 2026-06-19; P0 gate PASSED)
**Program:** "Konclude-class engine" — perf Lever #2 (the transformative one)
**Context:** `docs/superpowers/specs/2026-06-19-perf-frontier-levers-scoping.md`,
memory `sp2-perf-attribution-2026-06-19`. Lever #1 (adaptive budget) shipped a
modest sound win; #2 targets the disjunctive-branch-count wall that #1 can only
early-cut.

---

## 1. The opportunity (measured, not speculated)

The wedge's disjunctive search thrashes: on `e-interaction-situation`
(ore-15672-shoin) it explores **557,692 branches but visits only 48 distinct
full graph-states** (P0 probe, sound key = labels + edges + preds + neq +
nominals; ratio ≈ 0.00017). The same ~48 states are revisited hundreds of
thousands of times. **Memoizing the verdict per full graph-state would hit a
cached result on ~99.98% of branches** — collapsing these searches from minutes
to (near-)instant, potentially making them *terminate* instead of timing out.

## 2. Why this is sound BY CONSTRUCTION (not the FP-delicate caching problem)

The classic "tableau caching is unsound with inverse roles / nominals" hardness
is about caching a **sub-graph's / node's** satisfiability *independent of its
context* (a back-edge can change the verdict). **That is not what we're doing.**
The wedge's `solve` does **whole-graph save/restore** and recurses on the *entire*
graph (`hyper.rs` `fn solve`: per-disjunct `self.save()` → `apply_head_atom` →
`self.solve(depth-1)` → `self.restore`). The cacheable unit is therefore the
**full graph state**, which has no external context to get wrong:

> identical full graph state + fixed clause set ⇒ `solve` is a deterministic
> function ⇒ identical verdict.

So whole-graph-keyed memoization is **sound by construction**. The two apparent
landmines dissolve:

- **Depth:** cache only **decisive** verdicts (`Unsat`, `Sat`); **never `Stalled`**.
  `Unsat` ("no model exists") and `Sat` ("model found") are absolute — depth only
  governs whether the search could *decide* them, so a decisive verdict is valid
  at any depth. A `Stalled` is depth-relative and must not be cached.
- **Dependency-directed backjumping / dep-sets:** **do not cache the dep-set.** On
  a cached-`Unsat` hit, return a **conservative** dep-set (`DepSet::ALL` / all
  currently-active decision levels). A superset clash-dep only *disables backjump
  pruning for that hit* — it can never skip a branch that mattered, so it is sound
  (the existing `precise_card_deps`/`DepSet::ALL` fallbacks rely on exactly this
  monotonicity). We keep the whole re-exploration saving; we forfeit only pruning
  *on cache hits* (a second-order perf detail, not correctness).

**The entire remaining soundness obligation is KEY-COMPLETENESS** (§4) — a finite
audit, backstopped by the corpus closure-identity net + adversarial review.

## 3. Architecture

A per-search memo table on `HyperEngine`:
```
cache: HashMap<u64 /*full-state key*/, CachedVerdict>
enum CachedVerdict { Unsat, Sat }   // never Stalled
```
- **Lifetime:** one `decide()`/`decide_with_deadline()` call. Cleared at entry
  (`self.cache.clear()` alongside `self.stats = SearchStats::default()`). It is a
  **within-search** cache — NOT cross-pair (the cross-pair snapshot cache died
  FP-unsound; this is categorically different — see §5).
- **Lookup:** at the top of `solve` (after the deadline + adaptive-budget checks,
  before `horn_fixpoint`): compute `k = full_state_key()`; if `cache.get(k)` is
  `Some(Unsat)` → set `clash_deps = DepSet::ALL` (conservative) and return `Unsat`;
  `Some(Sat)` → return `Sat`.
- **Insert:** when `solve` is about to return a **decisive** verdict for the
  current state, `cache.insert(k, verdict)`. (Insert at the return sites that
  produce `Unsat`/`Sat`; skip `Stalled`.)
- **Gated:** `RUSTDL_WEDGE_CACHE` (default decided by the gate; likely default-ON
  once verdict-identity + perf confirmed, like Lever #1).

## 4. The key — completeness is the whole game

`full_state_key()` must hash **everything `solve` / `horn_fixpoint` /
`apply_head_atom` read from `self` that can affect the verdict.** Canonical
(union-find-resolved so merges don't create spurious distinctness;
order-independent across nodes). Audit checklist (a missed field = a wrong hit =
**FP**):
- per node (by representative): sorted **labels** (class ids); sorted **edges**
  `(role, target_rep)`; sorted **preds** `(role, source_rep)`;
- engine-level: the **neq** set (sorted `(a_rep, b_rep)`); the **representative /
  merge partition**; **nominal markers** (`self.nominals` range); open **`≤n`
  obligations** state; the `snapshot_backprop_aborted` flag; **any pending
  worklist / fired-clause state** that influences the next `horn_fixpoint` pass
  (CRITICAL: if the verdict can depend on un-drained worklist events not implied
  by the label/edge state, the key must include them — or the cache lookup must
  happen only at a worklist-quiescent point, e.g. immediately after
  `horn_fixpoint` drains, so the state is canonical).
- Conservative rule: **if unsure whether a field matters, include it** —
  over-inclusion only lowers the hit rate (a perf cost), never soundness.

**The lookup point matters:** placing the cache check right after
`horn_fixpoint` returns `Sat` (the graph is then closure-saturated and
worklist-quiescent, just before disjunctive branching) makes the state canonical
and the key minimal. Prefer that over the very top of `solve`.

## 5. Soundness vs the FP-dead cross-pair snapshot cache

`snapshot-cache-fp-soundness-fix`: the cross-PAIR snapshot cache trusted ONE
satisfying model across DIFFERENT subsumption queries — unsound on non-Horn
(`sup ∈ that-model ≠ sub ⊑ sup`). This is categorically different: (a)
**within a single search** (one query), (b) memoizing a **deterministic
verdict** of the **full** state (no cross-query model reuse, no
trust-a-model), (c) only **decisive** verdicts, (d) conservative deps. There is
no model-trust step. The FP surface is solely key-completeness (§4).

## 6. Testing & gates

1. **Key-completeness adversarial review** (mandatory): a reviewer audits
   `full_state_key` against every field `solve`/`horn_fixpoint`/`apply_head_atom`
   read, hunting a field whose omission lets two different-verdict states collide.
2. **Verdict-identity unit tests:** run a set of disjunctive ontologies with the
   cache ON vs OFF; assert identical verdicts AND identical resulting closures.
   Include `Unsat` (subsumption), `Sat` (non-subsumption), multi-level-branch, and
   inverse/nominal/cardinality cases (the FP-suspect constructs).
3. **Corpus closure-IDENTITY net (sacred):** FP=0 AND every closure byte-identical
   to baseline (galen/notgalen/sio/wine/ore-10908/ore-15672/alehif/ro/pizza/bibtex).
   Any closure change → a key-completeness bug → STOP/fix.
4. **The headline wall gate:** ore-15672 (138s→target seconds), wine, and **family**
   (the consistency stall — caching may make it *terminate*, recovering the SP2
   sound-MISS!) — measure wall + whether family flips to a decisive verdict. Even
   if family doesn't fully terminate, ore-15672/wine should collapse.
5. **Cache cost:** the per-lookup key hash must cost ≪ the saved sub-search.
   Measure: cache must not slow the fast corpus (galen/sio). Incremental key
   maintenance (update the hash on each mutation) is the optimization if a
   full-graph hash per lookup is too costly; start with the simple full hash and
   optimize only if the fast corpus regresses.

## 7. Decomposition

1. `full_state_key()` + the per-search cache field + lookup/insert at the
   post-`horn_fixpoint` quiescent point + `with_wedge_cache` opt-in; verdict-identity
   unit tests (cache ON==OFF on disjunctive fixtures).
2. Wire into reasoner wedge paths (gated `RUSTDL_WEDGE_CACHE`).
3. **Key-completeness adversarial review** + corpus closure-identity net + the
   ore-15672/wine/family wall gate.
4. Cache-cost / fast-corpus non-regression; incremental key only if needed.
5. Flip default per the gate; CLAUDE.md + memory.

## 8. Open questions for implementation

- **Lookup point:** confirm post-`horn_fixpoint`-`Sat` is the right quiescent
  point (worklist drained, graph closure-saturated) so the key is canonical and
  no un-drained event affects the verdict. If `find_open_disjunction` can read
  state not captured by the saturated graph, adjust.
- **Cost of the key:** full-graph hash per branch vs incremental. With 48 states /
  278k branches the net win is enormous even with a full hash, but the fast corpus
  (galen) must not regress — measure (§6.5).
- **`≤n` obligation state in the key:** confirm whether the open-`≤n` set is fully
  determined by edges+labels (then it's implied) or needs explicit inclusion.
- **family:** quantify whether caching makes the family consistency search
  terminate (a completeness recovery beyond perf) or merely faster.
