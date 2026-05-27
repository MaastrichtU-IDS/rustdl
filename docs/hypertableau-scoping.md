# Hypertableau — scoping & design

Drafted 2026-05-27. The default-mode gap on SROIQ-heavy
ontologies (pizza, SIO) is, per the session's diagnosis
([`pizza-convergence-diagnosis.md`](pizza-convergence-diagnosis.md),
[`session-summary-2026-05-27.md`](session-summary-2026-05-27.md)),
a **search-branching** problem that no per-step or learning
optimisation moves — pairs don't converge inside the per-pair
budget. Hypertableau (Motik, Shearer & Horrocks 2009, *Hypertableau
Reasoning for Description Logics*, JAIR; the algorithm HermiT runs)
is the established solution: it reduces the branching factor *by
construction*.

This doc scopes the work before any core code is written. It is
**not** a commitment to a specific timeline; it's the map that
de-risks a multi-month effort and lets it proceed in
sound-at-every-step phases.

## 1. Why hypertableau reduces branching

Today's tableau works on NNF concepts. A GCI like
`Pizza ⊑ ∃hasTopping.PizzaTopping ⊓ …` and a covering axiom
`PizzaTopping ≡ Cheese ⊔ Meat ⊔ …` become, after absorption,
rules whose **Or-shaped conclusions** force a branching decision
at every node that carries the trigger. With disjoint toppings,
the choices interact, and the naive DFS re-explores the same
local conflicts across sub-trees (the measured pizza explosion:
~12 k branch points per probe, `pizza-convergence-diagnosis.md`).

Hypertableau preprocesses the ontology into **DL-clauses** of the
form

```
U1 ∧ U2 ∧ … ∧ Um  →  V1 ∨ V2 ∨ … ∨ Vn
```

where the `Ui` are *atoms* (concept atoms `A(x)`, role atoms
`R(x,y)`, equality `x ≈ y`) over variables, and the head is a
disjunction of atoms. The hyper**resolution** rule fires a clause
only when its *entire body* is already satisfied in the model —
so a clause never speculatively branches; it branches only when
its body genuinely holds and its head isn't yet satisfied. That
is the structural win: branching is **demand-driven on satisfied
bodies**, not eager on every trigger label.

Key consequences:
- Far fewer, far more targeted branch points (only real,
  body-satisfied disjunctive heads).
- No NNF blow-up of nested `∃`/`∀`; clauses are flat implications.
- Termination via **anywhere blocking** (a node is blocked by any
  earlier node with a compatible label set, not just an ancestor
  with a matching parent role) + **pairwise/core blocking** for
  the SHOIQ features.

## 2. What rustdl reuses vs replaces

**Reused (the investment that stays):**
- `owl-dl-core` IR — `ConceptExpr`, `ConceptId`, `ClassId`,
  `RoleId`, `ConceptPool` interning. DL-clause atoms reference
  these.
- The horned-owl front end + `convert_ontology`.
- `owl-dl-saturation` EL closure — still the fast path; the
  orchestrator consults it first, exactly as today. Hypertableau
  only replaces the *tableau* fallback.
- The whole test apparatus: 87-fixture differential corpus,
  `tests/real_ontology_corpus.rs`, the ROBOT-docker reference
  harness, `scripts/bench-rustdl-modes.sh`.
- `--saturation-only` and the public API surface — unchanged.
- Diagnostic tooling (`RUSTDL_TRACE`, `RUSTDL_COUNTERS`,
  `tbox-stats`, etc.).

**Replaced (the rewrite, all inside `owl-dl-tableau` + a new
clausifier in `owl-dl-core`):**
- `owl-dl-core::absorb` → a **clausifier** producing DL-clauses
  (structural transformation + clausification, Motik §4). Absorption
  becomes a special case (Horn clauses fire without branching).
- `owl-dl-tableau::rules` (the `apply_*` family) → **hyperresolution
  rule application**: match a clause body against a node (and its
  role-neighbours), fire the head.
