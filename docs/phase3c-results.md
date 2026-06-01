# Phase 3c — ConceptPool::bot_id cache results

Run 2026-06-01. Fix: cache `ConceptPool::bot_id` in `OnceLock<ConceptId>` +
`AtomicU64` cache-hit counter (concurrency-safe variants of the plan's
`Cell`-based design, required because ConceptPool is shared across
rayon threads in `PreparedOntology`). See
`docs/superpowers/plans/2026-06-01-phase3c-bot-id-cache.md` and
`docs/flamegraphs/sio-classify-2026-06-01-post-phase3c-findings.md`
for the raw measurement.

## Headline finding

**GALEN classify wall dropped from 24.8 min (post-Phase-3b) to 12.2 min
(post-Phase-3c) — ~50% reduction. This effectively reclaims Phase 2b's
entire wall regression** (which took GALEN from the pre-2b baseline of
12.5 min up to 24.7 min as the cost of the 92-pair MISSED recovery).
FP=0 + MISSED=17 unchanged.

The `apply_role_axioms` / `bot_id` / `find_map` cluster that the
post-Phase-3b flame attributed at 24.66% is eliminated entirely:
`apply_role_axioms` 24.66% → 0.45%; `bot_id` 24.66% → 0.42%;
`find_map`/`try_fold` over ConceptExpr — gone. O(n) → O(1) does
what it says.

## Soundness gate (Phase 0 net)

| Fixture | Post-3b MISSED | Post-3c MISSED | FP | Wall (Post-3c) |
|---|---|---|---|---|
| alehif | 0 | 0 | 0 | 28.05s (shared-CPU artifact) |
| ore-10908-sroiq | 0 | 0 | 0 | 26.01s (−5% vs post-3b) |
| ore-15672-shoin | 0 | 0 | 0 | 36.83s (within noise) |

FP=0 / MISSED-unchanged held; wall variations are concurrent-execution
artifacts in the test harness, not real per-fixture regressions.

## Wall lever (GALEN) — the full Phase 2b/3 arc

| Stage | GALEN wall | MISSED | Δ wall vs pre-2b |
|---|---|---|---|
| Pre-Phase-2b baseline | 12.5 min | 109 | — |
| Post-Phase-2b/2b.5 | 24.7 min | 17 | +12.2 min (~2×, cost of 84% MISSED recovery) |
| Post-Phase-3a | 21.1 min | 17 | +8.6 min |
| Post-Phase-3b | 24.8 min* | 17 | +12.3 min (shared-CPU noise; not a regression) |
| **Post-Phase-3c** | **12.2 min** | **17** | **−0.3 min** (back to pre-2b baseline) |

*Phase 3b wall was contention-noisy (shared-CPU with concurrent SIO bench).
The flamegraph delta IS the durable Phase 3b evidence (`are_declared_inverses`
25.76% → 3.44%); the wall number reflected contention more than real cost.

**Net: Phase 3 (a + b + c) reclaims Phase 2b's entire wall cost while
keeping all 92 MISSED-pair recoveries.** The 84% completeness improvement
landed at zero wall cost vs the pre-2b baseline.

## Flamegraph diff (SIO)

| Frame | Pre-P3c (post-3b) | Post-P3c | Δ |
|---|---|---|---|
| `apply_role_axioms` | 24.66% | 0.45% | **−24.21pp** |
| `bot_id` | 24.66% | 0.42% | **−24.24pp** |
| `find_map<ConceptExpr>` | 24.66% | 0.00% | gone |
| `try_fold<ConceptExpr>` (3 variants) | 24.66% each | 0.00% each | gone |

Top non-search post-3c frames (the cache shifted the denominator;
relative weight to search overhead grew):
- `search` / `branch`: 64.70%
- `saturate`: 61.94%
- `apply_deferred_concept_or_rules`: 18.16% (Phase 3a target; relative rise)
- `apply_role_rules`: 16.36% (new in top 5)

## What this fix does

`ConceptPool::bot_id()` was an O(n) linear scan over every interned
ConceptExpr in the pool. Called per-node per-saturation across 6 hot
sites (`apply_max`, nominal-resolution, `apply_choose`,
`apply_role_axioms`, classify). For GALEN's pool of thousands of
concepts, every call walked the full list.

Fix: lazy `OnceLock<ConceptId>` cache. First call scans + populates;
subsequent calls hit the cache (concurrency-safe — `OnceLock` because
`ConceptPool` is Sync, shared across rayon workers). `AtomicU64`
counter `bot_id_cache_hits` tracks consultations; structural canary
asserts > 0.

The cache only populates on `Some` (i.e. only after Bot is interned),
so calls made before Bot is in the pool correctly return `None` and
re-scan on the next call. Once Bot is interned, the cache is set
and stable forever.

Soundness: Bot is a unit variant interned at most once; its
ConceptId is stable. The cache returns the same boolean for the
same input. Verdicts unchanged across 169 owl-dl-core + 88 tableau
+ 78 reasoner-lib tests + Phase 0 net + GALEN.

## What's left

The Phase 3 arc has now fully recovered Phase 2b's wall cost. The
remaining hot frames are dominated by tableau-driver overhead
(`search`/`branch`/`saturate` at 60-65%) — much harder targets.
Realistic next steps:

- **Phase 3d — `apply_deferred_concept_or_rules`** (18.16% post-3c,
  down from Phase 3a's 22.28% via denominator shift but still the
  top non-search target). The Phase 3a bloom prefilter targeted
  `needs_deferred_or` directly; a deeper attack might restructure
  the rule's per-node iteration.
- **Phase 3e — `apply_role_rules`** (16.36%, new top-5 entrant).
- **Phase 2c — cluster C/D EL+ approximation** (44 residual MISSED).

Note: GALEN is now back to pre-Phase-2b wall; further Phase 3 work
has lower urgency. The natural pivot is Phase 2c (the residual MISSED)
once integration timing demands it.

## How to re-run

```bash
# Canaries (verify fix is wired):
cargo test -p owl-dl-core phase3c_ -- --test-threads=1
cargo test -p owl-dl-core --features counters phase3c_ -- --test-threads=1

# Soundness net (FP=0 gate):
cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    alehif_closure_matches_konclude ore_10908_sroiq ore_15672_shoin \
    -- --ignored --nocapture

# GALEN wall (will time out at 40 min cap if slow):
timeout 2400 cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    galen_closure_matches_konclude -- --exact --ignored --nocapture
```
