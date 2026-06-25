# Marker-saturator ⊔ failed-literal look-ahead GATE — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Measure whether the full marker saturator, wired as a failed-literal propagator at the wedge's ⊔ points, collapses wine's hard-class model builds — a decisive GO/FLOOR/UNSOUND gate.

**Architecture:** A new public seed-saturation API in `owl-dl-saturation` (`build_base` once + `seed_unsat` per-call, via a cloned worklist engine and a reserved synthetic class) is wired behind `RUSTDL_SAT_LOOKAHEAD` into `hyper.rs`'s ⊔ branch path as a branch-scoped failed-literal drop. A controller-run harness measures branch collapse on hard wine classes against the MRV baseline and gates wine FP=0 first.

**Tech Stack:** Rust (edition 2024, 1.88+), `owl-dl-saturation` (WorklistEngine), `owl-dl-tableau` (HyperEngine wedge), `fixedbitset`, the existing `sat_class_probe`/`decide_pair_probe` test harness.

## Global Constraints

- FP=0 is sacred; the **wine** closure-diff (run FIRST) is the proof, not any by-construction argument.
- This is a **gate/spike**: Unit-2 hook + Unit-3 harness are throwaway; only the verdict doc is durable. The Unit-1 `seed_sat` API is the one keep-on-GO piece. Nothing merges to `main`; branch is `feat/marker-saturator-lookahead-gate` off `feat/build-once-redesign`.
- `RUSTDL_SAT_LOOKAHEAD` default OFF; flag-OFF path byte-identical to the integration branch base.
- `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets --all-features -- -D warnings` (pedantic) clean; `cargo test --workspace` green (flag-OFF).
- Toolchain: `export RUSTUP_HOME=/home/dumontier/.rustup CARGO_HOME=/home/dumontier/.cargo; export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$CARGO_HOME/bin:$PATH"`.
- Commit only when the controller says so; trailers on every commit:
  `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`
  `Claude-Session: https://claude.ai/code/session_01HSzon7V2wkhrudxBNAJduh`.

---

### Task 0: Branch setup

**Files:** none (git only).

- [ ] **Step 1: Create the branch off the integration branch**

```sh
git checkout feat/build-once-redesign
git checkout -b feat/marker-saturator-lookahead-gate
git log --oneline -1   # expect 832e486 (spec commit) at or near HEAD
```

No commit (branch creation only).

---

### Task 1: Unit 1 — `seed_sat` API in `owl-dl-saturation`

Build the node-independent base once, then answer "is `⊓seed` unsatisfiable?" per call by cloning the engine and seeding a reserved synthetic class.

**Files:**
- Create: `crates/owl-dl-saturation/src/seed_sat.rs`
- Modify: `crates/owl-dl-saturation/src/lib.rs` (add `pub mod seed_sat;`; make `WorklistEngine` + its field types `#[derive(Clone)]`; add a `pub(crate)` constructor path that reserves one extra synthetic class and exposes the built+run engine; expose `pub(crate)` accessors `inject_subsumer`, `inject_existential`, `run`, `is_unsat_class`).
- Test: tests live inline in `seed_sat.rs` under `#[cfg(test)]`.

**Interfaces:**
- Consumes: `owl_dl_core::ir::{InternalOntology, ClassId, RoleId, ConceptId, ConceptExpr}`; the existing `WorklistEngine` (private) and `Subsumers::is_unsatisfiable`.
- Produces (the public API later tasks rely on):
  - `pub struct SeedSaturator` — holds the once-built, fully-run base engine + `reserved_x: ClassId`.
  - `pub fn build_base(internal: &InternalOntology) -> SeedSaturator`
  - `pub fn SeedSaturator::seed_unsat(&self, atomic_seed: &[ClassId], exists_seed: &[(RoleId, ClassId)]) -> bool` — true iff `X ⊑ ⊥` derived where `X ⊑ aᵢ` for each atomic seed and `X ⊑ ∃rⱼ.cⱼ` for each existential seed. `X` is the reserved synthetic class; the call clones the base engine, injects, runs to fixpoint, and reads `is_unsatisfiable(X)`. The base is never mutated (clone-per-call).
  - `pub fn SeedSaturator::class_of_concept(&self, internal: &InternalOntology, cid: ConceptId) -> Option<ClassId>` — returns the atomic `ClassId` for an `Atomic` concept, else `None` (used by Unit 2 to build the atomic seed subset).