- `owl-dl-tableau::saturate` + `search` → the hypertableau
  derivation: deterministic clause application to fixpoint,
  then branch on a satisfied-body disjunctive head.
- `owl-dl-tableau::is_blocked` (pair blocking) → **anywhere
  blocking** with the core-blocking refinement for ≤n/nominals.

**New module boundary:** `owl-dl-tableau` keeps its public
`is_satisfiable` / `decide` signatures so `owl-dl-reasoner` and the
orchestrator are unaffected. The rewrite is behind that facade.

## 3. DL-clause representation (grounded in the IR)

Proposed types in a new `owl-dl-core::clause` module:

```rust
/// An atom in a DL-clause body or head. Variables are small
/// indices (0 = the "central" individual x, 1.. = successors yi).
enum Atom {
    Class(ClassId, Var),           // A(x)
    Role(RoleId, Var, Var),        // R(x, y)
    Equal(Var, Var),               // x ≈ y   (for ≤n / functional)
    // ∃ in a head becomes a "generate a successor" atom:
    Exists(RoleId, ClassId, Var),  // ∃R.A(x) — head-only
}

type Var = u32;

struct DlClause {
    body: Box<[Atom]>,   // conjunction; empty body = ⊤ ⊑ head
    head: Box<[Atom]>,   // disjunction; empty head = body ⊑ ⊥ (clash)
}
```

Clausification (Motik §4, structural transformation): introduce a
fresh atomic name for each compound sub-concept (Tseitin — rustdl
already does a limited version in saturation), then each axiom
becomes one or more `DlClause`s. Examples:
- `A ⊑ B`            → `A(x) → B(x)`
- `A ⊑ B ⊔ C`        → `A(x) → B(x) ∨ C(x)`
- `A ⊑ ∃R.B`         → `A(x) → Exists(R, B, x)`
- `A ⊑ ∀R.B`         → `A(x) ∧ R(x,y) → B(y)`
- `A ⊓ B ⊑ ⊥`        → `A(x) ∧ B(x) →` (empty head)
- `≤1 R.A`           → `A(x)∧R(x,y1)∧R(x,y2)∧A(y1)∧A(y2) → y1 ≈ y2`

The flat conjunctive bodies are exactly what makes hyperresolution
fire only on fully-satisfied premises.

## 4. The derivation loop

```
derive(clauses, root_concept):
  graph ← { x0 : {root_concept} }
  loop:
    # Hyperresolution to fixpoint (deterministic clauses):
    while some clause C with all body atoms matched at a node n
          and no head atom satisfied:
      if C.head is empty: return Clash(n)
      if C.head is a single atom: assert it (deterministic)
      else: record (n, C) as an open disjunctive choice
    # Generation: fire Exists head-atoms (create successors),
    #   unless n is blocked.
    # Branch: pick an open disjunctive choice, try each head atom
    #   with trail-based undo (this is the only branching point).
    if no open choice and no clash: return Sat
```

The crucial difference from today: the inner `while` only fires a
clause when its **whole body matches**, so most of the work is
deterministic Horn-style propagation, and the branch step has far
fewer, far more relevant choices.

## 5. Blocking

- **Anywhere blocking** (Motik §5): node `y` is blocked if some
  *earlier-created* node `x` (not necessarily an ancestor) has
  `L(y) ⊆ L(x)` under the blocking-relevant signature. This fires
  where pair-blocking can't, because it doesn't require a matching
  parent-role chain.
- **Core / pairwise blocking** for `≤n` + nominals (SHOIQ): block
  on the *pair* (label, predecessor-label) to stay sound under
  inverse roles and cardinality.
- rustdl's existing `label_sig` Bloom prefilter and the
  `blocking` summary array are reusable substrate for the
  subset test.

Note: the session found pizza's blocking correctly *doesn't* fire
because successors genuinely differ. Anywhere blocking won't
change that for the *toppings* — but hypertableau's smaller
branching is what makes the model converge, and anywhere blocking
caps the depth on the cyclic-`∃` parts (ingredients-of-ingredients).

## 6. Phased plan (sound + tested at every phase)

