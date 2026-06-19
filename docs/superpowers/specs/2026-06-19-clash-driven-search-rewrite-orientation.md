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

## 7. Immediate next step

Build **R0** (single-class probe tooling) + run **P0b** (depth-vs-thrash on
`e-interaction`, cleanly isolated). That one experiment picks sub-project 1's lever
(anywhere-blocking vs heuristics) and is the honest start of the rewrite — days, not
months, and it converts the multi-month decision from a guess into a measured branch.
