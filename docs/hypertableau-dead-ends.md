# Hypertableau dead-ends

Companion to [`hypertableau-summary.md`](hypertableau-summary.md). Each
entry is a path **that looked right** at decision time, was tried, and
was killed by measurement or grounding. Read this **before** picking up
the next phase — most of these are tempting on first principles, and
each one cost real time before being refuted. The pattern across them:
*measurement, not intuition, decided.*

If you find yourself drawn to one of these, re-read its "what killed
it" — that's where the time vanishes.

---

## 1. Node-granularity semi-naive worklist

**What was tried.** First cut of semi-naive Horn evaluation: a dirty-
*node* worklist, where re-firing a node's trigger-present clauses on
each re-dirty was supposed to prune fixpoint cost.

**What killed it.** Counter measurement: `match_attempts` went **52M
→ 57M** (a regression). The cause: class nodes gain many labels over
the run, and each new label re-dirties the node and re-fires all its
trigger-present clauses — same cost, more work.

**The lesson.** Per-node completeness ≠ per-event pruning. The
event-granularity model (`Label(n,c) / Edge / NodeNew` events firing
*only* their newly-enabled clauses) is what actually prunes — 52M →
2M (26×). Pre-commit any new fixpoint scheme to a counter measurement
*before* declaring it an optimization.

---

## 2. Sub-model caching across sups

**What was tried (was tempting, never built).** When the pizza probe
showed `SpicyPizza ⊑ X` taking 167,968 branches per pair across many
unrelated `X` sups, the obvious optimization looked like: compute
`SpicyPizza`-Sat *once*, test each sup against the cached model.

**What killed it.** Soundness trap. Any sup that propagates back into
the sub's model (`∀R⁻` / nominal coupling / cardinality) invalidates
the cache. Detecting that reliably is the problem the top-down
classifier already solves via hierarchy dispatch. Caching the model
without invalidation gives silent false negatives.

**The lesson.** The branch-count smoking-gun (identical 167,968
branches across unrelated sups) *looks like* recomputation; it's
actually **no backjumping / no learning** — the `¬sup` never cuts a
branch, so the engine re-walks the same dead sub-tree every pair.
Backjumping fixed it (pizza probe 4:44 → 13.2 s, ~21×). Caching would
have shipped an unsound classifier with the same speedup.

---

## 3. `n²`-within-tier sweep for same-tier inferred subsumptions

**What was tried.** The top-down classifier misses same-tier
subsumptions (closure seed only catches EL-derived ones). The obvious
fix: after each tier completes, test every same-tier pair through the
wedge.

**What killed it.** Measurement: pizza classify with `n²` sweep took
**> 24 min wall** (vs HF5 baseline of 21 s). Pizza tiers are large
enough × the slow wedge tail (a few SpicyPizza-style 2–5 s pairs) ×
many tiers → multiplicative blowup. Killed by `kill -9` rather than
shipping.

**The lesson.** Bounded effort ≠ bounded *expected* cost — when the
wedge has any 2–5 s tail, multiplying it by tier-sized `n²` blows up
fast. The replacement, **defined-sup × all-classes**, cuts the cost
1485 pairs × parallel × 200 ms cap → 0.5 s overhead, and catches *all*
15 missed pairs on pizza. The takeaway: candidates ⊆ "structurally-
likely sups" beats "every same-tier pair," even when "every pair" is
the more general framing.

---

## 4. Label-only dep-sets for backjumping

**What was tried.** First-cut dependency-directed backjumping tracked
decision-level dep-sets per *label*: when a label is asserted under a
decision, the label carries that decision in its dep-set; clashes
union body atoms' dep-sets; backjump when the current decision isn't
in the clash deps.

**What killed it.** *Pizza dropped 695 → 753 with 58 false positives*
(`Topping ⊑ VegetarianPizza` shapes — unsound backjumps). The cause:
a clause matching a successor via a role atom (`R(x,y) → D(x)`,
domain-style) depends on that successor *existing*. The successor was
created under a decision; the clause body has no class atom on `y`,
so label-only deps missed the decision dependency → under-counted
deps → backjumped past a real dependency → false `Unsat`.

