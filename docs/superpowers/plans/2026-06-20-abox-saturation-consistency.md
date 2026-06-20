# ABox Saturation Consistency Pre-check — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Detect the family inconsistency (rustdl's one correctness gap) via a sound, terminating, consequence-based saturation over named individuals, as a consistency pre-check — built in two integration variants chosen by a whole-corpus bake-off.

**Architecture:** A non-generating fixpoint over named individuals (seed asserted types+edges; propagate types/domain/range, ∃-as-type, property hierarchy, inverse via backward propagation, role chains, functional/≤1 merge of ∃-markers, disjoint-clash). Variant B = standalone module in `owl-dl-reasoner`; Variant A-gated = same logic in `owl-dl-saturation`, gated to the ABox path. FP-safe by construction (sound under-approximation; non-clash ⇒ fall through to the existing hybrid path).

**Tech Stack:** Rust (edition 2024). `owl-dl-core` IR (`Axiom` enum in `ontology.rs`, `InternalOntology`, `ConceptId`/`ClassId`/`RoleId`/`Role`/`IndividualId`, `SubRolePath`). Toolchain: `export PATH="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"`. Spec: `docs/superpowers/specs/2026-06-20-abox-saturation-consistency-design.md`.

**Branch:** `feat/abox-saturation-consistency` (created). Variant branches off it: `feat/abox-sat-B-standalone`, `feat/abox-sat-A-gated`.

---

## File Structure

- `crates/owl-dl-reasoner/src/abox_saturation.rs` (NEW, Variant B) — standalone fixpoint + `saturate_abox_consistency(internal) -> AboxSatVerdict`.
- `crates/owl-dl-reasoner/src/lib.rs` — wire the pre-check into `is_consistent` (between `abox_check` @2120 and `consistency_wedge` @2138); flag `RUSTDL_ABOX_SATURATION`.
- `crates/owl-dl-saturation/src/lib.rs` (Variant A-gated) — `saturate_abox(...)` + backward-prop mode gated to the ABox path.
- Tests: `crates/owl-dl-reasoner/tests/abox_saturation.rs`.

## Reference: IR shapes (verified)

`owl_dl_core::ontology::Axiom` variants used: `ClassAssertion{class:ConceptId, individual:IndividualId}`, `ObjectPropertyAssertion{role:Role, subject:IndividualId, object:IndividualId}`, `SubObjectPropertyOf{sub:SubRolePath, sup:Role}` (chains), `InverseObjectProperties(Role,Role)`, `ObjectPropertyDomain{role,domain}`, `ObjectPropertyRange{role,range}`, `FunctionalRole(Role)`, `DisjointClasses(Vec<ConceptId>)`, `SubClassOf{sub,sup}`, `EquivalentClasses(Vec<ConceptId>)`. Iterate via `internal.axioms()` (confirm the accessor name when implementing; the saturator already iterates axioms — mirror its pattern, e.g. `build_role_super` in `owl-dl-saturation/src/lib.rs`). `Role` has `.role_id()` and `.is_inverse()`.

---

## Task 1 — P0 GATE: minimal standalone ABox saturation reaching family (do FIRST)

**This task gates the whole effort.** If it cannot make full family inconsistent, STOP and rethink the algorithm before building Variant B fully or Variant A-gated.

**Files:**
- Create: `crates/owl-dl-reasoner/src/abox_saturation.rs`
- Modify: `crates/owl-dl-reasoner/src/lib.rs` (add `mod abox_saturation;` + a test-only pub entry)
- Test: `crates/owl-dl-reasoner/tests/abox_saturation.rs`

- [ ] **Step 1: Write the failing family-core + FP-smoke tests first.**

