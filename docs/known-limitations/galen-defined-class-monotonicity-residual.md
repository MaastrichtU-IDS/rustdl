# galen: 1 residual missed subsumption (defined-class ∃-monotonicity)

**Status:** open (sound; not scheduled). This is the one pair left after the
2026-07-11 incremental functional/`≤1`-merge fix closed the other 9 — see
[`docs/known-limitations/galen-inverse-functional-completeness.md`](galen-inverse-functional-completeness.md).
It is a **different mechanism**, not a residue of that fix.

**Pair:** `TibialTuberosity ⊑ TibialInterCondylarEminence`
(`http://ex.test/galen#TibialTuberosity` ⊑ `http://ex.test/galen#TibialInterCondylarEminence`).

## The axioms (verbatim from galen.ofn)

```
EquivalentClasses(TibialTuberosity
    ObjectIntersectionOf(Eminence
        ObjectSomeValuesFrom(isAtOtherEndOf LigamentumPatellae)
        ObjectSomeValuesFrom(isSpecificSolidDivisionOf Tibia)))

SubClassOf(Tibia
    ObjectSomeValuesFrom(hasSpecificSolidDivision TibialInterCondylarEminence))

InverseObjectProperties(hasSpecificSolidDivision isSpecificSolidDivisionOf)
FunctionalObjectProperty(isSpecificSolidDivisionOf)

EquivalentClasses(TibialInterCondylarEminence
    ObjectIntersectionOf(Eminence
        ObjectSomeValuesFrom(isSpecificSolidDivisionOf TibialPlateau)))
```

(Confirmed via `robot explain --reasoner HermiT --mode entailment`, which
independently derives the same pair, corroborating Konclude.)

## The justification (5 axioms)

Let `x : TibialTuberosity` (`TT`). We must show `x : TibialInterCondylarEminence` (`TICE`).

1. From `TT`'s definition, `x` has an `isSpecificSolidDivisionOf`-successor
   `w`, with `w : Tibia`.
2. From `Tibia ⊑ ∃hasSpecificSolidDivision.TICE`, `w` has a
   `hasSpecificSolidDivision`-successor `v`, with `v : TICE`.
3. Since `hasSpecificSolidDivision ≡ inverse(isSpecificSolidDivisionOf)`,
   step 2's edge also gives `v —isSpecificSolidDivisionOf→ w`.
4. From `TICE`'s own definition (`v : TICE`), `v` must have *some*
   `isSpecificSolidDivisionOf`-successor typed `TibialPlateau`. But
   `isSpecificSolidDivisionOf` is **functional**, and step 3 already gives `v`
   one such successor (`w`) — so that witness *is* `w` (a functional/`≤1`
   merge — the same mechanism the 2026-07-11 fix made incremental-and-fast).
   Hence **`w : TibialPlateau`**.
5. Back at `x`: the edge from step 1 (`x —isSpecificSolidDivisionOf→ w`) now
   witnesses `∃isSpecificSolidDivisionOf.TibialPlateau` — since `w` is now
   known to be `TibialPlateau` — so `x` satisfies `TICE`'s existential
   conjunct, and (with `x` already `Eminence`) `x : TICE`. ∎

## Why this is *not* closed by the 2026-07-11 merge fix

The functional merge at step 4 is exactly the mechanism the incremental
`horn_fixpoint` merge now handles, fast and by default. The reason this pair
still MISSES is *how the subsumption query reaches that merge*: `classify`
tests `TT ⊑ TICE` by checking satisfiability of `TT ⊓ ¬TICE`. Negating
`TICE`'s existential conjunct in NNF gives a **disjunction**:

```
¬TICE  ≡  ¬Eminence  ⊔  ∀isSpecificSolidDivisionOf.¬TibialPlateau
```

`x : Eminence` (from `TT`) immediately clashes the `¬Eminence` disjunct, so
the *only* surviving branch is `∀isSpecificSolidDivisionOf.¬TibialPlateau`.
Propagating that `∀` across `x`'s `isSpecificSolidDivisionOf`-edge to `w`
gives `w : ¬TibialPlateau` — which must then clash against the `w :
TibialPlateau` derived via the functional-merge chain (steps 1–4 above) in
the *same* branch.

That is: refuting `TT ⊓ ¬TICE` needs the disjunctive `¬`-expansion (choosing/
discarding the `¬Eminence` branch) **and** `∀`-propagation to run in the same
branch as the functional merge. The incremental merge lives entirely in
`horn_fixpoint` — the deterministic, Horn-only fixpoint loop — precisely
because that determinism is what makes it fast and safe to default on. This
pair needs genuine non-Horn tableau machinery (disjunctive branching +
`∀`-propagation) to interact with the merge, which `horn_fixpoint` does not
(and structurally should not) attempt. It is thus a distinct completeness gap,
not a regression or incompleteness in the merge itself.

