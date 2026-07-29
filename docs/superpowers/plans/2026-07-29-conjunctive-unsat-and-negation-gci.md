# Conjunctive-unsat + RHS-negation canonicalization — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix a shipped silent-incompleteness bug where the EL saturator drops `X ⊓ Y ⊑ ⊥` axioms the fragment gate certifies as complete, then canonicalize `X ⊑ ¬Y` into that (now-working) form so otherwise-EL ontologies stop falling onto the O(n²) hybrid path.

**Architecture:** Part A adds a `ConjunctiveUnsat { bodies }` rule kind to `owl-dl-saturation`, indexed and consumed exactly like the existing `ConjunctiveTrigger`, firing the existing `enqueue_unsat`. Part B adds an `owl-dl-core` pass over `InternalOntology.axioms` that rewrites `SubClassOf { sub, sup: Not(y) }` into `SubClassOf { sub: And([sub, y]), sup: Bot }`, plus a `told.rs` arm so told-disjoint coverage is preserved. Part A must land and be green before Part B.

**Tech Stack:** Rust (edition 2024), `horned-owl` for parsing, `cargo test` / `cargo clippy` / `cargo fmt`.

**Spec:** `docs/superpowers/specs/2026-07-29-negation-to-bot-gci-and-conjunctive-unsat-design.md`

## Global Constraints

- **Toolchain.** `rust-toolchain.toml` pins `1.95.0` but that toolchain has no `cargo` binary, and `cargo` is not on `PATH` in this environment. Every cargo command in this plan must be prefixed with:
  `export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"` and run as `RUSTUP_TOOLCHAIN=stable cargo …`.
- **Warnings are errors.** CI sets `RUSTFLAGS: -D warnings`; clippy `pedantic` is on workspace-wide; `unwrap_used` and `dbg_macro` are warn-level (therefore errors). Test files in this repo open with `#![allow(clippy::unwrap_used)]` — follow that.
- **Formatting.** `cargo fmt --all -- --check` must pass; `max_width = 100`.
- **FP=0 is absolute.** No change may introduce a subsumption that does not hold. A false `unsat` is the maximal FP (it entails every subsumption from that class) — treat any new `unsat` that an oracle does not confirm as stop-and-diagnose, never as tuning.
- **Part B flag.** `RUSTDL_NEG_TO_BOT_GCI`, default ON, read as `std::env::var("RUSTDL_NEG_TO_BOT_GCI").map_or(true, |v| v != "0")` — the established pattern at `convert.rs:225` and `convert.rs:2519`.
- **Part A ships unflagged.** It is a correctness fix on a default-ON path.
- **Ordering.** Do not start Task 5 until Tasks 1-4 are committed and green. Landing Part B on top of the unfixed saturator converts a slow-but-correct answer into a fast wrong one.

---

## File Structure

**Part A — `owl-dl-saturation`**

| File | Responsibility |
|---|---|
| `crates/owl-dl-saturation/src/lib.rs` | `ConjunctiveUnsat` struct, `ElRules::conjunctive_unsat` field, `conjunctive_unsat_by_body` index + growth, consumption in `process_subsumer`, emission in the `And`-LHS rule-collection arm, provenance counters |
| `crates/owl-dl-saturation/src/proof.rs` | `ElRule::ConjunctiveUnsat` variant, `ProofTrace::conjunctive_unsat_axiom` field |
| `crates/owl-dl-reasoner/tests/conjunctive_unsat.rs` | **new** — end-to-end canaries: the bug reproducer, the spelling differential, the complex-body case, the empty-bodies FP guard |

**Part B — `owl-dl-core`**

| File | Responsibility |
|---|---|
| `crates/owl-dl-core/src/negation_gci.rs` | **new** — the `X ⊑ ¬Y` → `X ⊓ Y ⊑ ⊥` pass and its unit tests. One responsibility, one file. |
| `crates/owl-dl-core/src/lib.rs` | `pub mod negation_gci;` declaration |
| `crates/owl-dl-core/src/convert.rs` | call the pass late in `convert_ontology` |
| `crates/owl-dl-core/src/told.rs` | recognize `And([A,B]) ⊑ Bot` as a told-disjoint pair |
| `crates/owl-dl-reasoner/tests/negation_to_bot_gci.rs` | **new** — end-to-end canaries: `¬∃R.C` and `¬(A⊓B)` reach the fast path, told-disjoint parity across all three spellings, flag ON/OFF identity |

---

# PART A — conjunctive-unsat rule

### Task 1: Reproduce the bug as a failing test

The bug: `crates/owl-dl-saturation/src/lib.rs`'s `And`-LHS rule-collection arm derives its
heads from `atomic_operands_on_right(sup, pool)` and an existential-RHS scan. With
`sup = Bot` both return empty, so the axiom is dropped. Meanwhile the fragment gate
(Lever 1b, `classify.rs`) admits `X ⊓ Y ⊑ ⊥`, so the ontology is routed to the
saturation-only fast path and reported complete.

**Files:**
- Test: `crates/owl-dl-reasoner/tests/conjunctive_unsat.rs` (create)

**Interfaces:**
- Consumes: `owl_dl_reasoner::classify(&SetOntology<RcStr>) -> Result<Classification, ReasonError>`; `Classification::unsatisfiable_classes(&self) -> Vec<&str>`; `Classification::is_subclass(&self, sub: &str, sup: &str) -> bool`.
- Produces: nothing consumed by later tasks (test-only).

- [ ] **Step 1: Write the failing test file**

Create `crates/owl-dl-reasoner/tests/conjunctive_unsat.rs`:

```rust
//! Canaries for `X ⊓ Y ⊑ ⊥` (the lowered-`⊥` disjointness GCI) in the EL saturator.
//!
//! Lever 1b (commit 3e3a731) admitted this form to the fragment gate, but the
//! saturator's rule collector derived heads only from an atomic or existential
//! RHS — with `sup = Bot` both are empty, so the axiom was SILENTLY DROPPED while
//! the gate certified the closure complete (the D10 unsound-completeness class).
//!
//! Run: `cargo test -p owl-dl-reasoner --test conjunctive_unsat`

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

const PFX: &str = "Prefix(:=<http://t/>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n";

fn parse(body: &str) -> SetOntology<RcStr> {
    let src = format!("{PFX}Ontology(<http://t/x>\n{body}\n)\n");
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    onto
}

/// Classify `body` and return the sorted list of unsatisfiable class IRIs.
fn unsat_of(body: &str) -> Vec<String> {
    let onto = parse(body);
    let c = owl_dl_reasoner::classify(&onto).expect("classify");
    let mut v: Vec<String> = c
        .unsatisfiable_classes()
        .into_iter()
        .map(std::string::ToString::to_string)
        .collect();
    v.sort();
    v
}

const DECLS: &str = "    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
";

/// THE BUG REPRODUCER. `C ⊑ A`, `C ⊑ B`, `A ⊓ B ⊑ ⊥` ⟹ `C` unsatisfiable.
/// Before the fix this returns an EMPTY unsat set while printing
/// "pure-EL — saturator alone is complete".
#[test]
fn conjunctive_bot_derives_unsat() {
    let body = format!(
        "{DECLS}    SubClassOf(ObjectIntersectionOf(:A :B) owl:Nothing)
    SubClassOf(:C :A)
    SubClassOf(:C :B)"
    );
    assert_eq!(
        unsat_of(&body),
        vec!["http://t/C".to_string()],
        "C ⊑ A, C ⊑ B, A ⊓ B ⊑ ⊥ entails C ⊑ ⊥"
    );
}

/// SPELLING DIFFERENTIAL — the direct gate for the bug. The same ontology
/// written `A ⊓ B ⊑ ⊥` and `DisjointClasses(A B)` must classify identically.
#[test]
fn conjunctive_bot_matches_disjoint_classes_spelling() {
    let and_bot = format!(
        "{DECLS}    SubClassOf(ObjectIntersectionOf(:A :B) owl:Nothing)
    SubClassOf(:C :A)
    SubClassOf(:C :B)"
    );
    let disjoint = format!(
        "{DECLS}    DisjointClasses(:A :B)
    SubClassOf(:C :A)
    SubClassOf(:C :B)"
    );
    assert_eq!(
        unsat_of(&and_bot),
        unsat_of(&disjoint),
        "the two spellings of disjointness must produce the same closure"
    );
}

/// FP GUARD (negatives-first). A class with only ONE of the two conjuncts must
/// stay satisfiable. Guards against a rule that fires on a partial body match.
#[test]
fn conjunctive_bot_does_not_over_fire() {
    let body = format!(
        "{DECLS}    Declaration(Class(:D))
    SubClassOf(ObjectIntersectionOf(:A :B) owl:Nothing)
    SubClassOf(:C :A)
    SubClassOf(:C :B)
    SubClassOf(:D :A)"
    );
    assert_eq!(
        unsat_of(&body),
        vec!["http://t/C".to_string()],
        "D has only A, so D must remain satisfiable"
    );
}
```

- [ ] **Step 2: Run the tests to verify the bug reproduces**

```bash
export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"
RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test conjunctive_unsat
```

