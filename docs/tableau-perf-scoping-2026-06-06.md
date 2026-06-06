# Scoping: making the complete engine tractable on nominal/disjunction-heavy
# ontologies (wine, and the hard-SROIQ frontier)

Scoped 2026-06-06. Question: what makes the complete (wedge/tableau) engine time
out on wine — and is there a tractable lever to close wine's residual 34 (and the
broader hard-SROIQ perf gap)?

**Verdict (measured, not inferred): the bottleneck is disjunction-branching
explosion in the wedge — not blocking, not model size, not completeness, not
merges.** The lever is conflict-driven search efficiency (nogood learning /
stronger dependency-directed backjumping). Multi-week, soundness-delicate, and
wine is an *outlier* relative to the otherwise-converging corpus.

## What was measured

| Probe | Result |
|---|---|
| `clause-stats wine` | **deferred=0** — the wedge fully represents wine (nominals + cardinality as 76 disjunctive clauses). Not a representation gap. |
| grape pair in **isolation** (`/tmp/grape.ofn`) | wedge **proves `Sancerre ⊑ SauvignonBlanc` (Unsat) in 0.10 ms**, disj=10, **merge=0**, stalled=0. Not a completeness gap, not a merge/backjump issue, not intrinsically hard. |
| `hyper-sat wine` (per-class, 1 s budget) | **90 / 137 classes STALL**, 1.20 M total branches, max depth 59. |
| stalled-class breakdown | disj branches dominate: CabernetFranc 21 798 disj / **0 merge** (depth 21); Merlot 21 376 disj / 0 merge; most stalled classes are **merge=0**. **restores = branches** → every branch fully unwound, pruning isn't capping the tree. |

Contrast (from the 2026-06-05 roadmap addendum): pizza blocks 35 % / 0 stalls;
SIO per-class sat 668 ms / 0 stalls. **Wine is qualitatively harder** — 76
disjunctive covering/disjointness clauses (every wine's colour/grape/region is a
covering disjunction) create a combinatorial branch tree the wedge re-explores.

## Why my first hypothesis was wrong

I initially named the lever as "precise merge DepSets to fix backjumping through
`≤n` merges" (the deferred `wedge-merge-deps-defeat-backjumping` memory note,
which explains `SpicyPizza`). The single-pair discriminator refuted it: the grape
pair has **merge=0** and is proved in 0.1 ms in isolation; the stalled wine
classes are disj-dominated with merge=0. Wine's explosion is **pure disjunction
branching**, a different lever. (Lesson re-confirmed: measure the per-pair
`disj/merge/stalled` before naming a lever.)

## The lever(s)

The wedge already has dependency-directed backjumping but **no learning**
(`restores = branches` on the stalled classes: it re-derives the same conflicts).
Highest-impact options, in rough order:

1. **Conflict-driven nogood learning** (CDCL-style, adapted to tableau): record
   the clash dep-set as a learned constraint so the search never re-enters an
   equivalent subtree. Directly attacks `restores = branches`. Multi-week;
   **soundness-critical** (a learned nogood with under-reported deps → unsound
   prune → false subsumption, the #1 failure mode). Heavy closure-diff + Konclude
   gating required.
2. **Stronger dependency-directed backjumping**, including the deferred
   `≤n`-merge precise-DepSet work (helps `SpicyPizza`-style cardinality pairs;
   *not* wine, which is merge=0).
3. **Relevance / module extraction per pair** (Lever D): restrict the 76
   disjunctions to those reachable from the pair's signature, so unrelated
   covering axioms don't branch. Cheaper than learning, but wine may be a single
   signature component (per `module-extraction-plan.md` §A, pizza/SIO were) —
   measure module sizes first.

## Recommendation

Wine is an **outlier**: the measured corpus (pizza, SIO, GALEN, ORE) already
converges (blocking fires, 0 stalls), so this lever does **not** unblock the
corpus — it targets covering-disjunction-heavy ontologies (wine and similar).
Cost/benefit: conflict-driven learning is a multi-week, soundness-critical engine
change for an *informational* (FP=0-only-gated) stressor plus future
covering-heavy inputs.

**Suggested order if pursued:** (a) measure wine's per-pair module sizes (cheap)
— if small, relevance/Lever D is the low-risk first cut; (b) only if modules are
large does conflict-driven learning become the necessary (and risky) lever. Bank
the region-cluster win (wine 57 → 34, FP=0) and treat the 34 as the documented
hard-SROIQ-disjunction frontier until a covering-heavy workload makes the
learning investment pay off.
