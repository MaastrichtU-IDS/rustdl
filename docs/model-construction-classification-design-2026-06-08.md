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

### PHASE 1 RESULT 2026-06-08 — B-perf is DEAD (told-bracket = 0%); posture (i) forced

Measured (positive-control-verified — told tables populated & queried correctly;
self-check `p1_pass_through_total == label_cache_pass_through`):

| ontology | pass_through | told-decided | % |
|---|---:|---:|---:|
| pizza | 137 | 0 | 0 % |
| ore-10908 | 0 | 0 | — |
| ore-15672 | 6 | 0 | 0 % |
| sio | 197 | 0 | 0 % |
| wine | 12 | 0 | 0 % |

The told-bracket eliminates **zero** tests beyond the shipped oracle+closure
(told-sub ⊆ closure, caught earlier; told-disjoint ⟹ `D∉labels`, pruned earlier).
**B-perf is marginal.** With the bracket dead, model-sharing's only justification
(test reduction) is also gone — and it carries A1-shaped risk. So **all of B-perf
is off the table.** The only non-marginal B content is wine. (Told access path
for the record: `prepared.told.{is_told_sub, are_told_disjoint}`, built in
`PreparedOntology::from_internal`, lib.rs:1811.)

### CORRECTION — "B-complete = terminating model construction" is WRONG. It's search.

The §2/§4 phrasing "terminating model construction on nominals" mischaracterizes
the wine wall, and the earlier measurement (`perf-attribution-2026-06-07.md`,
`wine_wedge_*_probe`) refutes it: on wine's hard pairs the model is **finite and
tiny — `node_count` stays at 10, a single branch completes in 0.0 ms, `is_blocked`
fires 0×**. There is nothing to terminate and nothing to block. The wall is a
**per-pair backtracking SEARCH explosion** (168k branches over disjunction+`≤n`,
`restores==branches`) trying to *find* the satisfying model — and the known search
levers are **dead** (1-UIP NO-GO, backjump dist≈1; MOMS reverted; simple learning
0-un-stalled).

