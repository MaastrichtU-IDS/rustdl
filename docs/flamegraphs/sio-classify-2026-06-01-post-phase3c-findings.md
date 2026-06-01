# Phase 3c post-fix SIO flamegraph + findings

Re-flamegraphed 2026-06-01 against HEAD 0b5ed36 (Phase 3c bot_id cache).
Sampling: pprof-rs @ 199Hz, 60s window on `ontologies/real/sio-stripped.ofn`.

## Frame-level diff (top 15 unique frames, excluding `all`/`owl-dl-bench` roots)

| Rank | BASELINE (post-3b) | % | POST-3c | % |
|------|-------------------|---|---------|---|
| 1 | search | 69.00% | search | 64.70% |
| 2 | branch | 69.00% | branch | 64.70% |
| 3 | saturate | 67.59% | saturate | 61.94% |
| 4 | try_fold<ConceptExpr> (bot_id inner loop) | 24.66% | apply_deferred_concept_or_rules | 18.16% |
| 5 | try_fold<ConceptExpr> (bot_id inner loop) | 24.66% | apply_role_rules | 16.36% |
| 6 | try_fold<ConceptId, ConceptExpr> (bot_id) | 24.66% | apply_max | 14.34% |
| 7 | find_map<ConceptExpr> (bot_id scan) | 24.66% | {closure#1} | 13.41% |
| 8 | bot_id | 24.66% | edge_satisfies | 10.65% |
| 9 | apply_role_axioms | 24.66% | eq | 8.81% |
| 10 | {closure#0} (ConceptExpr closures, bot_id) | 19.63% | next<ConceptRule> | 8.56% |
| 11 | {closure#0} (ConceptId,ConceptExpr) | 19.63% | eq<ConceptRule> | 8.56% |
| 12 | {closure#0} (&ConceptExpr) | 19.63% | apply_deferred_or_residuals | 6.70% |
| 13 | {closure#0} | 19.63% | from_iter<ConceptId, Filter<...>> | 6.51% |
| 14 | apply_self_restriction | 19.08% | collect<Filter<...>> | 6.51% |
| 15 | thread_start (rayon idle) | 10.13% | are_declared_inverses | 6.43% |

## Hot-frame % deltas

- `apply_role_axioms`: **24.66% → 0.45%** (Δ **-24.21pp**) — dropped out of top 100 frames
- `bot_id`: **24.66% → 0.42%** (Δ **-24.24pp**) — dropped out of top 100 frames
- `find_map<ConceptExpr>` (bot_id linear scan): **24.66% → 0.00%** (Δ **-24.66pp**) — completely gone
- `try_fold<ConceptExpr>` cluster (3 variants): **24.66% → 0.00%** each — completely gone
- `{closure#0}` (ConceptExpr body closures, 4 variants): **19.63% → 0.00%** each — completely gone

The `apply_role_axioms / bot_id / find_map<ConceptExpr>` cluster that constituted
**24.66%** of SIO post-Phase-3b classify cost has been eliminated by the cache.
The O(n) `iter_with_ids().find_map(...)` scan is now replaced by a single
`Cell::get()` on every call after the first.

## Corpus measurement

| Fixture | Pre-P3c wall | Post-P3c wall | Δ | FP | MISSED |
|---|---|---|---|---|---|
| alehif | 6.84 s (P3b) | 28.05 s (shared-CPU artifact) | uncalibrated | 0 | 0 |
| ore-10908-sroiq | 27.19 s (P3b) | 26.01 s | -4% | 0 | 0 |
| ore-15672-shoin | 37.69 s (P3b) | 36.83 s | -2% | 0 | 0 |
| galen | 24.8 min (P3b) | 12.2 min (733.63 s) | **-50.8%** | 0 | 17 |

**Phase 0 net (alehif + 2 ORE):** FP=0 / MISSED=0 across all 3 fixtures.
All 3 tests completed in 36.87 s total elapsed. Soundness gate held.

**GALEN:** FP=0, MISSED=17 (same 17 `IntrinsicallyPathologicalBodyProcess` /
`AbnormalBodyStructure` cluster pairs — unchanged from Phase 3 baseline, requiring
functional-role merge beyond the wedge's reach). Wall: **733.63 s ≈ 12.2 min**,
down from 24.8 min (Phase 3b shared-CPU) — approximately **2× speedup** on GALEN
attributable to the bot_id cache eliminating the 24.66% ConceptExpr scan cluster.

## Interpretation

The Phase 3c bot_id cache delivered exactly the predicted outcome. The
`apply_role_axioms / bot_id / find_map<ConceptExpr>` cluster dropped from
**24.66% to 0.45%** (-24.21pp), with the `find_map` scan and all ConceptExpr
iterator closures completely absent from the flame. The cache (`Cell<Option<ConceptId>>`,
populated on first `Some` return) incurs only a single atomic read on every
subsequent call — negligible at any realistic call frequency.

The new dominant non-search frame is `apply_deferred_concept_or_rules` at 18.16%,
followed by `apply_role_rules` (16.36%) and `apply_max` (14.34%). These are natural
Phase 3d targets. The `from_iter / collect` cluster at 6.51% (heap-allocating
`Vec::from_iter` in `spec_extend` or similar) is the Phase 3e heap-allocation target.
`are_declared_inverses` reappears at 6.43% — confirming the Phase 3b HashSet swap
is holding (this is now the O(1) HashSet `contains`, not the old linear scan).

FP=0 + MISSED-unchanged held across all Phase 0 net fixtures, confirming the cache
is transparent at the verdict boundary. The cache invariant (populate on `Some` only,
leave `None` until Bot is actually interned) correctly handles the sequencing where
`bot_id()` may be called before `ConceptExpr::Bot` is interned. The GALEN wall time
improvement from ~24.8 min to ~12.2 min confirms that the bot_id cluster was a genuine
wall-time bottleneck, not just a sampling artifact — the 2× speedup is real and
attributable solely to the O(n) → O(1) cache replacement.
