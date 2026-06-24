# Precise ≤n merge-causation gate — RESULTS + VERDICT — 2026-06-23

**Verdict: NO-GO.** The precise ≤n merge-causation backjumping (`RUSTDL_PRECISE_MERGE_DEPS=1`)
**produces false-`Unsat` on wine** (FP=232, 26 spuriously-unsatisfiable classes) — the
increment-3 / merge FP graveyard. Per the pre-committed bar (a single corpus FP = NO-GO),
the increment is NOT flipped default-ON and is NOT merged. The branch
`feat/precise-merge-deps` is abandoned for the feature; the integration branch
`feat/build-once-redesign` default behaviour is unaffected (flag defaults OFF; flag-OFF is
byte-identical to current main).

## What was built (and reviewed sound by construction)

The ≤n rule (`solve_at_most`/`partition_rec`) was given dependency-directed backjumping
mirroring the ⊔ rule: decision level `d = init_depth - depth`; merge causation `cause =
at_most_dep ∪ {d}` threaded through `merge_with_cause` (folded into merged labels + the
merge-inherited ≤n's `at_most_dep` instead of tainting); accumulate child clash deps in
`combined`; backjump when a partition's clash `!contains(d)`; exhaustion `clash_deps =
combined.remove(d)` instead of `DepSet::ALL`; decline to `DepSet::ALL` when a `≠` participates.
Gated `RUSTDL_PRECISE_MERGE_DEPS`, default OFF.

Per-task reviews (Tasks 1–3 deep soundness) and a final whole-branch review all rated it
**sound by construction** — the argument being that three causation channels (copied labels,
`birth_deps` fold, copied-edge → derived-clause via `birth_deps`) guarantee every
merge-caused clash carries `d`, so a clash lacking `d` is genuinely merge-independent and
the backjump is safe.

## The corpus gate refuted the by-construction argument

`konclude_closure_diff`, `RUSTDL_PRECISE_MERGE_DEPS=1`, `RUSTDL_TEST_PAIR_MS=1000`:

| fixture | rustdl | konclude | FP | MISSED | unsat (r/k) |
|---|---|---|---|---|---|
| bibtex | 16 | 16 | 0 | 0 | 0/0 |
| pizza | 158 | 158 | 0 | 0 | 0/0 |
| sio | 8904 | 8904 | 0 | 0 | 0/0 |
| galen | 27997 | 27997 | 0 | 0 | 0/0 |
| notgalen | 32739 | 32739 | 0 | 0 | 0/0 |
| ore-15672 | 142 | 142 | 0 | 0 | 0/0 |
| **wine** | **637** | **405** | **232** | **0** | **26/0** |

7 fixtures FP=0; **wine FP=232 with 26 spurious-unsat classes** (`rustdl unsat=26 >
konclude unsat=0` — the genuine-FP signature, not a print/reduction artifact: 26 classes
wrongly marked `⊑⊥` cascade into ~232 spurious subsumptions; the `konclude=405` vs the
flag-OFF `653` reflects the comparison universe shrinking once 26 classes are spuriously
unsat). The flag is the sole cause: flag-OFF is byte-identical to current main **by
construction** (`precise_merge_deps == false` ⇒ `cause = DepSet::EMPTY` ⇒ every changed
site takes the unchanged EMPTY-cause / `DepSet::ALL` path — verified in the final
whole-branch review), all flag-OFF `cargo test --workspace` is green, and the 7 other
fixtures hold FP=0 even with the flag ON. (A dedicated flag-OFF wine closure-diff re-run was
also launched as belt-and-suspenders; the B2c gate already recorded flag-OFF wine at
653=653 FP=0 on this same code minus the feature.)

**Conclusion:** the precise backjump is **unsound on wine**. There is a path where a
merge-caused clash does NOT carry `d`, so the `!child.contains(d)` short-circuit (or the
`combined.remove(d)` exhaustion) backjumps past a relevant decision and reports a false
clash → false `Unsat`. The three-channel causation account is **incomplete on wine's
construct mix** (nominals + inverse + ∀ + ≤n interacting): a likely hole is a clash arising
on a node *other than* the merge survivor (e.g. via ∀-propagation off the merged node, a
nominal identification the merge enables, or an inverse-role edge) whose deps were never
widened by `cause`. Wine is precisely the construct soup that exposes it; the simpler
corpus fixtures (and all synthetic canaries) do not.

## Why the reviews missed it (the lesson)

The by-construction argument was checked against the ⊔-rule analogy and the *direct* merge
effects, but the ≤n merge in wine triggers *indirect* downstream propagation (∀/nominal/
inverse) whose clash deps are not reached by the three enumerated channels. A passing
soundness review + green synthetic canaries are NOT evidence of FP=0 on the FP-critical
nominal/merge fragment — only the corpus oracle is. This is the third time this fragment has
produced a silent FP under a "sound by construction" change (increment-3's 33272; the
snapshot cache's FP=100 on pizza; now this). **The corpus gate is the only ground truth
here.**

## Consequence

- **NO-GO**: do not flip default-ON; do not merge the gate code. `feat/precise-merge-deps`
  abandoned for the feature (kept for analysis of the exact FP path if revisited).
- Integration branch + main unaffected (flag OFF = byte-identical).
- The Konclude-class merge-dependency architecture, to be FP-safe on wine, needs the
  causation to cover the *indirect* (∀/nominal/inverse-mediated) clash paths the ≤n merge
  enables — i.e. Konclude's full per-fact dependency-node graph
  (`CMERGEDCONCEPT`/`CMERGEDLINK`/`CMERGEDIndividual` nodes, not just a single branch tag on
  the at_most_dep). That is the deeper multi-month build, now with a concrete FP target to
  reproduce against (wine's 26 spurious-unsat classes).

## FP localization (build-step-0 for the deeper per-fact-dep-graph build)

Per-class `sat_class_probe` sweep of all 137 wine named classes, flag-OFF vs flag-ON
(`tests/precise_merge_fp_diag.rs`, throwaway; adaptive-budget OFF, 30 s/class):

- **Flag-OFF: 0 unsat.** (Confirms flag-OFF wine has no spurious unsat — FP=0 baseline.)
- **Flag-ON: 56 unsat**, including **`vin:Wine` and `food#Wine` themselves**. Because the
  root `Wine` concept becomes unsatisfiable, all 55 wine-type subclasses
  (AlsatianWine, RedWine, Chardonnay, …) cascade to unsat.

**The FP is catastrophic, not a corner case:** the precise ≤n backjump makes the *root*
`Wine` concept unsat. `Wine`'s own structure is the classic wine pattern — `≤1 hasColor`
/ `≤1 hasSugar` / `≤1 hasBody` cardinality over nominal value-partitions
(`∃hasColor.{Red,White,Rosé}` + `∀hasColor.WineColor`, etc.). So the lost-dependency path is
the **≤n-merge of value-partition successors interacting with the nominal identification and
the `∀`-range propagation** — exactly the *indirect* clash channels (clash arises via a
`∀`-propagated label or a nominal merge on a node other than the ≤n survivor) that the three
direct causation channels (label fold / birth_deps / copied-edge) do not widen by `d`.

**Consequence for the per-fact-dep-graph build:** the dependency graph must track merge
causation through **nominal identification (`CMERGEDIndividual`)** and **∀-range
propagation (`CMERGEDLINK`/`CMERGEDCONCEPT`)**, not just the ≤n survivor's labels/at_most_dep.
Concrete FP reproduction: `sat(vin:Wine)` returns Unsat under
`RUSTDL_PRECISE_MERGE_DEPS=1`, Sat under flag-OFF.

## Method note

The gate was decisive because it ran the corpus oracle with the flag ON — not because the
code reviewed clean. The pre-committed "single FP = NO-GO" bar held: a sound-by-construction
argument that survives multiple reviews is still refuted by one oracle FP, and the oracle
wins.
