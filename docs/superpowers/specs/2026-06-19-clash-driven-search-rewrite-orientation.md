# Clash-driven search rewrite — orientation & program decomposition

**Date:** 2026-06-19
**Status:** orientation (program-level; first sub-project P0 specified, build pending)
**Program:** "Konclude-class engine" — the search rewrite (the multi-month north star)
**Predecessors:** all bounded perf levers measured out — see
`sp2-perf-attribution-2026-06-19`, `perf-frontier-attributed`,
`conflict-learning-simple-is-weak`, `tableau-memory-fanout`. Levers shipped/killed:
SP1/SP1.1 (completeness), adaptive-budget (#1, modest, shipped), within-search
caching (#2, ineffective-as-built), trail (NO-GO), absorption (refuted),
anywhere-blocking (named but never built).

This is the **decomposition + first-P0** doc for the rewrite — NOT a single
implementation plan. The rewrite is multi-month, research-grade, FP-delicate (FP=0
is sacred), with a NO-GO history on the obvious techniques. It proceeds as
P0-gated sub-projects; each gets its own spec→plan→build→corpus-identity cycle
AFTER its P0 passes. Do not build blind.

---

## 1. The problem, precisely measured

The corpus perf gap is 3 SROIQ-hard outliers (ore-15672 ≈138s, wine, family) where
the hypertableau wedge's disjunctive search does not converge:
- **Konclude solves ore-15672 in ~5ms** — so it is NOT genuinely exponential / hard;
  the instance is easy. rustdl's *search strategy* is the problem.
- `e-interaction-situation` (SAT): **~591,000 disjunctive branches, all restored,
  depth pinned at the 256 cap**, revisiting only **48 distinct full graph-states**
  (sound-key reuse probe). The search **redundantly re-explores 48 states ~hundreds
  of thousands of times via different branch paths** — it is not progressing toward
  the (small, Konclude-found) model; it is thrashing.

**Diagnosis:** a search-strategy + redundancy problem, not intractability. The
engine (a) dives depth-first into non-model regions to the 256 cap, and (b)
re-derives the same 48 states without memory. Konclude avoids both via
heuristics + blocking + propagation.

## 2. What the prior NO-GOs constrain

- **Dependency-directed backjumping** (have it) — `bjgap≈1`: clashes are local
  leaves, backjumps skip ~1 level → near-exhaustive. Doesn't help alone.
- **Conflict learning** (simple nogoods 13%; 1-UIP NO-GO) — the disjunctive
  structure lacks the long-range dependencies CDCL exploits. Re-entering needs a
  new reason.
- **Within-search caching** (#2, built) — sound but ineffective: the 48 reused
  states bottom out as `Stalled` at the depth cap → non-decisive → uncacheable.
  **KEY INSIGHT: caching is blocked by the depth cap, not by lack of reuse.**
- **Adaptive budget** (#1, shipped) — early-cuts diverging searches; modest,
  doesn't fix the search.

## 3. The rewrite hypothesis (to be P0-tested, not assumed)

The two mechanisms most likely to convert this easy-for-Konclude instance from
thrash to fast, and how they compose:

1. **Anywhere/pairwise blocking** (replace ancestor-only + the 256 depth cap). The
   documented "real lever" (`tableau-memory-fanout`): bound the completion graph
   finitely so the search terminates without depth-first thrashing to a fixed cap.
   **FP-DELICATE** with inverse roles + qualified cardinality (precisely why rustdl
   uses conservative ancestor-only). This is the hard, soundness-critical core.
2. **Within-search caching, unblocked.** #2 is built + sound but inert because the
   recurring states are `Stalled` (depth-capped). With anywhere-blocking making the
   search finite, those 48 states reach **decisive** verdicts → become cacheable →
   the cache (already built) collapses the 591k→48 redundancy. **#2 may "switch on"
   for free once blocking lands.**
3. (If 1+2 insufficient) **branching heuristics / semantic branching** — guide the
   search to the model first (Konclude-style), reducing branch count at the source.

The compose story: **blocking makes the search finite + states decisive; the
already-built cache then collapses the redundancy.** That's the leading hypothesis;
the P0 tests it before any multi-month commitment.

## 4. FIRST P0 (clean, isolated — the gate before scoping sub-project 1)

The confounded part of orientation: `hyper-sat` runs all 82 classes and prints only
at the end, so long single-class runs never finish in a wrapper. **Fix the tooling
first**, then answer the pivotal question.

- **P0a — isolate one class.** Build a single-class satisfiability probe (e.g. a
  CLI `sat <class-iri>` subcommand, or an `#[ignore]` test that calls the wedge
  `decide` on `e-interaction` alone with a configurable depth + deadline + stats
  dump). No all-class loop.
- **P0b — depth-vs-thrash.** With that probe: does `e-interaction` **terminate**
  (find the model) given a raised depth cap + ample time, or keep thrashing the 48
  states deeper? Reports `max_branch_depth`, branch count, verdict.
  - **Terminates at higher depth** → the 256 cap was the limiter → sub-project 1 =
    make the search finite WITHOUT a fixed cap, i.e. **anywhere-blocking** (then #2
    caching switches on). 
  - **Still thrashes** → strategy-bound → sub-project 1 = **branching heuristics /
    semantic branching** (blocking alone won't help).
- **P0c — caching-after-blocking smoke (if P0b says depth/blocking).** Prototype the
  cheapest blocking that bounds the model, re-run with #2 cache ON → do the 48
  states now reach decisive verdicts and the cache collapse the search?

**Gate:** sub-project 1 (the first real, FP-delicate build) is scoped ONLY after P0b
identifies the lever and P0c (if applicable) shows the compose works on
`e-interaction`. If neither blocking nor heuristics moves the `e-interaction` branch
count in a prototype, the rewrite is reframed (or escalated as genuinely
multi-month-with-uncertain-payoff) before committing.

## 5. Tentative decomposition (each its own spec/plan/build AFTER its P0)

| SP | Scope | P0 | FP risk |
|---|---|---|---|
| **R0** | single-class probe tooling + P0b/P0c diagnosis | this doc's P0 | none (diagnostic) |
| **R1** | the lever P0b picks — **anywhere-blocking** (likely) or branching heuristics | e-interaction terminates / branch-count collapses; corpus closure-identity | HIGH (blocking soundness w/ inverse+card) |
| **R2** | switch on / tune within-search caching atop R1 (#2 is built) | ore-15672/wine wall collapse; closure-identity | med (key-completeness, mostly done) |
| **R3+** | remaining (heuristics, family termination, perf parity) | per-item | per-item |

## 6. Non-negotiables (carried from the whole program)

- **FP=0 is sacred.** Every sub-project gates on the corpus closure-**IDENTITY** net
  (byte-identical, not just FP=0). Anywhere-blocking is the most FP-delicate change
  in the codebase — adversarial soundness review mandatory at R1.
- **P0 before build.** No multi-month commitment without a prototype showing the
  lever moves the `e-interaction` branch count. The history (conflict-learning,
  caching, trail) is a graveyard of plausible-but-inert levers; the P0 is the filter.
- **Measure decisive-verdict-reuse, not just state-reuse** (the #2 lesson) — the
  caching P0 must check that reused states reach Unsat/Sat, not just that they recur.

## 6b. CORRECTION (2026-06-19, after first P0 attempts) — diagnostics were on PROXIES

The orientation above conflated **three different searches** that give **contradictory**
results, so §1–§5's specifics must be re-validated before scoping R1:
- **classify per-pair (the real 138s):** `sat(ewe ⊓ ¬sup)` — Q-clause-injected;
  **109 pairs (all sub = `epistemic-workflow-enactment`) time out.** THIS is the
  ground-truth bottleneck.
- **hyper-sat per-class:** `sat(e-interaction)` / `sat(e-usage)` — 591k branches /
  48 states / Stall. A *different* search (class sat, not pair sub⊓¬sup).
- **consistency ABox-probe:** `sat(e-interaction)` ABox-seeded — **0.01s, terminates.**
  Yet a *third* construction of "the same" class sat.

Two probe-driven conclusions were therefore WRONG:
- **"e-interaction terminates at depth 100k" was a wrong-probe artifact** (ABox-sat is
  trivial regardless of depth). **classify ore-15672 @ depth 100k + cache is STILL
  141.89s** — raising the depth cap does NOT fix the real per-pair searches, and the
  built cache stays inert. Depth is NOT the limiter for the real bottleneck.
- **The "48-state reuse" (hyper-sat) may not characterize the classify per-pair
  searches** at all — they're a different construction.

Intriguing live clue (worth R0): the SAME class is 0.01s as an ABox-sat but thrashes
as a Q-clause sat. If the classify oracle's query *construction* (Q-clause + clausify)
is what makes the search pathological — vs the ABox-seeded construction being trivial —
that could be a far cheaper fix than blocking. Disentangle in R0:
`ABox-sat(ewe ⊓ ¬sup)` for a timed-out `sup` — fast ⟹ construction-pathology (cheap);
thrash ⟹ the `¬sup` is genuinely hard.

## 6c. Corrected R0 (do FIRST, with clean per-pair tooling)

Build a **single-pair** probe for the EXACT classify search: `sat(sub ⊓ ¬sup)` via the
HyperCache `decide` path (the real construction), on a timed-out `(ewe, sup)` pair,
with configurable depth + stats + cache toggle. Then:
1. Confirm it reproduces the thrash (the per-pair search, not a proxy).
2. Disentangle: construction (Q-clause vs ABox) vs `¬sup`-hardness vs depth vs strategy.
3. Only then does R1's lever (blocking / heuristics / construction-fix) become scopable.
**Do not scope R1 until R0 reproduces and attributes the REAL per-pair thrash.** The
proxy diagnostics burned effort; the lesson is the perf playbook's own rule — *confirm
the frame on the real classify per-pair search*, which §1–§5 did not.

## 6d. BREAKTHROUGH (2026-06-19, R0 disentangler PAID OFF) — it's a CONSTRUCTION pathology, likely CHEAP

The R0 disentangler confirmed the live clue and **reframes the whole rewrite**:
**`sat(ewe ⊓ ¬sup)` decided via the ABox construction terminates in 0.00–0.01s
(`consistent` = not-subsumed, the CORRECT verdict) for EVERY tested timed-out pair**
(`¬task`, `¬design-choice`, `¬ontology-project-execution`, `¬rational-agent`,
`¬communication-situation`) — the *identical logical queries* that classify's
**Q-clause construction thrashes on** (1000ms timeout × 109 pairs = the 138s).

**Same engine, same query, ~10⁴× difference ⟹ the bottleneck is the classify per-pair
oracle's QUERY CONSTRUCTION** (fresh Q-class ≡ sub + clausified ¬sup, via
`HyperCache::decide`), **not intractability and not the depth cap.** The textbook
reduction `sub ⊑ sup ⟺ {x : sub ⊓ ¬sup} inconsistent` (ABox-seeded) is sound AND fast
here; rustdl's Q-clause wedge reduction explodes the disjunctive search on the same
content.

**This very likely makes the rewrite CHEAP, not multi-month:** route the classify
per-pair subsumption oracle through the fast construction (or fix the Q-clause
construction's extra disjunctive firing) instead of building anywhere-blocking. R1 is
re-scoped accordingly.

### Re-scoped R1 (was: anywhere-blocking → now: construction fix)
1. **Root-cause WHY the ABox construction is fast** (which engine/route/seeding it uses
   — same wedge with direct concept-seeding vs Q-class indirection? main tableau via the
   consistency route? different absorption/GCI firing?). Pin the exact difference.
2. **Prototype routing the classify per-pair oracle through the fast construction**
   (e.g. `is_subclass_of(sub,sup)` → `is_consistent({x : sub ⊓ ¬sup})` via the fast path).
3. **Gate:** ore-15672 classify collapses (138s → seconds) AND corpus closure-IDENTITY
   (FP=0, byte-identical — the construction change must not alter any verdict) AND no
   regression on the fast corpus. If verdict-identical + faster → ship.
   CAVEAT: confirm the fast path is COMPLETE (decides, not gives-up) on these pairs — it
   returned decisive `consistent` (model found), not Stalled, so it is; re-confirm corpus-wide.

If R1 lands, R2 (#2 caching) and the whole anywhere-blocking R-track may be UNNECESSARY —
the construction fix alone could close the SROIQ-hard tail cheaply. Validate before
declaring; the construction-difference must be understood (not a fluke of deadline/route).

## 7. Immediate next step

Build **R0** (single-class probe tooling) + run **P0b** (depth-vs-thrash on
`e-interaction`, cleanly isolated). That one experiment picks sub-project 1's lever
(anywhere-blocking vs heuristics) and is the honest start of the rewrite — days, not
months, and it converts the multi-month decision from a guess into a measured branch.