Expected: `conjunctive_bot_derives_unsat` FAILS with a left-hand side of `[]`.
`conjunctive_bot_matches_disjoint_classes_spelling` FAILS (`[]` vs `["http://t/C"]`).
`conjunctive_bot_does_not_over_fire` FAILS (`[]`).

If `conjunctive_bot_derives_unsat` PASSES, stop: the bug does not reproduce, and the
rest of Part A is moot. Re-read `crates/owl-dl-saturation/src/lib.rs`'s `And`-LHS arm
before continuing.

- [ ] **Step 3: Commit the failing canaries**

```bash
git add crates/owl-dl-reasoner/tests/conjunctive_unsat.rs
git commit -m "test(saturation): failing canaries for dropped \`X ⊓ Y ⊑ ⊥\` axioms

Lever 1b admitted the lowered-⊥ disjointness GCI to the fragment gate but the
saturator's rule collector drops it (heads come only from an atomic or
existential RHS; with sup=Bot both are empty). Result: missed unsatisfiability
reported as a complete pure-EL closure.

These tests currently FAIL. Fixed in the following commit."
```

---

### Task 2: Add the `ConjunctiveUnsat` rule and make the canaries pass

**Files:**
- Modify: `crates/owl-dl-saturation/src/lib.rs` (struct near `:2337`, `ElRules` field near `:2273`, index field near `:389`, index build near `:479`, growth block near `:655`, consumption in `process_subsumer` near `:1099`, emission in the `And`-LHS arm near `:3508`)

**Interfaces:**
- Consumes: `ElRules` (private struct), `WorklistEngine::enqueue_unsat(&mut self, c: ClassId)` at `lib.rs:924`, `self.subsumers.contains(c: ClassId, b: ClassId) -> bool`.
- Produces: `struct ConjunctiveUnsat { bodies: Vec<ClassId> }` and the field `ElRules::conjunctive_unsat: Vec<ConjunctiveUnsat>`, both used by Task 3.

- [ ] **Step 1: Add the rule struct**

In `crates/owl-dl-saturation/src/lib.rs`, immediately after the existing
`ConjunctiveTrigger` definition (currently at `:2337`):

```rust
struct ConjunctiveTrigger {
    bodies: Vec<ClassId>,
    head: ClassId,
}

/// Conjunctive unsatisfiability: when a class accumulates every `body` among
/// its subsumers, it is unsatisfiable. Lowered from `And(b₁ … bₙ) ⊑ ⊥` — the
/// n-ary generalisation of a `disjoint_pairs` entry.
///
/// Exists because the `And`-LHS arm of rule collection derives heads from
/// `atomic_operands_on_right(sup, _)` plus an existential-RHS scan, and with
/// `sup = Bot` both are empty — so before this rule the axiom was silently
/// dropped while the fragment gate (Lever 1b) certified the closure complete.
/// `directly_unsat` covers only a NON-conjunctive LHS.
#[derive(Debug, Clone)]
struct ConjunctiveUnsat {
    bodies: Vec<ClassId>,
}
```

Note the `#[derive(Debug, Clone)]`: `ConjunctiveTrigger` is `Clone`d at the
consumption site, and Task 3's provenance re-simulation needs `Debug` for the
`ElRules` derive to keep compiling.

- [ ] **Step 2: Add the `ElRules` field**

In `ElRules`, immediately after the `conjunctive_triggers` field (currently at `:2223`):

```rust
    /// Conjunctive triggers: when a class accumulates every `body`
    /// among its subsumers, it gains `head`.
    conjunctive_triggers: Vec<ConjunctiveTrigger>,
    /// Conjunctive unsat rules from `And(b₁ … bₙ) ⊑ ⊥`: when a class
    /// accumulates every `body` among its subsumers, it is unsatisfiable.
    conjunctive_unsat: Vec<ConjunctiveUnsat>,
```

- [ ] **Step 3: Add the dense index field**

In the engine struct, immediately after `conjunctive_by_body` (currently at `:389`):

```rust
    /// Dense per-class indices into `rules.conjunctive_triggers`.
    conjunctive_by_body: Vec<Vec<usize>>,
    /// Dense per-class indices into `rules.conjunctive_unsat`.
    conjunctive_unsat_by_body: Vec<Vec<usize>>,
```

- [ ] **Step 4: Build the index**

In `WorklistEngine::new`, immediately after the `conjunctive_by_body` build loop
(currently at `:478-483`):

```rust
        let mut conjunctive_by_body: Vec<Vec<usize>> = vec![Vec::new(); num_total_classes];
        for (idx, trigger) in rules.conjunctive_triggers.iter().enumerate() {
            for &body in &trigger.bodies {
                conjunctive_by_body[body.index() as usize].push(idx);
            }
        }
        let mut conjunctive_unsat_by_body: Vec<Vec<usize>> = vec![Vec::new(); num_total_classes];
        for (idx, rule) in rules.conjunctive_unsat.iter().enumerate() {
            for &body in &rule.bodies {
                conjunctive_unsat_by_body[body.index() as usize].push(idx);
            }
        }
```

Then add `conjunctive_unsat_by_body,` to the struct literal that `new` returns, beside
the existing `conjunctive_by_body,` entry.

- [ ] **Step 5: Grow the index with the class universe**

In `introduce_runtime_synthetic`, inside the `if needed > self.num_total_classes` block,
immediately after the `conjunctive_by_body` growth loop (currently at `:655-657`):

```rust
            while self.conjunctive_by_body.len() < needed {
                self.conjunctive_by_body.push(Vec::new());
            }
            while self.conjunctive_unsat_by_body.len() < needed {
                self.conjunctive_unsat_by_body.push(Vec::new());
            }
```

This is load-bearing: without it the consumption site indexes out of bounds and panics
as soon as a runtime Tseitin synthetic pushes the class id past `num_total_classes`.

No indexing of *newly added* `conjunctive_unsat` rules is needed in this function —
`TseitinAllocator::introduce` only ever appends `atomic_subsumptions` and
`conjunctive_triggers`, never `conjunctive_unsat`.

- [ ] **Step 6: Consume the rule in `process_subsumer`**

In `process_subsumer` (at `:1025`), immediately after the existing conjunctive-trigger
`if let Some(trigger_idxs) = …` block closes, add:

```rust
        // Conjunctive unsat (`And(b₁…bₙ) ⊑ ⊥`): every rule with D in its body
        // list may now fire on C if C has all the other bodies too.
        for ridx in self.conjunctive_unsat_by_body[d.index() as usize].clone() {
            let bodies = self.rules.conjunctive_unsat[ridx].bodies.clone();
            if bodies.iter().all(|b| self.subsumers.contains(c, *b)) {
                self.enqueue_unsat(c);
            }
        }
```

The `.clone()` of the index row mirrors the existing trigger loop and is required —
`enqueue_unsat` takes `&mut self`, so the borrow of `self.conjunctive_unsat_by_body`
cannot be held across it. Same reason for cloning `bodies`.

Proof recording is deliberately omitted here and added in Task 3.

- [ ] **Step 7: Emit the rule during rule collection**

In the `ConceptExpr::And(operands)` LHS arm (currently at `:3434`), immediately after
the `if !salvageable { return; }` guard (currently at `:3507-3509`) and BEFORE the
`atomic_operands_on_right` head loop:

```rust
            if !salvageable {
                return;
            }
            // `And(b₁…bₙ) ⊑ ⊥` is a disjointness assertion. Without this arm
            // `atomic_operands_on_right(Bot, _)` and the existential-RHS scan
            // below both return empty, so the axiom is silently DROPPED while
            // the fragment gate (Lever 1b) certifies the closure complete —
            // the D10 unsound-completeness class. `directly_unsat` covers only
            // a non-conjunctive LHS.
            //
            // An EMPTY body list would mean `⊤ ⊑ ⊥`, and a rule with no bodies
            // fires on EVERY class (`all()` over an empty iterator is `true`),
            // marking the whole vocabulary unsatisfiable. `ConceptPool` is not
            // expected to produce `And([])`, so rather than trust that we skip
            // it and leave the (global-inconsistency) case to the hybrid path —
            // a sound MISS instead of a catastrophic FP.
            if matches!(pool.get(sup), ConceptExpr::Bot) {
                if !bodies.is_empty() {
                    rules.conjunctive_unsat.push(ConjunctiveUnsat { bodies });
                }
                return;
            }
```

- [ ] **Step 8: Run the Task 1 canaries**

```bash
export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"
RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test conjunctive_unsat
```

Expected: all three tests PASS.

- [ ] **Step 9: Run the full saturation + reasoner suites**

```bash
RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-saturation
RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner
```

Expected: all PASS. In particular
`saturator_fragment_rejects_conjunctive_bot_with_functional` must still pass — the
`disjoint_ok` gate must keep forcing hybrid fallback when a functional role is present.

- [ ] **Step 10: fmt + clippy**

```bash
RUSTUP_TOOLCHAIN=stable cargo fmt --all -- --check
RUSTUP_TOOLCHAIN=stable cargo clippy -p owl-dl-saturation -p owl-dl-reasoner --all-targets --all-features -- -D warnings
```

Expected: both clean.

- [ ] **Step 11: Commit**

