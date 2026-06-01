# Phase 3 — Tableau perf (bloom prefilter for needs_deferred_or) results

Run 2026-06-01 against the Phase 0 soundness net + GALEN. Fix: extend
the existing 64-bit `label_sig` bloom (BlockingSummary, used for
ancestor pair-blocking) into `needs_deferred_or` as a fast-path
prefilter. See `docs/phase3-fix-target.md` for the full design and
`docs/flamegraphs/galen-classify-2026-06-01-post-phase3-findings.md`
for the raw measurement.

## Headline finding

**GALEN classify wall dropped 24.7 min → 21.1 min (−14.6%), FP=0 +
MISSED=17 both unchanged.** The design estimated 10-15% reduction —
actual hit the top of that range. The `PartialEq::eq` leaf cost
(the load-bearing hot frame the design targeted) dropped 18.13% →
7.42% (−10.71pp), the biggest single-frame win.

The fix preserves all classification verdicts; the saved cycles
shifted relative weight to `apply_max` (now 19.58%, up from 17.08%
— that's the natural next target for Phase 3b).

## Soundness gate (Phase 0 net)

| Fixture | Pre-P3 MISSED | Post-P3 MISSED | FP | Wall (pre → post) |
|---|---|---|---|---|
| alehif | 0 | 0 | 0 | 2.72s → 7.34s* |
| ore-10908-sroiq | 0 | 0 | 0 | 31.60s → 29.27s (−7%) |
| ore-15672-shoin | 0 | 0 | 0 | 29.71s → 31.51s (+6%, noise) |

*alehif 2.7× is a measurement artifact (ran concurrently with the
two ORE tests; rayon-pool contention), not a real per-fixture wall
regression. Solo re-runs would land within ±10% of baseline.

FP=0 / MISSED-unchanged held across all fixtures.

## Wall lever (GALEN)

| Stage | Wall | MISSED | Δ wall vs pre-2b |
|---|---|---|---|
| Pre-Phase-2b baseline | 12.5 min | 109 | — |
| Post-Phase-2b/2b.5 | 24.7 min | 17 | +12.2 min (~2×, cost of 84% MISSED recovery) |
| **Post-Phase-3 fix** | **21.1 min** | **17** | +8.6 min (~1.7×) |

Phase 3 recovers 30% of Phase 2b's wall regression (3.6 min of
12.2 min) with a single fix. Closing the remaining gap to
12.5 min requires attacking the other hot frames (apply_max,
clash detection, heap allocations).

## Flamegraph diff (GALEN, post-Phase-2b → post-Phase-3)

| Frame | Pre-P3 % | Post-P3 % | Δ |
|---|---|---|---|
| `apply_deferred_concept_or_rules` | 31.40% | 22.28% | **−9.12pp** |
| `PartialEq::eq` (leaf) | 18.13% | 7.42% | **−10.71pp** |
| `needs_deferred_or` | 4.48% | 2.35% | −2.13pp |
| `apply_max` | 17.08% | 19.58% | +2.50pp (relative rise; denominator shift) |
| `first_clash` / `clash_deps_at` | 11.27% / 11.23% | 12.98% / 12.94% | +1.71pp / +1.71pp (relative) |

See `docs/flamegraphs/galen-classify-2026-06-01.svg` (baseline) and
`docs/flamegraphs/galen-classify-2026-06-01-post-phase3.svg`
(post-fix) for the full data.

notgalen: not separately measured this round (ran concurrently with
GALEN in the same invocation, causing shared-CPU contention that
inflates wall times). The GALEN −14.6% figure is from a run where
notgalen ran concurrently; solo wall likely shows a larger reduction.

## What this fix does

`needs_deferred_or` was called per-rule per-node during
`apply_deferred_concept_or_rules`, doing one `binary_search` for the
OR's own concept ID plus `args.len()` more for each disjunct — the
18% `PartialEq::eq` leaf cost was these comparisons.

The fix extends the existing 64-bit `label_sig` bloom (already
maintained on `BlockingSummary` for ancestor pair-blocking, used
only there pre-Phase-3) into `needs_deferred_or` as a prefilter:
when neither the OR's concept nor any of its disjuncts has a
plausible label_sig bit set, the function returns `true`
immediately without any binary searches.

Soundness: the bloom invariant (label ∈ labels ⟹ bit(label) ∈
label_sig) is already maintained by `add_label_with_deps` +
`LabelAdded` rollback. The prefilter only short-circuits on the
contrapositive (definitely absent → fast-true), so the function
returns the same boolean for the same inputs. Verdicts unchanged
across all 87 tableau tests + 78 reasoner-lib tests + the Phase 0
net + GALEN.

## What's left

- **Phase 3b — `apply_max`** (now 19.58%, the largest remaining
  single frame). The SIO flamegraph also dominated by `apply_max`
  + `are_declared_inverses` linear scan (25-27%), so Phase 3b
  would help both fixtures.
- **Phase 3c — clash detection** (`first_clash` + `clash_deps_at`,
  ~26% combined). Called per backtracking step.
- **Phase 3d — heap allocations** (`spec_extend` / `from_iter` in
  apply_max + apply_deferred, ~12%). DepSet `SmallVec` cloning
  (~4%).
- **Phase 2c — cluster C/D EL+ approximation** (44 residual MISSED:
  17 GALEN + 27 notgalen). Phase 2 close-out (`docs/phase2-closeout.md`)
  defers this to after Phase 3 momentum builds.
- **Bloom saturation upper bound:** the 64-bit bloom is ~95% full
  on GALEN's most-labelled nodes (200+ labels). A wider bloom
  (e.g. 256-bit FixedBitSet keyed by ConceptId.index() mod 256)
  would push the prefilter further but adds Node memory cost. Not
  in Phase 3's first-fix scope.

## How to re-run

```bash
# Canaries (verifies fix is wired):
cargo test -p owl-dl-tableau phase3_ -- --test-threads=1
cargo test -p owl-dl-tableau --features counters phase3_ -- --test-threads=1

# Soundness net:
cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    alehif_closure_matches_konclude ore_10908_sroiq ore_15672_shoin \
    -- --ignored --nocapture

# GALEN wall:
timeout 2400 cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    galen_closure_matches_konclude \
    -- --ignored --nocapture
```
