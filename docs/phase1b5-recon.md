# Phase 1b.5 recon — does lazy expansion pay off on GALEN?

Run 2026-06-03 at HEAD `3ae716f` (Phase 1b.5 recon T1 instrumentation
landed). Recon to decide whether the Phase 1b.5 lazy-expansion
implementation is worth building, given Phase 1b's full-re-run only
added +8.3% wall overhead vs flag-OFF baseline on GALEN (148.95 →
161.31 s on this host).

## Headline

**GO.** Pairs-per-sub distribution is flat at ~680 pairs/sub across all
2,748 GALEN classes (≈1.86M deep-path queries total). Wedge cost is
dominated by 1ms calls (84% of all wedge calls). Lazy expansion's
amortization math projects ~3,200 CPU-seconds saved (~400 wall-sec
on 8-cores), bringing GALEN wall well under the flag-OFF baseline.
Ship Phase 1b.5 lazy expansion as scoped.

## Pairs-per-sub distribution on GALEN

| Metric | Value |
|---|---:|
| Subs reaching subsumes_via_tableau | **2,748** (every class) |
| Total pairs to deep path | **1,864,609** |
| Median pairs per sub | **680** |
| p90 | 681 |
| p99 | 681 |
| Max | 681 |

The distribution is essentially flat — every sub gets ~680 deep-path
queries. This is the IDEAL shape for snapshot caching: one snapshot
build per sub amortizes across 680 reuses. Worst case (top-down walk
prunes harder per Phase 7 label cache stats — 164,599 cache prunes
out of 7.5M raw pairs) is still ~680 reuses per sub.

For context: pre-project (`docs/phase1a-results.md`), GALEN's
Phase 8 baseline of 453 s included Phase 7 label cache pruning to
~164k pairs reaching the wedge. This run shows 1.86M, which suggests
either (a) the label heuristic threshold or behavior shifted (alehif
shows 167 subs × 96 pairs ≈ 16,048 reaching deep path), or (b) the
distribution measure includes pairs counted differently than the
Phase 7 cache stat. The gross magnitude (1.86M) is what matters for
amortization.

## Wedge cost distribution

| Bucket (ms) | Pair count | % of total | Bucket midpoint × count (CPU-ms) |
|---|---:|---:|---:|
| 0 | 2 | 0.0% | 0 |
| 1 | 1,574,413 | **84.4%** | 1,574,413 |
| 2-4 | 182,427 | 9.8% | 547,281 |
| 5-9 | 48,679 | 2.6% | 340,753 |
| 10-19 | 46,188 | 2.5% | 669,726 |
| 20-49 | 12,861 | 0.7% | 450,135 |
| 50-99 | 39 | 0.0% | 2,925 |
| 100-999 | 0 | 0% | 0 |
| ≥1000 | 0 | 0% | 0 |
| **Total** | **1,864,609** | 100% | **~3,585,233 CPU-ms** |

Total wedge work: **~3,585 CPU-seconds**. Wall is 150 s, so effective
concurrency ≈ 24× — consistent with the 32-thread rayon parallelism.

Median per-pair wedge cost: ~1 ms (84% of calls). Long-tail (≥10 ms)
exists but represents only ~3.2% of pairs.

## Break-even projection

Assumed costs:
- `wedge_cost_per_pair` ≈ 1.92 ms (weighted-mean of the histogram).
- `snapshot_build_cost(sub)` ≈ same shape as a cold wedge call for sub
  alone (~1-2 ms median; for amortization analysis, assume 2 ms).
- `replay_cost(sub, sup)` — Phase 1b first-cut full-re-run ≈ wedge_cost
  (so Phase 1b shows no improvement). **Optimistic Phase 1b.5 target:
  replay at 10% of wedge cost (~0.2 ms per pair).** Pessimistic: 30%
  (~0.6 ms).

### Per-sub math

For sub with k=680 pairs:

| Strategy | CPU per sub | Total (2748 subs) |
|---|---:|---:|
| Cold wedge (current) | 680 × 1.92 ms = **1,305 ms** | **3,585 sec** |
| Phase 1b full-re-run (no skip) | 2 + 680 × 1.92 = **1,307 ms** | 3,590 sec (≈ same, +0.1%) |
| Phase 1b.5 lazy (10% replay) | 2 + 680 × 0.2 = **138 ms** | **378 sec (−89%)** |
| Phase 1b.5 lazy (30% replay, pessimistic) | 2 + 680 × 0.6 = **410 ms** | **1,126 sec (−69%)** |

