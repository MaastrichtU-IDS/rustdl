# Wedge Node-Label Bitset Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the wedge's sorted-`Vec<ClassId>` node class-label representation with a `FixedBitSet` (O(1) `has`/`add`, bitwise `subset`/`disjoint`) to cut the ~11% membership-lookup + sorted-insert self-time on wedge-heavy SROIQ classification — *only if* a P0 profiling study shows it's net-positive.

**Architecture:** A profiling study (Task 1) decides pure-bitset / adaptive-hybrid / abort. If go: introduce an access *seam* over the labels (Task 2, pure refactor, byte-identical), then swap the backing `Vec`→`FixedBitSet` + a sparse `HashMap<ClassId,DepSet>` for deps behind that seam (Task 3), then broad A/B + keep/revert (Task 4).

**Tech Stack:** Rust, `fixedbitset 0.5` (already a workspace dep), `owl-dl-tableau` crate, the wedge engine in `crates/owl-dl-tableau/src/hyper.rs`.

## Global Constraints

- **FP=0 is SACRED.** The acceptance tripwire for every code task is **byte-identical classification closures corpus-wide** (md5 of sorted `direct`/`equiv` edges). Any diff on any fixture ⇒ revert the task.
- **Pure representation change** — no semantics/calculus change; backjumping behaviour (which branch a clash prunes) must be preserved exactly. The deps migration is the highest-risk piece.
- `cargo build --workspace` is warning-free under `RUSTFLAGS=-D warnings`; clippy `pedantic` on. `cargo fmt --all -- --check` must pass (CI rustfmt is a hard gate).
- Bitset sized to the **full class-id universe** (incl. nominals + Tseitin synthetics *above* `num_classes`), never to `num_classes` alone.
- `cargo test --workspace` green (61 result groups) at every commit.
- Cargo/toolchain on PATH: `export RUSTUP_HOME=/home/dumontier/.rustup CARGO_HOME=/home/dumontier/.cargo; export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$CARGO_HOME/bin:$PATH"`.
- Reuse the scratchpad harness: `rustdl-before`/`rustdl-after` binaries, the high-N median A/B (`highn2.sh` pattern), closure-md5, and `gdbsample.sh`. Scratchpad: `/tmp/claude-1007/-data-dumontier-rustdl/faed9e4e-6504-404f-977a-9496e3c8a905/scratchpad`.

---

### Task 1: P0 branchiness × density profiling study (GATING)

**Files:**
- Create (findings, committed): `docs/wedge-label-bitset-p0-results.md`
- Temporary instrumentation (reverted after the run, NOT committed): `crates/owl-dl-tableau/src/hyper.rs`

**Interfaces:**
- Consumes: existing `SearchStats { branches_taken, disj_branches, merge_branches, max_branch_depth, restores, node_clones }` (hyper.rs:354-372); the `hyper-sat` CLI probe (per-class branch dump).
- Produces: a **verdict** — `PURE_BITSET` | `HYBRID` | `ABORT` — recorded in the findings doc. Tasks 2-4 only run on `PURE_BITSET` or `HYBRID`.

**This task is a measurement, not TDD code.** Its deliverable is the verdict doc.

- [ ] **Step 1: Add throwaway density/clone instrumentation**

In `hyper.rs`, at the point the engine finishes a classify-level run (or per wedge `solve`), add an `eprintln!` gated on `std::env::var("RUSTDL_LABELSTATS").as_deref()==Ok("1")` that prints, to stderr, one line per engine instance: the class-universe width `W` (max `ClassId.index()+1` seen across nodes, including synthetics), node count `N`, total label count `Σlabels`, max labels on any node, and `node_clones` from `SearchStats`. Example:
```rust
if std::env::var("RUSTDL_LABELSTATS").as_deref() == Ok("1") {
    let w = self.nodes.iter().flat_map(|n| n.labels.iter().map(|c| c.index()+1)).max().unwrap_or(0);
    let n = self.nodes.len();
    let tot: usize = self.nodes.iter().map(|n| n.labels.len()).sum();
    let maxl = self.nodes.iter().map(|n| n.labels.len()).max().unwrap_or(0);
    eprintln!("LABELSTATS W={w} N={n} totlabels={tot} maxlabels={maxl} node_clones={}", self.stats.node_clones);
}
```
(Exact placement: in the function that returns the final `HyperResult` from `solve`/the engine drop — wherever `self.stats` is complete. If per-`solve`, aggregate over the run.)

