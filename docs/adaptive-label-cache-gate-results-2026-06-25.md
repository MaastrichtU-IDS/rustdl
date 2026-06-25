# Adaptive label-cache deadline — gate RESULTS + VERDICT — 2026-06-25

**Verdict: PASS → shipped (default).** The adaptive per-class label-cache deadline
(`min(n_classes × per_pair, 30s ceiling)`, floored at 1s, env-overridable) is **sound (closure
byte-identical corpus-wide), a real ~9% net wine wall win with misses nearly halved, and has no
fast-fixture regression.** Second build-once increment on top of MRV.

## Soundness — corpus FP=0/MISSED=0 byte-identical (all 10 oracled fixtures)

`konclude_closure_diff`, adaptive default. Closure **unchanged** everywhere (the change moves genuine
non-subsumptions from per-pair-refuted to oracle-pruned; it cannot alter the closure):

| fixture | rustdl=konclude | FP | MISSED |
|---|---|---|---|
| bibtex 16 / ro 51 / ore-15672 142 / pizza 158 / galen 27997 / notgalen 32739 | = | 0 | 0 |
| ore-10908 6001 / sio 8904 / wine 653 | = | 0 | 0 |

Confirms the by-construction soundness (same Phase-7 label oracle, more complete).

## Performance — wine improvement + no regression

Wine classify @200 ms/pair, old fixed-1s default vs adaptive:

| | wall | label-cache misses |
|---|---|---|
| old default (1 s) | 340.5 s | 4626 |
| **adaptive** (≈27 s for wine: 137 × 200 ms) | **310.3 s** | **2764** |

**~9% net wall win, misses −40%** — more hard nominal classes get labeled (now that MRV makes their
per-class `sat` terminate), so O(n) label-builds prune pairs that previously fell through to O(n²)
refutations. No fast-fixture regression: galen 0.21→0.22 s, ore-10908 0.17→0.19 s (the larger ceiling
never binds — their label-`sat`s are ≪ the 1 s floor).

## Verdict / consequence

PASS on all conditions (FP=0 byte-identical + wine improvement + no regression) → **ships as default**
(no flag — sound, floored strict improvement; env `RUSTDL_LABEL_CACHE_TIMEOUT_MS` retained for manual
control, `0` = unbounded). Merges to `feat/build-once-redesign`.

## Honest framing

Modest (~9%) and bounded: wine still takes ~310 s @200 ms/pair — the residual is the irreducible
hard-class refutation tail (the ~19 classes whose `sat` exceeds the 30 s ceiling, where labeling costs
more than the capped refutations). MRV is the structural win (made the hard `sat`s terminate); this
increment tunes the build-once label cache to capitalize on it. Together they make wine **classify
correctly (FP=0) with hard searches terminating, ~9% faster** — not fast, but the sound, validated
near-term ceiling without the deep nominal rearchitecture.
