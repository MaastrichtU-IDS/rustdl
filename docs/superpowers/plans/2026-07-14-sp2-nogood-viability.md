# SP2 — Node-local UNSAT No-Good Viability + Sound Prune — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Rev 2 (advisor-reworked).** Phase B was rebuilt after advisor review: the soundness oracle is **wedge-native** (the tableau's `verify_node_local_clash` is NOT reusable — wrong crate/IR/label-type), the core is **antecedent-seeded** (not greedy-over-the-closed-label-set, which yields the useless derived pair), the merge-taint guard uses live-path flags only, prune deps are **recomputed precisely at the prune site**, and Phase A is framed honestly as a **smoke test** (it can flag an empty/unreachable tail but can neither confirm nor soundly kill the subset-core mechanism).

**Goal:** Decide whether a sound node-local UNSAT no-good layer in the hypertableau wedge helps `classify ore_ont_10019` decide within budget — Phase A (near-free smoke test reusing the shipped shadow-dep probe), then Phase B (build the sound prune behind a flag and measure it directly) which is the actual go/no-go.

**Architecture:** Phase A reuses `SearchStats.clash_records` (already recorded on the live classify path under `RUSTDL_SHADOW_DEP_PROBE`) + `shadow_measures::analyze`, adding depth-binning. Phase B, behind `RUSTDL_WEDGE_NOGOOD`, adds to `hyper.rs`: (B0) a wedge-native node-local UNSAT oracle over `self.clauses`; (B1) antecedent-seeded, cost-bounded core extraction at the `fire_head` clash; (B2) a per-solve subsumption no-good store; (B3) record-at-clash + prune-at-branch with precisely recomputed deps; (B4) corpus gate + direct `ore_ont_10019` measurement.

**Tech Stack:** Rust (workspace crates `owl-dl-tableau`, `owl-dl-reasoner`, `owl-dl-cli` bin `rustdl`, `owl-dl-bench`). horned-owl. rayon. Tests via `cargo test`.

## Global Constraints

- Toolchain: **always** prefix cargo with `RUSTUP_TOOLCHAIN=stable`. Rebuild BOTH `-p owl-dl-cli -p owl-dl-bench` before any CLI/matrix run; confirm the binary is freshly built (the stale-binary trap has caused phantom results repeatedly).
- Branch: `feat/sp2-nogood` (already created off `main`; SP2 spec + this plan are on it). Do NOT work on `main`.
- **Soundness invariant: FP = 0 must never regress.** The FP gate MUST include the **non-Horn adversarial oracle** (`ore_ont_13723` vs Konclude), not just curated.
- **Completeness invariant for Phase B: MISSED = 0 / byte-identical curated closures.** The prune is sound, so a bug manifests as a MISS (over-prune), not an FP; the closure/differential gate is load-bearing and is what catches a mis-extracted core.
- Phase B behavior lands behind default-OFF `RUSTDL_WEDGE_NOGOOD`; the flag-OFF path must be byte-identical. A default flip is OUT OF SCOPE here (a separate reviewed commit if ever warranted).
- `clippy -D warnings` and `cargo fmt --all -- --check` clean on every commit. Test modules may carry `#![allow(clippy::unwrap_used)]` per repo precedent.
- Phase B's cross-node no-good generalization is **scoped to no-inverse / no-nominal ontologies** (true for `ore_ont_10019`). On that fragment a genuinely node-local UNSAT core (re-derivation-verified by the B0 oracle) is TBox-global and sound regardless of `≤n`-merge provenance — the oracle gate is the load-bearing soundness guarantee; the `at_most_tainted`/`nn_tainted` exclusion is a conservative extra that can only reduce prunes, never cause an FP. Do NOT trust the flag on inverse/nominal inputs (enforced by the FP oracle gate).

## Data / prerequisites (verified present)

