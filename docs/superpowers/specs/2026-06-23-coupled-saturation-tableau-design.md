# Coupled saturation–tableau for rustdl — scope / decomposition (design)

**Date:** 2026-06-23
**Status:** approved decomposition (brainstorming) → SP0 spike is the immediate next unit
**Type:** multi-month, multi-sub-project, soundness-critical architecture investment

Extend rustdl's EL+ saturator into a Konclude-style **approximated saturation** — a
sound, deterministic over-approximation of consequences over the *non-EL* constructs
(value-partitions / `≤n` / nominals) that are wine's actual hardness — computed once and
used to **seed** the per-pair wedge, so hard subsumption tests collapse. This is the
structurally-scoped wine lever from `docs/konclude-vs-rustdl-wine-2026-06-23.md`.

## Why this, and why now it's evidence-backed (not speculation)

Konclude classifies wine in ~200ms; rustdl DNFs at 1991s. Instrumentation (native
binary + source `github.com/konclude/Konclude`, cloned) pinned the mechanism to a
**saturation precompute** that makes per-test reasoning trivial. The decisive
**matched** measurement (same hard test, both engines):

`sat(AlsatianWine ⊓ ¬AmericanWine)` (satisfiable / a non-subsumption rustdl burns on):
- **Konclude: 58ms = 39ms saturation precompute + 1ms the test.**
- **rustdl: DNF at 60s.**

A single `-x` satisfiability query does **not** exercise KPSet pair-pruning, so this
isolates the per-test lever with no confound: **with the saturation, a hard refutation
rustdl can't finish in 60s is 1ms.** Per-test tree-shrinking via sound saturation is a
genuine, sound lever — distinct from the killed reuse-trap (which reused *one model's*
labels; saturation reuses *all-model* facts).

## Soundness model (the crux)

The saturation derives only **all-model consequences** (a sound under-approximation of
entailments — exactly the property rustdl's EL saturator already has). Seeding the wedge
from those facts is therefore sound. **This is precisely why it is sound where the
snapshot cache was not:** the snapshot cache replayed *one satisfying model's* node
labels (`sup ∈ that-model ≠ sub ⊑ sup` on non-Horn → FP, the reuse-trap); saturation
seeds with facts that hold in *every* model.

**Soundness obligation that does NOT come for free:** the *coupling* layer — seeding,
pruning branches that "contradict" saturated consequences, reusing a saturation across a
different `¬sup` injection — has its own FP surface. The snapshot cache *also* looked
obviously sound and ORE found 30+ FP. **Therefore every gate verifies FP=0 (byte-identical
closures) on the COUPLED system, never inferred from the saturator's soundness alone.**

## Honest prize (read before committing effort)

The beneficiaries are **wine + the heterogeneous, mostly-obscure-benchmark SROIQ DNF
tail** (ORE-2015 sample onts). The user's working corpus is already fast; this is a
**capability / architecture investment, not a working-corpus speedup**. And per-test
saturation **alone is not Konclude-parity** — Konclude's 120ms full-classify also relies
on KPSet pair-pruning (SP3 here). The saturation sub-projects deliver "hard tests become
tractable," not "wine classifies in 200ms," until SP3 lands too.

## Decomposition

Each sub-project is its own spec → plan → build, each with an FP=0 corpus gate. **SP0
gates the entire project** — the real sub-projects start only if SP0 clears.

### SP0 — GATING SPIKE (throwaway, ~weeks). Tests the LEAP, not the answered question.

The answered question is "does saturation help" (yes, matched-measured). The **unproven
leap** is: *can rustdl extend its EL+ saturator to handle the non-EL constructs that ARE
wine's hardness (value-partitions/`≤n`/nominal) and actually collapse the branching?*
Konclude's saturation reasons over exactly the constructs rustdl's saturator **drops by
construction**, so this is genuinely open. (An earlier "seed from the existing EL closure"
gate idea is rejected as confounded: wine is out-of-EL, so the EL closure is thin and a
null result would be pre-determined, not informative.)

**Spike:** hand-implement a saturation pass over wine's value-partition fragment —
**wine-specific / hard-coded and soundness-relaxed is fine; it is throwaway** — that
computes the deterministic value-partition consequences, and seed the wedge from them.

**Measure (the gate):**
1. Does `sat(AlsatianWine ⊓ ¬AmericanWine)` (and a few other rustdl-DNF wine pairs) drop
   from 60s-DNF toward Konclude's ballpark (sub-second)?
2. Does the tuned-corpus closure stay FP=0 on the coupled system? (Even though the spike
   may be unsound internally, this checks whether the *coupling shape* can be made sound.)

**Verdict rule (pre-committed):** proceed to SP1+ only if (1) the wine branching
**collapses** (DNF → sub-second on the matched pairs) AND (2) the coupling shape shows no
fundamental FP obstacle. If the branching does **not** collapse even with a wine-specific
hack, the general project is **dead — killed in weeks, not months.**

### SP1 — Sound saturation rules for the non-EL fragment

General, sound, all-model over-approximation rules for `∀` / `≤n` / nominal, extending
`crates/owl-dl-saturation`. Produces a reusable saturation structure (graph with role
edges, not just subsumer sets). FP gate on the saturator's derived closure.

### SP2 — Coupling / seed

The wedge starts from the saturation (seeded deterministic state) and prunes against it;
the saturation is reused across the O(n²) pairs. **FP gate on the coupled system** (the
snapshot-cache-precedent risk lives here).

### SP3 — KPSet possible-subsumer classification

Derive *possible* subsumers from the saturation and test only those, pruning the O(n²)
pair count. Required for full-classify parity (separate from per-test speed). FP gate.

## What this is NOT / out of scope

- Not the conflict-learning / clash-driven search lever (separately gated NO-GO; wine's
  per-pair backjumping already fires, the issue is recomputation-without-saturation).
- Not a working-corpus speedup; EL/Horn onts already route to the saturator fast path and
  are unaffected.
- SP1–SP3 are sketches here; each gets its own full design only after SP0 clears. This
  document scopes the decomposition and details SP0.

## References

- `docs/konclude-vs-rustdl-wine-2026-06-23.md` (mechanism + matched measurement).
- Steigmiller & Glimm, "Pay-As-You-Go DL Reasoning by Coupling Tableau and Saturation
  Procedures", JAIR 54 (2015) — the published algorithm (saturation = forward-chain the
  deterministic rules to fixpoint, defer disjunction/nominal/cardinality choices to the
  tableau, seed the tableau from the saturation). PDF saved in session tool-results.
- Konclude source (cloned, session scratchpad): `Source/Reasoner/Consistiser/CSaturation*`,
  `CApproximatedSaturationCalculationJob`, `CSaturationCommonDisjunctConceptsExtractor`.
