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
