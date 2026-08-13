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


## RE-RUN at a 60 s cap — and a third of the census changed bucket

Raw data: [`data-2026-08-12-dnf164-phase-census-60s.csv`](data-2026-08-12-dnf164-phase-census-60s.csv)
(carries both the 60 s and 20 s labels per ontology). Same harness, same pinned binary;
the **only** change is `--global-timeout-ms 20000 → 60000`.

| bucket | 20 s | **60 s** | median ms | med classes | med RSS |
|---|---|---|---|---|---|
| `label_cache_build` | 75 (46%) | **88 (54%)** | 53,326 | 15,116 | 0.44 GB |
| `tier_walk` | 11 (7%) | **27 (16%)** | 44,779 | 1,405 | 0.06 GB |
| `no-banner` | 36 (22%) | **24 (15%)** | — | — | 0.94 GB |
| `prepare` | 28 (17%) | **13 (8%)** | 31,155 | 28,465 | 1.44 GB |
| `unsat_probe` | 13 (8%) | 6 (4%) | 46,421 | 650 | 0.05 GB |
| `saturate` | 1 (1%) | 5 (3%) | 7,140 | 232,084 | 1.89 GB |
| `sweeps` | 0 | 1 (1%) | 44,974 | 35,196 | 1.37 GB |

**50 of 164 ontologies (30%) were reclassified.** The old `no-banner` 36 split into 24
still-silent, 8 `prepare`, 4 `saturate` — but the churn is much wider than that bucket:
`tier_walk` **more than doubled** (11 → 27) and `prepare` **halved** (28 → 13).

### The method has a structural limit, and this is it

The phases run **in sequence**, so "dominant phase" is really **"the phase the budget ran
out in"**. A short cap systematically over-attributes to early phases (`prepare`,
`saturate`) and starves late ones (`tier_walk`, `sweeps`) — which is exactly the observed
direction: at 60 s, ontologies get *past* the label cache and into the pair loop, and
`tier_walk` grows accordingly, while `prepare`-labelled members turn out to have merely
been early in their run.

**Consequence: a phase share from this method is not cap-independent and must always be
quoted with its cap.** What is stable is (a) which phases appear at all, (b) that
`label_cache_build` is the largest bucket at both caps, and (c) the per-ontology labels in
the CSV for the members that reported.

### What to plan against

* **`label_cache_build` 88 (54%)** — largest at both caps, so the ranking is robust even
  if the share is not. But the bucket profiles showed its *causes* differ internally
  (blocking on one member, `enumerate_matches` on three, graph cloning on exactly one), so
  this sizes an opportunity, not a lever.
* **`tier_walk` 27 (16%)** is 2.5× larger than the 20 s census suggested, which materially
  raises the value of the diagnosed-but-unbuilt surrogate-atom absorption work
  (`ore_ont_10019` sits here). At 11 members it was hard to justify; at 27 it is the
  second-largest bucket.
* **`no-banner` 24 (15%)** is the genuinely-unbounded class — not the 36/22% first
  reported. Small median classes are unavailable (they never report), but median RSS
  0.94 GB puts them with the heavy end.
* **`prepare` 13 (8%)** is half its earlier size, and the "39% stall in preprocessing"
  claim is now doubly unsupported: 13 + 24 = 37 (23%) even if every silent member were
  preprocessing, which the 4-member probe showed they are not.


## THIRD run at 120 s — the method converges, and `prepare` was almost entirely an artifact

| bucket | 20 s | 60 s | **120 s** | 60→120 |
|---|---|---|---|---|
| `label_cache_build` | 75 | 88 | **91 (55%)** | +3 |
| `tier_walk` | 11 | 27 | **35 (21%)** | **+8** |
| `no-banner` | 36 | 24 | **19 (12%)** | −5 |
| `saturate` | 1 | 5 | **8 (5%)** | +3 |
| `unsat_probe` | 13 | 6 | **4 (2%)** | −2 |
| `sweeps` | 0 | 1 | **4 (2%)** | +3 |
| `prepare` | 28 | 13 | **3 (2%)** | **−10** |

**Reclassification rate is falling: 30% (20→60), then 18% (60→120).** So the attribution
does converge rather than drift indefinitely — the earlier worry that this method
saturates is not borne out. `label_cache_build` is effectively stable (75 → 88 → 91).

Top flows 60→120: `prepare → label_cache_build` (10), `label_cache_build → tier_walk` (8),
`no-banner → saturate` (3).

### Predictions were registered in advance and 2 of 4 were wrong

