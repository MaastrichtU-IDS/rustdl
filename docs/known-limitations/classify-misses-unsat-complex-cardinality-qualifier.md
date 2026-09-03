# `classify` misses an unsatisfiable class when the cardinality qualifier is COMPLEX

**Status: RESOLVED 2026-09-03 by #98 (merged as `9761ca8`), which closed #91.**
Root cause was localised after all, and it was the THIRD instance of one pattern:
classify's unsat probe trusts a wedge `Sat` unless `needs_verify` fires, and the
`data_counting_classes` / `nominal_counting_classes` clauses sitting in that very
expression had no sibling for object cardinality over a COMPLEX qualifier.
`cardinality_qualifier` Tseitin-names such a filler, so the wedge counts a synthetic
name without relating it to the members and cannot see that the `≤` and `≥` range
over the SAME set. Fixed by `complex_qualifier_counting_classes` +
`RUSTDL_COMPLEX_QUALIFIER_VERIFY` (**default ON**, `=0` reverts).

**"No flag recovers it" was true and was the diagnostic clue, not a dead end** — the
three flags tried (`TRUST_SAT=0`, `CLASSIFY_VERIFY_REFUTATIONS=1`, `SPIKE_CARD_ATOM=1`)
all target the SUBSUMPTION path, and this lives on the SATISFIABILITY probe, which had
no escape hatch for the construct. The section below is kept as the defect record.

The five ontologies enumerated at the bottom were re-measured under a two-arm sweep of
the fix: the predicate provably fires on **4 of them and finds no new unsatisfiable
class on any**, so this doc's own "all 5 show zero unsat disagreement" finding is
independently confirmed from the other direction. ORE is inert for this fix; the
evidence is the canaries plus the oracle adjudication. See
`docs/benchmarks/2026-09-03-complex-qualifier-verify-flip-sweep.md`.

## The defect

```
SubClassOf(:A ObjectMaxCardinality(1 :r ObjectIntersectionOf(:B :C)))
SubClassOf(:A ObjectMinCardinality(2 :r ObjectIntersectionOf(:B :C)))
```

`A` is unsatisfiable — it needs ≥2 and ≤1 `r`-successors in the *same* class.

| | `classify` `unsatisfiable` | per-pair `rustdl sat A` |
|---|---|---|
| complex filler `ObjectIntersectionOf(:B :C)` | **`[]`** — MISSED | `unsat` ✅ |
| atomic filler `:B` (control) | `['A']` ✅ | `unsat` ✅ |

Konclude and HermiT both report `EquivalentClasses(owl:Nothing, A)`. rustdl's own
per-pair `sat` query is right in both rows; only `classify` is wrong, and only
with the complex filler. The atomic control is what makes this a qualifier-shape
defect rather than a cardinality defect.

## This is NOT the `trust_sat` mechanism

Unlike the sibling finding in
`wedge-drops-self-under-forall.md`, **no flag recovers it**:

| config | `unsatisfiable` |
|---|---|
| default | `[]` |
| `RUSTDL_HYPERTABLEAU_TRUST_SAT=0` | `[]` |
| `RUSTDL_CLASSIFY_VERIFY_REFUTATIONS=1` | `[]` |
| `RUSTDL_SPIKE_CARD_ATOM=1` | `[]` |

So it is not the wedge-`Sat`-trusted silent MISS that explains #66, #78 and the
`Self`-under-`∀` case. It lives in whatever `classify` uses to decide class
satisfiability, which on this input disagrees with the `sat` subcommand.

This is the same *family* as issue #66's "secondary observation" — that `classify`
trusts saturation for satisfiability too, and did not flag classes HermiT calls
unsatisfiable. That observation was pizza-specific and is now resolved on pizza
(both `IceCream` and `CheeseyVegetableTopping` are reported at the default). This
is a live instance of the same disagreement on a different shape.

## Where it was found

Probing a *different* recorded suspicion: CLAUDE.md's #78 entry warns that
`fresh_class_id` scans `Atom::Class` and `Atom::Exists` but not
`Atom::AtMost(_, Some(c), _, _)`, so "an ontology whose largest class id occurs
only in an `AtMost` qualifier would seed the H3b encoder too low and alias real
classes".

**That hazard is REFUTED — do not spend time on it.** Two independent reasons:

1. Every call site clamps: `fresh_class_id(&base).index().max(num_classes)`.
   Real class ids are all `< num_classes`, so a fresh id can never alias one.
2. A qualifier that is not a named class goes through `atomic_name_of`, which
   emits `Q(x) → filler(x)` — so the synthetic id DOES appear in a `Class` atom
   and IS seen by the scan.

Adding `AtMost`/`AtLeast` to the scan would be harmless but buys nothing. The
fixture written to demonstrate the aliasing FP produced no FP; it produced this
MISS instead.

## Severity — corpus impact MEASURED: zero observed

Completeness, not soundness, and silent.

**Corpus probe (2026-08-30).** I expected real reach here and was wrong.

* Shape census over all **1,920** ORE ontologies: only **5** carry a qualified
  cardinality whose filler is a constructor rather than a named class
  (`ore_ont_11647`, `15514`, `668`, `9012`, `9540`) — 0.26%.
* Measured, not grepped (this repo's "grep ≠ gate" rule): **all 5 show zero unsat
  disagreement** against a Konclude oracle. `ore_ont_9012` carries the shape with
  `MISSED=0`. `ore_ont_11647` has `MISSED=80` but `unsat_disagreement=0`, so its
  misses are not unsat misses and are not this defect.
* `ore_ont_668` first read as `323 vs 322` — a **false alarm in my own
  instrument**: the one "missing class" was `owl:Thing`, which Konclude lists among
  unsat classes on an inconsistent KB while rustdl signals the same thing through
  `consistent: false` (it does report `false` there). The harness's
  `aligned_closures` excludes `Thing`/`Nothing` for exactly this reason; my ad-hoc
  comparison did not. **Exclude `owl:Thing` before comparing unsat sets.**

**Pre-existing, not a v0.4.24 regression:** identical on the pinned v0.4.23
control and the v0.4.24 candidate.

Wider context from the same probe: across the 391 scored ontologies rustdl misses
**89 unsat classes in 8 ontologies, with 0 in the FP direction** — soundness
intact. **82 of those 89 rest on a Konclude-only oracle** (HermiT returned
NO_OUTPUT on `ore_ont_16321`/`ore_ont_4198`), so they are single-peer and
unconfirmed; `ore_ont_6951`'s 2 are peer-agreed. Those misses are a separate,
deeper gap — on `ore_ont_16321` rustdl's own per-pair `sat` also answers `sat`, so
classify and the per-pair surface AGREE there and it is not this family.