## Future work

Not scheduled. Closing it would mean either (a) routing this specific pattern
(a disjunction whose only live branch immediately needs a functional-merge
witness) through the full wedge search rather than the Horn shortcut, or (b) a
purpose-built defined-class ∃-monotonicity rule that recognizes
`C ≡ D ⊓ ∃R.E`, `E' ⊑ ∃R⁻.C'` (`R` functional, `R⁻` its declared inverse),
`C' ≡ D ⊓ ∃R.F`, `E ⊑ F`-shaped patterns and derives `C ⊑ C'` directly without
disjunctive search. Neither is attempted here.

## Pointers

- Parent finding: `docs/known-limitations/galen-inverse-functional-completeness.md`
- Authoritative numbers: `docs/benchmarks/2026-07-11-curated/MATRIX.md` (regenerated
  post-fix; galen `MISSED 1`)

## Follow-up diagnosis (2026-07-12): the abstract pattern is NOT the gap

Two minimal reproductions were built and BOTH derive `TT ⊑ TICE` correctly in rustdl:
1. **Told filler subsumption:** `TT ≡ E ⊓ ∃g.Sub`, `TICE ≡ E ⊓ ∃g.Sup`, `Sub ⊑ Sup` → rustdl derives `TT ⊑ TICE` (Konclude agrees).
2. **Merge-derived filler subsumption:** same, but `Sub ⊑ Sup` is itself derived via the functional/≤1-inverse merge (`Sub ⊑ ∃f.M`, `f ≡ inv(g2)`, `Functional(g2)`, `M ≡ ∃g2.Sup`) → rustdl still derives both `Sub ⊑ Sup` and `TT ⊑ TICE`.

So the defined-class ∃-monotonicity rule works in isolation, even over a merge-derived filler
subsumption. The galen miss is therefore **context/scale-dependent** (as the original galen
≤1 investigation was): at full 2748-class scale the top-down classifier's label-cache /
candidate-recovery / rule-ordering drops this one pair, even though the underlying reasoning
is present. Not budget-bound (identical at 250 ms and 3000 ms). Closing it needs in-galen
instrumentation (which pair-candidate is pruned and why) rather than a rule addition — there
is no minimal repro to anchor a fix. Low ROI (1 of 28007 subsumptions; sound MISS; fast),
so deferred. rustdl remains sound (FP=0) and near-complete on galen (MISSED 1).

## Confirmed mechanism + fix triage (2026-07-12, advisor-consulted)

**Root cause (confirmed, code-grounded):** the per-class **label-cache prune** in the
top-down classifier drops `(TT, TICE)`. `classify_labels(TT)` runs a plain `sat(TT)` with
no target sup and reads root-node labels; a defined class enters root labels only via a
base Horn clause, but `TICE`'s backward direction `Eminence ⊓ ∃g.TibialPlateau ⊑ TICE`
has an `∃` in the body — not clausal — so `TICE ∉ labels(TT)` and the pair is pruned at
`crates/owl-dl-reasoner/src/classify.rs:1677-1700` before `subsumes_via_tableau` ever
runs. The prune is an **unsound over-approximation for ∃-bearing defined sups**
(completeness only — never FP; hence the self-reported `misses=0`). Only this one pair
escapes both safety nets (EL-closure short-circuit + conjunctive back-fold): its filler
subsumption `Tibia ⊑ TibialPlateau` is merge-derived (not an EL-closure fact) and its sup
body is existential (no base-clause back-fold). Both minimal repros pass because the
single-pair `is_subclass_of` API bypasses the label cache entirely.

**Existing flag does NOT work as a fix.** `RUSTDL_CLASSIFY_DEFINED_SWEEP=1` bypasses the
prune and full-tableau-verifies defined sups — which **explodes galen's wall**: the matrix
did not finish in 6:40 (unbounded) and galen `classify --pair-timeout-ms 250` did not
finish in 3:20. Bypassing the prune makes classify verify *all* of galen's defined-class
sups via the tableau, and many of those probes are expensive/non-terminating (the same
cyclic structure that makes a direct `is_subclass_of(TT,TICE)` hang >90 s). So the sweep
is not a viable default.

**Viable fix (deferred, low ROI):** a *targeted* improvement — teach `classify_labels`
to include ∃-bearing defined classes as candidate sups (a definitional back-fold /
∃-monotonicity-into-defined-class step in the label computation), so the prune stays a
sound over-approximation and only *genuine* candidate pairs (like `TT⊑TICE`) are
verified — not all defined sups. This is a deeper engine change to the wedge's label
machinery. For a single subsumption out of 28 007 (sound MISS, fast), it is not worth the
risk now; documented here for a future dig. **rustdl remains sound (FP=0) and
near-complete on galen (MISSED 1).**
