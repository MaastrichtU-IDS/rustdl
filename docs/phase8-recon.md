# Phase 8 — ORE-10908 recon (post-Phase-7 label heuristic)

Run 2026-06-03 at HEAD `46aa76d`. Temporary `Instant::now()` instrumentation
in `crates/owl-dl-reasoner/src/classify.rs::classify_top_down_internal`
and `::find_direct_parents_top_down` (reverted after probe). Goal: localize
the residual 19.32 s wall on ORE-10908-sroiq after Phase 7's label heuristic
shipped (Konclude wall ~1.61 s, ratio ~12×; target ≤5×).

Companion to:
- `docs/phase5-{recon,walltime-probe,variance-check,downstream-probe}.md`
- `docs/phase6-results.md`
- `docs/phase7-results.md`
- `docs/perf-2026-06-02-konclude-vs-rustdl.md`

## Probe results (ORE-10908)

Three runs total (instrumentation iterated; numbers below are from the
final, most-instrumented run unless noted). `classify --pair-timeout-ms 200
ontologies/external/ore-10908-sroiq.ofn`. Wall reported by
`/usr/bin/time`: **20.51 s / 20.20 s / 17.61 s** across runs (variance
from host load; the relative breakdown within each run is consistent).
`n=692` satisfiable classes.

### Section breakdown of `classify_top_down_internal` (run 1)

Section probe TOTAL (20.437 s) sums to within ~75 ms of wall (20.51 s) —
no instrumentation gaps.

| Section | Wall (s) | % of TOTAL |
|---|---|---|
| `saturate()` | 0.038 | 0.19% |
| `PreparedOntology::from_internal()` | 0.001 | 0.01% |
| **Phase 7 label_cache build (parallel)** | 0.588 | 2.88% |
| Unsat probes (parallel) | 4.663 | 22.82% |
| **Tier walk** (parallel) | 12.882 | **63.03%** |
| Defined-sup sweep (parallel) | 2.263 | 11.07% |
| Entailed matrix build | 0.001 | 0.00% |
| **TOTAL** | 20.437 | 100% |

### Per-pathway breakdown in `find_direct_parents_top_down` (run 1)

Counts are total across the tier-parallel walk; wall is **summed
thread-time**, so the pathway sum exceeds the parent section wall by the
parallel speedup factor.

| Pathway | Count | Total thread-wall (s) | Mean per call |
|---|---|---|---|
| `closure.contains` hits (fast path) | 5 999 | 0.000 | 0.043 µs |
| `label_cache_pruned` (early skip) | 25 552 | 0.011 | 0.412 µs |
| `label_cache_pass_through` (→ subsumes_via_tableau) | 0 | 0.000 | — |
| `label_cache_misses` (→ subsumes_via_tableau) | **588** | **16.775** | **28.528 ms** |
| `LabelOracle::Unsat` (return true) | 0 | 0.000 | — |

Engine-side counters: `tableau_subsumption_calls = 0`,
`hyper_refuted = 7 469`, `hyper_proven = 0`, `timed_out_pairs = 0`. So
the 28.5 ms / call cache-miss cost is **hyper wedge** time on
`sub ⊓ ¬sup` queries, not the actual tableau.

### Label-cache verdict distribution (run 2 + run 3)

Re-runs added probes to characterize the cache itself:

| Run | `Sat` | `Unsat` | `NoVerdict` | misses observed |
|---|---|---|---|---|
| 2 | 657 (95.0%) | 0 | **35 (5.1%)** | 1 340 |
| 3 | 662 (95.7%) | 0 | **30 (4.3%)** | 1 100 |

So ~30-35 classes account for **all** cache misses (~30-45 candidate-
ancestor pairs each). The miss count varies with run because the
NoVerdict set varies — see next probe.

### Per-class label-cache wall + the NoVerdict cause (run 3)

| Verdict | n | median (ms) | p95 (ms) | max (ms) | budget |
|---|---|---|---|---|---|
| Sat | 662 | 0.7 | 81.6 | 212.1 | 200 |
| **NoVerdict** | **30** | **341.0** | **542.9** | **630.9** | **200** |

**This is the load-bearing finding.** Every `NoVerdict` class spent
*more than 200 ms* in `classify_labels` and bailed at the per-pair
deadline (`HyperResult::Stalled`). The cause is **purely the deadline**,
not structural (out-of-fragment). The Sat path completes in median
0.7 ms; the NoVerdict tail is 500× slower and capped.

The deadline is `per_pair_timeout.map(|t| Instant::now() + t)` =
**200 ms**, inherited from the CLI's `--pair-timeout-ms 200`. The
classify_labels deadline is reusing the per-pair-query budget despite
doing a different (and rarer) job — one wedge-sat call **per class**
(692 calls), not per pair (~78 k pairs in the walk).

