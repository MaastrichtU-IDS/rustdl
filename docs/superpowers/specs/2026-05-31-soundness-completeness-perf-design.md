# Design — soundness, completeness, and performance plan (2026-05-31)

Status: approved design, pre-implementation-plan.
Companion to `docs/handoff-2026-05-30.md` (engine state), `docs/hypertableau-summary.md`
(capstone), `docs/hypertableau-dead-ends.md` (failure log), and
`docs/architecture-roadmap.md` (perf levers, partly superseded now that the
hypertableau wedge is the default accelerator).

## Goal

A balanced, payoff-ranked plan that pushes the default classify path on all
three axes — soundness, completeness, performance — with every phase ending in
a **measurable diff** (pass/revert per `moms-plan.md` §A). The soundness axis
specifically aims to **earn the default-on** state of `trust_sat`: today it is
fast and 0-FP on the validated corpus, but it trusts `Sat` verdicts on *any*
ontology, where soundness is only empirically established on ~8 ontologies.

## Current state (the starting line)

- **Defaults ON** since 2026-05-29: `RUSTDL_HYPERTABLEAU`,
  `RUSTDL_HYPER_DOUBLE_BLOCK`, `RUSTDL_HYPERTABLEAU_TRUST_SAT` all
  `map_or(true, …)` in `reasoner/src/lib.rs`.
- **Sound** (0 FP vs Konclude) on every measured ontology. The historical SIO
  38-FP cluster was a 4-line saturation bug (`process_fact` range propagation),
  fixed 2026-05-28 — **not** a blocking bug.
- **Completeness gaps:** GALEN ~109 MISSED, notgalen ~27, SIO 2.
- **Perf:** the saturator is now the bottleneck (SIO 68 s, notgalen 10 min are
  saturation-dominated, not tableau); a known 4× Or-body trigger regression
  (`fddf2ee`) is unoptimized.

## Three framing facts (these drive the sequence)

1. **The verification lever is the keystone.** Tableau-verifying the wedge's
   *fast* `NotSubsumed` verdicts is simultaneously the biggest completeness win
   *and* a soundness improvement (it stops blind-trusting `Sat`).

2. **"Earn the default-on" is NOT a double-blocking audit.** Dead-end #12 is
   decisive: full HF2 double-blocking left the SIO FPs *unchanged*; the only FP
   ever found was the saturation bug, caught by **broadening the corpus** and
   **changing the experimental frame** (`--saturation-only`). Double-blocking is
   shipped theoretical insurance that was never the proven culprit — auditing it
   is low value. The soundness deliverable is **empirical breadth** +
   **fragment characterization**.

3. **Not all MISSED are cheaply recoverable.** GALEN ~109 / notgalen ~27 are
   mostly "trust_sat skipped a check the tableau could answer" → recoverable by
   the verification lever. SIO 2 and the deep GALEN functional-role pattern are
   genuine calculus gaps where even the full tableau times out (SIO_010092 did
   not finish in 3+ hrs with `RUSTDL_HYPERTABLEAU=0`). **The plan does not
   promise 100% on SIO.**

## Execution order (P3 before P2, per approval)

Phases are numbered by **execution order**. Each ends in a measurable diff.

### Phase 0 — Soundness net + fragment characterization

The real "earn the default-on." Built first because it is the gate every
subsequent change is measured against.

- Pull more ORE 2015 ontologies, weighted toward **inverse + cardinality +
  role-hierarchy** (the interaction that historically produced FPs). Wire them
  all into `crates/owl-dl-reasoner/tests/konclude_closure_diff.rs`; gate in CI.
- Write the **fragment-completeness statement**: precisely which fragment the
  hyper engine is *provably* complete on (Horn + EL is provable; full
  SROIQ-with-current-rules is "verified by composition," not proven). Since
  `trust_sat` is sound **iff** the engine is complete on the workload, this
  statement *is* the proof that earns default-on, and is shared groundwork for a
  future auto-gate (Phase 4). Landed as [`docs/fragment-completeness.md`](../../fragment-completeness.md).
- **Measurable:** N new ontologies diffed at 0 FP (or a new bug found — also a
  win); a written fragment statement checked into `docs/`.

### Phase 1 — Selective trust-sat verification (keystone)

Serves completeness *and* soundness; mechanism is ~90% wired.

- **Policy:** when the wedge returns `NotSubsumed` **fast** (threshold to be
  tuned, e.g. <50 ms), do not trust it — tableau-verify. A 5 ms `NotSubsumed` is
  more likely "didn't try hard enough" than a genuine model.
