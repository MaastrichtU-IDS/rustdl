# SP2 — Node-local UNSAT No-Good Viability + Sound Prune — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Decide cheaply whether a sound node-local UNSAT no-good layer in the hypertableau wedge helps `classify ore_ont_10019` decide within budget — Phase A (near-free kill-check reusing the shipped shadow-dep probe), then Phase B (build the sound prune behind a flag and measure it directly) only if Phase A doesn't hard-kill.

**Architecture:** Phase A reuses `SearchStats.clash_records` (already recorded on the live classify path under `RUSTDL_SHADOW_DEP_PROBE`) and the `shadow_measures::analyze` report, adding **depth-binning** — the one signal the advisor flagged as decisive and missing. Phase B builds a per-solve, node-local, core-keyed, subsumption-matched UNSAT prune in `hyper.rs` behind `RUSTDL_WEDGE_NOGOOD`, using the existing `verify_node_local_clash` oracle for soundness and merge-taint flags for the derivation-local exclusion.

**Tech Stack:** Rust (workspace crates `owl-dl-tableau`, `owl-dl-reasoner`, `owl-dl-cli` bin `rustdl`, `owl-dl-bench`). horned-owl. rayon. Tests via `cargo test`.

## Global Constraints

- Toolchain: **always** prefix cargo with `RUSTUP_TOOLCHAIN=stable` (pinned 1.95.0 lacks cargo). Rebuild BOTH `-p owl-dl-cli -p owl-dl-bench` before any CLI/matrix run; confirm the binary is freshly built (the stale-binary trap has caused phantom results repeatedly).
- Branch: `feat/sp2-nogood` (already created off `main`; the SP2 spec commit is on it). Do NOT work on `main`.
- **Soundness invariant: FP = 0 must never regress.** The FP gate MUST include the **non-Horn adversarial oracle** (`ore_ont_13723` vs Konclude), not just curated — curated is mostly EL/Horn where disjunction-FP cannot manifest.
- **Completeness invariant for Phase B: MISSED = 0 / byte-identical curated closures.** The prune is sound, so a bug manifests as a MISS (over-prune), not an FP; the closure/differential gate is load-bearing.
- Phase B behavior lands behind default-OFF `RUSTDL_WEDGE_NOGOOD`; flip default-ON (if ever) only in a separate reviewed commit after FP=0 AND MISSED=0 on curated + the non-Horn oracle + no curated wall regression.
- `clippy -D warnings` and `cargo fmt --all -- --check` clean on every commit. Test modules may carry `#![allow(clippy::unwrap_used)]` per repo precedent.
- Soundness of Phase B's cross-node no-good generalization is **scoped to no-inverse / no-nominal ontologies** (true for `ore_ont_10019`); the merge-taint exclusion covers the remaining `≤n` path. Do not trust the flag on inverse/nominal inputs (enforced by the FP oracle gate).

## Data / prerequisites (verified present)

- `~/data/ore-run/input/ore_ont_10019.ofn` (the target; dense SROIQ, no inverse/nominal, has `=n`).
- `~/data/ore-run/input/ore_ont_13723.ofn` + `~/data/ore-run/oracle/ore_ont_13723-classified.owx` (non-Horn FP oracle).
- Curated corpus under `ontologies/real/` (fetch via `scripts/fetch-real-ontologies.sh` if absent).

## What already exists (reuse — do NOT rebuild)

