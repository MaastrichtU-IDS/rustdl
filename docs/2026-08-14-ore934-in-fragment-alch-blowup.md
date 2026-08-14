# `ore_ont_934`: an in-fragment ALCH ontology the hybrid cannot classify

**Date:** 2026-08-14 · **This meets the documented reopen criterion for the CB arc.**

## The finding

A **604-line, 108-class, pure-ALCH TBox** — reduced from ORE's `ore_ont_934` — **does not
classify in rustdl at 180 s**, while **Konclude classifies it in 0.09 s** with a genuine
216-`SubClassOf` taxonomy. Fixture committed as `docs/ore934-pure-alch-core.ofn`.

The reduced core contains **only**: `SubClassOf`, `DisjointClasses` (146),
`ObjectAllValuesFrom` (48), `ObjectSomeValuesFrom` (38), `ObjectUnionOf` (14),
`ObjectIntersectionOf` (3), `ObjectComplementOf` (2), `SubObjectPropertyOf` (18).
**No** transitivity, inverse, functional, cardinality, nominals, ABox, or datatypes.

The full ontology is worse in wall but identical in outcome, and — notably — Konclude
returns the **same taxonomy (216 `SubClassOf`, 326 classes) for all three variants**, so the
ABox and the functional/inverse/cardinality axioms are irrelevant to this hierarchy:

| variant | lines | Konclude | rustdl |
|---|---|---|---|
| full `ore_ont_934.owl` | 1,106 | **0.38 s** | DNF @600 s |
| TBox only | 730 | **0.12 s** | DNF @120 s |
| ALCH + transitivity | 607 | **0.09 s** | DNF @120 s |
| **pure ALCH** | **604** | **0.09 s** | **DNF @180 s** |

## Why this matters: the CB arc's reopen criterion is met

The consequence-based-engine pursuit was closed on evidence on 2026-07-28:

> **its GO criterion has ZERO candidates: all 8 target DNF onts complete on v0.4.6, market
> 8/289 → 0/289.** Reopen only by exhibiting an in-fragment ont the hybrid cannot solve.

This is that ontology, and it is **derived from a real ORE ontology, not synthetic** — which
matters, because this branch (`feat/cb-alch-taming`) already carries a *synthetic*
"adversarial ∀-disjunctive ALCH blowup" baseline. A real instance is a materially stronger
warrant than an adversarial construction.

Stated conservatively: this establishes **one** in-fragment counterexample, plus one sibling
(`ore_ont_8273`) sharing its shape. It does **not** by itself establish an addressable
population — see the sizing question below, and note that this arc has twice produced a
population estimate that collapsed under a proper check.

## Where the wall goes

Measured with `--global-timeout-ms` to force a banner (the run never terminates otherwise):

```
# classes: 108
# satisfiability probes: saturation=0 tableau=108
# subsumption: saturation=0 tableau=0
# wall breakdown ms: label_cache_build=12831 unsat_probe=47163 tier_walk=0
```

Three facts, each independently checked:

1. **The label cache decides ZERO of the 108 classes** after 12.8–16.6 s of work
   (`saturation=0 tableau=108`). With `RUSTDL_LABEL_HEURISTIC=0` the wall is unchanged and
   `label_cache_build` drops to 0 — the time simply moves to `unsat_probe`. On this ontology
   the label cache is pure overhead.
2. **`unsat_probe` expands to fill whatever budget remains** — 13.4 s at a 30 s global cap,
   43.4 s at 60 s, 73.4 s at 90 s. Each of the 108 per-class probes gets the *global*
   deadline via `effective_deadline`, so at the default (no global cap, 1000 ms per pair)
   every class burns its full 1 s ⇒ ~108 s, which is exactly the census's 103,541 ms.
3. **`tier_walk` never starts** (`subsumption: saturation=0 tableau=0`). Not one pair is
   ever compared. The classification fails before the classifier begins.

**A single class's satisfiability probe does not terminate in 240 s.** That is the core
result: this is not an aggregate cost, it is one undecidable-in-practice probe repeated 108
times.

`--pair-timeout-ms` does not rescue it. At 50 ms the stall merely relocates to `tier_walk`
(108 classes ⇒ up to 5,832 pairs × 50 ms ≈ 291 s).

## Profile: not flat

Sampled during `unsat_probe` (perf, dwarf call-graph, 199 Hz, 25 s):

| frame | share |
|---|---|
| `owl_dl_tableau::rules::apply_role_rules::{{closure}}` | **24.7%** |
| `owl_dl_tableau::rules::apply_max` | 11.1% |
| `owl_dl_tableau::rules::apply_role_rules` | 8.7% |
| `owl_dl_tableau::TableauContext::concrete_domain_clash` | 7.6% |