- **Hard constraint — candidate filtering is an explicit, measured deliverable,
  not an afterthought.** The unfiltered defined-sup sweep with `trust_sat=false`
  exceeded 8000 CPU-min (dead-end #3). Restrict verification to
  structurally-likely sups (told-subsumer neighborhood / defined-sup
  candidates); never n². The per-call `trust_sat` override on
  `subsumes_via_tableau` already exists (`b8d8695`); only the policy is missing.
- **Measurable:** GALEN MISSED 109 → target ~20–40, wall +1–3 min (not +hours);
  notgalen 27 → comparable; 0 FP held by the Phase 0 net.

### Phase 2 — Deep completeness calculus (was P3)

The genuine calculus gaps. Verify-before-build: run the canary before building
each.

- **2a — Functional-role inference in the saturator (EL++ rule).** For a
  functional role `R` with sub-properties `R_i ⊑ R`: when `X` has `∃R_i.A` and
  `∃R_j.B` for distinct `R_i, R_j ⊑ R`, the witnesses coincide, so
  `X ⊑ ∃R.(A ⊓ B)`. Targets GALEN's `<Region>Pathology` /
  `PathologicalCondition` cluster (~50–80 of 109).
  Landed: `docs/phase2a-results.md`. **Outcome contrary to estimate:**
  the rule is sound and terminating, but recovers 0 of GALEN's 109
  MISSED. The handoff's PathologicalCondition trace did not describe
  what's actually missing in GALEN. Phase 2b's first deliverable now
  shifts to re-diagnosing GALEN MISSED before any rule design.
- **2b (revised after 2b.0 diagnosis) — fix compound existential-body lowering.**
  The saturator's compound-body lowering misses the nested `∃R.(B ⊓ ∃S.C)`
  shape. Calculus already documented; this is an implementation gap. Target:
  ~60 of 109 GALEN MISSED. The original ≥n + disjointness target is
  empirically inapplicable on GALEN (zero cardinality / zero disjointness
  axioms in the sampled minimal modules); preserved here as historical record:
  ~~≥n + disjointness for PairedBodyStructure cluster (~20–30)~~.
  - **2b.0 — Re-diagnose GALEN MISSED.** Before any rule design,
    extract the concrete (sub, super) pairs from the corpus-diff
    harness output and walk the GALEN axioms to identify the actual
    missing derivation step. Phase 2a's empirical falsification of
    the handoff's trace makes this gating work — without it, 2b risks
    the same outcome.
    Landed: `docs/phase2b-galen-diagnosis.md`. Headline: 6 of 8 sampled
    pairs (~60 of 109) need a fix to compound LHS/RHS existential-body
    lowering (calculus already documented; implementation gap, not new
    calculus); 2 of 8 (~24) need functional-role + disjointness propagation
    (extension of Phase 2a). The spec's named ≥n + disjointness lever is
    EMPIRICALLY INAPPLICABLE (zero cardinality + zero disjointness axioms
    in any sampled minimal module). Phase 2b proper plan is reordered:
    main = bug fix; extension = functional-role refinement.
  Landed: `docs/phase2b-results.md`. GALEN MISSED 109 → 17 (92 recovered,
  84%, FP=0). Combined fix landed in two commits: P2b body-side
  (022ca50) + P2b.5 LHS-And-RHS-existential (b64d331); the P2b.5 was
  needed after the original P2b alone recovered only 5/60, with a
  re-trace identifying the actual bail-out one level upstream from
  the P2b.0 diagnosis's pointer. notgalen unchanged (27→27) — those
  are cluster C/D, needing the functional-role + covering extension
  plan.
  **Phase 2 closes (2026-06-01).** Final state: 92 of 136 GALEN+notgalen
  MISSED recovered (~68%), FP=0 held. 44 residual MISSED in cluster C/D
  shape (functional-role + covering / sibling-collapse). The per-pair
  analysis (`docs/phase2b-galen-pair-analysis.md` pairs 06/07) lists an
  EL+ approximation option for cluster C/D, so this is "tractable but
  lower-priority than Phase 3 perf," not "outside scope." Phase 2c
  (cluster C/D) queued for after Phase 3. See `docs/phase2-closeout.md`.
  **Phase 2c.0 landed (2026-06-01):** `docs/phase2c-galen-diagnosis.md`
  + `docs/phase2c-cluster-shift.md`. Confirmed the 17 GALEN + 27 notgalen
  residual MISSED predominantly map to Phase 2b.0's cluster C
  (functional-role + covering / sibling-collapse): 24-pair confident
  floor (12 GALEN + 12 notgalen sharing `IntrinsicallyPathologicalBodyProcess`),
  15 anonymous-notgalen middle, 5 GALEN F-tail body-structure (top end =
  44/44). No genuinely new shapes vs Phase 2b.0 — F-tail body-structure
  pairs were always in the original 109 but unsampled. Phase 2c proper
  targets Option 3 EL+ approximation: pattern-match the triangle
  (`R_i ⊑ R_f` functional + `∃R_i.X` + covering on the R_f-target range)
  in absorbed-TBox shape, lower to materialised existential.
  Hypertableau-extension Options 1 and 2 deferred.
  Phase 2c shipped, measured, and reverted: see `docs/phase2c-results.md`.
  0 / 44 predicted corpus pairs recovered; rule sound but cannot reach the
  IPBP-derivation cluster because the saturator propagates subsumers not
  facts to subclasses. Architectural prerequisite (fact-on-subclass
  propagation) deferred to a potential Phase 2d.
- **Honesty:** SIO 2 and some deep GALEN pairs may still not converge — recorded
  as residual gaps, not a deliverable.
- **Measurable:** per-lever MISSED reduction via the corpus diff.

### Phase 3 — Saturator performance (was P2)

Independent of the completeness phases — verification tractability rides on
filtering, not saturator speed, so this is genuinely swappable and was deferred
per approval.

- Flamegraph the saturator (SIO 68 s / notgalen 10 min are saturation-dominated).
- Fix the known 4× Or-body trigger regression (`fddf2ee`).
- **Measurable:** SIO + notgalen wall reductions; verdicts unchanged.

Landed (first fix): `docs/phase3-results.md`. Empirical Phase 3
target was the TABLEAU, not the EL saturator — both GALEN and SIO
flamegraphs showed the saturator at <1%; the bottleneck is in
`apply_deferred_concept_or_rules` (GALEN) and `apply_max` (SIO).
First Phase 3 fix targeted GALEN's `needs_deferred_or` via bloom
prefilter; result: 24.7 min → 21.1 min (−14.6%), FP=0 + MISSED=17
held. Phase 3b queued for `apply_max` (helps both GALEN and SIO),
followed by clash detection (Phase 3c) and heap allocations
(Phase 3d).
Phase 3b landed: `docs/phase3b-results.md`. Flamegraph evidence —
`are_declared_inverses` linear scan 25.76% → 3.44% via hashbrown::HashSet
swap; `apply_max` 27.93% → 6.51% as a result. FP=0 + MISSED-unchanged
held. Wall improvements uncalibrated (shared-CPU contention this
session); flamegraph is the durable comparison. Phase 3c queued
for the new top non-search frame: `apply_role_axioms` / `bot_id` linear
scan at 24.66%.
Phase 3c landed: `docs/phase3c-results.md`. `ConceptPool::bot_id`
cached via `OnceLock` (concurrency-safe; `ConceptPool` is Sync across
rayon workers); `apply_role_axioms` / `bot_id` / `find_map` cluster
24.66% → 0.45% (eliminated). **GALEN wall 24.8 min → 12.2 min —
Phase 3 (a + b + c) has now reclaimed Phase 2b's entire wall regression
while keeping the 92-pair MISSED recovery. GALEN is back to the pre-2b
baseline of 12.5 min.** FP=0 + MISSED=17 unchanged. Phase 3d/3e queued
for `apply_deferred_concept_or_rules` (18.16% remaining) and
`apply_role_rules` (16.36% new top-5); these are lower priority now
that GALEN wall is recovered.

### Phase 4 — Generalization capstone

With the broad corpus (P0), the verification net (P1), and the fragment
statement (P0), make default-on *defensible*: gate `trust_sat` by the fragment
check (auto-enable only where provably complete) or a runtime agreement-check
when a reference classification exists.

## Out of scope (explicit)

- Double-blocking audit (low value — never the proven culprit; see framing #2).
- Datatypes / concrete domains (orthogonal coverage).
- Full role-chain automata (orthogonal coverage).
- Deep model caching (measured dead for pizza/SIO — `architecture-roadmap.md`
  §"per-class sat(A) does NOT converge").

## Cross-cutting discipline

- Every phase ends in a pass/revert measurable diff (`moms-plan.md` §A).
- The Konclude corpus diff is the soundness net — canaries are necessary but not
  sufficient (dead-end #4: label-only dep-sets passed every hand test, corpus
  caught the 58 FPs).
- Run the canary before building (verify-before-build: HF3b/c/HF4b were achieved
  by composition, not built).
- Measurement before shipping any "this should help" (dead-ends #1, #5, #7, #10).

## Acceptance criteria for the plan

Not a deliverable itself — declares direction. Each phase lands with its own
measured diff and, where it changes the calculus or caches, a corpus-or-larger
soundness diff before the change is trusted.
