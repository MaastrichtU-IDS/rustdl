# Model-derived realization types — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Read entailed realization types off the one witness model the #57 pseudo-model already builds — using the hyper engine's per-label `DepSet` (empty ⟺ deterministic ⟺ entailed) — so deterministic types skip the per-pair `{a} ⊓ ¬C` probe.

**Architecture:** The hyper `HyperEngine` builds one `Sat` ABox witness model; each `HyperNode` carries `label_deps` parallel to `labels`. A new accessor returns the empty-dep labels of merge-untouched individual nodes. The reasoner builds the model once and exposes both the complete label set (existing #57 prune) and the deterministic subset (new read-off). `instance_check_with_closure` becomes a four-way decision: told-true → deterministic-read-off-true → witness-prune-false → probe. Gated `RUSTDL_MODEL_DERIVED_TYPES`, default OFF.

**Tech Stack:** Rust (edition 2024), workspace crates `owl-dl-tableau` + `owl-dl-reasoner`; rayon per-individual loop; `horned-owl` OFN parsing in tests.

## Global Constraints

- Build/test with `export PATH="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"; export RUSTUP_TOOLCHAIN=stable` (per CLAUDE.md toolchain gotcha).
- FP=0 is the crown-jewel invariant: a read-off may only ever return an entailed positive. Never trade it for speed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` must stay clean (pedantic on; `unwrap_used` warn).
- New behaviour is gated `RUSTDL_MODEL_DERIVED_TYPES`, **default OFF**; flag-off path must be byte-identical to today.
- Commit only when the plan says; commit trailers end with:
  `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>` and
  `Claude-Session: https://claude.ai/code/session_01BPU4DH5DXn2jmpuXdfijF7`.
- Branch: `feat/model-derived-realize` (already created off `origin/main`).

---

### Task 1: Hyper engine — merge-touch flag + deterministic-labels accessor

**Files:**
- Modify: `crates/owl-dl-tableau/src/hyper.rs` (`HyperNode` struct; `DepSet` impl; `merge_with_cause` ~3759; new accessor near `seeded_individual_labels` ~2419)
- Test: `crates/owl-dl-tableau/src/hyper.rs` (`#[cfg(test)]` module — add unit tests inline, the crate's convention)

**Interfaces:**
- Produces: `HyperEngine::seeded_individual_deterministic_labels(&self, individual_idx: u32) -> Option<Vec<ClassId>>` — the empty-`label_deps` labels of individual `individual_idx`, or `None` if out of range OR the individual is merge-touched (merged away, or its representative absorbed a merge).
- Produces: `DepSet::is_empty(self) -> bool`.
- Consumes: existing `HyperNode.labels`, `HyperNode.label_deps`, `self.resolve`, `self.nodes`.

- [ ] **Step 1: Add `DepSet::is_empty` (write test first)**

In the `#[cfg(test)]` module of `hyper.rs`, add:

```rust
#[test]
fn depset_is_empty_only_for_empty_not_all() {
    assert!(DepSet::EMPTY.is_empty());
    assert!(!DepSet::ALL.is_empty()); // overflow ⇒ not empty (excludes ALL)
    assert!(!DepSet::singleton(3).is_empty());
}
```

- [ ] **Step 2: Run it — expect FAIL (no `is_empty`)**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-tableau depset_is_empty_only_for_empty_not_all`
Expected: FAIL — `no method named is_empty`.

- [ ] **Step 3: Implement `DepSet::is_empty`**

In `impl DepSet` (after `highest_level`):

```rust
/// `true` iff no decision level is present. `ALL` (overflow) is NOT empty —
/// it means "depends on everything", so it reports `false`. This is the
/// read-off soundness test: empty ⟺ derived with no branch decision.
pub(crate) fn is_empty(self) -> bool {
    self.highest_level().is_none()
}
```

- [ ] **Step 4: Run it — expect PASS**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-tableau depset_is_empty_only_for_empty_not_all`
Expected: PASS.

- [ ] **Step 5: Add `absorbed_merge` field to `HyperNode`**

In `struct HyperNode` (~line 174) add the field after `label_deps`:

```rust
    /// Set on the surviving representative whenever any node is merged into
    /// it via `merge_with_cause`. Read-off (`seeded_individual_deterministic_labels`)
    /// excludes merge-touched individuals: the `≤n`/functional merge caller
    /// passes `cause_deps = EMPTY`, so a branch-triggered merge can leave a
    /// moved label with an `EMPTY` dep — reading it as entailed would be an FP.
    absorbed_merge: bool,
```

Initialise it `false` at EVERY `HyperNode { … }` construction site (search `HyperNode {` in `hyper.rs`; add `absorbed_merge: false,`). Also mirror in any shadow/clone if the struct derives it — check for `#[derive(Clone)]` and manual node copies.

- [ ] **Step 6: Set the flag in `merge_with_cause` (write guard test first)**

Add test:

```rust
#[test]
fn merge_marks_survivor_absorbed() {
    // Minimal engine with two mergeable individual nodes; force a merge and
    // assert the surviving representative is flagged merge-touched.
    // (Construct via the same helper other merge tests in this file use.)
    let mut e = tiny_two_node_engine();          // existing/adapted test helper
    let (a, b) = (HNode(0), HNode(1));
    assert!(e.merge_with_cause(b, a, DepSet::EMPTY));
    let rep = e.resolve(a);
    assert!(e.nodes[rep.index()].absorbed_merge, "survivor must be flagged");
}
```

If no `tiny_two_node_engine` helper exists, build the smallest engine an existing merge test in `hyper.rs` uses and adapt it; keep the assertion (survivor flagged) as the behavior under test.

- [ ] **Step 7: Run it — expect FAIL**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-tableau merge_marks_survivor_absorbed`
Expected: FAIL — field always `false`.

- [ ] **Step 8: Set `absorbed_merge` on the survivor**

In `merge_with_cause`, after the survivor representative is determined (where `bi` = survivor node data is taken), set the flag on the surviving representative node:

```rust
        // Read-off guard: mark the survivor merge-touched so
        // `seeded_individual_deterministic_labels` excludes it (the `≤n`
        // EMPTY-cause path can leave a moved label EMPTY-dep — an FP if read).
        // `survivor` here is the representative the merge collapses onto.
        self.nodes[survivor.index()].absorbed_merge = true;
```

(Use whatever local names the function has for the surviving node index; both `merge` and `merge_with_cause` route through here, so both are covered.)

- [ ] **Step 9: Run it — expect PASS**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-tableau merge_marks_survivor_absorbed`
Expected: PASS.

- [ ] **Step 10: Add the accessor (write test first)**

```rust
#[test]
fn deterministic_labels_only_empty_dep_and_skip_merged() {
    // Node 0: labels [C(empty dep), D(dep on level 2)] → read-off = [C].
    let mut e = single_node_engine_with_labels(&[
        (class_c(), DepSet::EMPTY),
        (class_d(), DepSet::singleton(2)),
    ]);
    assert_eq!(e.seeded_individual_deterministic_labels(0), Some(vec![class_c()]));
    // After a merge touches node 0's rep, read-off returns None (probe instead).
    e.nodes[0].absorbed_merge = true;
    assert_eq!(e.seeded_individual_deterministic_labels(0), None);
}
```

Adapt `single_node_engine_with_labels` from the closest existing `hyper.rs` test constructor.

- [ ] **Step 11: Run it — expect FAIL (no accessor)**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-tableau deterministic_labels_only_empty_dep_and_skip_merged`
Expected: FAIL — `no method named seeded_individual_deterministic_labels`.

- [ ] **Step 12: Implement the accessor**

Next to `seeded_individual_labels` (~2419):

```rust
/// The empty-`label_deps` (deterministic ⟹ entailed-in-all-models) labels of
/// individual `individual_idx`, for the model-derived realize read-off
/// (`RUSTDL_MODEL_DERIVED_TYPES`). Returns `None` when the index is out of
/// range OR the individual is **merge-touched** — its representative differs
/// from `individual_idx` (merged away) or absorbed a merge — because the
/// `≤n`/functional merge path (`merge_with_cause`, EMPTY cause) can leave a
/// moved label EMPTY-dep, which is NOT safe to read as entailed. Callers treat
/// `None` as "no read-off for this individual" (fall through to probing).
#[must_use]
pub fn seeded_individual_deterministic_labels(&self, individual_idx: u32) -> Option<Vec<ClassId>> {
    let idx = individual_idx as usize;
    if idx >= self.nodes.len() {
        return None;
    }
    let rep = self.resolve(HNode(individual_idx));
    if rep != HNode(individual_idx) || self.nodes[rep.index()].absorbed_merge {
        return None; // merge-touched ⇒ no read-off (sound: probe instead)
    }
    let node = &self.nodes[rep.index()];
    Some(
        node.labels
            .iter()
            .zip(node.label_deps.iter())
            .filter(|(_, d)| d.is_empty())
            .map(|(c, _)| *c)
            .collect(),
    )
}
```

- [ ] **Step 13: Run it + full tableau suite — expect PASS**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-tableau`
Expected: new tests PASS, no regressions.

- [ ] **Step 14: Commit**

```bash
git add crates/owl-dl-tableau/src/hyper.rs
git commit -m "feat(hyper): merge-touch flag + deterministic-label accessor for realize read-off

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01BPU4DH5DXn2jmpuXdfijF7"
```

---

### Task 2: Reasoner — build the witness model once, expose both views

**Files:**
- Modify: `crates/owl-dl-reasoner/src/lib.rs` (`ABoxConsistency::base_model_types` ~3335; `PreparedOntology::realize_base_model_types` ~4679)
- Test: `crates/owl-dl-reasoner/src/lib.rs` (`#[cfg(test)]`) — a struct-shape unit test is optional; the real gate is Task 4/5.

**Interfaces:**
- Produces: `struct WitnessModel { complete: Vec<HashSet<ClassId>>, deterministic: Vec<HashSet<ClassId>> }` (both indexed by individual id; a merge-touched individual's `deterministic[i]` is the empty set).
- Produces: `ABoxConsistency::witness_model(&self, deadline) -> Option<WitnessModel>`.
- Produces: `PreparedOntology::realize_witness_model(&self, deadline) -> Option<WitnessModel>`.
- Consumes: `HyperEngine::seeded_individual_labels`, `seeded_individual_deterministic_labels` (Task 1).

- [ ] **Step 1: Define `WitnessModel` and the single-build method**

Replace `base_model_types`'s body with a shared builder. Add near it:

```rust
/// One `ABox` witness model, both views from a SINGLE engine build:
/// `complete` = every individual's full atomic-class label set (the #57
/// prune source); `deterministic` = the empty-dep subset per individual
/// (the model-derived read-off source; empty set for a merge-touched
/// individual). `None` when no clash-free completion is available.
pub(crate) struct WitnessModel {
    pub(crate) complete: Vec<std::collections::HashSet<owl_dl_core::ir::ClassId>>,
    pub(crate) deterministic: Vec<std::collections::HashSet<owl_dl_core::ir::ClassId>>,
}

pub(crate) fn witness_model(
    &self,
    deadline: Option<std::time::Instant>,
) -> Option<WitnessModel> {
    use owl_dl_tableau::hyper::HyperResult;
    let mut engine = self.build_seeded_engine();
    match engine.decide_with_deadline(HYPER_WEDGE_DEPTH, deadline) {
        HyperResult::Sat => {
            let n = self.num_individuals;
            let complete = (0..n)
                .map(|i| engine.seeded_individual_labels(i).unwrap_or_default().into_iter().collect())
                .collect();
            let deterministic = (0..n)
                .map(|i| {
                    engine
                        .seeded_individual_deterministic_labels(i)
                        .unwrap_or_default()
                        .into_iter()
                        .collect()
                })
                .collect();
            Some(WitnessModel { complete, deterministic })
        }
        HyperResult::Unsat | HyperResult::Stalled => None,
    }
}
```

Make `base_model_types` delegate (keeps its existing consumer byte-identical):

```rust
pub(crate) fn base_model_types(
    &self,
    deadline: Option<std::time::Instant>,
) -> Option<Vec<std::collections::HashSet<owl_dl_core::ir::ClassId>>> {
    self.witness_model(deadline).map(|m| m.complete)
}
```

- [ ] **Step 2: Expose from `PreparedOntology`**

Next to `realize_base_model_types` (~4679):

```rust
pub(crate) fn realize_witness_model(
    &self,
    deadline: Option<std::time::Instant>,
) -> Option<crate::WitnessModel> {
    self.consistency.as_ref().and_then(|c| c.witness_model(deadline))
}
```

(Path-qualify `WitnessModel` per its module; if `ABoxConsistency` is in the same module, `WitnessModel` unqualified is fine.)

- [ ] **Step 3: Build — expect clean compile**

Run: `RUSTUP_TOOLCHAIN=stable cargo build -p owl-dl-reasoner`
Expected: compiles; `base_model_types` unchanged for its existing caller.

- [ ] **Step 4: Run reasoner tests — expect no regression**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner realize`
Expected: PASS (behaviour unchanged — nothing consumes `deterministic` yet).

- [ ] **Step 5: Commit**

```bash
git add crates/owl-dl-reasoner/src/lib.rs
git commit -m "feat(reasoner): WitnessModel — build one model, expose complete + deterministic views

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01BPU4DH5DXn2jmpuXdfijF7"
```

---

### Task 3: Realize loop — gate flag + four-way decision

**Files:**
- Modify: `crates/owl-dl-reasoner/src/realize.rs` (env gate near `pseudo_model_enabled` ~107; `instance_check_with_closure` ~280; `realize_tableau_internal` model build ~922 and per-individual loop ~940)

**Interfaces:**
- Produces: `fn model_derived_types_enabled() -> bool` (default OFF).
- Consumes: `PreparedOntology::realize_witness_model` (Task 2); `WitnessModel.{complete,deterministic}`.
- Changes: `instance_check_with_closure` gains `deterministic_types: Option<&HashSet<ClassId>>`.

- [ ] **Step 1: Add the gate (write test first)**

In the `#[cfg(test)]` module of `realize.rs`:

```rust
#[test]
fn model_derived_types_default_off() {
    // Unset ⇒ off. (Serialised via the crate's env test lock if present.)
    let _g = crate::test_env_lock();               // use existing lock helper if present
    std::env::remove_var("RUSTDL_MODEL_DERIVED_TYPES");
    assert!(!super::model_derived_types_enabled());
    std::env::set_var("RUSTDL_MODEL_DERIVED_TYPES", "1");
    assert!(super::model_derived_types_enabled());
    std::env::remove_var("RUSTDL_MODEL_DERIVED_TYPES");
}
```

If the crate has no `test_env_lock`, drop that line (the assertion is what matters); if flakiness appears, add serialisation later.

- [ ] **Step 2: Run — expect FAIL**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner model_derived_types_default_off`
Expected: FAIL — `no function model_derived_types_enabled`.

- [ ] **Step 3: Implement the gate**

Near `pseudo_model_enabled` (~107):

```rust
/// The model-derived deterministic type read-off (increment-1). **Default
/// OFF** — enabled only by explicit `RUSTDL_MODEL_DERIVED_TYPES=1` (or any
/// non-empty, non-"0" value) — until the differential+oracle gate passes.
fn model_derived_types_enabled() -> bool {
    std::env::var_os("RUSTDL_MODEL_DERIVED_TYPES").is_some_and(|v| v != "0" && !v.is_empty())
}
```

- [ ] **Step 4: Run — expect PASS**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner model_derived_types_default_off`
Expected: PASS.

- [ ] **Step 5: Thread the deterministic set into `instance_check_with_closure`**

Change the signature (add param after `base_types`):

```rust
    base_types: Option<&HashSet<ClassId>>,
    deterministic_types: Option<&HashSet<ClassId>>,
```

Insert the read-off arm immediately after the told-closure loop, BEFORE the `base_types` prune:

```rust
    for told in told_classes_of(internal, individual_id) {
        if closure.contains(told, class_id) {
            return Ok(true);
        }
    }
    // Model-derived read-off: a deterministic (empty-dep) label on a
    // merge-untouched individual node is entailed in every model ⇒ Ok(true)
    // with no probe. Verdict-preserving: probing would return the same true.
    if let Some(dt) = deterministic_types
        && dt.contains(&class_id)
    {
        return Ok(true);
    }
    if let Some(bt) = base_types
        && !bt.contains(&class_id)
    {
        return Ok(false);
    }
```

- [ ] **Step 6: Build the model once and pass both views in `realize_tableau_internal`**

Replace the `base_model` build (~922) so the model is built when EITHER shortcut is on, and derive both per-individual slices:

```rust
    let witness: Option<WitnessModel> = if pseudo_model_enabled() || model_derived_types_enabled() {
        let witness_deadline = pseudo_model_witness_deadline_from_env();
        prepared.realize_witness_model(Some(witness_deadline))
    } else {
        None
    };
    // `complete` feeds the #57 prune only when pseudo-model is on; `deterministic`
    // feeds the read-off only when model-derived is on. Either being off ⇒ that
    // view is treated as absent for every pair (unchanged behaviour).
    let complete_view = witness.as_ref().filter(|_| pseudo_model_enabled()).map(|m| &m.complete);
    let det_view = witness.as_ref().filter(|_| model_derived_types_enabled()).map(|m| &m.deterministic);
```

In the `par_iter` loop (~946) derive per-individual slices and pass them:

```rust
            let base_types = complete_view.and_then(|m| m.get(idx));
            let det_types = det_view.and_then(|m| m.get(idx));
            …
                if instance_check_with_closure(
                    internal, &closure, &prepared,
                    class_id, individual_id, pair_deadline,
                    base_types, det_types,
                )? {
```

Update the `WitnessModel` import at the top of `realize.rs` (`use crate::WitnessModel;`).

- [ ] **Step 7: Build + reasoner realize tests — expect PASS (flag off ⇒ unchanged)**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner realize`
Expected: PASS. With the flag off, `det_view` is `None` ⇒ behaviour byte-identical to today.

- [ ] **Step 8: Commit**

```bash
git add crates/owl-dl-reasoner/src/realize.rs
git commit -m "feat(realize): model-derived deterministic read-off (RUSTDL_MODEL_DERIVED_TYPES, default OFF)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01BPU4DH5DXn2jmpuXdfijF7"
```

---

### Task 4: Correctness fixtures — read-off, must-probe, merge-guard (TDD, negatives first)

**Files:**
- Create: `crates/owl-dl-reasoner/tests/model_derived_realize.rs`
- Create fixtures inline as OFN strings (follow the `parse(...)` + `convert_ontology` pattern in `realize.rs` tests ~1008).

**Interfaces:**
- Consumes: public `realize` API (or `realize_internal` via the crate's test surface) with `RUSTDL_MODEL_DERIVED_TYPES` toggled.

- [ ] **Step 1: Negative FP guard — disjunction-dependent type must NOT be read off**

Write a fixture where `a`'s membership in `C` holds only via a disjunction (so its label is branch-dependent), and assert that ON vs OFF `realize` give the SAME types for `a` (i.e. ON does not spuriously add a branch-only type). Also assert `a ∈ C` is still reported iff genuinely entailed. Use a fixture with `EquivalentClasses`/`ObjectUnionOf` so DepSets populate.

```rust
// pseudo-outline — fill with concrete IRIs following realize.rs test style
#[test]
fn disjunction_dependent_type_not_read_off() {
    let onto = parse(&fixture_disjunctive_membership());
    let on  = realize_with_flag(&onto, true);
    let off = realize_with_flag(&onto, false);
    assert_eq!(on, off, "read-off must be verdict-identical to probing");
}
```

- [ ] **Step 2: Merge-guard FP case — ≤n/functional merge moved label must NOT be read off**

Fixture: a functional (or `≤1`) role whose two successors get merged inside a disjunctive branch, moving a label onto a named individual. Assert ON == OFF (the moved EMPTY-dep label is NOT emitted as a spurious type because the individual is merge-touched → probed).

- [ ] **Step 3: Run Steps 1–2 — expect FAIL if the guard/read-off is wrong**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test model_derived_realize`
Expected: PASS given Tasks 1–3 (the guard makes ON==OFF). If either FAILS, the read-off is unsound — STOP and fix Task 1/3, do not weaken the test.

- [ ] **Step 4: Positive — deterministic type IS read off (and speeds up)**

Fixture: `ClassAssertion` + a `SubClassOf`/domain chain that deterministically types `a : C` via the model (no disjunction). Assert `a ∈ C` reported ON and OFF (verdict-identical) — this is the case the read-off accelerates. Optionally assert (behind a comment, not timing-asserted) that it needs no probe.

- [ ] **Step 5: Run all — expect PASS**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test model_derived_realize`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/owl-dl-reasoner/tests/model_derived_realize.rs
git commit -m "test(realize): read-off / must-probe / merge-guard fixtures (negatives first)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01BPU4DH5DXn2jmpuXdfijF7"
```

---

### Task 5: Ship gate — differential + oracle on disjunctive onts

**Files:**
- Create: `/mnt/um-share-drive/dumontier/rustdl-scratch/model_derived_gate.sh` (validation harness; not committed to the repo)

**Interfaces:** consumes the release `rustdl` binary + the `dnf58.paths` / disjunctive-ORE lists + the ROBOT/Konclude harness already in `rustdl-scratch`.

- [ ] **Step 1: Build release**

Run: `RUSTUP_TOOLCHAIN=stable cargo build --release -p owl-dl-cli`
Expected: `target/release/rustdl` fresh (confirm mtime).

- [ ] **Step 2: Differential ON vs OFF (byte-identity = soundness)**

For each ont in `{ore_ont_13723, a disjunctive-ORE sample, the 15 model-building onts}`, run `rustdl realize --json` with `RUSTDL_MODEL_DERIVED_TYPES=0` and `=1` (both isolated, `-P 1`, generous `RUSTDL_PSEUDO_MODEL_WITNESS_MS`), sort type rows, compare md5.

Expected: **byte-identical on every ont.** Any diff = an ON-only type = a candidate FP → STOP, reduce to the offending (ind,class), check its `label_deps`/merge status. Do not proceed to flip default.

- [ ] **Step 3: Oracle subset (FP=0)**

For the same onts, assert read-off (ON) types ⊆ HermiT (ROBOT `classassertion`) ∪ Konclude realization. Reuse `kon58.tsv`/`herm58.tsv` harness.
Expected: subset holds (FP=0).

- [ ] **Step 4: Curated-corpus regression**

Run `rustdl realize` ON vs OFF on the curated ABox fixtures (sio, wine, pizza, an alehif ABox variant) — byte-identical.
Expected: identical.

- [ ] **Step 5: Record results + (separately) propose the default flip**

Write the gate outcome (byte-identity table + oracle subset + which of the 15 the read-off actually converts DNF→complete) to `docs/benchmarks/2026-07-28-model-derived-realize/`. Only after the gate is green, propose flipping `RUSTDL_MODEL_DERIVED_TYPES` default-ON in a separate commit (user-approved), mirroring #57/backfold. Do NOT flip the default inside this plan.

---

## Self-Review

- **Spec coverage:** components (a)/(b)/(c) → Tasks 1/2/3; soundness merge-guard → Task 1 (flag) + Task 4 (guard test); differential+oracle gate → Task 5; witness-deadline parameter → Task 3 Step 6 (reuses `pseudo_model_witness_deadline_from_env`) and Task 5 (generous budget). Out-of-scope increment-2 not planned (correct).
- **Placeholders:** Task 4 fixtures are outlined, not literal OFN — flagged as "fill following realize.rs test style"; acceptable because the assertions (ON==OFF) are concrete and the fixture shape is specified. The implementer must write real OFN, not leave `todo!`.
- **Type consistency:** `WitnessModel{complete,deterministic}`, `witness_model`, `realize_witness_model`, `seeded_individual_deterministic_labels`, `model_derived_types_enabled`, `is_empty`, `absorbed_merge` used consistently across tasks. `instance_check_with_closure` new param `deterministic_types: Option<&HashSet<ClassId>>` matches its call site.
