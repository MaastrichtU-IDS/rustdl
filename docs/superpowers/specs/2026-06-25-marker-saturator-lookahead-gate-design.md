# Marker-saturator ⊔ failed-literal look-ahead — gate design (2026-06-25)

**Status:** design / gate spec. SP-A of the wine→Konclude program.

## Goal

Decide, by measurement, whether wiring the **full marker saturator**
(`owl-dl-saturation`: NomKey / ForallKey / MaxKey / functional-merge) as a
**failed-literal propagator at the wedge's ⊔ points** collapses wine's hard-class
model builds toward Konclude-class — or proves a floor. One env-gated flag
(`RUSTDL_SAT_LOOKAHEAD`, default OFF), throwaway-allowed code, the verdict doc is
the durable deliverable (like the 5 prior wine gates).

## Context — the reframing this gate tests

Wine classify is ~310 s vs Konclude ~114 ms. The wall is **not** the saturation
closure (the saturator already outputs wine's complete, correct closure 201=201,
FP=0, in seconds — B2c). The wall is **per-pair refutation**: the wedge proves
non-subsumptions, and ~19 hard classes (SweetWine, Zinfandel, …) won't build a
satisfying model within budget → `LabelOracle::NoVerdict` → their ~4638 pairs fall
through to per-pair tableau refutation (4688 timed-out pairs). Those hard model
builds thrash (68 796 branches) on one pattern: **exactly-1 role over a fixed
nominal set** (`hasColor ∈ {Red,White,Rosé}`, `hasSugar ∈ {Dry,OffDry,Sweet}`, …).

The decisive asymmetry: the **saturator solves that exact pattern fast and FP=0**
via its marker machinery, while the **wedge re-derives it by blind ⊔-branching +
≤1-merge** and thrashes. This gate tests whether feeding the saturator's reasoning
into the wedge's branch choice removes the thrash.

### Why this is not a re-tread

Two ⊔-look-ahead relations are already NO-GO this session, both excluded here:
- **told-disjointness** (SP-B): pruned = 0 — too weak; wine's determinacy lives in
  ∀/nominal/≤n, never surfacing as a told-disjoint named-class disjunct.
- **horn_fixpoint look-ahead** (det-pruning): FP = 232 + only 18–34 % collapse —
  horn_fixpoint is deterministic and **skips the ≤n-merge / nominal-id**, the exact
  constructs wine's determinacy lives in, so it over-reports ≤n-clashes (drops live
  disjuncts → unsound) and under-collapses the rest.

