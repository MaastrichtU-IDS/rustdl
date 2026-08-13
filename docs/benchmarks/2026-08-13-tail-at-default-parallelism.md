# Is the 164-ontology DNF tail a 1-thread measurement artifact?

**Date:** 2026-08-13 · **Answer: no.** 139 of the 164 (85%) still DNF at default parallelism.

## Why this was measured before any further work

Every number in the 2026-08-08 → 2026-08-13 arc is single-threaded. The harness
(`owl-reasoner-harness`, via `missed-net.sh`) hardcodes `--threads 1`, which pins
`RAYON_NUM_THREADS=1`. That covers the 164-ontology tail definition, all three phase
censuses (20 s / 60 s / 120 s), the bucket profiles, the peer triage, and the MISSED
baseline of 5,198.

Users do not run that way. `rustdl` classifies with rayon at default parallelism, and the
gap is not small: `ore_ont_934`'s `unsat_probe` alone goes **103.5 s → 4.1 s** from 1 to 32
threads, a 25× swing. If the tail were substantially smaller at 32 threads, then the
population this arc has been optimising against would be partly an artifact of the
measurement pin — and the *prioritisation* derived from it (`label_cache_build` dominant at
55%, `tier_walk` second) would describe a distribution users never see.

This check was pre-registered as gating any further architectural work:

> "No architectural work should be scoped against a population that has not survived that
> check." — `docs/benchmarks/2026-08-13-dnf164-peer-triage.md`

## Method

The 164 stems from `baselines/2026-08-12-dnf164.txt`, re-run through the harness `run`
subcommand **directly** (bypassing `missed-net.sh`, which would re-pin `--threads 1`):

```sh
HARNESS_OUT_DIR=$S/raw/2026-08-13-tail32 ./target/release/owl-reasoner-harness run \
  --corpus /data/dumontier/ore-run/pool_sample/files \
  --only $H/baselines/2026-08-12-dnf164.txt \
  --reasoner $H/wrappers/run-rustdl.sh --args '{}' \
  --cap-secs 60 --threads 32 --ext owl \
  --out $S/runs/2026-08-13-tail32/tail32.jsonl
```

Binary `bin/rustdl-envcache-fa46c75` (sha-pinned in the manifest — the same binary that
defined the 164). Sequential, one invocation per ontology, quiet host, same 60 s cap that
defined the population. Harness confirmed `164 requested, 164 resolved, 0 missing,
threads Some(32)`. Elapsed ~2.6 h.

## Result

| | count | share |
|---|---|---|
| completed within 60 s | **25** | 15% |
| still DNF | **139** | 85% |

Recovered walls: median **34.1 s**, p90 50.7 s, max 58.5 s — all bunched just under the
cap, which is the signature of ontologies that were marginal at 1 thread rather than
ontologies parallelism made tractable.

### Recovery is confined to the parallel phases

Joined against the 120 s phase census (`data-2026-08-12-dnf164-phase-census-120s.csv`):

| 1-thread bucket | recovered | of | rate |
|---|---|---|---|
| `sweeps` | 3 | 4 | 75% |
| `tier_walk` | 10 | 35 | 29% |
| `label_cache_build` | 12 | 91 | 13% |
| `no-banner` | 0 | 19 | 0% |
| `saturate` | 0 | 8 | 0% |
| `unsat_probe` | 0 | 4 | 0% |
| `prepare` | 0 | 3 | 0% |

This is mechanistically coherent rather than noise: `tier_walk`, `label_cache_build` and
the sweeps are the rayon-parallel phases, and they are exactly where recovery happens.
`saturate` and `prepare` are serial, and recover nothing. Parallelism helps precisely where
parallelism exists.

`label_cache_build` recovering only 13% despite being the parallel phase with the largest
membership is also consistent with the earlier profiling, which measured it **CPU-bound at
92% parallel efficiency**. Efficiency is not the binding constraint — magnitude is:
`ore_ont_6134` needs ~6,412 s of CPU in that phase, so 32 threads still leaves ~200 s
against a 60 s cap. Parallelism cannot rescue work of that size, only work near the
boundary.

### Memory is the cost

RSS across all 164 at 32 threads: median **1.10 GB**, p90 6.4 GB, **max 24.1 GB**. The
per-pair tableau graphs are per-worker (`#cores × ~30 MB/graph`, per
`memory/tableau-memory-fanout.md`), so parallelism trades wall for RSS. Recovering 25
ontologies at up to 24 GB peak is a different bargain than the wall numbers alone suggest,
and it is the reason `RAYON_NUM_THREADS` remains the documented mitigation for the
memory tail.

## What this establishes, and what it does not

**Establishes:** the tail is not a measurement artifact. At 85% survival, the phase
attribution, the bucket ranking, and the peer-triage conclusion (91% peer-solvable, so the
residual is algorithmic rather than intrinsic) all describe a population users actually
hit. Nothing in the arc needs its frame restated.

**Does not establish that the default-parallelism tail is exactly 139.** This measured one
direction only — recovery among ontologies that DNF'd at 1 thread. It cannot see the
reverse: an ontology that completes at 1 thread but fails at 32, which is physically
possible via the RSS multiplication above (a 24 GB peak is close enough to trouble on a
smaller host). Establishing the true default-parallelism tail needs a full 1,920-ontology
sweep at 32 threads. So the honest statement is **the tail is at least 139**, and 139 is a
lower bound rather than a count.

**Reporting discipline.** The 25 recoveries are "recovers at 32 threads relative to a
1-thread-defined population" — not an unqualified improvement, and not comparable to the
recovery counts in the v0.4.x sweeps, which were 1-threaded on both arms. Mixing the two
frames is the error that produced a "~35 recoveries" estimate for the two-arm portfolio
when the true figure was 3: **32-threaded spot checks compared against a 1-threaded
population.** That happened twice in one session. The frame belongs in the sentence, every
time.

## Consequence

The gate is passed, so the population stands and target selection can proceed on the
evidence already gathered. Two secondary readings worth carrying forward:

1. **`tier_walk` is more parallelism-sensitive than `label_cache_build`** (29% vs 13%).
   Both are already parallel, so this reflects magnitude, not headroom — but it means
   `tier_walk`'s 35 members sit closer to the cap than the bucket ranking by count implies.
2. **The 19 `no-banner` and 8 `saturate` members are untouched by parallelism entirely.**
   Any work aimed at them must be algorithmic; there is no throughput lever to pull.

Raw data: `data-2026-08-13-tail-at-default-parallelism.csv`.