- [ ] **Step 2: Build and run the broad sweep**

Build: `cargo build --release -p owl-dl-cli`. Then over the broad set — `ontologies/external/{galen,notgalen,ore-10908-sroiq,ore-15516-alchoiq,ore-15672-shoin,alehif-test}.ofn`, `ontologies/real/{sio,wine,pizza,ro,sulo,shoiq-knowledge,bibtex,family,go-basic}.ofn`, and an ORE-pilot slice (recovered onts `ore_ont_{699,1508,12698,12536}` + still-DNF `ore_ont_{10080,7499}` via `/data/dumontier/ore-run/pilot/<ont>.owl/canon.owx`) — run `RUSTDL_LABELSTATS=1 ./target/release/rustdl classify --pair-timeout-ms 1000 <f>` (cap slow onts at 200s) capturing the `LABELSTATS` lines, and `./target/release/rustdl hyper-sat --per-class-timeout-ms 100 <f>` for the branch dump (disj/merge/depth). Record into a table.

- [ ] **Step 3: Characterize and decide**

For each ontology compute the regime: **density** = `totlabels / (N × W)` (fraction of universe set per node) and **branchiness** = `branches_taken` / `node_clones` magnitude. Per-clone cost crossover: bitset clone `≈ W/8` bytes vs Vec clone `≈ 4 × (totlabels/N)` bytes — flag ontologies where `W/8 > 4 × avg_labels_per_node` (bitset clone costlier) AND `node_clones` is high (clone cost matters). Write `docs/wedge-label-bitset-p0-results.md` with the table and the verdict:
  - **PURE_BITSET** if no high-clone ontology is in the bitset-costlier regime (bitset wins/neutral everywhere that branches).
  - **HYBRID** if the bitset is costlier only in identifiable high-branch/sparse-wide ontologies — record the selector signal (e.g. `W/8 > 4×avg_labels AND expected-branchy`).
  - **ABORT** if bitset clone is costlier on the branchy onts broadly (the win can't beat the clone bloat).

- [ ] **Step 4: Revert instrumentation, commit findings**

```bash
git checkout -- crates/owl-dl-tableau/src/hyper.rs
git add docs/wedge-label-bitset-p0-results.md
git commit -m "perf(wedge-label-bitset): P0 branchiness×density study + verdict"
```

- [ ] **Step 5: GATE.** If verdict is `ABORT`, stop here — update `docs/wedge-label-bitset-p0-results.md` with the abort rationale, note it in perf memory, and do not proceed. Otherwise continue to Task 2 (carry the `HYBRID` selector signal forward if applicable).

---

### Task 2: Label-access seam (pure refactor, Vec-backed, byte-identical)

Introduce accessor methods so every label read/write goes through a seam, WITHOUT changing the backing `Vec`. This makes Task 3's field swap localized.

**Files:**
- Modify: `crates/owl-dl-tableau/src/hyper.rs` (the `impl HyperNode` block ~209-245; the `subset_sorted` callers ~1066/1069/1091; `labels_disjoint` ~1927; iteration ~1133/1356; `subset_sorted` fn ~2847)
- Test: `crates/owl-dl-tableau/src/hyper.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Produces (methods on `HyperNode`, Vec-backed in this task):
  - `fn has(&self, c: ClassId) -> bool` (exists)
  - `fn add(&mut self, c: ClassId, deps: DepSet) -> bool` (exists; keep-first semantics)
  - `fn deps_of(&self, c: ClassId) -> DepSet` (exists)
  - `fn labels_iter(&self) -> impl Iterator<Item = ClassId> + '_` — ascending class ids
  - `fn label_count(&self) -> usize`
  - `fn is_label_subset_of(&self, other: &HyperNode) -> bool` — `self.labels ⊆ other.labels`
- Consumes: nothing new.

- [ ] **Step 1: Write failing tests for the seam methods**

Add to the test module:
```rust
#[test]
fn seam_iter_is_ascending_and_complete() {
    let mut n = HyperNode::default();
    n.add(cls(5), DepSet::EMPTY); n.add(cls(1), DepSet::EMPTY); n.add(cls(3), DepSet::EMPTY);
    assert_eq!(n.labels_iter().collect::<Vec<_>>(), vec![cls(1), cls(3), cls(5)]);
    assert_eq!(n.label_count(), 3);
    assert!(n.has(cls(3)) && !n.has(cls(2)));
}
#[test]
fn seam_subset() {
    let mut a = HyperNode::default(); let mut b = HyperNode::default();
    a.add(cls(1), DepSet::EMPTY); a.add(cls(2), DepSet::EMPTY);
    b.add(cls(1), DepSet::EMPTY); b.add(cls(2), DepSet::EMPTY); b.add(cls(3), DepSet::EMPTY);
    assert!(a.is_label_subset_of(&b));
    assert!(!b.is_label_subset_of(&a));
}
#[test]
fn seam_keep_first_deps() {
    let mut n = HyperNode::default();
    assert!(n.add(cls(1), DepSet::EMPTY));
    assert!(!n.add(cls(1), DepSet::ALL)); // already present → keep first
    assert_eq!(n.deps_of(cls(1)), DepSet::EMPTY);
}
```

- [ ] **Step 2: Run tests — expect FAIL** (`labels_iter`/`label_count`/`is_label_subset_of` undefined)

Run: `cargo test -p owl-dl-tableau hyper::tests::seam 2>&1 | tail`
Expected: FAIL (method not found).

- [ ] **Step 3: Implement the seam methods (Vec-backed)**

In `impl HyperNode`:
```rust
fn labels_iter(&self) -> impl Iterator<Item = ClassId> + '_ {
    self.labels.iter().copied()
}
fn label_count(&self) -> usize { self.labels.len() }
fn is_label_subset_of(&self, other: &HyperNode) -> bool {
    subset_sorted(&self.labels, &other.labels)
}
```

- [ ] **Step 4: Route the direct-access sites through the seam**

- `subset_sorted(&self.nodes[n].labels, &self.nodes[m].labels)` at ~1066/1069/1091 → `self.nodes[n.index()].is_label_subset_of(&self.nodes[m.index()])` (read the exact local bindings first; preserve the parent-label pair checks unchanged in structure).
- `labels_disjoint` (~1927): replace `for &ca in la` / `for &cb in lb` with `for ca in self.nodes[..].labels_iter()` (clone node-index reads out of the borrow as today; keep the disjoint-pairs lookup identical).
- iteration `for c in self.nodes[idx].labels.clone()` (~1133) → `let labs: Vec<ClassId> = self.nodes[idx].labels_iter().collect(); for c in labs`.
- `…labels.contains(&seed)` (~1356) → `self.nodes[root_rep.index()].has(seed)`.

Do NOT change `subset_sorted`'s body, the `labels`/`label_deps` fields, snapshot, or `add`/`deps_of` in this task.

- [ ] **Step 5: Run unit tests + workspace tests** — expect PASS

Run: `cargo test -p owl-dl-tableau 2>&1 | tail` then `cargo test --workspace 2>&1 | grep -c 'test result: ok'` (expect 61).

- [ ] **Step 6: FP gate — byte-identical closures**

Build `cargo build --release -p owl-dl-cli`, then run the closure-md5 over the broad set and diff against the pre-task baseline (capture baseline from `git stash`/main binary first if not already saved). All must match (pure refactor).
```bash
for f in ontologies/real/sio.ofn ontologies/external/ore-15516-alchoiq.ofn ontologies/external/galen.ofn ontologies/real/pizza.ofn; do
  ./target/release/rustdl classify --pair-timeout-ms 1000 "$f" 2>/dev/null | grep -E '^(direct|equiv)' | sort | md5sum
done
```
Expected: identical to baseline hashes.

- [ ] **Step 7: fmt/clippy + commit**

```bash
cargo fmt --all -- --check && cargo clippy -p owl-dl-tableau --all-targets --all-features -- -D warnings
git add crates/owl-dl-tableau/src/hyper.rs
git commit -m "refactor(wedge): label-access seam over node labels (Vec-backed, byte-identical)"
```

---

### Task 3: Swap backing to FixedBitSet + sparse deps map

Change the field types behind the Task-2 seam. This is the FP-critical task.

**Files:**
- Modify: `crates/owl-dl-tableau/src/hyper.rs` (`HyperNode` struct ~137-143; `impl HyperNode` has/add/deps_of/labels_iter/label_count/is_label_subset_of; node creation ~681/725/1002; snapshot `pre_capture_labels` ~284/1399/1539 + the `label_deps` reconstruction ~1478; `labels_disjoint`)
- Modify: `crates/owl-dl-tableau/Cargo.toml` (add `fixedbitset.workspace = true` if not present)
- Test: `crates/owl-dl-tableau/src/hyper.rs` test module

**Interfaces:**
- Consumes: the Task-2 seam (callers unchanged).
- Produces: `HyperNode { labels: FixedBitSet, label_deps: HashMap<ClassId, DepSet>, … }`; a way to size a node's bitset to the universe width. Seam method signatures UNCHANGED.

- [ ] **Step 1: Add the dep & confirm universe sizing source**

In `crates/owl-dl-tableau/Cargo.toml` ensure `fixedbitset.workspace = true`. Determine the class-universe width: the max `ClassId.index()+1` over all clauses' atoms plus the nominal range (search where nominal/synthetic ids are assigned — the engine builder knows `num_classes` and the nominal range start). Store `class_universe_width: usize` on the engine struct, set once at construction. (Read the engine constructor to find where clauses/nominals are known; size = `max(all class ids in clauses, nominal range end, synthetic ids) + 1`.)

- [ ] **Step 2: Write failing tests for bitset-backed semantics + sizing**

```rust
#[test]
fn bitset_add_has_deps_keepfirst() {
    let mut n = HyperNode::with_universe(64);
    assert!(n.add(cls(40), DepSet::EMPTY));   // synthetic-range id > typical num_classes
    assert!(n.has(cls(40)) && !n.has(cls(41)));
    assert!(!n.add(cls(40), DepSet::ALL));     // keep-first
    assert_eq!(n.deps_of(cls(40)), DepSet::EMPTY);
    assert_eq!(n.deps_of(cls(41)), DepSet::EMPTY); // absent → EMPTY
}
#[test]
fn bitset_subset_and_iter() {
    let mut a = HyperNode::with_universe(64); let mut b = HyperNode::with_universe(64);
    for c in [1,40] { a.add(cls(c), DepSet::EMPTY); }
    for c in [1,40,60] { b.add(cls(c), DepSet::EMPTY); }
    assert!(a.is_label_subset_of(&b) && !b.is_label_subset_of(&a));
    assert_eq!(a.labels_iter().collect::<Vec<_>>(), vec![cls(1), cls(40)]); // ascending
}
```

- [ ] **Step 3: Run — expect FAIL** (`with_universe` undefined, field type mismatch). Run: `cargo test -p owl-dl-tableau hyper::tests::bitset 2>&1 | tail`.

- [ ] **Step 4: Change the field types + sizing constructor**

```rust
// struct HyperNode
labels: FixedBitSet,
label_deps: std::collections::HashMap<ClassId, DepSet>,
```
Add `fn with_universe(w: usize) -> Self { HyperNode { labels: FixedBitSet::with_capacity(w), label_deps: HashMap::new(), ..Default::default() } }` and ensure every node-creation site (681/725/1002) builds the bitset at `class_universe_width` (replace `..HyperNode::default()` label init by setting `labels: FixedBitSet::with_capacity(self.class_universe_width)`). NOTE: `#[derive(Default)]` gives a 0-width bitset + empty map — fine for the `..Default::default()` spread as long as the three real creation sites set the width explicitly.

- [ ] **Step 5: Rewrite the seam method bodies**

```rust
fn has(&self, c: ClassId) -> bool { self.labels.contains(c.index()) }
fn add(&mut self, c: ClassId, deps: DepSet) -> bool {
    if self.labels.contains(c.index()) { return false; }     // keep-first
    self.labels.insert(c.index());
    if deps != DepSet::EMPTY { self.label_deps.insert(c, deps); } // sparse
    true
}
fn deps_of(&self, c: ClassId) -> DepSet {
    if self.labels.contains(c.index()) {
        self.label_deps.get(&c).copied().unwrap_or(DepSet::EMPTY)
    } else { DepSet::EMPTY }
}
fn labels_iter(&self) -> impl Iterator<Item = ClassId> + '_ {
    self.labels.ones().map(|i| ClassId::new(u32::try_from(i).expect("class id fits u32")))
}
fn label_count(&self) -> usize { self.labels.count_ones(..) }
fn is_label_subset_of(&self, other: &HyperNode) -> bool { self.labels.is_subset(&other.labels) }
```
(`is_subset` requires equal-width sets — both sized to `class_universe_width`, guaranteed by Step 4.) Also update `add_label`/`add_label_via_backprop` (925/968) and any other writer that touched `labels`/`label_deps` directly to go through these or set both consistently.

- [ ] **Step 6: Migrate snapshot**

- `pre_capture_labels: Vec<Vec<ClassId>>` (284) — keep the type (a captured class-id list); populate from `hn.labels_iter().collect()` at 1399; the membership test at 265-its-use reads it as a list — convert that consumer to a set/contains as needed (read the exact use). Simpler: keep `Vec<ClassId>` and the consumer's logic unchanged (it already works on a list).
- The `label_deps = vec![birth_deps; labels.len()]` reconstruction at 1478 → rebuild the map: `hn.label_deps = hn.labels.ones().map(|i| (ClassId::new(i as u32), snap_node.birth_deps)).collect();` (only if `birth_deps != EMPTY`, else leave empty for sparsity — but match the existing semantics: it assigned birth_deps to ALL, so insert for all when birth_deps non-empty).

- [ ] **Step 7: `labels_disjoint`** — replace `for &ca in la` with `for ca in self.nodes[ai].labels_iter()` (and `cb` likewise); the disjoint-pairs lookup is unchanged. Remove `subset_sorted` if now unused (or keep if `pre_capture` still uses it).

- [ ] **Step 8: Build, unit tests, workspace tests**

Run: `cargo build -p owl-dl-tableau 2>&1 | tail`; `cargo test -p owl-dl-tableau 2>&1 | tail`; `cargo test --workspace 2>&1 | grep -c 'test result: ok'` (expect 61).

- [ ] **Step 9: FP gate — byte-identical closures corpus-wide**

Build `cargo build --release -p owl-dl-cli`; run closure-md5 over the FULL broad set (galen/notgalen/sio/wine/ore-10908/ore-15672/ore-15516/alehif/ro/sulo/pizza/shoiq-knowledge/bibtex/family/go-basic + the ORE-pilot slice) and diff vs the Task-2 baseline hashes. **ANY diff ⇒ the deps/snapshot migration broke backjump/blocking semantics — debug or revert.** This is the sacred gate.

- [ ] **Step 10: fmt/clippy + commit**

```bash
cargo fmt --all -- --check && cargo clippy -p owl-dl-tableau --all-targets --all-features -- -D warnings
git add crates/owl-dl-tableau/src/hyper.rs crates/owl-dl-tableau/Cargo.toml
git commit -m "perf(wedge): FixedBitSet node labels + sparse deps map (byte-identical)"
```

---

### Task 4: Broad A/B, keep/revert, and (if HYBRID) the regime selector

**Files:**
- Modify (only if HYBRID verdict): `crates/owl-dl-tableau/src/hyper.rs`
- Create: append results to `docs/wedge-label-bitset-p0-results.md`

**Interfaces:**
- Consumes: Task-3 binary (`rustdl-after`), a Task-2-or-main baseline binary (`rustdl-before`).
- Produces: keep/revert decision; if kept, the `[Unreleased]` CHANGELOG entry.

- [ ] **Step 1: Build both binaries**

Save the current (Task-3) build as `$S/rustdl-after`; `git stash` or check out the pre-Task-3 commit, `cargo build --release -p owl-dl-cli`, save as `$S/rustdl-before`; restore.

- [ ] **Step 2: Broad high-N A/B**

Run the `highn2.sh`-pattern harness over the broad set: high-N (8) interleaved median+min on fast onts (sio, ore15516, pizza, r699, r12698, alehif, ore-10908), single capped runs on slow (wine, ore-15672, go-basic), **galen as the EL control (must stay flat ±2%)**, with per-ont closure-md5 (must be OK everywhere). Record into the results doc.

- [ ] **Step 3: Decide keep/revert**

- **Keep** if the aggregate on wedge onts beats the +/-2% noise floor (faster), galen flat, all md5 OK.
- **Revert** if flat/negative (`git checkout main -- crates/owl-dl-tableau/src/hyper.rs` equivalent; discard the branch's code commits, keep the P0 + results docs). Record the negative result.
- If **HYBRID**: implement the regime selector (engine picks `FixedBitSet` vs a `Vec`-backed fallback by the P0 signal — e.g. `class_universe_width` vs expected density/branchiness, decided at engine construction, NOT per-node-runtime). Re-run Step 2 for the hybrid binary; keep only if it beats the floor without an md5 diff.

- [ ] **Step 4: If kept — CHANGELOG + final gate**

Add an `[Unreleased] ### Performance` bullet to `CHANGELOG.md` (numbers from Step 2). Re-run `cargo test --workspace` (61 groups), `cargo fmt --all -- --check`, clippy. Commit:
```bash
git add CHANGELOG.md docs/wedge-label-bitset-p0-results.md
git commit -m "perf(wedge): keep bitset labels — <X>% on wedge-heavy SROIQ, FP=0"
```

- [ ] **Step 5: Final review + merge handoff** — dispatch a final code review over the whole branch (FP-critical deps/blocking/snapshot migration), then hand to `superpowers:finishing-a-development-branch`. Do NOT merge/push until the user asks.

---

## Self-Review

**Spec coverage:** bitset labels (T3) ✓; sparse deps map (T3 step5) ✓; full-universe sizing (T3 step1/4) ✓; all touch points — has/add/deps_of (T3), subset/disjoint blocking (T2 seam + T3), iteration 1133/1356 (T2), snapshot pre_capture + 1478 (T3 step6) ✓; P0 branchiness×density study + verdict (T1) ✓; broad eval + galen control + closure-md5 (T1/T4) ✓; adaptive-hybrid path (T4 step3) ✓; abort path (T1 step5) ✓; out-of-scope items not touched ✓.

**Placeholder scan:** the engine-constructor universe-sizing source (T3 step1) and the exact snapshot-consumer at line 265 (T3 step6) are "read the exact code first" — these are genuine read-then-edit points, not hand-waves; the surrounding code is given. No TODO/TBD left.

**Type consistency:** `has`/`add`/`deps_of`/`labels_iter`/`label_count`/`is_label_subset_of`/`with_universe` signatures are identical across T2 (Vec-backed) and T3 (bitset-backed) — that invariance is the whole point of the seam. `DepSet::EMPTY`/`DepSet::ALL` and `ClassId::new`/`.index()` match existing usage.