**Crucially: every hand-built engine test PASSED.** The corpus diff
was the only thing that caught it.

**The lesson.** Dep-set tracking must capture **every** source of
dependence, including node existence. The fix was a per-node
`birth_deps` (the decision-set under which the node was created),
unioned at every clause-body match site. And: **the corpus diff is the
soundness net for dependency propagation** — canaries are necessary
but not sufficient.

---

## 5. ⊤-internalization for hard-antecedent GCIs

**What was tried.** HF1's first cut for "antecedent with no body-
encodable trigger" (e.g. `∀R.C ⊑ D`): internalize as `⊤ ⊑ ¬sub ⊔ sup`
— the textbook NNF approach.

**What killed it.** Measurement: SIO hyper-sat went from **0.45 s →
did-not-finish**. The `⊤`-headed clauses fire at *every* node, and
`¬∀ → ∃` generates successors *everywhere*. SIO's ~10 such axioms
multiplied combinatorially. Killed before commit.

**The lesson.** "Trigger-less" clauses are a pathological shape: the
fixpoint can't prune them. Partial absorption — splitting the
antecedent into a *soft* (body-encodable, fires only when triggered)
and *hard* (NNF'd into the head) part — emits `A(x) → ∃R.¬C ⊔ D`,
*triggered* by `A`, so it doesn't fire everywhere. Almost-hard
residue (no soft trigger at all) is rare enough (~2 axioms on ro)
that `⊤`-internalization is fine for that fallback at that scale.

---

## 6. Declared-equivalence sweep for orchestrator misses

**What was tried.** The top-down classifier misses same-tier
equivalences (closure can't bridge them). The obvious cheap fix: for
each `EquivalentClasses(A, B)` axiom in the ontology, mark A and B
mutually subsumed in `direct_supers` — no probes, the axiom is the
proof.

**What killed it.** *Caught nothing on pizza.* Inspection revealed
pizza's equivalences are all of the shape `EquivalentClasses(Name,
ComplexExpression)` — never name-to-name. The 8 inferred-equivalence
misses (`SpicyPizza ↔ SpicyPizzaEquivalent`, etc.) come from
*structurally-equivalent complex definitions* that Konclude proves
equi-sat. There are no atomic name-pairs to capture.

**The lesson.** The "declared equivalence" framing assumes a syntactic
shape that real ontologies don't use. The actual fix (defined-sup
sweep) targets the *consequent* of the equivalence (the named class
on the LHS of a complex-RHS definition), and runs the wedge —
inferred-equivalence is recovered automatically when both directions
of subsumption are tested. Reading the ontology beats reasoning about
its syntactic shape from theory.

---

## 7. Fire-once-only `≥n` generation guard

**What was tried.** First cut of HF3a `≥n` generation: gate it by a
per-node "already fired" flag, so the rule can't loop on cyclic
ontologies. Tested with three crafted canaries: all green.

**What killed it.** *Pizza dropped 695 → 682 (13 lost).* The probe
header showed `stalled: 15` (was 2). Diagnosis: `InterestingPizza`
already has its `≥3` toppings via `∃`-witnesses; fire-once generated
3 *redundant* fresh successors, ballooning the `≤2` merge tree past
the search budget → 13 pairs that were `Unsat` before became
`Stalled` → treated as "not subsumed."

**The lesson.** A sound rule can regress *completeness* via search-
blowup → budget → `Stalled`. The fix was a **count-based** guard
(skip if `distinct_role_succ ≥ n`) *plus* fire-once. The corpus diff
caught it (canaries didn't).

---

## 8. Chronological "added-after-marker" dep over-approximation

**What was tried (almost).** The advisor initially proposed a
trail-based over-approx for backjumping: a derivation "depends on"
decision `d` iff the atom was added after `d`'s snapshot marker.
Sound (over-approx never under-counts), simpler than per-label
propagation.

**What killed it.** Tracing on the canary: `A8` is *asserted* at
decision level 8, which is chronologically *after* the markers of
d2..d7. The over-approx therefore says the clash `[A1, A8]` depends
on `{1..8}` → contains `d` (current level) → no backjump → blowup
remains. The middle decisions only stay out of the dep-set when we
track *which decision asserted each clash atom*, not chronological
order. (Advisor agreed on reconcile.)

**The lesson.** "Sound but useless" is a real failure mode for
over-approximations. The cheaper representation has to *separate
signal from noise* — chronology mixes them. Per-label dep-sets
propagated through derivation are the price; the canary is the gate
for whether it's worth it.

---

## 9. Model-validation as a path to `Sat`-soundness

**What was tried (research-grade, never built; weighed and rejected).**
After H4 showed the Unsat-only wedge can't move the classify wall,
the idea was: extract a *blocking-aware certificate* from a `Sat`
completion, validate it against the dropped axioms, and trust `Sat`
on success.

**What killed it.** Three soundness obstacles, each independently
fatal:
1. *Cardinality* interacts non-locally with blocking — a blocked
   node's cardinality constraints aren't satisfied at the blocker.
2. *Anywhere blocking is unsound with inverses* — the "completion"
   isn't a model of the inverse-bearing theory to begin with, so
   there's nothing to validate against.
3. *Nominal validation is a merge fixpoint*, not a one-shot check —
   it reproduces the NN-rule.

Each fix to a soundness obstacle pulls in machinery that, taken
together, *reconstitutes the full Motik/Shearer/Horrocks calculus*.
"Model-validation" collapses into "implement the full sound+complete
hypertableau" — i.e. HF1–HF5.

**The lesson.** When a "validation" approach requires fixing every
calculus rule the under-approximating engine drops, it isn't validation
— it's the missing engine. The HF1–HF5 path delivered the same goal
soundly; this dead-end was rejected up front and the time went to HF1
instead.

---

## 10. Aggressive blocking / MOMS-style branching heuristics

**What was tried (memory: [[rustdl-moms-local]]).** Three static
disjunct-ordering experiments (MOMS-style classical SAT heuristic),
applied to the hyper engine's branching.

**What killed it.** **Zero wall change** in all three experiments.
DL tableau disjunctions are node-local — they don't share constraints
the way SAT clauses do, so MOMS' "most-occurring-in-shortest-clauses"
signal doesn't translate. The branching order didn't matter because
the search was bottlenecked elsewhere (the no-learning re-walk
problem, eventually fixed by backjumping).

**The lesson.** Classical SAT heuristics aren't a free pickup for DL
tableaux. The right lever was a different kind of search-quality fix
(dependency-directed backjumping with per-label dep-sets), not a
better branching order. Recognize when an analogy doesn't carry over;
profile first, transplant heuristics later.

---

## 11. Default-on flags before generalization is proven

**What was tried (was tempting, repeatedly resisted).** "Now that the
corpus is at 100 % / 0 FP, just flip the defaults — `RUSTDL_HYPERTABLEAU`
+ `RUSTDL_HYPERTABLEAU_TRUST_SAT` default-on."

**What killed it.** SIO (1585 classes, untested at default-on time)
turned out to produce **38 false positives under trust-Sat** — the
anywhere-blocking-with-inverses interaction the corpus didn't
exercise. Defaulting it on would have shipped silent unsoundness to
every user with an inverse-heavy ontology.

**The lesson.** "Validated on the corpus" ≠ "validated generally."
Opt-in flags are the *design contract* for sound-where-validated —
they keep the per-workload trust decision in the user's hands until a
calculus-level fix (HF2 double-blocking) makes the broader claim sound.
Don't default-on what's not proven beyond the validation set.

---

## 12. Hunting the SIO unsoundness anywhere but where it was

**What was tried (five theories, all wrong).** The 38 SIO FPs under
`RUSTDL_HYPERTABLEAU_TRUST_SAT` were misattributed in sequence:
1. *Anywhere blocking unsound with inverses.* Implemented full HF2
   double-blocking. Corpus sound, ro 11× slower, SIO FPs unchanged.
2. *Label-equality too strict.* Switched to subset pair-blocking
   (Horrocks 1998 / Motik 2009 §3.4). ro 11× recovered, SIO FPs
   unchanged.
3. *Canon-vs-hierarchy namespace mismatch.* `build_role_hierarchy`
   used raw role IDs while clausifier canonicalized them. Fixed
   (real correctness improvement, addressed a TODO). SIO FPs
   unchanged.
4. *Other tableau-side mechanisms.* Various profiling counters
   shipped (`is_blocked_calls`, `block_compares`) — informative for
   ro perf but irrelevant to the FPs.
5. *Maybe the orchestrator's defined-sup sweep.* Tested via
   `classify --saturation-only`: **the FPs persisted in 0.1 s with
   no tableau at all** — locating the bug in EL saturation, not the
   tableau path.

**What killed it (theories 1-5).** Theory 5's measurement
(`--saturation-only` reproduces the FPs at 0.1 s) ruled out everything
above and pointed at `owl-dl-saturation`. The actual bug: `process_fact`
propagated `ObjectPropertyRange(R, C)` to the existential's *target
type* — so `A ⊑ ∃R.B` + `Range(R) = C` derived `B ⊑ C`. Unsound: a `B`
that's nobody's R-successor isn't subject to the range. The unsound
derivation was even **encoded in a passing test**
(`property_range_propagates_to_targets`) — the test was asserting the
bug as a feature. Konclude on the same axiom shape correctly gives
`Dog ⊑ Thing`, `Person ⊑ Thing`, no `Dog ⊑ Person`.

**The fix** was 4 lines of code (remove the `enqueue_subsumer(target,
rng)` block) + invert the test. SIO FP count 38 → 0.

**The lesson.** When first-principles theories about a bug all fail
empirically, **change the experimental frame** rather than crafting
the sixth theory. `classify --saturation-only` was a 30-second test
that would have localized the bug on turn one. The tableau-side
hunting (1-4) cost a session's worth of careful but misdirected work.
Always isolate which *engine layer* is responsible before chasing
specific calculus bugs in that layer.

A milder lesson: **tests that encode their target's behavior as the
expected result are not regression tests, they're regression
amplifiers.** The unsound range propagation passed CI for the entire
session because the test was checking the wrong thing. The Konclude
diff on the corpus *almost* caught it (pizza/ro/sulo didn't trigger
the unsound rule), but SIO did, and it was attributed to the wrong
layer.

---

---

## 13. Wall-time-as-filter for selective trust-sat verification

**What was tried.** Phase 1 (`docs/superpowers/plans/2026-05-31-phase1-selective-trust-sat.md`)
introduced a per-call wall-time threshold on the hyper wedge: if a
`NotSubsumed` verdict took less than `RUSTDL_HYPER_TRUST_SAT_MIN_MS` ms,
distrust it and ask the tableau. The hypothesis (from the handoff): a
fast NotSubsumed is "wedge gave up without trying," and so is worth
verifying; a slow NotSubsumed is the wedge engaging seriously. The spec
estimated 50 ms as a starting default.

**What killed it.** Single-thread sweep on alehif (smallest baseline,
1.76 s, 247 classes, FP=0 / MISSED=0 historically) at thresholds 1, 5,
10, 20, 30 ms returned wall times flat at ~405–410 s — **≈230× the
baseline at every threshold**. This means virtually every wedge
`NotSubsumed` verdict completes in **under 1 ms**: trivially-not-subsumed
and didn't-try-hard-enough verdicts are indistinguishable by stopwatch
at the relevant resolution. Soundness was preserved (FP=0) across the
broadened Phase 0 net at the 50 ms default, but the wall blowup made
GALEN and notgalen unmeasurable, so the "MISSED 109 → ≤ 40" lever
target could not be achieved.

**The lesson.** Wall-time discriminates wedge-engagement vs wedge-
give-up only if the two have different runtime distributions. The data
says they don't — both finish in sub-ms time. A working selective-verify
lever would need a different signal (per-pair wedge-rule-fire count,
saturation-snapshot delta, or a per-class structural "interestingness"
score), not a stopwatch. The mechanism shipped (sound, opt-in via env
var) for users who can profile their specific workload; the default is
off, per the dead-end #11 discipline ("don't default-on what isn't
proven").

**Recovery path.** Phase 2 of the design spec (`docs/superpowers/specs/
2026-05-31-soundness-completeness-perf-design.md` §"Phase 2 — Deep
completeness calculus") takes over the GALEN/notgalen MISSED-reduction
goal via functional-role inference and ≥n-with-disjointness rules —
the genuine calculus gaps that the handoff originally identified as
the root cause.

---

---

## 14. Synthetic-id-tracked witness for functional-role merge

**What was tried.** Phase 2a Task 4 implemented the EL++ functional-
role witness-merge rule with `merged_witness: HashMap<(ClassId,
RoleId), ClassId>` tracking a single synthetic id per (sub, R_f)
pair. The intent: when a new fact (sub, R_i, A) arrives and R_i ⊑
R_f functional, merge {prev_witness, A} into a new Tseitin synthetic;
update prev_witness; emit (sub, R_f, new_synthetic). Loop prevention
was supposed to come from the dedup short-circuit (`prev == fact.target`).

**What killed it.** The 3-sub-property fan-in canary (T5) hung
indefinitely. Trace showed unbounded growth: each emission produces
a fresh synthetic that itself re-triggers the rule on existing facts
(via reflexive R_f ∈ functional_supers_of(R_f)), producing yet
another synthetic. The synthetic IDs grow `49 → 50 → 51 → ...` with
each new fact in the (sub, R_f) chain. The GALEN scan showed
`ProcessModifierAttribute` has 12 sub-properties; corpus-diff on
GALEN would have hung deterministically.

**The lesson.** Tracking a per-pair WITNESS by synthetic id makes
the rule's termination depend on synthetic-id stability across
re-firings — which doesn't hold when each merge produces a fresh id
that re-triggers. The fix (T4.5): track the ATOM SET per pair
(`merged_atom_sets: HashMap<(ClassId, RoleId), BTreeSet<ClassId>>`)
of original-vocabulary atomic class IDs. Termination is by
construction: the set is monotonically bounded by the atomic
vocabulary, so per (sub, R_f) the rule fires at most
|atomic_vocabulary| times. The atom-set redesign passes both the
3-property and 4-property canaries in milliseconds.

**Recovery path.** T4.5 redesign atomic-content tracking is the
shipped form. See `crates/owl-dl-saturation/src/lib.rs` (T4 commit
124d0ca → T4.5 commit f2e2d7c) and `docs/phase2a-results.md`.

---

## 15. Sub-role witness propagation (Phase 2c)

**Hypothesis.** Phase 2a's functional-role witness-merge emits on the
functional super-role R_f but not back down to sub-roles R_k ⊑ R_f
where downstream existential triggers live (e.g. IPBP's defining
trigger on `hasIntrinsicPathologicalStatus`). Propagating the merged
synthetic back to every R_k on which X already has a fact would close
the IPBP-derivation MISSED cluster (Phase 2c.0 predicted 24-44 pairs).

**Status.** Sound and terminating, 0 / 44 predicted corpus recovery.

**Why it failed.** The saturator propagates subsumers (not facts) to
subclasses. The IPBP-cluster's "second feed-in fact" lives on a parent
class (e.g. `PathologicalBodyProcess`) and the subclass inherits the
subsumer at `process_subsumer` time without materializing the fact on
`facts_by_sub[subclass]`. Phase 2c's rule is fact-time; it iterates
`facts_by_sub[X]`; sees only one fact; doesn't fire. The rule does
fire on classes with two directly-materialized facts (3× on pair_06),
but those emissions don't reach any downstream trigger because the
downstream existential heads don't sit on the propagated sub-roles.

**Cost when shipped.** +7% wall on GALEN (12.2 → 13.1 min) and notgalen
(~30 → 32.1 min), paid every classify run for zero benefit. Reverted in
commit cc2019e.

**Don't try this again without first solving** fact-on-subclass
propagation at `process_subsumer` time. A re-attempt at sub-role
witness propagation without that prerequisite will hit the same
empirical 0 / N result.

**Cross-references.** `docs/phase2c-fix-target.md` (design analysis;
the "Predicted walkthrough on pair_06 (and what actually happened)"
section is the empirical reckoning); `docs/phase2c-galen-diagnosis.md`
(Phase 2c.0 cluster shift); `docs/phase2c-results.md` (measurement
headline); `crates/owl-dl-reasoner/tests/phase2c_pair_06_canary.rs`
(gap-asserting canary, kept).

### RESOLVED 2026-06-01

Phase 2d (fact-on-subclass propagation at `process_subsumer` and
`push_fact`, commit b78c5fd) provides the architectural prerequisite
this entry called out. Phase 2c-redux re-applies the original Phase 2c
rule unchanged on top of Phase 2d (commit 34a2b62) — now fires
because `facts_by_sub[X]` contains inherited facts.

**Combined result on GALEN**: MISSED 17 → 0 (full parity with Konclude,
`rustdl_closure = 27997 = konclude_closure`); wall +6.5% (12.55 →
13.36 min). On notgalen: MISSED 27 → 18 (9 recovered, IPBP-cluster);
wall +2.7%. FP=0 throughout. See `docs/phase2d-2c-redux-results.md`.

The §15 "don't try this again without first solving" framing was
correct: Phase 2c alone couldn't fire because facts weren't materialized
on subclasses. Phase 2d's fact-inheritance unblocks it cleanly without
needing case-split / covering / hypertableau extension — the original
Phase 2c witness-coincidence argument extends because inherited facts
preserve the same model-theoretic witness as the parent's fact.

---

## 16. Edge-keyed role-rule indexing (Phase 3e)

**Hypothesis.** Replace `apply_role_rules`'s per-rule × per-edge
`edge_satisfies` cost (7.26% of SIO post-3d) with O(1) HashMap lookup
per edge against role-keyed rule indices populated in `finalize()`.

**Status.** Sound and terminating; SIO `apply_role_rules` top frame
16.36% → 8.87% (−7.49pp); GALEN wall +2.34% across two samples.

**Why it failed (workload-dependent break-even).** Per-edge HashMap
lookup cost vs saved `edge_satisfies` traversal cost depends on rule
density per role. SIO has many rules per role → the indexing wins
because edge_satisfies traversal would have fired many times per edge.
GALEN has few rules per role and many edges per node → the indexing
HashMap probe cost exceeds the saved traversal. Net wall regression
on GALEN despite the SIO flame win.

**Cost when shipped.** +2.34% GALEN wall, four new
`HashMap<RoleId, Vec<_>>` on `AbsorbedTBox` + a
`finalize_role_edge_indices` method invoked from
`PreparedOntology::from_internal`. Reverted in commit a2a4d7f.

**Don't reattempt without first solving** workload-dependent dispatch
(profile rule density at finalize time and gate the indexed path on a
threshold); OR use a simpler single-direction index to reduce per-edge
lookup cost; OR cache `matching_edges` results per role with
reset-on-edge-change semantics. A naive reattempt with the same
four-HashMap design will hit the same GALEN regression.

**Cross-references.** `docs/phase3e-fix-target.md` (design analysis is
reusable for workload-adaptive variant); `docs/phase3e-results.md`
(measurement headline + GALEN regression evidence).

---

## 17. apply_max post-3d residual exhausted (Phase 3f)

**Hypothesis:** Phase 3f targeted `apply_max` (post-3c flame: 14.34%) for the same recon-first 6-task pattern as Phases 3d/3e.

**Status:** killed at recon (T1). No shippable target found; Phase 3f deferred without implementation.

**What the recon found** (`docs/phase3f-recon.md`, commit d9cc1ca):
- apply_max is actually **11.42% post-3d** (not 14.34% — Phase 3d's hoist also shaved ~3pp from apply_max as a denominator-redistribution side effect; plan baseline was stale post-3c).
- Dominant inner cost: `edge_satisfies` at 64.4% of apply_max (7.35% of total SIO), split 51% `are_declared_inverses` HashSet probe / 45% `is_sub_role` binary_search. Both already O(1); the cost is irreducible probe overhead, not algorithmic.
- The workload-neutral candidates (D: O(c²) pair loop, E: `compute_max_merge_deps`, F: `c_neighbours.contains` dedup) are **completely absent from the flame** — empirically zero on SIO.
- The only workload-neutral candidate present is C: `maxes` Vec allocation (21.2% of apply_max = 2.42% of total). Best-case surgical removal predicts ~0.7% GALEN wall reduction (Phase 3d's 18→3pp flame translated to 4.5% wall, a ~30% conversion ratio; Stage 1's 2.42pp at the same ratio is ~0.7%). Below the tightened ≥2% ship threshold and inside the noise floor disambiguated in Phase 3e.
- The §16-shape Stage 2 (`edge_satisfies` caching) is the actual cost lever but repeats Phase 3e's workload-dependent break-even pattern. Even a counter-gated workload-adaptive variant pays the HashMap construction cost on every classify.

**Cost when shipped:** none (recon-only; no code committed).

**Don't reattempt without first:** capturing a fresh post-3d (or later) baseline flame — the post-3c numbers used as Phase 3f's planning baseline were stale by ~3pp on apply_max alone. Phase 3 returns are exhausted on this function until a hashbrown alternative or a structurally different approach (e.g. merging multiple Max constraints over the same role) presents itself.

**Cross-references:** `docs/phase3f-recon.md` (full drill-down with sample counts); §16 (the workload-dependence pattern that made Stage 2 a non-starter); `docs/superpowers/plans/2026-06-01-phase3f-apply-max.md` (deferred plan).

---

## 18. ORE-15672 NoVerdict cluster — search-budget-bound, not rule-bound (Phase 9)

**Hypothesis:** ORE-15672's residual 17× Konclude wall (post-Phase-8)
is driven by a specific SROIQ construct that the wedge can't handle
in budget; identifying it would point to a saturator extension
(nominals, qualified cardinality, inverse-role propagation) that
would close the gap.

**Status:** killed at recon (`docs/phase9-recon.md`, commit `ded4368`).
No surgical lever; ORE-15672 is accepted as out-of-scope for the
incremental approach.

**What the recon found:**
- The "46-class NoVerdict cluster" framing from Phase 8 was a
  miscount. Only **3 classes** are NoVerdict: `e-collaboration-situation`,
  `e-interaction-situation`, `epistemic-workflow-enactment`. The
  `misses=46` counter records tier-walk consultation events (each
  staller consulted ~15 times).
- Increasing the cache-build budget to 30,000 ms doesn't convert
  any of the 3 to Sat — they're truly intractable in any practical
  budget at the current architecture's search shape.
- **No dominant construct.** A near-identical sibling class
  (`e-usage-situation`) with the same `EquivalentClasses` shape
  resolves to Sat in <1 s. The wedge demonstrably handles the
  construct shape. What discriminates pass/fail is the joint-expansion
  cost of 3 `∃proper-part.X` hops where `proper-part` is transitive
  + inverse, layered on an inherited 5-conjunct body containing
  `ObjectHasValue` + ancestor `ObjectMinCardinality(5 setting-for)`.
- Each candidate saturator extension was structurally ruled out:
  - **Nominal-singleton handling**: ruled out because
    `epistemic-influence-situation` (same cardinality-rich body sans
    `ObjectHasValue`) resolves.
  - **Qualified cardinality**: ruled out because no ≤n/=n in the
    ontology.
  - **Inverse-role propagation**: ruled out because `e-usage-situation`
    (uses the same inverse role) resolves.

**Cost when shipped:** none (recon-only; no code committed).

**Don't reattempt without first:** measuring whether the workload
profile shifts (the only structurally-different angle that could
help is Konclude-style shared-model caching, which is dead-end §2
territory with uncertain benefit on small workloads). For
single-ontology gaps without a transferable mechanism, accepting
the gap is the honest answer.

**Cross-references:** `docs/phase9-recon.md` (full breakdown with
per-class construct profile and the sibling-class disambiguation
that ruled out each extension candidate); `docs/phase8-results.md`
(Phase 8's note now corrected to reflect the 3-not-46 count); §2
(the Konclude-style alternative).

---

## 19. Per-class `BackPropRisk` refinement alone (Phase 3a)

**What was tried (recon-only).** Phase 1a shipped an ontology-wide
`BackPropRisk` classifier: any axiom touching inverse / nominal /
cardinality flags the whole ontology Unsafe. SROIQ workloads
(ore-15672, ore-10908, pizza) are uniformly Unsafe, so the snapshot
cache never engages on them. Phase 3 hypothesis: refine to per-class
classification + lean on the runtime sentinel (Phase 1b T3) as the
safety net. Spec §6 Phase 3 target: `ore-15672 ≤ 10× Konclude` (~17.5s
vs current 29s, ~40% wall reduction).

**What killed it.** The per-class refinement is *structurally* viable —
recon measured 82-99% of SROIQ classes would be Safe under per-class
classification (ore-15672: 67/82, ore-10908: 683/692, ore-15516:
82/84). But the projected wall savings are bounded:
- On ore-15672, **96% of wall is `tier_walk`** (28s of 29s) — per the
  Phase 2a recon instrumentation. Snapshot cache replaces wedge calls,
  not the per-pair tableau calls that dominate tier_walk.
- Per dead-end §18, ore-15672's residual cost is **search-budget
  exhaustion on 3 hard classes** — intrinsic tableau search,
  unreachable by snapshot reuse on Safe classes.
- Projected per-class snapshot savings on ore-15672: ~1-2s wall
  (~10% reduction). Projected post-Phase-3 wall: ~27s. Spec §6 target
  ≤ 17.5s still unreachable.

**The lesson.** A structural lever that's architecturally sound can
still be the wrong lever if the *dominant cost* it doesn't address is
the load-bearing one. Per-class refinement IS the right tool for
"unlock snapshot on SROIQ", but the project's ore-15672 wall is
dominated by an orthogonal cost (the §18 hard-class cluster) — so
Phase 3 doesn't deliver the spec target alone. Pairs with §18's
"accept the gap" recommendation: closing ore-15672 needs Konclude-style
sub-tableau / multi-class-search (out of scope for the snapshot cache
project).

**Cost when shipped:** none beyond the recon instrumentation
(`BackPropRisk::classify_class` + the per-class counter +
`# per-class BackPropRisk:` banner line; commit `a6983ed`). All
diagnostic; the runtime ontology-wide classifier is unchanged.

**Don't reattempt without first:** addressing dead-end §18's hard-class
cluster (would require structurally-different sub-tableau-caching work,
not snapshot refinement). Or: measuring on a SROIQ workload where
tier_walk is NOT dominated by a small hard-class cluster (different
distribution shape would change the cost-benefit calculus).

**Cross-references:** `docs/phase3a-recon.md` (full measurement + analysis);
`docs/phase2a-recon.md` (the wall_breakdown instrumentation that
identified tier_walk as load-bearing on SROIQ); §18 (ore-15672
hard-class cluster); §2 (Konclude-style sub-tableau caching — the
structurally-different alternative).

---

## Meta-lesson

Every dead-end above had a *plausible first-principles motivation* and
was killed by **either**:
- a counter / wall measurement that contradicted the prediction
  (#1, #3, #5, #7, #10, #13),
- a corpus diff that caught what the canary didn't (#4, #6, #7),
- a traced argument on the actual canary / repro (#2, #8, #9),
- or a measurement on a workload outside the original validation set
  (#11).

**None** was killed by reasoning from theory alone. The forward-going
discipline: every "this should help" deserves a measurement *before*
shipping; every "this is sound" deserves a corpus-or-larger diff
*before* trusting; every analogy to a neighbouring domain deserves a
profile *before* transplanting.
