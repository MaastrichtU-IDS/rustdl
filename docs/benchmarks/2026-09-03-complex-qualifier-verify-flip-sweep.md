# `RUSTDL_COMPLEX_QUALIFIER_VERIFY` default-ON flip: two-arm sweep (2026-09-03)

Gates the default-ON flip shipped in #98 (`9761ca8`, closes #91). The PR's own
evidence was 5 canaries, an oracle adjudication and 6 curated fixtures — enough for
the FIX, not for the DEFAULT, whose failure mode is wall time and whose precedent in
this repo is explicit: *"a flag flipped on a 12-ontology benchmark took 4 others from
~5 s to DNF."*

**Verdict: flip is safe. 0 answer changes, 0 outcome changes, wall flat.**

## What the flag does

classify's unsat probe trusts a wedge `Sat` unless `needs_verify` fires. The flag adds
a third clause beside `data_counting_classes` and `nominal_counting_classes` (#49):
withdraw trust for a class carrying a `Min`/`Max` over a COMPLEX qualifier. Direction
of risk is one-way — it only ever swaps a wedge `Sat` for the complete main-tableau
path, so being wrong costs wall time, never an answer.

## Frame: 497, and it is a superset BY CONSTRUCTION

Not a grep guess. `concept_has_complex_qualifier_counting` can only return `true` if a
`Min` or `Max` node exists, and:

* only the three cardinality axioms construct one (`convert.rs:1037/1042/1047`);
* NNF **preserves** filler complexity rather than creating it — `normalize.rs:68/72/123/129`
  carry `c_nnf` through, so `¬(≥n r.C)` becomes `≤(n−1) r.C` with the filler untouched,
  and an atomic filler cannot become complex;
* `derive_functional_max_cardinality` emits `≤1 r.⊤`, whose filler is `Top`.

So an ontology with no `(Object|Data)(Min|Max|Exact)Cardinality` is unreachable for the
predicate. That is **497 of 1,920**, all swept.

## The PR's "5 of 1,920" is GREP-derived; the gate figure is 4

Reproduced exactly with a same-line grep for a cardinality whose filler opens another
constructor — which is what identifies it as a grep rather than a gate measurement,
the distinction Lever 1 already paid for. A **whitespace-insensitive** scan finds **7**,
adding two `DataExactCardinality → DataOneOf` ontologies an object-only pattern cannot
see.

Measured by gate — the `# satisfiability probes: tableau=` delta, ON vs OFF, which is
a direct fire-detector because withdrawn trust *is* an extra tableau probe:

| ontology | tableau probes OFF → ON | fires |
|---|---|---|
| `ore_ont_11647` | 0 → 20 | **yes** |
| `ore_ont_15514` | 0 → 4 | **yes** |
| `ore_ont_9012` | 0 → 1 | **yes** |
| `ore_ont_9540` | 13 → 15 | **yes** |
| `ore_ont_668` | 0 → 0 | no |
| `ore_ont_12824` | 11 → 11 | no (DKey filler is atomic) |
| `ore_ont_10109` | — | DNF both arms, uninformative |

So the PR's count is right for the wrong reason: two members the grep found do not
fire, and two it missed do not either. **Prove the instrument fires before reading a
null result** — without this delta, "0 answer changes" would be indistinguishable from
"the flag never engaged".

## Sweep

One pinned binary `rustdl-cq91-544df29` (sha `0be4cfb516f6`), **the env var the only
difference**, recorded in both manifests (`rustdl_env: ''` vs
`RUSTDL_COMPLEX_QUALIFIER_VERIFY=0`), 60 s cap, `--threads 1`, harness `run`.

| | result |
|---|---|
| outcomes | **452 ok/ok, 44 dnf/dnf, 1 err/err — 0 `ok → dnf`** |
| answers | **447 IDENTICAL, 50 both-empty, 0 DIFFER** |
| wall | ON 3398.0 s vs OFF 3405.0 s = **−0.21%** |
| >1 s slower AND >1.5× | **0** |

**Answers compared as the TRIPLE (direct rows, `unsat`, equivalence groups)** over
non-`#` lines, so every timing banner and `incomplete` is excluded by construction.
That is load-bearing here, not hygiene: this fix's success mode is *finding* an
unsatisfiable class, which **elides** that class's Hasse rows — so a row-count
comparison would read a correct fix as a regression. Fifth instance of the
direct-vs-closure trap.

## The one real cost

`ore_ont_11647`: **1.43 s → 1.76 s (+23%)**, identical answers, reproducible across 3
sequential repeats with arm order alternated (ON 1.76/1.75/1.76, OFF 1.44/1.43/1.42).
Attributable — it is the ontology taking 20 extra tableau probes. The other three
firing members are flat.

## `ore_ont_9540`: flat and safe, but do not read a cross-arm hash on it

The documented label-cache guard case, so it got the most attention.

* Arm ON is **self-inconsistent: 7 of 8 runs one row-set, 1 of 8 another** (the
  cold-cache first run). Arm OFF: **8/8 stable**.
* The deviation is **one Hasse row** — `UJI_Wall ⊑ Object_type` where the other runs
  derive the nearer `UJI_Wall ⊑ Possible_UJI_Wall`. A run that failed to derive the
  nearer parent: a sound truncation miss, the #76 / `ore_ont_1508` signature.
* 1-in-8 vs 0-in-8 is not separable from noise, and "extra probes steal budget from
  the tier walk" is a plausible mechanism — so the control was run rather than the
  argument made: **at `--pair-timeout-ms 1000` both arms are identical, 41 rows, same
  hash, 217.34 s vs 217.22 s.** Budget truncation, not the fix.
* Wall at the default is flat: 14.18–14.25 s across all 6 interleaved runs.

Two runs cannot establish stability here — 3 would have shown ON as "stable" and OFF as
"stable" and invited a cross-arm read of a difference that is within-arm.

## The headline is that ORE is inert for this fix

On all 4 firing ontologies the main tableau **confirms** the wedge's `Sat`. **No new
unsatisfiable class anywhere in 497.** The independent confirmation is that #91's own
limitation doc had already found "all 5 show zero unsat disagreement" against a
Konclude oracle, from the other direction.

So this sweep is **non-regression evidence only**; the fix's actual evidence is its 5
canaries plus the Konclude/HermiT adjudication. Same shape recorded for #70, #72, #78
and #81 — the constructs these fixes address are essentially absent from ORE, which is
exactly why the corpus could not have validated any of them.

## Caveats

* Sweep wall was measured at `JOBS=4`, so aggregate wall is contended and the **−0.21%
  sign is not meaningful**; the per-member figures above are sequential re-measurements
  on a settling host (load average ~3.9, decaying from the sweep). The claims that
  matter — 0 answer changes, 0 `ok → dnf` — do not depend on wall.
* The frame excludes the 1,423 ontologies with no cardinality axiom. That exclusion is
  by construction, argued above, not by sampling.