- `RUSTDL_SHADOW_DEP_PROBE` / `HyperEngine::with_shadow_dep_probe`, **already wired into the live classify path** at `crates/owl-dl-reasoner/src/lib.rs:2608` (`decide_with_stats`) and `:2671` (`classify_labels`); populates `SearchStats.clash_records`.
- `ClashRecord { branch_depth: u32, real: DepSetSnapshot, shadow: DepSetSnapshot, clash_label_key: u64 }` (`hyper.rs`, `record_clash` at ~1255).
- `shadow_measures::analyze(&[ClashRecord]) -> ShadowReport { n_clashes, bjgap_real, bjgap_shadow, reusable_nogood_frac, distinct_nogoods, revisit_frac, revisit_context_shared_frac }` and `Histogram { min, median, p90, max, mean }` (`crates/owl-dl-tableau/src/shadow_measures.rs`).
- `crates/owl-dl-reasoner/tests/shadow_dep_gate.rs` — a ready GO/NO-GO harness template (wine-specific): `sat_class_probe` / `decide_pair_probe` → `stats.clash_records` → `analyze` → `print_report`.
- `verify_node_local_clash(pool: &ConceptPool, tbox: &AbsorbedTBox, hierarchy: &RoleHierarchy, labels: &[ConceptId], max_iters: usize) -> bool` (`saturate.rs:213`, exported at `lib.rs:87`) — runs node-local rules on a throwaway isolated node; the reusable soundness oracle for "is this label-set node-locally unsatisfiable."
- Merge-taint state on `HyperNode`: `at_most_tainted`, `nn_tainted`, `shadow_merge_cause`.
- `DepSet` (`hyper.rs:86`); the clash site sets `self.clash_deps` just before returning `FireOutcome::Clash`; `current_branch_level` tracks decision depth.

---

# PHASE A — Stage 0 kill-check (depth-binned reuse on ore_ont_10019)

### Task A1: Depth-binned reuse measure in `shadow_measures`

**Files:**
- Modify: `crates/owl-dl-tableau/src/shadow_measures.rs`
- Test: same file (unit tests at bottom, `#[cfg(test)]`).

**Interfaces:**
- Consumes: `ClashRecord { branch_depth, shadow, clash_label_key }`, existing `analyze`.
- Produces: `pub fn analyze_by_depth(records: &[ClashRecord], split_depth: u32) -> DepthBinnedReport` where
  ```rust
  pub struct DepthBinnedReport {
      pub split_depth: u32,
      pub shallow: ShadowReport, // records with branch_depth < split_depth
      pub deep: ShadowReport,    // records with branch_depth >= split_depth
      pub n_shallow: usize,
      pub n_deep: usize,
  }
  ```
  The decisive figure the caller reads is `deep.reusable_nogood_frac` (dep-set-keyed no-good reuse in the deep tail) and `deep.revisit_frac`.

- [ ] **Step 1: Write the failing test.**

Add to the `#[cfg(test)]` module in `shadow_measures.rs`:
```rust
#[test]
fn analyze_by_depth_splits_shallow_and_deep() {
    use crate::hyper::{ClashRecord, DepSetSnapshot};
    let snap = |levels: Vec<u32>| DepSetSnapshot {
        highest: levels.last().copied(),
        count: levels.len() as u32,
        levels,
    };
    // Two shallow clashes (depth 1,2) sharing one nogood dep-set (reusable);
    // two deep clashes (depth 50,60) sharing a different nogood dep-set.
    let recs = vec![
        ClashRecord { branch_depth: 1,  real: snap(vec![1]),  shadow: snap(vec![1]),  clash_label_key: 100 },
        ClashRecord { branch_depth: 2,  real: snap(vec![1]),  shadow: snap(vec![1]),  clash_label_key: 100 },
        ClashRecord { branch_depth: 50, real: snap(vec![9]),  shadow: snap(vec![9]),  clash_label_key: 200 },
        ClashRecord { branch_depth: 60, real: snap(vec![9]),  shadow: snap(vec![9]),  clash_label_key: 200 },
    ];
    let r = analyze_by_depth(&recs, 10);
    assert_eq!(r.n_shallow, 2);
    assert_eq!(r.n_deep, 2);
    // Each partition has a nogood recurring across its 2 records => frac 1.0.
    assert!((r.deep.reusable_nogood_frac - 1.0).abs() < 1e-9, "deep reuse {}", r.deep.reusable_nogood_frac);
    assert!((r.shallow.reusable_nogood_frac - 1.0).abs() < 1e-9);
}
```