The reasoner must pass the full real-corpus regression after each
phase. Run hypertableau **behind a feature flag / opt-in** until
it reaches parity, so the current tableau stays the default and
the regression gate compares the two.

- **Phase H0 — clausifier (no reasoning change).** New
  `owl-dl-core::clause` + a `clausify(internal) -> Vec<DlClause>`.
  Unit-test clause shapes against hand-built ontologies. The
  tableau still runs the old path; clauses are computed and
  validated structurally only. A `rustdl clause-stats FILE`
  diagnostic (like `tbox-stats`) reports clause counts/shapes.
- **Phase H1 — Horn-only hyperresolution.** Implement the
  deterministic derivation for clauses with ≤1 head atom (no
  branching) + Exists generation + anywhere blocking. Gate behind
  `--hypertableau`. Validate on the *pure-EL* corpus (GO, anatomy)
  where there's no branching — output must match the saturation
  closure exactly.
- **Phase H2 — disjunctive heads + branching.** Add the branch
  step with trail undo. Validate on the full SROIQ corpus
  (pizza/SIO/family) behind the flag; unsat sets must match
  HermiT. **This is the phase where pizza/SIO walls are
  measured** — the payoff check.
- **Phase H3 — core blocking + ≤n/nominals.** Complete SHOIQ
  feature coverage; the 87-fixture differential corpus must be
  green under the flag.
- **Phase H4 — flip the default**, retire the old tableau (or keep
  it as `--legacy-tableau` for benchmarking, mirroring
  `--n2-classify`).

Each phase is independently committable and leaves the default
reasoner sound.

## 7. Risks

- **Clausification correctness** is the foundation; a wrong clause
  is an unsound reasoner. Phase H0's structural tests + H1's
  EL-closure cross-check are the guards.
- **Blocking soundness under inverse roles + cardinality** is the
  classic SROIQ trap (the session already hit a pair-blocking
  cycle workaround). Core blocking must be implemented carefully;
  the differential corpus is the gate.
- **Scope creep / long red period.** Mitigated by the feature flag
  — the old tableau stays default until H4, so `main` is always
  shippable.
- **It might not be enough.** Hypertableau is necessary for
  default-mode parity but HermiT also has ~15 years of additional
  optimisation. Realistic target: "within 2–3× of HermiT on
  pizza/SIO," not "beat it." (Per `outperform-hermit-plan.md`.)

## 8. Honest cost/benefit

- **Cost:** multi-month; a near-total rewrite of `owl-dl-tableau`
  plus a clausifier, done in 5 phases, each gated on the full
  regression.
- **Benefit:** default-mode convergence on SROIQ-heavy ontologies
  — the one class of workloads `--saturation-only` can't serve
  soundly (pizza loses 20 % of edges under sat-only).
- **The alternative already shipped:** `--saturation-only` beats
  Konclude/HermiT on the mostly-EL corpus today. Hypertableau is
  worth it **iff** SROIQ-heavy default-mode parity is a goal in
  its own right, not just "good benchmarks on bio-ontologies."

## §H0 — clausifier measurement (2026-05-27, shipped)

Phase H0 is done (`4d38a22`): the DL-clausifier + `rustdl
clause-stats`. No reasoning change. The clause-shape distribution
across the corpus **validates the hypertableau hypothesis**:

| Workload | total | Horn | disjunctive | ⊥-headed | ∃-head | deferred |
|---|---|---|---|---|---|---|
| pizza | 704 | 674 (96%) | **30** | 409 | 163 | 21 |
| SULO | 62 | 58 | 4 | 24 | 6 | 11 |
| SIO | 2379 | 2338 (98%) | **41** | 227 | 409 | 162 |
| family | 283 | 279 | 4 | 5 | 113 | 112 |
| RO | 232 | 218 | 14 | 13 | 10 | 171 (74%) |
| GO | 72697 | 72697 (100%) | 0 | 0 | 14168 | 0 |

