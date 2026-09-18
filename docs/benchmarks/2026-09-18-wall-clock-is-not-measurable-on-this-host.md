# Wall-clock is not a usable metric on a shared host — use CPU time

**2026-09-18.** Recorded because several conclusions in the #128 / #159 / #160 work were
wrong for this reason, and each looked well-supported at the time.

## The measurement

The same release binary, classifying `ontologies/real/wine.ofn`, minutes apart:

    wall:  19.82 s   and   8.41 s      -> 2.4x spread on IDENTICAL code
    CPU (user+sys), 5 interleaved reps: 88.3 - 92.9 s  -> 5% spread

The host is 96 cores with other tenants. Load average at the time of the slow reading:
**131 / 135** (1 and 5 minute), with `qemu-system-x86_64` at 385% CPU, `oxigraph` at 90%,
`k3s` at 64%, and Microsoft Defender (`wdavdaemon` + a jar-scanning python) at 86% and 49%.

So a 2x wall difference between two runs carries **no information about the code**.

## What this invalidated

| conclusion | what happened |
|---|---|
| same-tier broadening costs "~3x, negligible" | 10-ontology serial sample; the 248-ontology sweep said **median 15x** |
| 9 ORE ontologies regressed under a fix | all 9 were **SAME** on serial recheck — pure contention |
| `ore_ont_7499` loses 12 entailments | serially it loses **1**; the 12 was budget starvation under load |
| `ore_ont_12698` loses 5 entailments | serially **0** — load noise |
| a sub-property-density predicate separates cost outliers | refuted on 324 points; the 3 it "caught" were the 3 it was fitted to |

The pattern is always the same shape: a plausible mechanism gets attached to a difference
that was actually scheduling.

## Rules

1. **Measure CPU time, not wall**, for anything comparing two builds.
   `/usr/bin/time -f "%U %S"` and sum. Wall is only meaningful for "does this finish inside
   a deadline", never for "is this faster".
2. **Interleave the arms** within one loop (A,B,A,B…), never two separate sweeps. A
   wall-clock per-pair budget means a contended run does less work and reports MORE
   `MISSED`, so separately scheduled sweeps are not comparable at all.
3. **Recheck every DIFF serially before believing it.** The ORE closure-diff harness at its
   default `RUSTDL_TEST_PAIR_MS=200` under `-P 8` produced a **~1.5% spurious-DIFF rate**,
   and it is not confined to the arm under test — `ore_ont_205`'s BASE arm gave
   `38636/38638 MISSED=2` loaded and `38638/38638 MISSED=0` serially.
4. **≥5 reps, report the median and the range.** Three reps gave "galen ~5% slower" where
   five gave "1.7% faster".
5. **Answer data (FP / MISSED / row counts) is robust; wall is not** — provided the per-pair
   budget is not binding. When it IS binding, load converts into missed entailments, which
   is how contention masquerades as a correctness regression (rule 3).

## Corollary for default flips

A flip's *cost* case cannot be built on wall numbers from this host without CPU-time
confirmation. Its *gain* case (entailments recovered, FP) is safe. #128's flip was decided
on an ORE sweep whose answer data holds; its "median 1.00x" wall figure should be read as
"no CPU-time regression detected on spot checks", which is the weaker claim that the data
actually supports.
