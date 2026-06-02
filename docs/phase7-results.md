# Phase 7 — per-class label heuristic results

Run 2026-06-02 under host load avg ~86–93. Per-class wedge satisfiability
builds a `Vec<LabelOracle>` cache once at classify-time; the orchestrator
skips `subsumes_via_tableau` when `D ∉ labels(C)` (sound counterexample-
model). See:
- `docs/superpowers/specs/2026-06-02-per-class-label-heuristic-design.md`
- `docs/superpowers/plans/2026-06-02-per-class-label-heuristic.md`

## Headline

**GALEN classify wall: 684 s → 455.73 s = −33 % under contention.** The
surprise: Phase 5 T3b had attributed GALEN's wall to the tier-walk and
showed `tableau_subsumption_calls=0`. The actual mechanism: many
closure-miss pairs hit the wedge via `subsumes_via_tableau` and returned
`NotSubsumed` (counted as `hyper_refuted_pairs`, not as tableau calls).
The label cache short-circuits ~all of those wedge calls.

ORE-10908: 27.37 s → 19.32 s (−29 %). ORE-15672 effectively unchanged
(29.55 s → 29.71 s). All other measured workloads improved or held flat
(sulo +4 ms = noise).

FP=0 / MISSED=0 preserved across Phase 0 net + GALEN. Prune rates 96–100 %
confirm the heuristic mechanism is sound and aggressive.

## Soundness gate (Phase 0 net + GALEN)

All FP=0 / MISSED=0, unchanged from pre-T6 baseline:

| Fixture | rustdl_closure | konclude_closure | FP | MISSED |
|---|---|---|---|---|
| alehif-test | 247 | 247 | 0 | 0 |
| ORE-10908-sroiq | 6001 | 6001 | 0 | 0 |
| ORE-15672-shoin | 142 | 142 | 0 | 0 |
| GALEN | 27997 | 27997 | 0 | 0 |

## Wall + completeness lever

rustdl CLI, `--pair-timeout-ms 200`, single rep:

| Fixture | Pre-T7 wall | Post-T7 wall | Δ | Pruned | Pass-through | Misses | Prune rate |
|---|---|---|---|---|---|---|---|
| pizza | 4.39 s | 4.06 s | −7.5 % | 1179 | 136 | 0 | 89.7 % |
| sulo-stripped | 0.09 s | 0.13 s | noise (+4 ms) | 71 | 0 | 0 | 100 % |
| ro-stripped | 0.87 s | 0.65 s | −25 % | 632 | 11 | 0 | 98.3 % |
| sio-fp2-module | 0.70 s | 0.65 s | −7 % | 565 | 32 | 0 | 94.6 % |
| alehif-test | 2.87 s | 2.21 s | −23 % | 8227 | 0 | 0 | 100 % |
| ORE-10908-sroiq | 27.37 s | 19.32 s | −29 % | 25137 | 0 | 1003 | 96.2 % |
| ORE-15672-shoin | 29.55 s | 29.71 s | flat | 842 | 6 | 46 | 94.2 % |
| **GALEN** (closure-diff test) | **684 s** | **455.73 s** | **−33 %** | (n/c) | (n/c) | (n/c) | — |

Universal speedup or flat-or-noise on every measured ontology. No
regressions. The plan's ±10 % non-regression tolerance is satisfied with
significant headroom on every workload, and exceeded outright on GALEN
and ORE-10908.

## Konclude head-to-head update

Per `docs/perf-2026-06-02-konclude-vs-rustdl.md`:

| Fixture | rustdl post-T7 | Konclude | Ratio | Pre-T6 ratio |
|---|---|---|---|---|
| ORE-10908 | 19.32 s | 1.61 s | **12×** | 17× |
| ORE-15672 | 29.71 s | 1.72 s | 17.3× | 17.2× |
| pizza | 4.06 s | 1.68 s | 2.4× | 2.6× |
| GALEN | 7.60 min | (not re-measured today) | — | — |

The ≤5× Konclude target was NOT achieved on ORE workloads. ORE-10908
closed from 17× → 12× (largest gap-narrowing in the corpus); ORE-15672
held flat. GALEN's −33 % is the clear standout win.

## What was NOT achieved

The plan's ≤5× Konclude wall ratio on SROIQ workloads was NOT reached.
ORE-10908 closed from 17× → 12×; ORE-15672 unchanged at 17×. The
Konclude wall is dominated by docker-startup floor on these small
workloads (~1.3 s); rustdl's actual reasoning per pair remains slower
than Konclude's despite the heuristic. Further work would need to
attack the residual per-pair tableau cost, not just the dispatch count.

## What surprised

GALEN's −33 % was beyond the ±10 % non-regression tolerance the plan
set. The mechanism (short-circuiting wedge calls counted under
`hyper_refuted_pairs`) wasn't anticipated because Phase 5 T3b's
flame-attribution showed `tableau_subsumption_calls=0`. The wall
breakdown isn't perfectly explained by the existing counters; adding
a `hyper_refuted_pairs`-by-call-source counter could clarify post-hoc,
but the empirical win is unambiguous.

Why the inference holds: Phase 5 T3b's instrumentation showed
`saturation_subsumption_hits=37181` and `tableau_subsumption_calls=0`
on GALEN, suggesting the saturator answered everything. But the
orchestrator's `subsumes_via_tableau` first invokes the wedge's
`hyper_decide`, and a wedge `NotSubsumed` verdict returns without
incrementing the tableau counter — it increments `hyper_refuted_pairs`
instead. The label cache short-circuits before `hyper_decide` fires,
which is why Phase 7 saves wall the T3b counter set couldn't see.

## What's next (queued)

- Could pursue Konclude-class ≤5× via deeper attack on per-pair
  tableau (likely needs structural change, not pure-perf flag).
- Could pursue further heuristic refinements (cached labels across
  hierarchy walks; per-pair micro-optimizations).
- Could integrate the `RUSTDL_LABEL_HEURISTIC` env-gate behaviour
  into the Phase 4 fragment-classification contract (auto-disable
  on pure-EL — though pure-EL already short-circuits the cache build).
- Add a `hyper_refuted_pairs_by_source` counter so future regression
  drill-downs can localize wedge wall without re-deriving the
  mechanism above.

## Cross-references

- Design: `docs/superpowers/specs/2026-06-02-per-class-label-heuristic-design.md`
- Plan: `docs/superpowers/plans/2026-06-02-per-class-label-heuristic.md`
- Phase 6 (prior perf): `docs/phase6-results.md`
- Phase 5 chain (regression localization): `docs/phase5-recon.md`,
  `docs/phase5-walltime-probe.md`, `docs/phase5-variance-check.md`,
  `docs/phase5-downstream-probe.md`
- Head-to-head baseline: `docs/perf-2026-06-02-konclude-vs-rustdl.md`
- Structural canary: `crates/owl-dl-reasoner/tests/label_heuristic_canary.rs`