Takeaways:
- **The corpus is overwhelmingly Horn** (pizza 96 %, SIO 98 %,
  GO 100 %). Branching concentrates in a *handful* of disjunctive
  clauses — pizza **30**, SIO **41**. Under hyperresolution those
  fire only on satisfied bodies; the ~96 % Horn clauses are pure
  deterministic propagation. This is the structural reason
  hypertableau should converge where the eager-NNF tableau
  explodes.
- **Deferred coverage gaps** are now quantified: RO 74 %, family
  40 %, SIO 7 %, pizza 3 %. RO and family lean heavily on the
  cardinality / nominal constructs H0 doesn't clausify — those are
  H3's scope, and the numbers say H3 is load-bearing for RO/family
  but not pizza/SIO/GO.
- GO is pure Horn with many ∃-heads — confirms the EL fast path is
  the right default there; hypertableau would just reproduce the
  saturation closure.

## §H1 / §H1b — Horn engine shipped; clausifier foundation must change

**H1 (shipped, `da54c5b`):** a standalone Horn hyperresolution
engine (`owl-dl-tableau::hyper`). Clause-body matching (class
atoms on `x`, a role atom binding a successor `y`, and class atoms
on `y` — the `R(x,y) ∧ E(y) → F(x)` back-prop shape), ∃-generation
with witness reuse, anywhere blocking. 7 unit tests, all green,
including `existential_backprop_derives_subsumer_on_root` (the
engine derives `C ⊑ F` from the hand-clausified `∃R.E ⊑ F`).