- [ ] **Step 2: Run it to verify it fails.**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-tableau --lib analyze_by_depth_splits 2>&1 | tail -5`
Expected: FAIL — `analyze_by_depth` / `DepthBinnedReport` not found.

- [ ] **Step 3: Implement `analyze_by_depth` + `DepthBinnedReport`.**

Add near `analyze`:
```rust
pub struct DepthBinnedReport {
    pub split_depth: u32,
    pub shallow: ShadowReport,
    pub deep: ShadowReport,
    pub n_shallow: usize,
    pub n_deep: usize,
}

#[must_use]
pub fn analyze_by_depth(records: &[ClashRecord], split_depth: u32) -> DepthBinnedReport {
    let shallow: Vec<ClashRecord> =
        records.iter().filter(|r| r.branch_depth < split_depth).cloned().collect();
    let deep: Vec<ClashRecord> =
        records.iter().filter(|r| r.branch_depth >= split_depth).cloned().collect();
    DepthBinnedReport {
        split_depth,
        n_shallow: shallow.len(),
        n_deep: deep.len(),
        shallow: analyze(&shallow),
        deep: analyze(&deep),
    }
}
```
(`ClashRecord` derives `Clone` — confirm; it does, per `SearchStats.clash_records` cloning in `save`. If not, add `#[derive(Clone)]`.)

- [ ] **Step 4: Run the test to verify it passes.**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-tableau --lib analyze_by_depth_splits 2>&1 | tail -5`
Expected: PASS.

- [ ] **Step 5: fmt + clippy + commit.**

```bash
RUSTUP_TOOLCHAIN=stable cargo fmt --all -- --check
RUSTUP_TOOLCHAIN=stable cargo clippy -p owl-dl-tableau --all-targets --all-features -- -D warnings
git add crates/owl-dl-tableau/src/shadow_measures.rs
git commit -m "feat(measures): depth-binned nogood-reuse report (SP2 Stage 0)"
```

### Task A2: ore_ont_10019 kill-check harness + findings + kill gate

**Files:**
- Create: `crates/owl-dl-reasoner/tests/sp2_nogood_gate.rs` (model on `shadow_dep_gate.rs`).
- Create/append: `docs/2026-07-14-sp2-nogood-findings.md`.

**Interfaces:**
- Consumes: `owl_dl_reasoner::sat_class_probe` (or `decide_pair_probe`), `shadow_measures::{analyze, analyze_by_depth}`, `RUSTDL_SHADOW_DEP_PROBE`.
- Produces: a printed depth-binned report per stalled class; a findings note with the go/no-go verdict.

- [ ] **Step 1: Identify the stalled classes to probe.**

Run (rebuild first): `RUSTUP_TOOLCHAIN=stable cargo build --release -p owl-dl-cli && ./target/release/rustdl hyper-sat ~/data/ore-run/input/ore_ont_10019.ofn --per-class-timeout-ms 300 2>&1 | grep -i stalled | head -20`
Record the stalled class IRIs (e.g. `HydroxylGroup`, `EtherGroup`, `SulfoxideGroup`, `OxygenAtom`, … — the depth-74–80 ones from the SP0 findings note).

- [ ] **Step 2: Write the harness test (`#[ignore]`d gate, run manually).**

