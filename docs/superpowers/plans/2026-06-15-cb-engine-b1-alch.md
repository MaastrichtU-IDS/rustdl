# Consequence-Based Engine B1 (ALCH) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan. Steps use checkbox (`- [ ]`) syntax. **This plan is structured for PARALLEL execution:** Task 0 (interface freeze) is sequential and MUST land first; Tasks A/B/C/D then run **simultaneously** in separate worktrees (they touch disjoint files and code against Task 0's frozen interface); Task E integrates + gates.

**Goal:** A new `owl-dl-cb` crate implementing a consequence-based ALCH classification engine (global, no per-pair probing, no tableau), sound + complete for ALCH, run side-by-side with the current hybrid and validated by differential equivalence on the ALC fixtures.

**Architecture:** Consume the post-NNF `InternalOntology` IR → normalize to clausal form (`⊓Aᵢ ⊑ ⊔Lⱼ`) → saturate a graph of **contexts** (core + derived disjunctive clauses) via the Simančík–Kazakov–Horrocks / Bate-et-al inference rules → read the hierarchy off the saturated graph. Out-of-ALCH ⇒ `OutOfFragment` (orchestrator defers). Gated `RUSTDL_CB_ENGINE`, default OFF.

**Tech Stack:** Rust (edition 2024). Reuses `owl-dl-core` IR (`ConceptPool`, `ConceptExpr`, `ClassId`, `Role`, `InternalOntology`, `Axiom`). No tableau/saturation dependency.

**Spec:** `docs/superpowers/specs/2026-06-15-cb-engine-b1-alch-design.md`

**Build/test prelude** (no `cargo` on PATH):
```sh
export PATH="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"
```

**Cardinal contract:** sound AND complete for ALCH ⇒ the CB hierarchy must EQUAL the current sound+complete hybrid's on every ALC ontology (FP=0 AND MISSED=0). The differential-equivalence gate (Task E) is the empirical proof. Out-of-ALCH constructs must route to `OutOfFragment` — never a CB answer.

**IR reference (verify against `crates/owl-dl-core/src/ir.rs`):**
`ConceptExpr = Top | Bot | Atomic(ClassId) | Nominal(IndividualId) | SelfRestriction(Role) | Not(ConceptId) | And(Box<[ConceptId]>) | Or(Box<[ConceptId]>) | Some(Role,ConceptId) | All(Role,ConceptId) | Min(u32,Role,ConceptId) | Max(u32,Role,ConceptId)`. `ConceptPool::get(id) -> &ConceptExpr`; builders `atomic/not/and/or/some/all/top/bot`. `InternalOntology { concepts: ConceptPool, axioms: Vec<Axiom>, vocabulary }`. `Axiom = SubClassOf{sub,sup} | EquivalentClasses(Vec<ConceptId>) | …`.

---

## Task 0 — Interface freeze (SEQUENTIAL; the enabler for parallelism)

**Files:**
- Create: `crates/owl-dl-cb/Cargo.toml`, `crates/owl-dl-cb/src/lib.rs`, `crates/owl-dl-cb/src/model.rs`, `crates/owl-dl-cb/src/normalize.rs`, `crates/owl-dl-cb/src/engine.rs`, `crates/owl-dl-cb/src/classify.rs`
- Modify: root `Cargo.toml` workspace members

This task defines the frozen contract A/B/C/D build against. It compiles (with `todo!()`/stub bodies) but does nothing yet.

- [ ] **Step 1: Create the crate + register in workspace**

`crates/owl-dl-cb/Cargo.toml`:
```toml
[package]
name = "owl-dl-cb"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
owl-dl-core = { workspace = true }
hashbrown = { workspace = true }

[lints]
workspace = true
```
Add `"crates/owl-dl-cb"` to root `Cargo.toml` `[workspace] members` (and `default-members` if appropriate).

- [ ] **Step 2: `model.rs` — the FROZEN shared types** (all parallel agents depend on these; do not change them after this task without re-syncing the agents)

