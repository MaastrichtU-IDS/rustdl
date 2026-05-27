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