So B-complete is: **find wine's (provably small) satisfying models without the
per-pair backtracking explosion HermiT/Konclude somehow avoid** — and *how* they
avoid it is **NOT characterized**. It is not the told-bracket (0%), not blocking
(model is tiny), not the dead search levers. rustdl already matches HermiT's
*global pruning* (label oracle prunes 96–100%, = HermiT's possible-subsumers); the
gap is the *per-candidate test efficiency* on the residual `D∈labels` candidates,
where HermiT's model construction is fast and rustdl's backtracking detonates.

**The genuine prerequisite (NOT a build): characterize WHY HermiT/Konclude
construct wine's models efficiently** where rustdl's per-pair wedge explodes —
literature deep-read (HermiT's deterministic-vs-nondeterministic rule ordering,
its model-merging across the classification, Konclude's saturation-driven
completion) and, if feasible, profiling/observing HermiT on a wine pass_through
pair. Until that mechanism is named, there is nothing concrete to build — the
"architectural rewrite" has no defined mechanism yet, only a target.

### CHARACTERIZATION RESULT 2026-06-08 — mechanism NAMED; B-complete is ≤n-reform, not greenfield

Literature dig (HermiT JAIR 36/2009; Konclude/saturation-coupling JAIR 54/2015;
Konclude JWS 27/2014), cross-checked against `hyper.rs`. The wine explosion is
**not a missing big mechanism** — rustdl already does Horn-first hyperresolution,
restricted *semantic* branching (`search.rs` — do NOT "add semantic branching"),
dep-directed backjumping, and global label-oracle pruning. The cause is **how the
two nondeterminisms compose**:

- **M1 (most likely dominant) — `≤n` partition-enumeration is nested under
  disjunction branching and re-run per disjunction branch.** `solve` branches on
  `find_open_disjunction`; deeper in the recursion `find_open_at_most` →
  `solve_at_most`/`partition_rec` (hyper.rs:1330/1544/1564) enumerate
  restricted-growth partitions of the successors and recurse `solve` *inside each
  partition* — so the ~60k merge-branches are re-enumerated within each of the
  ~108k disjunction branches (the JAIR §3.1 And/Or-branching interaction). HermiT
  encodes at-most as a **DL-clause with a disjunction-of-equalities head over the
  concrete present successors** (JAIR §3.1.1/§4.1), discharged by *unit
  propagation* — merges are **forced** once all-but-one equality disjunct is
  refuted, not enumerated. **Change: deep but bounded** — replace the
  partition-enumeration discharge of `≤n` with clause-head resolution in the same
  hyperresolution loop the disjunction path already uses. (The clausifier already
  produces `AtMost` heads; it's the *solver* discharge that must change.) Also
  fixes the `clash_deps = DepSet::ALL` (hyper.rs:1553) backjump-defeat as a
  side-effect.
- **M2 (the real content of "`with_nominals` didn't help") — nominal identity
  must FORCE merges before partitioning.** rustdl's `apply_nn_rule` (hyper.rs:931)
  merges nodes already carrying the same nominal *label*; but wine's nominal-
  filler successors (`∃R.{a}`, `≤1 R`) don't carry the literal nominal label at
  partition time, so `must_be_distinct` (1604) is false and the full partition
  fan-out runs. HermiT's **NI-rule** (JAIR §3.2, annotated equalities) and
  Konclude's **`C°≤`-rule + `#mcands`** (JAIR'15 §3.1) force co-nominal successors
  equal so `#mcands ≤ m` and the node is never "critical" → no branching merge.
  **Change: medium** — eagerly merge/force co-nominal-filler successors (propagate
  the nominal guard) so the merge-candidate count reflects identity *before*
  `partition_rec`.
- **M3 (deepest, optional) — Konclude's deterministic/non-deterministic
  saturation-graph split + critical-node "patching"** resolves many residual
  ("critical") pairs without a full tableau test. rustdl's label oracle already
  matches Konclude's *global* pruning; this adds the *patching loop* that shrinks
  the critical set. New subsystem; highest-effort/highest-payoff.
- **Backjumping confirmed a dead fallback** (our 1-UIP dist≈1 + the `DepSet::ALL`
  site): the thesis is *generate fewer branches* (M1/M2), not prune better.

**The open question that picks the first increment (NOT yet measured):** is M1 or
M2 primary on wine? The `with_nominals`-didn't-help datum *leans M1* (collapsing
merges can't dent a disjunction-dominated total). **Decisive next measurement:
instrument `#mcands` (how many `≤n` successors are *actually* merge-eligible vs
forced-distinct) on the real wine completion graph.** Small `#mcands` but rustdl
still branches → M2 (force the merges). Genuinely large fan-out independent of
merges → M1 (clause-head encoding). That result picks the first engine increment.

**So B-complete = reform the wedge's `≤n` cardinality discharge (M1) + eager
nominal-filler merge (M2), in `hyper.rs` — a deep but BOUNDED change to
`solve`/`solve_at_most`/`partition_rec`/`apply_nn_rule`, NOT a new engine.** Every
increment FP=0 + MISSED=0 corpus-gated (the `≤n` discharge is soundness-critical).
Sources + exact sites in the characterization agent's report (banked below).

## §7 Implementation plan (TDD-first) — the reform

The first increment is picked by the `#mcands` measurement (running 2026-06-08).
The scaffolding below applies to whichever (M1/M2) wins.

**Soundness obligations (the `≤n` discharge is the most soundness-critical site
in the engine — treat as such):**
- **Verdict-equivalence**, not just FP=0: the new discharge must produce the SAME
  Sat/Unsat verdict as the current partition-enumeration on every input (the
  reachable-partitions/models set is identical; only order-redundancy + forced
  merges change — same argument as the existing `solve_at_most` canonical-
  partition comment). A change that flips a verdict either way is a regression.
- **FP=0 AND MISSED=0**, full corpus closure-diff vs HermiT/Konclude
  (pizza/ore-10908/ore-15672/sio/wine + GALEN/notgalen sanity), every increment.
  MISSED now has teeth (§0.3) — a too-aggressive forced-merge that drops a model
  is a MISSED, not just slower.
- Preserve the `precise_card_deps_preserves_{unsat,sat}_verdict` tests
  (owl-dl-tableau) as the regression baseline; the `DepSet::ALL` at
  `solve_at_most:1553` should *improve* (narrow) under M1, not regress.

**TDD order (project discipline — canaries FIRST):**
1. Synthetic canaries with known verdicts BEFORE touching the engine: `≤n` alone,
   `≤n` + nominal fillers (`≤1 R` + `∃R.{a}` + `∃R.{b}` with a,b distinct →
   clash; a,b same → merge), `≤n` nested under a disjunction (the wine shape),
   plus **soundness NEGATIVES** (cases that must stay Sat / must stay Unsat).
2. The verdict-equivalence harness: random-ish `≤n` instances decided by BOTH the
   old partition path and the new discharge; assert identical verdicts (keep the
   old path behind a flag during bring-up for A/B).
3. Implement; gate on canaries → verdict-equivalence → full corpus FP=0/MISSED=0
   → the wine-wall *wall* measurement (does the 168k-branch explosion actually
   collapse? — the whole point).

**M2 branch (if `#mcands` says merges are forceable — medium):** at successor
creation for nominal-filler existentials and at `≤n` solve time, eagerly merge
co-nominal-filler successors / propagate the nominal guard so `must_be_distinct`
+ the merge-candidate count reflect individual identity *before* `partition_rec`.
Sound because co-nominal successors *denote the same individual* (forced, not
chosen). Sites: successor creation (`fire_exists`/`fire_at_least`), `apply_nn_rule`
(:931), `must_be_distinct` (:1604). Lower risk; may be only a partial win if M1
is also primary.

**M1 branch (if disjunction fan-out is independent of `≤n` — deep):** replace
`solve_at_most`/`partition_rec`'s partition-enumeration discharge with HermiT's
encoding — a disjunction-of-equalities clause head over the *concrete present*
successors, fed into the SAME hyperresolution + semantic-branching path the
disjunction rule already uses, so equalities are *unit-propagated* (forced once
all-but-one disjunct is refuted) instead of enumerated. The clausifier already
emits `AtMost` heads; the change is the solver discharge + wiring the `≈`/merge as
a clause consequence. Fixes the `DepSet::ALL` backjump-defeat as a side effect.
Multi-session; the single most soundness-delicate change in the codebase — do it
behind an A/B flag with the verdict-equivalence harness live throughout.

**Session-honesty note:** the M1 implementation is *not* a session-tail change —
a `≤n`-discharge rewrite rushed while fatigued is precisely how an FP ships. The
right cadence: pick the increment (`#mcands`), land the canaries + equivalence
harness, then implement in a focused, corpus-gated session.

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
