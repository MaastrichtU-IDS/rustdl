# Phase census of the DNF tail (164 ontologies)

**Date:** 2026-08-12 · **Binary:** `fa46c75` (v0.4.17 + the env-flag hot-loop fix)
· **Raw data:** [`data-2026-08-12-dnf164-phase-census.csv`](data-2026-08-12-dnf164-phase-census.csv)
· **Population:** the 164 ontologies that DNF in
[`data-2026-08-11-env-flag-hot-loop-sweep.csv`](data-2026-08-11-env-flag-hot-loop-sweep.csv)
(cap 60 s, `--threads 1`)

## Why this exists

Five successive levers were measured out on "the DNF tail" (phase bounding, per-class
budget precision, the divergence early-cut, per-class locality, and — before them — a
series of absorption ideas). Profiling two members then found **unrelated dominant
costs** in each. That makes any lever proposal naming "the DNF set" unfounded until the
set is partitioned, which is what this does.

Method: run each with `--global-timeout-ms 20000`, which makes a would-be DNF return
*with* its phase banner, and take the largest recorded phase. Orchestrated by
`owl-reasoner-harness` (`missed-net.sh sweep … --global-timeout-ms 20000`); the banner is
read from the harness's captured per-item `.out` files. Peak RSS is taken from the
2026-08-11 sweep rather than re-measured.

## The partition

| dominant phase | n | share | median ms | median classes | median peak RSS |
|---|---|---|---|---|---|
| `label_cache_build` | **75** | 46% | 17,560 | 8,022 | 0.26 GB |
| **`no-banner`** | **36** | 22% | — | — | 1.19 GB |
| `prepare` | **28** | 17% | 14,230 | **58,278** | 0.72 GB |
| `unsat_probe` | 13 | 8% | 13,104 | 831 | 0.02 GB |
| `tier_walk` | 11 | 7% | 12,478 | 1,040 | 0.03 GB |
| `saturate` | 1 | 1% | 7,186 | 232,084 | 1.89 GB |

## Reading the buckets

**`label_cache_build` (75, 46%) is the single biggest cluster**, and median 8,022 classes
says why: the phase costs `n × per-class`. This is the cluster already characterised in
`docs/2026-08-11-label-cache-expensive-classes.md`, where the root cause on `ore_ont_6134`
is `HyperEngine::save` cloning the whole ~6,000-node graph per branch (~3.2 M `HyperNode`
copies). **But note that document's own warning:** a second member, `ore_ont_12432`, was
instead dominated by `env_read_lock` and was fixed by caching two feature flags. So even
within this bucket the causes differ, and 75 members should not be assumed to share one.

**`no-banner` (36, 22%) is a measurement failure, not a phase.** All 36 emit **exactly 0
bytes** and die on SIGTERM. Two hypotheses were tested:

* *Conversion-bound?* **Refuted for 2 of 3 probed** — `ore_ont_10621` converts in 1.11 s
  and `ore_ont_11196` in 0.39 s via `tbox-stats` (convert+absorb only), yet both emit
  nothing. (`ore_ont_10689` at 58.09 s is genuinely conversion-heavy, so the bucket is
  mixed.)
* *So what is it?* The banner prints **only when classify returns**. These are the
  ontologies where **`--global-timeout-ms` does not take effect at all** — the deadline is
  not honoured, so `timeout` kills the process first. That is a documented-behaviour gap:
  the flag's help promises a "hard *give me whatever you have in N ms* bound".

Sampling `ore_ont_10621` inside the stall captured only **16 thread-stacks** — it is
**single-threaded** there, in hashbrown internals (`find_inner`, `insert_tail`,
`lowest_set_bit`). Sequential preprocessing, not the parallel classify.

