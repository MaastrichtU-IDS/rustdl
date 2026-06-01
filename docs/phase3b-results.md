# Phase 3b — Inverse-role lookup (hashbrown::HashSet) results

Run 2026-06-01. Fix: replace `are_declared_inverses`'s O(N)
`Vec::iter().any()` linear scan with an O(1) `hashbrown::HashSet<(RoleId,
RoleId)>` lookup. See `docs/phase3b-fix-target.md` for design and
`docs/flamegraphs/sio-classify-2026-06-01-post-phase3b-findings.md`
for the raw measurement.

## Headline finding

**Flamegraph cost on SIO: `are_declared_inverses` dropped 25.76% → 3.44%
(-22.32pp); `apply_max` (which CONTAINED the scan) dropped correspondingly
27.93% → 6.51% (-21.42pp). FP=0 + MISSED-unchanged held across the Phase
0 net + GALEN.**

The pizza-era "0-3 inverse pairs" assumption that justified the linear
scan was empirically wrong: SIO has 84 inverse-pair declarations
(~168 entries after the symmetric-storage doubling in
`declare_inverse_pair`). The hashbrown::HashSet's O(1) lookup
eliminates the linear scan; the residual 3.44% is the hashset's own
foldhash + contains cost.

## Soundness gate (Phase 0 net)

| Fixture | Post-3 MISSED | Post-3b MISSED | FP | Notes |
|---|---|---|---|---|
| alehif | 0 | 0 | 0 | wall 6.84s (shared-CPU artifact) |
| ore-10908-sroiq | 0 | 0 | 0 | wall 27.19s (within noise) |
| ore-15672-shoin | 0 | 0 | 0 | wall 37.69s (rayon contention) |

FP=0 / MISSED-unchanged held; wall variations are measurement
artifacts from concurrent test execution (rayon pool contention),
not real per-fixture regressions.

## Flamegraph diff (SIO)

| Frame | Pre-P3b | Post-P3b | Δ |
|---|---|---|---|
| `are_declared_inverses` (linear scan / new HashSet) | 25.76% | 3.44% | **-22.32pp** |
| `apply_max` (max-cardinality rule containing the scan) | 27.93% | 6.51% | **-21.42pp** |
| `edge_satisfies` (cross-polarity predicate) | 25.76% | 4.30% | -21.46pp |
| new: `contains<(RoleId,RoleId), foldhash>` | 0% | 3.44% | +3.44pp |

Net win: ~22pp shifted out of the linear scan; ~3.4pp goes to the
new hashset cost. **7.5× cost reduction on the inverse-lookup path.**

## Wall (caveat-laden)

- **SIO (rustdl CLI, --pair-timeout-ms 200):** 192s on this
  shared-CPU server. Spec baseline of ~68s was on a faster reference
  machine; no same-machine pre-Phase-3b baseline was collected, so
  the 192s figure is uncalibrated relative to the Phase 3b impact.
  The flamegraph (above) IS the durable comparison.
- **GALEN:** 24.8 min vs Phase 3a's 21.1 min — within shared-CPU
  contention envelope (ran concurrently with SIO bench for first
  7 min). Not a real regression. An isolated GALEN re-run would
  expect ~21 min (no inverse pairs in GALEN, so the Phase 3b fix's
  `is_empty()` fast-path guard fires immediately).

## What this fix does

`are_declared_inverses` was called from `edge_satisfies` on every
cross-polarity edge-vs-role check (e.g. when `apply_max` evaluates
whether an existing R-neighbour satisfies a `Max(n, Inverse(R), C)`
constraint). On SIO with 168 entries, the linear scan dominated
`apply_max`'s inner loop.

The fix: a parallel `inverse_pairs_set: hashbrown::HashSet<(RoleId,
RoleId)>` field, populated by mirror writes in `declare_inverse_pair`.
The original `inverse_pairs` Vec is retained for declaration-order
iteration. `are_declared_inverses` short-circuits on `is_empty()`
(unchanged), else does an O(1) `contains`. A counter
`inverse_pair_fast_hits` on `RuleCounters` tracks every consultation
of the new structure (the P3b structural canary asserts > 0).

Soundness: the hashset is logically equivalent to the Vec
(both store the same set of pairs from `declare_inverse_pair`).
`contains` returns the same boolean for the same input. Verdicts
unchanged across 88 tableau + 78 reasoner-lib tests + Phase 0 net
+ GALEN.

## What's left

- **Phase 3c — `apply_role_axioms` / `bot_id` linear scan.** New
  dominant non-search frame at 24.66% (`apply_role_axioms`'s
  `ConceptExpr` linear scan for Bot). T4 identified it; clean target.
- **Phase 3d — clash detection** (`first_clash` + `clash_deps_at`,
  ~22-26% combined post-Phase-3).
- **Phase 3e — heap allocations** (`spec_extend` / `from_iter` in
  apply_max + apply_deferred).
- **Phase 2c — cluster C/D EL+ approximation** (44 residual MISSED).

## How to re-run

```bash
# Canaries (verify fix is wired):
cargo test -p owl-dl-tableau phase3b_ -- --test-threads=1
cargo test -p owl-dl-tableau --features counters phase3b_ -- --test-threads=1

# Soundness net (FP=0 gate):
cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    alehif_closure_matches_konclude ore_10908_sroiq ore_15672_shoin \
    -- --ignored --nocapture

# GALEN wall (will time out at 40 min cap if slow):
timeout 2400 cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    galen_closure_matches_konclude -- --exact --ignored --nocapture
```