- [ ] **Step 1: Reserve a synthetic class + make the engine cloneable**

In `lib.rs`, locate `saturate_with_config` (line ~120) and the `WorklistEngine` struct (line ~161). Add `#[derive(Clone)]` to `WorklistEngine` and to any owned field type that is not already `Clone` (the field types are `Subsumers`, `Vec<FixedBitSet>`, `Vec<ExistentialFact>`, `HashSet`, `VecDeque`, `ElRules`, `TseitinAllocator`, `HashMap`, `BTreeSet` — add `#[derive(Clone)]` to `ExistentialFact`, `ElRules`, `TseitinAllocator`, `AtomicSubsumption`, `ConjunctiveTrigger`, and any other owned struct/enum reachable from a field if the compiler reports it is not `Clone`). Build after each addition to let the compiler name the next missing `Clone`.

- [ ] **Step 2: Add `pub(crate)` engine hooks in `lib.rs`**

Add these methods on `impl WorklistEngine` (near `run`, ~663). `inject_subsumer` mirrors how `seed` enqueues a told subsumption; `inject_existential` mirrors how an `∃` fact is pushed. Confirm the exact queue field names against `seed` (~508) and `push_fact` (~739) before writing — they are `todo_subsumer: VecDeque<(ClassId, ClassId)>` and `push_fact(ExistentialFact) -> Option<usize>`.

```rust
impl WorklistEngine {
    /// Enqueue `c ⊑ d` as a starting fact and record it in the closure,
    /// exactly as `seed` does for told subsumptions. Used by the seed-sat API.
    pub(crate) fn inject_subsumer(&mut self, c: ClassId, d: ClassId) {
        self.todo_subsumer.push_back((c, d));
    }

    /// Enqueue `c ⊑ ∃role.target` as a starting existential fact.
    pub(crate) fn inject_existential(&mut self, c: ClassId, role: RoleId, target: ClassId) {
        self.push_fact(ExistentialFact::new(c, role, target));
    }

    pub(crate) fn is_unsat_class(&self, c: ClassId) -> bool {
        self.subsumers.is_unsatisfiable(c)
    }
}
```

If `ExistentialFact` has no `new`, construct it with its public/`pub(crate)` field literal form as used inside `push_fact`'s callers (read ~739–805 to copy the exact construction).

- [ ] **Step 3: Expose a base-builder that reserves one extra class id**

In `lib.rs`, add a `pub(crate)` function that mirrors `saturate_with_config` but sizes the universe with **one extra** synthetic id (the reserved `X`) and returns the run engine plus that id:

```rust
/// Build and fully run the base engine, reserving one extra synthetic class id
/// `X` (above the Tseitin universe) that carries no axioms. Returns `(engine, X)`.
pub(crate) fn build_run_engine_with_reserved(
    internal: &InternalOntology,
) -> (WorklistEngine, ClassId) {
    let n = internal.vocabulary.num_classes();
    let role_super_map = build_role_super(internal);
    let (rules, tseitin, num_total_classes, maybe_trace) =
        collect_el_rules_with_provenance(internal, &role_super_map, false);
    let role_super = freeze_role_super(&role_super_map);
    // Reserve one id above the existing universe for the seed query class X.
    let reserved_x = ClassId::new(u32::try_from(num_total_classes).expect("fits u32"));
    let mut engine = WorklistEngine::new(
        n,
        num_total_classes + 1, // size all bitsets to include X
        rules,
        tseitin,
        role_super,
        false,
        maybe_trace,
    );
    engine.seed(internal);
    engine.run();
    (engine, reserved_x)
}
```