### Causal chain (consequence)

1. A class is "hard" (>200 ms standalone wedge sat) → `NoVerdict` in
   cache.
2. In the tier walk, that class is the `sub` for ~38 candidate
   ancestors on average.
3. Each (sub, sup) pair re-runs `subsumes_via_tableau` (hyper wedge on
   `sub ⊓ ¬sup`, 28.5 ms median).
4. **30 hard classes × 38 candidates × 28.5 ms ≈ 32.5 CPU-seconds** —
   accounts for the entire 16.8 s thread-time on cache_misses (parallel
   speedup ~2×).

If those 30 classes were `Sat` instead of `NoVerdict`, those 1 100-1 340
miss pairs would convert to `Pruned` (0.4 µs each), saving ~12-16 s of
tier_walk thread-time — **~50-60% of total wall**.

## Comparison to GALEN T3b probe

GALEN's pre-Phase-7 T3b (`docs/phase5-downstream-probe.md`) showed:
saturate 0.12%, from_internal 0.004%, unsat_probes 2.66%, tier_walk +
sweep 97.22%, matrix 0.001%. ORE-10908 post-Phase-7 has a different
shape — the label cache prunes 97.7% of the would-be tableau surface,
so unsat_probes (23%) and sweep (11%) become *proportionally visible*,
and the remaining 63% in tier_walk traces back to a small NoVerdict
tail in the label cache, not a per-pair-cost monolith.

## Inferences for next-step choice

Mapping the four candidate levers against the breakdown:

- **Cache-build coverage** (raise the deadline for `classify_labels`
  during cache build). The NoVerdict bail is deadline-bound; the 30
  outlier classes have median 341 ms / max 631 ms walls — well within a
  1-2 s budget. A larger deadline for the standalone wedge-sat (used
  only at cache-build time, ~30 outlier classes total) converts the
  entire 12-16 s tier_walk cache-miss bucket into ~0.4 µs prunes.
  **Highest-ROI lever; cheapest implementation** (one parameter
  change). Projected wall: ~20 s → ~6-8 s, closing most of the gap to
  Konclude.
- **Per-pair (wedge) cost reduction** — would directly hit the 28.5 ms
  median, but the cache-miss bucket evaporates entirely under the
  coverage lever above. Pursue only if coverage stalls or the residual
  rebalances onto unsat_probes / sweep.
- **Saturator extension** — closure_hits (5 999) is already the fast
  path. Hard to justify ahead of coverage given measured impact.
- **Stop** — not warranted; 12× gap to Konclude with a concrete,
  cheaply-implementable lever sitting in front of us.

### Concrete recommendation

**Phase 8 should raise the deadline for `classify_labels` during the
Phase-7 cache build.** The standalone wedge-sat call needed to classify
each class's labels is being capped at the per-pair budget (200 ms) it
inherited from the CLI flag, but the cache-build pass is a one-time
per-class cost (n=692 here), not a per-pair cost. Decoupling these two
budgets — e.g., a separate `RUSTDL_LABEL_BUDGET_MS` env (default ~2-5 s,
or `per_pair_timeout × 25`) — converts the ~30 NoVerdict outliers to
`Sat` oracles and eliminates the cache-miss bucket. The label-cache
build itself only costs 2.9% of wall today, so even a 10× budget
increase still keeps it well under 10% while clearing the dominant
residual.

## Surprises

- **Phase 7's prune rate is excellent** (97.7%) — the label heuristic
  itself is doing its job. The residual is concentrated in 30 outlier
  classes the cache *can't* classify in time, not in any systemic
  per-pair-cost issue.
- **`tableau_subsumption_calls = 0`** — the slow tableau path never
  fires on this workload. Every "miss" is being handled by the hyper
  wedge. The bottleneck is wedge-shape, not tableau-shape.
- **The label_cache build is cheap (2.9%)**, not expensive — the
  natural assumption ahead of probing was that Phase 7 added a
  significant build-cost pass. It did not.
- **The NoVerdict cause is the per-pair budget being reused for a
  per-class call** — a budget-decoupling oversight in the Phase 7
  wiring, not a fundamental engine limitation. (Surfaced only by the
  per-class wall histogram in run 3; the initial single-section probe
  pointed at tier_walk but didn't reach this root cause.)

## Cross-references

- Phase 5 T3b GALEN probe (the template):
  `docs/phase5-downstream-probe.md`.
- Phase 7 results (where the 96% prune rate is documented):
  `docs/phase7-results.md`.
- Head-to-head measurement that motivated the gap target:
  `docs/perf-2026-06-02-konclude-vs-rustdl.md`.
- Per-class label heuristic design (where the deadline-reuse choice
  was made):
  `docs/superpowers/specs/2026-06-02-per-class-label-heuristic-design.md`.
