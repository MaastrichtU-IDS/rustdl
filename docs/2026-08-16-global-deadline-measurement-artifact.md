# The DNF tail is partly a measurement artifact: `--global-timeout-ms` defaults to 0

**Date:** 2026-08-16 · **23 of 27 ontologies previously counted as flat DNFs produce sound
output — 15 of them byte-identical to KM — once classify is given an INTERNAL deadline.**

## The defect

`classify --global-timeout-ms` defaults to **0 (unbounded)**. Every rustdl sweep in this arc
used a wrapper that sets no internal deadline, so an ontology exceeding the harness cap is
killed **externally** and prints nothing.

That is the worst of both worlds, because classify **already degrades gracefully**: it seeds
its entailment matrix from the saturation closure before the label cache and tier walk, so an
INTERNAL deadline yields a sound partial hierarchy. An EXTERNAL kill yields nothing at all.

| `ore_ont_11311` | result |
|---|---|
| unbounded + external kill @240 s | **no output** |
| `--global-timeout-ms 5000` | **10,658 direct rows in 5.8 s** |
| `--saturation-only` | 10,658 rows in 1.13 s |
| KM v0.2.32 | 4.8 s |

The 5 s run, the saturation-only run, and KM all agree: **79,803 closure pairs, identical**.

## Result on the 27 "KM-only" ontologies

Same host, 240 s harness cap, 1 thread; rustdl v0.4.19 with an internal 220 s deadline.

| | no internal deadline | **internal deadline** |
|---|---|---|
| produce usable output | **5 / 27** | **23 / 27** |

Adjudicated against KM v0.2.32 per ontology:

| verdict | n |
|---|---|
| **EXACT match with KM** | **15** |
| sound but partial (FP=0, some MISSED) | 4 |
| flagged FP vs KM → **all FP=0 adjudicated** | 3 |
| still no output | 4 |
| **false positives, adjudicated** | **0** |

Exact matches include some large hierarchies: `ore_ont_15803` 2,432,194 pairs,
`ore_ont_7581` 1,246,911, `ore_ont_16444` 1,189,232, `ore_ont_11460` 446,454 — all identical
to KM.

## The three apparent FPs were KM under-reporting

A raw rustdl-vs-KM diff flagged `ore_ont_7499` (330), `7914` (30), `9663` (34). Against
**Konclude ∪ KM**, with TOP-equivalence and unsatisfiable classes normalised on both sides,
every one is **0**.

On `ore_ont_7499`, Konclude reports **3,298 pairs KM does not**. KM is not an oracle on this
ontology, and a two-way diff against it manufactures false positives.

**This is the fourth occurrence of this artifact in this project, and the fourth time the
adjudicated answer is zero.** The rule, restated: adjudicate via `X − (Konclude ∪ HermiT ∪ …)`
with symmetric equivalence expansion and TOP/unsat normalisation. Never a raw two-way diff.

## What this does and does not change

**Does:** the "21 ontologies KM solves and rustdl cannot" is substantially a measurement
artifact. rustdl produces sound, often exact answers on 19 of them; it was never given the
chance to degrade. Every DNF count in this arc — including the 153-ontology tail — carries
the same caveat, because they all used the same unbounded wrapper.

**Does not:**

* **"Produces output" ≠ "solves it".** KM does `ore_ont_16444` in 11.4 s; rustdl needs ~222 s
  and stops at the deadline. The honest categorisation is three-way — *nothing* / *sound
  partial* / *complete* — not a binary win.
* **4 remain genuinely unsolved** even with the deadline (`ore_ont_10621`, `16744`, `4572`,
  `8737`), and `ore_ont_3215` returns only 508,721 of 3,923,171 pairs (13%).
* **It does not resize the tail.** 27 ontologies is not 153. Sizing needs a corpus-scale
  re-measure with an internal deadline, not an extrapolation.

## Two corrections made while finding this

Recorded because both were wrong in the same investigation and each was refuted by the next
command:

1. **"The hybrid path emits 0 rows on timeout."** False — with a global deadline it emits the
   full 10,658 rows. The zeros were externally-killed runs that never printed. I had the
   evidence in hand and misread which runs it came from.
2. **"The fix is to seed the entailment matrix from the saturation closure."** Already
   implemented. The actual defect is that nothing bounds the run by default, so the existing
   degradation never triggers.

## Open question for a default

Should `--global-timeout-ms` default to non-zero? Arguments both ways:

* **For:** a user running `rustdl classify` on one of these gets nothing today, where a
  bounded run returns a sound hierarchy — on 15 of 27, the *complete* one.
* **Against:** a default deadline silently truncates results on ontologies that would have
  finished, converting complete answers into `incomplete: true` ones. That is a completeness
  regression for everyone who currently waits.

This is a default flip and needs the full three-clause gate (`ok → dnf` = 0, ΔMISSED < 5%, no
verdict change), plus a decision about what value could possibly suit both a 0.15 s median and
a 220 s tail. **Not inferable from 27 ontologies.**
