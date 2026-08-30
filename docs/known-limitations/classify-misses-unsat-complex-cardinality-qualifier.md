# `classify` misses an unsatisfiable class when the cardinality qualifier is COMPLEX

**Status: live on v0.4.24 (`0642465`). Not fixed. Reproduced, stable across 3
runs, adjudicated against BOTH oracles, and isolated by an atomic-filler
control. Root cause not localised.**

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

## Severity

Completeness, not soundness, and silent. Unmeasured corpus impact: qualified
cardinality with a complex filler is not rare, so unlike the `Self` finding this
one may have real reach — worth a corpus probe before deciding priority.
