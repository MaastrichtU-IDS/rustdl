# Pseudo-model Realize Shortcut Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** On the off-fragment (tableau) `realize` path, compute one ABox witness model and refute most `(individual, class)` non-membership pairs without a `{a} ⊓ ¬C` probe — completeness-preserving, sound, gated `RUSTDL_PSEUDO_MODEL`, default-on only if an ORE/custom-ontology assessment passes.

**Architecture:** Reuse `main`'s `ConsistencyCache` (holds `clauses` + `seed: AboxSeed` + `sub_roles` + `num_classes`/`num_individuals`, and `decide` already builds `HyperEngine::new_seeded(…).with_nominals(…)`). Add a per-individual label reader to the wedge (sibling of the existing root-label reader `satisfiability_labels`), a `base_model_types` builder on `ConsistencyCache`, and a short-circuit in `realize`'s `instance_check_with_closure`.

**Tech Stack:** Rust (edition 2024); `owl-dl-tableau` hypertableau (`hyper.rs`), `owl-dl-reasoner` (`lib.rs`, `realize.rs`).

## Global Constraints
- Build/test `RUSTUP_TOOLCHAIN=stable cargo …` (pinned 1.95.0 often lacks cargo). Clippy pedantic `-D warnings`; `unwrap`/`dbg` only under `#[cfg(test)]`; rustfmt max_width=100 (`cargo fmt --all`); raw string w/o inner `#`/`"` → `r"..."`.
- **Soundness (cardinal):** the prune is the *sound membership direction* — `class ∉ individual's-witness-model-types ⇒ not an entailed member` (a genuine counter-model). It is **completeness-preserving** ONLY if the label reader returns the individual's **complete completed** label set; under-reporting ⇒ a genuine member pruned = a MISS (never a false positive). `Unsat`/`Stalled` witness ⇒ no model ⇒ shortcut skipped (fall back to the normal probe). Flag-off path is byte-identical to today.
- **Isolated worktree** `/Users/micheldumontier/code/rustdl-wt/pseudo-model`, branch `feat/pseudo-model-realize`. A concurrent agent works elsewhere — never leave this worktree; never touch other branches.
- Applies ONLY to the off-fragment tableau realize path; the saturation fast path (`RUSTDL_REALIZE_SATURATION`) is untouched. No classify/consistency/API-shape change.

## File Structure
- `crates/owl-dl-tableau/src/hyper.rs` — `seeded_individual_labels` (new per-individual completion-label reader).
- `crates/owl-dl-reasoner/src/lib.rs` — `ConsistencyCache::base_model_types` + `PreparedOntology::realize_base_model_types`.
- `crates/owl-dl-reasoner/src/realize.rs` — `RUSTDL_PSEUDO_MODEL` flag, compute the witness once in `realize_internal`, short-circuit in `instance_check_with_closure`.
- Tests: `hyper.rs` `#[cfg(test)]`; `crates/owl-dl-reasoner/tests/pseudo_model_realize.rs` (new).
- `docs/2026-07-26-pseudo-model-assessment.md` (new) — the ORE/custom assessment results + default-on decision.

---

## Task 1: `HyperEngine::seeded_individual_labels` — per-individual completion labels

**Files:** `crates/owl-dl-tableau/src/hyper.rs` (+ `#[cfg(test)]`).

**Interfaces — Produces:**
```rust
/// After a `Sat` verdict on a `new_seeded` (ABox) engine, the COMPLETE atomic-class
/// label set of the nominal node for individual index `i` in the witness completion.
/// `None` if there was no satisfying completion (call only after `decide` returned Sat).
pub fn seeded_individual_labels(&self, individual_idx: u32) -> Option<Vec<ClassId>>;
```
- Consumes: the engine's retained completion graph (the same state `satisfiability_labels(seed)` reads for the root — mirror it, but resolve the node for individual `individual_idx` instead of the root). `new_seeded` creates one node per individual index (see `AboxSeed` doc, `hyper.rs:~393`); resolve that node (following any merges via the engine's union-find), and collect its **full** atomic-class label set (the completed labels, not just seeded/told).

