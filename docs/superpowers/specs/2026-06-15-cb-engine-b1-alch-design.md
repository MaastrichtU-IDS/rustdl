# Consequence-based engine — B1 (ALCH core) — Design

**Date:** 2026-06-15
**Status:** Approved (brainstorming + user), pre-plan
**Author:** rustdl (Michel Dumontier + Claude)

## Context & goal

This is the first slice of **Architecture B** ("the north star" from
`spec/global-model-rewrite` §6): a from-scratch **consequence-based** SROIQ
classification engine that decides the whole hierarchy globally — no per-pair
satisfiability probing, no tableau, no backtracking — to be run **side by side**
with the current per-pair tableau/wedge hybrid.

The existing EL saturator (`owl-dl-saturation`) is consequence-based but
**complete only for EL** — it silently drops disjunction, `∀`, complement, and
cardinality (those fall to the tableau). The Sequoia / Simančík–Kazakov–Horrocks
consequence-based calculus handles disjunction via **context structures** with
*disjunctive clauses* and ordered resolution — a different data model the EL
subsumer-closure cannot express. So this is a **new engine** (new crate), with
the EL saturator as the disjunction-free special case.

**Arch B is decomposed by expressivity** (each slice = its own spec→plan→build):

| Slice | Fragment | New capability | Side-by-side target |
|---|---|---|---|
| **B1 (this spec)** | **ALCH** | context/clause core + `⊔` + `∀` + `¬` | alehif (ALC) + synthetics |
| B2 | ALCHQ | `≤n`/`≥n` | shoiq-knowledge, wine |
| B3 | SHIQ | inverse roles | ore-15672, ore-10908 |
| B4 | SROIQ | nominals + RBox (chains/transitivity) | full corpus parity |

**B1 goal:** a new crate `owl-dl-cb` implementing the consequence-based ALCH
calculus, sound **and** complete for ALCH, run side-by-side with the current
engine and validated by **differential equivalence** (identical hierarchy) on
the ALC fixtures + FP=0/MISSED=0 vs the oracle. Default OFF, opt-in,
comparison-only — the existing engine remains the production default.

## 1. Architecture & data model

New crate **`owl-dl-cb`**, fully independent of `owl-dl-saturation` /
`owl-dl-tableau`. It consumes the existing post-NNF `InternalOntology` IR
(`ConceptPool`, `ClassId`, `Role`) — no new front-end — and produces a
`Classification` comparable to the current engine's.

**Front-end — clausal normalization.** ALCH axioms normalize to clauses
`⊓ᵢ Aᵢ ⊑ ⊔ⱼ Lⱼ`, where each `Aᵢ` is an atomic concept and each `Lⱼ` is a
**literal**: atomic `B`, `∃R.B`, or `∀R.B`. NNF (already done by the IR) pushes
`¬` onto atoms; nested/compound concepts are named via a **structural
transformation** (introduce a fresh definitional atom `X` with `X ≡ <subconcept>`)
so every clause is flat (atoms on the left, literals on the right). `⊥`/`⊤`
handled as the empty disjunction / empty conjunction.

**Fragment gate (runs first).** Any construct outside ALCH — `≤n`/`≥n`
(`Max`/`Min`), inverse role, nominal (`{a}` / `ObjectHasValue` / NomKey),
datatype/DKey, role chain, transitive/other role characteristics beyond
hierarchy — ⇒ the engine returns **`OutOfFragment`** and the orchestrator defers
to the existing engine. (Role hierarchy `R⊑S` IS in ALCH and handled.)

**Core types:**
- `Context { core: ConceptSet, clauses: Vec<Clause>, succ: Vec<(Role, ContextId)> }`
  — reasoning about an element satisfying `core`.
- `Clause { premise: ConjOfAtoms, conclusion: DisjOfLiterals }` — a derived sequent
  (`premise → conclusion`). The premise atoms are drawn from the context's
  core/derived atoms; the conclusion is a disjunction of literals.
- `ContextGraph { contexts: Vec<Context>, by_core: HashMap<ConceptSet, ContextId>,
  worklist }` — contexts are **reused by core** (the termination + sharing key).
  Classification seeds one **root context per atomic class** with `core = {A}`.

## 2. The calculus (consequence-based ALCH)

A fixpoint over contexts. **No backtracking, no per-pair probing.** Inference
rules (Simančík–Kazakov–Horrocks 2011 "Consequence-Based Reasoning beyond Horn";
Bate et al. 2016 "Extending Consequence-Based Reasoning to SROIQ", restricted to
ALCH):

- **Core resolution (`R⊑` / hyperresolution):** if context `v` has derived every
  `Aᵢ` of an ontology clause `⊓Aᵢ ⊑ ⊔Lⱼ`, derive the clause `⊤ → ⊔Lⱼ` in `v`.
  (The EL Horn rules are the `k=1` disjunction-free special case.)