**H1b finding (the important one): clausifying from the absorbed
TBox is the wrong foundation.** The end-to-end cross-check
(engine on the clausifier's output vs the EL entailment) **fails**
on `∃R.E ⊑ F`. `clause-stats` shows why: that axiom lands in the
`deferred` bucket. Absorption rewrites `∃R.E ⊑ F` into the
disjunctive residual `⊤ ⊑ ∀R.¬E ⊔ F` — a tableau-friendly form —
and the clausifier can't turn that disjunction into the Horn
clause `R(x,y) ∧ E(y) → F(x)` the engine needs. So the engine is
EL-complete (proved with hand-built clauses) but the *pipeline*
isn't, because the clausifier loses ∃-on-LHS.

The fix is **structural-transformation clausification from the NNF
axioms** (Motik §4), recognising `∃`/`∀`/`⊓`/`⊔` by polarity
directly, instead of clausifying the already-absorbed TBox. The
absorbed-TBox route was a fine H0 shortcut for *measuring* clause
shapes, but it bakes in tableau-specific disjunctive choices that
are wrong for hyperresolution.

Revised phase order:

- **H1c (new, next):** rebuild the clausifier as a
  structural transformation over NNF axioms. Then the ignored
  spec `hyper_horn_matches_el_closure_with_existential_backprop`
  must pass, and a broader cross-check — hyper-engine root labels
  vs `owl-dl-saturation` closure on GO/anatomy — must match
  exactly. *Only then is H1 truly validated.*
- H2 (disjunctive branching) and beyond proceed on the rebuilt
  clausifier.

This is exactly the kind of foundational correction the phased,
gated approach is meant to surface early — before branching is
layered on a clausifier that silently drops axioms.

## §H1c — structural-transformation clausifier (shipped)

The clausifier was rebuilt (`clausify` now structural-transforms
the NNF GCI axioms directly instead of the absorbed TBox).
Antecedent `∃` becomes body role+class atoms (`∃R.E ⊑ F` →
`R(x,y) ∧ E(y) → F(x)`), antecedent top-level `Or` splits per
disjunct, consequent `∀` moves the role into the body, `And`
splits, `Or` builds disjunctive heads, `Not(atomic)` and `⊥`
become ⊥-headed clauses. Cardinality / nominals / nested
antecedent `∀`/`Or`/`Not` are still deferred (H3), counted.

The H1b cross-check `hyper_horn_matches_el_closure_with_existential_backprop`
is now un-ignored and **passes** — the pipeline derives `C ⊑ F`
from `∃R.E ⊑ F`. Deferred counts dropped sharply, confirming the
new clausifier covers far more of the corpus:

| Workload | deferred (from-absorbed) | deferred (structural) |
|---|---|---|
| pizza | 21 | 17 |
| SIO | 162 | 91 |
| family | 112 | 14 |
| RO | 171 | 17 |
| GO | 0 | 0 |

Remaining deferrals are cardinality + nominals (H3 scope). With a
sound, mostly-complete clausifier and a validated Horn engine, the
next phase is **H2 — disjunctive-head branching** (and the pizza/
SIO wall measurement that is the whole effort's payoff check).

## §H2 — disjunctive-head branching (shipped)

The engine ([`owl-dl-tableau::hyper`]) gained backtracking search
over disjunctive-head clauses via [`HyperEngine::decide(max_depth)`],
making it a complete decision procedure for the Horn + disjunctive
fragment the H1c clausifier produces (cardinality/nominals still
deferred to H3). Mechanics:

- Horn propagation runs to fixpoint (`horn_fixpoint`); non-Horn
  clauses are skipped there (`fire_clause` guards on `is_horn`).
- `find_open_disjunction` then looks for a disjunctive clause whose
  body matches a node-binding and whose head disjuncts are **all**
  unsatisfied there. A clause with an already-true disjunct is *not*
  a branch point (avoids redundant branching).
- Each disjunct is asserted in turn over a **saved copy** of the
  node vector (`self.nodes.clone()`); the search recurses. Restore
  happens only on a failed (`Unsat`/`Stalled`) branch — a `Sat`
  branch keeps its completion, so `root_labels()` stays meaningful.
  (Save/restore over a trail: correctness-first; trail-based undo is
  a perf follow-up if H2b validates the approach on real walls.)
- Both Horn firing and branching assert through one shared
  `apply_head_atom`, so `∃`-head disjuncts get the same witness-reuse
  + anywhere-blocking treatment.

**Three-valued result lattice** (soundness-critical): `Sat` if *any*
branch is satisfiable; `Unsat` only if *every* branch is decisively
unsatisfiable; `Stalled` if a branch hit the depth/iteration bound
and no branch decisively succeeded — so a depth-limited run can
never report a false `Unsat`. Eight unit tests cover first-branch
sat, restore-to-second-branch, exhaustive-failure unsat, multi-level
backtracking (sat-deep and unsat), satisfied-disjunct-skipped,
depth-bound→`Stalled`-not-`Unsat`, and `decide`≡`run` on Horn input.

Heuristic ordering (which open clause, which disjunct first) is
"first encountered, head order" — a deliberate non-choice; tuning is
a follow-up, not part of this increment.

**Next — H2b (measurement, the payoff check):** wire `decide`
behind a per-class / per-pair classify loop, clausify pizza + SIO,
and measure the hypertableau wall against the saturation+tableau
default. This is the experiment the whole effort exists to run; if
the walls don't move, the moms-plan §A criterion applies (revert
rather than ship working-but-unhelpful machinery).

## §H2b — wall probe (`rustdl hyper-sat`), first results

`HyperEngine::decide` is instrumented (`SearchStats`:
`branches_taken`, `restores`, `max_branch_depth`) and exposed via
`owl_dl_reasoner::hyper_sat_probe` + the `rustdl hyper-sat` CLI,
which runs concept-satisfiability per named class with a per-class
wall budget. **It is a performance probe, not a correctness claim:**
the H1c clausifier defers cardinality/nominals, so the clause set is
an under-approximation. Dropping axioms only removes constraints, so
`Models(full) ⊆ Models(fragment)` — a `Unsat` verdict is sound for
the full ontology, but a `Sat` verdict is **not**. The headline
metric is `classes_branched` (classes whose decision actually
exercised branching); a fast `Sat` with `branches_taken == 0` was
pure Horn propagation and says nothing about hypertableau.

First measurements (this server, depth 256, 5 s per-class budget):

**pizza** (702 clauses, 25 disjunctive, 17 deferred): 99 classes in
**12.3 ms total**, but only **2 classes branched** (depth 1). This
is expected, not a disappointment: pizza's diagnosed wall is in
**pair subsumption** `sat(A ⊓ ¬B)`, not bare `sat(A)` — a single
named root rarely entails the antecedents of the covering
disjunctions, so they never open. Reaching the pizza wall needs
**H2c**: inject `¬B` at the root (fresh `Q` with `Q→A`, `Q∧B→⊥`,
seed root `Q`) — a small clausifier add, not H3.

**sio-stripped** (2474 clauses, 41 disjunctive, 91 deferred): 1585
classes in **16.3 s total**, **817 classes branched**, max branch
depth **14**, zero `Stalled`. Slowest branched class 316 ms. The
default reasoner's `sat` on the two branchiest classes **times out
> 20 s** (one ran 135 s then errored). So on SIO the engine *does*
exercise real branching and finishes fast where the default stalls.

**Discriminator resolved — the wall moved on SIO bare-sat.** The
confound was that the dropped 91 axioms could be where the default
spends its time. The test is answer agreement: Konclude (full
ontology, all axioms) on
`ontologies/real/konclude-input/sio-stripped.owx` classifies in
**136 ms** and finds **0 unsatisfiable classes** (1606 SubClassOf
axioms; the only `owl:Nothing` mention is its declaration). Hyper —
over the fragment, *minus* the 91 deferred axioms — also returns **0
unsat**. The verdicts agree, so the drop is innocent here: the 91
axioms introduce no unsatisfiability hyper could miss. Therefore
hyper's 16.3 s (vs the default's > 135 s on a single class) is a
genuine architecture win on SIO bare concept-satisfiability, not an
artefact of skipping work. The §A criterion is satisfied for this
probe.