Create `crates/owl-dl-reasoner/tests/sp2_nogood_gate.rs` modeled on `shadow_dep_gate.rs`:
```rust
//! SP2 Stage 0 kill-check: on ore_ont_10019's stalled classes, is node-local
//! UNSAT-nogood reuse (a) present at all and (b) concentrated in the DEEP tail
//! (where it could change the depth-cap outcome) vs shallow (backjumping already
//! handles it)? Read-only; requires RUSTDL_SHADOW_DEP_PROBE=1.
//! Run:
//!   RUSTDL_SHADOW_DEP_PROBE=1 RUSTDL_ADAPTIVE_BUDGET=0 cargo test -p owl-dl-reasoner \
//!     --release --test sp2_nogood_gate -- --ignored --nocapture
#![allow(clippy::unwrap_used, clippy::doc_markdown)]
use owl_dl_tableau::shadow_measures::{analyze, analyze_by_depth};
use std::time::Duration;
// ... load ore_ont_10019 (copy the load() pattern from shadow_dep_gate.rs, path
//     ~/data/ore-run/input/ore_ont_10019.ofn; ONT_NS = its ontology namespace),
//     iterate the stalled-class IRIs from Step 1, call sat_class_probe(&ont, iri,
//     256, Some(Duration::from_secs(30))), and for each print:
//       analyze(&stats.clash_records)              // aggregate
//       analyze_by_depth(&stats.clash_records, D)   // D = split (e.g. 40; ~half the
//                                                   // observed depth-cap tail)
//     reporting n_clashes, deep.reusable_nogood_frac, deep.revisit_frac,
//     deep.revisit_context_shared_frac, bjgap_shadow histogram, n_deep/n_shallow.
```
(Reuse `sat_class_probe`'s exact signature by reading `shadow_dep_gate.rs`. Pick the depth split `D` after Step 1 from the observed stalled depths — roughly the midpoint between shallow branching and the depth cap.)

- [ ] **Step 3: Run the harness, both in-budget and asymptotic.**

```bash
RUSTUP_TOOLCHAIN=stable cargo build --release -p owl-dl-cli
# asymptotic (no divergence cut, generous deadline):
RUSTDL_SHADOW_DEP_PROBE=1 RUSTDL_ADAPTIVE_BUDGET=0 RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --release --test sp2_nogood_gate -- --ignored --nocapture 2>&1 | tee /tmp/sp2-asymptotic.txt | tail -60
# in-budget (defaults, adaptive budget ON):
RUSTDL_SHADOW_DEP_PROBE=1 RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --release --test sp2_nogood_gate -- --ignored --nocapture 2>&1 | tee /tmp/sp2-inbudget.txt | tail -60
```

- [ ] **Step 4: Write findings + apply the kill gate.**

Create `docs/2026-07-14-sp2-nogood-findings.md` recording, per stalled class and aggregate: `deep.reusable_nogood_frac`, `deep.revisit_frac`, `deep.revisit_context_shared_frac`, the bjgap distribution, and the deep-vs-shallow split — for both runs. Then the **verdict**:
- **HARD-ZERO KILL** (stop; do NOT do Phase B): deep-tail reuse ≈ 0 (`deep.reusable_nogood_frac` ≈ 0 AND `deep.revisit_frac` ≈ 0), i.e. every deep clash is a distinct no-good — no-goods cannot prune the deep tail. Record the pivot recommendation (2a stronger blocking, or bound `search.rs`) and STOP the plan here.
- **PROCEED to Phase B**: any non-trivial deep-tail reuse (per user "commit through Stage 1", borderline ⇒ proceed). Note that a high `bjgap_shadow` with high `reusable_nogood_frac` is the encouraging case (reuse exists AND backjumping isn't already collapsing it); low `revisit_context_shared_frac` warns the reuse is context-divergent (the reuse-trap — Phase B's per-solve scope + node-local restriction handles it, but flag it).

- [ ] **Step 5: Commit.**

```bash
git add crates/owl-dl-reasoner/tests/sp2_nogood_gate.rs docs/2026-07-14-sp2-nogood-findings.md
git commit -m "test(reasoner): SP2 Stage 0 depth-binned nogood kill-check on ore_ont_10019 + findings"
```

---

# PHASE B — Stage 1 sound node-local core-keyed prune (only if Phase A did not hard-kill)

> Entered only when Task A2 Step 4 says PROCEED. All Phase B code is behind default-OFF `RUSTDL_WEDGE_NOGOOD`; the flag-OFF path must stay byte-identical.

### Task B1: Minimal node-local UNSAT core extraction (behind the flag, read-only)

**Files:**
- Modify: `crates/owl-dl-tableau/src/hyper.rs` — add the flag field + builder (mirror `with_shadow_dep_probe`), and a core-extraction method.
- Test: `crates/owl-dl-tableau/tests/wedge_nogood.rs` (create).

**Interfaces:**
- Consumes: the clashing node's `labels: &[ClassId]`, `verify_node_local_clash`, the node's merge-taint flags.
- Produces:
  - `HyperEngine::with_wedge_nogood(self) -> Self` + field `wedge_nogood: bool` (default false in all 3 constructors).
  - `fn extract_node_local_core(&self, clash_node: HNode) -> Option<Vec<ClassId>>` — returns `Some(minimal clashing label subset)` iff the clash is **node-local and derivation-local** (see soundness), else `None`. Minimal = greedily drop labels while `verify_node_local_clash` still holds.

- [ ] **Step 1: Write the failing test — extraction finds a minimal multi-step core and rejects merge-tainted / edge clashes.**

In `crates/owl-dl-tableau/tests/wedge_nogood.rs`, build a tiny TBox with a **non-syntactic** node-local clash: `A ⊑ B`, `B ⊓ C ⊑ ⊥` (so `{A,C}` clashes via one Horn step, and the minimal core is `{A,C}` — NOT the final `{B,C}` pair and NOT the full label-set). Assert:
```rust
// (pseudocode — use the crate's real TBox/engine construction, see existing
//  owl-dl-tableau/tests for the pattern)
let core = extract_via_engine(&tbox, &[A, C, /*noise*/ D, E]); // node labels {A,C,D,E}
assert_eq!(sorted(core.unwrap()), sorted(vec![A, C])); // minimal, noise dropped
// a satisfiable set yields None:
assert!(extract_via_engine(&tbox, &[D, E]).is_none());
```
(Confirm `verify_node_local_clash` is reachable from the test; it is exported at `owl_dl_tableau::verify_node_local_clash`.)

- [ ] **Step 2: Run to verify it fails.**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-tableau --test wedge_nogood 2>&1 | tail -5`
Expected: FAIL (no `extract_node_local_core` / helper).

- [ ] **Step 3: Implement the flag + extraction.**

Add `wedge_nogood: bool` (default `false` in `new`/`new_with_prebuilt`/`new_seeded`), `with_wedge_nogood`, and:
```rust
/// Minimal node-local UNSAT core of `clash_node`'s label-set, or None if the
/// clash is not soundly generalizable (edge/successor evidence, or a
/// merge-tainted node — see SP2 soundness scope). Read-only.
fn extract_node_local_core(&self, clash_node: HNode) -> Option<Vec<ClassId>> {
    let n = self.resolve(clash_node);
    // Derivation-local exclusion: any merge taint disqualifies (a label may be
    // merge-inherited, unsound to generalize across nodes).
    if self.nodes[n.0 as usize].at_most_tainted
        || self.nodes[n.0 as usize].nn_tainted
        || self.node_has_merge_cause(n) { return None; }
    let mut labels: Vec<ClassId> = self.nodes[n.0 as usize].labels.clone();
    // Must actually be node-locally unsatisfiable (excludes edge/∃/≤n clashes).
    if !verify_node_local_clash(self.pool, self.tbox, self.hierarchy, &labels, 4096) {
        return None;
    }
    // Greedy minimization: drop any label the clash survives without.
    let mut i = 0;
    while i < labels.len() {
        let removed = labels.remove(i);
        if verify_node_local_clash(self.pool, self.tbox, self.hierarchy, &labels, 4096) {
            // still clashes without it — keep it dropped
        } else {
            labels.insert(i, removed);
            i += 1;
        }
    }
    Some(labels)
}
```
(Wire `self.pool`/`self.tbox`/`self.hierarchy` access as the engine already holds them — confirm field names against `hyper.rs`. `node_has_merge_cause` reads `shadow_merge_cause` for the node; if that field is a global map, adapt the check.)

- [ ] **Step 4: Run to verify it passes.**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-tableau --test wedge_nogood 2>&1 | tail -5`
Expected: PASS (core `{A,C}`, noise dropped; `None` for satisfiable + tainted + edge clashes).

- [ ] **Step 5: fmt + clippy + commit.**

```bash
RUSTUP_TOOLCHAIN=stable cargo fmt --all -- --check
RUSTUP_TOOLCHAIN=stable cargo clippy -p owl-dl-tableau --all-targets --all-features -- -D warnings
git add crates/owl-dl-tableau/src/hyper.rs crates/owl-dl-tableau/tests/wedge_nogood.rs
git commit -m "feat(hyper): node-local UNSAT core extraction behind RUSTDL_WEDGE_NOGOOD (SP2 B1)"
```

### Task B2: Per-solve no-good store + subsumption match

**Files:**
- Modify: `crates/owl-dl-tableau/src/hyper.rs` — per-solve store field + record/lookup methods.
- Test: `crates/owl-dl-tableau/tests/wedge_nogood.rs`.

**Interfaces:**
- Produces:
  - Field `nogood_store: Vec<Vec<ClassId>>` (per-solve; each entry a sorted core), reset at the start of `decide_with_deadline`.
  - `fn nogood_record(&mut self, core: Vec<ClassId>)` — inserts a sorted core (dedup; skip if an existing core already subsumes it).
  - `fn nogood_subsumes(&self, node_labels: &BTreeSet<ClassId>) -> bool` — true iff some stored core ⊆ `node_labels`.

- [ ] **Step 1: Write the failing test — subsumption semantics.**

```rust
#[test]
fn nogood_store_subsumption() {
    let mut s = NoGoodStore::default();      // (or exercise via the engine)
    s.record(vec![a, c]);                    // stored core {a,c}
    assert!(s.subsumes(&set(&[a, c, d, e]))); // superset is pruned
    assert!(!s.subsumes(&set(&[a, d, e])));   // missing c -> not pruned
}
```
(If keeping the store as engine methods rather than a standalone struct, write the test against a minimal engine instance instead. Prefer a small standalone `NoGoodStore` struct in `hyper.rs` for testability + isolation.)

- [ ] **Step 2: Run to verify it fails.**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-tableau --test wedge_nogood nogood_store_subsumption 2>&1 | tail -5`
Expected: FAIL.

- [ ] **Step 3: Implement `NoGoodStore` (sorted-vec cores, subset match).**

```rust
#[derive(Default)]
pub(crate) struct NoGoodStore { cores: Vec<Vec<ClassId>> } // each sorted ascending
impl NoGoodStore {
    fn record(&mut self, mut core: Vec<ClassId>) {
        core.sort_unstable();
        core.dedup();
        // skip if an existing core already subsumes the new one (keep minimal set)
        if self.cores.iter().any(|c| is_subset_sorted(c, &core)) { return; }
        // drop existing cores the new (smaller) one subsumes
        self.cores.retain(|c| !is_subset_sorted(&core, c));
        self.cores.push(core);
    }
    fn subsumes(&self, labels: &std::collections::BTreeSet<ClassId>) -> bool {
        self.cores.iter().any(|c| c.iter().all(|x| labels.contains(x)))
    }
    fn clear(&mut self) { self.cores.clear(); }
}
// is_subset_sorted(a, b): every element of a is in sorted b.
```
Add a `NoGoodStore` field to `HyperEngine`; `clear()` it at the top of `decide_with_deadline` (per-solve reset).

- [ ] **Step 4: Run to verify it passes.**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-tableau --test wedge_nogood nogood_store_subsumption 2>&1 | tail -5`
Expected: PASS.

- [ ] **Step 5: fmt + clippy + commit.**

```bash
git add crates/owl-dl-tableau/src/hyper.rs crates/owl-dl-tableau/tests/wedge_nogood.rs
git commit -m "feat(hyper): per-solve node-local nogood store with subsumption match (SP2 B2)"
```

### Task B3: Wire record-at-clash + prune-at-branch; verdict-identity gate

**Files:**
- Modify: `crates/owl-dl-tableau/src/hyper.rs` — record a core at each node-local clash; check `nogood_subsumes` at the branch-decision site before descending a disjunct.
- Test: `crates/owl-dl-cli/tests/incremental_fixpoint_identity.rs`-style differential (reuse the existing harness pattern) — add a `RUSTDL_WEDGE_NOGOOD` OFF-vs-ON classify identity test on the 4 fully-resolving fixtures.

**Interfaces:**
- Consumes: `extract_node_local_core` (B1), `NoGoodStore` (B2).
- Produces: pruning behavior under the flag; a `nogood_prunes` counter + a `nogood_prunes_netnew` counter on `SearchStats` (net-new = the pruned branch's decision level was NOT already subsumed by the child clash's `clash_deps`, i.e. backjumping would not have skipped it).

- [ ] **Step 1: Record cores at clash (flag-gated).**

At the clash site where `self.clash_deps` is set / `record_clash` is called, add (only when `self.wedge_nogood`): `if let Some(core) = self.extract_node_local_core(clash_node) { self.nogood_store.record(core); }`. Keep the OFF path untouched.

- [ ] **Step 2: Prune at the branch-decision site (flag-gated).**

In the disjunction branch driver (`find_open_disjunction` / the `solve` descent), before asserting a disjunct onto a node, if `self.wedge_nogood` and the resulting node's label-set (as a `BTreeSet`) is `nogood_subsumes`, skip that branch as UNSAT (report the clash with an appropriate `DepSet` — reuse the stored context's deps or `DepSet::ALL` conservatively for correctness of backjumping). Increment `nogood_prunes`; increment `nogood_prunes_netnew` when the prune is not one backjumping would already make.

