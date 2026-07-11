# Incremental functional/≤1 Merge in the Horn Fixpoint — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make galen classify fast and complete (MISSED 10→0, seconds) by moving deterministic `≤1`/functional merges out of the nondeterministic `solve`/`solve_at_most` search layer and into `horn_fixpoint` as an incremental completion rule (mirroring the existing `apply_nn_rule` nominal merge), with resolve-on-read replacing physical predecessor-edge rewriting.

**Architecture:** In `crates/owl-dl-tableau/src/hyper.rs`: (1) fire `≤1` merges incrementally from `process_event` (both successor-added and constraint-added triggers); (2) resolve-on-read at the hot sites that assumed root-successor-only merges; (3) conservative merge dep-set for backjumping; (4) narrow the `solve`-layer `≤n` handling to n≥2. All behind the existing `RUSTDL_INVERSE_FUNC_MERGE` flag until proven, then default-on.

**Tech Stack:** Rust (edition 2024). `RUSTUP_TOOLCHAIN=stable` on every cargo command (pinned 1.95.0 lacks `cargo`; bare build fails/reuses stale binary).

## Global Constraints

- **Soundness gate (hard):** corpus-wide FP=0 — `konclude_closure_diff` shows FP=0 on every fixture, unchanged. A single FP means STOP (unsound).
- **Completeness gate:** galen `MISSED 10→0` vs the Konclude oracle; no *new* MISSED on any other corpus ontology.
- **Termination/perf gate:** galen classification terminates in seconds; a K-ring cyclic fixture scales ~linearly (not O(K³)); no material wall regression on wine/sio/pizza.
- Clean `cargo clippy --all-targets --all-features -- -D warnings` and `cargo fmt --check` for touched crates.
- The change is **gated behind `RUSTDL_INVERSE_FUNC_MERGE`** (default OFF) through Task 1–2; Task 3 flips the default only after all gates pass.
- Pattern to mirror for the in-fixpoint merge: `apply_nn_rule` (`hyper.rs:3298`) — it already does an incremental in-place `merge_with_cause` inside `horn_fixpoint` for nominals, including the merge-causation dep-set folding. Reuse its structure.
- This is subtle reasoner-core code: at each site the plan names, READ the surrounding code and the mirrored precedent before editing; do not blind-transcribe.

---

## Task 1: Incremental `≤1`/functional merge + resolve-on-read (behind the flag)

**Files:**
- Modify: `crates/owl-dl-tableau/src/hyper.rs` — `process_event` `Event::Edge` arm (`~1583`) and `AtMost` head-atom application (`~3000`), `find_open_at_most` (`~2583`), `merge_with_cause` cause site, and the resolve-on-read read sites.
- Test: `crates/owl-dl-tableau/src/hyper.rs` `#[cfg(test)]` unit tests; `crates/owl-dl-reasoner/tests/funcmerge_inverse.rs`.

**Interfaces:**
- Consumes: `merge_with_cause(surv: HNode, folded: HNode, cause: DepSet) -> bool` (returns clash); `resolve(HNode)->HNode`; `distinct_role_succ(node, role, qual)->Vec<HNode>` (already inverse-aware behind the flag); `card_clash_deps(node,&succs)->DepSet` (`~1120`); `Node.at_most: &[(Role,Option<ClassId>,u32)]`; `Role::flip`; `inverse_func_merge_enabled()`.
- Produces: a private helper `fn enforce_at_most_one(&mut self, node: HNode, role: Role, qual: Option<ClassId>) -> FireOutcome` that, when `distinct_role_succ` ≥ 2 for a `≤1`, merges them in place and returns `Clash`/`NoChange`.

- [ ] **Step 1: Write the failing unit test (RED) — inverse `≤1` merges inside the fixpoint**

Add to the `#[cfg(test)] mod tests` in `hyper.rs` (first read the block to reuse its graph-construction helpers; adapt names to the real test API):

