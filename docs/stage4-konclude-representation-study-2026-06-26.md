# Stage-4 — Konclude representation study (the engine's de-risking sub-project)

**Status:** findings (durable). The user committed to the from-scratch nominal-calculus
reimplementation; since internal mechanism-probes are exhausted (no transplantable single lever
found), the only way to de-risk the build is to study Konclude's actual source and identify the
structural invariant that keeps its dependencies sparse / its search small on the wine pattern.
Konclude `master` cloned to scratchpad; three focused agents read
`Source/Reasoner/Kernel/{Cache,Process,Algorithm,Calculation,Strategy}` against the precise
question, each mapped to a refuted mechanism. This doc synthesizes + reconciles with our own
measurements (SP-0, would-prune, minimal-sound-key).

## What Konclude actually does (three convergent reads, all cited to source)

### 1. Cardinality + disjunction (the search-shape lever)
- **No algebraic/ILP cardinality exists** — grep over all of `Source/Reasoner` for
  algebra/inequation/integer-program/arithmetic = zero hits; `≤n` is standard
  nondeterministic choose-and-merge (`applyATMOSTRule`, tableau alg ~14859). **This
  independently re-confirms our would-prune gate (marginal=0): algebraic cardinality is not
  Konclude's lever either.**
- Konclude avoids the disjunction/value-assignment explosion via **saturation-driven
  DETERMINISTIC concept expansion**, in layers:
  - **GCI absorption** (`CTriggeredImplicationGCIAbsorberPreProcess`): a disjunction with ≤1
    non-triggerable disjunct becomes a triggered implication (`CCIMPL`) that fires
    *deterministically* (priority 9, inside deterministic territory) — the `⊔` rule is never
    reached.
  - **priority-delayed OR + containment** (`planORProcessing` ~16489): if a disjunct is already
    in the label, or a trigger can be deferred, or ≤1 live disjunct remains (others pruned by
    saturation clash flags) → apply deterministically, no branch.
  - **deterministic-expansion cache** (`CSaturationNodeAssociatedDeterministicConceptExpansion`,
    `requiresNonDeterministicExpansion`): when saturation proved a node's expansion deterministic,
    the tableau **bulk-applies all cached concepts with zero branching** (~21812). Agent 3's best
    candidate for wine's fast path.
  - **saturation ATMOST-functional-merge insufficiency** (`tryATMOSTConceptSuccessorMerging`
    ~3958): when `≤1 R` has two provably-unmergeable (distinct-nominal / told-disjoint)
    successors, mark the class `INDSATFLAGINSUFFICIENT` and propagate to ancestors → feeds the
    deterministic cache / prunes.
- **Verdict (Agent 3):** not a single transplantable mechanism — an entangled, co-designed
  combination (absorption needs the priority queue; the det-cache needs the full saturation
  pass). Honest caveat: *could not confirm which mechanism is decisive on a live wine class.*

### 2. Reuse / caching (the soundness lever)
- Classification caches use **sparse keys but only by side-stepping the hard nominal case**:
  the signature-satisfiable cache is entered only when the node has **no nominal connection**
  (restores a tree → context = one ancestor edge); the reuse-completion-graph cache fires only
  on the predecessor-free class root and, when reuse isn't provably safe, is **demoted to one
  branch of a binary choice whose sibling is full sound expansion** (a bad reuse just
  backtracks). The unifying invariant: **trust cached UNSAT freely (monotone); trust cached SAT
  only under a checkable context-independence condition — else verify or branch.**
- The one **context-rich** cache (`CBackendRepresentativeMemoryCache`: label + full neighbourhood
  + nominal markers) is for **realization/instance-retrieval, not classification** — zero uses in
  `Classifier/`.
- The replicable fragment is `CSaturationNodeAssociatedDependentNominalSet` (a *compact explicit
  set of depended-on nominal IDs* as an invalidation key) — but **it only stays sparse because
  Konclude's dependency tracking yields a small explicit set upstream.** This matches our
  minimal-sound-key gate exactly: sound reuse needs sparse upstream deps, which rustdl lacks.

