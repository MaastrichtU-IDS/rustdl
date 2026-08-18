# `realize` ignores DERIVED individual equality, silently

**Found:** 2026-08-18 · **Status:** **BOTH halves now FIXED (2026-08-18).** Functional half by
option A (the gate refuses a functional/inverse-functional role together with an
`ObjectPropertyAssertion`, so the tableau realizes it, and the tableau folds functional
merges). Inverse-functional half by `RUSTDL_INVERSE_FUNC_MAX` — **default OFF pending a
corpus sweep**, see the section at the end. · **Severity:** missed entailments with **no
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
| `rustdl realize --json` | had **no `incomplete` field at all** — the miss was silent. **A field was added 2026-08-18**, but it reports only CUT PROBES; it does NOT cover this defect, because no probe is cut here — the equality is simply never folded. So this miss remains silent even with the new signal. |

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

---

## THE INVERSE-FUNCTIONAL HALF IS FIXED — and the mechanism was one missing axiom

**Date:** 2026-08-18 · Flag `RUSTDL_INVERSE_FUNC_MAX`, **default OFF**.

| | `x` | `y` |
|---|---|---|
| default (flag OFF) | `A` | `B` |
| **`RUSTDL_INVERSE_FUNC_MAX=1`** | **`A`, `B`** | **`A`, `B`** |

### What was actually missing

The section above says the tableau path misses inverse-functional merges and calls that "a
second, independent gap in a different engine". **That framing was wrong in a useful way: the
merge was already implemented and default-ON.** `hyper.rs` walks a node's `preds` and merges
`r`-predecessors under `RUSTDL_INVERSE_FUNC_MERGE` — but it is triggered by `node.at_most`, i.e.
by an explicit `≤1` constraint on the node. Nothing ever put one there.

`convert.rs::derive_functional_max_cardinality` emits `∃r.⊤ ⊑ ≤1 r.⊤` for
`FunctionalRole(r)` and **had no inverse-functional counterpart**, so in the reproducer node
`z` — the shared filler, the node whose predecessors must merge — never acquired the `≤1 r⁻`
constraint that fires the merge it needed. The fix emits the missing GCI:

```
InverseFunctionalRole(r)  ⟹  ∃r⁻.⊤ ⊑ ≤1 r⁻.⊤
```

which is the *definition* of inverse-functionality, so it is entailed and cannot introduce a
false positive. **This is the "two engines" reading corrected: one engine, one absent input.**

### Why the fast path is not lost

A derived `≤1` is an unrecognised `Max` to `saturator_complete_fragment`, which would have
pushed **every** inverse-functional-bearing ontology off the saturation fast path — a large
silent perf regression from a flag whose purpose is a narrow realize fix. So the gate learned
the new shape (`is_derived_inverse_functional_max`), exactly as it already knew the functional
one. Verified: all three `inverse_functional/` fixtures report `# mode: pure EL` at **both**
flag settings.

The soundness argument for admitting it is the same one the FP-critical audit established for
the bare `InverseFunctionalRole` admission (`docs/2026-08-18-fp-critical-audit.md` §1): in that
fragment there are no nominals, no `ABox` and no inverse role *use*, so the canonical model is a
tree, every witness has exactly one predecessor, and an at-most-one bound on `r⁻` holds by
construction. The saturator dropping it costs nothing there; the GCI exists for the **wedge**,
which does enforce it.

### Evidence

* **The canary is retired** — `derived_equality_should_share_types` was `#[ignore]`d *for
  failing* and now runs and passes with the flag set.
* **A negative control pins the flag load-bearing** —
  `default_off_still_drops_derived_inverse_functional_equality` asserts the default is *still*
  incomplete, so the fix cannot silently become a no-op. It carries an instruction to delete
  itself when the default flips.
* **Closures identical ON vs OFF** on the three `inverse_functional/` fixtures, `pizza`, `ro`
  and `sio` — the direction of risk here is FP (the change ADDS a constraint), so this was
  checked rather than assumed.

### Why it ships default OFF

The change emits an axiom into **every** ontology carrying an inverse-functional role, and the
wedge then enforces a `≤1` it previously did not. That is a behavioural change on a broad
population, and this repo's own record is explicit that a 12-ontology benchmark is not a
population — a flag flipped on one took four ontologies from ~5 s to DNF. **A flip needs the
two-arm ORE sweep plus a ΔMISSED arm.** Neither has been run.

Note the flip is *also* what would let the shipped `RUSTDL_PSEUDO_MODEL` default recover its
falsified soundness-by-construction argument, since the witness would then apply
inverse-functional merges. That makes the sweep worth running, not a reason to skip it.
