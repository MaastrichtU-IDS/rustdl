# SP-A: Approximated Saturation (forced-disjunct) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a sound, precomputed **forced-disjunct** inference over atomic-class disjunctions (`C ⊑ D₁⊔…⊔Dₙ`: drop disjuncts told-disjoint with C's context; one survivor ⟹ `C ⊑ Dₖ`; none ⟹ `C ⊑ ⊥`), feeding the existing saturator/wedge so disjunctive structure is resolved before the tableau branches.

**Architecture:** A new preprocessing pass `derive_forced_disjuncts` in `owl-dl-core`, run in `convert_ontology` right after the existing `derive_disjunction_existentials` (which already implements the companion common-disjunct rule). It reads the transitively-closed told tables and appends derived `Axiom::SubClassOf` axioms. **No engine change** — the saturator already turns `Atomic ⊑ Bot` into unsat (Phase D4) and `C ⊑ E` into a told subsumption.

**Tech Stack:** Rust (edition 2024, 1.88+), `owl-dl-core` crate. Spec: `docs/superpowers/specs/2026-06-23-approximated-saturation-sp-a-design.md`.

## Global Constraints

- **FP=0 is sacred.** Every closure change must be additive (recovered MISSED) and oracle-sound; no spurious subsumption/unsat.
- Scope: **atomic-class disjuncts only.** If any disjunct is non-atomic (esp. `Nominal`), emit nothing for that disjunction (nominal value-partition forcing is a deferred, separately-gated increment — the SP1 increment-3 FP lived there).
- `cargo fmt --all -- --check` clean; `cargo clippy --workspace --all-targets --all-features -- -D warnings` clean (pedantic on); `cargo test --workspace` green.
- Branch off `main`: `feat/approx-saturation-sp-a`.
- Toolchain on PATH: `export RUSTUP_HOME=/home/dumontier/.rustup CARGO_HOME=/home/dumontier/.cargo; export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$CARGO_HOME/bin:$PATH"`.
- Commit only when the user asks. End commit messages with:
  `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>` and
  `Claude-Session: https://claude.ai/code/session_01HSzon7V2wkhrudxBNAJduh`.

## Pre-existing facts (verified 2026-06-23)

- `crates/owl-dl-core/src/disjunction_existential.rs:36` `pub fn derive_disjunction_existentials(onto: &mut InternalOntology)` — builds `told = build_told_tables(onto)`, iterates `&onto.axioms` (Phase 1, immutable), pushes derived axioms (Phase 2, mutable). Its `collect_from_sup`'s `ConceptExpr::Or(_)` arm ALREADY emits `C ⊑ E` (common-disjunct, Rule 1). Called at `convert.rs:2157`.
- `crates/owl-dl-core/src/told.rs`: `ToldTables::super_classes(c: ClassId) -> &[ClassId]` (told-subsumers, transitively closed), `are_told_disjoint(a: ClassId, b: ClassId) -> bool` (transitively closed), `build_told_tables(onto: &InternalOntology) -> ToldTables`.
- IR: `ConceptPool::get(ConceptId) -> &ConceptExpr`; `ConceptExpr::{Atomic(ClassId), Or(Vec<ConceptId>), Nominal(IndividualId)}`; `ConceptPool::bot() -> ConceptId` (cached). `Axiom::SubClassOf { sub: ConceptId, sup: ConceptId }`. `InternalOntology { axioms: Vec<Axiom>, concepts: ConceptPool, .. }`.

---

### Task 1: forced-disjunct pass + unit canaries

**Files:**
- Create: `crates/owl-dl-core/src/approx_saturation.rs`
- Modify: `crates/owl-dl-core/src/lib.rs` (add `pub mod approx_saturation;`)

**Interfaces:**
- Produces: `pub fn derive_forced_disjuncts(onto: &mut owl_dl_core::ontology::InternalOntology)` — appends derived `Axiom::SubClassOf` axioms in place. Idempotent-safe to call once after `derive_disjunction_existentials`.

- [ ] **Step 1: Write the module with failing unit tests**

Create `crates/owl-dl-core/src/approx_saturation.rs`:

```rust
//! SP-A: approximated saturation — forced-disjunct precomputation.
//!
//! For a GCI `C ⊑ D₁ ⊔ … ⊔ Dₙ` with **atomic** disjuncts: a disjunct `Dᵢ` is
//! *incompatible with C* iff `C` itself, or some told-subsumer `G` of `C`, is
//! told-disjoint from `Dᵢ`. Let `K` be the compatible disjuncts.
//!   * `|K| == 1` ⟹ emit `C ⊑ Dₖ` (the survivor is forced).
//!   * `|K| == 0` ⟹ emit `C ⊑ ⊥` (every disjunct clashes; `C` unsatisfiable).
//!   * `|K| ≥ 2` ⟹ emit nothing.
//!
//! Sound by construction: the told tables are a subset of true entailment, so a
//! disjunct is dropped only when genuinely entailed-disjoint — this can only
//! *miss* a forcing, never invent one. Companion of
//! [`crate::disjunction_existential`] (common-disjunct, Rule 1), which already
//! ships. Scope: **atomic disjuncts only** — any `Nominal` disjunct ⟹ skip the
//! whole disjunction (nominal value-partition forcing is a deferred increment).

use crate::ir::{ConceptExpr, ConceptId};
use crate::ontology::{Axiom, InternalOntology};
use crate::told::{ToldTables, build_told_tables};
use owl_dl_core_ir_classid_placeholder; // replaced below

/// Target of a forced disjunction: a specific surviving disjunct, or bottom.
enum Forced {
    Class(ConceptId),
    Bot,
}

/// Append derived `C ⊑ Dₖ` / `C ⊑ ⊥` forced-disjunct axioms to `onto`.
pub fn derive_forced_disjuncts(onto: &mut InternalOntology) {
    let told = build_told_tables(onto);
    // Phase 1 (immutable borrow): decide each atomic-disjunction GCI.
    let mut derived: Vec<(ConceptId, Forced)> = Vec::new();
    for ax in &onto.axioms {
        let Axiom::SubClassOf { sub, sup } = ax else {
            continue;
        };
        // `sub` must be atomic so its told-subsumers define the context.
        let ConceptExpr::Atomic(c) = onto.concepts.get(*sub) else {
            continue;
        };
        let ConceptExpr::Or(disjuncts) = onto.concepts.get(*sup) else {
            continue;
        };
        // Collect atomic disjuncts; bail (scope guard) on any non-atomic
        // (Nominal/compound) disjunct — no nominal value-partition forcing here.
        let mut atomic: Vec<(ConceptId, crate::ir::ClassId)> = Vec::with_capacity(disjuncts.len());
        let mut all_atomic = true;
        for &d in disjuncts {
            if let ConceptExpr::Atomic(did) = onto.concepts.get(d) {
                atomic.push((d, *did));
            } else {
                all_atomic = false;
                break;
            }
        }
        if !all_atomic {
            continue;
        }
        let c = *c;
        let survivors: Vec<ConceptId> = atomic
            .iter()
            .copied()
            .filter(|&(_, did)| !is_incompatible(c, did, &told))
            .map(|(cid, _)| cid)
            .collect();
        match survivors.len() {
            1 => derived.push((*sub, Forced::Class(survivors[0]))),
            0 => derived.push((*sub, Forced::Bot)),
            _ => {}
        }
    }
    if derived.is_empty() {
        return;
    }
    // Phase 2 (mutable borrow): intern Bot + push axioms.
    let bot = onto.concepts.bot();
    for (sub, target) in derived {
        let sup = match target {
            Forced::Class(cid) => cid,
            Forced::Bot => bot,
        };
        if sub != sup {
            onto.axioms.push(Axiom::SubClassOf { sub, sup });
        }
    }
}

/// `d` is incompatible with class `c` iff `c` itself or any told-subsumer of `c`
/// is told-disjoint from `d`.
fn is_incompatible(c: crate::ir::ClassId, d: crate::ir::ClassId, told: &ToldTables) -> bool {
    if told.are_told_disjoint(c, d) {
        return true;
    }
    told.super_classes(c).iter().any(|&g| told.are_told_disjoint(g, d))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::ConceptExpr;
    use crate::ontology::Axiom;

    // Test helper: build a tiny InternalOntology with named atomic classes and
    // push the given axioms, then run derive_forced_disjuncts and return the
    // axiom set as (sub_expr, sup_expr) pairs for assertion.
    // NOTE: implementer — use the crate's existing test constructors for
    // InternalOntology / ConceptPool (see told.rs tests, which build ontologies
    // directly). Mirror that exact construction style here.

    #[test]
    fn forced_disjunct_fires() {
        // C ⊑ A ⊔ B, C ⊑ G, Disjoint(G, A) ⟹ derive C ⊑ B.
        let (mut onto, ids) = build(&["C", "A", "B", "G"]);
        let (c, a, b, g) = (ids["C"], ids["A"], ids["B"], ids["G"]);
        push_sub_or(&mut onto, c, &[a, b]);
        push_sub(&mut onto, c, g);
        push_disjoint(&mut onto, g, a);
        derive_forced_disjuncts(&mut onto);
        assert!(has_atomic_sub(&onto, c, b), "expected derived C ⊑ B");
        assert!(!has_atomic_sub(&onto, c, a), "must NOT derive C ⊑ A");
    }

    #[test]
    fn forced_to_bot() {
        // C ⊑ A ⊔ B, C ⊑ G, Disjoint(G,A), Disjoint(G,B) ⟹ C ⊑ ⊥.
        let (mut onto, ids) = build(&["C", "A", "B", "G"]);
        let (c, a, b, g) = (ids["C"], ids["A"], ids["B"], ids["G"]);
        push_sub_or(&mut onto, c, &[a, b]);
        push_sub(&mut onto, c, g);
        push_disjoint(&mut onto, g, a);
        push_disjoint(&mut onto, g, b);
        derive_forced_disjuncts(&mut onto);
        assert!(has_sub_bot(&onto, c), "expected derived C ⊑ ⊥");
    }

    #[test]
    fn undetermined_emits_nothing() {
        // C ⊑ A ⊔ B with no disjointness ⟹ nothing derived (no spurious C⊑A/C⊑B).
        let (mut onto, ids) = build(&["C", "A", "B"]);
        let (c, a, b) = (ids["C"], ids["A"], ids["B"]);
        let before = onto.axioms.len();
        push_sub_or(&mut onto, c, &[a, b]);
        let after_push = onto.axioms.len();
        derive_forced_disjuncts(&mut onto);
        assert_eq!(onto.axioms.len(), after_push, "no axiom should be derived");
        assert!(!has_atomic_sub(&onto, c, a) && !has_atomic_sub(&onto, c, b));
        let _ = before;
    }

    #[test]
    fn nominal_disjunction_not_touched() {
        // C ⊑ {x} ⊔ {y} (nominal disjuncts) ⟹ nothing derived (scope guard).
        let (mut onto, ids) = build(&["C"]);
        let c = ids["C"];
        let x = onto.concepts.nominal(new_individual(&mut onto, "x"));
        let y = onto.concepts.nominal(new_individual(&mut onto, "y"));
        let or = onto.concepts.or(vec![x, y]);
        let csub = onto.concepts.atomic(c);
        onto.axioms.push(Axiom::SubClassOf { sub: csub, sup: or });
        let before = onto.axioms.len();
        derive_forced_disjuncts(&mut onto);
        assert_eq!(onto.axioms.len(), before, "nominal disjunction must be skipped");
    }
}
```