Confirm `WorklistEngine::new` (~273) sizes its bitsets from the `num_total_classes` argument; if any index is built from `internal` directly rather than the passed size, adjust so `X`'s id is within range of `subsumers`, `subsumed_by`, `unsatisfiable`, and the per-class `Vec` indices (push empty entries for the reserved id as needed). The reserved id must be addressable by `inject_subsumer`/`is_unsat_class` without panic.

- [ ] **Step 4: Write `seed_sat.rs`**

```rust
//! Seed-saturation query API for the ⊔ failed-literal look-ahead gate.
//!
//! `build_base` runs the marker saturator once; `seed_unsat` answers
//! "is `⊓seed` unsatisfiable?" by cloning the base engine, injecting the
//! seed into a reserved synthetic class `X`, running to fixpoint, and
//! reading `X ⊑ ⊥`. The base is immutable across calls (clone-per-call) —
//! naive but correct; the gate measures branch counts, not wall.

use owl_dl_core::ir::{ClassId, ConceptExpr, ConceptId, InternalOntology, RoleId};

use crate::{build_run_engine_with_reserved, WorklistEngine};

pub struct SeedSaturator {
    base: WorklistEngine,
    reserved_x: ClassId,
}

#[must_use]
pub fn build_base(internal: &InternalOntology) -> SeedSaturator {
    let (base, reserved_x) = build_run_engine_with_reserved(internal);
    SeedSaturator { base, reserved_x }
}

impl SeedSaturator {
    /// True iff `⊓atomic_seed ⊓ ⊓∃exists_seed` is unsatisfiable per the
    /// marker saturator. Clones the base engine per call.
    #[must_use]
    pub fn seed_unsat(
        &self,
        atomic_seed: &[ClassId],
        exists_seed: &[(RoleId, ClassId)],
    ) -> bool {
        let mut e = self.base.clone();
        let x = self.reserved_x;
        for &a in atomic_seed {
            e.inject_subsumer(x, a);
        }
        for &(r, c) in exists_seed {
            e.inject_existential(x, r, c);
        }
        e.run();
        e.is_unsat_class(x)
    }

    #[must_use]
    pub fn class_of_concept(
        &self,
        internal: &InternalOntology,
        cid: ConceptId,
    ) -> Option<ClassId> {
        match internal.concepts.get(cid) {
            ConceptExpr::Atomic(c) => Some(*c),
            _ => None,
        }
    }
}
```

- [ ] **Step 5: Write the unit tests (inline in `seed_sat.rs`)**

Use the same `InternalOntology` builder helpers the existing `lib.rs` tests use (read the `#[cfg(test)]` block at ~4250+ for the `class(&internal, "Name")` helper and the ontology-construction pattern; reuse that harness module or replicate its builder).

```rust
#[cfg(test)]
mod tests {
    use super::*;
    // Build a tiny ontology with: DisjointClasses(A, B); C ⊑ ∃r.{a}; ∀r.K on D;
    // a ∉ K (so D ⊓ ∃r.{a} clashes via ForallKey). Reuse the lib.rs test builder.

    #[test]
    fn told_disjoint_seed_is_unsat() {
        // A, B told-disjoint → seed {A, B} unsat.
        let (internal, ids) = build_disjoint_ab(); // helper: returns class ids
        let sat = build_base(&internal);
        assert!(sat.seed_unsat(&[ids.a, ids.b], &[]));
        assert!(!sat.seed_unsat(&[ids.a], &[]));      // A alone is sat
        assert!(!sat.seed_unsat(&[ids.a, ids.c], &[])); // A, C compatible
    }

    #[test]
    fn forall_key_seed_is_unsat() {
        // The relation horn_fixpoint MISSED: ∀r.K + ∃r.{a}, a ∉ K → unsat.
        let (internal, ids) = build_forall_key_clash();
        let sat = build_base(&internal);
        // D carries ∀r.K; seed D together with ∃r.a-witness → clash.
        assert!(sat.seed_unsat(&[ids.d], &[(ids.r, ids.a_nom)]));
        assert!(!sat.seed_unsat(&[ids.d], &[(ids.r, ids.k_member)])); // in-range: sat
    }
}
```

