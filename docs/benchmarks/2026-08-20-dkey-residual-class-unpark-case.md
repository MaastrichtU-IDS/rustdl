# The parked DKey residual class is BIGGER than recorded, and it owns 8 of the DNF tail

**Date:** 2026-08-20 · **Raw data:** `data-2026-08-20-dkey-conversion-bound-9.tsv`
· **Reached by** reading ONE failing ontology (`ore_ont_10929`) rather than running a population
study — the method note this repo's record recommends and which two of my own population studies
earlier today vindicated by failing.

**Headline: 8 DNF-tail members are CONVERSION-bound on the data-property path, with essentially no
reasoning to do. Aggregate conversion wall 1,182 s → 14 s (82×) with that path disabled. The
`RUSTDL_DKEY_MERGING_GATE` spec parked an on-demand disjointness oracle on an estimated
addressable set of "~4 ontologies"; the tail alone holds 8.**

## How it was found

`ore_ont_10929` is a DNF with **12 classes** and a 49 MB file. `tbox-stats` (parse + convert, no
classification) takes **97.6 s of which `convert_ms` = 96,461** — so **99% of its wall is
conversion and none is reasoning.** Its content is ~244k `ABox` assertions: 110,802
`DataPropertyAssertion` (60,323 **distinct** literals, 20 data properties), 110,378
`ObjectPropertyAssertion`, 22,954 `ClassAssertion`.

Flag isolation on that one ontology:

| arm | wall | `convert_ms` | `concept_rules` |
|---|---:|---:|---:|
| baseline | 95.7 s | 94,593 | 57,355 |
| **`RUSTDL_DATA_PROPERTIES=0`** | **2.6 s** | **1,480** | 12 |
| `RUSTDL_DKEY_MERGING_GATE=0` | 100.1 s | 98,942 | 57,355 |
| `RUSTDL_BOUNDED_DKEY_DISJOINT=0` | **203 s (timeout)** | — | — |

The merging gate changes nothing, and *removing* bounded seeding is far worse — consistent with
seeding being a real cost centre that is expensive even when bounded.

## Why the gate cannot help these, by design

`RUSTDL_DKEY_MERGING_GATE` skips DKey disjointness seeding only where the role component is **not**
merge-inducing. Compare the ontology the gate famously fixed against this one:

| | assertions | distinct literals | `FunctionalDataProperty` | `DataPropertyRange` | gate |
|---|---:|---:|---:|---:|---|
| `ore_ont_9347` | 19,160 | 13,562 | **0** | **0** | skips → 49.5 M rules → **113** |
| `ore_ont_10929` | 110,802 | **60,323** | 2 | 25 | **correctly declines** |

So this is precisely the residual class the spec predicted and parked — *"`ore_ont_5368` is the
negative control … its component is genuinely merge-inducing, so its pairs ARE consumable and the
gate correctly declines."*

## The class, measured (not grepped)

Selection was by grep (≥1000 `DataPropertyAssertion` **and** a merge-inducing data role) — and
**grep ≠ gate**, so every candidate was then *measured* with `tbox-stats`, both arms:

| ontology | `convert_ms` ON → OFF | wall ON → OFF | speedup |
|---|---|---|---:|
| `ore_ont_8445` | timeout → 478 | 303.2 s → 1.1 s | **275×** |
| `ore_ont_4141` | 111,071 → 228 | 111.7 s → 0.5 s | **223×** |
| `ore_ont_5368` | 38,474 → 75 | 38.8 s → 0.2 s | **194×** |
| `ore_ont_4572` | timeout → 1,388 | 300.9 s → 2.4 s | **125×** |
| `ore_ont_2504` | 187,172 → 1,342 | 206.5 s → 2.0 s | **103×** |
| `ore_ont_1833` | 29,360 → 310 | 29.7 s → 0.5 s | 59× |
| `ore_ont_10929` | 96,461 → 1,508 | 97.6 s → 2.7 s | 36× |
| `ore_ont_15635` | 90,325 → 2,531 | 92.6 s → 4.8 s | 19× |
| `ore_ont_5548` | 565 → 74 | 0.7 s → 0.2 s | 3.5× |
| **aggregate** | | **1,182 s → 14 s** | **82×** |

**`ore_ont_5548` is a false member** — it carries the signature but converts in 0.7 s, so it is not
conversion-bound. The genuinely affected set is **8**, and two of them (`4572`, `8445`) do not
finish conversion at 300 s.

`ore_ont_5368` matters specially: the spec named it *"the strongest candidate"* for unparking, a
27 GB DNF. It converts in **0.2 s** with the data path off.

## What this does and does not establish

* **Established:** 8 tail members spend 30–300+ s in conversion on the data-property path, and
  their class counts (12, 36, 337, …) mean there is almost no reasoning to do. These are **waste,
  not hardness** — and Konclude classifies ~91% of this tail.
* **NOT established — the 82× is a CEILING, not the lever's value.** `RUSTDL_DATA_PROPERTIES=0`
  deletes the semantics; any real fix must preserve it. This is the same distinction the
  `abox_check` work drew about `RUSTDL_ABOX_CHECK=0`: *quote the fraction, never the bound.*
* **NOT isolated:** the cost is measured on the *data-property path as a whole*. DKey disjointness
  seeding is the leading suspect (the merging-gate design predicts this exact class, and disabling
  bounded seeding is worse), but I did not separate seeding from DKey interning or per-assertion
  lowering. **Do that before designing.**

## The unpark case, and the cheaper alternative to price first

The spec parked Lever 2 on **work-to-reward**: "~4 ontologies against new side-table hooks in four
consumers, three of which have none", with the revisit condition *"if one of the four is
independently needed (`5368` … is the strongest candidate)"*. Both halves have moved: the set is
**8 in the tail** (~6% of it), and `5368` is in it.

**But price the cheap option first.** Being merge-inducing does not mean all C(k,2) pairs are
*consumable* — with 60,323 distinct literals across 20 data properties, the overwhelming majority
still cannot co-label a node. A **tighter consumability test** might capture most of the win with no
new side-table architecture. That is a hypothesis, not a finding; it is cheap to test and should
gate any decision to build the oracle.

Encouraging note carried over from the spec: *"no consumer iterates the full pair set, so an oracle
stays architecturally feasible."*
