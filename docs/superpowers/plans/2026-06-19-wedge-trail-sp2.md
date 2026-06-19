# Wedge Backtracking Trail (SP2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the hypertableau wedge's per-branch whole-graph clone (`Snapshot`) with a log-and-undo trail (ported from the proven `TableauTrail`), so disjunctive backtracking costs O(changes-since-checkpoint) instead of O(graph) — cutting the wall on the disjunctive-branching outliers (ore-15672 138s, wine, family) with byte-identical verdicts.

**Architecture:** A `HyperTrail` (modeled on `crates/owl-dl-tableau/src/trail.rs`) logs every wedge graph mutation since a `checkpoint()`; `rollback_to(cp)` replays entries in reverse. The `⊔` branch driver swaps "clone graph / restore clone" for "checkpoint / rollback". Pure save/restore mechanics — verdicts must not change.

**Tech Stack:** Rust (edition 2024); `owl-dl-tableau` crate (`hyper.rs`); the Konclude/closure-diff corpus net + `rustdl hyper-sat` as the measurement harness; `samply` profiler.

**Spec:** `docs/superpowers/specs/2026-06-19-wedge-trail-sp2-design.md`

**Soundness law:** This is a pure save/restore refactor — the gate is **verdict-identity** (byte-identical corpus closures), stricter than FP=0. A missed/incorrect trail entry = wrong rollback = wrong verdict (possible FP). The corpus closure-identity net is the backstop; run it before flipping the default.

---

## Conventions

- Toolchain on PATH: `export PATH="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"`.
- Build: `cargo build --release -p owl-dl-cli`.
- Branch is already `feat/wedge-trail-sp2` (spec committed at `0a6e12e`). Do NOT touch main.
- Measurement: `rustdl hyper-sat ontologies/external/ore-15672-shoin.ofn` prints per-class `branches`/`restores`/`depth`/`node_clones` + totals.

---

## Task 1: P0 measurement gate (GO/NO-GO — do this before building the trail)

The one open question: is cheaper-per-branch enough, or is the 60,858 branch count itself the wall? Answer cheaply before building.