```bash
git add crates/owl-dl-saturation/src/lib.rs
git commit -m "fix(saturation): derive unsat from \`And(b₁…bₙ) ⊑ ⊥\` (was silently dropped)

The And-LHS rule-collection arm derived heads from atomic_operands_on_right plus
an existential-RHS scan; with sup=Bot both are empty, so the axiom evaporated
while the Lever 1b fragment gate certified the closure complete. directly_unsat
covers only a non-conjunctive LHS.

Adds ConjunctiveUnsat { bodies }, indexed by conjunctive_unsat_by_body exactly
like ConjunctiveTrigger, consumed in process_subsumer via the existing
enqueue_unsat. An empty body list is skipped rather than firing on the whole
vocabulary (sound MISS over catastrophic FP).

Makes the tests/conjunctive_unsat.rs canaries pass, including the
AndBot-vs-DisjointClasses spelling differential."
```

---

### Task 3: Axiom provenance for the new rule

Every other rule kind carries a parallel provenance `Vec` so `justify`/`explain` can
name the responsible axiom. Without it, an unsatisfiability derived by
`ConjunctiveUnsat` has no explanation.

**Files:**
- Modify: `crates/owl-dl-saturation/src/proof.rs` (`ElRule` enum near `:47`, `ProofTrace` struct near `:139`)
- Modify: `crates/owl-dl-saturation/src/lib.rs` (consumption site from Task 2 Step 6; provenance counters near `:2655` and `:2781-2855`; `ProofTrace` construction near `:2912`)
- Test: `crates/owl-dl-reasoner/tests/conjunctive_unsat.rs` (extend)

**Interfaces:**
- Consumes: `ConjunctiveUnsat`, `ElRules::conjunctive_unsat` (Task 2); `ProofTrace::record(&mut self, fact: DerivedFact, inf: Inference)`; `DerivedFact::{Sub, Unsat}`; `Inference { rule, premise_facts, axiom_refs }`; `AxiomRef(pub usize)`.
- Produces: `ElRule::ConjunctiveUnsat`; `ProofTrace::conjunctive_unsat_axiom: Vec<Option<usize>>`.

- [ ] **Step 1: Add the `ElRule` variant**

In `crates/owl-dl-saturation/src/proof.rs`, immediately after the
`ConjunctiveTrigger` variant (currently at `:62`):

```rust
    /// Conjunctive trigger: all `Bᵢ ∈ supers(C)` ⟹ `C ⊑ head`.
    ConjunctiveTrigger,
    /// Conjunctive unsat: all `Bᵢ ∈ supers(C)` and `And(B₁…Bₙ) ⊑ ⊥` ⟹ `C ⊑ ⊥`.
    ConjunctiveUnsat,
```

- [ ] **Step 2: Add the `ProofTrace` provenance field**

In `pub struct ProofTrace`, immediately after `conjunctive_trigger_axiom` (at `:151`):

```rust
    /// Axiom provenance for conjunctive triggers.
    pub(crate) conjunctive_trigger_axiom: Vec<Option<usize>>,
    /// Axiom provenance for conjunctive-unsat rules.
    pub(crate) conjunctive_unsat_axiom: Vec<Option<usize>>,
```

- [ ] **Step 3: Record the inference at the consumption site**

Replace the Task 2 Step 6 block in `process_subsumer` with the proof-recording version:

```rust
        // Conjunctive unsat (`And(b₁…bₙ) ⊑ ⊥`): every rule with D in its body
        // list may now fire on C if C has all the other bodies too.
        for ridx in self.conjunctive_unsat_by_body[d.index() as usize].clone() {
            let bodies = self.rules.conjunctive_unsat[ridx].bodies.clone();
            if bodies.iter().all(|b| self.subsumers.contains(c, *b)) {
                if self.record_proofs && !self.subsumers.unsatisfiable.contains(c.index() as usize)
                {
                    let premises: Vec<DerivedFact> =
                        bodies.iter().map(|&b| DerivedFact::Sub(c, b)).collect();
                    let ax_ref = self
                        .proof_trace
                        .as_ref()
                        .and_then(|t| t.conjunctive_unsat_axiom.get(ridx).copied().flatten())
                        .map(AxiomRef);
                    let inf = Inference {
                        rule: ElRule::ConjunctiveUnsat,
                        premise_facts: premises,
                        axiom_refs: ax_ref.into_iter().collect(),
                    };
                    if let Some(t) = self.proof_trace.as_mut() {
                        t.record(DerivedFact::Unsat(c), inf);
                    }
                }
                self.enqueue_unsat(c);
            }
        }
```

- [ ] **Step 4: Size the provenance vector**

In the provenance builder, beside the other `num_*` bindings (currently at `:2654-2660`):

```rust
    let num_conj = rules.conjunctive_triggers.len();
    let num_conj_unsat = rules.conjunctive_unsat.len();
```

and beside the other `let mut *_axiom` bindings (currently at `:2662-2668`):

```rust
    let mut conjunctive_trigger_axiom: Vec<Option<usize>> = vec![None; num_conj];
    let mut conjunctive_unsat_axiom: Vec<Option<usize>> = vec![None; num_conj_unsat];
```

- [ ] **Step 5: Track the counter in the re-simulation**

The builder re-simulates the axiom-to-rule mapping with before/after counters. Add a
`conj_unsat_cur` counter beside the existing `conj_cur`, and in **both** re-simulation
sites — the `Axiom::SubClassOf` arm (currently at `:2783-2812`) and the
`Axiom::EquivalentClasses` arm (currently at `:2813-2854`) — add the matching
before/after pair. In the `SubClassOf` arm:

```rust
                    let b_c = mini_rules.conjunctive_triggers.len();
                    let b_cu = mini_rules.conjunctive_unsat.len();
```
…after `lower_sub_class_of(…)`:
```rust
                    let a_c = mini_rules.conjunctive_triggers.len();
                    let a_cu = mini_rules.conjunctive_unsat.len();
```
…and beside the other `.fill(Some(ax_idx))` lines:
```rust
                    conjunctive_unsat_axiom[conj_unsat_cur..conj_unsat_cur + (a_cu - b_cu)]
                        .fill(Some(ax_idx));
                    conj_unsat_cur += a_cu - b_cu;
```

Apply the identical three additions inside the `EquivalentClasses` arm's inner
`i != j` loop.

- [ ] **Step 6: Add the field to the `ProofTrace` construction**

In the struct literal that builds the `ProofTrace` (currently at `:2905-2920`), add
`conjunctive_unsat_axiom,` beside the existing `conjunctive_trigger_axiom,`.

- [ ] **Step 7: Write the provenance test**

Append to `crates/owl-dl-reasoner/tests/conjunctive_unsat.rs`:

```rust
/// PROVENANCE. An unsatisfiability derived from `And(A,B) ⊑ ⊥` must be
/// explainable — `justify` has to name the responsible axiom rather than
/// returning an empty justification.
#[test]
fn conjunctive_bot_unsat_is_justifiable() {
    let body = format!(
        "{DECLS}    SubClassOf(ObjectIntersectionOf(:A :B) owl:Nothing)
    SubClassOf(:C :A)
    SubClassOf(:C :B)"
    );
    let onto = parse(&body);
    let js = owl_dl_reasoner::justify::justify(
        &onto,
        &owl_dl_reasoner::justify::parse_query("unsat http://t/C").expect("parse query"),
    )
    .expect("justify");
    assert!(
        !js.is_empty(),
        "unsat C must have at least one non-empty justification"
    );
}
```

Before running, confirm the exact `justify` entry point and query syntax:

```bash
grep -n 'pub fn justify\|pub fn parse_query' crates/owl-dl-reasoner/src/justify.rs
grep -rn 'unsat' crates/owl-dl-reasoner/src/justify.rs | grep -i 'query\|parse' | head
```

Adjust the call and the query string to the real signature — do **not** invent one. If
`justify` has no `unsat` query type, use the CLI-equivalent query for class
unsatisfiability that `parse_query` does accept, and note the substitution in the
commit message.

- [ ] **Step 8: Run the tests**

```bash
export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"
RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test conjunctive_unsat
RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-saturation
```

Expected: all PASS.

- [ ] **Step 9: fmt + clippy + commit**

```bash
RUSTUP_TOOLCHAIN=stable cargo fmt --all -- --check
RUSTUP_TOOLCHAIN=stable cargo clippy -p owl-dl-saturation -p owl-dl-reasoner --all-targets --all-features -- -D warnings
git add crates/owl-dl-saturation/src/proof.rs crates/owl-dl-saturation/src/lib.rs crates/owl-dl-reasoner/tests/conjunctive_unsat.rs
git commit -m "feat(saturation): axiom provenance for ConjunctiveUnsat

Adds ElRule::ConjunctiveUnsat and ProofTrace::conjunctive_unsat_axiom, wired
through both re-simulation arms (SubClassOf and EquivalentClasses) so an unsat
derived from \`And(b₁…bₙ) ⊑ ⊥\` names its responsible axiom in justify/explain."
```

---

### Task 4: Complex-body coverage and the corpus gate

`bodies` already handles non-atomic `And` operands (an `∃R.C` operand is lowered to a
marker class by the existing arm), so `∃R.C ⊓ D ⊑ ⊥` is covered by the same rule. Pin
that, then validate against the corpus.

**Files:**
- Test: `crates/owl-dl-reasoner/tests/conjunctive_unsat.rs` (extend)