- `~/data/ore-run/input/ore_ont_10019.ofn` (target; dense SROIQ, no inverse/nominal, has `=n`).
- `~/data/ore-run/input/ore_ont_13723.ofn` + `~/data/ore-run/oracle/ore_ont_13723-classified.owx` (non-Horn FP oracle).
- Curated corpus under `ontologies/real/`; SROIQ fixtures `crates/owl-dl-bench/fixtures/{27_eight_way_disjunction_sat,18_diamond_subsumption_unsat}.ofn` + `ontologies/regression/funcmerge-cyclic.ofn`.

## Ground truth in the code (verified 2026-07-14 — do NOT re-derive; DO re-confirm line numbers, they drift)

- `HyperEngine<'c>` (`hyper.rs:523`) holds `clauses: &'c [DlClause]`, `disjoint_pairs: Arc<HashSet<(u32,u32)>>` (`:532`), `nodes: Vec<HyperNode>`, `indexes: Arc<ClauseIndexes>`, `stats`, `init_depth`, `current_branch_level` (`:700`), a worklist. **It has NO `ConceptPool`/`AbsorbedTBox`/`RoleHierarchy` field** — the wedge reasons over clausal `DlClause`, so the tableau's `saturate::verify_node_local_clash` (which needs pool/tbox and `ConceptId`, a *different* newtype than the wedge's node-label `ClassId`, whose space also includes preimage-less Tseitin names) is **NOT usable here**.
- `HyperNode` (`hyper.rs:174`): `labels: Vec<ClassId>` (`:176`), `label_deps: Vec<DepSet>` (`:180`, per-label decision-level dep-set), `birth_deps: DepSet` (`:221`), live-path taint flags `at_most_tainted`/`nn_tainted` (set at `:3039`/`:3065`). `shadow_merge_cause: DepSet` (`:261`) is written **only under `shadow_dep_probe`** — it is EMPTY under `RUSTDL_WEDGE_NOGOOD`, so it must NOT be used in Phase B.
- `fire_head` (`hyper.rs:3243`): the node-local clash site. When `clause.head.is_empty()` (a `body→⊥` clause matched), it sets `self.clash_deps = body_deps` (or `DepSet::ALL` if `nn_tainted`), resolves the clashing node `xn = self.resolve(xnode)`, and (only under `shadow_dep_probe`) calls `record_clash`, then returns `FireOutcome::Clash`. `body_deps` is the matched body's decision-level dep-set (the antecedent handle).
- `build_disjoint_pairs` (`hyper.rs:890`) extracts told-disjoint `(a,b)` pairs from ⊥-headed two-`Class`-atom clauses into `disjoint_pairs`. A core that is exactly such a pair is caught eagerly by clause firing on any node carrying both → caching it prunes nothing.
- `decide_with_deadline` (`hyper.rs:1777`) resets `self.stats` and sets `self.init_depth = max_depth` at entry; it is per-decide (per class-pair) — the valid per-solve reset point for the no-good store. `current_branch_level` (`:700`, updated at branch points; read by `record_clash` at `:1259`) is the live decision level.
- Shadow probe already wired into the LIVE classify path: `reasoner/src/lib.rs:2608` (`decide_with_stats`), `:2671` (`classify_labels`). `ClashRecord { branch_depth, real, shadow, clash_label_key }` derives `Clone` (`hyper.rs:433`). `shadow_measures::analyze(&[ClashRecord]) -> ShadowReport`.
- Probe entry points for the Phase A harness: `owl_dl_reasoner::sat_class_probe` (`lib.rs:1439`) / `decide_pair_probe` (`lib.rs:1402`); template harness `crates/owl-dl-reasoner/tests/shadow_dep_gate.rs`.

---

# PHASE A — Stage 0 smoke test (depth-binned recurrence on ore_ont_10019)

> **What Phase A can and cannot conclude (read before building):** the two shipped metrics do NOT measure the node-local *subset-core* recurrence Phase B exploits. `reusable_nogood_frac` keys on the shadow **decision-level dep-set** (`shadow.levels`) — a *backjumping-structure* recurrence, not a label-core recurrence. `revisit_frac` keys on the **full** `clash_label_key`, which undercounts (successors share small cores, not full sets). A node-local core is a *subset* of the label-set, decoupled from decision context, so **both metrics can read ≈0 while true core-reuse is high.** Therefore Phase A is a **necessary-not-sufficient smoke test**: it can flag an *empty or unreachable deep tail* (a real kill), but it can neither confirm viability nor soundly kill the subset-core mechanism. The real go/no-go is Phase B's direct `nogood_prunes_netnew` (B4). Phase A is worth running only because it is nearly free and rules out the degenerate "no deep clashes at all" case.