**Files:** none (measurement only; record results in this plan's Results section).

- [ ] **Step 1: Confirm the baseline explosion**

Run: `export PATH="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"; cargo build --release -p owl-dl-cli && ./target/release/rustdl hyper-sat ontologies/external/ore-15672-shoin.ofn 2>&1 | grep -E '^#'`
Expected: `e-usage-situation` + `e-interaction-situation` Stalled, `branches≈60858/27670`, `node_clones≈branches`, `match_attempts` in the tens of millions, `depth=256`.

- [ ] **Step 2: Profile where the 5000ms goes (the trail's ceiling)**

Run samply on a single hard class probe (use `--per-class-timeout-ms 5000`):
```sh
samply record -o /tmp/ewe.profile.json -- ./target/release/rustdl hyper-sat ontologies/external/ore-15672-shoin.ofn --per-class-timeout-ms 5000
```
Then inspect the recorded profile's top frames (samply prints a local URL; or read the JSON for the heaviest symbols). **Record the fraction of time in** graph-clone (`<HyperNode as Clone>` / `Vec::clone` under the branch driver) + Horn re-propagation (`horn_fixpoint` / `match_body` / `enumerate_matches`) **vs** essential search. If clone+re-propagate is a large majority (expected, given `node_clones==branches`), the trail's ceiling is high.

- [ ] **Step 3: Bound the branch count**

Determine whether `e-usage-situation`'s search *terminates* at a bounded branch count (trail will crack it) or grows unboundedly (trail gives only a constant factor). Run with a long deadline and watch whether it ever returns `Sat`/`Unsat` instead of `Stalled`:
```sh
timeout 120 ./target/release/rustdl hyper-sat ontologies/external/ore-15672-shoin.ofn --per-class-timeout-ms 110000 2>&1 | grep -E 'e-usage-situation|e-interaction'
```
Expected one of: (a) returns `Sat` at some bounded `branches=N` (→ trail cracks it if N×cheap-branch is fast); or (b) still `Stalled` at 110s (→ branch count is the wall; trail is constant-factor only).

- [ ] **Step 4: GO/NO-GO decision — record it in Results**

GO (proceed to Task 2) iff Step 2 shows clone+re-propagate is the dominant cost AND Step 3 shows either a bounded branch count or that the constant-factor alone plausibly takes ore-15672 to a few seconds. NO-GO → stop, write the finding in Results + memory, and re-scope toward branch-count reduction (search heuristics / caching — a separate future sub-project). **Do not build the trail on a NO-GO.**

---

## Task 2: `HyperTrail` type (no wiring)

Port the `TableauTrail` shape to the wedge's state. Pure data structure + unit tests; nothing in `HyperEngine` calls it yet.

**Files:**
- Create: `crates/owl-dl-tableau/src/hyper_trail.rs`
- Modify: `crates/owl-dl-tableau/src/hyper.rs` (add `mod hyper_trail;` / `use`)
- Test: in `hyper_trail.rs` `#[cfg(test)] mod tests`

- [ ] **Step 1: Write the failing unit test**

In `hyper_trail.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use owl_dl_core::ir::{ClassId, HNode}; // adjust import to where HNode lives (hyper.rs)

    #[test]
    fn checkpoint_rollback_is_lifo_marker() {
        let mut t = HyperTrail::default();
        let cp = t.checkpoint();
        t.push(HyperTrailEntry::LabelAdded { node: HNode::new(0), concept: ClassId::new(7) });
        t.push(HyperTrailEntry::NodeCreated { prior_len: 1 });
        assert_eq!(t.len_since(cp), 2);
        let drained = t.drain_to(cp);
        assert_eq!(drained.len(), 2); // LIFO order: NodeCreated then LabelAdded
        assert!(matches!(drained[0], HyperTrailEntry::NodeCreated { .. }));
        assert_eq!(t.len_since(cp), 0);
    }
}
```
(`HNode` is defined in `hyper.rs:53` — re-export or reference it; if `HNode`/`ClassId` constructors differ, match the real signatures.)

- [ ] **Step 2: Run it; expect failure**

Run: `cargo test -p owl-dl-tableau hyper_trail::tests::checkpoint_rollback_is_lifo_marker`
Expected: FAIL (types not defined).

- [ ] **Step 3: Implement the trail type**

In `hyper_trail.rs`, mirror `trail.rs`. One entry variant per mutable wedge state captured by the current `Snapshot` (`nodes` labels+edges+preds, `representative`, `neq`, `block_index`, `origin`):
```rust
use crate::hyper::HNode;
use owl_dl_core::ir::{ClassId, Role};

/// One reversible wedge mutation. Undo logic lives in `HyperEngine`
/// (it owns the graph); the trail only records what to undo.
#[derive(Debug, Clone)]
pub enum HyperTrailEntry {
    Checkpoint,
    /// `concept` added to `node`'s label set. Undo: remove it.
    LabelAdded { node: HNode, concept: ClassId },
    /// `(role, target)` appended to `from.edges` and `(role, from)` to
    /// `target.preds`. Undo: pop both (append-only between checkpoints).
    EdgeAdded { from: HNode, role: Role, target: HNode },
    /// A node was allocated; `prior_len` = nodes-arena length before it.
    /// Undo: truncate `nodes`/`representative`/`origin` to `prior_len`.
    NodeCreated { prior_len: usize },
    /// `node`'s union-find representative was set; `prev` = old value.
    /// Undo: restore `representative[node] = prev`.
    Merged { node: HNode, prev: HNode },
    /// `(a,b)` appended to `neq`. Undo: pop it.
    NeqAdded { a: HNode, b: HNode },
    /// `node` pushed onto `block_index[role]`. Undo: pop it.
    BlockIndexAdded { role: Role, node: HNode },
    /// `node`'s origin bit was set; `prev` = old value. Undo: restore.
    OriginSet { node: HNode, prev: bool },
}

#[derive(Debug, Clone, Copy)]
pub struct HyperCheckpoint {
    pub(crate) position: usize,
}

#[derive(Debug, Default)]
pub struct HyperTrail {
    entries: Vec<HyperTrailEntry>,
}

impl HyperTrail {
    #[must_use]
    pub fn checkpoint(&mut self) -> HyperCheckpoint {
        let position = self.entries.len();
        self.entries.push(HyperTrailEntry::Checkpoint);
        HyperCheckpoint { position }
    }

    pub fn push(&mut self, e: HyperTrailEntry) {
        self.entries.push(e);
    }

    #[must_use]
    pub fn len_since(&self, cp: HyperCheckpoint) -> usize {
        // entries after the checkpoint marker
        self.entries.len().saturating_sub(cp.position + 1)
    }

    /// Pop entries back to (and including) the checkpoint marker,
    /// returning the popped non-marker entries in LIFO (undo) order.
    /// The caller applies the per-entry undo against the graph.
    pub fn drain_to(&mut self, cp: HyperCheckpoint) -> Vec<HyperTrailEntry> {
        assert!(cp.position < self.entries.len(), "checkpoint already rolled back");
        assert!(
            matches!(self.entries[cp.position], HyperTrailEntry::Checkpoint),
            "checkpoint handle does not point at a Checkpoint entry"
        );
        let mut out = Vec::with_capacity(self.entries.len() - cp.position);
        while self.entries.len() > cp.position + 1 {
            out.push(self.entries.pop().expect("non-empty by loop guard"));
        }
        self.entries.pop(); // drop the Checkpoint marker
        out
    }
}
```
(If `HNode` isn't `pub` in `hyper.rs`, make it `pub(crate)`. Keep `drain_to` returning entries so the undo — which needs `&mut HyperEngine` — lives in the engine, avoiding a borrow tangle.)

- [ ] **Step 4: Run the unit test; expect pass**

Run: `cargo test -p owl-dl-tableau hyper_trail`
Expected: PASS. Then `cargo clippy -p owl-dl-tableau --all-targets -- -D warnings` clean; `cargo fmt --all`.

- [ ] **Step 5: Commit**

```sh
git add crates/owl-dl-tableau/src/hyper_trail.rs crates/owl-dl-tableau/src/hyper.rs
git commit -m "feat(wedge): HyperTrail type (SP2 — log-and-undo, no wiring yet)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: Logged mutation helpers + audit every mutation site (flag-gated)

Add a `trail: Option<HyperTrail>` to `HyperEngine` and route every branch-reversible mutation through a logged helper that pushes the matching entry **when the trail is active**. Keep the clone-based `Snapshot` path as the default; the trail is off until Task 5.

**Files:**
- Modify: `crates/owl-dl-tableau/src/hyper.rs`
- Test: `crates/owl-dl-tableau/src/hyper.rs` `#[cfg(test)]`

- [ ] **Step 1: Add the trail field + undo dispatcher**

Add `trail: Option<HyperTrail>` to `struct HyperEngine` (init `None` in every constructor: `new`, `new_with_prebuilt`, `new_seeded`, `from_snapshot*`). Add an `apply_undo` method:
```rust
fn apply_undo(&mut self, entry: HyperTrailEntry) {
    match entry {
        HyperTrailEntry::Checkpoint => {}
        HyperTrailEntry::LabelAdded { node, concept } => {
            self.nodes[node.index()].remove_label(concept); // mirror add_label's structure
        }
        HyperTrailEntry::EdgeAdded { from, role, target } => {
            let f = &mut self.nodes[from.index()];
            if let Some(p) = f.edges.iter().rposition(|&(r, t)| r == role && t == target) {
                f.edges.remove(p);
            }
            let t = &mut self.nodes[target.index()];
            if let Some(p) = t.preds.iter().rposition(|&(r, s)| r == role && s == from) {
                t.preds.remove(p);
            }
        }
        HyperTrailEntry::NodeCreated { prior_len } => {
            self.nodes.truncate(prior_len);
            self.representative.truncate(prior_len);
            self.snapshot_origin.truncate(prior_len);
        }
        HyperTrailEntry::Merged { node, prev } => {
            self.representative[node.index()] = prev;
        }
        HyperTrailEntry::NeqAdded { a, b } => {
            if let Some(p) = self.neq.iter().rposition(|&(x, y)| x == a && y == b) {
                self.neq.remove(p);
            }
        }
        HyperTrailEntry::BlockIndexAdded { role, node } => {
            if let Some(ix) = self.block_index.as_mut() {
                if let Some(v) = ix.get_mut(&role) {
                    if let Some(p) = v.iter().rposition(|&n| n == node) {
                        v.remove(p);
                    }
                }
            }
        }
        HyperTrailEntry::OriginSet { node, prev } => {
            self.snapshot_origin[node.index()] = prev;
        }
    }
}
```
(Match field/method names to the real `HyperNode` — `remove_label` may need adding mirror to the existing label-add. Confirm `snapshot_origin` is the `origin` field the `Snapshot` saves.)

- [ ] **Step 2: Route mutations through logged helpers**

For each mutation kind, push the trail entry **before** mutating (so `prior_len`/`prev` capture the pre-state), guarded by `if let Some(t) = self.trail.as_mut() { t.push(...) }`. Audit these known sites in `hyper.rs` (the `Snapshot` captures exactly this state, so this list is complete by construction — cross-check against `struct Snapshot`):
  - **Label add** — the `add_label` method (the sole label-insertion path): log `LabelAdded` for each genuinely-new concept (skip if already present, mirroring add_label's dedup).
  - **Edge add** — the `.edges.push(...)` + `.preds.push(...)` pairs (≈ lines 1404, 1537, 2018, 2378, 2417, 2512 and the `derive_role_edge` site): log `EdgeAdded`. Prefer funneling all edge adds through one private `fn push_edge(&mut self, from, role, target)` and logging there.
  - **Node create** — `new_node` (captures `prior_len = self.nodes.len()` before push): log `NodeCreated`.
  - **Merge** — every write to `self.representative[..]`: log `Merged { prev }`.
  - **Neq** — every `self.neq.push(...)`: log `NeqAdded`.
  - **Block index** — every `block_index ... push`: log `BlockIndexAdded`.
  - **Origin** — every write to `snapshot_origin[..]` within a branch: log `OriginSet { prev }`.

- [ ] **Step 3: Build + existing suite green (trail still off)**

Run: `cargo build --release -p owl-dl-cli && cargo test -p owl-dl-tableau`
Expected: all pass (trail is `None` everywhere, so behavior is unchanged). Clippy clean; fmt.

- [ ] **Step 4: Commit**

```sh
git add crates/owl-dl-tableau/src/hyper.rs
git commit -m "feat(wedge): logged mutation helpers + undo dispatcher (SP2, trail off by default)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 4: Branch driver swap + verdict-identity differential test

Make the branch driver use checkpoint/rollback when the trail is active, and prove (differentially) it produces identical verdicts to the clone path before removing the clone.

**Files:**
- Modify: `crates/owl-dl-tableau/src/hyper.rs` (the `⊔` branch recursion ~1226+ and a `with_trail()` opt-in)
- Test: `crates/owl-dl-tableau/src/hyper.rs` `#[cfg(test)]`

- [ ] **Step 1: Add `with_trail()` opt-in**

```rust
#[must_use]
pub fn with_trail(mut self) -> Self {
    self.trail = Some(HyperTrail::default());
    self
}
```

- [ ] **Step 2: Swap the branch driver (trail path)**

In the disjunction branch loop, when `self.trail.is_some()`: replace "clone graph (`Snapshot`) → try disjunct → on fail restore clone" with:
```rust
let cp = self.trail.as_mut().expect("trail active").checkpoint();
// ... apply disjunct + propagate (logged mutations accumulate on the trail) ...
// on Unsat/Stalled of this disjunct:
let undo = self.trail.as_mut().expect("trail active").drain_to(cp);
for e in undo { self.apply_undo(e); }
self.stats.restores += 1;
```
Leave the `Snapshot`-clone path intact for `self.trail.is_none()` (default). Do NOT bump `node_clones` on the trail path.

- [ ] **Step 3: Write the verdict-identity differential test**

```rust
#[test]
fn trail_matches_clone_verdict_on_disjunctive_inputs() {
    // A handful of disjunctive clause sets exercising ⊔, ≤n merge, ∃-gen.
    for clauses in disjunctive_fixtures() {
        let mut clone_engine = HyperEngine::new(&clauses, root());
        let mut trail_engine = HyperEngine::new(&clauses, root()).with_trail();
        let v_clone = clone_engine.decide(256);
        let v_trail = trail_engine.decide(256);
        assert_eq!(v_clone, v_trail, "trail verdict diverged from clone");
    }
}
```
(`disjunctive_fixtures()` — build 5–8 small clause sets directly with `DlClause`/`Atom`, reusing the construction style already in `hyper.rs` tests like `horn_chain_derives_transitive_subsumers`. Include at least one `Sat`, one `Unsat`, one multi-level-branch case.)

- [ ] **Step 4: Run; iterate until identical**

Run: `cargo test -p owl-dl-tableau trail_matches_clone_verdict_on_disjunctive_inputs`
Expected: PASS. A divergence means a mutation site is unlogged or an undo is wrong — fix the audit (Task 3) until identical. Then full `cargo test -p owl-dl-tableau` green; clippy; fmt.

- [ ] **Step 5: Commit**

```sh
git add crates/owl-dl-tableau/src/hyper.rs
git commit -m "feat(wedge): trail-based branch checkpoint/rollback + verdict-identity test (SP2)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 5: Flip default ON + corpus closure-identity + P0 wall + perf

**Files:**
- Modify: `crates/owl-dl-reasoner/src/lib.rs` (the `HyperCache::decide`/`classify_labels`/consistency engine constructions — add `.with_trail()`), or flip `HyperEngine` to default-trail and remove the opt-in. Prefer the smallest change that makes the production wedge use the trail.

- [ ] **Step 1: Enable the trail on the production wedge paths**

Add `.with_trail()` to the `HyperEngine` constructions in the reasoner's wedge paths (`lib.rs` `HyperCache::decide` ~1103, `classify_labels` ~1139, `ConsistencyCache` ~1334). Build.

- [ ] **Step 2: P0 wall measurement**

Run: `./target/release/rustdl hyper-sat ontologies/external/ore-15672-shoin.ofn 2>&1 | grep -E '^#'`
Expected: `node_clones` ≈ 0 on the trail path; `e-usage-situation`/`e-interaction-situation` wall materially reduced (record the numbers). Then `time ./target/release/rustdl classify ontologies/external/ore-15672-shoin.ofn` — record vs the 138s baseline.

- [ ] **Step 3: Corpus closure-IDENTITY net (the sacred gate)**

Run: `cargo test --release -p owl-dl-reasoner --test konclude_closure_diff -- --include-ignored --nocapture 2>&1 | grep -iE 'rustdl_closure=|FP=|MISSED=|test result'`
Expected: every `*_closure_matches_*` fixture **FP=0 MISSED=0 with byte-identical closures** to the pre-SP2 baseline (galen 27997, notgalen 32739, sio 8904, wine 653, ore-10908 6001, ore-15672 142, alehif 247, ro 158, pizza 499, bibtex 16). The `family_inconsistency_detected` line failing under `--include-ignored` is expected. **Any closure change or FP → STOP and revert** (a trail undo is wrong).

- [ ] **Step 4: Perf non-regression on fast fixtures**

Run: `for f in galen sio; do echo -n "$f: "; { /usr/bin/time -v ./target/release/rustdl classify ontologies/$( [ $f = sio ] && echo real || echo external)/$f*.ofn >/dev/null ; } 2>&1 | grep -oE 'Elapsed.*: [0-9:.]+' | grep -oE '[0-9:.]+$'; done`
Expected: galen ≈0.2s, sio ≈2s (no regression — the trail must not slow the common case).

- [ ] **Step 5: Commit (accept) or revert**

If P0 wall improved AND closures byte-identical AND no perf regression:
```sh
git add -A
git commit -m "feat(wedge): enable backtracking trail on production wedge paths (SP2)

ore-15672 <baseline>→<new>; node_clones≈0; corpus closures byte-identical; FP=0.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```
Else: revert the flip, record the finding in Results, and stop (re-scope to branch-count reduction).

---

## Task 6: Remove the `Snapshot` clone path + docs

Only after Task 5 accepts.

- [ ] **Step 1: Delete the dead clone path**

Remove `struct Snapshot` and its clone-save/restore branch arm (now unused). Make the trail unconditional (drop the `Option`/opt-in if every path uses it). Build + full `cargo test --workspace` green; clippy `--workspace --all-targets --all-features -D warnings`; fmt.

- [ ] **Step 2: CLAUDE.md + Results**

Add an SP2 entry to CLAUDE.md (`owl-dl-tableau` section): wedge now backtracks via a log-and-undo `HyperTrail` (O(changes), ported from `TableauTrail`) instead of whole-graph clone; ore-15672 `<baseline>→<new>`; closures byte-identical; reference this plan + the spec. Fill the Results section below.

- [ ] **Step 3: Commit**

```sh
git add -A
git commit -m "refactor(wedge): drop Snapshot clone path; SP2 results + CLAUDE.md

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Results

(Filled during execution. Record: P0 GO/NO-GO + evidence; ore-15672 baseline→new wall; node_clones before/after; corpus closure-identity confirmation; perf non-regression.)
