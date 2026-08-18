# `realize` ignores DERIVED individual equality, silently

**Found:** 2026-08-18 · **Status:** FUNCTIONAL half FIXED 2026-08-18 (option A — the gate now refuses a functional/inverse-functional role together with an `ObjectPropertyAssertion`, so the tableau realizes it); INVERSE-FUNCTIONAL half still open · **Severity:** missed entailments with **no
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
* **For inverse-functional, the tableau path misses it too**: `RUSTDL_REALIZE_SATURATION=0`
  gives the same `x : A`, `y : B`. So for that construct it is not merely a fragment-gate
  problem, and the flag is not a workaround.

## REFINEMENT (2026-08-18): the two paths fail differently, and it matters

Repeating the experiment for a **functional**-forced equality —
`FunctionalObjectProperty(:r)` with `r(x,y)`, `r(x,z)`, so `y = z`
(`fixtures/realize_derived_same/functional.ofn`) — separates the two mechanisms.
Both fixtures, both paths, 2 runs each, stable:

| forced equality | saturation path (**default**) | tableau path (`RUSTDL_REALIZE_SATURATION=0`) |
|---|---|---|
| inverse-functional | `x : A`, `y : B` ✗ | `x : A`, `y : B` ✗ |
| **functional** | `y : A`, `z : B` ✗ | **`y : A, B` and `z : A, B` ✓** |

So:

* **The saturation realize path is uniformly wrong** — it drops BOTH functional and
  inverse-functional forced equalities. This is the single defect responsible for the default
  behaviour in both cases, and folding `SaturationResult.derived_same` would fix both.
* **The tableau handles functional merges but not inverse-functional ones.** So there are two
  independent gaps, not one, and they are in different engines.
* **A workaround therefore exists for the functional case only**:
  `RUSTDL_REALIZE_SATURATION=0` gives the correct answer. There is no workaround for the
  inverse-functional case.

This corrects the bullet above, which was written from the inverse-functional fixture alone and
generalised one construct too far.

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

**The refinement above strengthens the case for exactly this fix.** Because the saturation
path is the DEFAULT and is wrong for *both* constructs, folding `derived_same` there would
correct the default behaviour in both cases — not just one. It would leave a residual: the
tableau path would still miss inverse-functional merges, so `RUSTDL_REALIZE_SATURATION=0`
would remain wrong for that construct. That residual is a second, independent gap in a
different engine and should be tracked separately rather than blocking the first fix.

Not attempted here because `realize` has no folding infrastructure to extend — this is a
designed change, and an ad-hoc attempt in this area was already reverted once today
(`accelerator_share_deadline`,
`docs/2026-08-17-classify-has-no-budget-allocation.md`). Scoping it properly means deciding
where the fold lives, whether `most_specific_types` is recomputed after folding, and how the
absent `incomplete` signal should behave when folding is skipped.

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
