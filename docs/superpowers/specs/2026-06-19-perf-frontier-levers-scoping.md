# Perf-frontier levers — scoping (#1 adaptive budget, #2 search-count reduction)

**Date:** 2026-06-19
**Status:** scoping (pre-spec) — two levers, presented for sequencing/approval
**Context:** `sp2-perf-attribution-2026-06-19` memory. After SP1/SP2/SP1.1, the
corpus perf gap is **3 SROIQ-hard outliers** (ore-15672 138s, wine ~33min/54s-knob,
family stall), all **disjunctive-branch-COUNT-bound** (e-interaction: 593K branches
in 110s, no convergence; `merge=0`, small graph → not cardinality/blocking/closure).
Every bounded structural lever (absorption, anywhere-blocking, wedge trail) is
measured out. Konclude does these in ms.

This doc scopes the two remaining levers and their **mandatory P0 gates**. Neither
is built until its gate passes + a full spec/plan is approved.

---

## Lever #1 — Adaptive per-pair budget (divergence detection)

### Goal

Stop wasting the per-pair deadline on searches that will never terminate. Today
each subsumption probe burns its full `--pair-timeout-ms` (default 1000ms) before
giving up; ore-15672's 138s is ~109 hard pairs × ~1000ms. **Cut a *diverging*
search early** (sound under-approximation) → reclaim most of that wall.

### Approach

Monitor the wedge's existing `SearchStats` during a probe (no new instrumentation —
`branches_taken`, `restores`, `max_branch_depth`, node count are already tracked).
Periodically (every N branches) evaluate a **divergence predicate**:
- `restores ≈ branches` (every branch failing — no progress toward a model), AND
- `max_branch_depth` pinned at the cap, AND
- node count monotonically growing (model not stabilizing).
When it fires → return `Stalled`/"not subsumed" **early** instead of waiting for the
deadline. This is the SAME sound under-approximation the deadline already produces —
just sooner.

### Soundness

**FP=0 trivially.** Early-cut only ever turns a probe into "not subsumed" — it can
**lose a subsumption (MISS), never invent one**. The entire risk is *recall*: cutting
a search that *would* have terminated → a new MISS. So the predicate must fire **only**
on genuinely-diverging searches, never on slow-but-progressing ones (the discriminator:
a converging search has `restores < branches` or unpinned depth).

### P0 gate

On ore-15672 + wine, at a fixed budget: does early-cut **reduce wall materially
WITHOUT increasing corpus MISSED** vs the plain deadline? I.e. the pairs it cuts are
exactly the ones that time out anyway (non-subsumptions). Measure: corpus closure net
**FP=0 + MISSED unchanged** (byte-identical closures) + ore-15672/wine wall drop.

### Risk / effort

**Bounded, FP-safe, ~weeks.** The only failure mode is recall loss (caught by the
MISSED gate). "Convergence-risky" = the predicate tuning (N, the thresholds) must be
conservative; the gate quantifies it. Realistic outcome: ore-15672 138s → ~15–30s
(cut hard pairs at ~100ms not 1000ms), MISSED unchanged.

### Open questions (for the spec)

- Exact predicate + N (evaluation cadence). Start conservative (large N, strict
  divergence), loosen only while MISSED stays 0.
- Per-pair vs per-search: the wedge decide loop is the natural hook (it owns the
  branch counters).
- Interaction with the existing deadline: adaptive-cut is an *early* deadline; the
  hard deadline stays as the backstop.

---

## Lever #2 — Disjunctive-search-COUNT reduction (P0 PROBE FIRST)

### Goal