(Implementer: replace the `owl_dl_core_ir_classid_placeholder` import line — it is a marker, not real code; `crate::ir::ClassId` is referenced fully-qualified. Build the `tests` helpers — `build`, `push_sub`, `push_sub_or`, `push_disjoint`, `has_atomic_sub`, `has_sub_bot`, `new_individual` — by mirroring the ontology/ConceptPool construction in `crates/owl-dl-core/src/told.rs`'s `#[cfg(test)] mod tests` (e.g. `disjoint_classes_pairwise`), which already builds an `InternalOntology` with atomic classes + `Axiom::DisjointClasses`/`SubClassOf` directly. `push_sub_or` interns `concepts.or(vec![concepts.atomic(d) for d])` and a `SubClassOf{ sub: concepts.atomic(c), sup: or }`.)

- [ ] **Step 2: add the mod line**

Modify `crates/owl-dl-core/src/lib.rs` — add alongside the other `pub mod` lines (e.g. near `pub mod disjunction_existential;`):

```rust
pub mod approx_saturation;
```

- [ ] **Step 3: Run unit tests to verify they fail then pass**

```sh
export RUSTUP_HOME=/home/dumontier/.rustup CARGO_HOME=/home/dumontier/.cargo
export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$CARGO_HOME/bin:$PATH"
cargo test -p owl-dl-core --lib approx_saturation
```
Expected: 4 tests pass (`forced_disjunct_fires`, `forced_to_bot`, `undetermined_emits_nothing`, `nominal_disjunction_not_touched`).

- [ ] **Step 4: fmt + clippy on the crate**

```sh
cargo fmt --all -- --check
cargo clippy -p owl-dl-core --all-targets --all-features -- -D warnings
```
Expected: clean.

---

### Task 2: wire into `convert_ontology` + end-to-end integration canaries

**Files:**
- Modify: `crates/owl-dl-core/src/convert.rs:2157` (call after `derive_disjunction_existentials`)
- Create: `crates/owl-dl-reasoner/tests/approx_saturation_forced_disjunct.rs`

**Interfaces:**
- Consumes: `crate::approx_saturation::derive_forced_disjuncts` (Task 1).

- [ ] **Step 1: wire the call**

In `crates/owl-dl-core/src/convert.rs`, immediately after line 2157
`crate::disjunction_existential::derive_disjunction_existentials(&mut out);` add:

```rust
    // SP-A: forced-disjunct over atomic disjunctions. Runs AFTER
    // derive_disjunction_existentials so it sees the common-subsumer axioms that
    // pass adds (richer told tables ⟹ more forcings). Sound; atomic-only.
    crate::approx_saturation::derive_forced_disjuncts(&mut out);
```

- [ ] **Step 2: Write the failing integration test**

Create `crates/owl-dl-reasoner/tests/approx_saturation_forced_disjunct.rs` — end-to-end via OFN + the reasoner, asserting the *verdict* (forced subsumption / unsat / no-spurious):

```rust
//! SP-A integration canaries: forced-disjunct resolves atomic disjunctions
//! end-to-end (via convert_ontology → saturation), without false positives.
#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

fn classify(src: &str) -> owl_dl_reasoner::Classification {
    let mut r = Cursor::new(src.to_string().into_bytes());
    let (ont, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut r, ParserConfiguration::default()).unwrap();
    owl_dl_reasoner::classify_ontology(&ont).unwrap()
}

const BASE: &str = "Prefix(:=<http://t/>)\nOntology(<http://t/o>\n";

#[test]
fn forced_disjunct_resolves_to_survivor() {
    // C ⊑ A⊔B, C ⊑ G, Disjoint(G,A) ⟹ C ⊑ B should be entailed.
    let src = format!(
        "{BASE}\
Declaration(Class(:C)) Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:G))
SubClassOf(:C ObjectUnionOf(:A :B))
SubClassOf(:C :G)
DisjointClasses(:G :A)
)"
    );
    let cls = classify(&src);
    assert!(
        cls.is_subclass_of("http://t/C", "http://t/B"),
        "forced-disjunct: C ⊑ B must be entailed"
    );
}

#[test]
fn forced_to_bot_makes_unsat() {
    // C ⊑ A⊔B, C ⊑ G, Disjoint(G,A), Disjoint(G,B) ⟹ C unsatisfiable.
    let src = format!(
        "{BASE}\
Declaration(Class(:C)) Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:G))
SubClassOf(:C ObjectUnionOf(:A :B))
SubClassOf(:C :G)
DisjointClasses(:G :A)
DisjointClasses(:G :B)
)"
    );
    let cls = classify(&src);
    assert!(
        cls.is_unsatisfiable("http://t/C"),
        "forced-to-bot: C must be unsatisfiable"
    );
}

#[test]
fn undetermined_no_false_positive() {
    // C ⊑ A⊔B with no disjointness ⟹ neither C⊑A nor C⊑B may be entailed.
    let src = format!(
        "{BASE}\
Declaration(Class(:C)) Declaration(Class(:A)) Declaration(Class(:B))
SubClassOf(:C ObjectUnionOf(:A :B))
)"
    );
    let cls = classify(&src);
    assert!(!cls.is_subclass_of("http://t/C", "http://t/A"), "no spurious C⊑A");
    assert!(!cls.is_subclass_of("http://t/C", "http://t/B"), "no spurious C⊑B");
    assert!(!cls.is_unsatisfiable("http://t/C"), "C must stay satisfiable");
}
```

(Implementer: confirm the exact public reasoner API names — `classify_ontology`, `Classification::is_subclass_of(&str,&str)`, `is_unsatisfiable(&str)` — against `crates/owl-dl-reasoner/src/lib.rs`; adjust call sites to the real signatures if they differ, e.g. a `ReasonError` return or IRI-resolution helper. The assertions' intent is fixed; only the binding glue may need adjustment.)

- [ ] **Step 3: Run integration tests**

```sh
cargo test -p owl-dl-reasoner --test approx_saturation_forced_disjunct
```
Expected: 3 tests pass.

- [ ] **Step 4: Full workspace test + fmt + clippy**

```sh
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
```
Expected: all green; no warnings.

---

### Task 3: FP=0 corpus gate (tuned + ORE sweep)

**Files:** none (verification only). Uses release binaries + existing harnesses.

**Interfaces:** Consumes the wired pass (Tasks 1-2).

- [ ] **Step 1: Build the SP-A binary and a main-base binary**

```sh
export RUSTUP_HOME=/home/dumontier/.rustup CARGO_HOME=/home/dumontier/.cargo
export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$CARGO_HOME/bin:$PATH"
cargo build --release -p owl-dl-cli
cp target/release/rustdl /tmp/rustdl-spa
git stash --include-untracked   # or checkout main in a worktree
git checkout main
cargo build --release -p owl-dl-cli
cp target/release/rustdl /tmp/rustdl-base
git checkout feat/approx-saturation-sp-a
git stash pop
```

- [ ] **Step 2: Tuned-corpus closure-diff FP gate**

```sh
cargo test -p owl-dl-reasoner --test konclude_closure_diff --release -- --ignored --nocapture 2>&1 | grep -E "FP=|MISSED="
```
Expected: `FP=0` on every fixture (galen, notgalen, alehif, ore-10908, ore-15672, sio, wine, ro, pizza, bibtex). MISSED may *decrease* (recovery) but must never indicate an FP. Any `FP>0` ⟹ STOP, the pass is unsound, revert.

- [ ] **Step 3: ORE saturation-only before/after sweep (the increment-3 lesson)**

For each ont in `/data/dumontier/ore-run/pool_sample/files/ore_ont_*.owl` (and `/data/dumontier/ore-run/pilot/*/in.owl`), run both binaries `classify --saturation-only` and diff the `^(equiv|direct)` closure. Use a per-ont timeout of **180s** and capture the binary exit code **directly** (NOT after a pipe — a pipeline's `$?` is `sort`'s, which masks timeouts as empty/false-DIFFs; run the binary to a temp file first, check `$?`, then grep the file).

For any ont whose closure changes: it must be **additive** (`removed == 0`). The increment-3 FP signature is `removed ≈ before` (mass closure replacement) and/or a jump in `# satisfiability probes: saturation=N` (classes flagged unsat). Spot-check any additive change against the in-dir oracle (`pilot/*/diff.json` or `hermit.txt`/`kon.owx`) — every added edge must be in the oracle. Any non-additive change or unverifiable unsat ⟹ STOP and diagnose (likely a told-table soundness gap).

Expected: closures additive-or-identical; zero spurious-unsat cascades.

- [ ] **Step 4: Record the gate outcome**

Write `docs/sp-a-fp-gate-results-2026-06-23.md` summarizing: tuned-corpus FP/MISSED table, ORE sweep DIFF count + additive verification, and any recovered subsumptions (oracle-confirmed). This is the durable FP=0 evidence.

---

## Self-Review

**Spec coverage:** Rule 2 (forced-disjunct) — Task 1/2. Rule 1 (common-disjunct) — pre-existing in `disjunction_existential.rs` (noted; integration test `forced_disjunct_resolves_to_survivor` exercises the combined pipeline). Atomic-only scope guard — Task 1 `nominal_disjunction_not_touched` + Step-1 code. Soundness/FP gate — Task 3 (tuned + ORE, the increment-3 signature check). Nominal/construction/build-once/reuse deferral — out of scope per spec; not in plan. ✓

**Placeholder scan:** the `owl_dl_core_ir_classid_placeholder` import is explicitly flagged as a marker to delete; test helpers reference an existing concrete pattern (`told.rs` tests) rather than "TBD". No other placeholders.

**Type consistency:** `derive_forced_disjuncts(&mut InternalOntology)` consistent across Task 1 (produces) and Task 2 (consumes). `ConceptExpr::{Atomic,Or,Nominal}`, `ConceptId`, `ClassId`, `ToldTables::{super_classes,are_told_disjoint}`, `ConceptPool::{get,bot,atomic,or,nominal}`, `Axiom::SubClassOf{sub,sup}` — all match the verified APIs.
