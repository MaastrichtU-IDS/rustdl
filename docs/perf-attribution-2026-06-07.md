# Performance attribution — post-v0.3.6 (2026-06-07)

Measured + flamegraph-attributed the corpus perf picture after the
nominal-completeness work (wine 57→0). **Conclusion: the remaining perf frontier
is hard — every candidate lever is already-optimized, a known dead-end, or
soundness-nuanced. No clean low-risk high-value win is sitting here.** Banked as a
scoping deliverable rather than rushed into a build.

## Current classify walls (`classify --pair-timeout-ms 200`, single run)

| ontology | wall | vs 06-04 | note |
|---|---:|---|---|
| **wine** | **412.7 s** | (was n/a) | the corpus's dominant cost |
| ore-15672 | 29.1 s | flat | SROIQ-N ratio (~16× Konclude) |
| sio-stripped | 30.4 s | ~flat | SROIQ ratio (~13×) |
| ore-10908 | 5.3 s | flat | inside the ≤5× target |
| pizza | 2.1 s | 3.5→2.1 | fine |

The completeness levers gave wine **MISSED=0 but did not help its wall** — the
saturator now *derives* all 622 subsumptions, yet the per-pair tableau still runs.

## wine 412 s — attributed: redundant tableau invocation (tableau contributes 0)

`classify` stats: `subsumption: saturation=622 tableau=0`; `tier_walk=409.6s`
(≈ the whole wall); **`timed-out pairs: 9165`** (× 200 ms cap); `hyper-proven: 21`.
So **the per-pair tableau finds ZERO subsumptions** — it burns 200 ms on each of
9165 wedge-stalled pairs to default to "not subsumed" (all correct, FP=0/MISSED=0).
Flamegraph (60 s of the real classify): the cost is the **full completion over
the ABox-seeded graph** — `apply_nominal_assignment`, `apply_min`, `apply_forall`,
`apply_and`, `first_clash`, `clash_deps_at`, ~25 % rayon idle/wait.
**`is_blocked` is NOT hot** (overturns the earlier counter-based guess taken on a
single `Fruit` decide — confirm on the real run, always).

**The finding is "these tableau calls shouldn't happen," not "make a rule
faster."** Only 21 of the non-trivial pairs are settled by the wedge; the other
9165 *stall* in the wedge and fall through to the slow tableau. Lever = the
**orchestrator**: settle the wedge-stalled non-subsumptions without the 200 ms
ABox-seeded tableau. Two sub-questions, both real:
- *Why does the wedge stall on 9165 wine pairs?* (it has its own nominal/
  cardinality handling). If it returned `NotSubsumed` it would short-circuit via
  `trust_sat`; it doesn't, so it must `Stall`.
- *The ABox-seed-skip is NOT sound-for-completeness* (advisor): a named
  individual CAN force a universal `C ⊑ D` via nominals (`C ⊑ ∃R.{a}`, `a:E` ⟹
  `C ⊑ ∃R.E`), so dropping the seed risks a silent MISSED regression on
  Horn-shortcircuited ontologies — and wine's own 57→0 may lean on seeded
  individuals. Must be gated on MISSED=0 across the ABox-bearing fixtures, not
  just FP=0.

Highest *value* (412 s → potentially seconds) but soundness-area orchestrator
work — a careful, reviewed, fresh undertaking, not a session-tail build.

## sio-stripped / ore-15672 (the roadmap SROIQ ratio) — `edge_satisfies` call volume

Flamegraph: `are_declared_inverses` ~11.5 % + `is_sub_role` ~8 % + `apply_role_chains`
6.4 % + `apply_deferred_or_residuals` 8.5 % + `first_clash`/`clash_deps_at` ~11 %.
Both `are_declared_inverses` and `is_sub_role` live inside **`edge_satisfies`**
(`lib.rs:602`), the per-edge role-match predicate. The lookups are already O(1)
(Phase 3b hashset); the cost is **call volume** (per-edge × per-role-atom during
rule application). Reducing the calls = edge-keyed role indexing = **Phase 3e,
which was reverted** (+2.34 % GALEN regression — workload-dependent break-even;
dead-end ledger §16). So the obvious lever here is a *known dead-end*. A genuine
win needs a different structure (e.g. memoizing `edge_satisfies(s,w)` per role-pair
— but the lookup is already O(1), so the saving is call/branch overhead, likely
marginal) — verify redundancy (calls vs distinct `(s,w)` args) with a counter
before investing, or it repeats 3e.

## Shared frame — `apply_deferred_or_residuals` (wine 5.5 %, sio 8.5 %)

The one frame hot in both. **Already bloom-prefiltered** (Phase 3,
`needs_deferred_or` + `label_sig`, `rules.rs:589–609`). The residual is intrinsic
work over the deferred-OR rules per node; no obvious further win.

## Recommendation

- **Don't** open the editor on a lever now: the SROIQ one is the reverted-3e
  dead-end (verify redundancy first), the shared frame is already optimized, and
  the wine one is soundness-area orchestrator work.
- **Highest value = wine's redundant-tableau-invocation** (412 s → seconds): a
  fresh, reviewed undertaking — understand *why the wedge stalls on 9165 pairs*
  (the real root) and/or make the ABox-seed-skip with a MISSED=0 gate across the
  ABox-bearing fixtures. Soundness-critical; do it with full discipline.
- Cheap pre-work for next time: a redundancy counter on `edge_satisfies`/
  `are_declared_inverses` (calls vs distinct args) settles the SROIQ lever with
  evidence instead of a flamegraph %.
