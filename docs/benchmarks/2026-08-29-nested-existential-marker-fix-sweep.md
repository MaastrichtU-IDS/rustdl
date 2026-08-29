# Two-arm ORE sweep for the nested-existential marker fix (#80, #82)

**Arms.** `BEFORE` = `4d6612a` (sha `818f10de6607`), `AFTER` = `09e2297` (sha `db533d3746ea`).
Both pinned immediately after their build and verified against **two discriminating inputs**
before use — `nested-mono.ofn` (BEFORE `[(A,F)]`, AFTER `[(A,F),(C,D)]`) and `chainpoison.ofn`
(BEFORE `unsat=[]`, AFTER `unsat=[C]`). An arm that cannot be told apart from the other cannot
validate anything.

**Population.** All **1,920** ORE ontologies. Scoping was considered and rejected: the only sound
superset is "contains `ObjectSomeValuesFrom`", which is 1,480 of 1,920, so selection costs more
than it saves. A same-line grep for nesting finds 183, but line-based matching would miss
multi-line formatting and is not a superset.

**Method.** `owl-reasoner-harness` `sweep` leg per arm, 60 s cap, `--threads 1`, 6 chunks, one
invocation per ontology, resumable JSONL, arm sha recorded in each chunk manifest. Legs run
**sequentially, not concurrently**: the cap is wall-based and cap-hits are the metric, so
concurrent legs would manufacture spurious DNFs through CPU contention.

## Result: 1 outcome change in 1,920

| | BEFORE | AFTER |
|---|---|---|
| ok | 1780 | 1779 |
| dnf | 137 | 138 |
| err_reject | 3 | 3 |

**REGRESSED: 1** (`ore_ont_9429`). **RECOVERED: 0.** Aggregate wall flat, peak RSS max unchanged
at 19.1 GB.

### `ore_ont_9429` — real, reproducible, NOT root-caused

Three interleaved runs per arm at a 300 s cap, single-thread:

| arm | walls | σ |
|---|---|---|
| BEFORE | 55.6, 55.5, 55.4 s | ~0.1 s |
| AFTER | 70.8, 70.7, 70.7 s | ~0.05 s |

**+27%, and it is not a boundary flip.** Both arms **complete** (`rc=0`) at 300 s — the DNF is
purely an artifact of the 60 s cap, which 55.5 s sat just under and 70.7 s sits over.

**The obvious explanation was checked and FAILED.** The fix adds self-facts to nested existential
markers, so "this ontology is nesting-heavy" is the natural story. It is not: `ore_ont_9429` has
174 nested-`∃` occurrences over 163 lines, while the median *among sampled files that have any
nesting* is **582**. Files with far more nesting did not regress. The cause is therefore
**unknown**, and is recorded as unknown rather than given a plausible-sounding attribution.

## What this sweep does and does not establish

**Does:** the fix causes no completion regression anywhere in the corpus except the single
slowdown above, and no ontology stopped terminating.

**Does NOT:** answer identity. `out_sha256` is null on every row of this run and no `.out` files
were captured, so this leg compares COMPLETION only. Curated-fixture answer identity comes from
the FP=0 net (22/22, every closure byte-identical: galen 27997, notgalen 32739, sio 8904,
wine 653, ore-10908 6001, ore-15672 142, alehif 247, ro 158, pizza 499, sulo 51, bibtex 16, all
`FP=0 MISSED=0`). Whether the entailments the fix ADDS on ORE ontologies are correct needs the
Konclude ∪ HermiT MISSED net — a two-arm diff cannot answer it, because the fix is *supposed* to
change answers on nested-existential shapes.

**Wall is order-confounded.** Arm order was fixed (BEFORE then AFTER), which this repo's record
shows buys a ~3.4% page-cache phantom. The aggregate figure is context only and must not be
quoted as a speed delta. It did not need balancing because the metric is completion, not wall.

## Verdict

Ship. The change closes #80 entirely, #82 entirely (both halves), and half of #81, against a cost
of one ontology in 1,920 running 27% slower while still completing. Recorded rather than buried,
with the un-root-caused slowdown left open.