- [ ] **Step 3: Add the differential identity test (the gate).**

Add a test (in `crates/owl-dl-cli/tests/`) mirroring `incremental_fixpoint_identity.rs`: classify each of `funcmerge-cyclic`, `pizza`, `27_eight_way_disjunction_sat`, `18_diamond_subsumption_unsat` with `RUSTDL_WEDGE_NOGOOD` `0` vs `1`; assert sorted verdict lines are byte-identical (these fully resolve, so a sound prune cannot change verdicts — a diff means over-prune=MISS or a bug).

- [ ] **Step 4: Run the gate; iterate to byte-identical.**

```bash
RUSTUP_TOOLCHAIN=stable cargo build -p owl-dl-cli
RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-cli --test wedge_nogood_identity 2>&1 | tail -8
RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-tableau 2>&1 | tail -5   # OFF-path guard
```
Expected: identity test PASS; tableau suite unchanged. **If the identity test FAILS**, the prune dropped a real subsumption (over-prune / unsound core) — debug the core extraction or the subsumption check until byte-identical. Do not proceed until PASS.

- [ ] **Step 5: fmt + clippy + commit.**

```bash
git add crates/owl-dl-tableau/src/hyper.rs crates/owl-dl-cli/tests/wedge_nogood_identity.rs
git commit -m "feat(hyper): record-at-clash + prune-at-branch node-local nogoods; verdict-identity gate (SP2 B3)"
```

