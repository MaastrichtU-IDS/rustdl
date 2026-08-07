# `label_cache_build` is unbounded and dominates 12 of the 13 non-search-bound tail ontologies

**Date:** 2026-08-07 · **Status:** localised, **NOT fixed** — the exact site is not yet found
**Severity:** DNF (no classification at all) on 12 measured ORE ontologies

## The cluster

Of the 39 Set-A tail members Konclude classifies in under 1 s, 13 still DNF even with
per-pair search capped to ~zero (`--pair-timeout-ms 1`), i.e. per-pair search is **not** their
cost. Phase breakdown, single-threaded, forced to emit via `--global-timeout-ms 30000`:

| dominant phase | n |
|---|---:|
| **`label_cache_build`** | **12** (84–100% of wall) |
| `prepare` | 1 (`ore_ont_5368`, the known 18.6 M-axiom DKey case) |

`tier_walk ≈ 0` throughout — **the pair loop never starts**, which is exactly why a per-pair
budget cannot help this cluster. Class counts span 309 → 8,025, so it is not simply "big
ontologies".

## The defect: the phase is not bounded by anything

On `ore_ont_16056` (**309 classes**, 1,425 concept rules) `label_cache_build` is ~34.4 s and is
**invariant** to every knob that should bound or cheapen it:

| configuration | `label_cache_build` |
|---|---:|
| default | 34,392 ms |
| `RUSTDL_LABEL_CACHE_TIMEOUT_MS=1` | 34,356 ms |
| `RUSTDL_LABEL_CACHE_TIMEOUT_MS=10` | 34,352 ms |
| `RUSTDL_CLASSIFY_LABELS_AMORTIZE=0` | 34,377 ms |
| `RUSTDL_CLASSIFY_LABELS_AMORTIZE=1` | 34,347 ms |
| `RUSTDL_CLASSIFY_BACKFOLD=0` | 34,355 ms |
| `RUSTDL_ADAPTIVE_BUDGET=0` | 34,302 ms |
| `RUSTDL_HYPER_INCREMENTAL_FIXPOINT=0` | 35,287 ms |
| `RUSTDL_ITERATIVE_DEEPENING=0` | 34,311 ms |
| `RUSTDL_MAX_NODES=2000` | 34,377 ms |
| `RUSTDL_SAT_SEED=0` | 38,732 ms |

**±0.3% across ten configurations.** With 309 classes and a 1 ms per-class budget the
deadline-bounded *search* can account for at most ~309 ms, so ~34 s is elsewhere.

**It also overruns the global deadline:** under `--global-timeout-ms 25000`,
`label_cache_build` still reports **34.4 s**. The per-class early-return
(`classify.rs:2300`) makes classes return instantly once the global deadline passes, so ≥9 s
ran *inside a single call* past a deadline that call was given.

## What is ruled out — and what that leaves

The deadline **is** plumbed correctly: `classify.rs:2288-2310` computes `cache_ms`, wraps it as
`per_class_cache_dur`, takes `effective_deadline(global_deadline, …)`, and passes it to
`classify_labels`, which hands it to `engine.decide_with_deadline(...)` (`lib.rs:~4110`). So
the *search* is bounded.

Ruled out by measurement: the search itself (deadline-insensitive), engine construction
(`RUSTDL_LABEL_AMORTIZE_MARK` **proves** the amortized path is taken — `engaged` vs
`full-rebuild` — and it changes nothing here), backfold, sat-seed, adaptive budget,
incremental fixpoint, iterative deepening, and the node cap.

That leaves **unguarded work inside `classify_labels` outside `decide_with_deadline`** — the
`extras` seed loops before it, and `satisfiability_labels` / label extraction after it. Both
are per class and neither consults a deadline.

## Contrast that localises the bug

`RUSTDL_LABEL_CACHE_TIMEOUT_MS` **does** work on `ore_ont_15010`: 103.98 s → 5.64 s for
byte-identical output (`docs/known-limitations/label-cache-budget-starved-by-small-pair-timeout.md`).
Same knob, honoured there, ignored here. Whatever differs between those two paths is the
bug's signature, and `15010`/`16056` are a ready-made discriminating pair.

## Why this is a good target despite being unfixed

- **Addressable set of 12**, versus the 1–2 of every other lever examined this week.
- **The fix shape is sound by construction.** The label cache is an *accelerator*: it prunes
  96–100% of pairs. A partial cache means less pruning — slower per pair, never a wrong
  answer. So bounding the phase cannot cost soundness, only time.
- The bound is missing, not mis-tuned, which avoids the `n × F` trade that killed the
  floor-raise idea in the sibling document.

## Next step (not attempted here)

Profile or instrument **inside** `classify_labels`, splitting construct / solve / post. Two
notes for whoever does it: `perf` is unavailable in this environment (`samply` is present),
and the string `match engine.decide_with_deadline(HYPER_WEDGE_DEPTH, deadline) {` occurs
**twice** in `lib.rs`, so a naive single-anchor patch will hit the wrong site — disambiguate
before editing. No code changes were made; the tree is clean.