### 3. Merge-dependency representation
- Konclude stores **one scalar `mBranchingTag` per fact** (= MAX of ancestor branching levels),
  not a dep-set; a deterministic merge contributes **exactly one integer**
  (`CMERGEDCONCEPTDependencyNode` = `max(mergeDecisionLevel, conceptLevel)`). Clash dep = list of
  leaf `(node, tag)` pairs; backjump target = `MAX(leaf tags)`; nominals trigger an
  `exactIndiNodeTracking` conservative fallback.

## Reconciliation with our own data — Agent 2's "transplant the scalar to sparsify" is REFUTED

Agent 2 read the scalar `mBranchingTag` as "sparse-by-a-replicable-invariant, transplant it to
fix rustdl's dense `birth_deps`." **Our measurements refute this for the backjumping purpose:**

- **SP-0** computed rustdl's *precise* per-fact merge deps and found them **identical to the dense
  deps** (shadow=real=0) on wine's hot path: the partition-exhaustion `≤n` clashes are
  **genuinely globally-dependent**, not a tracking artifact.
- A scalar MAX is **coarser** than a dep-set — it cannot make a genuinely-dense dependency sparse;
  it can only lose information (hence Konclude's `exactIndiNodeTracking` nominal fallback). And
  **narrowing rustdl's deps was unsound** (precise-merge-deps FP=232; the clash genuinely depends
  on the wide context).