```rust
//! Consequence-based context/clause data model (frozen interface).
use owl_dl_core::ir::{ClassId, ConceptId, Role};
use std::collections::BTreeSet;

/// A normalized clause body atom: an atomic concept that must hold.
pub type Atom = ConceptId; // invariant: pool.get(_) is Atomic | Top

/// A normalized clause head literal: atomic `B`, `∃R.B`, or `∀R.B` (B atomic).
/// Represented as the interned ConceptId of that literal.
pub type Literal = ConceptId;

/// A normalized ontology clause `⊓ premise ⊑ ⊔ head`.
/// Empty `head` = `⊑ ⊥`. Empty `premise` = `⊤ ⊑ …`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OntClause {
    pub premise: Vec<Atom>,
    pub head: Vec<Literal>,
}

/// A clause derived *within a context* — `premise → ⊔ head` (premise atoms
/// hold in the context's core/derived set).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DerivedClause {
    pub premise: Vec<Atom>,   // sorted, deduped
    pub head: Vec<Literal>,   // sorted, deduped (empty = ⊥)
}

pub type ContextId = usize;

/// A context: reasoning about an element whose core conjunction holds.
#[derive(Clone, Debug, Default)]
pub struct Context {
    pub core: BTreeSet<Atom>,          // the conjunction defining this context
    pub clauses: Vec<DerivedClause>,   // derived sequents (deduped via `seen`)
    pub seen: BTreeSet<DerivedClause>, // membership guard for `clauses`
    pub succ: Vec<(Role, ContextId)>,  // ∃-generated successor edges
}

/// The saturated context graph.
#[derive(Default)]
pub struct ContextGraph {
    pub contexts: Vec<Context>,
    pub by_core: hashbrown::HashMap<BTreeSet<Atom>, ContextId>,
}
```

- [ ] **Step 3: `lib.rs` — the FROZEN public API**

```rust
//! Consequence-based ALCH classification engine (Arch B, slice B1).
//! Sound+complete for ALCH; returns `OutOfFragment` otherwise. See
//! docs/superpowers/specs/2026-06-15-cb-engine-b1-alch-design.md.
mod model;
mod normalize;
mod engine;
mod classify;

pub use model::{Atom, Literal, OntClause, DerivedClause, Context, ContextGraph, ContextId};

use owl_dl_core::ontology::InternalOntology;

/// Outcome of a CB classification attempt.
pub enum CbOutcome {
    /// ALCH input: a sound+complete class hierarchy (direct atomic subsumptions).
    Classified(CbHierarchy),
    /// Input uses a construct outside ALCH (≤n/≥n, inverse, nominal, datatype,
    /// role chain/transitivity, Self) — caller must defer to another engine.
    OutOfFragment(&'static str), // reason
}

/// Atomic-class subsumption result: for each class, its (direct + told) atomic
/// subsumers, plus the unsatisfiable set. Comparable to the reasoner's
/// `Classification`. (Transitive closure computed by the consumer/harness.)
#[derive(Debug, Default)]
pub struct CbHierarchy {
    /// (sub, sup) atomic-class subsumption pairs (sub ⊑ sup), excluding
    /// reflexive and Top/Bot, over the *full* (transitively closed) relation.
    pub subsumptions: std::collections::BTreeSet<(ClassId, ClassId)>,
    /// Classes proven unsatisfiable.
    pub unsat: std::collections::BTreeSet<ClassId>,
}
use owl_dl_core::ir::ClassId;

/// Classify `internal` with the consequence-based engine.
pub fn classify(internal: &InternalOntology) -> CbOutcome {
    match normalize::normalize(internal) {
        Err(reason) => CbOutcome::OutOfFragment(reason),
        Ok(norm) => {
            let graph = engine::saturate(&norm);
            CbOutcome::Classified(classify::read_hierarchy(&norm, &graph))
        }
    }
}
```

- [ ] **Step 4: module stubs** (compile with `todo!()`), defining the FROZEN signatures A/B build against:

`normalize.rs`:
```rust
use owl_dl_core::ontology::InternalOntology;
use crate::model::OntClause;
/// Normalized ALCH ontology: clausal axioms + the atomic-class vocabulary.
pub struct Normalized {
    pub clauses: Vec<OntClause>,
    pub classes: Vec<owl_dl_core::ir::ClassId>,            // reportable atomic classes
    pub role_hierarchy: Vec<(owl_dl_core::ir::Role, owl_dl_core::ir::Role)>, // R⊑S (for ∀-prop)
    pub pool: owl_dl_core::ir::ConceptPool,                // owned, may gain definitional atoms
}
/// Normalize to ALCH clausal form, or `Err(reason)` if out of fragment.
pub fn normalize(_internal: &InternalOntology) -> Result<Normalized, &'static str> { todo!() }
```
`engine.rs`:
```rust
use crate::{model::ContextGraph, normalize::Normalized};
/// Saturate the context graph under the consequence-based ALCH rules.
pub fn saturate(_norm: &Normalized) -> ContextGraph { todo!() }
```
`classify.rs`:
```rust
use crate::{model::ContextGraph, normalize::Normalized, CbHierarchy};
/// Read the atomic-class hierarchy off the saturated graph.
pub fn read_hierarchy(_norm: &Normalized, _graph: &ContextGraph) -> CbHierarchy { todo!() }
```

- [ ] **Step 5: verify it builds + commit**

Run: `cargo build -p owl-dl-cb` → Finished (todo!() bodies compile).
```bash
git add crates/owl-dl-cb Cargo.toml
git commit -m "feat(cb): freeze owl-dl-cb B1 interface (crate skeleton, model, public API, stubs)"
```

---

## PARALLEL GROUP — Tasks A, B, C, D run SIMULTANEOUSLY (separate worktrees, disjoint files, all off the Task-0 commit)

### Task A — `normalize.rs`: IR → ALCH clausal NF + fragment gate

**Files:** Modify `crates/owl-dl-cb/src/normalize.rs`; Test inline `#[cfg(test)]`.

- [ ] **Step 1: failing tests** (clausal NF + fragment gate). Cover: `A ⊑ B ⊓ C` → two clauses `{A}⊑{B}`,`{A}⊑{C}`; `A ⊑ B ⊔ C` → `{A}⊑{B,C}`; `A ⊓ B ⊑ C`; `A ⊑ ∃R.B` / `∀R.B`; nested `A ⊑ ∃R.(B⊔C)` → definitional atom `X`, `{A}⊑{∃R.X}`,`{X}⊑{B,C}`; `¬`/NNF; `EquivalentClasses` → both directions; `⊑⊥`. Fragment gate: `Max`/`Min`/inverse `Role`/`Nominal`/`SelfRestriction`/datatype(DKey)/role-chain/transitive → `Err`.
- [ ] **Step 2: run, see fail.**
- [ ] **Step 3: implement** the structural transformation: recurse `ConceptExpr`; flatten `And` on the left, `Or` on the right; introduce a fresh definitional `ClassId`+`Atomic` for any non-literal subconcept (cache by ConceptId to dedup); emit clauses; the fragment gate returns `Err(reason)` on first out-of-ALCH construct (`Min`/`Max`/`Nominal`/`SelfRestriction`/inverse `Role`/datatype DKey IRI/`TransitiveRole`/role-chain `SubObjectPropertyOf(Chain..)`). Role hierarchy (`SubObjectPropertyOf{Role}`) is RECORDED into `Normalized.role_hierarchy` (the frozen field from Task 0) — not a clause — for the engine's `∀`-propagation.
- [ ] **Step 4: run, see pass.**
- [ ] **Step 5: fmt+clippy `-D warnings`; commit** `feat(cb): ALCH clausal normalization + fragment gate`.

### Task B — `engine.rs` + `classify.rs`: the consequence-based calculus (CRITICAL PATH; opus)

**Files:** Modify `crates/owl-dl-cb/src/engine.rs`, `crates/owl-dl-cb/src/classify.rs`; Test inline.

