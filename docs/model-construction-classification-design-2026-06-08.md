# Model-construction-centric classification — design spec (B)

Drafted 2026-06-08. The user committed to "B" — the move toward HermiT/Konclude-
style global classification — after A (sentinel-guarded snapshot reuse) was
measured dead (`reuse-trap-A1-scoping-2026-06-08.md`). This is the design pass;
**no engine code yet** — produce this spec + the Phase-1 measurement, then review.

## §0 Non-negotiables (read first)

1. **This is EVOLUTION, not a rewrite. Do NOT greenfield.** rustdl already ships
   the HermiT classification skeleton — the Phase-7 **label oracle** (possible
   subsumers from a model), the **told-subsumer/told-disjoint tables**
   (`told.rs`, known subsumers/non-subsumers), the **`find_direct_parents_top_down`**
   traversal, the **wedge** (per-class model construction), and the **snapshot**
   (model capture). All are sound today. B *completes and connects* these; any
   design that throws them away re-introduces FP risk in machinery that is
   currently correct. The spec forbids replacing the working parts.
2. **FP=0 stays sacred.** The A1 lesson is load-bearing here (see §5).
3. **Completeness gains teeth (NEW).** Per-pair `trust_sat` made completeness a
   soft, by-composition property. B *derives the hierarchy from constructed
   models*, so model-construction completeness becomes **load-bearing for
   correctness**. This forces an explicit posture decision (§2).

## §1 What ships today (the foundation) + the gap

Classify today: saturation closure → per-class label oracle (wedge-Sat model →
`D∉labels(C)` is a sound non-subsumption, prunes 96–100%) → for the residual
`D∈labels(C)` candidates ("pass_through"), an explicit per-pair wedge/tableau
test. The cost — and the wine wall — live entirely in that **per-pair test of the
`pass_through` candidates**.

The gap vs HermiT/Konclude:
- HermiT brackets each class between **known** subsumers (told ∪ deterministic
  model core) and **possible** subsumers (model labels), and only *tests* the
  candidates strictly between. rustdl has the *possible* half (label oracle) and
  a *known* half (told) but **does not bracket** — every `D∈labels` candidate
  still gets a full test even when told already settles it.
- HermiT/Konclude **share/merge models** across classes; rustdl builds one per
  class.
- HermiT's model construction **terminates** on nominals/cardinality; rustdl's
  wedge does **not** (the wine wall — measured this session).

## §2 B carries TWO goals — split them, and choose a completeness posture

These are independent and must not hide under one "B":

- **B-perf — test minimization via known/possible bracketing + model sharing.**
  Faster classification *on the workloads that already terminate*. Does NOT touch
  wine. Sound-preserving; the question is whether the payoff exists (§4 Phase 1).
- **B-complete — terminating model construction on the nominal/cardinality
  fragment.** This is the *only* thing that closes the wine wall. Hard; it is the
  HermiT hypertableau model-construction + blocking work.

**Completeness-posture fork (a decision for the user; Phase 1 informs, does not
decide):**
- **(i) Guarantee completeness** → terminating-construction-on-nominals is a
  *blocking prerequisite*, not a late phase, because the hierarchy is read from
  models. B = the termination project first.
- **(ii) Sound under-approximation with a documented fragment** → B is "faster on
  what already works" (B-perf), wine stays gapped (knob: `--pair-timeout-ms 25`),
  completeness is by-composition as today. B = bracketing + sharing, no
  termination obligation.

You cannot have "global model reuse" (perf) and "closes wine" (completeness)
cheaply under one effort. Pick the posture before Phase 2.

## §3 Components, mapped to goals + risk

| Component | Goal | Builds on | Risk |
|---|---|---|---|
| **Known/possible bracket** — settle `D∈labels` candidates via told ∪ deterministic-model-core *without a test*; test only the strict gap | B-perf | label oracle + told.rs | Low (told is already sound); the deterministic-model-core extraction needs a "this label is forced, not a disjunctive choice" proof |
| **Model sharing/merging** across classes | B-perf | wedge model + a structural key | **RED — A1-shaped.** Two classes sharing a model ⇒ one can inherit a `Subsumed` that holds only in the other's model. Any "structurally-equal" key needs a *canonical-model* soundness proof, exactly like the label oracle has. Do NOT ship without it. |
| **Terminating model construction** on nominals/cardinality | B-complete | wedge (`hyper.rs`) blocking + NN-rule | High — the wine wall; HermiT-grade blocking on nominals. The actual hard problem. |

