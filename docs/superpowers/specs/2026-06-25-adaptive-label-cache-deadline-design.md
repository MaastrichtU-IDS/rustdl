# Adaptive label-cache deadline — build-once tuning — Design

**Second increment of the build-once direction** (after MRV shipped). The label cache is a
build-once structure: one per-class `sat(C)` yields `labels(C)`, pruning all of C's pairs (O(n)
builds replacing O(n²) refutations). The build's per-class deadline (currently a fixed 1000 ms
default) cuts off wine's hard nominal classes — which MRV now makes *terminate* (1–16 s) — leaving
them unlabeled → their pairs fall through to per-pair refutation. This increment makes the deadline
**adaptive** so the now-tractable hard classes get labeled, capturing the validated ~13% wine wall win.

## Evidence (validated this session)

Wine classify @200 ms/pair, MRV on, label-cache deadline 5 s / 30 s / 60 s → net wall **343 / 300 /
310 s** (misses 4009 / 2652 / 2098; tier-walk 333 / 258 / 237 s monotone-down; build 10 / 42 / 73 s
monotone-up). Net is U-shaped, **optimum ~30 s (~13% off the default)** — a real net win (tier-walk
savings exceed the build cost up to ~30 s), bounded past it by the hard-class tail. `n_classes ×
per_pair` (137 × 200 ms = 27 s) matches the optimum, giving a principled, self-tuning formula.

## Soundness (perf-only, by construction)

This changes **only how long the per-class label build runs**, not what it computes. The label oracle
is the existing Phase-7 mechanism (already default-ON, FP=0-validated): `LabelOracle::Sat(labels)`
prunes a pair (C,D) iff `D ∉ labels(C)` — sound because the satisfying model witnesses `C ⊓ ¬D` (a
counter-model to `C ⊑ D`). A label-cache **miss** (`NoVerdict`) falls through to per-pair refutation —
also sound. So building *more* labels only converts genuine non-subsumptions from "refuted per-pair"
to "pruned by oracle": **the classification closure is byte-identical (FP=0/MISSED=0 unchanged)** — only
fewer tableau tests are issued. No new soundness surface; the corpus gate confirms byte-identity at
scale. (The label oracle's `trust_sat` reliance is pre-existing and unchanged.)

## Mechanism

At the label-cache build site (`classify.rs`, where `per_class_cache_dur` is currently
`label_cache_timeout_ms()`), compute the per-class deadline adaptively from the inputs already in
scope (`n` = class count, `per_pair_timeout: Option<Duration>`):

```
adaptive_ms =
    if RUSTDL_LABEL_CACHE_TIMEOUT_MS is set explicitly → that value (env always wins, unchanged)
    else:
        let base = per_pair_timeout.map(|d| d.as_millis() as u64).unwrap_or(CEILING_MS);
        // refute-the-row break-even: labeling C is worth it iff sat(C) < cost of refuting C's
        // ~n pairs (each ≤ per_pair). Cap at the validated ceiling; floor at the prior default.
        (n as u64 * base).clamp(DEFAULT_FLOOR_MS, CEILING_MS)
```
- `DEFAULT_FLOOR_MS = 1000` (the current default — never go below current behaviour).
- `CEILING_MS = 30_000` (the validated optimum; beyond it the build cost overtakes the tier-walk
  savings — see Evidence).
- `per_pair_timeout == None` (explicit unbounded `--pair-timeout-ms 0`): use `CEILING_MS` as `base`
  (generous-but-bounded; refutations are unbounded-expensive there so labeling is most valuable, but
  the ceiling bounds the build).
- The existing `effective_deadline(global_deadline, per_class_cache_dur)` still caps each per-class
  deadline at the global deadline — unchanged.

For wine (n=137): at 200 ms/pair → `min(27.4 s, 30 s)` = 27.4 s ≈ the measured optimum; at the CLI
default 1 s/pair → `min(137 s, 30 s)` = 30 s. Fast fixtures: their classes' label-`sat` is ≪ 1 s, so
the larger deadline never binds → no build-time regression.

## Components

- `crates/owl-dl-reasoner/src/lib.rs`: keep `label_cache_timeout_ms()` for the explicit-env path;
  add the `CEILING_MS`/`DEFAULT_FLOOR_MS` consts (or inline). The adaptive computation lives at the
  call site (it needs `n` + `per_pair_timeout`).
- `crates/owl-dl-reasoner/src/classify.rs`: at the `per_class_cache_dur` assignment (~1221), replace
  the fixed `label_cache_timeout_ms()` with: explicit-env value if set, else the adaptive
  `clamp(DEFAULT_FLOOR_MS, CEILING_MS)` of `n × per_pair_base`. A unit test on a small extracted
  `fn adaptive_label_cache_ms(n, per_pair, env_override) -> u64` (pure function — easy to test the
  clamp/floor/ceiling/env-override/None branches).

## Testing / gate

1. **Unit:** `adaptive_label_cache_ms` — env-override wins; `n×per_pair` clamped to [1s, 30s]; None
   per-pair → ceiling; tiny n → floor. (Pure function, exhaustive branch coverage.)
2. **Corpus FP=0 byte-identical (soundness confirmation):** `konclude_closure_diff` across all oracled
   fixtures with the adaptive deadline → FP=0/MISSED=0 byte-identical (expected by construction; the
   closure must not change — only the test count does).
3. **Wine net-wall improvement (the point):** wine classify wall, default (pre-change) vs adaptive →
   confirm the ~13% improvement (≈343→~300 s at 200 ms/pair) and that misses drop.
4. **No fast-fixture regression:** classify wall on galen/notgalen/ore/sio, before vs after → no
   material build-time increase (their label-`sat`s are ≪ the floor, so the larger ceiling never
   binds).

## Success criteria

`adaptive_label_cache_ms` unit-tested; corpus FP=0/MISSED=0 **byte-identical** (closure unchanged);
wine net-wall improves (~13%); no fast-fixture regression. On pass it ships (default behaviour —
no flag needed, since it's sound + a strict improvement bounded by the floor); the env override
`RUSTDL_LABEL_CACHE_TIMEOUT_MS` is retained for manual control. If any fixture's closure changes
(it must not) or any regresses, revert and diagnose.

## What this is NOT

Not a new soundness surface (same label oracle, just more complete). Not a flag (it's a sound,
floored, strict improvement — but the env override stays). Not a wine collapse — a validated ~13%
incremental on top of MRV, bounded by the irreducible hard-class tail.