- **`⊔` resolution (ordered):** disjunctive clauses resolve against each other
  and against derived atoms — ordered resolution (a fixed atom order) keeps the
  derivation terminating and refutationally complete. This is the disjunctive
  reasoning the EL saturator lacks (reasoning by cases).
- **`∃`-Succ:** if `v` derives `∃R.B`, find-or-create a successor context `u`
  with `B ∈ core(u)`; add edge `v —R→ u`. (Reuse by core ⇒ no infinite tree.)
- **`∀`-Pred (propagation):** if `v` derives `∀R.B` (or `∀S.B` with `R⊑S`) and
  `v —R→ u`, propagate `B` into `u`'s core/clauses.
- **`⊥`:** a context deriving an atom `A` and a clause `A → ⊥` (i.e. `A` together
  with `¬A`) derives `⊥` — its core is unsatisfiable; classify reports the core's
  atoms appropriately (an unsatisfiable class subsumes/is-subsumed per the
  standard `⊥` semantics).

**Classification:** for atomic classes `A`, `B`: `A ⊑ B` iff `B` is a consequence
in `A`'s root context (`⊤ → B` derivable, i.e. every model of the core entails
`B`). Read for all classes from the saturated context graph — globally.

**Termination:** contexts are reused by core ⇒ finitely many cores ⇒ finitely
many contexts; per-context clause sets are bounded by the (finite) literal
vocabulary ⇒ the fixpoint terminates. **Completeness for ALCH** is the calculus's
published guarantee; the empirical proof is the differential gate (§4).

## 3. Classification interface & side-by-side orchestration

**Public API:** `owl_dl_cb::classify(&InternalOntology) -> CbOutcome`, where
`CbOutcome = Classified(Classification) | OutOfFragment`. `Classified` only for
ALCH inputs.

**Gate `RUSTDL_CB_ENGINE` (default OFF).** When ON and the ontology is ALCH,
classify routes through the CB engine; otherwise (gate off, or `OutOfFragment`)
the existing per-pair hybrid runs **unchanged**. The two engines are fully
independent.

**Comparison harness — the "two engines side by side" deliverable:** a bench/CLI
path (`owl-dl-bench cb-diff <onto>` and/or `rustdl classify --cb`) that runs
**both** engines on the same ontology and reports: hierarchies identical
(differential equivalence)? per-engine wall + peak RSS. The head-to-head on the
ALC fixtures is the B1 outcome.

## 4. Soundness/completeness contract & testing

**Contract:** sound **and** complete for ALCH. On any ALCH ontology the CB
hierarchy must **equal** the current sound+complete hybrid's. FP=0 **and**
MISSED=0 are both required.

**Tests (negatives-first; the disjunctive cases the EL saturator provably misses
are the headline):**
- **Disjunctive subsumption:** `A⊑B⊔C, B⊑D, C⊑D ⟹ A⊑D` (CB finds it; EL saturator
  cannot).
- **`∀`+`∃`+`¬` clash → unsat:** `A⊑∀R.B, A⊑∃R.C, C⊓B⊑⊥ ⟹ A⊑⊥`.
- **Reasoning-by-cases unsat; role-hierarchy `∀`-propagation; `⊥`-propagation up
  `∃`.**
- **Fragment-boundary canaries:** `≤n`/inverse/nominal/datatype/chain inputs →
  `OutOfFragment` (orchestrator defers; never a wrong answer from out-of-fragment).
- **Headline differential gate:** CB hierarchy **==** current-engine hierarchy on
  **alehif** (ALC, 247 classes) + the ALC-fragment of the corpus; plus FP=0/MISSED=0
  vs the oracle (`alehif-test-classified.owx`, 247 pairs).
- **Independent opus review** of the calculus implementation (soundness +
  completeness of the inference rules + termination argument) before merge.

## 5. Scope & non-goals (B1)

- **In:** ALCH = `⊓ ⊔ ¬ ∃ ∀` + role hierarchy + `⊑/≡/⊥/⊤`; classification
  (consistency as a by-product).
- **Out (later slices):** `≤n`/`≥n` (B2), inverse roles (B3), nominals + RBox
  chains/transitivity (B4), datatypes. All → `OutOfFragment` in B1.
- **Default OFF**, opt-in, comparison-only. The existing engine remains the
  production default. Growing the engine (B2+) or making it default is gated on
  B1's side-by-side results.

## Risks
- **Calculus correctness** (soundness+completeness+termination of the inference
  rules) is the core risk — mitigated by the differential-equivalence gate on
  real ALC + negatives-first canaries + the independent opus review. A complete
  consequence-based ALCH calculus is well-established in the literature; the risk
  is implementation fidelity, not the theory.
- **Structural-transformation / normalization fidelity** — getting clausal NF
  exactly equisatisfiable; canaries on nested/compound concepts guard it.
- **Scope creep** — the fragment gate must be airtight so out-of-ALCH never
  produces a (possibly wrong) CB answer; it defers instead.
