# `realize` ignores DERIVED individual equality, silently

**Found:** 2026-08-18 · **Status:** open, not fixed · **Severity:** missed entailments with **no
incompleteness signal**, and two surfaces of one binary contradicting each other.

Found by following up the open question left by
`docs/2026-08-18-fp-critical-audit.md` §1 — whether inverse-functional + `ABox` is complete on
the `is_pure_el` path for `realize`, where individual identity is observable. **It is not.**

## Reproducer

`crates/owl-dl-reasoner/tests/fixtures/realize_derived_same/inverse-functional.ofn`:

```
InverseFunctionalObjectProperty(:r)
ClassAssertion(:A :x)
ClassAssertion(:B :y)
ObjectPropertyAssertion(:r :x :z)
ObjectPropertyAssertion(:r :y :z)
```

`r` is inverse-functional and both `x` and `y` are `r`-predecessors of `z`, so **`x = y`** is
entailed. Therefore `x : A`, `x : B`, `y : A`, `y : B` all hold.

| surface | result |
|---|---|
| `rustdl realize` | **`x : A` and `y : B`** — 2 type assertions missing |
| `rustdl individuals --json` | `same_groups: [["x","y"]]` — **the equality IS derived** |
| `rustdl realize --json` | has **no `incomplete` field at all** — the miss is silent |

## The gap is DERIVED vs ASSERTED, isolated by control

Adding an explicit `SameIndividual(:x :y)` to the same file:

| | realize output |
|---|---|
| asserted `SameIndividual(x,y)` | `x : A, B` and `y : A, B` — **correct** |
| derived (inverse-functional) | `x : A` and `y : B` — **incomplete** |

`individuals` reports `same_groups: [["x","y"]]` in **both** cases. So the equality is known;
only realize's type computation fails to use it.

## Why asserted works and derived does not

`realize` has **no equality folding of its own**. `realize_saturation_eligible`
(`realize.rs:747`) simply refuses `Axiom::SameIndividual(_) => false`, pushing such ontologies
off the saturation fast path to the tableau, which merges the nodes — that is the whole
mechanism behind the working asserted case.

Nothing plays that role for a *derived* equality:

* `saturator_complete_fragment` **admits** `InverseFunctionalRole` (see the FP-critical audit
  §1: sound for CLASS classification, because the canonical model is a tree), so the ontology
  is not kicked off the fast path.
* The EL saturator never reads `Axiom::InverseFunctionalRole`, so it derives no merge.
* **The tableau path misses it too**: `RUSTDL_REALIZE_SATURATION=0` gives the same `x : A`,
  `y : B`. So this is not merely a fragment-gate problem — that flag is not a workaround.

## Why it is not a false positive

Subtractive only: the missing rows are entailments rustdl fails to report. FP=0 is unaffected.
But it is worse than an ordinary MISS in one respect — `realize --json` emits no `incomplete`
field, so a consumer cannot distinguish "these are all the types" from "some types were
dropped". Classification's `incomplete` flag has no analogue here.

## Toward a fix (not attempted)

`SaturationResult.derived_same` already records functional / inverse-functional-forced
equalities — the data exists. A fix would union types across those groups in
`realize_via_saturation_internal`, which is sound because `derived_same` holds only entailed
equalities.

Two reasons it was not attempted here:

1. It would only fix the saturation path, and the **tableau path misses it as well**. A fix
   addressing one and not the other would leave the contradiction in place under a flag flip.
2. `realize` currently has no folding infrastructure at all, so this is a designed change, not
   an extension. An ad-hoc attempt in this area was already reverted once today
   (`accelerator_share_deadline`, `docs/2026-08-17-classify-has-no-budget-allocation.md`).

**Pinned by** `crates/owl-dl-reasoner/tests/realize_derived_same.rs`: the asserted-equality
control asserts today's correct behaviour and runs; the derived-equality test asserts the
CORRECT (currently failing) behaviour and is `#[ignore]`d with this file referenced. Remove the
`#[ignore]` when fixing.

## Adjudication status

Not peer-adjudicated: the HermiT wrapper produced no output for this fixture and Konclude's
CLI path here is classification, not realization. The entailment is nonetheless certain —
`InverseFunctional(r) + r(x,z) + r(y,z) ⊨ x = y` is definitional, and **rustdl's own
`individuals` query already derives it**, so the reasoner contradicts itself without needing an
external oracle.