Write `build_disjoint_ab` and `build_forall_key_clash` as test helpers constructing the ontologies via the existing builder. The `forall_key` fixture must reproduce wine's pattern (`∀hasColor.{Red,White,Rosé}` + `∃hasColor.{Sweet}`-style mismatch) at minimal size; if the in-tree builder cannot express `∀R.OneOf`, load a tiny hand-written `.ofn` fixture from `crates/owl-dl-saturation/tests/fixtures/` instead and parse it via the same path `lib.rs` tests use.

- [ ] **Step 6: Run the tests and fmt/clippy**

```sh
cargo test -p owl-dl-saturation seed_sat -- --nocapture
cargo fmt --all -- --check
cargo clippy -p owl-dl-saturation --all-targets --all-features -- -D warnings
```
Expected: both tests PASS; fmt/clippy clean.

- [ ] **Step 7: Confirm the OFF-path is untouched**

```sh
cargo test -p owl-dl-saturation
```
Expected: all pre-existing saturation tests still PASS (the `Clone` derives + reserved-id builder must not change `saturate`'s output).

- [ ] **Step 8: Commit**

```sh
git add crates/owl-dl-saturation/src/seed_sat.rs crates/owl-dl-saturation/src/lib.rs \
        crates/owl-dl-saturation/tests/fixtures 2>/dev/null
git commit -m "feat(sat-gate): seed-saturation API (build_base + seed_unsat) for ⊔ look-ahead

<trailers>"
```

---

### Task 2: Unit 2 — `RUSTDL_SAT_LOOKAHEAD` ⊔ failed-literal hook in `hyper.rs`

Drop disjuncts the seed-saturator proves dead, branch-scoped, at the MRV-chosen ⊔.

**Files:**
- Modify: `crates/owl-dl-tableau/src/hyper.rs` (HyperEngine: add `sat_lookahead: Option<Arc<SeedSaturator>>` field + builder; the drop in the ⊔ branch loop; instrumentation counters).
- Modify: `crates/owl-dl-tableau/Cargo.toml` (add `owl-dl-saturation` dep if not already present — check first).
- Modify: `crates/owl-dl-reasoner/src/lib.rs` (env reader `hyper_sat_lookahead_enabled()`; build the `SeedSaturator` once and thread it into the wedge at every wedge-construction site, mirroring the MRV `with_mrv_ordering` wiring).
- Test: `crates/owl-dl-tableau/tests/sat_lookahead_drop.rs` (new).

**Interfaces:**
- Consumes: `owl_dl_saturation::seed_sat::{SeedSaturator, build_base}` (Task 1); the existing MRV `find_open_disjunction` (~1936) and the ⊔ branch loop (~1734–1773); the existing `head_atom_satisfied`/`SearchStats`.
- Produces:
  - `HyperEngine` field `sat_lookahead: Option<Arc<SeedSaturator>>` + `with_sat_lookahead(self, Arc<SeedSaturator>) -> Self` builder + `#[cfg(test)] sat_lookahead_for_test`.
  - `SearchStats` counters `lookahead_calls: u64`, `lookahead_dropped: u64`, `lookahead_forced_single: u64`.
  - `reasoner::hyper_sat_lookahead_enabled() -> bool` (default OFF: `std::env::var_os("RUSTDL_SAT_LOOKAHEAD").is_some_and(|v| v != "0" && !v.is_empty())`).

- [ ] **Step 1: Add the field, builder, counters (no behaviour yet)**

Mirror the `mrv_ordering` scaffolding exactly. Add `sat_lookahead: Option<std::sync::Arc<owl_dl_saturation::seed_sat::SeedSaturator>>` to `HyperEngine`, default `None` in all 3 constructors (`new` ~729, `new_with_prebuilt` ~768, `new_seeded` ~1630). Add:

```rust
#[must_use]
pub fn with_sat_lookahead(
    mut self,
    s: std::sync::Arc<owl_dl_saturation::seed_sat::SeedSaturator>,
) -> Self {
    self.sat_lookahead = Some(s);
    self
}
```

Add the three `u64` counters to `SearchStats` (find its struct def) and initialise them to 0. Build to confirm scaffolding compiles. No commit yet.

- [ ] **Step 2: Write the failing test**

First, locate the in-crate probe harness: `grep -rn "fn sat_class_probe\|struct SearchStats\|fn .*_probe" crates/owl-dl-tableau/tests crates/owl-dl-tableau/src`. Mirror whichever existing test constructs a `HyperEngine` and runs a class-satisfiability probe returning `SearchStats` (the MRV selection test added with the `mrv_ordering` merge is the closest analog). Build the test on that harness with this exact shape and these exact assertions:

```rust
// crates/owl-dl-tableau/tests/sat_lookahead_drop.rs
//
// A node whose label forces C ⊑ D1 ⊔ D2 where D1 is told-disjoint with a label
// atom (so seed_unsat({label.., D1}) = true) and D2 is live. Look-ahead ON drops
// D1 → forced single; verdict unchanged (Sat). OFF branches both.

#[test]
fn lookahead_drops_dead_disjunct_off_branches_it() {
    let (internal, c_id) = build_disjunctive_node_fixture(); // D1 dead, D2 live
    let sat = std::sync::Arc::new(owl_dl_saturation::seed_sat::build_base(&internal));

    // OFF: construct the HyperEngine as the analog test does, no with_sat_lookahead.
    let stats_off = run_sat_probe(&internal, c_id, /*lookahead=*/ None);
    // ON: same construction + .with_sat_lookahead(sat.clone()).
    let stats_on = run_sat_probe(&internal, c_id, Some(sat.clone()));

    assert_eq!(stats_on.verdict, stats_off.verdict, "verdict must be invariant");
    assert!(stats_on.verdict.is_sat(), "control class is satisfiable");
    assert!(stats_on.lookahead_dropped >= 1, "ON drops the dead disjunct");
    assert!(stats_on.lookahead_forced_single >= 1, "ON forces the lone survivor");
    assert_eq!(stats_off.lookahead_dropped, 0, "OFF never drops");
}
```

`build_disjunctive_node_fixture` and `run_sat_probe` are thin wrappers you write over the located harness (`run_sat_probe` constructs the engine with/without `.with_sat_lookahead` and returns its `SearchStats`). Adjust `verdict.is_sat()` to whatever the harness's verdict enum exposes (e.g. `matches!(v, Verdict::Sat)`). Keep the four assertions exactly.

- [ ] **Step 3: Run it to confirm it fails**

```sh
cargo test -p owl-dl-tableau --test sat_lookahead_drop
```
Expected: FAIL (look-ahead drop not implemented; counters stay 0).

- [ ] **Step 4: Implement the drop in the ⊔ branch path**

At the ⊔ branch site (~1734–1773), after MRV selects `(ci, node, binding)` and before the per-disjunct branch loop, when `self.sat_lookahead` is `Some(sat)`:

1. Build the **seed** from the node's current label: `atomic_seed` = the label concepts that map to atomic `ClassId` via `sat.class_of_concept(internal, cid)`; `exists_seed` = the `∃R.C`-marker subset (the same markers `head_atom_satisfied`'s `Atom::Exists` arm reads — collect `(role_id, filler_class)` for non-inverse role with atomic filler).
2. For each candidate disjunct `Dₖ`: form `seed_k = (atomic_seed + atomic(Dₖ), exists_seed + ∃(Dₖ))` and call `sat.seed_unsat(...)`; bump `lookahead_calls`. If it returns `true`, mark `Dₖ` dead (skip it in the branch loop), bump `lookahead_dropped`.
3. Live survivors = candidates not dropped. If `0` → treat as a clash for this node (no branch; return as the existing "all disjuncts fail" path does). If `1` → branch only it and bump `lookahead_forced_single`. If `≥2` → branch survivors as today.
4. **Branch-scoped:** compute this drop set fresh at each ⊔ visit; never persist it across backtracks.