**Interfaces:**
- Consumes: everything from Tasks 1-3. Produces: nothing.

- [ ] **Step 1: Write the complex-body tests**

Append to `crates/owl-dl-reasoner/tests/conjunctive_unsat.rs`:

```rust
/// COMPLEX BODY. `∃R.C ⊓ D ⊑ ⊥` — the `bodies` collector lowers the `∃R.C`
/// operand to a marker class, so the same rule covers it.
#[test]
fn conjunctive_bot_with_existential_body_derives_unsat() {
    let body = "    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(Class(:X))
    Declaration(ObjectProperty(:R))
    SubClassOf(ObjectIntersectionOf(ObjectSomeValuesFrom(:R :C) :D) owl:Nothing)
    SubClassOf(:X ObjectSomeValuesFrom(:R :C))
    SubClassOf(:X :D)";
    assert_eq!(
        unsat_of(body),
        vec!["http://t/X".to_string()],
        "X has both ∃R.C and D, and their conjunction is unsatisfiable"
    );
}

/// FP GUARD for the complex-body case: only the existential, no D.
#[test]
fn conjunctive_bot_with_existential_body_does_not_over_fire() {
    let body = "    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(Class(:X))
    Declaration(ObjectProperty(:R))
    SubClassOf(ObjectIntersectionOf(ObjectSomeValuesFrom(:R :C) :D) owl:Nothing)
    SubClassOf(:X ObjectSomeValuesFrom(:R :C))";
    assert!(
        unsat_of(body).is_empty(),
        "X has only ∃R.C, so nothing is unsatisfiable"
    );
}

/// N-ARY. Three-operand conjunction; a class with two of the three stays
/// satisfiable, a class with all three does not.
#[test]
fn conjunctive_bot_ternary_requires_all_bodies() {
    let body = "    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:E))
    Declaration(Class(:Two))
    Declaration(Class(:Three))
    SubClassOf(ObjectIntersectionOf(:A :B :E) owl:Nothing)
    SubClassOf(:Two :A)
    SubClassOf(:Two :B)
    SubClassOf(:Three :A)
    SubClassOf(:Three :B)
    SubClassOf(:Three :E)";
    assert_eq!(
        unsat_of(body),
        vec!["http://t/Three".to_string()],
        "only the class carrying all three conjuncts is unsatisfiable"
    );
}
```

- [ ] **Step 2: Run them**

```bash
export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"
RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test conjunctive_unsat
```

Expected: all PASS. If `conjunctive_bot_with_existential_body_derives_unsat` fails, the
`∃` operand's marker is not being matched — read
`existential_body_alternatives` / `introduce_existential_marker` before changing the
rule, and record the finding rather than weakening the test.

- [ ] **Step 3: Whole-workspace test run**

```bash
RUSTUP_TOOLCHAIN=stable cargo test --workspace
```

Expected: green. Any pre-existing test that now fails is a **signal**, not a nuisance:
it most likely encodes the buggy behaviour. Read it, and if it asserted a missing
unsat, update it and say so explicitly in the commit message.

- [ ] **Step 4: Curated-corpus closure diff**

```bash
./scripts/fetch-real-ontologies.sh   # if ontologies/real is absent
RUSTUP_TOOLCHAIN=stable cargo build --workspace --release
for f in ontologies/real/*.ofn; do
  echo "=== $f"
  timeout 300 ./target/release/rustdl classify "$f" 2>/dev/null \
    | grep -v '^#' | sort > "/tmp/claude-1007/-data-dumontier-rustdl/8e753f2f-e24e-4be2-8c66-c6e13e322bae/scratchpad/$(basename "$f").after"
done
```

Compare each against the same run on `git stash`-ed (pre-Part-A) code. Expected: most
fixtures byte-identical. **Any ontology whose output changes must be validated against
the Konclude∩HermiT oracle** — every newly reported `unsat` confirmed, FP=0. A new
MISS is stop-and-diagnose.

- [ ] **Step 5: Record the measurement and commit**

```bash
git add crates/owl-dl-reasoner/tests/conjunctive_unsat.rs
git commit -m "test(saturation): complex-body, n-ary, and FP-guard canaries for ConjunctiveUnsat

Pins that \`∃R.C ⊓ D ⊑ ⊥\` is covered by the same rule (the bodies collector
lowers the existential operand to a marker), that an n-ary conjunction requires
ALL bodies, and that partial body matches never fire.

Corpus closure diff: <fill in — which fixtures changed, and the oracle verdict
for each newly reported unsat>."
```

---

# PART B — RHS-negation canonicalization

**Do not start until Tasks 1-4 are committed and green.**

### Task 5: The rewrite pass (unwired)

Key fact that determines the design: `nnf_axioms` (`normalize.rs:163`) returns a **new**
`Vec<Axiom>` and does not mutate `ontology.axioms` — there is an existing test
`nnf_axioms_leaves_original_axioms_unchanged` pinning that. So `InternalOntology.axioms`
stays in its original, pre-NNF form, and both the saturator's rule collection and the
fragment gate read that original list. Therefore "before NNF" means: **a pass over
`InternalOntology.axioms` itself.**

**Files:**
- Create: `crates/owl-dl-core/src/negation_gci.rs`
- Modify: `crates/owl-dl-core/src/lib.rs`

**Interfaces:**
- Consumes: `InternalOntology { axioms: Vec<Axiom>, concepts: ConceptPool, .. }`; `Axiom::SubClassOf { sub: ConceptId, sup: ConceptId }`; `ConceptExpr::{Not, And, Bot}`; `ConceptPool::get(&self, ConceptId) -> &ConceptExpr`, `ConceptPool::and(&mut self, Vec<ConceptId>) -> ConceptId`, `ConceptPool::bot(&mut self) -> ConceptId`.
- Produces: `pub fn rewrite_negated_supers(onto: &mut InternalOntology) -> usize` — rewrites in place, returns the number of axioms rewritten.

- [ ] **Step 1: Confirm the `ConceptPool` constructor names**

```bash
grep -n 'pub fn and\|pub fn bot\|pub fn not\|pub fn top' crates/owl-dl-core/src/ir.rs | head
```

Use the exact names this prints in the code below. `pool.not(...)` is known to exist
(`convert.rs:875` calls it); confirm `and` and `bot`.

- [ ] **Step 2: Write the failing unit tests**

Create `crates/owl-dl-core/src/negation_gci.rs`:

```rust
//! Canonicalize a negated GCI right-hand side into a lowered-`⊥` GCI.
//!
//! `X ⊑ ¬Y ≡ X ⊓ Y ⊑ ⊥` is an unconditional logical equivalence, so this pass
//! cannot change the entailment set — only which engine answers. Its value is
//! that `is_el_concept` / `is_saturator_concept` reject `ConceptExpr::Not`
//! outright, so a single `A ⊑ ¬B` axiom routes an otherwise-EL ontology onto the
//! O(n²) hybrid path, while `X ⊓ Y ⊑ ⊥` is in-fragment (Lever 1b) and — since the
//! `ConjunctiveUnsat` rule landed — completely reasoned over by the saturator.
//!
//! **This must run BEFORE NNF.** `nnf_axioms` pushes negation to atomic leaves,
//! so post-NNF `X ⊑ ¬(A ⊓ B)` has already become `X ⊑ ¬A ⊔ ¬B` — an `Or`, and the
//! opportunity is gone. Pre-NNF the same axiom becomes `X ⊓ A ⊓ B ⊑ ⊥`, fully
//! EL-positive. Since `nnf_axioms` returns a fresh Vec and leaves
//! `InternalOntology.axioms` untouched, "before NNF" means "a pass over
//! `InternalOntology.axioms`".

use crate::ir::{ConceptExpr, ConceptId};
use crate::ontology::{Axiom, InternalOntology};

/// Is the `RUSTDL_NEG_TO_BOT_GCI` lever enabled? Default ON; `=0` reverts.
fn enabled() -> bool {
    std::env::var("RUSTDL_NEG_TO_BOT_GCI").map_or(true, |v| v != "0")
}

/// Rewrite every `SubClassOf { sub, sup }` whose `sup` is a negation — or an
/// `And` containing one — into the equivalent lowered-`⊥` form. Returns the
/// number of axioms rewritten.
pub fn rewrite_negated_supers(onto: &mut InternalOntology) -> usize {
    if !enabled() {
        return 0;
    }
    let mut rewritten = 0usize;
    let mut extra: Vec<Axiom> = Vec::new();
    for i in 0..onto.axioms.len() {
        let Axiom::SubClassOf { sub, sup } = onto.axioms[i] else {
            continue;
        };
        // Split the RHS into its negated and positive parts. A top-level `Not`
        // yields one negated part and no positive part; a top-level `And` is
        // partitioned operand-wise so `X ⊑ A ⊓ ¬B` yields `X ⊑ A` plus
        // `X ⊓ B ⊑ ⊥` (otherwise the negation survives inside the `And` and the
        // axiom stays out-of-fragment).
        let (negated, positive) = split_rhs(sup, &onto.concepts);
        if negated.is_empty() {
            continue;
        }
        // `X ⊓ y₁ ⊓ … ⊓ yₙ ⊑ ⊥` for the negated parts.
        let mut conj = vec![sub];
        conj.extend(negated);
        let and_id = onto.concepts.and(conj);
        let bot_id = onto.concepts.bot();
        onto.axioms[i] = Axiom::SubClassOf {
            sub: and_id,
            sup: bot_id,
        };
        // `X ⊑ pᵢ` for each surviving positive part.
        for p in positive {
            extra.push(Axiom::SubClassOf { sub, sup: p });
        }
        rewritten += 1;
    }
    onto.axioms.extend(extra);
    rewritten
}

/// Partition a GCI right-hand side into (inner concepts of negated parts,
/// positive parts). `¬Y` contributes `Y` to the first list; an `And` is
/// partitioned operand-wise; anything else is a single positive part.
fn split_rhs(sup: ConceptId, pool: &crate::ir::ConceptPool) -> (Vec<ConceptId>, Vec<ConceptId>) {
    match pool.get(sup) {
        ConceptExpr::Not(inner) => (vec![*inner], Vec::new()),
        ConceptExpr::And(ops) => {
            let mut neg = Vec::new();
            let mut pos = Vec::new();
            for &op in ops {
                if let ConceptExpr::Not(inner) = pool.get(op) {
                    neg.push(*inner);
                } else {
                    pos.push(op);
                }
            }
            (neg, pos)
        }
        _ => (Vec::new(), Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::ConceptExpr;

    /// `A ⊑ ¬B` becomes `A ⊓ B ⊑ ⊥`.
    #[test]
    fn atomic_negation_becomes_bot_gci() {
        let mut o = InternalOntology::new();
        let a = o.vocabulary.intern_class("http://t/A");
        let b = o.vocabulary.intern_class("http://t/B");
        let a_c = o.concepts.atomic(a);
        let b_c = o.concepts.atomic(b);
        let not_b = o.concepts.not(b_c);
        o.axioms.push(Axiom::SubClassOf {
            sub: a_c,
            sup: not_b,
        });

        assert_eq!(rewrite_negated_supers(&mut o), 1);
        assert_eq!(o.axioms.len(), 1);
        let Axiom::SubClassOf { sub, sup } = o.axioms[0] else {
            panic!("expected SubClassOf");
        };
        assert!(matches!(o.concepts.get(sup), ConceptExpr::Bot));
        let ConceptExpr::And(ops) = o.concepts.get(sub) else {
            panic!("expected And LHS, got {:?}", o.concepts.get(sub));
        };
        let mut got: Vec<ConceptId> = ops.clone();
        got.sort();
        let mut want = vec![a_c, b_c];
        want.sort();
        assert_eq!(got, want, "LHS must be A ⊓ B");
    }

    /// `X ⊑ A ⊓ ¬B` becomes `X ⊓ B ⊑ ⊥` PLUS `X ⊑ A` — otherwise the negation
    /// survives inside the `And` and the axiom stays out-of-fragment.
    #[test]
    fn conjunctive_rhs_splits_positive_and_negated() {
        let mut o = InternalOntology::new();
        let x = o.vocabulary.intern_class("http://t/X");
        let a = o.vocabulary.intern_class("http://t/A");
        let b = o.vocabulary.intern_class("http://t/B");
        let x_c = o.concepts.atomic(x);
        let a_c = o.concepts.atomic(a);
        let b_c = o.concepts.atomic(b);
        let not_b = o.concepts.not(b_c);
        let rhs = o.concepts.and(vec![a_c, not_b]);
        o.axioms.push(Axiom::SubClassOf { sub: x_c, sup: rhs });

        assert_eq!(rewrite_negated_supers(&mut o), 1);
        assert_eq!(o.axioms.len(), 2, "one ⊥-GCI plus one positive GCI");
        let has_positive = o.axioms.iter().any(|ax| {
            matches!(ax, Axiom::SubClassOf { sub, sup } if *sub == x_c && *sup == a_c)
        });
        assert!(has_positive, "X ⊑ A must survive as its own axiom");
    }

    /// A negation-free RHS is untouched.
    #[test]
    fn positive_axioms_are_inert() {
        let mut o = InternalOntology::new();
        let a = o.vocabulary.intern_class("http://t/A");
        let b = o.vocabulary.intern_class("http://t/B");
        let a_c = o.concepts.atomic(a);
        let b_c = o.concepts.atomic(b);
        o.axioms.push(Axiom::SubClassOf { sub: a_c, sup: b_c });
        let before = o.axioms.clone();

        assert_eq!(rewrite_negated_supers(&mut o), 0);
        assert_eq!(o.axioms, before, "no negation ⇒ no change");
    }

    /// A negation on the LEFT is NOT touched: `¬A ⊑ B` is a covering axiom
    /// (`⊤ ⊑ A ⊔ B`), not a disjointness, and rewriting it would be wrong.
    #[test]
    fn left_hand_negation_is_untouched() {
        let mut o = InternalOntology::new();
        let a = o.vocabulary.intern_class("http://t/A");
        let b = o.vocabulary.intern_class("http://t/B");
        let a_c = o.concepts.atomic(a);
        let b_c = o.concepts.atomic(b);
        let not_a = o.concepts.not(a_c);
        o.axioms.push(Axiom::SubClassOf {
            sub: not_a,
            sup: b_c,
        });
        let before = o.axioms.clone();

        assert_eq!(rewrite_negated_supers(&mut o), 0);
        assert_eq!(o.axioms, before, "LHS negation is a covering axiom");
    }

    /// The flag reverts the pass. Serialised because it mutates the process env.
    #[test]
    fn flag_off_reverts() {
        static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _g = ENV_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let prev = std::env::var_os("RUSTDL_NEG_TO_BOT_GCI");
        // SAFETY: set_var is unsafe under edition 2024; serialised by ENV_LOCK
        // and restored below.
        unsafe { std::env::set_var("RUSTDL_NEG_TO_BOT_GCI", "0") };

        let mut o = InternalOntology::new();
        let a = o.vocabulary.intern_class("http://t/A");
        let b = o.vocabulary.intern_class("http://t/B");
        let a_c = o.concepts.atomic(a);
        let b_c = o.concepts.atomic(b);
        let not_b = o.concepts.not(b_c);
        o.axioms.push(Axiom::SubClassOf {
            sub: a_c,
            sup: not_b,
        });
        let before = o.axioms.clone();
        let n = rewrite_negated_supers(&mut o);

        // SAFETY: see above.
        unsafe {
            match &prev {
                Some(v) => std::env::set_var("RUSTDL_NEG_TO_BOT_GCI", v),
                None => std::env::remove_var("RUSTDL_NEG_TO_BOT_GCI"),
            }
        }
        assert_eq!(n, 0);
        assert_eq!(o.axioms, before);
    }
}
```

The exact names `o.vocabulary.intern_class`, `o.concepts.atomic`, `o.concepts.and`,
`o.concepts.bot`, `o.concepts.not`, and whether `Axiom`/`Vec<Axiom>` derive
`Clone`/`PartialEq` must be confirmed against the real API before running:

```bash
grep -n 'pub fn intern_class' crates/owl-dl-core/src/vocabulary.rs
grep -n 'pub fn atomic\|pub fn and\|pub fn bot\|pub fn not' crates/owl-dl-core/src/ir.rs
grep -n 'pub enum Axiom' -B 3 crates/owl-dl-core/src/ontology.rs
```

Adapt the test bodies to the real signatures. If `Axiom` is not `PartialEq`, compare
a `format!("{:?}", …)` of the axiom list instead of the values.

- [ ] **Step 3: Declare the module**

In `crates/owl-dl-core/src/lib.rs`, beside the other `pub mod` declarations:

```rust
pub mod negation_gci;
```

- [ ] **Step 4: Run the unit tests**

```bash
export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"
RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-core negation_gci
```

Expected: all 5 PASS.

- [ ] **Step 5: fmt + clippy + commit**

```bash
RUSTUP_TOOLCHAIN=stable cargo fmt --all -- --check
RUSTUP_TOOLCHAIN=stable cargo clippy -p owl-dl-core --all-targets --all-features -- -D warnings
git add crates/owl-dl-core/src/negation_gci.rs crates/owl-dl-core/src/lib.rs
git commit -m "feat(core): pass rewriting \`X ⊑ ¬Y\` to \`X ⊓ Y ⊑ ⊥\` (not yet wired)

A logical equivalence, so it cannot change the entailment set — only which
engine answers. is_el_concept / is_saturator_concept reject ConceptExpr::Not, so
one negated GCI routes an otherwise-EL ontology onto the O(n²) hybrid path.

Partitions a conjunctive RHS operand-wise (X ⊑ A ⊓ ¬B yields X ⊓ B ⊑ ⊥ plus
X ⊑ A). Leaves LHS negation alone — \`¬A ⊑ B\` is a covering axiom, not a
disjointness. Gated RUSTDL_NEG_TO_BOT_GCI (default ON). Wired in the next commit."
```

---

### Task 6: Wire the pass and preserve told-disjoint coverage

**Files:**
- Modify: `crates/owl-dl-core/src/convert.rs` (late in `convert_ontology`, near `:2245`)
- Modify: `crates/owl-dl-core/src/told.rs` (the `Axiom::SubClassOf` arm at `:124-130`, helpers near `:222-237`)