**Which makes `no-banner` and `prepare` plausibly the same phenomenon**, split only by
whether the run happened to return before the kill. If so, **64 of 164 (39%) stall in
sequential preprocessing and never reach parallel reasoning at all.** `prepare`'s median
of **58,278 classes** (and `saturate`'s single member at 232,084) is consistent with
that: these are very large ontologies where `PreparedOntology::from_internal` — a full EL
saturation plus `HyperCache::build` plus `ConsistencyCache::build` plus NNF plus absorb —
dominates.

**`unsat_probe` (13) and `tier_walk` (11)** are small and have small ontologies (median
831 / 1,040 classes) and tiny RSS (0.02–0.03 GB). `tier_walk` is the bucket containing
`ore_ont_10019`, the diagnosed-but-unbuilt surrogate-atom absorption case.

## What this changes

1. **No preprocessing phase is deadline-bounded.** 39% of the tail is plausibly there,
   and for the `no-banner` 22% the user-facing `--global-timeout-ms` contract is not
   honoured. Bounding preprocessing would convert 36 silent kills into partial answers
   *and* make them measurable — currently they cannot even be attributed.
2. **`prepare` (28) and `no-banner` (36) have never been investigated.** Every lever to
   date targeted `label_cache_build` or `tier_walk` — together 86 of 164 — while the
   64-ontology preprocessing class went unexamined.
3. **The wedge-trailing target is worth its cost only if it generalises.** It came from
   one member of the 75-strong `label_cache_build` bucket, and the second member sampled
   had a different cause entirely. Profile more of the 75 before committing.
4. **RSS is not co-located with time.** The `no-banner` and `prepare` buckets carry the
   memory (1.19 / 0.72 GB medians, up to 18.52 GB) while `unsat_probe` and `tier_walk` sit
   at 0.02–0.03 GB. A memory fix and a time fix address different ontologies.

## Caveats

* **A 20 s cap makes phase *shares* comparable, not absolute walls.** A phase that
  dominates at 20 s might not at 60 s. The dominant-phase label is the claim here; the
  millisecond columns are for ordering within a bucket.
* **`--global-timeout-ms` is itself unreliable on this population** — that is finding #1,
  and it means the 128 ontologies that *did* report were bounded while 36 were not. The
  partition of the reporting 128 is sound; the 36 are classified only as "unbounded".
* Single-thread outcome (the DNF list) but **default parallelism** during the census, so
  RSS from the sweep and phase timings here come from different thread configurations.


## CORRECTION (2026-08-12, same day): the `no-banner` bucket is largely a CAP ARTIFACT

Finding #1 above — "22% of the tail is where `--global-timeout-ms` does not take effect"
— **overstates the case, and the census's own 20 s cap is why.** Re-probing 4 of the 36
`no-banner` members at a **60 s** cap:

| member | at 20 s | at 60 s | dominant |
|---|---|---|---|
| `ore_ont_14459` | silent | **reports** | `saturate` 9,953 ms |
| `ore_ont_20` | silent | **reports** | `sweeps` 15,579 ms |
| `ore_ont_7507` | silent | **reports** | `prepare` 45,673 ms |
| `ore_ont_10621` | silent | **still silent** | genuinely unbounded |

**3 of 4 were simply slower than 20 s**, not unbounded. So `no-banner` is not a failure
mode; it is *"did not finish within the census cap"*, and its members redistribute across
the real phases — including `sweeps`, which the original partition showed as **zero**
members and which is in fact `ore_ont_20`'s dominant phase at 15.6 s.

**What survives:** `ore_ont_10621` is silent even at 60 s, so a genuinely unbounded class
exists — but the 36-member, 22% figure is not its size, and the honest count from this
sample is nearer **1 in 4 of the bucket**, i.e. ~9 ontologies. Do not quote 22%.

**What this does to the derived claims:**

* **"39% stall in sequential preprocessing" is not supported.** It combined the 28
  `prepare` members with all 36 `no-banner` on the strength of one sampled member. Of the
  4 now probed, one is `prepare`-dominated (`7507`, 45.7 s — the largest single prepare
  figure seen), one is `saturate`, one is `sweeps`, and one is unknown. The `prepare`
  class is real and `7507` strengthens it, but the 39% aggregate was an inference, not a
  measurement.
* **The single-threaded finding is unaffected** — 12 stacks vs 396 is a direct
  observation, independent of which phase owns the wall.
* **The `label_cache_build` (75) and `tier_walk` (11) buckets are unaffected**; they
  reported and were attributed.

**Method lesson, recorded because it cost a wrong headline:** a cap chosen to make a
census affordable *becomes a classifier*, and every item that fails to report is silently
assigned to a bucket that looks like a finding. The fix is to re-probe the
non-reporting bucket at a larger cap before naming it — which is the same
"a timeout is not a neutral sampler" rule the corpus-measurement skill already states for
populations, applied here to *attribution*.
