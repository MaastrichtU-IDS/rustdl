# Four-way on the DNF tail: KM v0.2.11 solves 68%, Konclude 89%, rustdl 15%

**Date:** 2026-08-14 · **Population:** the 164 ontologies rustdl fails to classify
(defined at 60 s, harness `--threads 1`) · **All figures at a common 60 s cap.**

## Result

| engine | solves | share |
|---|---|---|
| rustdl @1 thread | 0 / 164 | 0% (population definition) |
| rustdl @32 threads | 25 / 164 | 15% |
| **KM v0.2.11** (`--route production_all`) | **111 / 164** | **68%** |
| **Konclude 0.7.0** native | **146 / 164** | **89%** |

Classified walls: Konclude median **4.95 s**, KM median **13.45 s**.

| KM × Konclude | n |
|---|---|
| both solve | 109 |
| Konclude only | 37 |
| **KM only** | **2** (`ore_ont_15803`, `ore_ont_8475`) |
| neither | 16 |
| **peer union** | **148 / 164 = 90%** |

KM's failures: 50 timeouts, 2 out-of-fragment refusals, 1 OOM at the memory cap.

## The headline reading

**Adding KM barely moves the peer frontier: 146 → 148, +2 ontologies.** The
2026-08-13 triage conclusion — the tail is a gap against a peer, not intrinsic hardness —
rests on **Konclude specifically**, and a second independent implementation does not
broaden it much. Set B (solved by no peer) goes 16 → 16 by KM's arrival, and only
shrinks because rustdl itself solves one.

**KM is not "Konclude in Rust."** `engine/src/konclude_ht/` is a Rust reimplementation of
Konclude's hypertableau, so the 21-point gap (68% vs 89%) between them on the same
population, same host, same cap is an *implementation and strategy* difference, not a
language one. This is worth holding onto: it means "a hypertableau can do this in 5 s" is
not automatically true of any hypertableau — Konclude's advantage over its own
reimplementation is about the same size as KM's advantage over rustdl.

**rustdl solves one ontology neither peer does:** `ore_ont_7192` (at 32 threads). So the
three-engine intersection of unsolved is **15**, not 16, and rustdl is not uniformly
dominated. One instance is one instance — but it is evidence against reading the tail as
"rustdl is simply behind on everything."

## Two corrections made during analysis, both of which changed the number

**1. The caps did not match.** The 2026-08-13 triage ran Konclude at `PEER_CAP=120`; KM ran
at 60 s. Four ontologies take Konclude 70–105 s (`ore_ont_1066` 105.3 s, `15687` 83.3 s,
`15803` 83.0 s, `2874` 70.2 s). Comparing KM@60 against Konclude@120 would have understated
KM by 4. **Konclude is 150/164 at cap 120 and 146/164 at cap 60**; this document uses 146
throughout, and any comparison against the triage's headline must note the cap.

**2. Four KM "empty" outputs were correct answers, not stubs.** My first content rule
labelled `EMPTY_STUB` any KM output whose `subsumptions` list was empty. Four such
ontologies (`ore_ont_4141`, `5753`, `8445`, `11287`) carry `"consistent": false` — they are
**inconsistent KBs**, for which an empty subsumption list is the right output. Konclude
agrees: its output for them is the `EquivalentClasses`/`Nothing` collapse signature. That
error cost KM 4 ontologies (107 → 111) and would have overstated the rustdl-vs-KM gap.

The general lesson is the one this arc keeps re-learning in new costume: **judging peer
outcome from content is necessary but not sufficient — the content rule itself needs a
discriminating check.** "Empty" is ambiguous between *failed* and *correctly empty*, exactly
as Konclude's silence is ambiguous between *non-entailment* and *under-reporting*.

## Threats to validity, checked rather than assumed

**The memory cap is asymmetric and it bound once.** KM runs under a mandatory
`ulimit -v 20GB` (uncapped it reached 237 GB on a 100-class ontology and was OOM-killed at
898 s, degrading the whole host). **Konclude runs with no cap.** `ulimit -v` limits *virtual*
address space, so RSS staying under the ceiling does not prove the cap was not binding —
verified directly by capturing stderr: `ore_ont_15687` fails with `memory allocation of
16106127360 bytes failed`. That is 1 ontology, and Konclude DNFs on it too, so the union is
unaffected — but KM's 111 is a floor, not a ceiling, and a fully fair run would give KM the
same unbounded memory Konclude gets.

**KM refuses 2 ontologies outright** (`ore_ont_2738`, `2874`) with
`unsupported: out of fragment: named role expected, got ObjectInverseOf`. That is a coverage
limit, not a search failure — a different kind of "no answer" than a timeout, and reported
separately above rather than folded into the DNF count.

**The route matters, and using KM's default would have been unfair.** KM's bare `default`
route DNFs on `ore_ont_10019` where `production_all` takes **0.25 s**. This run uses
`production_all`, KM's own winning bundle. An earlier version of the wrapper exported the
`KM_*` bundle as environment variables onto the `kobayashi-marust` worker; the route is a
**CLI flag** (`km classify --route`), so that wrapper silently measured KM's default and
reproduced its timeout. A discriminating control on `ore_ont_10019` (0.25 s under
`production_all`, DNF under `default`, same binary) caught it before the 164-item sweep ran.

**Not a completeness comparison.** Outcome only. KM emits Tseitin definers (`Q_*`) in
`subsumptions`, and per the 2026-08-05 retraction, 73% of an earlier "KM FP" figure was a
TOP-equivalence normalisation artifact. "KM classified it" means it produced output, not
that the output is right. Any completeness claim needs the definer filter plus a
Konclude ∪ HermiT adjudication.

## Solve rate by rustdl's own phase bucket

Bucket = the phase dominating rustdl's 1-thread wall at a 120 s cap.

| bucket | n | KM | Konclude |
|---|---|---|---|
| `label_cache_build` | 91 | 70% | 89% |
| `tier_walk` | 35 | 74% | 94% |
| `no-banner` | 19 | 37% | 79% |
| `saturate` | 8 | 38% | **100%** |
| `unsat_probe` | 4 | 50% | **100%** |
| `sweeps` | 4 | 75% | 75% |
| `prepare` | 3 | 67% | 67% |

Konclude solves **100%** of the `saturate` and `unsat_probe` buckets — the two where 32-way
parallelism recovered **nothing** (0/8, 0/4). Those 12 ontologies are therefore the
sharpest available targets: a peer does all of them, parallelism does none of them, so the
cost is algorithmic and demonstrably avoidable. That is a better-specified target than the
91-member `label_cache_build` bucket, where Konclude itself leaves 11 unsolved.

Raw data: `data-2026-08-14-dnf164-four-way.csv`.