- [ ] **Step 1 — read the template.** Read `satisfiability_labels` (`hyper.rs:2374`) + the `AboxSeed`/`new_seeded` node creation (`hyper.rs:~393`, and `ConsistencyCache::build`/`decide` in `lib.rs`). Confirm how a seeded individual maps to a node id and how a node's completed label set is read (and that it reflects the *final* completion, post-saturation/merges). If a merge folds individual `i` into another node, resolve to the survivor.

- [ ] **Step 2 — RED (completeness canary, the load-bearing test).** A seeded engine over an ABox where an individual gets a **derived** (not asserted) class in the completion — assert `seeded_individual_labels(i)` includes that derived class (guards §3's under-reporting landmine). E.g. seed `{a} ⊑ D`, `D ⊑ E` (so `a` derives `E`); after `decide` == `Sat`, labels of `a` must contain both `D` and `E`. Build it via `HyperEngine::new_seeded` in a hyper.rs unit test mirroring existing seeded-engine tests. Run; FAIL (method absent).

- [ ] **Step 3 — GREEN.** Implement `seeded_individual_labels` mirroring `satisfiability_labels`'s label-collection, keyed to the individual's (merge-resolved) node. Return `Some(labels)` when the completion exists (post-`Sat`), `None` otherwise. Filter to real atomic `ClassId`s (exclude synthetic/`fresh_q`/nominal-marker ids if `satisfiability_labels` does — match its filtering exactly so "labels" means named classes).

- [ ] **Step 4 — run + a negative canary.** Completeness canary passes; add a canary that an individual NOT in a class in the (only) model does NOT have it in its labels. `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-tableau seeded_individual_labels`; then `-p owl-dl-tableau` full; clippy `-p owl-dl-tableau --all-targets -- -D warnings`; `cargo fmt --all`.

- [ ] **Step 5 — commit:** `feat(hyper): seeded_individual_labels — per-individual completion labels for the pseudo-model realize shortcut`.

## Task 2: `base_model_types` — one witness model → per-individual type sets

**Files:** `crates/owl-dl-reasoner/src/lib.rs`. Test: `lib.rs` `#[cfg(test)]` or Task 3's integration file.

**Interfaces — Produces:**
```rust
impl ConsistencyCache {
    /// One ABox witness model → each individual's COMPLETE type set, or `None`
    /// when no clash-free completion is available (Unsat/Stalled/deadline).
    pub(crate) fn base_model_types(&self, deadline: Option<std::time::Instant>)
        -> Option<Vec<std::collections::HashSet<ClassId>>>;
}
impl PreparedOntology {
    pub(crate) fn realize_base_model_types(&self, deadline: Option<std::time::Instant>)
        -> Option<Vec<std::collections::HashSet<ClassId>>>;
}
```

- [ ] **Step 1 — RED.** In `lib.rs` `#[cfg(test)]`: build a `PreparedOntology` from an ABox where individual `a` is a member of `D` (asserted) and derives `E` (`D ⊑ E`), and is provably NOT a member of some other declared class `F`. Assert `realize_base_model_types(None)` returns `Some(v)` with `v[a_idx]` ⊇ `{D, E}` and NOT containing `F`. Run; FAIL (method absent).

- [ ] **Step 2 — GREEN.** Implement `ConsistencyCache::base_model_types` as a sibling of `decide` (`lib.rs:~3276`): build the same `HyperEngine::new_seeded(&self.clauses, &self.seed).with_sub_roles(...).with_nominals(self.num_classes, self.num_individuals)` (+ the same `with_double_blocking`/`with_precise_card_deps`/`with_adaptive_budget` gates `decide` applies), `decide_with_deadline(HYPER_WEDGE_DEPTH, deadline)`; on `Sat`, `(0..num_individuals).map(|i| engine.seeded_individual_labels(i).unwrap_or_default().into_iter().collect())`; `Unsat`/`Stalled` ⇒ `None`. Add `PreparedOntology::realize_base_model_types` delegating to `self.consistency.as_ref().and_then(|c| c.base_model_types(deadline))` (mirrors the existing `decide`/consistency accessors).

- [ ] **Step 3 — run.** `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner base_model_types`; `--lib`; clippy `-p owl-dl-reasoner --all-targets -- -D warnings`; `cargo fmt --all`.

- [ ] **Step 4 — commit:** `feat(reasoner): base_model_types — one ABox witness model's per-individual type sets`.

## Task 3: wire the shortcut into `realize` (flag + short-circuit)

**Files:** `crates/owl-dl-reasoner/src/realize.rs`. Test: `crates/owl-dl-reasoner/tests/pseudo_model_realize.rs` (new).

**Interfaces:**
- `RUSTDL_PSEUDO_MODEL` flag (default OFF for now — flipped to ON in Task 4 only if the assessment passes). `fn pseudo_model_enabled() -> bool` (default per Task 4; start `false`).
- `instance_check_with_closure` gains a `base_types: Option<&HashSet<ClassId>>` param; short-circuit `class ∉ base_types ⇒ Ok(false)` after the told-closure fast path, before the `{a} ⊓ ¬C` probe.

- [ ] **Step 1 — RED (verdict-identity + prune-fires canaries).** In `tests/pseudo_model_realize.rs` (`#![allow(clippy::unwrap_used)]`): an off-fragment nominal ABox with a known realization (individual `a` : `A`, `A ⊑ C`, and a `B` that `a` is provably NOT). With `RUSTDL_PSEUDO_MODEL=1`: `realize(&onto)` gives `a`'s types == the OFF result (verdict-identical) AND includes `C`, excludes `B`. (RED: the flag/param don't exist yet.) Run; FAIL.