**Interfaces:**
- Consumes: `crate::negation_gci::rewrite_negated_supers(&mut InternalOntology) -> usize` (Task 5).
- Produces: no new public API.

- [ ] **Step 1: Write the failing told-disjoint test**

`told.rs:124-130` currently records a told-disjoint pair from `SubClassOf(A, Not(B))`
via `as_not_atomic` (`told.rs:230`). After the rewrite that arm stops matching, so
coverage would silently shrink — and the told tables feed the classify tier walk and
the tableau. Add to the `#[cfg(test)] mod tests` in `crates/owl-dl-core/src/told.rs`:

```rust
    /// All three spellings of "A and B are disjoint" must yield the same told
    /// pair: `DisjointClasses(A B)`, `A ⊑ ¬B`, and `A ⊓ B ⊑ ⊥`. The third is
    /// what the negation-GCI rewrite produces, and it is also how users write
    /// disjointness directly.
    #[test]
    fn and_bot_gci_records_told_disjoint_pair() {
        let mut o = InternalOntology::new();
        let a = o.vocabulary.intern_class("http://t/A");
        let b = o.vocabulary.intern_class("http://t/B");
        let a_c = o.concepts.atomic(a);
        let b_c = o.concepts.atomic(b);
        let and_ab = o.concepts.and(vec![a_c, b_c]);
        let bot = o.concepts.bot();
        o.axioms.push(Axiom::SubClassOf {
            sub: and_ab,
            sup: bot,
        });

        let told = build_told_tables(&o);
        assert!(
            told.disjoint_with(a).contains(&b),
            "A ⊓ B ⊑ ⊥ must record A and B as told-disjoint"
        );
    }
```

Confirm the real builder and accessor names first — they may differ:

```bash
grep -n 'pub fn build_told_tables\|pub fn disjoint_with\|pub struct ToldTables' -A 4 crates/owl-dl-core/src/told.rs | head -30
```

- [ ] **Step 2: Run it to verify it fails**

```bash
export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"
RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-core and_bot_gci_records_told_disjoint
```

Expected: FAIL — the assertion is false, because no arm matches `And ⊑ Bot`.

- [ ] **Step 3: Add the told-disjoint arm**

In `crates/owl-dl-core/src/told.rs`, replace the `Axiom::SubClassOf` arm
(currently at `:124-130`):

```rust
            Axiom::SubClassOf { sub, sup } => {
                let sub_atom = as_atomic(*sub, pool);
                if let (Some(a), Some(b)) = (sub_atom, as_atomic(*sup, pool)) {
                    add_edge(&mut direct_super, a, b);
                } else if let (Some(a), Some(b)) = (sub_atom, as_not_atomic(*sup, pool)) {
                    add_disjoint_pair(&mut disjoint, a, b);
                } else if let Some((a, b)) = as_atomic_pair_to_bot(*sub, *sup, pool) {
                    // `A ⊓ B ⊑ ⊥` — the form the negation-GCI rewrite produces,
                    // and the form users write directly. Without this arm,
                    // rewriting `A ⊑ ¬B` would silently shrink told-disjoint
                    // coverage (the tier walk and the tableau read these tables).
                    add_disjoint_pair(&mut disjoint, a, b);
                }
            }
```

and add the helper beside `as_not_atomic` (currently at `:230`):

```rust
/// `And(Atomic(a), Atomic(b)) ⊑ Bot` ⟹ `Some((a, b))`. Exactly two atomic
/// operands: an n-ary conjunction asserts only that the whole conjunction is
/// unsatisfiable, which does NOT make any particular pair disjoint.
fn as_atomic_pair_to_bot(
    sub: ConceptId,
    sup: ConceptId,
    pool: &ConceptPool,
) -> Option<(ClassId, ClassId)> {
    if !matches!(pool.get(sup), ConceptExpr::Bot) {
        return None;
    }
    let ConceptExpr::And(ops) = pool.get(sub) else {
        return None;
    };
    if ops.len() != 2 {
        return None;
    }
    let a = as_atomic(ops[0], pool)?;
    let b = as_atomic(ops[1], pool)?;
    Some((a, b))
}
```

The `ops.len() != 2` restriction is a soundness requirement, not a simplification:
from `A ⊓ B ⊓ E ⊑ ⊥` it does **not** follow that `A` and `B` are disjoint.

- [ ] **Step 4: Run the told test**

```bash
RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-core told
```

Expected: PASS, and every pre-existing `told` test still green.

- [ ] **Step 5: Wire the pass into `convert_ontology`**

In `crates/owl-dl-core/src/convert.rs`, immediately after the
`crate::approx_saturation::derive_forced_disjuncts(&mut out);` call (currently at
`:2245`) and BEFORE `decompose_long_chains(&mut out);`:

```rust
    // Canonicalize `X ⊑ ¬Y` into `X ⊓ Y ⊑ ⊥` (a logical equivalence). The gates
    // reject `ConceptExpr::Not` outright, so one negated GCI routes an
    // otherwise-EL ontology onto the O(n²) hybrid path; the lowered-⊥ form is
    // in-fragment (Lever 1b) and completely reasoned over by the saturator's
    // ConjunctiveUnsat rule. Runs on the fully populated IR, and BEFORE any NNF
    // view is taken — `nnf_axioms` would already have turned `¬(A ⊓ B)` into an
    // `Or`. Gated RUSTDL_NEG_TO_BOT_GCI (default ON).
    let _ = crate::negation_gci::rewrite_negated_supers(&mut out);
```

Placement rationale to preserve: it must run **after** the passes that add axioms
(`derive_disjunction_existentials`, `derive_forced_disjuncts`) so any negated GCI they
emit is also canonicalized, and **before** `decompose_long_chains` so the role-chain
pass sees a stable axiom list.

- [ ] **Step 6: Run the workspace suite**

```bash
RUSTUP_TOOLCHAIN=stable cargo test --workspace
```

Expected: green. A failure here is most likely a test that asserted on the *shape* of
converted axioms (a `Not` on a RHS). Read it: if it is shape-asserting, update it and
name it in the commit; if it is a *behaviour* assertion, the rewrite has changed
semantics and that is a stop-and-diagnose.

- [ ] **Step 7: fmt + clippy + commit**

```bash
RUSTUP_TOOLCHAIN=stable cargo fmt --all -- --check
RUSTUP_TOOLCHAIN=stable cargo clippy --workspace --all-targets --all-features -- -D warnings
git add crates/owl-dl-core/src/convert.rs crates/owl-dl-core/src/told.rs
git commit -m "feat(core): wire the negation→⊥-GCI rewrite; preserve told-disjoint coverage

Calls rewrite_negated_supers late in convert_ontology (after the axiom-adding
passes, before decompose_long_chains) and teaches told.rs to read the
\`And(A,B) ⊑ ⊥\` form so told-disjoint coverage does not shrink when \`A ⊑ ¬B\`
is rewritten. The new arm also picks up natively-written \`A ⊓ B ⊑ ⊥\`, a pair the
table missed before, so coverage strictly increases. Restricted to exactly two
atomic operands — from \`A ⊓ B ⊓ E ⊑ ⊥\` no particular pair is disjoint."
```

---

### Task 7: End-to-end fast-path canaries

**Files:**
- Test: `crates/owl-dl-reasoner/tests/negation_to_bot_gci.rs` (create)

**Interfaces:**
- Consumes: `owl_dl_reasoner::classify`; `Classification::{unsatisfiable_classes, is_subclass, stats}`; `ClassificationStats::fragment` (a `FragmentClassification`).

- [ ] **Step 1: Confirm how to observe the fast path**

The CLI prints `# fragment: pure-EL …` / `# mode: pure EL (saturation-only)`.
Programmatically that is `Classification::stats().fragment`. Confirm the enum path and
variant names:

```bash
grep -n 'pub fragment' crates/owl-dl-reasoner/src/classify.rs
grep -n 'pub enum FragmentClassification' -A 12 crates/owl-dl-reasoner/src/classify.rs
```

Use the exact names below.

- [ ] **Step 2: Write the tests**

Create `crates/owl-dl-reasoner/tests/negation_to_bot_gci.rs`:

```rust
//! End-to-end canaries for the `X ⊑ ¬Y` → `X ⊓ Y ⊑ ⊥` canonicalization.
//!
//! The rewrite is a logical equivalence, so these assert two things: the
//! entailments are unchanged, AND the ontology now reaches the saturation
//! fast path (which is the whole point — `ConceptExpr::Not` is rejected by
//! `is_el_concept` / `is_saturator_concept`).
//!
//! Run: `cargo test -p owl-dl-reasoner --test negation_to_bot_gci`

#![allow(clippy::unwrap_used, unsafe_code)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::classify::{Classification, FragmentClassification};
use std::io::Cursor;

const PFX: &str = "Prefix(:=<http://t/>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n";

fn parse(body: &str) -> SetOntology<RcStr> {
    let src = format!("{PFX}Ontology(<http://t/x>\n{body}\n)\n");
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    onto
}

fn classify(body: &str) -> Classification {
    owl_dl_reasoner::classify(&parse(body)).expect("classify")
}

fn unsat(c: &Classification) -> Vec<String> {
    let mut v: Vec<String> = c
        .unsatisfiable_classes()
        .into_iter()
        .map(std::string::ToString::to_string)
        .collect();
    v.sort();
    v
}

/// `A ⊑ ¬B` + `C ⊑ A` + `C ⊑ B` ⟹ `C` unsat, AND the ontology reaches the
/// pure-EL fast path (before the rewrite the `Not` forced the hybrid path).
#[test]
fn atomic_negation_reaches_fast_path() {
    let body = "    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    SubClassOf(:A ObjectComplementOf(:B))
    SubClassOf(:C :A)
    SubClassOf(:C :B)";
    let c = classify(body);
    assert_eq!(unsat(&c), vec!["http://t/C".to_string()]);
    assert_eq!(
        c.stats().fragment,
        FragmentClassification::PureEl,
        "atomic negation on a GCI RHS must no longer force the hybrid path"
    );
}

/// `X ⊑ ¬∃R.C` becomes `X ⊓ ∃R.C ⊑ ⊥` — in-fragment. Post-NNF it would have
/// become `X ⊑ ∀R.¬C`, which is out-of-fragment, so this test is what pins the
/// PRE-NNF placement of the pass.
#[test]
fn negated_existential_reaches_fast_path() {
    let body = "    Declaration(Class(:C))
    Declaration(Class(:X))
    Declaration(Class(:Y))
    Declaration(ObjectProperty(:R))
    SubClassOf(:X ObjectComplementOf(ObjectSomeValuesFrom(:R :C)))
    SubClassOf(:Y :X)
    SubClassOf(:Y ObjectSomeValuesFrom(:R :C))";
    let c = classify(body);
    assert_eq!(
        unsat(&c),
        vec!["http://t/Y".to_string()],
        "Y is both X (no R-successor in C) and has one"
    );
    assert_eq!(
        c.stats().fragment,
        FragmentClassification::PureEl,
        "¬∃R.C on a GCI RHS must lower to an EL-positive ⊥-GCI (pre-NNF placement)"
    );
}

/// `X ⊑ ¬(A ⊓ B)` becomes `X ⊓ A ⊓ B ⊑ ⊥`. Post-NNF it would be `¬A ⊔ ¬B`,
/// an `Or` — the second pin on the pre-NNF placement.
#[test]
fn negated_conjunction_reaches_fast_path() {
    let body = "    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:X))
    Declaration(Class(:Y))
    SubClassOf(:X ObjectComplementOf(ObjectIntersectionOf(:A :B)))
    SubClassOf(:Y :X)
    SubClassOf(:Y :A)
    SubClassOf(:Y :B)";
    let c = classify(body);
    assert_eq!(unsat(&c), vec!["http://t/Y".to_string()]);
    assert_eq!(c.stats().fragment, FragmentClassification::PureEl);
}

/// FP GUARD (negatives-first). The rewrite must not invent entailments: a class
/// carrying only ONE side of the negation stays satisfiable, and no spurious
/// subsumption appears.
#[test]
fn negation_rewrite_does_not_over_derive() {
    let body = "    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:D))
    SubClassOf(:A ObjectComplementOf(:B))
    SubClassOf(:D :A)";
    let c = classify(body);
    assert!(unsat(&c).is_empty(), "nothing is unsatisfiable here");
    assert!(!c.is_subclass("http://t/D", "http://t/B"), "D ⊑ B must NOT hold");
}

/// FLAG IDENTITY. Entailments must be identical with the lever off — only the
/// engine that answers may differ. Serialised because it mutates the process env.
#[test]
fn flag_off_gives_identical_entailments() {
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _g = ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let body = "    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    SubClassOf(:A ObjectComplementOf(:B))
    SubClassOf(:C :A)
    SubClassOf(:C :B)";

    let on = unsat(&classify(body));

    let prev = std::env::var_os("RUSTDL_NEG_TO_BOT_GCI");
    // SAFETY: set_var is unsafe under edition 2024; serialised by ENV_LOCK and
    // restored immediately after the classify call.
    unsafe { std::env::set_var("RUSTDL_NEG_TO_BOT_GCI", "0") };
    let off = unsat(&classify(body));
    // SAFETY: see above.
    unsafe {
        match &prev {
            Some(v) => std::env::set_var("RUSTDL_NEG_TO_BOT_GCI", v),
            None => std::env::remove_var("RUSTDL_NEG_TO_BOT_GCI"),
        }
    }

    assert_eq!(on, off, "the rewrite is a logical equivalence");
}
```

- [ ] **Step 3: Run them**

```bash
export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"
RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test negation_to_bot_gci -- --test-threads=1
```

Expected: all 5 PASS. `--test-threads=1` because two tests mutate the process
environment.

If `negated_existential_reaches_fast_path` reports a non-`PureEl` fragment, the pass is
running after an NNF view is taken, or `is_el_concept` rejects something else in the
lowered form — investigate before weakening the assertion.

- [ ] **Step 4: Commit**

```bash
RUSTUP_TOOLCHAIN=stable cargo fmt --all -- --check
RUSTUP_TOOLCHAIN=stable cargo clippy -p owl-dl-reasoner --all-targets --all-features -- -D warnings
git add crates/owl-dl-reasoner/tests/negation_to_bot_gci.rs
git commit -m "test(reasoner): end-to-end canaries for the negation→⊥-GCI rewrite

Pins that atomic negation, ¬∃R.C, and ¬(A ⊓ B) on a GCI RHS all reach the
pure-EL fast path with correct unsatisfiability; that no entailment is invented
(FP guard); and that flag-OFF entailments are identical. The ¬∃ and ¬(A⊓B) tests
are what pin the PRE-NNF placement — post-NNF they become ∀ and ⊔ respectively."
```

---

### Task 8: Corpus and ORE validation gate

This is the task that decides whether Part B ships. No code changes.

**Files:** none (measurement only; write results into the commit message and the spec).

- [ ] **Step 1: Build release**

```bash
export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"
RUSTUP_TOOLCHAIN=stable cargo build --workspace --release
```

Confirm freshness (a stale binary has produced false results in this repo before):

```bash
ls -la target/release/rustdl
```

- [ ] **Step 2: Flag ON/OFF byte-identity across the curated corpus**

```bash
S=/tmp/claude-1007/-data-dumontier-rustdl/8e753f2f-e24e-4be2-8c66-c6e13e322bae/scratchpad
for f in ontologies/real/*.ofn; do
  b=$(basename "$f")
  RUSTDL_NEG_TO_BOT_GCI=1 timeout 600 ./target/release/rustdl classify "$f" 2>/dev/null | grep -v '^#' | sort > "$S/$b.on"
  RUSTDL_NEG_TO_BOT_GCI=0 timeout 600 ./target/release/rustdl classify "$f" 2>/dev/null | grep -v '^#' | sort > "$S/$b.off"
  if diff -q "$S/$b.on" "$S/$b.off" >/dev/null; then echo "IDENTICAL $b"; else echo "DIFF     $b"; fi
done
```

Expected: `IDENTICAL` for every fixture. **Any `DIFF` is a bug** — the rewrite is a
logical equivalence, so a difference means the implementation is not faithful to it.
Diagnose before proceeding; do not proceed with a known DIFF.

- [ ] **Step 3: Reproduce the headline speedup**

```bash
S=/tmp/claude-1007/-data-dumontier-rustdl/8e753f2f-e24e-4be2-8c66-c6e13e322bae/scratchpad
O=/data/dumontier/ore-run/pool_sample/files/ore_ont_9318.owl
printf "OFF: "; ( time RUSTDL_NEG_TO_BOT_GCI=0 timeout 600 ./target/release/rustdl classify "$O" > "$S/9318.off" 2>/dev/null ) 2>&1 | grep real
printf "ON:  "; ( time RUSTDL_NEG_TO_BOT_GCI=1 timeout 600 ./target/release/rustdl classify "$O" > "$S/9318.on"  2>/dev/null ) 2>&1 | grep real
grep -v '^#' "$S/9318.off" | sort > "$S/9318.off.s"; grep -v '^#' "$S/9318.on" | sort > "$S/9318.on.s"
diff -q "$S/9318.off.s" "$S/9318.on.s" && echo "IDENTICAL closure" || diff "$S/9318.off.s" "$S/9318.on.s" | head
```

Expected: OFF ≈ 21 s, ON ≈ 1 s, closures identical. (The 2026-07-29 measurement was
21.5 s → 0.909 s with 19 479 closure lines, taken before Part A landed — treat the
*speedup* as the expectation and the ON-vs-OFF identity as the assertion, not the
absolute line count.)

- [ ] **Step 4: Enumerate the real recovery set by gate probe, not by grep**

grep ≠ gate — this repo has a recorded incident where a grep-based estimate (67) was
4× the real gate-eligible count (~40). Probe the actual fragment verdict:

```bash
S=/tmp/claude-1007/-data-dumontier-rustdl/8e753f2f-e24e-4be2-8c66-c6e13e322bae/scratchpad
: > "$S/recovery.tsv"
for O in /data/dumontier/ore-run/pool_sample/files/*.owl; do
  b=$(basename "$O" .owl)
  fon=$(RUSTDL_NEG_TO_BOT_GCI=1 timeout 120 ./target/release/rustdl classify "$O" 2>/dev/null | grep -m1 '^# fragment:')
  foff=$(RUSTDL_NEG_TO_BOT_GCI=0 timeout 120 ./target/release/rustdl classify "$O" 2>/dev/null | grep -m1 '^# fragment:')
  [ "$fon" != "$foff" ] && printf '%s\t%s\t%s\n' "$b" "$foff" "$fon" >> "$S/recovery.tsv"
done
wc -l < "$S/recovery.tsv"
```