~33% in role-rule application, consistent with 48 `∀` propagating into 38 `∃`-generated
successors under 14 `⊔` branch points. This contrasts with the 2026-08-13 `tier_walk`
profile, which was **flat** (largest area 14%, five ~10% slices) and is what closed the
micro-lever direction. A 33% concentration is a different situation.

Two cautions on reading it. `apply_max` at 11% is surprising in a core with **no**
cardinality axioms — so its work must come from lowered `≤1`/functional-ish structure, and
that should be confirmed before it is targeted. And `concrete_domain_clash` at 7.6% appears
in a core with **no datatypes**; a fast path for that frame was built and measured at
**zero** on 2026-08-13, but that null was on `tier_walk`, so it does not transfer here — and
that experiment also skipped its own fires-check, so it is a weak null in either direction.

## The cluster is two patterns, not one

The four `unsat_probe`-bucket ontologies split cleanly:

| stem | classes | ∀ | ∃ | ⊔ | disjoint | inverse | Konclude |
|---|---|---|---|---|---|---|---|
| `ore_ont_934` | 108 | **50** | 38 | 14 | 146 | 18 | 0.46 s |
| `ore_ont_8273` | 316 | **88** | 180 | 47 | 63 | 166 | 0.29 s |
| `ore_ont_7828` | 831 | **0** | 22 | 11 | 1 | 0 | 0.10 s |
| `ore_ont_10517` | 904 | **0** | 35 | 13 | 1 | 0 | 0.41 s |

`934`/`8273` are ∀-disjunctive. `7828`/`10517` have **no `∀` at all and essentially no
disjointness** (1 `DisjointClasses` each), yet still burn ~118 s in `unsat_probe`. With
almost nothing to clash against, a *satisfiability* probe should be easy, so those two are a
**separate and currently unexplained** failure — do not fold them into the ALCH story.

## What was ruled out, by measurement

* **The ABox** — TBox-only still DNFs. 163 `ObjectPropertyAssertion` + 150 `ClassAssertion`
  removed changes nothing.
* **Any single construct** — six one-construct ablations of the TBox (drop
  `DisjointClasses` / `FunctionalObjectProperty` / `InverseObjectProperties` /
  `ObjectOneOf`+`MinCardinality` / domain+range) **all still DNF**. The blowup is the
  ∀/∃/⊔ *combination*, not one axiom type.
* **Budget tuning** — neither `--pair-timeout-ms 50` nor `1000` completes; the cost relocates
  rather than reducing.

## Method note: a Konclude timing was nearly published wrong

The first Konclude run on the reduced core reported **0.13 s** and its captured output was
**1,193 bytes with 0 `<Class>` tags and 0 `SubClassOf`** — the signature of the 896-byte
stub Konclude writes on junk input. That looked like "Konclude refuses my ablated file", which
would have destroyed the finding.

It was neither. `run-konclude.sh` writes the taxonomy to `HARNESS_OUT_DIR` or a positional
argument and prints its **log** to stdout; I had captured the log. The log itself said
`Finished class classification in 55 ms … expressiveness 'SHI'`. Re-run with `-o` pointed at
a real file: **34,200 bytes, 326 classes, 216 `SubClassOf`**.

Both halves of the near-miss came from the same recorded rule — *judge peer outcome from
content, not exit code* — and the rule fired correctly, then required knowing **which stream
carries the content**. The check is only as good as pointing it at the right output.

## Open question before any architectural work: how large is the addressable set?

**This is one ontology plus one sibling.** Two population estimates in this arc collapsed
under scrutiny (a "~35 recoveries" portfolio estimate whose true value was 3; an
absorption-shape census with AUC 0.480, below chance), and both failures came from
generalising a shape count into a predicted rescue.

So before scoping a CB engine or any other architectural response, the question to answer is:
**how many of the 156 remaining DNF ontologies have a peer-solvable, ∀-disjunctive,
in-fragment core?** Two properties make that measurable rather than speculative:

* the predicate is checkable statically (`∀ > 0 ∧ ⊔ > 0` over an otherwise-ALCH signature), and
* the peer answer is already in hand for all 164 (`data-2026-08-14-dnf164-four-way.csv`).

A shape census sizes a population; it does not predict a rescue — so pair it with the
`--pair-timeout-ms 1` addressability pre-check, which for this ontology is *already* known to
fail (the cost relocates to `tier_walk` rather than vanishing), meaning per-pair budget
reduction cannot rescue it and something structural must.