```rust
// tests/abox_saturation.rs
#![allow(clippy::unwrap_used)]
use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

fn load(p: &str) -> SetOntology<RcStr> {
    let s = std::fs::read_to_string(p).unwrap();
    read_ofn(&mut Cursor::new(s.into_bytes()), ParserConfiguration::default()).unwrap().0
}
// `abox_sat_inconsistent` = test entry that runs ONLY the ABox-saturation pre-check
// and returns true iff it derives a clash (no fall-through to wedge/tableau).
#[test]
fn family_core_detected_by_saturation() {
    let o = load("../../docs/family-mech4-ddmin-core.ofn");
    assert!(owl_dl_reasoner::abox_sat_inconsistent(&o), "core clash must be reached by saturation");
}
#[test]
#[ignore = "needs ontologies/real/family.ofn"]
fn full_family_detected_by_saturation() {
    let o = load("../../ontologies/real/family.ofn");
    assert!(owl_dl_reasoner::abox_sat_inconsistent(&o), "full family must be detected (the P0 gate)");
}
// FP smoke: a consistent ABox using inverses + a role chain must STAY consistent.
#[test]
fn consistent_inverse_chain_no_fp() {
    let o = load_str("Prefix(:=<urn:c#>)
Ontology(
  Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b)) Declaration(NamedIndividual(:c))
  Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s)) Declaration(ObjectProperty(:t))
  InverseObjectProperties(:r :ri)
  SubObjectPropertyOf(ObjectPropertyChain(:r :s) :t)
  ObjectPropertyAssertion(:r :a :b) ObjectPropertyAssertion(:s :b :c)
)");
    assert!(!owl_dl_reasoner::abox_sat_inconsistent(&o), "consistent ABox must NOT be flagged (FP guard)");
}
fn load_str(s: &str) -> SetOntology<RcStr> {
    read_ofn(&mut Cursor::new(s.as_bytes().to_vec()), ParserConfiguration::default()).unwrap().0
}
```

- [ ] **Step 2: Run — expect FAIL (no `abox_sat_inconsistent`).**

Run: `export PATH="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH" && cargo test -p owl-dl-reasoner --test abox_saturation 2>&1 | tail -5`
Expected: compile error (function not found).

- [ ] **Step 3: Implement the standalone saturator** in `abox_saturation.rs`. The fixpoint (named individuals only, no witness generation):