This is the calculus. Implement the consequence-based ALCH inference rules (Simančík–Kazakov–Horrocks 2011; Bate et al. 2016 §ALCH restriction). Derive the rules faithfully; the tests + Task E's differential gate are the correctness net.

- [ ] **Step 1: failing unit tests** — the disjunctive/∀ cases the EL saturator CANNOT do (these are the headline):
  - `A⊑B⊔C, B⊑D, C⊑D ⟹ A⊑D` (reasoning by cases).
  - `A⊑∀R.B, A⊑∃R.C, C⊓B⊑⊥ ⟹ A⊑⊥`.
  - `A⊑∃R.C, C⊑⊥ ⟹ A⊑⊥` (⊥ up ∃).
  - `∀`-propagation over role hierarchy: `A⊑∀S.B, A⊑∃R.C, R⊑S ⟹ C gets B`.
  - Pure-EL still correct: `A⊑∃R.B, B⊑C, ∃R.C⊑D ⟹ A⊑D`.
  Assert via `read_hierarchy(...).subsumptions` / `.unsat`.
- [ ] **Step 2: run, see fail.**
- [ ] **Step 3: implement the calculus.** Data flow:
  - `saturate(norm)`: seed one root `Context` per reportable class `A` (`core={A}`); worklist of contexts/clauses; drain to fixpoint.
  - **Core resolution:** when a context's derived atoms satisfy an `OntClause` premise, add its head as a `DerivedClause`.
  - **`⊔` ordered resolution:** resolve disjunctive `DerivedClause`s against derived atoms + each other under a fixed atom order (so the closure is finite + refutationally complete).
  - **`∃`-Succ:** a derived `∃R.B` literal ⇒ find-or-create successor context (`by_core` reuse) with `B` in core; record edge.
  - **`∀`-Pred:** a derived `∀R.B` (or `∀S.B`, `R⊑S` via `norm.role_hierarchy`) on a context with an `R`-edge ⇒ propagate `B` to the successor.
  - **`⊥`:** a `DerivedClause` with empty head whose premise holds ⇒ context core unsatisfiable.
  - Termination: contexts reused by core (`by_core`); per-context clause set deduped via `Context::seen`.
  - `read_hierarchy`: for each class `A`, `A⊑B` iff B derivable in A's root context (the closure entails `⊤→B`); collect transitively; `unsat` = classes whose root context derived `⊥`. Exclude reflexive + Top/Bot.
- [ ] **Step 4: run, see pass.**
- [ ] **Step 5: fmt+clippy; commit** `feat(cb): consequence-based ALCH calculus (contexts + ⊔/∃/∀/⊥ rules) + hierarchy read-off`.
- [ ] **Step 6: DONE_WITH_CONCERNS report** flagging any rule whose completeness you're unsure of (Task E's opus review + differential gate will scrutinize).

### Task C — side-by-side harness (cb-diff + `rustdl classify --cb`)

**Files:** Modify `crates/owl-dl-bench/src/main.rs` (add `cb-diff` subcommand); `crates/owl-dl-cli/src/...` (add `--cb` flag to `classify`); Test inline.

- [ ] **Step 1:** failing test for a `cb_diff(internal) -> DiffReport` helper (in `owl-dl-bench`): runs `owl_dl_cb::classify` and the current `owl_dl_reasoner` classify, returns `{ cb_outcome, identical: bool, only_in_cb: Vec<(String,String)>, only_in_current: Vec<(String,String)>, cb_wall_ms, cur_wall_ms }`. Test on a tiny ALC ontology (assert identical).
- [ ] **Step 2: run, see fail** (cb_diff undefined).
- [ ] **Step 3: implement** `cb_diff`: parse → `convert_ontology` to `InternalOntology` → `owl_dl_cb::classify`; if `OutOfFragment`, report that + skip diff; else compute both hierarchies' transitive closures, diff the (sub,sup) sets, time each, capture RSS if available. Wire `cb-diff <path>` subcommand (prints the report) and a `rustdl classify --cb` flag (uses the CB engine, errors-with-message if OutOfFragment).
- [ ] **Step 4: run, see pass.**
- [ ] **Step 5: fmt+clippy; commit** `feat(bench): cb-diff side-by-side harness + classify --cb`.

