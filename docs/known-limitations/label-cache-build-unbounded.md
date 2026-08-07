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

## LOCALISED to a single line, by instrumentation (2026-08-07)

A temporary construct/solve split inside `classify_labels` (since reverted) settles it:

```
[lc] calls=5 construct=0ms solve=17158ms
[lc] calls=6 construct=0ms solve=17158ms
[lc] calls=7 construct=0ms solve=34342ms
```

- **`construct = 0 ms`.** Engine construction is not the cost, confirming the amortization
  result independently.
- **`classify_labels` is called only 7 times** on a 309-class ontology — the rest are skipped
  by the global-deadline early return. So this is not "many small calls".
- **Individual `solve` calls take ~17 s each** (calls 1–5 total 17,158 ms; call 7 alone adds
  ~17,184 ms) against a **1 ms** label-cache deadline.

**The cause is `hyper.rs:2899-2903`: `fn solve` checks the deadline exactly once, at entry,
and that is the ONLY runtime deadline check in the whole hyper engine.** A `solve` frame whose
work stays inside its own body therefore never re-consults it.

### A fix was attempted at the obvious place and did NOT work — recorded so it is not retried

Adding a strided deadline check inside `horn_fixpoint`'s worklist drain (mirroring the
saturator's shipped `DEADLINE_CHECK_STRIDE = 4096`) left `label_cache_build` at
**34,298 ms vs 34,392 ms** — unchanged — and the ontologies still DNF. Verified the patch was
in the binary and force-rebuilt before concluding.

**So the 17 s is inside a single event, not spread across worklist events.** That excludes the
drain loop and points at `process_event` (`:2188`) and below it `match_body` (`:4104`) /
`enumerate_matches` (`:4135`) — the non-Horn fire loop already documented as ~25% of self
time. The change was reverted; the tree is clean.

### Why the remaining fix needs care rather than another quick patch

`enumerate_matches` and `match_body` take `&self` and *return matches*. Bailing out of them
early drops matches, and a dropped match must surface as **`Stalled`** — not as a silent
`Sat`. A silent `Sat` would still be sound in the FP sense (fewer derived facts ⇒ fewer
clashes ⇒ a MISS) but it would be **silently incomplete without the `incomplete` flag**, which
is precisely the failure mode this codebase treats as worse than a DNF. So the fix is a
plumbing change through the match path, not a one-line stride.

## Next step (not attempted here)

Add a deadline check inside the match/fire path (`process_event` → `match_body` /
`enumerate_matches`), propagating exhaustion as `HyperResult::Stalled` so the `incomplete`
signal is preserved. Then re-measure `ore_ont_16056` (`label_cache_build` must fall below the
label-cache budget) and re-run the 12-ontology cluster.

Three traps that cost time here, for whoever continues:
- `perf` is unavailable in this environment (`samply` is present).
- `match engine.decide_with_deadline(HYPER_WEDGE_DEPTH, deadline) {` occurs **twice** in
  `lib.rs` — line 4100 is `classify_labels`, line 4461 is `base_model_types`. A single-anchor
  patch silently hits the wrong site; patch by line number.
- `cargo build` reporting `Finished … in 0.10s` means **nothing was rebuilt**. Verify with
  `strings target/release/rustdl | grep <marker>` or `touch` the file before trusting a
  measurement.

No code changes ship from this investigation; the tree is clean and rebuilt.