```rust
#[test]
fn le1_merge_fires_across_inverse_edge_in_fixpoint() {
    // SAFETY: single-threaded test; sets the opt-in flag for this test only.
    unsafe { std::env::set_var("RUSTDL_INVERSE_FUNC_MERGE", "1"); }
    // Build: A -f-> N ; f ≡ inverse(g) ; Functional(g) (≤1 g on N) ;
    // N -g-> M with M labelled Y. The inverse edge gives N a g-successor A;
    // ≤1 g forces A == M, so A must gain label Y.
    // (Construct via the same in-crate builder the sibling hyper.rs tests use;
    //  assert A's resolved labels contain Y after horn_fixpoint.)
    // ... build graph g, run eng.horn_fixpoint(FIXPOINT_ITERS) ...
    // assert!(eng.nodes[eng.resolve(a).index()].has(y));
    unsafe { std::env::remove_var("RUSTDL_INVERSE_FUNC_MERGE"); }
}
```

(If constructing the engine graph directly from the test is impractical with the private API, SKIP this in-crate unit test and rely on the `funcmerge_inverse` integration test in Step 5 as the RED→GREEN anchor — note the choice in the report. Do NOT weaken the integration assertions.)

- [ ] **Step 2: Run it — expect FAIL**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-tableau le1_merge_fires_across_inverse -- --nocapture`
Expected: FAIL (A lacks label Y — the merge doesn't fire in the fixpoint today; it's routed through `solve`).

- [ ] **Step 3: Add `enforce_at_most_one` and the successor-added trigger**

Implement `enforce_at_most_one` mirroring `apply_nn_rule` (`hyper.rs:3298`): resolve `node`; for the `(role,qual,1)` constraint, collect `distinct_role_succ(node, role, qual)`; if ≥2, merge the tail into the head with `merge_with_cause(head, other, cause)` where `cause = self.card_clash_deps(node, &succs)` (conservative — advisor §6; do NOT use a births∪level union). Return `Clash` on a clashing merge, else `NoChange`. Gate the whole helper on `crate::inverse_func_merge_enabled()`.

Call it from `process_event`'s `Event::Edge(src, role, tgt)` arm (`~1583`), AFTER the existing clause-firing, for BOTH endpoints (an edge changes `src`'s `role`-successor set AND `tgt`'s `Inverse(role)`-successor set — the funcmerge case needs the `tgt` check):
```rust
if crate::inverse_func_merge_enabled() {
    for c in self.at_most_ones(src, role) { // (role,qual) pairs on src with n==1 matching `role`
        if matches!(self.enforce_at_most_one(src, role, c), FireOutcome::Clash) { return FireOutcome::Clash; }
    }
    for c in self.at_most_ones(tgt, role.flip()) {
        if matches!(self.enforce_at_most_one(tgt, role.flip(), c), FireOutcome::Clash) { return FireOutcome::Clash; }
    }
}
```
(`at_most_ones(node, role)` = the qualifiers of `node.at_most` entries with `n==1` whose role matches `role` under the hierarchy; inline if trivial.)

- [ ] **Step 4: Add the constraint-added trigger + narrow `find_open_at_most`**

Where an `Atom::AtMost` head is applied to a node (`~3000–3024`, the site that inserts `(role,qual,n)` into `node.at_most`): if `n==1` and `inverse_func_merge_enabled()`, call `enforce_at_most_one(node, role, qual)` right after the insert (covers "≤1 added to a node that already has ≥2 successors" — advisor §2). Return its `Clash` if any.

In `find_open_at_most` (`~2583`): when the flag is on, skip `n==1` violations (they're handled in the fixpoint); keep `n≥2`. Leave a debug assertion that no `n==1` violation remains when the flag is on.

- [ ] **Step 5: Add resolve-on-read at the hot sites (advisor §4 / B)**

At each site that reads a node's edges/preds/labels assuming merges are root-successor-only, resolve the node first. Concretely (read each, adapt): `enumerate_matches` (`~2907`, the `edges`/`preds` iteration — resolve each target/source), `fire_exists` (`~3143`, witness-reuse resolve), the `Event::Edge` back-prop `preds` collection (`~1603`, resolve each `p`), and `add_label`'s target. Guard each new `resolve` so it is a no-op when nothing is merged (resolve is identity then). This is what lets a folded predecessor be read correctly without physically rewriting its in-edges.

- [ ] **Step 6: Run the unit test + funcmerge — expect PASS**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-tableau le1_merge_fires_across_inverse -- --nocapture` → PASS.
Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test funcmerge_inverse` → PASS (the test already sets `RUSTDL_INVERSE_FUNC_MERGE=1`; A⊑Y/A⊑Z).
Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-tableau` → all pass (no regression in the flag-off default path).