Concretely, gate the branch loop on a `live: Vec<usize>` (indices into the head) computed from the look-ahead when `sat_lookahead.is_some()`, else `(0..head_len).collect()`. The existing loop iterates `live` instead of `0..head_len`.

Keep the seed-building helper small and local (a private `fn lookahead_live_disjuncts(&mut self, internal, ci, node, binding) -> Vec<usize>`); do **not** thread `internal` if the engine already holds the concept pool — reuse the engine's existing pool/label accessors used by `head_atom_satisfied`.

- [ ] **Step 5: Run the test to confirm it passes**

```sh
cargo test -p owl-dl-tableau --test sat_lookahead_drop
```
Expected: PASS.

- [ ] **Step 6: Wire the env flag + build the saturator once (reasoner)**

In `crates/owl-dl-reasoner/src/lib.rs`: add `hyper_sat_lookahead_enabled()` (default OFF, idiom above). At every wedge-construction site that already calls `with_mrv_ordering`, when the flag is on, build `Arc::new(owl_dl_saturation::seed_sat::build_base(internal))` **once per ontology** (hoist above the per-pair loop — never per pair) and pass it via `.with_sat_lookahead(arc.clone())`. Add `owl-dl-saturation` to `owl-dl-reasoner`'s deps if absent.

- [ ] **Step 7: Confirm flag-OFF byte-identical + clippy**