So the scalar dep is **not** the lever, and SP-0 + the FP=232 stand. The reconciliation: Konclude
does not *backjump* these dense clashes better — it **never generates them**. Its
saturation-driven deterministic expansion (§1) resolves the value-choices before branching, so
the partition-exhaustion `≤n` clashes (SP-0's 78%) don't arise. The dense-dependency clashes are a
property of the *blind disjunctive search* rustdl does; Konclude does a different, mostly-
deterministic expansion. The scalar dep + compact nominal-set + sound-reuse are **supporting
machinery** for an engine that already avoids the hard search — not the cause of the speed.

## The identified lever (evidence-reconciled) and the one open crux

**Lever:** saturation-driven **deterministic concept expansion** — pre-resolve value-choices in a
strong saturation pass so the nondeterministic branches are never generated. rustdl **already does
a partial version**: the ∃-seed (shipped 15×, wine 49s→3.2s) feeds the saturator's derived
deterministic ∃-facts into the wedge; B2b/B2c made the saturator *complete-in-output* on wine (201
edges = full closure). This is the SAME family as Konclude's mechanism — and it is exactly the
arc's existing, FP-gateable, additive style (forced-disjunct / ∃-seed), **not** a wholesale
rewrite.

**The crux the study narrowed but could not close** (Agent 3's explicit uncertainty): our Stage-4
characterization found the residual **8 wine classes are genuinely nondeterministic** for rustdl —
their ∃-facts are seeded and they still don't collapse. The open question is binary and decisive:

- **(A) Konclude resolves these same 8 classes DETERMINISTICALLY** — via a determinism rustdl's
  saturator lacks (GCI absorption into triggered implications, and/or the specific saturation
  ATMOST-insufficiency propagation). Then there is a **specific, identifiable, sound, additive
  saturator increment** (port that determinism), gateable FP=0 the same way B1–B2c were — a real,
  bounded GO.
- **(B) Konclude ALSO treats them as nondeterministic but branches cheaply** (small per-branch
  cost + the side-stepping/tree-restoration reuse). Then it is the dense-dep / cheap-branching
  wall our gates already proved rustdl cannot cross soundly (SP-0, minimal-sound-key, would-prune)
  — and the only path is the wholesale architecture, no bounded increment.

**This is decidable by one measurement: run Konclude on Gamay (and the 8) with branch/statistics
output and read the branch count per hard class.** Few branches ⇒ (A) deterministic ⇒ port the
specific saturator determinism. Many cheap branches ⇒ (B) the wall. This replaces "wholesale
reimplementation on faith" with a concrete go/no-go on the *only* lever the study leaves standing.

## Code comparison (follow-up): the specific missing determinism — identified

Two further agents pinned Konclude's two determinism mechanisms to exact trigger shapes and audited
rustdl for each. Result: rustdl is missing a **specific, concrete** determinism, in the shape the
elimination argument predicted.

**Mechanism 1 — disjunction → deterministic triggered implication.**
- Konclude (`CTriggeredImplicationGCIAbsorberPreProcess`): a disjunction is absorbed when ≤1
  disjunct is non-"triggerable"; a positively-appearing **named class** IS triggerable, so
  `Red ⊔ White ⊔ Rosé` is absorbed into `¬Red ⊓ ¬White → Rosé` (and rotations), firing
  deterministically when n−1 disjuncts are excluded. **Handles nominal-bearing value disjunctions.**
- rustdl (`approx_saturation::derive_forced_disjuncts` + SP-B1/B2a in `process_subsumer`/
  `process_unsat`): forces a disjunct only when all-but-one are eliminated by told/derived
  **disjointness**, and **atomic disjuncts ONLY** — nominal unions like `Red ⊔ White ⊔ Rosé` are
  rejected at registration. So rustdl's saturator does NOT deterministically resolve wine's
  value-partition disjunctions; they fall to the wedge's per-branch `live`-filter (the det-
  resolution-gate 18–34%, vs Konclude ~100%).

**Mechanism 2 — ≤1/functional merge → class-unsat, saturation-time + global.**
- Konclude: global saturation check; when `≤1 R` has two unmergeable successors (distinct
  nominals / told-disjoint qualifiers) marks the class `INSUFFICIENT` and propagates up
  (predecessors) and down (`copyDepending` ≈ subclasses) in one pass.
- rustdl: the functional witness-merge → `C ⊑ ⊥` path **and** ancestor+subclass propagation are
  present and full — **but only when the two qualifiers are related by a `DisjointClasses` axiom.**
  The EL saturator **does not feed `DifferentIndividuals` into `disjoint_pairs` for NomKeys**, so
  `∃R.{a} ⊓ ∃R.{b}` with `Functional(R)` and distinct nominals `a≠b` does **not** trigger
  merge-unsat at the class level (the `abox_saturation` module catches only the ground-individual
  version). This is precisely wine's value-partition shape (`≤1 hasColor` over distinct color
  nominals), and it is a **global saturation-time** check — categorically beyond the frontier
  would-prune probe (marginal=0), exactly as the advisor noted.

**The identified candidate increment (concrete, sound, additive, FP-gateable):** feed
`DifferentIndividuals` (and UNA where it applies) into the saturator's `disjoint_pairs` as
`DisjointClasses(NomKey(a), NomKey(b))`, so the functional-merge synthetic becomes unsat on the
nominal value-partition case; this lets the existing B2a forced-disjunct machinery resolve more
value-disjunctions deterministically, enriching the ∃-seed → collapsing more of the residual 8
hard classes in the wedge. Same family/style as B1–B2c + the ∃-seed. Whether it actually collapses
wine (and stays FP=0) is the build+gate question; per the three-times-unsound history of nominal
pruning, the gate is **wine closure-diff FP=0, run wine FIRST**, before believing any wall number.
This converts "wholesale Konclude reimplementation on faith" into one specific, testable saturator
rule.

## Net

The study was productive: it **independently re-confirmed** algebraic-cardinality and
reuse/caching are not Konclude's levers (matching our gates), **refuted** the scalar-dep transplant
against SP-0, and **isolated** the one standing lever — saturation-driven deterministic expansion,
which rustdl already does partially. The wholesale-rewrite scope is **not yet justified**; the next
step is the cheap Konclude-branch-stats measurement on Gamay to decide (A) bounded saturator
increment vs (B) the wall. Source: clone at
`scratchpad/konclude-src`; agent reports captured in this session.
