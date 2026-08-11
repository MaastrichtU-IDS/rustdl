# Benchmark: per-iteration feature-flag reads in the wedge hot loops

**Date:** 2026-08-11 · **Change:** `88e0634` (`perf(wedge): stop reading feature flags
per-iteration`) · **Baseline:** v0.4.17 (`158fe5a`) plus the pre-change tree
· **Raw data:** [`data-2026-08-11-env-flag-hot-loop-sweep.csv`](data-2026-08-11-env-flag-hot-loop-sweep.csv)
(1,920 rows: outcome, wall, peak RSS per arm)

## What changed

Two feature-flag accessors were called from the wedge's innermost loops, and each does
a `std::env::var_os`, which takes the **process-global `env_read_lock`**. Under rayon,
32 threads serialise on it.

* `hyper_fixpoint_deadline_enabled()` — `horn_fixpoint`'s drain loop. The condition read
  `flag && steps.is_multiple_of(STRIDE)`, and `&&` is left-to-right, so the `getenv` ran
  on **every iteration** rather than every 256th.
* `hyper_match_deadline_enabled()` — `enumerate_matches`' recursion.

Both are now read **once per `HyperEngine`** into struct fields (beside the existing
`at_most_exhaust_probe`), and the cheap stride test is evaluated first.

Per-engine and **not** a process-wide `OnceLock`: the `OnceLock` variant measured
*faster* (9.65× vs 4.69× on the same ontology) but broke the `zero_is_off` canary,
because the canaries set these vars per test and the first test to run wins.

## Method

* Harness: `owl-reasoner-harness` `missed-net.sh sweep`, one invocation per ontology,
  wall + peak-RSS recorded, resumable JSONL, binary sha pinned in the run manifest.
* Population: all **1,920** ORE ontologies (`missed-net/work/all-files.txt`).
* `--cap-secs 60`, `--threads 1`, `JOBS=4`, no classify args (default budgets).
* Host: `fsesrv-g1` (32 cores / 251 GB). **Not** `fsesrv-node000003`.
* Both arms from pinned binaries built from the same tree ± the change.
* Every nominal difference re-measured **min-of-3 on a quiet host** before being
  reported, because `JOBS=4` sweeps have produced contention artifacts throughout this
  design record.

## Corpus result

| | BASE | NEW |
|---|---|---|
| outcomes | 1752 ok / 166 dnf / 2 err_reject | 1754 ok / 164 dnf / 2 err_reject |
| `ok → dnf` | — | **0** |
| `dnf → ok` | — | 2 nominal → **1 genuine** |
| wall over 1,752 both-ok | 3,682 s | **3,478 s (−5.56%)** |
| >1.5× and >2 s faster | — | **18 ontologies** |

## Re-measurement changed two of the four nominal differences

| ontology | sweep | min-of-3, quiet host | verdict |
|---|---|---|---|
| `ore_ont_9299` | dnf → ok | dnf @90 s → **6.15 s** | **genuine recovery** |
| `ore_ont_14351` | dnf → ok | 61.28 → 66.78 s, ok in **both** | 60 s-cap artifact |
| `ore_ont_5927` | 4.74 → 10.11 s | 4.33 → 4.30 s | noise |
| `ore_ont_10838` | 5.28 → 9.31 s | 5.06 → 5.26 s | noise |

**Reported tally: 1 genuine recovery, 0 regressions.** Not 2 and 0.

## Individual speedups (min-of-3, quiet host)

| ontology | base | new | factor |
|---|---|---|---|
| `ore_ont_15010` | 22.95 s | **0.64 s** | **36×** |
| `ore_ont_7203` | 23.67 s | **1.84 s** | **13×** |
| `ore_ont_7775` | 13.76 s | **3.18 s** | **4.3×** |
| `ore_ont_12432` `label_cache_build` | 58,608 ms | 12,491 ms | 4.69× |

## How it was found

Four count-based levers had already failed on this cluster (phase bounding, per-class
budget precision, the divergence early-cut, per-class locality). None could see this,
because `SearchStats` has no counter for it.

Self-time profiling did. `perf` was unavailable on this host, so a gdb-attach sampling
profiler (20 attaches × all threads = 660 thread-stacks) was used:

| self-time | `ore_ont_6134` | `ore_ont_12432` |
|---|---|---|
| `RwLock` read / read_unlock / is_read_lockable | 12% | **76%** |
| allocation + node copying | ~49% | negligible |
| `subset_sorted` ← `is_blocked` | 18% | 0.8% |
| actual reasoning | — | ~3.5% |

Walking up the `12432` stacks gave `is_read_lockable ← RwLock::read ← env_read_lock ←
getenv`. After the fix, `getenv` and `env_read_lock` frames are **0**.

Note the two cluster members have **unrelated** dominant costs — which is why the
second profile was required before acting on the first, and why a single lever cannot
fix this cluster.

## Gates

* Workspace **1,605 pass / 0 fail**.
* FP=0 soundness net: **zero** `FP>0` and zero `MISSED>0` lines.
* Canaries `fixpoint_deadline_default` + `match_deadline_default`: 4 + 4 pass.
* `cargo fmt --check` and `clippy -D warnings` clean.

## Consequences recorded elsewhere

* **Supersedes a known limitation.**
  `docs/known-limitations/label-cache-budget-starved-by-small-pair-timeout.md` records
  `ore_ont_15010` needing `RUSTDL_LABEL_CACHE_TIMEOUT_MS=30000` to go 103.98 s → 5.64 s.
  It is now **0.64 s at defaults with no flags**. That doc's mechanism is not wrong, but
  the wall it diagnosed was mostly `env_read_lock` contention and its worked example no
  longer reproduces — re-measure before relying on it.
* **A per-iteration `getenv` is invisible to every gate this project has** (ΔMISSED,
  FP=0, two-arm outcome sweeps), because it changes no answer. Both offending sites were
  introduced by changes whose own gates all passed.
* Remaining hot-path `std::env::var_os` sites are unaudited: `classify.rs` has 17,
  `owl-dl-reasoner/src/lib.rs` has 89. Most are per-query and harmless; any in a
  per-node or per-clause path costs the same global lock.