| prediction | outcome |
|---|---|
| `label_cache_build` shrinks (median 53 s of a 60 s cap ⇒ mostly capped) | **WRONG** — grew 88 → 91 |
| `tier_walk` grows | ✓ 27 → 35 |
| `no-banner` shrinks to ~15–20 | ✓ 19 |
| `prepare`/`saturate` stable (early phases either finish or don't) | **WRONG both** — `prepare` collapsed 13 → 3, `saturate` grew 5 → 8 |

The `prepare` error is the instructive one. I reasoned that an early sequential phase
either completes or doesn't, so its count should be cap-insensitive. Wrong: a phase is
labelled dominant only until a *later* phase outgrows it, so `prepare` members were
steadily reclassified as their label-cache work overtook a fixed ~30 s preparation cost.
**An early phase's count is the most cap-sensitive, not the least.**

### `prepare` is not a class — retract it

At 28 members (17%) it looked like a major unexamined cluster; at **3 (2%)** it is noise,
and 10 of its 13 60-second members flowed into `label_cache_build`. Every claim built on
it is withdrawn:

* **"39% stall in sequential preprocessing"** — already withdrawn once; the residue
  (`prepare` 3 + `no-banner` 19 = 22, **13%**) is a third of the original claim even
  taking every silent member as preprocessing.
* The proposal to "investigate the single-threaded preprocessing class first" is
  **retracted**. The single-threadedness observation (12 stacks vs 396) stands as a
  measurement, but it describes ~13% of the tail, not 39%, and the biggest part of what it
  described has turned out to be label-cache work.

### What to plan against — final

* **`label_cache_build` 91 (55%)** — stable across all three caps and the clear target.
* **`tier_walk` 35 (21%)** — still growing (11 → 27 → 35), so 35 is a *lower* bound. This
  is the strongest remaining argument for the diagnosed-but-unbuilt surrogate-atom
  absorption work: at 11 members it was unjustifiable, at ≥35 it is the second-largest
  cluster.
* **`no-banner` 19 (12%)** is the genuinely-unbounded class — nothing reports it even at
  120 s.
* `saturate` 8, `unsat_probe` 4, `sweeps` 4, `prepare` 3 are individually too small to
  plan against.


## CRITICAL CORRECTION: all three censuses are CONTENTION-DISTORTED

Asked whether `unsat_probe` deserved the same absolute-cost treatment that found the
duplicate saturation in `prepare`, its four apparent outliers were re-measured **alone**.
They do not reproduce:

| ontology | census `unsat_probe` | measured ALONE | real dominant phase |
|---|---|---|---|
| `ore_ont_934` | 103.5 s | **4.1 s** | `tier_walk` 112.7 s |
| `ore_ont_7828` | 118.1 s | **21.3 s** | `tier_walk` 98.1 s |
| `ore_ont_10517` | 117.9 s | **20.9 s** | `tier_walk` 98.8 s |
| `ore_ont_8273` | 96.4 s | **10.0 s** | `tier_walk` 104.7 s |

Same binary, same 120 s budget. Verified this is **not** the closure-reuse change: the
pre-fix and post-fix binaries give identical breakdowns on `ore_ont_934`
(unsat_probe 4,078 vs 4,073 ms; tier_walk 112,671 vs 112,629 ms; 110 rows both).

### The cause: 4× thread oversubscription

`missed-net.sh` runs `JOBS=4` concurrent processes, and each `rustdl` uses **default rayon
parallelism (32 threads)** — so 4 × 32 = **128 threads on 32 cores**. The harness's
`--threads 1` governs its own dispatch, not the reasoner's internal fan-out.

The distortion is **non-uniform and severe**, which is why it was not visible as a simple
scale factor:

* early phases inflate — `ore_ont_934`'s `label_cache_build` 3.3 s → 16.5 s (5×) and
  `unsat_probe` 4.1 s → 103.5 s (**25×**), because the unsat probe fans 108 per-class
  tableau probes across rayon and is worst hit by oversubscription;
* later phases are **starved to zero** — `tier_walk` 112.7 s → **0**, because the budget
  was exhausted before it began.

So contention does not merely scale the numbers; it **reassigns the dominant phase**.

### What is invalidated

* **All phase *values* in all three censuses** (20 s / 60 s / 120 s) are
  contention-inflated by an unknown, per-phase-varying factor.
* **The dominance partition is distorted**, on top of the cap sensitivity already
  documented. The `unsat_probe` bucket (13 → 6 → 4) is now believed to be **entirely
  artifactual** — all four members are really `tier_walk`.
* **The absolute-cost totals are not usable**: `label_cache_build` 10,051 s,
  `tier_walk` 3,326 s, `unsat_probe` 735 s, `sweeps` 326 s. Directions may hold; magnitudes
  do not.

### What survives, and why

Everything measured on **single runs** is unaffected:

* **The duplicate saturation in `prepare`** — found and fixed. Both halves were measured
  individually (`ore_ont_8475`: 46,836 ms then 46,318 ms) and the fix verified on single
  runs (95,169 → 48,347 ms). The `1,082 s` tail-wide total came from census data and is
  therefore suspect, but the per-ontology halving is real.
* **The env-flag hot-loop fix** — single-run measurements plus a two-arm sweep where *both*
  arms carried the same contention, so the comparison holds even if the absolutes do not.
* **The bucket profiles** (gdb sampling, single runs) — including the kill of the
  wedge-trailing lever and the `subset_sorted`/`enumerate_matches` findings.
* **The 164-ontology DNF membership itself** — from the two-arm sweep, where contention
  applied equally.

### And a real finding falls out of the correction

`tier_walk` takes **98–113 s on ontologies of 108–904 classes**. That is pathological on
its face, and it is the *same* bucket as `ore_ont_10019` and the diagnosed-but-unbuilt
surrogate-atom absorption work. Four ontologies just moved into it from `unsat_probe`, and
it was already growing across caps (11 → 27 → 35). It is now the best-supported target in
the tail — arrived at, ironically, by correcting the instrument rather than by any lever.

### Fix for any future census

Pin the reasoner's fan-out so total threads ≈ cores: `RAYON_NUM_THREADS=8` with `JOBS=4`,
or `JOBS=1` and accept the wall. **Record both numbers** — the skill already says "record
the thread pin" for RSS; this shows it governs *phase attribution* too.


## RETRACTION of the "contention-distorted" correction — the census was RIGHT

The section above claims all three censuses are contention-distorted by 4×32=128 threads on
32 cores, and that the `unsat_probe` bucket is artifactual. **Both claims are false.**

`missed-net.sh` passes `--threads 1` to the harness, and the harness sets
**`RAYON_NUM_THREADS=1`** on the reasoner — verified by reading `/proc/<pid>/environ` of a
live census child: `RAYON_NUM_THREADS=1`, 2 threads in the process. So the census is
*under*-subscribed (≈4-8 threads on 32 cores), not over.

The real explanation is **thread count, not contention.** The census runs single-threaded;
my "measured alone" re-runs used default parallelism (32 threads). Reproducing `ore_ont_934`
at each setting:

| threads | `label_cache_build` | `unsat_probe` | `tier_walk` |
|---|---|---|---|
| **1** | **16,469 ms** | **103,528 ms** | **0** |
| 8 | 4,714 ms | 14,072 ms | 101,209 ms |
| 32 | 3,927 ms | 4,077 ms | 112,020 ms |
| *census recorded* | *16,454* | *103,541* | *0* |

The single-thread row matches the census to within 0.1%. The census was a **valid
single-threaded profile**; the mismatched measurement was mine.

### The genuine finding underneath

**Phases parallelise at very different rates**, so *which phase to optimise depends on the
thread count*:

* `unsat_probe` — **25×** from 1→32 threads (103.5 s → 4.1 s). Near-perfectly parallel; it
  is a per-class independent probe loop.
* `label_cache_build` — only **4.2×** (16.5 s → 3.9 s).
* `tier_walk` — *appears* from nowhere at ≥8 threads, because at 1 thread the budget is
  exhausted before it starts.

Consequences:

1. **Both frames are legitimate and they rank differently.** Single-thread is the right
   frame for reproducible comparison (and matches the `--threads 1` condition the DNF list
   itself was produced under). Default parallelism is the right frame for user-facing
   priority. On `ore_ont_934` the former says `unsat_probe`, the latter says `tier_walk`.
2. **`label_cache_build` scaling only 4.2× while `unsat_probe` scales 25× is itself a
   target** — and it is consistent with `label_cache_build` being the largest bucket at every
   cap. A phase that refuses to parallelise on a 32-core machine is a better lever than one
   that already does.
3. **The `unsat_probe` bucket is real, not artifactual** — in single-threaded terms. Its four
   members were correctly classified.

### What this costs the record

The `unsat_probe`-is-artifactual claim, and the sweeping "all phase values are
contention-inflated", are withdrawn. The absolute-cost totals (`label_cache_build` 10,051 s,
`tier_walk` 3,326 s, `unsat_probe` 735 s) are **valid single-thread figures** and may be
quoted as such.

**Method lesson, and it is the same one twice:** I diagnosed a discrepancy between two of my
own measurements by inventing a mechanism (contention) instead of *reading the environment of
the running process*. One `cat /proc/<pid>/environ` — which the harness makes trivial because
it records and controls the pin — would have shown `RAYON_NUM_THREADS=1` immediately. Check
what the measurement actually did before theorising about why it disagrees.