## §4 Phased plan (measurement-first)

**Phase 1 — measure B-perf's ceiling (zero new engine; do this, then review).**
The label oracle *already* eliminates the `D∉labels` pairs. So the bracket's
*new* contribution is only: **of the `label_cache_pass_through` pairs (the
`D∈labels` candidates that currently still get a wedge/tableau test), how many are
already decided by told-subsumer or told-disjoint?** Instrument exactly that
count, corpus-wide (pizza, ore-10908, ore-15672, sio, wine, galen/notgalen are
Horn-shortcircuited so N/A).
- **Decision rule:** if a large fraction of `pass_through` is told-decidable → a
  bracket is a real, sound, low-risk perf win → pursue B-perf. If near-zero (the
  shipped oracle+told already capture the cheap pairs) → **B-perf is marginal**,
  and the only prize is wine-via-termination → B collapses to the B-complete
  termination project, and the posture fork (§2) is forced to (i).

**Phase 2+ — depends on the posture chosen (§2), not pre-committed here.**
- If B-perf (posture ii): formalize the bracket (Phase 1 proved the payoff), then
  model-sharing *only* with the canonical-key proof (§3 red).
- If B-complete (posture i): scope terminating nominal model construction as its
  own design pass — this is the deep `hyper.rs` blocking/NN-rule work; it has its
  own dead-ends (do not re-enter via search/learning — that was measured dead).

## §5 Soundness/completeness obligations + dead-ends NOT to repeat

- **A1 (the governing lesson):** a `Subsumed` derived from *one* model is unsound
  on non-Horn (`sup∈one-model ≠ sub⊑sup`). B must *bracket-and-verify*, never
  "read subsumptions off the model." The known half must be genuinely *forced*
  (told / deterministic core), not "present in this model."
- **Model-validation per pair** (re-verify each model-derived subsumption) ≈
  dead-end §9 and defeats the perf purpose — verify §9's exact scope before
  relying on it.
- **Do NOT re-enter via search/learning:** 1-UIP NO-GO (backjump dist ≈1),
  simple nogood learning 0-un-stalled, MOMS reverted, gate-loosening FP-unsound,
  A1 dead. The wine wall is *model construction/termination*, not search.
- **FP=0 gate every increment** on the full corpus closure-diff vs HermiT/Konclude.

## §6 Anchors

- Code: `crates/owl-dl-tableau/{hyper.rs,snapshot.rs}`,
  `crates/owl-dl-core/src/told.rs`, `crates/owl-dl-reasoner/src/{lib.rs,classify.rs}`
  (`LabelOracle`, `find_direct_parents_top_down`, `label_cache_pass_through`).
- Literature: Glimm, Horrocks, Motik, Stoilos — *"Optimising Ontology
  Classification"* (ISWC 2010) = the known/possible bracket + enhanced traversal;
  Motik/Shearer/Horrocks — *Hypertableau* (JAIR 2009) = the model construction;
  Konclude (Steigmiller et al.) = saturation+tableau hybrid + parallel.
- Prior rustdl: `docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md`
  (the snapshot-cache project this supersedes for non-Horn),
  `reuse-trap-A1-scoping-2026-06-08.md`, `hypertableau-dead-ends.md`.
- Memory: [[next-big-bet-reuse-trap-nominal-termination]],
  [[snapshot-gate-loosening-dead-end]], [[corpus-parity-achieved]].

## Recommendation

Write done. **Next: run the Phase-1 measurement** (`pass_through` pairs decidable
by told alone) — it's pure measurement on shipped components and it *forces the
goal fork*: a real B-perf payoff, or "B is the wine-termination project." Then
stop for review and the posture decision. Do **not** start engine code before the
posture is chosen — the perf path and the termination path share almost no code.