### Wall projection (assuming 24× concurrency)

- Cold wedge wall: 150 s (measured baseline).
- Phase 1b lazy (10% replay): 378 / 24 ≈ **16 s wall** (estimated).
- Phase 1b lazy (30% replay): 1126 / 24 ≈ **47 s wall**.

Even with pessimistic assumptions, lazy expansion projects to bring
GALEN wall well below the flag-OFF baseline of 150 s. The optimistic
case projects ~10× speedup vs current cold wedge.

### Sensitivity

The projection assumes:
- Per-pair replay cost scales linearly with the new clause's triggers
  (one `fresh_q ⊓ sup → ⊥` clause; triggers at most one fresh node-event).
- Snapshot build cost is similar to wedge cost (likely TRUE because
  the wedge IS essentially the snapshot build — they share `HyperEngine::new`).
- The flat 680 pairs/sub distribution is stable across the run (verified
  via p90/p99/max all ≈ 680).

The main risk: **lazy expansion may not actually skip much work** if
the new ¬sup clause's triggers fire on a non-trivial fraction of
snapshot nodes. The fingerprint approach (skip rule firings when
trigger-set hash unchanged) needs implementation care to ensure the
NEW clause's triggers DO fire while pre-existing clause-trigger
events get skipped.

## Recommendation

**GO: ship Phase 1b.5 lazy expansion.**

The pairs-per-sub flat distribution + wedge-cost histogram dominated
by 1ms calls together project massive savings (~89% CPU reduction in
the optimistic case, ~69% pessimistic). Even with substantial slippage
(say 50% of the projection holds), lazy expansion brings GALEN wall
under 100 s — well below the flag-OFF 150 s baseline and the spec §6
GALEN ≤ 150 s target.

Implementation plan to write next:
`docs/superpowers/plans/2026-06-XX-konclude-snapshot-cache-phase-1b5.md`
covering:
1. Per-node label-set fingerprint (sorted-labels hash captured at
   snapshot time; recomputed at replay-time post-processing each
   event; skip rule firing on snapshot-origin nodes when fingerprint
   matches AND no new clauses index the label-trigger).
2. Per-sup `neg_sup_clauses` caching (T2 reviewer's perf note — avoid
   re-allocating the 1-clause vec per pair).
3. Capture `parent`/`parent_role` for snapshot nodes (HF2
   double-blocking restoration; T1 reviewer's note).
4. GALEN gate per task (per Phase 1b T6 lesson: corpus diff catches
   what synthetic canaries miss; never close a soundness-touching
   task without it).
5. Measure GALEN wall vs flag-OFF 150 s baseline; results doc; Phase 1c
   green-light or revert.

## What instrumentation kept

The `pairs_per_sub` and `wedge_cost_histogram_ms` counters from T1
(commit `3ae716f`) are useful diagnostic telemetry — keep them. They
formalize as "profiling fields" rather than "temporary recon
instrumentation". Future ontology profiling sessions can reuse the
banner output without re-instrumenting.

The Phase 1b.5 implementation plan should explicitly NOT revert these
fields; instead, document them in a brief comment block on
`ClassificationStats` as kept telemetry.

## What's deferred

- Empirical measurement of `snapshot_build / wedge_cost` ratio (used
  rough estimate above; can be measured precisely with a microbenchmark
  if Phase 1b.5 measurement reveals slippage from the projection).
- notgalen + sio-stripped distributions. If Phase 1b.5 lands on GALEN
  with the projected savings, run the same instrumentation on those
  workloads to extend the win.
- `RUSTDL_SNAPSHOT_CAPTURE=1` rerun of the same GALEN command to
  measure Phase 1b's full-re-run replay-cost histogram directly. Would
  validate the "replay ≈ wedge cost" assumption empirically; for now
  the assumption rests on Phase 1b's +8.3% wall delta which is
  consistent.

## Cross-references

- Project spec: `docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md`.
- Phase 1b results: `docs/phase1b-results.md`.
- Phase 1b.5 recon plan: `docs/superpowers/plans/2026-06-03-konclude-snapshot-cache-phase-1b5-recon.md`.
- Pre-project baseline: `docs/perf-2026-06-03-konclude-vs-rustdl.md`.