- [ ] **Step 2 — GREEN.** Add `pseudo_model_enabled()` (start default OFF: `std::env::var_os("RUSTDL_PSEUDO_MODEL").is_some_and(|v| v != "0" && !v.is_empty())`). In `realize_internal`'s off-fragment path, once: `let base_model = if pseudo_model_enabled() { prepared.realize_base_model_types(deadline_for_witness) } else { None };` (witness deadline: reuse the realize budget or `None`). Thread `base_model.as_ref().map(|m| &m[idx])` into each individual's loop and into `instance_check_with_closure`; add:
  ```rust
  if let Some(bt) = base_types && !bt.contains(&class_id) { return Ok(false); }
  ```
  after the told-closure loop, before building `pool.and([nom, ¬cls])`. (Mirror #23's realize.rs structure, adapted to main's current signatures.)

- [ ] **Step 3 — verdict-identity test.** Add an ON-vs-OFF byte-identity assertion for `realize` on the fixture (run `realize` twice, toggling the flag via `std::env::set_var` in-test, compare `entailed_types`/`most_specific_types`) — the in-repo slice of the §6 gate. Plus a `None`-witness fallback test (an inconsistent/Stalled ABox ⇒ `base_model` None ⇒ normal path; here an inconsistent ABox already `Err(Inconsistent)`s — use a benign ABox and assert results unchanged flag on/off).

- [ ] **Step 4 — run.** `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test pseudo_model_realize`; `--lib`; clippy; fmt.

- [ ] **Step 5 — commit:** `feat(reasoner): pseudo-model realize shortcut behind RUSTDL_PSEUDO_MODEL (default off)`.

## Task 4: ORE/custom assessment → default-on decision

**Files:** `crates/owl-dl-reasoner/src/realize.rs` (flip default if pass), `docs/2026-07-26-pseudo-model-assessment.md` (new).

- [ ] **Step 1 — verdict-identity, prune ON vs OFF (completeness gate).** For each ABox-bearing fixture reachable in-env (curated `sulo` + any `ontologies/real/*` with individuals; ORE ABox ontologies if the corpus is fetchable — `scripts/fetch-real-ontologies.sh`, may need Linux), run `realize` (or `rustdl realize --json`) with `RUSTDL_PSEUDO_MODEL=1` vs `=0` and diff the per-individual types. **Any divergence blocks default-on** (a completeness regression = a bug in Task 1's label completeness). Author a small **custom nominal-ABox fixture** (MIE-style: nominals + defined class + property domain + assertions) as a guaranteed off-fragment case.
- [ ] **Step 2 — oracle soundness on the custom nominal ABox.** Run a HermiT (ROBOT `reason --axiom-generators "ClassAssertion" --include-indirect true`) oracle on the custom fixture; confirm rustdl's realized types with the prune == the oracle (FP=0, no new MISS vs OFF). Reuse the `docker/robot` oracle pattern from the #48/#43 work.
- [ ] **Step 3 — wall.** Measure realize wall ON vs OFF on the custom nominal ABox (and any ORE ABox available); record the speedup.
- [ ] **Step 4 — decide + document.** Write `docs/2026-07-26-pseudo-model-assessment.md` with the three results. **If all pass:** flip `pseudo_model_enabled()` to default-ON (`is_none_or(|v| v != "0" && !v.is_empty())`) and update its doc comment + CLAUDE.md soundness-contract note. **If the corpus/ORE tier is unreachable in-sandbox:** keep default-OFF, ship as opt-in, and document the ORE assessment as the pre-default-on gate (mirroring the #40 corpus bake-off handling) — surface this to the human.
- [ ] **Step 5 — full validation + commit.** `RUSTUP_TOOLCHAIN=stable cargo test --workspace --exclude owl-dl-py`; workspace clippy `--all-features -D warnings`; fmt. Commit: `feat(reasoner): pseudo-model realize assessment + default decision` (+ `docs:` for the assessment).

---

## Self-Review

**1. Spec coverage:** §2 mechanism → Tasks 1–3. §3 soundness (complete-label read; None-fallback) → Task 1 completeness canary + Task 3 fallback test + the Global soundness constraint. §4 impl (hyper primitive / base_model_types / realize wiring) → Tasks 1/2/3 with the exact `main` anchors (`ConsistencyCache::decide` lib.rs:~3276, `satisfiability_labels` hyper.rs:2374, `instance_check_with_closure` realize.rs:194). §5 flag → Task 3. §6 assessment + default-on gate → Task 4 (verdict-identity + oracle + wall). §7 testing → Tasks 1–3 canaries + Task 4 gate. §8 non-goals (no instance_check_wedge, no reuse caches, no API change) respected. §9 open items (does main expose the reader? → resolved: no per-*individual* reader; `satisfiability_labels` is root-only, so Task 1 adds the sibling; label-completeness → Task 1 Step 2 canary). ✓

**2. Placeholder scan:** No TBD. Task 1's "mirror `satisfiability_labels` / confirm node mapping + merge-resolution" names exactly what to read and adapt (the primitive's internals depend on the wedge's node/label representation, which the implementer confirms against the real code in Step 1 — not a blank). Task 4's "if corpus unreachable, ship opt-in" is an explicit branch, not a gap.

**3. Type consistency:** `seeded_individual_labels(u32) -> Option<Vec<ClassId>>` (Task 1) consumed by `base_model_types` (Task 2) which returns `Option<Vec<HashSet<ClassId>>>`, exposed as `realize_base_model_types` (Task 2) and consumed by `instance_check_with_closure`'s `base_types: Option<&HashSet<ClassId>>` (Task 3). `RUSTDL_PSEUDO_MODEL` / `pseudo_model_enabled()` consistent (Task 3 default OFF → Task 4 flips iff pass). `ClassId` used throughout (the class index compared is `class_id`, matching the witness set's element type).
