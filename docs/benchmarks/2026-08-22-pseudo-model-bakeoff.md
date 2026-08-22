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

---

## COVERAGE EXTENSION FAILED, AND THAT IS THE RESULT

The recommended next step above — re-run only the OFF arm at a larger cap — was run: 38 ontologies,
**600 s** cap (5× the original), OFF arm only, ON having already completed all 38.

**37 of 38 still time out. The extension converted exactly 1.**

So `RUSTDL_PSEUDO_MODEL=0` is not marginally slow on these ontologies, it is **intractable**, and no
amount of additional budget makes the comparison obtainable. Final tally on the high-risk subset:

| | count |
|---|---:|
| ABox + `InverseFunctional` | 73 |
| correctly refuse (inconsistent KB, both arms) | 19 |
| consistent | 54 |
| ON completes | 38 |
| OFF completes (120 s → 600 s) | 16 → **17** |
| **comparable** | **17** |
| **ontologies losing entailed types** | **0** |

## What this means for the falsified argument

**The obligation cannot be discharged empirically by this route.** A verdict-identity bake-off needs
both arms, and the arm without the prune does not finish on 37 of 54 consistent high-risk
ontologies. Running longer does not fix it — that is now measured, not assumed.

Where comparison IS possible the prune costs nothing (17/17, 0 lost pairs). That is real evidence,
and it is the strongest available on this subset, but it is not the identity result the record asked
for and should not be quoted as one.

**A caveat the flag's own documentation is missing:** it offers `RUSTDL_PSEUDO_MODEL=0` as "the
workaround" for the falsified soundness argument. That workaround is **intractable on 37 of these 54
ontologies at a 600 s cap**. Anyone relying on it to recover the lost inverse-functional entailments
will simply get no answer on realistic inverse-functional inputs. The fix therefore has to be the
one the doc already identifies as correct — apply inverse-functional merges in the `ABox`-seeded
wedge consistency completion, as it already does functional ones — and not the flag.

## Where usable coverage actually lives

The **1,071 ABox-bearing ontologies WITHOUT `InverseFunctional`** are lower-risk by the theory (the
falsified clause does not bite there) but, being cheaper, are far more likely to complete in *both*
arms. That is the frame to run for breadth. It cannot exercise the falsified case — which is exactly
why it is complementary rather than a substitute.