### Task B4: Corpus FP/MISSED gate, measure ore_ont_10019, decide

**Files:**
- Append: `docs/2026-07-14-sp2-nogood-findings.md`.
- Modify (only if the direct measurement warrants and gates pass): a default flip is OUT OF SCOPE here — record the decision; a flip is a separate follow-up.

- [ ] **Step 1: Curated FP=0 AND MISSED=0 with the flag ON.**

```bash
RUSTUP_TOOLCHAIN=stable cargo build --release -p owl-dl-cli -p owl-dl-bench
RUSTDL_WEDGE_NOGOOD=1 ./target/release/owl-dl-bench matrix --tier curated --out /tmp/m-ng --pair-timeout-ms 1000 --global-timeout-s 120
grep -o '"reasoner":"rustdl"[^}]*' /tmp/m-ng/results.jsonl | grep -oE '"fp":[0-9]+|"missed":[0-9]+' | sort | uniq -c
```
Expected: every rustdl `fp` and `missed` = 0. **If any MISSED>0, STOP** — over-prune; return to B3.

- [ ] **Step 2: Non-Horn FP oracle (ore_ont_13723), flag ON.**

Run the `konclude_closure_diff::ore_one_closure_matches_oracle` test (env `ORE_ONE_INPUT=~/data/ore-run/input/ore_ont_13723.ofn ORE_ONE_ORACLE=~/data/ore-run/oracle/ore_ont_13723-classified.owx`, `RUSTDL_WEDGE_NOGOOD=1`, `--ignored --nocapture`). Assert FP=0 (closures byte-identical to the oracle).