Two honest caveats:
1. **Hyper is still ~100× slower than Konclude** (16.3 s vs 0.14 s).
   The naive save/restore search has no ordering heuristics, no
   trail, and no dependency-directed backjumping — that gap is the
   real follow-up work, not the existence of a win.
2. **Pizza's wall is untouched** by bare-sat (see above) — H2c
   (pair-subsumption with `¬B` injection) is the probe that reaches
   it.

## §H2c — pair-subsumption probe (`hyper-classify-probe`)

`hyper-sat` showed pizza's wall is *not* in bare `sat(A)`. H2c
reaches it: subsumption `sub ⊑ sup` is decided by the standard
reduction to unsatisfiability of `sub ⊓ ¬sup`, encoded with a fresh
helper concept `q`: `q → sub` and `q ∧ sup → ⊥`, seeding the root
with `q`. `owl_dl_reasoner::hyper_subsumption_probe` runs it over
every ordered class pair; `rustdl hyper-classify-probe FILE` exposes
it (`--dump-subsumptions` emits the entailed pairs for set
comparison). Same asymmetry: `Unsat` (subsumption holds) is sound
for the full ontology, `Sat` (not subsumed) is not, so the
`subsumptions` count is a sound **lower bound** on the true hierarchy.

**Result on pizza (validated against Konclude's closure):**

- **Sound:** 0 false positives across all 581 reported subsumptions
  — every one is in Konclude's hierarchy closure.
- **84 % complete:** hyper finds 581 of Konclude's 695 closure pairs
  (over the same 99 named classes). The **114 misses cluster on
  exactly the deferred constructs**: `X ⊑ InterestingPizza`
  (`≡ Pizza ⊓ hasTopping min 3` — min-cardinality, deferred),
  `X ⊑ VegetarianTopping` / `VegetarianPizza` (∀-completion of the
  topping closure). Confirmed causal, not a bug.
- **Reaches the wall:** 192 pairs branch (vs bare-sat's 2); the probe
  finishes all 9702 pairs in 1.14 s where the default `classify`
  times out > 30 s.

**Pizza's wall is NOT yet claimed moved.** Unlike SIO, pizza's
dropped axioms are **load-bearing** — the 1.14 s is partly speed from
skipping the hard 16 % (the cardinality/∀ subsumptions). This is the
hollow-win risk, made concrete. The gate is **H3** (cardinality + ∀-
completion clausification), *not* heuristics or deeper depth — the
missing axioms are the binding constraint. §A does not say revert
(the probe is sound and SIO is a genuine win); it says block the
pizza wall claim until H3.

### SIO vs pizza — the load-bearing distinction

Both probes reach the engine and both have a deferred-axiom count, so
they look alike. The discriminator outcome is **opposite**, and that
is the finding that matters for future work:

| | deferred drop | agreement vs Konclude | verdict |
|---|---|---|---|
| **SIO** (bare-sat) | **innocent** | 0 missed unsat (both 0 unsat) | wall moved — claim it |
| **pizza** (pair-sub) | **load-bearing** | 114/695 missed, all cardinality/∀ | gated on H3 |

The lesson: a fast hyper result is only a win once answer-agreement
against a reference reasoner rules out the axiom-drop confound. SIO
passed; pizza is pending H3.

## §H3a — antecedent DNF-distribution (shipped)

The first slice of H3, and the cheapest. The 114 pizza misses split
into three *different* mechanisms (counted by the superclass not
derived): VegetarianTopping (37), the VegetarianPizza/NonVegetarian
family (~52, antecedent-`∀` from `¬∃`), and InterestingPizza (20,
min-cardinality). VegetarianTopping is the odd one out — it needs no
engine work at all.

`VegetarianTopping ≡ PizzaTopping ⊓ (Cheese ⊔ Fruit ⊔ … ⊔ Vegetable)`.
The `⊒` direction puts a covering `Or` *inside* an antecedent
conjunction. The old `encode_antecedent` returned a single
conjunction and bailed (`None`) on a nested `Or`. H3a makes it return
**disjunctive normal form** — a list of alternative bodies — and
`clausify_gci` emits one consequent clause per alternative:
`A ⊓ (B ⊔ C) ⊑ D` → `A⊓B → D`, `A⊓C → D`. These are **Horn**, so the
engine is untouched. `And` is a cross-product, `Or` a union, `∃`
recurses (one fresh `y` per occurrence, shared across that
occurrence's alternatives — sound because each alternative is a
separate, variable-local clause). A cross-product cap
(`ANTECEDENT_DNF_CAP = 64`) defers pathological blow-ups rather than
exploding the clause set.

**Validated on pizza (vs Konclude closure):** misses **114 → 77**
(the entire VegetarianTopping family of 37 unlocked, as predicted),
subsumptions 581 → 618, deferred 17 → 16. Still **0 false positives**;
completeness **84 % → 89 %**. The residual 77 are exactly the two
harder mechanisms, each deserving its own scoped phase when started:

- **antecedent-`∀`** (~52): `VegetarianPizza ≡ Pizza ⊓ ¬∃hasTopping.Fish
  ⊓ ¬∃hasTopping.Meat`; after NNF the antecedent has `∀hasTopping.¬Fish`
  — a universal in the clause *body*, which standard DL-clauses can't
  express directly. Needs the hypertableau `∀`-in-body mechanism.
- **min-cardinality** (20): `InterestingPizza ≡ Pizza ⊓ ≥3 hasTopping`
  — the non-deterministic `≤n`/`≥n` machinery (successor merging),
  which interacts with branching.

Neither is "tune heuristics"; both are real clausifier+engine work.
H3a confirms the diagnosis was causal and clears the cheap third of
the pizza gap with zero engine risk.

**H3b is shipped** (scoped in
[`hypertableau-h3b-scoping.md`](hypertableau-h3b-scoping.md)): the
antecedent-`∀`/`¬` family is unlocked not by deriving universals
positively but by expanding `¬sup` in NNF and asserting it **Q-gated**
at the root (`Q(x) → d1 ∨ … ∨ dk`), with minimal negative literals
(complement classes, only for atoms under `Not`) and structural names
for `∃R.(⊓literals)`. The engine is untouched — it's all probe-side
encoding. Validated vs Konclude: pizza misses **77 → 29**, 48
unlocked (the entire antecedent-`∀`/`¬` family), **0 false positives**,
completeness **89 % → 95.8 %**.

**Multi-role body matching is shipped** (the first engine change since
H2). The clausifier already *emitted* Horn clauses with chained role
atoms (`A(x) ∧ R(x,y) ∧ B(y) ∧ S(y,z) ∧ C(z) → D(x)` — the
`SpicyPizzaEquivalent` shape), but `match_body` only matched a single
role atom. It now enumerates every homomorphism of the body's
variable-tree into the graph: `eval_order` topologically sorts the
role atoms (each non-`X` var is the target of one role atom whose
source is already bound — a tree rooted at `X`), and a recursive
descent binds successors and checks class constraints. The single-var
`Ybind` became a sorted `Binding = Vec<(Var, HNode)>`; unsupported
shapes (equality, non-tree, > `MAX_BODY_VARS`) return `None` (deferred,
counted). Validated vs Konclude: pizza misses **29 → 24**
(SpicyPizzaEquivalent's 5), **0 false positives**, completeness
**95.8 % → 96.5 %**.

The clean residual 24 is the two genuinely-hard remaining mechanisms:
min-cardinality 20 (`InterestingPizza ≡ ≥3 hasTopping`, H3c — the
`≤n`/`≥n` successor merging, interacts with branching), and nominals 4
(`RealItalianPizza`'s `hasValue` + the two pizzas reaching
`ThinAndCrispyPizza` transitively through it).

## Search quality — profiling toward H4 (the Konclude gap)

With pizza at 96.5 % completeness, the remaining barrier to flipping
the engine on (H4) is speed: SIO bare-sat is ~116× slower than
Konclude. Before guessing the fix (trail vs heuristics vs other), the
engine was instrumented (`SearchStats`: `match_attempts`,
`node_clones`, `fixpoint_passes`) and run on the full SIO `hyper-sat`
load (`perf`/samply unavailable — `perf_event_paranoid=4`, no sudo —
so operation counters, not a sampler).

**The profile is decisive and overturns the trail hypothesis:**

| counter | SIO bare-sat (1585 classes, ~23 s) |
|---|---|
| **match_attempts** | **1 350 534 374** (clause×node Horn match tries) |
| node_clones | 2 147 |
| fixpoint_passes | 21 768 |

The cost is **1.35 billion `match_body` attempts** — the Horn fixpoint
re-scans all 2 474 clauses at every node on every pass, with no index
on which clauses could possibly fire. `node_clones` is 2 147, so the
save/restore-vs-trail question is irrelevant here; the trail would
save nothing. This is clause-iteration, not branching cost.

**Next target — semi-naive Horn evaluation with clause indexing.**
Index clauses by the body class atoms (the trigger); when a node gains
a label, only re-attempt clauses whose body mentions that label
(semi-naive / given-clause evaluation), instead of re-scanning the
whole clause set every pass. This directly attacks the 1.35 B. (A
secondary, smaller win: `match_body` now allocates per call — the
multi-role refactor's `eval_order` builds `bound`/`order`/`used` Vecs
on the hot path; avoid for the common 0–1 role body.) This is the
data-chosen H4-enabling increment, not cardinality (H3c) or nominals.

## 9. Recommended entry point

Phase H0 (clausifier + `clause-stats`) is the natural first
session: it's self-contained, adds no reasoning risk (old path
untouched), and its output immediately tells us the clause-shape
distribution of the corpus — the hypertableau analog of the
`tbox-stats` measurement that grounded lazy unfolding. Start there
when the multi-month effort begins.

## References

- Motik, Shearer, Horrocks. *Hypertableau Reasoning for
  Description Logics.* JAIR 36 (2009), 165–228.
- [`outperform-hermit-plan.md`](outperform-hermit-plan.md) — the original strategy framing.
- [`pizza-convergence-diagnosis.md`](pizza-convergence-diagnosis.md) — why branching, not blocking, is the bottleneck.
- [`cdbl-plan.md`](cdbl-plan.md) — the learning approach that's sound but bounded by the same convergence wall.
