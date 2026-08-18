# Fixing `realize`'s dropped derived individual equality — design

> **OUTCOME 2026-08-18: option A IMPLEMENTED and it is CORPUS-INERT.** The functional case is
> fixed on the default path (`y : A, B` and `z : A, B`, was `y : A` / `z : B`) and its canary is
> live. But the guard fires on **0 of 64** ORE ontologies that carry a functional /
> inverse-functional role together with object property assertions — none of them is on the
> saturation fast path, so realize's TBox gate already rejected them. **This fixes a reachable
> shape, not a corpus problem. Do not cite it as a corpus improvement.**
>
> **Gate 3 had to be run twice, and the first run was VACUOUS.** It selected on
> `functional role + ObjectPropertyAssertion` and reported "22 ontologies, 0 regressions, 0
> gains, wall +0.3%". All 22 were hybrid or DNF on classify, so the guard never fired and the
> two arms were identical by construction — the flat wall was the tell. The guard needs THREE
> conditions together (TBox-eligible **and** the characteristic **and** ground edges); two were
> checked. Same error shape as testing a hypothesis on the DNF tail, and the reason this repo's
> method notes demand proving the instrument fires by a criterion declared in advance.
>
> **Gate 4 also failed** and is unresolved: of 35 population probes, 33 timed out at 45 s,
> leaving 2 usable samples and 0 derived equalities. That zero is a broken instrument, not
> evidence of rarity. The syntactic upper bound is ~168 of 1,920.
>
> **Proxy caveat:** the "0 of 64" scan used classify's `# mode: pure EL` banner as a proxy for
> realize eligibility, which also accepts `saturator_complete_fragment` and
> `tbox_only_saturator_eligible`. So it may UNDERCOUNT, and 0-of-64 is strong evidence of
> inertness rather than proof.

**Date:** 2026-08-18 · **Status:** option A IMPLEMENTED (see OUTCOME above) · Defect:
`docs/known-limitations/realize-drops-derived-individual-equality.md`

## The defect in one table

Measured, both fixtures × both paths, 2 runs each, stable:

| forced equality | saturation path (**DEFAULT**) | tableau path (`RUSTDL_REALIZE_SATURATION=0`) |
|---|---|---|
| inverse-functional (`r(x,z)`, `r(y,z)` ⊨ `x=y`) | `x:A`, `y:B` ✗ | `x:A`, `y:B` ✗ |
| functional (`r(x,y)`, `r(x,z)` ⊨ `y=z`) | `y:A`, `z:B` ✗ | `y:A,B` `z:A,B` ✓ |

`rustdl individuals` derives the equality in **both** cases (`same_groups`), and
`realize --json` has **no `incomplete` field**, so the miss is silent. Asserted
`SameIndividual` works, because `realize_saturation_eligible` refuses it
(`realize.rs:747`) and the tableau then merges — that is the only equality mechanism realize
has.

## The obstacle that makes this a design, not an edit

`SaturationResult.derived_same` is produced by **`abox_saturation`** (`abox_saturation.rs`),
the reasoner-level ABox fixpoint that `individuals.rs` runs. Realize's fast path uses a
**different** pass — `saturate_for_realize` in `owl-dl-saturation`, returning
`(subsumers, nominal_by_ind)`.

So "just fold `derived_same`" requires realize to run a pass it does not currently run — and
that pass is the one carrying the adaptive 3 s / 12 s budget on the classify path, because it
is expensive on large ABoxes (`ore_ont_5368`: 5,936 ms, dominated by a prelude that walks
18.6 M lowered axioms and is **budget-independent**). Adding it unconditionally to realize
would be a perf regression on exactly the ABox-heavy inputs realize is used for.

## Options

### A. Refuse the constructs in the gate (RECOMMENDED)

Extend `realize_saturation_eligible` to return `false` when the ontology has a
`FunctionalRole` / `InverseFunctionalRole` declaration **and** any
`ObjectPropertyAssertion` — mirroring exactly what it already does for `SameIndividual`.

* **Correct for the functional case** by construction: the tableau folds it (measured).
* **No worse for inverse-functional**: still wrong, but now wrong *consistently* on both
  paths, and the residual is the separately-tracked tableau gap rather than a
  path-dependent contradiction.
* **Smallest possible change**, consistent with the existing design, and no new pass.
* **Cost:** those ontologies take the tableau realize path, which is slower — and
  `RUSTDL_REALIZE_PAIR_TIMEOUT_MS` (default 750 ms) then bounds each probe, so a large ABox
  could lose types to the budget instead. **That trade must be measured before shipping**:
  swap a silent wrong answer for a slower, possibly-budget-truncated one only if the
  truncation is rarer than the current miss.

### B. Run `abox_saturation` in realize, gated on a cheap precondition

Same precondition as A (functional-ish role + object property assertions), but instead of
falling back, run `abox_saturation`, then union `entailed_types` across each `derived_same`
group and recompute `most_specific_types` from the union.

* **Fixes both constructs on the default path** — strictly more correct than A.
* Keeps the fast path's speed for the (majority) ontologies where no merge is possible.
* **Cost:** an extra fixpoint on the ontologies that do qualify, unbounded in realize (the
  3 s/12 s adaptive budget is classify-specific). Needs its own budget decision, and note the
  prelude cost is budget-independent so a budget does not bound it.

### C. Do nothing; document only

Already done (the known-limitation doc plus two `#[ignore]`d canaries and a non-ignored pin on
the tableau asymmetry). Acceptable if realize on functional-role ABoxes is not a user-facing
path — **unknown, and worth establishing before choosing**.

## Recommendation

**A**, then reassess B. It is small, uses an existing mechanism, and converts a
path-dependent contradiction into one consistently-tracked gap. B is the more complete fix but
introduces a new unbounded pass on a path that currently has none, which is how the
`accelerator_share_deadline` attempt went wrong earlier the same day — a plausible mechanism
shipped without measuring what it displaced.

## Gates any implementation must pass

1. **Remove the `#[ignore]`** from `derived_functional_equality_should_share_types` (A and B)
   and from `derived_equality_should_share_types` (B only). Both are verified to fail today, so
   they are real gates rather than placeholders.
2. **Keep `tableau_path_does_handle_functional_equality` passing** — it pins the only
   workaround.
3. **Measure the displaced cost**, not just the fixed verdict. For A: realize wall and type
   counts on ABox-bearing ORE ontologies with functional roles, before/after, watching for
   types lost to `RUSTDL_REALIZE_PAIR_TIMEOUT_MS` rather than gained. For B: the added fixpoint
   wall on the qualifying population.
4. **Size the population first.** How many ORE ontologies have a functional or
   inverse-functional role *and* object property assertions? If that set is tiny, C is the
   right answer and A/B are not worth their risk. This check costs one corpus scan and should
   precede any code — the session's own repeated lesson is that a shape census sizes a
   population while a target list predicts nothing.
5. **`realize --json` should gain an `incomplete` signal** regardless of which option is taken.
   The absence of one is arguably the more serious half of the defect: a consumer currently
   cannot distinguish a complete type set from a truncated one, and both A (budget truncation)
   and B (skipped fold) can produce truncation.

## Explicitly out of scope

The tableau's inverse-functional gap. `RUSTDL_INVERSE_FUNC_MERGE` is default-ON in
`owl-dl-tableau`, yet the tableau realize path still misses this merge — so either the flag
does not reach the realize probes or the ABox seeding does not apply it. That is a second
investigation in a different engine and should not block a fix to the first.
