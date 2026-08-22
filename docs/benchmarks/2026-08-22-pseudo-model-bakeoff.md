# `RUSTDL_PSEUDO_MODEL` bake-off — 0 lost entailments where comparable, and the prune is worth 2.4× completions

**Why this exists.** `RUSTDL_PSEUDO_MODEL` ships **default ON**, and its stated basis for shipping
without an ORE verdict-identity bake-off — "sound by construction: an entailed type is in every
model, hence in the witness, hence never pruned" — is **falsified** (the witness applies functional
but not inverse-functional merges, so on an inverse-functional `ABox` it is not a model). The
record therefore reclassified that bake-off as load-bearing rather than optional. This is it,
partially discharged.

## Frame

The flag no-ops without an `ABox` (`realize_base_model_types` returns `None`), so the population is
the **1,144 ABox-bearing** ORE ontologies. Of those, **73 carry
`InverseFunctionalObjectProperty`** — the subset where the falsified argument predicts losses. That
is the frame here: **test where the theory predicts failure**, not a representative sample.

Two arms, `realize --json`, default flags otherwise, 120 s cap, single-thread, P=6.

## Result

| | count |
|---|---:|
| correctly refuse (inconsistent KB) — **both arms** | 19 |
| ON produces output | **38** |
| OFF produces output | **16** |
| **comparable (both arms)** | **16** |

On all **16** comparable ontologies:

* **lost (individual, type) pairs: 0**
* gained pairs: **0** (must be 0 — the prune is subtractive; a non-zero value would mean the
  instrument is wrong)
* aggregate pairs **ON 1,842 = OFF 1,842**

**So the prune costs nothing where it can be checked.** The 19 refusals are correct behaviour, not
failures — "ontology is inconsistent; every assertion is trivially entailed", the documented v0.3.36
short-circuit.

## The coverage limit is STRUCTURAL, and is itself a finding

Coverage is 16 of 73 (22%) because the **OFF arm times out on 38** while ON times out on 16.
Comparison needs both arms, so disabling the prune is what caps coverage.

Read the other way, that asymmetry quantifies the benefit: among the 54 consistent members, the
prune takes completions from **16 → 38 (2.4×)** at a 120 s cap. The previously recorded figure was
**1.59× on a synthetic fixture**; this is the first measurement on real ORE data, and it is larger.

## Status: PARTIAL — what would finish it

**Do not read this as discharging the obligation.** 22% coverage on the high-risk subset, and zero
coverage of the 1,071 ABox-bearing ontologies without `InverseFunctional` (where the falsified
clause does not bite, so they are lower-risk but not no-risk).

Next step: re-run **only the OFF arm** on the 38 timeouts at a much larger cap (600 s+). That is the
single change that raises coverage, because ON already completes 38 of them.

## Method notes

* **`comparable = 0` was my own bug, not a result.** The first pass used a generic JSON walker that
  recursed infinitely (`walk(x, v)` on the same dict re-finds the `iri` key), killing 38 rows
  outright and reporting zero comparisons. Had I read that as "no losses found" it would have been a
  false all-clear. The schema is flat and known —
  `individuals[].{iri,types,direct_types}` — so a schema-aware parser is both correct and simpler.
  Fixed without re-running the reasoner, since both arms' JSON was already on disk.
* **The gained-pairs column is the instrument's own check.** The prune is subtractive by
  construction, so any gain means the comparison is broken rather than the engine surprising.