```sh
cargo test -p owl-dl-tableau
cargo test -p owl-dl-reasoner
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
```
Expected: all green with the flag unset (default OFF path unchanged).

- [ ] **Step 8: Commit**

```sh
git add crates/owl-dl-tableau crates/owl-dl-reasoner
git commit -m "feat(sat-gate): RUSTDL_SAT_LOOKAHEAD ⊔ failed-literal drop (default OFF)

<trailers>"
```

---

### Task 3: Unit 3 — gate harness, measurement, and verdict (controller-run)

**Files:**
- Create: `crates/owl-dl-reasoner/tests/sat_lookahead_gate.rs` (branch-count probes; `#[ignore]`d, run explicitly).
- Create (durable): `docs/marker-saturator-lookahead-gate-results-2026-06-25.md`.

**Interfaces:**
- Consumes: the `sat_class_probe`/`decide_pair_probe` big-stack harness (adaptive-budget OFF) used by prior gates; `RUSTDL_SAT_LOOKAHEAD`; `konclude_closure_diff` (the existing oracle test) with `TEST_PAIR_MS`.

- [ ] **Step 1: Branch-collapse probe (measure branches, ignore wall)**

Add `#[ignore]` probe tests mirroring the MRV/det-lookahead gate harness: `sat(SweetWine)` and `sat(AlsatianWine ⊓ ¬AmericanWine)` from `ontologies/.../wine.ofn` (resolve the corpus path as prior gates do; corpus is fetched on demand). Run each with `RUSTDL_SAT_LOOKAHEAD` unset and set, recording `branches`, `restores`, `lookahead_dropped`, `lookahead_forced_single`, and the verdict. Print a table.