The **full marker saturator** is the untested relation: it *does* carry the
∀/nominal/≤n reasoning horn_fixpoint lacks (NomKey/ForallKey/MaxKey/functional-merge),
and it is FP=0 corpus-wide **including wine 653=653**. Also excluded: the per-fact
dep-graph / merge-backjump direction (NO-GO #2/#3) — categorically different, it
*narrows* deps (removes entailments → FP-dangerous), whereas this *strengthens a
sound consequence relation* (only adds entailments).

### NOT sound by construction

A "sound by construction" argument has been refuted **4×** this session (increment-3,
snapshot cache, precise backjump, det-pruning). This look-ahead re-saturates a node
label that may carry **branch-dependent** synthetic markers (NomKey for branch-merged
individuals, etc.); the saturator's TBox-level ⊥ is a genuine entailment over the
*TBox*, but whether it remains a genuine entailment for a node carrying branch-merged
nominals is exactly the nominal-context hole that killed det-pruning. **The gate is the
wine corpus oracle, run FIRST — not a soundness argument.** pizza/bibtex FP=0 is not
evidence; wine itself is.

## Architecture — three units

### Unit 1 — seed-saturation engine (`owl-dl-saturation`)

Refactor the saturator into a node-independent **base** built once + a cheap
**per-seed unsat query**:

- `build_base(internal: &InternalOntology) -> SeedSaturator` — runs the existing
  `saturate` machinery once and retains the working state (told tables, marker
  indices, derived closure) needed to extend it.
- `SeedSaturator::seed_unsat(&self, seed: &[ConceptId]) -> bool` — introduce a fresh
  synthetic class `X` with `X ⊑ sᵢ` for each `s ∈ seed` (lowered through the
  saturator's existing `atomic_or_tseitin_body` chokepoint so ∃/compound seeds reuse
  the Tseitin path and connect to the existing marker closure), run the consequence
  rules from `X` to fixpoint over a **clone** of the base working state, and return
  whether `X ⊑ ⊥` was derived. The clone-per-call is naive but acceptable: **the gate
  measures branch counts, not wall** (incremental seeding is the perf engineering done
  *only if* branches collapse).

Engine choice (the design fork, resolved): use `owl-dl-saturation`, **not**
`abox_saturation`. `abox_saturation` is readier to seed-saturate but carries only
domain/range/functional/disjoint + the functional ∃-merge clash — it lacks the
`ForallKey` (∀R.OneOf) reasoning wine's color/sugar ∀-partitions need. The full
marker saturator is the one proven complete-in-output on wine.

### Unit 2 — ⊔ failed-literal hook (`owl-dl-tableau::hyper`)

Behind `RUSTDL_SAT_LOOKAHEAD` (default OFF; flag-OFF path byte-identical). At the
**MRV-chosen** open disjunction, for each disjunct `Dₖ`:
- collect the seed = the node's current label restricted to the atomic-class +
  `∃R.C`-marker subset the saturator consumes, **plus** `Dₖ`;
- `seed_unsat(seed)` → if `true`, `Dₖ` is a **failed literal**: drop it from this
  branch's disjunct set.

Then: 0 survivors ⟹ the node clashes (no branch); 1 survivor ⟹ forced (no choice);
≥2 ⟹ branch the survivors. **The drop is branch-scoped** — recomputed at each ⊔
visit, never global — so on backtrack a dropped disjunct is reconsidered.

**Soundness direction of the drop, under-seeding:** restricting the seed to the
atomic + ∃-marker subset is a sound *under*-approximation for the drop — fewer
constraints ⟹ the saturator derives ⊥ *less* often ⟹ we drop *fewer* disjuncts ⟹ we
never wrongly drop on account of *missing* a constraint. The residual FP risk is the
nominal-context one above (a derived ⊥ that is branch-dependent), which the wine FP
gate is designed to catch.

Instrument: `SearchStats` branches/restores (existing) + new counters
`lookahead_dropped`, `lookahead_forced_single`, `lookahead_calls`.

### Unit 3 — gate harness + protocol

Reuse the existing `sat_class_probe` / `decide_pair_probe` big-stack harness
(adaptive-budget OFF, depth cap as prior gates).

1. **Branch collapse (measure branches, ignore wall):** `sat(SweetWine)` and
   `sat(AlsatianWine ⊓ ¬AmericanWine)`, look-ahead ON vs OFF. **The OFF baseline is
   already MRV-on** (MRV is default-ON): Alsatian ≈ 1227 branches, SweetWine ≈ 12 366
   branches. So this gate measures look-ahead ON *against the MRV baseline*, not the
   66 k raw. Record branches, restores, dropped, forced_single, and the verdict
   (Sat/Unsat/DNF). A collapse to *spurious Unsat* is **not** a win — sanity-check the
   verdict stays Sat.
2. **Wine FP=0 — run FIRST, before declaring any GO:** `konclude_closure_diff` on
   wine with look-ahead ON, tight per-pair deadline (≈25 ms — spurious subsumptions
   complete fast, reproducing FP signals quickly as in prior gates). Require
   `rustdl_closure = konclude_closure = 653`, FP=0, MISSED=0.
3. **Flag-OFF byte-identical:** confirm the OFF path is unchanged vs the integration
   branch base (full `owl-dl-tableau` suite + a wine closure spot-check).

### Verdicts (pre-committed)

- **GO** — an order-of-magnitude collapse *below the MRV baseline* (SweetWine
  ≈ 12 366 → low hundreds or fewer; Alsatian ≈ 1227 → tens or fewer) **AND** wine
  FP=0/MISSED=0. → the Konclude-class path; the engineering follow-on is incremental
  seed-saturation (drop the clone-per-call) + promoting the look-ahead to a real wedge
  feature.
- **FLOOR** — branches stay in the thousands (SweetWine within ~2× of the MRV
  baseline). → wine's nondeterminism is irreducible
  even under our *strongest sound relation*; a rigorous, documented limit → commit
  to SP-B (closure-only classification), since Konclude-class is mandatory.
- **UNSOUND** — wine FP > 0. → the branch-dependent-nominal-context hole (same place
  det-pruning died). Record the reproduction; → SP-B. (Optional one-shot retry:
  restrict the seed further to provably-non-branch-dependent markers; if still FP,
  the lever is dead.)

## Scope / non-goals

- **Throwaway-allowed:** the Unit-2 hook and Unit-3 harness are spike code; only the
  verdict doc `docs/marker-saturator-lookahead-gate-results-2026-06-25.md` is durable.
  The Unit-1 `SeedSaturator` refactor is the one piece worth keeping on GO (it is the
  foundation of the engineering follow-on) but is not merged by this gate.
- **No incremental saturation** in the gate (naive clone-per-call). That is the
  follow-on, gated on GO.
- **No default change.** `RUSTDL_SAT_LOOKAHEAD` stays OFF; main stays pristine; the
  branch is off `feat/build-once-redesign`.

## Testing

- Unit 1: a synthetic `seed_unsat` test — two told-disjoint atomic seeds ⟹ true; two
  compatible seeds ⟹ false; a `ForallKey`-driven clash (`∃R.{a}` + `∀R.K`, `a∉K`) ⟹
  true (the relation horn_fixpoint missed).
- Unit 2: a unit test that look-ahead-ON drops a provably-dead disjunct on a small
  synthetic node while OFF branches it; verdict ON == OFF on a SAT control.
- Unit 3: the gate harness itself (the measurement), plus the flag-OFF
  byte-identical check.

## Global constraints

- FP=0 is sacred; the **wine** closure-diff (run first) is the proof, not any
  by-construction argument.
- `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets --all-features
  -- -D warnings` (pedantic) clean; `cargo test --workspace` green (flag-OFF).
- Toolchain: `RUSTUP_HOME=/home/dumontier/.rustup CARGO_HOME=/home/dumontier/.cargo`
  + stable bin on PATH.
- Commit only when asked; trailers `Co-Authored-By: Claude Opus 4.8
  <noreply@anthropic.com>` + `Claude-Session:
  https://claude.ai/code/session_01HSzon7V2wkhrudxBNAJduh`.
- Branch `feat/marker-saturator-lookahead-gate` off `feat/build-once-redesign`.