### Task A1: Depth-binned report in `shadow_measures`

**Files:**
- Modify: `crates/owl-dl-tableau/src/shadow_measures.rs`
- Test: same file (`#[cfg(test)]`).

**Interfaces:**
- Consumes: `ClashRecord { branch_depth, shadow, clash_label_key }` (Clone), existing `analyze`/`ShadowReport`.
- Produces: `pub fn analyze_by_depth(records: &[ClashRecord], split_depth: u32) -> DepthBinnedReport` with `shallow`/`deep: ShadowReport`, `n_shallow`/`n_deep: usize`, `split_depth: u32`. Reader interprets `deep.*` as the *deep-tail* figures.

- [ ] **Step 1: Write the failing test.**
```rust
#[test]
fn analyze_by_depth_splits_shallow_and_deep() {
    use crate::hyper::{ClashRecord, DepSetSnapshot};
    let snap = |levels: Vec<u32>| DepSetSnapshot { highest: levels.last().copied(), count: levels.len() as u32, levels };
    let recs = vec![
        ClashRecord { branch_depth: 1,  real: snap(vec![1]), shadow: snap(vec![1]), clash_label_key: 100 },
        ClashRecord { branch_depth: 2,  real: snap(vec![1]), shadow: snap(vec![1]), clash_label_key: 100 },
        ClashRecord { branch_depth: 50, real: snap(vec![9]), shadow: snap(vec![9]), clash_label_key: 200 },
        ClashRecord { branch_depth: 60, real: snap(vec![9]), shadow: snap(vec![9]), clash_label_key: 200 },
    ];
    let r = analyze_by_depth(&recs, 10);
    assert_eq!((r.n_shallow, r.n_deep), (2, 2));
    assert!((r.deep.reusable_nogood_frac - 1.0).abs() < 1e-9);
    assert!((r.shallow.reusable_nogood_frac - 1.0).abs() < 1e-9);
}
```
- [ ] **Step 2: Run to verify it fails.** `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-tableau --lib analyze_by_depth_splits 2>&1 | tail -5` → FAIL (not found).
- [ ] **Step 3: Implement.**
```rust
pub struct DepthBinnedReport { pub split_depth: u32, pub shallow: ShadowReport, pub deep: ShadowReport, pub n_shallow: usize, pub n_deep: usize }
#[must_use]
pub fn analyze_by_depth(records: &[ClashRecord], split_depth: u32) -> DepthBinnedReport {
    let shallow: Vec<ClashRecord> = records.iter().filter(|r| r.branch_depth < split_depth).cloned().collect();
    let deep: Vec<ClashRecord> = records.iter().filter(|r| r.branch_depth >= split_depth).cloned().collect();
    DepthBinnedReport { split_depth, n_shallow: shallow.len(), n_deep: deep.len(), shallow: analyze(&shallow), deep: analyze(&deep) }
}
```
- [ ] **Step 4: Run to verify it passes.** Same command → PASS.
- [ ] **Step 5: fmt + clippy + commit.**
```bash
RUSTUP_TOOLCHAIN=stable cargo fmt --all -- --check && RUSTUP_TOOLCHAIN=stable cargo clippy -p owl-dl-tableau --all-targets --all-features -- -D warnings
git add crates/owl-dl-tableau/src/shadow_measures.rs
git commit -m "feat(measures): depth-binned clash-recurrence report (SP2 Stage 0)"
```

### Task A2: ore_ont_10019 smoke-test harness + findings

**Files:**
- Create: `crates/owl-dl-reasoner/tests/sp2_nogood_gate.rs` (model on `shadow_dep_gate.rs`).
- Create: `docs/2026-07-14-sp2-nogood-findings.md`.