- [ ] **Step 3: Measure ore_ont_10019 classify (the verdict).**

```bash
for f in 0 1; do echo "=== nogood=$f ==="; for ab in 0 1; do echo "-- adaptive_budget=$ab --"; gtimeout -s KILL 120 env RUSTDL_WEDGE_NOGOOD=$f RUSTDL_ADAPTIVE_BUDGET=$ab RUSTDL_AGGREGATE_DEADLINE_MS=60000 ./target/release/rustdl classify ~/data/ore-run/input/ore_ont_10019.ofn --pair-timeout-ms 250 2>&1 | grep -iE 'incomplete|# classes|direct|real|nogood'; done; done
```
Record: stalled/incomplete-pair delta, any newly-decided class (hierarchy diff), wall, and `nogood_prunes` / `nogood_prunes_netnew`.

- [ ] **Step 4: Apply the decision criterion + write findings.**

Append to `docs/2026-07-14-sp2-nogood-findings.md`:
- **2b VIABLE** iff the flag flips ≥1 currently-stalled class to *decided* within budget (or materially lowers the stalled-count with a credible path), **driven by net-new deep-tail prunes** (`nogood_prunes_netnew` non-trivial). If so: recommend the default-ON flip as a separate reviewed commit (out of scope for this plan) and record the gate evidence.
- **2b DEAD** iff no class flips, net-new prunes are negligible, or wall is unmoved (the CDBL 0%-wall outcome repeats). Record the pivot: (2a) stronger blocking or bound `search.rs`; leave the flag default-OFF (or revert B1–B3 if it adds cost with no benefit — controller's call).

- [ ] **Step 5: Commit.**

```bash
git add docs/2026-07-14-sp2-nogood-findings.md
git commit -m "docs(reasoner): SP2 node-local nogood direct measurement + VIABLE/DEAD verdict"
```

---

## Self-review notes

- **Spec coverage:** Stage 0 (spec §Stage 0) → Phase A (A1 depth-binning, A2 harness+kill gate). Stage 1 (spec §Stage 1) → Phase B (B1 core extraction, B2 store+subsumption, B3 record/prune+identity gate, B4 corpus gate+measure+verdict). Soundness scope (node-local, merge-taint exclusion, `verify_node_local_clash`, per-solve, no-inverse/nominal) → B1/B2. FP=0 + non-Horn oracle + MISSED=0 gates → B3/B4. Decision criterion → A2 Step 4 (kill) + B4 Step 4 (viable/dead).
- **Reuse-first:** Phase A adds only `analyze_by_depth` + a harness; everything else (`analyze`, `clash_records`, probe wiring) is shipped. `verify_node_local_clash` is reused as the soundness oracle in B1.
- **Risk owned:** B3 Step 4 is the iterate-to-byte-identical loop (over-prune = MISS); the advisor's "reject cheaply" is honored by the A2 hard-zero kill gate before any Phase B effort.
- **Open confirmations the implementer resolves in-task (flagged inline):** exact `sat_class_probe` signature + ore_ont_10019 namespace (A2 Step 2); the precise clash-site + branch-descent hook lines (B3 Steps 1–2); `pool`/`tbox`/`hierarchy`/`shadow_merge_cause` field access on `HyperEngine`/`HyperNode` (B1 Step 3); whether `ClashRecord` needs `#[derive(Clone)]` (A1 Step 3).
- **Default flip is explicitly out of scope** — B4 records the VIABLE/DEAD verdict; any flip is a separate reviewed commit, per the roadmap discipline.