Record the count and the named ontologies. Also time the previously-DNF candidates
`ore_ont_2397` and `ore_ont_10032` (reported DNF >120 s → 1.07 s / 2.23 s) with a
600 s timeout on both flag settings.

- [ ] **Step 5: Oracle every ontology whose output changed**

For each ontology where the **closure** (not just the fragment verdict) differs
between the pre-Part-A baseline and now, run the Konclude∩HermiT oracle and confirm
FP=0 — every newly reported `unsat` and every new subsumption must be entailed. Follow
the existing oracle harness pattern (`crates/owl-dl-reasoner/tests/materialize_oracle.rs`
shows the `robot reason --reasoner hermit` invocation this repo uses).

**A single unconfirmed new subsumption or `unsat` is stop-and-fix, not a tuning
matter.**

- [ ] **Step 6: Record the results in the spec and commit**

Append a "## Measured results (YYYY-MM-DD)" section to
`docs/superpowers/specs/2026-07-29-negation-to-bot-gci-and-conjunctive-unsat-design.md`
with: the corpus ON/OFF identity result; the `ore_ont_9318` walls; the gate-probe
recovery count and ontology list; the walls for `2397` / `10032`; and the oracle
verdict for every changed ontology.

```bash
git add docs/superpowers/specs/2026-07-29-negation-to-bot-gci-and-conjunctive-unsat-design.md
git commit -m "docs: measured results for conjunctive-unsat + negation→⊥-GCI

<fill in: corpus ON/OFF identity, ore_ont_9318 walls, gate-probe recovery count
and named ontologies, 2397/10032 walls, oracle verdict per changed ontology>"
```

---

### Task 9: Correct the stale documentation this work touched

Three stale claims were found while designing. They mislead the next reader and are
cheap to fix. Each is independent — commit separately if a reviewer objects to one.

**Files:**
- Modify: `CLAUDE.md`
- Modify: `crates/owl-dl-reasoner/src/classify.rs` (prose near `:1149-1155`)

> **Scope decision (2026-07-29).** A third stale claim was found — the debug-build
> wall sweep in `crates/owl-dl-cb/tests/cb_blowup.rs` — but that file exists only
> on `feat/cb-alch-taming`, not on `main`, and this branch is cut from `main`. That
> correction is **out of scope here**; it is already recorded in the park record
> appended to `docs/superpowers/specs/2026-07-28-cb-lazy-successor-design-seed.md`
> on the CB branch, which is where a reader of that branch will look. Steps below
> cover only the two claims that live on `main`.

- [ ] **Step 1: Fix the `DisjointClasses` claim in CLAUDE.md**

CLAUDE.md's soundness contract says `DisjointClasses` is `[excluded conservatively —
disjoint×functional-merge unproven]` from `saturator_complete_fragment`. Stale since
commit `a97cca0`: it is **admitted under `disjoint_ok`** (i.e. whenever no functional or
inverse-functional role is present). Correct the wording, and add a sentence recording
that the lowered-`⊥` form `X ⊓ Y ⊑ ⊥` is admitted on the same gate and — as of this
work — completely reasoned over by the saturator's `ConjunctiveUnsat` rule.

- [ ] **Step 2: Fix the matching prose in `classify.rs`**

The comment block at `classify.rs:1149-1155` says `DisjointClasses` is "EXCLUDED here",
immediately above the code arm (`:1325`) that admits it under `disjoint_ok`. Reword the
comment to match the code.

- [ ] **Step 3: Correct the CB blowup baseline doc comment**

`crates/owl-dl-cb/tests/cb_blowup.rs`'s wall-time sweep is a **debug-build**
measurement, and `N_BLOWUP = 13` with a 30 s timeout does not hold in release: release
S1 does n=13 in ~6.4 s, and B1 (the completeness oracle) is the *worse* engine
(n=12 ≈ 19 s, n=13 timeout). Update the doc comment with both build profiles and note
that `s1_blows_up_on_adversarial` is a **debug-only** baseline. Do **not** change
`N_BLOWUP` or delete the test — the CB arc is parked and this is a documentation
correction only, recorded in the park note at
`docs/superpowers/specs/2026-07-28-cb-lazy-successor-design-seed.md`.

- [ ] **Step 4: Verify and commit**

```bash
export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"
RUSTUP_TOOLCHAIN=stable cargo test --workspace
RUSTUP_TOOLCHAIN=stable cargo fmt --all -- --check
RUSTUP_TOOLCHAIN=stable cargo clippy --workspace --all-targets --all-features -- -D warnings
git add CLAUDE.md crates/owl-dl-reasoner/src/classify.rs crates/owl-dl-cb/tests/cb_blowup.rs
git commit -m "docs: correct three stale claims found while designing this work

1. CLAUDE.md and classify.rs:1149 both say DisjointClasses is excluded from
   saturator_complete_fragment; it has been admitted under disjoint_ok since
   a97cca0. The code arm at classify.rs:1325 contradicts the comment above it.
2. cb_blowup.rs's wall sweep is debug-build; in release S1 does n=13 in ~6.4s
   and B1 is the worse engine. N_BLOWUP is left as-is (debug-only baseline, CB
   arc parked)."
```

---

## Self-Review

**1. Spec coverage.** Walking the spec section by section:

| Spec requirement | Task |
|---|---|
| Part A: `ConjunctiveUnsat { bodies }` rule kind | Task 2 Steps 1-2 |
| Part A: index + growth + consumption | Task 2 Steps 3-6 |
| Part A: emission in the `And`-LHS arm | Task 2 Step 7 |
| Part A: axiom provenance for justify/explain | Task 3 |
| Part A: `∃R.C ⊓ D ⊑ ⊥` covered by the same rule | Task 4 Step 1 |
| Part A: `DisjointnessClash` left untouched | Task 2 (adds a new arm only; no step modifies it) |
| Part B: rewrite `X ⊑ ¬Y` → `X ⊓ Y ⊑ ⊥` | Task 5 |
| Part B: pre-NNF placement | Task 5 (module doc + `convert.rs` placement in Task 6 Step 5), pinned by Task 7 Steps 2 tests 2-3 |
| Part B: conjunctive-RHS recursion | Task 5 Step 2 test `conjunctive_rhs_splits_positive_and_negated` |
| Part B: told-disjoint preservation | Task 6 Steps 1-3 |
| Part B: flag `RUSTDL_NEG_TO_BOT_GCI` default ON | Task 5 (`enabled()`), tested Task 5 + Task 7 |
| Out of scope: `EquivalentClasses(A, ¬B)` whole axiom | Task 5 — the pass matches `Axiom::SubClassOf` only |
| Out of scope: widening `disjoint_ok` | No task touches it; Task 2 Step 9 pins the guard test |
| Gate: spelling differential | Task 1 test 2 |
| Gate: Konclude∩HermiT oracle on changed output | Task 4 Step 4, Task 8 Step 5 |
| Gate: Part B ON/OFF byte-identity | Task 8 Step 2, Task 7 test 5 |
| Gate: `ore_ont_9318` speedup | Task 8 Step 3 |
| Gate: report recovery count by gate probe, not grep | Task 8 Step 4 |
| Regression canary: 3-axiom reproducer | Task 1 test 1 |
| Regression canary: `¬∃R.C` in-fragment | Task 7 test 2 |
| Regression canary: `¬(A ⊓ B)` in-fragment | Task 7 test 3 |
| Must stay green: functional-role fallback guard | Task 2 Step 9 |
| Rollout: A unflagged and first, B flagged and second | Task ordering + Global Constraints |

No gaps. One item is in the plan but not the spec — Task 9 (stale-doc corrections);
it is additive, separable, and explicitly labelled as such.

**2. Placeholder scan.** The `<fill in …>` markers in Task 4 Step 5 and Task 8 Step 6
commit messages are deliberate: they are *measurement results* that do not exist until
the task runs, and the surrounding steps state exactly which numbers to collect. No
step describes work without showing the code or the command. No "add error handling"
or "similar to Task N" instructions.

**3. Type consistency.** `ConjunctiveUnsat { bodies: Vec<ClassId> }` is defined in
Task 2 Step 1 and referenced by that exact name and field in Steps 6-7 and in Task 3.
`ElRules::conjunctive_unsat` and `conjunctive_unsat_by_body` are used consistently
throughout. `ElRule::ConjunctiveUnsat` and `ProofTrace::conjunctive_unsat_axiom`
(Task 3 Steps 1-2) are referenced with those names in Steps 3-6.
`rewrite_negated_supers(&mut InternalOntology) -> usize` is defined in Task 5 and
called with that signature in Task 6 Step 5. `as_atomic_pair_to_bot` is defined and
used only within Task 6 Step 3.

Three tasks (5 Step 1/Step 2, 6 Step 1, 7 Step 1) begin with an explicit `grep` to
confirm real API names before writing code against them, because the plan author read
those call sites but not every signature — following the guessed name instead of the
confirmed one is the failure mode to avoid.