```sh
# build once
cargo build -p owl-dl-reasoner --release --tests
# OFF baseline (MRV on by default): expect Alsatian ~1227 br, SweetWine ~12366 br
RUSTDL_ADAPTIVE_BUDGET=0 cargo test -p owl-dl-reasoner --release sat_lookahead_gate -- --ignored --nocapture
# ON
RUSTDL_ADAPTIVE_BUDGET=0 RUSTDL_SAT_LOOKAHEAD=1 cargo test -p owl-dl-reasoner --release sat_lookahead_gate -- --ignored --nocapture
```
Record the numbers. **Verdict thresholds:** GO = SweetWine → low-hundreds-or-fewer AND Alsatian → tens-or-fewer (order-of-magnitude below the MRV baseline) AND both verdicts stay `Sat`. FLOOR = SweetWine stays within ~2× of 12366. A collapse to spurious `Unsat` is **not** a GO — flag it.

- [ ] **Step 2: Wine FP=0 — RUN FIRST before declaring GO**

```sh
TEST_PAIR_MS=25 RUSTDL_SAT_LOOKAHEAD=1 cargo test -p owl-dl-reasoner --release \
  konclude_closure_diff_wine -- --ignored --nocapture
```
(Use the exact wine closure-diff test name/path the prior gate docs used; if it is a single parameterised test, pass the wine filter.) Require `rustdl_closure = konclude_closure = 653`, FP=0, MISSED=0. **If FP > 0 → verdict UNSOUND** (the branch-dependent-nominal-context hole); record the spurious-subsumption count and the repro command.

- [ ] **Step 3: Flag-OFF byte-identical spot-check**

```sh
cargo test -p owl-dl-tableau           # full suite, flag unset
TEST_PAIR_MS=25 cargo test -p owl-dl-reasoner --release konclude_closure_diff_wine -- --ignored
```
Expected: green; wine 653=653 with the flag unset (identical to the integration branch base).

- [ ] **Step 4: Write the verdict doc**

Create `docs/marker-saturator-lookahead-gate-results-2026-06-25.md` with: the branch table (OFF vs ON for both classes, with dropped/forced counters), the wine FP=0 result (run-first), the flag-OFF check, and the **VERDICT** (GO / FLOOR / UNSOUND) with the one-paragraph consequence:
  - GO → engineering follow-on: incremental seed-saturation (drop clone-per-call) + promote look-ahead to a real wedge feature; next spec.
  - FLOOR → commit SP-B (closure-only completeness audit), since Konclude-class is mandatory.
  - UNSOUND → record repro; commit SP-B (optionally one retry restricting the seed to provably-non-branch-dependent markers).

- [ ] **Step 5: Commit the verdict doc (only — spike code stays on the branch, unmerged)**

```sh
git add docs/marker-saturator-lookahead-gate-results-2026-06-25.md \
        crates/owl-dl-reasoner/tests/sat_lookahead_gate.rs
git commit -m "docs(sat-gate): marker-saturator ⊔ look-ahead gate verdict (<GO/FLOOR/UNSOUND>)

<trailers>"
```

- [ ] **Step 6: Report to the controller**

Surface the verdict + numbers. Do NOT merge to `main`. On GO, the next step is a follow-on spec (incremental seed-saturation); on FLOOR/UNSOUND, the next step is the SP-B closure-only spec.

---

## Notes for the implementer

- The seed-saturation **drop direction is sound under under-seeding** (restricting the seed to the atomic + ∃-marker subset can only make the saturator derive ⊥ *less* often → never a wrong drop on account of a missing constraint). The residual FP risk is the nominal-context one — caught by the Task-3 wine gate, not argued away.
- If Task 1 Step 3's reserved-id sizing fights `WorklistEngine::new` (some index sized from `internal` not the passed count), the fallback is to add the reserved `X` as a real declared class in a **cloned** `InternalOntology` at `build_base` time (one extra `Declaration`), which guarantees every index covers it; document the choice in the commit.
- Per-call clone of the engine is the known cost; the gate measures branches, so do not optimise it. If a wine probe does not terminate within ~20 min wall, that itself is data (record it) — but the expectation from the MRV baseline (12366 branches) is that ~tens of thousands of clone+small-run calls complete in minutes.