The only way to actually *match* Konclude on the hard tail: make the search explore
*fewer* branches (vs #1, which just stops futile ones sooner). Multi-month,
research-grade, **uncertain yield** — so it is **gated on a cheap viability probe
before any design commitment.**

### P0 PROBE (days, do this BEFORE scoping the build)

**The viability question: do the 593K branches re-explore equivalent sub-states
(→ caching has headroom) or are they genuinely distinct (→ caching can't help)?**

Probe: instrument the wedge at each `⊔` branch point to hash the branch's graph-state
(e.g. the multiset of node label-sets + edge structure) and count **distinct vs total**
states across `e-interaction-situation`'s search. Outcome:
- **High repeat ratio** → within-search status caching has real headroom → proceed to
  the conditional design below.
- **Mostly distinct** → caching won't help; the lever is branching heuristics /
  learning (lower prior — 1-UIP was a measured NO-GO, bjgap≈1) → likely **STOP** and
  accept the knob + Lever #1 as the answer.

This probe is a throwaway instrumentation (like the SP2 `RUSTDL_FIXPOINT_ITERS` probe);
it decides whether #2 is worth a multi-month build.

**P0 PROBE RESULT (2026-06-19): VIABLE — STRONG.** On `e-interaction-situation`:
**581,496 branches → only 14 distinct graph-states** (label-multiset hash; reuse ratio
≈ 0.00002). The 14 states stabilized within the first 50k branches; the search then
revisited them ~40,000× each — it is trapped in a tiny attractor, not exploring a large
space. (Second hard class: 64,762 branches → 453 distinct, ratio 0.007 — also massive
reuse.) **Lower bound** (labels only, not edges), so true reuse is ≥ this. Verdict:
within-search caching has enormous headroom — it could collapse these searches to
near-instant. **Proceed to a full #2 caching spec.** The design crux is the FP-safe key
(labels + edges + inverse/nominal/cardinality context — a label-only key would be
UNSOUND if two same-label states have different edge context + different verdicts) and
verdict handling (Unsat carries a dep-set; Sat terminates; **Stalled/incomplete must NOT
be cached** — that was the cross-pair snapshot's FP death).

### Conditional design (only if the probe shows reuse)

**Within-search status caching.** Cache `(graph-state-key → Sat/Unsat verdict)` and
short-circuit re-exploration of an equivalent sub-search.
- **Distinct from the FP-dead cross-pair snapshot cache** (which trusted ONE model
  across *different* pairs). Within a *single* search, a sub-graph's (un)satisfiability
  given its full local context is deterministic → caching is sound IF the key captures
  enough context.
- **The FP-delicate crux:** the cache key must include everything the verdict depends
  on — not just the local label set, but inverse-edge context, nominal identity, and
  cardinality constraints reaching the sub-graph. An under-specified key = a wrong
  cache hit = **FP**. This is why it's research-grade and gated.

Alternatives if caching's key proves intractable: semantic branching / disjunct-ordering
heuristics (cut the count without caching) — untested, separate sub-probe.

### Soundness

The cross-pair snapshot cache died FP-unsound (`snapshot-cache-fp-soundness-fix`).
Within-search caching is a *different* soundness argument (deterministic within one
search) but **equally FP-delicate** — the corpus closure-IDENTITY net + adversarial
review are mandatory, and the key-completeness proof is the whole game.

### Risk / effort

**High risk, multi-month, uncertain yield** — for wall-time-only gains on inputs
already sound + knob-complete. The P0 probe is the cheap filter; do not commit the
build without it (and without a key-soundness design that survives review).

---

## Recommended sequencing

1. **Lever #1 first** — bounded, FP-safe, weeks, real wall win (ore-15672 ~5–10×),
   MISSED-gated. Highest value/effort ratio; ships robustness against pathological
   hangs regardless of #2.
2. **Lever #2 P0 probe** — days; decides viability. Only if it shows branch-state reuse
   AND a sound cache key is designable does the multi-month build get greenlit (own
   spec/plan + adversarial soundness review).

Both are wall-time-only on the 3 pathological fixtures (already FP=0 + knob-complete);
neither touches the already-fast EL/Horn/moderate-SROIQ corpus. If "highest
performance" means real-world robustness → #1 is the answer. If it means matching
Konclude on the hard SROIQ tail → #2, gated.