- [ ] **Step 7: Lints + commit**

Run: `RUSTUP_TOOLCHAIN=stable cargo clippy -p owl-dl-tableau -p owl-dl-reasoner --all-targets --all-features -- -D warnings` and `cargo fmt -p owl-dl-tableau -- --check` → clean.
```bash
git add crates/owl-dl-tableau/src/hyper.rs crates/owl-dl-reasoner/tests/funcmerge_inverse.rs
git commit -m "feat(hyper): fire ≤1/functional merges incrementally in horn_fixpoint + resolve-on-read (behind RUSTDL_INVERSE_FUNC_MERGE)"
```

---

## Task 2: Gates — galen fast+complete, corpus FP=0, K-ring linear (flag ON)

**Files:**
- Create: `crates/owl-dl-reasoner/tests/funcmerge_scaling.rs` (K-ring scaling guard).
- Test/verify only otherwise (uses `~/eval-tools` + the matrix harness).

**Interfaces:** consumes Task 1 behind the flag.

- [ ] **Step 1: K-ring scaling guard test**

Create `crates/owl-dl-reasoner/tests/funcmerge_scaling.rs`: generate a ring of `K` copies of the funcmerge pattern (class/role IRIs parameterized by index), classify with the flag set, assert it (a) derives the ring's entailed subsumptions and (b) completes — and record wall for K=5,10,20 to show sub-cubic growth. Keep K small enough to run in CI (≤20). Set/clear `RUSTDL_INVERSE_FUNC_MERGE` in the test (SetEnv guard pattern).

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test funcmerge_scaling -- --nocapture` → PASS; note the K=5/10/20 walls in the report (should not explode).

- [ ] **Step 2: Soundness gate — closure-diff FP=0 with the flag ON**

Run: `RUSTUP_TOOLCHAIN=stable RUSTDL_INVERSE_FUNC_MERGE=1 cargo test -p owl-dl-reasoner --test konclude_closure_diff --release -- --ignored --nocapture`
Expected: every present fixture `FP=0`; MISSED unchanged-or-better. Record each fixture's line. **Any FP>0 → STOP and report (unsound).**

- [ ] **Step 3: galen completeness + termination gate**

Build fresh: `RUSTUP_TOOLCHAIN=stable cargo build --release -p owl-dl-cli`.
Direct check: `time RUSTUP_TOOLCHAIN=stable RUSTDL_INVERSE_FUNC_MERGE=1 gtimeout 120 ./target/release/rustdl classify ~/eval-tools/work/galen.ofn > /tmp/galen-inc.out 2>&1; echo exit=$?` → exits 0 in seconds (not the pre-fix >6min DNF); `grep 'galen#Femur' /tmp/galen-inc.out` shows Femur now gains `BodySpace`/`Space`.
Matrix check: `RUSTUP_TOOLCHAIN=stable RUSTDL_INVERSE_FUNC_MERGE=1 ./target/release/owl-dl-bench matrix --tier curated --out /tmp/matrix-inc --pair-timeout-ms 250 --global-timeout-s 60` then verify galen `rustdl FP 0 MISSED 0` and whole-tier rustdl FP=0 (the python check from the finish flow).

- [ ] **Step 4: Record gates + commit the scaling test**

```bash
git add crates/owl-dl-reasoner/tests/funcmerge_scaling.rs
git commit -m "test: K-ring scaling guard for incremental ≤1 merge (sub-cubic)"
```
Record in the report: K-ring walls, closure-diff FP/MISSED per fixture, galen wall+MISSED, whole-tier FP. If galen MISSED>0 or any FP>0 or galen doesn't terminate fast → STOP, report; the fix is incomplete/unsound, do not proceed to Task 3.

---

## Task 3: Flip default ON, regenerate matrix, update docs

**Files:**
- Modify: `crates/owl-dl-tableau/src/lib.rs` (`inverse_func_merge_enabled` default) — or the call sites — to default ON.
- Modify: `crates/owl-dl-reasoner/tests/funcmerge_inverse.rs` (drop the now-unnecessary flag set, since default-on).
- Regenerate: `docs/benchmarks/2026-07-11-curated/` (matrix at new default).
- Modify: `README.md`, `CLAUDE.md`, `docs/known-limitations/galen-inverse-functional-completeness.md` (galen now complete by default).

**Interfaces:** consumes Tasks 1–2 (all gates green).

- [ ] **Step 1: Flip the default**

Change `inverse_func_merge_enabled()` to default ON (e.g. `std::env::var_os("RUSTDL_INVERSE_FUNC_MERGE").map_or(true, |v| v != "0")`), keeping `=0` as an escape hatch. Update the doc-comment. Drop the `RUSTDL_INVERSE_FUNC_MERGE=1` set from `funcmerge_inverse.rs` (default now derives it) — keep the `RUSTDL_CLASSIFY_SAME_TIER` set if still needed; verify the test still passes at default.

- [ ] **Step 2: Verify default gates**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test funcmerge_inverse --test funcmerge_scaling` → PASS at default.
Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test konclude_closure_diff --release -- --ignored --nocapture` → FP=0 all fixtures at default.
Rebuild fresh CLI + `time ... classify galen.ofn` at DEFAULT → fast + Femur gains Space.

- [ ] **Step 3: Regenerate the authoritative matrix (default)**

`RUSTUP_TOOLCHAIN=stable cargo build --release -p owl-dl-cli -p owl-dl-bench`
`RUSTUP_TOOLCHAIN=stable ./target/release/owl-dl-bench matrix --tier curated --out docs/benchmarks/2026-07-11-curated --pair-timeout-ms 250 --global-timeout-s 60`
Verify galen `rustdl FP 0 MISSED 0`, whole-tier rustdl FP=0.

- [ ] **Step 4: Update docs to "sound; complete on the curated corpus"**

In `README.md`, `CLAUDE.md`, and `docs/known-limitations/galen-inverse-functional-completeness.md`: update the galen residual from "misses 10 by default (opt-in fix)" to "closed by default — galen now classifies completely (MISSED=0); the incremental functional/≤1 merge is on by default". Keep the honest history (what was wrong + how it was fixed). Restore the completeness-contract statement now that Horn⟹MISSED=0 holds on galen.

- [ ] **Step 5: Full lints + commit**

Run: `RUSTUP_TOOLCHAIN=stable cargo clippy -p owl-dl-tableau -p owl-dl-reasoner -p owl-dl-bench --all-targets --all-features -- -D warnings`; `cargo fmt --all -- --check`.
```bash
git add crates/owl-dl-tableau/src/lib.rs crates/owl-dl-reasoner/tests/funcmerge_inverse.rs docs/benchmarks/2026-07-11-curated docs/known-limitations README.md CLAUDE.md
git commit -m "feat(hyper): default-on incremental ≤1 merge; galen now complete; regen matrix + docs"
```

---

## Self-Review

**Spec coverage:**
- Incremental `≤1` merge in `horn_fixpoint` (both triggers) → Task 1 Steps 3–4. ✓
- Resolve-on-read (no in-edge rewrite) → Task 1 Step 5. ✓
- Conservative merge dep-set (`card_clash_deps`/ALL) → Task 1 Step 3. ✓
- Narrow `find_open_at_most` to n≥2 → Task 1 Step 4. ✓
- Termination gate + K-ring → Task 2 Steps 1,3. ✓
- Soundness FP=0 / galen MISSED 10→0 → Task 2 Steps 2–3. ✓
- Default flip + matrix regen + docs → Task 3. ✓
- Precedent (apply_nn_rule) referenced → Global Constraints + Task 1 Step 3. ✓
- Gated during dev, default only after gates → flag flow across tasks. ✓

**Placeholder scan:** The unit-test bodies in Task 1 Step 1 are intentionally sketched because the in-crate graph-builder API must be read first (the step says so and gives a concrete fallback to the integration test); every command, gate, and the merge/dep/trigger logic is concrete. The reasoner-core edits are "read-the-site-and-mirror-`apply_nn_rule`" by necessity — anchors (file:line), the pattern, the dep-set, and the exact call shape are all specified.

**Type consistency:** `merge_with_cause(HNode,HNode,DepSet)->bool`, `resolve(HNode)->HNode`, `card_clash_deps(HNode,&[HNode])->DepSet`, `distinct_role_succ(HNode,Role,Option<ClassId>)->Vec<HNode>`, `enforce_at_most_one(HNode,Role,Option<ClassId>)->FireOutcome`, `inverse_func_merge_enabled()->bool` used consistently across tasks and matched to the code read from `hyper.rs`.