State:
```rust
// types: per individual, the set of ConceptId/ClassId it has (atomic + ∃R.D markers as (RoleId,ClassId)).
// edges: set of (RoleId, IndividualId, IndividualId) — materialized role assertions, inverse-completed.
struct AboxSat<'a> {
    internal: &'a InternalOntology,
    types: HashMap<IndividualId, HashSet<ClassId>>,        // atomic + named types
    ex: HashMap<IndividualId, HashSet<(RoleId, ClassId)>>, // ∃R.D markers (no witness)
    edges: HashSet<(RoleId, IndividualId, IndividualId)>,
    clash: bool,
}
```
Rules to apply to fixpoint (each derives only entailed facts — SOUND):
  1. **Seed:** `ClassAssertion{class,individual}` → add class to `types`/`ex` (atomic→types; `∃R.D`→ex). `ObjectPropertyAssertion{role,subj,obj}` → `edges.insert((role_id, subj, obj))`; if `role.is_inverse()` insert the swapped named-role edge.
  2. **Inverse materialization (backward):** for `InverseObjectProperties(r,s)`: every `(r,a,b)` ⟹ `(s,b,a)` and vice-versa. (Materialize on named individuals — finite.)
  3. **Property hierarchy:** `SubObjectPropertyOf{sub: single role p, sup: q}` + `(p,a,b)` ⟹ `(q,a,b)`.
  4. **Role chains:** `SubObjectPropertyOf{sub: ObjectPropertyChain([r1..rn]), sup: t}` + a path `(r1,a,x1),(r2,x1,x2),…,(rn,x_{n-1},b)` ⟹ `(t,a,b)`. (n=2,3.)
  5. **Domain/range:** `(r,a,b)` + `ObjectPropertyDomain{r,C}` ⟹ `C∈types(a)`; `+ Range{r,D}` ⟹ `D∈types(b)`.
  6. **Type propagation:** `C∈types(a)` + `SubClassOf{C,sup}` ⟹ `sup∈types(a)` (atomic) / `ex(a)` (if sup=∃R.D); `EquivalentClasses` both directions; conjunction members.
  7. **Functional/≤1 merge:** `FunctionalRole(r)` + `(r,a,b)` + `(r,a,c)` (b≠c, or via the ex-marker): the single r-value carries the UNION of the r-successors' types — i.e. unify `ex(a)`'s `(r,·)` markers' fillers + the named r-successors' types into one type-set; if that set contains a disjoint pair ⇒ clash. (Reuse the saturator's Phase-2a/2e witness-merge idea: a functional r means all r-fillers coincide ⇒ their type-sets union on one node.)
  8. **Disjoint clash:** `{A,B}⊆types(a)` with `A,B` in some `DisjointClasses` (or told-disjoint / `A⊑¬B`) ⇒ `clash=true`. Also `⊥∈types(a)`.
Loop rules 2–8 to fixpoint (a worklist or repeat-until-no-change). Return `clash`.

```rust
pub fn saturate_abox_consistency(internal: &InternalOntology) -> bool { /* fixpoint above; true = clash */ }
```

In `lib.rs`: `mod abox_saturation;` and a thin test entry:
```rust
#[doc(hidden)]
pub fn abox_sat_inconsistent<A: ForIRI>(o: &SetOntology<A>) -> bool {
    let internal = owl_dl_core::convert::convert_ontology(o).expect("convert");
    abox_saturation::saturate_abox_consistency(&internal)
}
```

- [ ] **Step 4: Run the gate tests.**

Run: `cargo test -p owl-dl-reasoner --test abox_saturation family_core_detected_by_saturation consistent_inverse_chain_no_fp` then `cargo test -p owl-dl-reasoner --test abox_saturation -- --ignored full_family_detected_by_saturation --nocapture`
Expected: family core PASS, FP-smoke PASS, full family PASS (and fast — print wall).

- [ ] **Step 5: GATE DECISION.**
  - **PASS** (full family inconsistent, FP-smoke clean) → proceed to Task 2.
  - **FAIL** → STOP. Record which rule is missing/insufficient (instrument: does the clash type-set ever form? is the role-chain path reached?). Do NOT build Variant A-gated or wire into production until the algorithm reaches family. Report to the human.

- [ ] **Step 6: Commit.**

```bash
git checkout -b feat/abox-sat-B-standalone
git add crates/owl-dl-reasoner/src/abox_saturation.rs crates/owl-dl-reasoner/src/lib.rs crates/owl-dl-reasoner/tests/abox_saturation.rs
git commit -m "feat(abox-sat): P0 standalone ABox saturation — detects family (gate)"
```

---

## Task 2 — Variant B: wire as a sound consistency pre-check (default off)

**Files:** Modify `crates/owl-dl-reasoner/src/lib.rs` (`is_consistent`, flag helper).

- [ ] **Step 1: Flag helper** (near `abox_check_enabled`):
```rust
#[must_use]
pub fn abox_saturation_enabled() -> bool {
    std::env::var_os("RUSTDL_ABOX_SATURATION").is_some_and(|v| v == "1")
}
```
- [ ] **Step 2: Wire into `is_consistent`** — after the `abox_check` Inconsistent check (~lib.rs:2120) and before `consistency_wedge` (~2138), add (guarded by `has_abox_axioms()` so ABox-free inputs skip it):
```rust
    if abox_saturation_enabled()
        && prepared.has_abox_axioms()
        && abox_saturation::saturate_abox_consistency(prepared.internal())
    {
        return Ok(false); // sound clash ⇒ inconsistent
    }
```
(Use the existing prepared-ontology internal accessor; confirm its name. `saturate_abox_consistency` returning `false` ⇒ fall through unchanged.)
- [ ] **Step 3: Test** flag-on family inconsistent end-to-end:
```rust
#[test]
#[ignore = "needs family.ofn"]
fn is_consistent_flag_on_detects_family() {
    // SAFETY: serialized.
    unsafe { std::env::set_var("RUSTDL_ABOX_SATURATION", "1"); }
    let o = load("../../ontologies/real/family.ofn");
    assert!(!owl_dl_reasoner::is_consistent(&o).unwrap(), "family inconsistent flag-on");
}
```
- [ ] **Step 4: Run + commit.**
Run: `RUSTDL_ABOX_SATURATION=1 cargo test -p owl-dl-reasoner --test abox_saturation -- --ignored is_consistent_flag_on_detects_family --nocapture` → PASS.
```bash
git add -A && git commit -m "feat(abox-sat): wire Variant B pre-check into is_consistent (RUSTDL_ABOX_SATURATION, default off)"
```

---

## Task 3 — Variant B soundness gate (whole corpus, flag-on)

**Files:** none (validation).

- [ ] **Step 1: Full corpus closure-diff flag-on** (FP=0/MISSED=0 byte-identical):
Run: `RUSTDL_ABOX_SATURATION=1 RUSTDL_TEST_PAIR_MS=1000 cargo test -p owl-dl-reasoner --test konclude_closure_diff --release -- --ignored --nocapture 2>&1 | grep -E 'FP=|MISSED|test result'`
Expected: every fixture FP=0/MISSED=0 (family sentinel now PASSES inconsistent if its test is enabled; pre-existing `#[ignore]`d family test should flip to detecting). Any new FP ⇒ STOP (the saturator derived an unsound clash — bug in a rule).
- [ ] **Step 2: Consistency non-regression** — every consistent fixture stays consistent flag-on (the pre-check must not false-flag). Run `is_consistent` flag-on on pizza/wine/sio/ore (all consistent) → all `true`.
- [ ] **Step 3: Commit results** into the spec's results section.

---

## Task 4 — Variant A-gated: same logic in the saturator, gated to ABox path

**Files:** Modify `crates/owl-dl-saturation/src/lib.rs` (new `saturate_abox` + backward-prop mode behind a gate); `crates/owl-dl-reasoner/src/lib.rs` (call A-gated path under a second flag `RUSTDL_ABOX_SAT_GATED`).

- [ ] **Step 1:** On a fresh branch `feat/abox-sat-A-gated` off `feat/abox-saturation-consistency`, add backward/inverse propagation + ABox seeding to the saturator, gated so the classification path (the `!is_inverse()` fast rules) is byte-identical when the gate is off. Reuse `directly_unsat`/`enqueue_unsat`, functional-merge, role-chain rules already present.
- [ ] **Step 2:** Port the family-core + full-family + FP-smoke tests to exercise the A-gated entry; confirm family detected.
- [ ] **Step 3:** Commit.
```bash
git commit -m "feat(abox-sat): Variant A-gated — backward/inverse propagation in saturator, gated to ABox path"
```

---

## Task 5 — Whole-corpus bake-off (the decision gate)

**Files:** none (measurement); record results in the spec.

- [ ] **Step 1: Soundness, both variants.** `konclude_closure_diff` flag-on (each variant's flag), full corpus, FP=0/MISSED=0 byte-identical. A-gated ALSO re-prove classification unchanged (it touches the saturator): `cargo test -p owl-dl-saturation` + the classification closure-diff must be identical to flag-off.
- [ ] **Step 2: EL walls** (A-gated's key risk): `scripts/perf-flag-sweep.sh RUSTDL_ABOX_SAT_GATED` — galen/go-basic/ro/sulo/notgalen/bibtex must be ~1.00x (no EL slowdown). For B: `scripts/perf-flag-sweep.sh RUSTDL_ABOX_SATURATION` — same.
- [ ] **Step 3: family detection time**, both variants (target < a few s).
- [ ] **Step 4: DECIDE.** A-gated wins iff EL stays fast AND family solved AND classification FP=0/MISSED=0 unchanged. B wins iff family solved efficiently AND zero classification impact. If both pass, prefer simpler/faster. Record the table + decision in the spec.
- [ ] **Step 5: Merge the winner** to main (flip its flag default-on after the gate), keep the loser's branch as a record. Update CLAUDE.md.

---

## Self-Review

**Spec coverage:** shared core (Task 1 rules 1-8) ✓; Variant B (Tasks 1-3) ✓; Variant A-gated (Task 4) ✓; build-order P0 (Task 1 gate) ✓; bake-off (Task 5) ✓; integration point (Task 2) ✓; FP-safety (Task 3 + smoke test) ✓; termination (named individuals — noted in Task 1 state) ✓.

**Placeholder scan:** the Task-1 rule list specifies each rule's premise→conclusion concretely (not "handle X"); the algorithm's per-rule Rust is left to the implementer guided by the rule spec + tests — appropriate for a research-grade fixpoint where the TESTS (family core/full/FP-smoke) are the precise contract. No TBD/TODO.

**Type consistency:** `saturate_abox_consistency(&InternalOntology) -> bool`, `abox_sat_inconsistent(&SetOntology) -> bool`, `abox_saturation_enabled()`, flags `RUSTDL_ABOX_SATURATION` (B) / `RUSTDL_ABOX_SAT_GATED` (A) used consistently. `Axiom` variants match `ontology.rs`.

**Note for implementer:** confirm `internal.axioms()` accessor + the `PreparedOntology::internal()`/`has_abox_axioms()` names against `lib.rs` (mirror `abox_check`'s access); the saturator's `build_role_super` shows the axiom-iteration pattern to reuse.