### Task D — test suite: negatives-first canaries + ALC differential fixtures

**Files:** Create `crates/owl-dl-cb/tests/cb_alch.rs`, `crates/owl-dl-cb/tests/cb_fragment_gate.rs`.

- [ ] **Step 1:** write integration canaries against the public `owl_dl_cb::classify` API (parse OFN → `convert_ontology` → `classify`). Disjunctive subsumption, ∀+∃+¬ unsat, by-cases unsat, role-hierarchy ∀-prop, EL-still-correct (mirror Task B's cases at the public-API level). Fragment-gate: `≤n`/inverse/nominal/datatype/Self/chain/transitive each → `OutOfFragment(reason)`.
- [ ] **Step 2: run** (fail until B+A land — that's expected for the parallel build; the tests are the contract).
- [ ] **Step 3:** no impl (tests only); ensure they compile against the frozen API.
- [ ] **Step 4: commit** `test(cb): ALCH canaries + fragment-gate suite` (tests may be red until integration — note in commit).

---

## Task E — integration, differential gate, opus review (SEQUENTIAL, after A–D)

**Files:** none new (verification + fixes).

- [ ] **Step 1: merge A/B/C/D worktrees** onto the Task-0 base; resolve any `Normalized`/`model.rs` drift (should be none if the freeze held). Build workspace.
- [ ] **Step 2: all `owl-dl-cb` tests green** — `cargo test -p owl-dl-cb` (Task B unit + Task D canaries). Fix integration gaps.
- [ ] **Step 3: THE DIFFERENTIAL GATE.** Run `cb-diff` on **alehif** (ALC; `ontologies/external/alehif-test.ofn`) and any ALC-fragment fixtures. Required: **`identical: true`** (CB hierarchy == current hybrid's). Also FP=0/MISSED=0 vs the oracle (`alehif-test-classified.owx`, 247 pairs). If not identical, the calculus has a soundness or completeness bug — diagnose (`only_in_cb` = FP/over-derivation; `only_in_current` = MISSED/incompleteness); STOP and fix before proceeding.
- [ ] **Step 4: fragment-gate corpus safety** — run `cb-diff` on the non-ALC fixtures (wine, ore-10908, ore-15672, sio, shoiq) and confirm each reports `OutOfFragment` (never a wrong CB hierarchy).
- [ ] **Step 5: perf** — record cb wall vs current wall on alehif (the head-to-head; the whole point of "side by side").
- [ ] **Step 6: clippy `-D warnings` workspace; fmt.**
- [ ] **Step 7: INDEPENDENT OPUS REVIEW** of the calculus (`engine.rs`): soundness + completeness of each inference rule, the ordered-resolution termination argument, and the `read_hierarchy` correctness — the differential gate is empirical, the opus review is the structural check. Address findings; re-run Step 3.
- [ ] **Step 8: commit** the integration + any fixes; do NOT flip `RUSTDL_CB_ENGINE` default (stays OFF; B1 is comparison-only).

---

## Notes for the executor
- **Default OFF, comparison-only.** B1 proves the architecture + competitiveness on ALC; it is NOT the production default. Growing to B2+ or defaulting is gated on B1's differential + perf results.
- **The fragment gate is FP-critical-adjacent:** an out-of-ALCH ontology must NEVER get a CB hierarchy (it could be wrong) — it must `OutOfFragment`. Exhaustive gate canaries (Task D) + Step 4 are the guard.
- **The differential-equivalence gate (Step 3) is the real correctness proof** — "both FP=0" is insufficient; the hierarchies must be byte-identical on ALC.
- Parallelism: A/B/C/D are disjoint-file + interface-frozen. B is the long pole (the calculus). C/D code against the frozen public API and may be red until B/A land — that's expected; they encode the contract.