- [ ] **Step 1: Identify the stalled classes.** `RUSTUP_TOOLCHAIN=stable cargo build --release -p owl-dl-cli && ./target/release/rustdl hyper-sat ~/data/ore-run/input/ore_ont_10019.ofn --per-class-timeout-ms 300 2>&1 | grep -i stalled | head -20` — record the depth-74–80 stalled class IRIs (per the SP0 findings: HydroxylGroup, EtherGroup, SulfoxideGroup, OxygenAtom, …) and the observed max depth (to pick the `split_depth`).
- [ ] **Step 2: Write the harness (`#[ignore]`d gate).** Create `sp2_nogood_gate.rs` modeled on `shadow_dep_gate.rs`: load `ore_ont_10019.ofn`, for each stalled class call `sat_class_probe(&ont, iri, 256, Some(Duration::from_secs(30)))`, and print for its `stats.clash_records`: `analyze` (aggregate) and `analyze_by_depth(&records, D)` with `D` ≈ the midpoint between shallow branching and the observed depth cap. Report `n_deep/n_shallow`, `deep.reusable_nogood_frac`, `deep.revisit_frac`, `deep.revisit_context_shared_frac`, and the `bjgap_shadow` histogram. **Header comment must state the "necessary-not-sufficient smoke test" framing above** so no reader misreads the numbers as a viability verdict.
- [ ] **Step 3: Run both asymptotic and in-budget.**
```bash
RUSTUP_TOOLCHAIN=stable cargo build --release -p owl-dl-cli
RUSTDL_SHADOW_DEP_PROBE=1 RUSTDL_ADAPTIVE_BUDGET=0 RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --release --test sp2_nogood_gate -- --ignored --nocapture 2>&1 | tee /tmp/sp2-asymptotic.txt | tail -60
RUSTDL_SHADOW_DEP_PROBE=1 RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --release --test sp2_nogood_gate -- --ignored --nocapture 2>&1 | tee /tmp/sp2-inbudget.txt | tail -60
```
- [ ] **Step 4: Write findings + apply the (weak) gate.** Create `docs/2026-07-14-sp2-nogood-findings.md` recording the per-class + aggregate numbers (both runs), then:
  - **EMPTY-TAIL KILL (the only sound kill Phase A can make):** if the stalled classes have essentially **no deep clashes** (`n_deep` ≈ 0) — there is nothing for a deep-tail prune to act on. Stop; record the pivot (2a stronger blocking, or bound `search.rs`).
  - **PROCEED to Phase B** otherwise (per "commit through Stage 1"). Note explicitly in the findings that a low `deep.reusable_nogood_frac`/`deep.revisit_frac` is NOT a kill (those metrics don't lower-bound subset-core reuse), and that `bjgap_shadow` (large ⇒ backjumping already jumps far) plus low `revisit_context_shared_frac` are *warnings* about the reuse-trap that Phase B's per-solve scope + node-local oracle neutralize — not kills.
- [ ] **Step 5: Commit.**
```bash
git add crates/owl-dl-reasoner/tests/sp2_nogood_gate.rs docs/2026-07-14-sp2-nogood-findings.md
git commit -m "test(reasoner): SP2 Stage 0 depth-binned clash smoke test on ore_ont_10019 + findings"
```

---

# PHASE B — Stage 1 sound node-local core-keyed prune (only if Phase A did not empty-tail-kill)

> All Phase B code is behind default-OFF `RUSTDL_WEDGE_NOGOOD`; the flag-OFF path must stay byte-identical.

### Task B0: Wedge-native node-local UNSAT oracle

**Files:**
- Modify: `crates/owl-dl-tableau/src/hyper.rs` (a `HyperEngine` method).
- Test: `crates/owl-dl-tableau/tests/wedge_nogood.rs` (create).

**Interfaces:**
- Produces: `fn node_local_unsat(&self, labels: &[ClassId]) -> bool` — true iff the `ClassId` label-set is node-locally unsatisfiable **in the wedge's own clause system** (`self.clauses` + `self.disjoint_pairs`), using ONLY node-local clauses (body atoms all `Class` on the single variable; no `Role`/`∃`/`≥`/`≤`/edge atoms). Pure/read-only; no `&mut self`, no graph mutation.

Algorithm (no throwaway engine needed — operate on a local label-set):
1. Start `set = labels.to_vec()` as a `HashSet<ClassId>`.
2. Fixpoint: for each clause `cl` in `self.clauses` whose body atoms are all `Class(_, X)` (single variable, no role atoms) and whose body is fully contained in `set`:
   - if `cl.head.is_empty()` → return `true` (a `body→⊥` node-local clash);
   - else if the single head atom is `Class(c, X)` and `c ∉ set` → insert `c`, mark changed.
   Also check `self.disjoint_pairs`: if any `(a,b)` with both `a,b ∈ set` → return `true`.
3. Repeat until no change; then return `false`.
   Bound the loop by `self.clauses.len() + set.len()` iterations defensively.

- [ ] **Step 1: Write the failing test.** In `wedge_nogood.rs`, construct a `HyperEngine` over hand-built `DlClause`s encoding `A⊑B` (`Class(A,X) → Class(B,X)`) and `B⊓C⊑⊥` (`Class(B,X) ⊓ Class(C,X) → ⊥`). Assert: `node_local_unsat(&[A, C])` is `true` (via the multi-step A→B then B⊓C→⊥); `node_local_unsat(&[A])` is `false`; `node_local_unsat(&[D, E])` is `false`. (Copy the `DlClause`/`Atom` construction pattern from existing `owl-dl-tableau` unit tests — grep `DlClause {` in `hyper.rs`/`clause.rs` tests.)
- [ ] **Step 2: Run to verify it fails.** `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-tableau --test wedge_nogood node_local 2>&1 | tail -5` → FAIL.
- [ ] **Step 3: Implement `node_local_unsat`** per the algorithm above. Confirm the `Atom` variants + how to test "body atoms all `Class` on one variable" against the real `Atom`/`DlClause` types (grep `enum Atom`, `struct DlClause`).
- [ ] **Step 4: Run to verify it passes.** Same command → PASS (crucially `{A,C}` → true through the multi-step derivation, proving the oracle sees non-syntactic cores).
- [ ] **Step 5: fmt + clippy + commit.**
```bash
git add crates/owl-dl-tableau/src/hyper.rs crates/owl-dl-tableau/tests/wedge_nogood.rs
git commit -m "feat(hyper): wedge-native node-local UNSAT oracle over self.clauses (SP2 B0)"
```

### Task B1: Antecedent-seeded, cost-bounded core extraction (behind the flag)

**Files:**
- Modify: `crates/owl-dl-tableau/src/hyper.rs` — flag field + builder + extraction method.
- Test: `crates/owl-dl-tableau/tests/wedge_nogood.rs`.

**Interfaces:**
- Produces:
  - `HyperEngine::with_wedge_nogood(self) -> Self` + field `wedge_nogood: bool` (default `false` in all 3 constructors, `:937`/`:988`/`:2100`).
  - `fn extract_node_local_core(&self, clash_node: HNode, body_deps: DepSet) -> Option<Vec<ClassId>>` — returns `Some(minimal sorted antecedent core)` or `None` if not soundly generalizable.

Extraction (fixes the greedy-yields-derived-pair bug — seed from ANTECEDENT labels, not the closed set):
1. `let xn = self.resolve(clash_node);` If `self.nodes[xn].at_most_tainted || self.nodes[xn].nn_tainted` → return `None` (conservative; the B0 oracle is the real soundness backstop).
2. **Seed = the node's *decision-implicated antecedent* labels**: the labels `labels[i]` whose `label_deps[i]` shares a decision level with `body_deps` (i.e. `!label_deps[i].intersect(body_deps).is_empty()`), UNION any label with EMPTY `label_deps` that is present (seed/root-given). This is a small set (a handful), not the full closed label-set — both cheaper and it keeps the *antecedent* labels (`{A,C}`) rather than the derived pair (`{B,C}`). (Confirm `DepSet` exposes an intersection/`is_empty`; if not, compare `levels`.)
3. If `!self.node_local_unsat(&seed)` → the seed alone doesn't clash node-locally (the clash needed edge/successor/merge evidence, or derived-only labels): return `None`. This is the derivation-local soundness check.
4. Minimize: greedily drop each label from `seed` while `node_local_unsat` still holds. Result is a minimal antecedent core.
5. **Filter syntactic told-disjoint pairs:** if the core is exactly a pair `(a,b)` with `(min,max) ∈ self.disjoint_pairs`, return `None` (caught eagerly by clause firing; caching prunes nothing).
6. Return `Some(sorted core)`.

- [ ] **Step 1: Write the failing test.** Reuse the B0 clause fixture plus a decision context: build a clash where the node's labels are `{A, B, C, noise…}`, `A` and `C` carry decision-level deps overlapping `body_deps`, `B` is derived (its `label_deps` traces only through `A`). Assert `extract_node_local_core(...)` returns `{A, C}` (antecedent, minimal, `B`/noise dropped), and returns `None` for a told-disjoint-pair-only clash and for a merge-tainted node. (Constructing `label_deps` state requires driving the engine to a real clash; if that is heavy, split: unit-test the seed+minimize+filter logic on a small helper that takes `(labels, label_deps, body_deps)` explicitly, and integration-test the wiring in B3.)
- [ ] **Step 2: Run to verify it fails.** `... --test wedge_nogood extract_node_local_core 2>&1 | tail -5` → FAIL.
- [ ] **Step 3: Implement the flag + `extract_node_local_core`** per the algorithm. Add `wedge_nogood: bool` to the struct + all 3 constructors + `with_wedge_nogood`.
- [ ] **Step 4: Run to verify it passes.** Same command → PASS (`{A,C}`; `None` for disjoint-pair/tainted).
- [ ] **Step 5: fmt + clippy + commit.**
```bash
git add crates/owl-dl-tableau/src/hyper.rs crates/owl-dl-tableau/tests/wedge_nogood.rs
git commit -m "feat(hyper): antecedent-seeded node-local core extraction behind RUSTDL_WEDGE_NOGOOD (SP2 B1)"
```

### Task B2: Per-solve subsumption no-good store

**Files:** Modify `hyper.rs`; test `wedge_nogood.rs`.

**Interfaces:** a `NoGoodStore { cores: Vec<Vec<ClassId>> }` (each core sorted) with `record(core)` (dedup; keep only minimal cores — skip if an existing core subsets the new one, drop existing cores the new one subsets), `subsumes(labels: &BTreeSet<ClassId>) -> bool` (some stored core ⊆ labels), `clear()`. Field on `HyperEngine`; `clear()` at the top of `decide_with_deadline` (per-solve reset — verified reset point).

- [ ] **Step 1: Failing test** — `record(vec![a,c]); assert!(subsumes(&set(&[a,c,d]))); assert!(!subsumes(&set(&[a,d])));` and minimality (recording `{a,c}` then `{a,c,d}` keeps only `{a,c}`).
- [ ] **Step 2: Run → FAIL.**
- [ ] **Step 3: Implement `NoGoodStore`** (sorted-vec cores; `is_subset_sorted` helper for record dedup; `subsumes` via `BTreeSet::contains`). Add field; `clear()` in `decide_with_deadline`.
- [ ] **Step 4: Run → PASS.**
- [ ] **Step 5: fmt + clippy + commit.** `git commit -m "feat(hyper): per-solve subsumption nogood store (SP2 B2)"`

### Task B3: Record-at-clash + prune-at-branch (precise deps) + verdict-identity gate

**Files:** Modify `hyper.rs`; create `crates/owl-dl-cli/tests/wedge_nogood_identity.rs`.

**Interfaces:** adds `nogood_prunes: u64` + `nogood_prunes_netnew: u64` to `SearchStats`.

- [ ] **Step 1: Record cores at the clash (flag-gated).** In `fire_head` (`hyper.rs:3243`) at the `clause.head.is_empty()` branch, after `xn`/`clash_deps` are set, when `self.wedge_nogood`: `if let Some(core) = self.extract_node_local_core(xn, body_deps) { self.nogood_store.record(core); }`. OFF path untouched.
- [ ] **Step 2: Prune at the branch-decision site (flag-gated), with precise recomputed deps.** In the disjunction branch driver (`solve` / `find_open_disjunction`), **after** asserting a disjunct's Class atom onto the node but before recursing, when `self.wedge_nogood`: build the node's label `BTreeSet`; if `nogood_store.subsumes(&set)`, prune this branch as UNSAT. **Recompute the prune's dep-set precisely at this site**: `let core_deps = self.nodes[n].birth_deps ∪ (⋃ label_deps[i] for labels[i] in the matched core);` set `self.clash_deps = core_deps` (NOT `DepSet::ALL`, NOT any "stored context" deps — those are unsound/undefined). Increment `nogood_prunes`. Compute net-new: `let d = self.current_branch_level; if core_deps.contains_level(d) { nogood_prunes_netnew += 1; }` — net-new means the current decision is genuinely responsible, so backjumping would NOT have skipped this branch (a `d ∉ core_deps` prune is backjump-redundant). (Confirm a `DepSet::contains_level`/bit-test exists; else test via `levels`.)
- [ ] **Step 3: Differential identity test (the gate).** In `crates/owl-dl-cli/tests/wedge_nogood_identity.rs`, mirror `incremental_fixpoint_identity.rs`: classify `funcmerge-cyclic`, `pizza`, `27_eight_way_disjunction_sat`, `18_diamond_subsumption_unsat` with `RUSTDL_WEDGE_NOGOOD` `0` vs `1`; assert sorted verdict lines byte-identical (these fully resolve, so a sound prune cannot change verdicts — a diff = over-prune=MISS or bug).
- [ ] **Step 4: Run the gate; iterate to byte-identical.**
```bash
RUSTUP_TOOLCHAIN=stable cargo build -p owl-dl-cli
RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-cli --test wedge_nogood_identity 2>&1 | tail -8
RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-tableau 2>&1 | tail -5   # OFF-path guard
```
Expected: identity PASS; tableau suite unchanged. **If identity FAILS**, the prune dropped a real subsumption (unsound/over-broad core, or a `node_local_unsat` bug) — debug B0/B1 until byte-identical. Do not proceed until PASS.
- [ ] **Step 5: fmt + clippy + commit.** `git commit -m "feat(hyper): record+prune node-local nogoods with precise deps; verdict-identity gate (SP2 B3)"`

### Task B4: Corpus FP/MISSED gate, measure ore_ont_10019, decide

**Files:** append `docs/2026-07-14-sp2-nogood-findings.md`.

- [ ] **Step 1: Curated FP=0 AND MISSED=0, flag ON.**
```bash
RUSTUP_TOOLCHAIN=stable cargo build --release -p owl-dl-cli -p owl-dl-bench
RUSTDL_WEDGE_NOGOOD=1 ./target/release/owl-dl-bench matrix --tier curated --out /tmp/m-ng --pair-timeout-ms 1000 --global-timeout-s 120
grep -o '"reasoner":"rustdl"[^}]*' /tmp/m-ng/results.jsonl | grep -oE '"fp":[0-9]+|"missed":[0-9]+' | sort | uniq -c
```
Every rustdl `fp`/`missed` = 0. **If any MISSED>0, STOP** — over-prune; return to B3 Step 4.
- [ ] **Step 2: Non-Horn FP oracle (ore_ont_13723), flag ON.** Run `konclude_closure_diff::ore_one_closure_matches_oracle` (env `ORE_ONE_INPUT=~/data/ore-run/input/ore_ont_13723.ofn ORE_ONE_ORACLE=~/data/ore-run/oracle/ore_ont_13723-classified.owx RUSTDL_WEDGE_NOGOOD=1 --ignored --nocapture`). Assert FP=0.
- [ ] **Step 3: Measure ore_ont_10019 classify (the verdict).**
```bash
for f in 0 1; do echo "=== nogood=$f ==="; for ab in 0 1; do echo "-- adaptive_budget=$ab --"; gtimeout -s KILL 120 env RUSTDL_WEDGE_NOGOOD=$f RUSTDL_ADAPTIVE_BUDGET=$ab RUSTDL_AGGREGATE_DEADLINE_MS=60000 ./target/release/rustdl classify ~/data/ore-run/input/ore_ont_10019.ofn --pair-timeout-ms 250 2>&1 | grep -iE 'incomplete|# classes|direct|real'; done; done
# nogood-prune counters (add a one-line stderr dump under RUSTDL_WEDGE_NOGOOD, or read SearchStats via a probe):
```
Record: incomplete-pair delta, any newly-decided class (hierarchy diff), wall, and `nogood_prunes` / `nogood_prunes_netnew`. The authoritative net-new cross-check is the flag ON-vs-OFF `branches_taken`/`restores` delta.
- [ ] **Step 4: Apply the decision criterion + write findings.**
  - **2b VIABLE** iff the flag flips ≥1 currently-stalled class to *decided* within budget (or materially lowers the stalled-count with a credible path), **driven by net-new deep-tail prunes** (`nogood_prunes_netnew` non-trivial). Recommend a default-ON flip as a separate reviewed commit (out of scope here).
  - **2b DEAD** iff no class flips, `nogood_prunes_netnew` is negligible, cores never match (`nogood_prunes ≈ 0`), or wall is unmoved (the CDBL 0%-wall outcome). Record the pivot (2a stronger blocking, or bound `search.rs`); leave the flag default-OFF (controller decides whether to revert B0–B3 or keep dormant). Explicitly note the ceiling: a node-local prune front-runs at most **one** child `horn_fixpoint`, which SP1 already made ~56× cheaper — so even many prunes may not move wall.
- [ ] **Step 5: Commit.** `git commit -m "docs(reasoner): SP2 node-local nogood direct measurement + VIABLE/DEAD verdict"`

---

## Self-review notes

- **Spec coverage:** Stage 0 → Phase A (A1 depth-binning, A2 smoke test + empty-tail kill). Stage 1 → Phase B (B0 wedge-native oracle, B1 antecedent-seeded core, B2 store, B3 record/prune + identity gate, B4 corpus gate + measure + verdict). Soundness scope (node-local, oracle-verified, per-solve, no-inverse/nominal, taint exclusion) → B0/B1/Global Constraints. FP=0 + non-Horn oracle + MISSED=0 → B3/B4.
- **Advisor rework encoded:** #1 wedge-native oracle = B0 (no `verify_node_local_clash`). #2 antecedent-seeded core = B1 Step-3 seed rule (fixes greedy-yields-`{B,C}`). #3 taint via live-path flags only, oracle is the backstop = B1 Step-1 + Global Constraints (no `shadow_merge_cause`). #4 precise recomputed prune deps = B3 Step-2. #5 Phase A relabeled smoke test = Phase A preamble + A2 Step-4. #6 corrected net-new = B3 Step-2. #7 real clash site `fire_head` = B3 Step-1. #8 disjoint-pair filter + ceiling = B1 Step-5 + B4 Step-4.
- **Risk owned:** B3 Step 4 is the iterate-to-byte-identical loop (over-prune = MISS, caught by the identity + curated-MISSED gates). Phase A is honestly a smoke test, not a viability gate.
- **Open confirmations the implementer resolves in-task:** exact `Atom`/`DlClause` shape for the node-local body test (B0 Step 3); `DepSet` intersection / `contains_level` API (B1 Step 2, B3 Step 2); exact `solve`/`find_open_disjunction` branch-assert site for the prune hook (B3 Step 2); `sat_class_probe` signature + `ore_ont_10019` namespace (A2 Step 2).
- **Default flip is out of scope** — B4 records VIABLE/DEAD; any flip is a separate reviewed commit.
