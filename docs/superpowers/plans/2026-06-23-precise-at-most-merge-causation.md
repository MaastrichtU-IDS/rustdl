# Precise ≤n merge-causation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the at-most (`≤n`) merge rule precise dependency-directed backjumping (mirroring the ⊔ rule), replacing its `DepSet::ALL` clash reporting — Konclude's `CMERGEDependencyNode` branch-tag realized on rustdl's tableau, gated `RUSTDL_PRECISE_MERGE_DEPS` (default OFF during the gate).

**Architecture:** `solve_at_most`/`partition_rec` (hyper.rs) currently merge with `DepSet::EMPTY` causation (→ taint → `card_clash_deps` `DepSet::ALL`) and report `DepSet::ALL` at partition exhaustion. The ⊔ rule (hyper.rs:1707–1752) already does textbook backjumping over a structurally identical enumeration. This change makes the `≤n` rule mirror it: assign a decision level `d`, pass `{d} ∪ at_most_dep` as merge causation, accumulate child clash deps, backjump on `!contains(d)`, propagate `combined.remove(d)` at exhaustion — and **decline to `DepSet::ALL` whenever a `≠` participates** (untracked `≠`-provenance, the FP hole).

**Tech Stack:** Rust (edition 2024), crate `owl-dl-tableau` (the wedge `HyperEngine`), crate `owl-dl-reasoner` (engine construction + env gate, `konclude_closure_diff` corpus test).

## Global Constraints

- **FP=0 is sacred.** A single corpus FP (closure-diff regression) is a NO-GO → revert. This is FP-critical merge code (the increment-3 false-`Unsat` graveyard).
- **Flag-OFF path byte-identical to current `main`.** `precise_merge_deps == false` must run the exact existing code (EMPTY cause + `DepSet::ALL`). Default OFF during the gate.
- Soundness rests on `{d} ∪ at_most_dep ∪ per-fact c_deps` being the COMPLETE causation on `≠`-free merges (structural identity to the ⊔ rule). The precise path MUST decline (→ `DepSet::ALL`) whenever a `≠`/merge-taint participates.
- Branch: `feat/precise-merge-deps` off `feat/build-once-redesign` (NOT off `main`).
- `cargo fmt --all -- --check` clean; `cargo clippy --workspace --all-targets --all-features -- -D warnings` clean (pedantic on); `cargo test --workspace` green.
- Toolchain (prefix every cargo command): `export RUSTUP_HOME=/home/dumontier/.rustup CARGO_HOME=/home/dumontier/.cargo; export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$CARGO_HOME/bin:$PATH"`
- Commit only when the human asks. Commit messages end with a blank line then:
  `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`
  `Claude-Session: https://claude.ai/code/session_01HSzon7V2wkhrudxBNAJduh`

---

## File Structure

- `crates/owl-dl-tableau/src/hyper.rs` — all engine changes: new `precise_merge_deps` field + `merge_precise_declined` flag, `with_precise_merge_deps` builder, the `merge_with_cause` precise branch, the `solve_at_most`/`partition_rec` backjumping, and the white-box tests (in the existing `#[cfg(test)] mod tests`).
- `crates/owl-dl-reasoner/src/lib.rs` — `hyper_precise_merge_deps_enabled()` env helper (default OFF) + wiring into engine construction next to `hyper_precise_card_deps_enabled()`.
- `crates/owl-dl-reasoner/tests/sat_guide_gate.rs` is NOT reused; the branch measurement reuses `decide_pair_probe` directly in Task 6.

---

### Task 1: `precise_merge_deps` gate scaffolding (field + builder + env helper), flag-OFF byte-identical

**Files:**
- Modify: `crates/owl-dl-tableau/src/hyper.rs` (`HyperEngine` struct ~448–575; constructors `new` ~729, `new_with_prebuilt` ~768, `new_seeded` ~1630; builder near `with_precise_card_deps` ~781)
- Modify: `crates/owl-dl-reasoner/src/lib.rs` (env helper near `hyper_precise_card_deps_enabled` ~1181; engine wiring near ~1014)
- Test: `crates/owl-dl-tableau/src/hyper.rs` (`mod tests`)

**Interfaces:**
- Produces: `HyperEngine` field `precise_merge_deps: bool`; field `merge_precise_declined: bool` (per-`solve_at_most` decline flag, used in Tasks 2–3); `pub fn with_precise_merge_deps(mut self) -> Self`; `owl_dl_reasoner::hyper_precise_merge_deps_enabled() -> bool` (default **false**).

- [ ] **Step 1: Write the failing test**

In `mod tests`:
```rust
#[test]
fn precise_merge_deps_builder_sets_flag_and_off_is_default() {
    let role = Role::Named(RoleId::new(0));
    let a = cls(0);
    let clauses = vec![DlClause { body: vec![Atom::Class(a, X)], head: vec![] }];
    // Default OFF: a plain engine has precise_merge_deps == false; builder flips it.
    let off = HyperEngine::new(&clauses, a);
    assert!(!off.precise_merge_deps_for_test());
    let on = HyperEngine::new(&clauses, a).with_precise_merge_deps();
    assert!(on.precise_merge_deps_for_test());
}
```
Add a test-only accessor next to the field:
```rust
#[cfg(test)]
pub(crate) fn precise_merge_deps_for_test(&self) -> bool { self.precise_merge_deps }
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `cargo test -p owl-dl-tableau --lib precise_merge_deps_builder -- --nocapture`
Expected: FAIL to compile (field/builder absent).

- [ ] **Step 3: Add the field + flag + builder**

In the `HyperEngine` struct, next to `precise_card_deps: bool` (~470):
```rust
    /// Opt-in (`RUSTDL_PRECISE_MERGE_DEPS`, via [`Self::with_precise_merge_deps`]):
    /// the ≤n merge rule does dependency-directed backjumping (precise merge
    /// causation) instead of reporting `DepSet::ALL`. Default OFF.
    precise_merge_deps: bool,
    /// Per-`solve_at_most`-call flag: set true when the precise path encounters a
    /// `≠`/merge-taint it cannot attribute, forcing the conservative `DepSet::ALL`
    /// fallback at partition exhaustion. Reset at each `solve_at_most` entry.
    merge_precise_declined: bool,
```
Add `precise_merge_deps: false,` and `merge_precise_declined: false,` to the struct literal in EACH of `new`, `new_with_prebuilt`, `new_seeded`.

Next to `with_precise_card_deps` (~781):
```rust
    /// Enable precise ≤n merge-causation backjumping. See [`Self::precise_merge_deps`].
    #[must_use]
    pub fn with_precise_merge_deps(mut self) -> Self {
        self.precise_merge_deps = true;
        self
    }
```
clippy may flag the field as never-read until Task 3 — add `#[allow(dead_code)]` on `merge_precise_declined` ONLY (it is read in Task 3), with comment `// read in Tasks 2-3`; `precise_merge_deps` is read by the test accessor so needs no allow.

- [ ] **Step 4: Add the reasoner env helper + wiring**

In `crates/owl-dl-reasoner/src/lib.rs`, next to `hyper_precise_card_deps_enabled` (~1181):
```rust
/// `RUSTDL_PRECISE_MERGE_DEPS` (default OFF — opt-in during the gate). When set
/// to a non-empty value other than "0", the ≤n merge rule does precise
/// dependency-directed backjumping. See the precise-merge-deps spec.
pub fn hyper_precise_merge_deps_enabled() -> bool {
    std::env::var_os("RUSTDL_PRECISE_MERGE_DEPS").is_some_and(|v| v != "0" && !v.is_empty())
}
```
At the engine construction site where `with_precise_card_deps` is chained (~1014):
```rust
            if hyper_precise_merge_deps_enabled() {
                engine = engine.with_precise_merge_deps();
            }
```
NOTE: apply this at EVERY engine-construction site that chains `with_precise_card_deps` (grep `with_precise_card_deps(` in lib.rs — there may be more than one decide path). Mirror each.

- [ ] **Step 5: Run the test + the flag-OFF suite**

Run: `cargo test -p owl-dl-tableau --lib precise_merge_deps_builder -- --nocapture` → PASS.
Run: `cargo test -p owl-dl-tableau` → all existing tests PASS (no behavior change; field is inert).

- [ ] **Step 6: fmt + clippy + commit**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
git add crates/owl-dl-tableau/src/hyper.rs crates/owl-dl-reasoner/src/lib.rs
git commit  # message: "feat(arch): RUSTDL_PRECISE_MERGE_DEPS gate scaffolding (default OFF, inert)" + trailers
```

---

### Task 2: `merge_with_cause` precise branch — fold cause into ≤n dep instead of tainting; decline on ≠

**Files:**
- Modify: `crates/owl-dl-tableau/src/hyper.rs` (`merge_with_cause` ~2121–2195, especially the `are_neq` early-out ~2134 and the `at_most_tainted` set ~2188–2194)
- Test: `crates/owl-dl-tableau/src/hyper.rs` (`mod tests`)

**Interfaces:**
- Consumes: `precise_merge_deps`, `merge_precise_declined` (Task 1).
- Produces: precise behavior inside `merge_with_cause` active ONLY when `self.precise_merge_deps && cause_deps != DepSet::EMPTY`. No signature change (still `merge_with_cause(&mut self, s_i, s_j, cause_deps) -> bool`).

- [ ] **Step 1: Write the failing white-box test**

```rust
#[test]
fn precise_merge_fold_avoids_taint_on_clean_merge() {
    // Two mergeable successors of a node, ≤1 forces a merge. With a non-empty
    // cause and precise mode, the merge-inherited ≤n must fold the cause into
    // at_most_dep WITHOUT setting at_most_tainted (the taint exists only because
    // causation was untracked). White-box: drive merge_with_cause directly.
    let role = Role::Named(RoleId::new(0));
    let a = cls(0);
    let clauses = vec![DlClause { body: vec![Atom::Class(a, X)], head: vec![] }];
    let mut eng = HyperEngine::new(&clauses, a).with_precise_merge_deps();
    let (si, sj) = eng.make_two_succs_with_atmost_for_test(role); // helper below
    let cause = DepSet::EMPTY.insert(3); // {d=3}
    let clashed = eng.merge_with_cause_for_test(si, sj, cause);
    assert!(!clashed);
    assert!(!eng.node_at_most_tainted_for_test(si), "clean precise merge must NOT taint");
    assert!(eng.node_at_most_dep_for_test(si).contains(3), "cause folded into at_most_dep");
}
```
Add the minimal test-only helpers next to the field accessor (Task 1):
```rust
#[cfg(test)]
pub(crate) fn merge_with_cause_for_test(&mut self, i: HNode, j: HNode, c: DepSet) -> bool {
    self.merge_with_cause(i, j, c)
}
#[cfg(test)]
pub(crate) fn node_at_most_tainted_for_test(&self, n: HNode) -> bool {
    self.nodes[self.resolve(n).index()].at_most_tainted
}
#[cfg(test)]
pub(crate) fn node_at_most_dep_for_test(&self, n: HNode) -> DepSet {
    self.nodes[self.resolve(n).index()].at_most_dep
}
```
For `make_two_succs_with_atmost_for_test`: add a test helper that pushes two fresh nodes (via the same `new_node` path the engine uses), gives `sj` a `≤1` `at_most` entry with some `at_most_dep`, and returns `(si, sj)`. Read `new_node` (~1012) and the `at_most` field usage to construct it; keep it minimal (no edges needed for this dep-fold assertion).

- [ ] **Step 2: Run it to confirm it fails**

Run: `cargo test -p owl-dl-tableau --lib precise_merge_fold_avoids_taint -- --nocapture`
Expected: FAIL (today taints unconditionally).

- [ ] **Step 3: Implement the precise branch in `merge_with_cause`**

At the `at_most_tainted` block (~2188–2194), replace the unconditional taint with a precise-aware version:
```rust
        if !self.nodes[s_j.index()].at_most.is_empty() {
            let sj_dep = self.nodes[s_j.index()].at_most_dep;
            let ni = &mut self.nodes[s_i.index()];
            if self.precise_merge_deps && cause_deps != DepSet::EMPTY {
                // Precise path: causation IS tracked (cause_deps carries the ≤n
                // decision level), so fold it into at_most_dep and do NOT taint.
                ni.at_most_dep = ni.at_most_dep.union(sj_dep).union(cause_deps);
            } else {
                // Conservative path (untracked causation): taint → card_clash_deps
                // falls back to DepSet::ALL.
                ni.at_most_tainted = true;
                ni.at_most_dep = ni.at_most_dep.union(sj_dep);
            }
        }
```
At the `are_neq` early-out (~2134): keep `self.clash_deps = DepSet::ALL` (sound), but ALSO record the decline so `solve_at_most` won't try to be precise after a `≠`-driven clash:
```rust
        if self.are_neq(s_i, s_j) {
            self.clash_deps = DepSet::ALL;
            self.merge_precise_declined = true; // ≠-provenance untracked
            return true;
        }
```
Remove the `#[allow(dead_code)]` on `merge_precise_declined` (now read).

NOTE: the existing NN-merge-taint propagation block immediately after (the `nn_tainted` fold, ~2196+) is UNCHANGED — it is a different (nominal) taint channel and stays conservative.

- [ ] **Step 4: Run the test + flag-OFF suite**

Run: `cargo test -p owl-dl-tableau --lib precise_merge_fold_avoids_taint -- --nocapture` → PASS.
Run: `cargo test -p owl-dl-tableau` → all PASS (flag-OFF unchanged; precise branch dormant until Task 3 passes non-empty cause from `solve_at_most`).

- [ ] **Step 5: fmt + clippy + commit**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
git add crates/owl-dl-tableau/src/hyper.rs
git commit  # "feat(arch): merge_with_cause precise branch — fold ≤n cause instead of taint; decline on ≠" + trailers
```

---

### Task 3: `solve_at_most`/`partition_rec` dependency-directed backjumping

**Files:**
- Modify: `crates/owl-dl-tableau/src/hyper.rs` (`solve_at_most` ~1994–2005, `partition_rec` ~2015–2071, `find_open_at_most` to get the violating node's `at_most_dep`)
- Test: `crates/owl-dl-tableau/src/hyper.rs` (`mod tests`) — the verdict-preservation pair

**Interfaces:**
- Consumes: `precise_merge_deps`, `merge_precise_declined`, `init_depth` (~421), the violating node + its `at_most_dep`.
- Produces: precise exhaustion deps; the backjumping loop. No signature change to `solve_at_most`.

NOTE on the violating node's dep: `solve_at_most(succs, n, depth)` does not currently receive the violating node. `find_open_at_most` (~2077) returns `(node, succs, n)`; the `solve` caller (~1755) passes `succs`/`n` only. Thread the violating `node` (an `HNode`) into `solve_at_most` so its `at_most_dep` is available — update the call site at ~1755 (`return self.solve_at_most(node, &succs, n as usize, depth);`).

- [ ] **Step 1: Write the failing verdict-preservation tests**

Mirror `precise_card_deps_preserves_{unsat,sat}_verdict` (hyper.rs:2941/2983) but with `.with_precise_merge_deps()`:
```rust
#[test]
fn precise_merge_deps_preserves_unsat_verdict() {
    let role = Role::Named(RoleId::new(0));
    let (a, b, c, d1, d2) = (cls(0), cls(1), cls(2), cls(3), cls(4));
    let clauses = vec![
        DlClause { body: vec![Atom::Class(a, X)], head: vec![Atom::Exists(role, b, X)] },
        DlClause { body: vec![Atom::Class(a, X)], head: vec![Atom::Exists(role, c, X)] },
        DlClause { body: vec![Atom::Class(b, X), Atom::Class(c, X)], head: vec![] },
        DlClause { body: vec![Atom::Class(a, X)], head: vec![Atom::AtMost(role, None, 1, X)] },
        DlClause { body: vec![Atom::Class(a, X)], head: vec![Atom::Class(d1, X), Atom::Class(d2, X)] },
    ];
    let off = HyperEngine::new(&clauses, a).decide(64);
    let on = HyperEngine::new(&clauses, a).with_precise_merge_deps().decide(64);
    assert_eq!(off, HyperResult::Unsat);
    assert_eq!(on, off, "precise-merge-deps changed the verdict — UNSOUND");
}

#[test]
fn precise_merge_deps_preserves_sat_verdict() {
    let role = Role::Named(RoleId::new(0));
    let (a, b, c) = (cls(0), cls(1), cls(2));
    let clauses = vec![
        DlClause { body: vec![Atom::Class(a, X)], head: vec![Atom::Exists(role, b, X)] },
        DlClause { body: vec![Atom::Class(a, X)], head: vec![Atom::Exists(role, c, X)] },
        DlClause { body: vec![Atom::Class(a, X)], head: vec![Atom::AtMost(role, None, 1, X)] },
    ];
    let off = HyperEngine::new(&clauses, a).decide(64);
    let on = HyperEngine::new(&clauses, a).with_precise_merge_deps().decide(64);
    assert_eq!(off, HyperResult::Sat);
    assert_eq!(on, off, "precise-merge-deps changed the verdict — UNSOUND");
}
```

- [ ] **Step 2: Run them to confirm they pass OFF and need the impl**

Run: `cargo test -p owl-dl-tableau --lib precise_merge_deps_preserves -- --nocapture`
Expected: these may already PASS (the impl is verdict-preserving by design). They are GUARDRAILS, not red-first drivers — confirm they pass after Step 3 and would FAIL if the impl flipped a verdict. (If you want a red-first signal, temporarily make `combined.remove(d)` return `DepSet::EMPTY` and confirm a verdict flips/over-backjumps, then revert.)

- [ ] **Step 3: Implement the backjumping**

Update the `solve` call site (~1755) and `solve_at_most` signature to take the violating `node`:
```rust
    fn solve_at_most(&mut self, node: HNode, succs: &[HNode], n: usize, depth: usize) -> HyperResult {
        self.merge_precise_declined = false;
        let precise = self.precise_merge_deps;
        let d = u32::try_from(self.init_depth - depth).unwrap_or(u32::MAX);
        let at_most_dep = self.nodes[self.resolve(node).index()].at_most_dep;
        let cause = if precise { at_most_dep.insert(d) } else { DepSet::EMPTY };
        let mut groups: Vec<Vec<HNode>> = Vec::with_capacity(n);
        let mut any_stalled = false;
        let mut combined = DepSet::EMPTY;
        if let Some(sat) =
            self.partition_rec(succs, 0, &mut groups, n, depth, &mut any_stalled, cause, d, &mut combined)
        {
            return sat;
        }
        if any_stalled {
            return HyperResult::Stalled;
        }
        if precise && !self.merge_precise_declined {
            self.clash_deps = combined.remove(d);
        } else {
            self.clash_deps = DepSet::ALL;
        }
        HyperResult::Unsat
    }
```
In `partition_rec`, extend the signature with `cause: DepSet, d: u32, combined: &mut DepSet` and at the complete-partition arm (~2024–2048):
```rust
        if idx == succs.len() {
            let saved = self.save();
            self.stats.branches_taken += 1;
            self.stats.merge_branches += 1;
            let mut clashed = false;
            'blocks: for block in groups.iter() {
                let rep = block[0];
                for &other in &block[1..] {
                    if self.merge_with_cause(rep, other, cause) {
                        clashed = true;
                        break 'blocks;
                    }
                }
            }
            if !clashed {
                match self.solve(depth - 1) {
                    HyperResult::Sat => return Some(HyperResult::Sat),
                    HyperResult::Unsat => {
                        let child = self.clash_deps;
                        if self.precise_merge_deps && !self.merge_precise_declined && !child.contains(d) {
                            // Backjump: this ≤n decision didn't contribute.
                            self.clash_deps = child;
                            self.restore(saved);
                            return Some(HyperResult::Unsat);
                        }
                        *combined = combined.union(child);
                    }
                    HyperResult::Stalled => *any_stalled = true,
                }
            } else {
                // merge-time clash: fold its deps into combined (it set clash_deps)
                *combined = combined.union(self.clash_deps);
            }
            self.restore(saved);
            return None;
        }
```
Thread `cause, d, combined` through the two recursive `partition_rec` calls (~2055, ~2064). For the flag-OFF path, `cause == EMPTY`, `precise_merge_deps == false` ⇒ the backjump branch is skipped and exhaustion uses `DepSet::ALL` — byte-identical to today (the `*combined` accumulation is dead when `DepSet::ALL` is reported).

IMPORTANT — `Some(HyperResult::Unsat)` semantics: returning `Some(Unsat)` from `partition_rec` short-circuits the enumeration (like `Some(Sat)`). Verify `solve_at_most` returns it directly (the `if let Some(sat) = ... { return sat; }` returns ANY `Some`, including `Unsat` — that is the desired backjump short-circuit; the returned `clash_deps` is already set).

- [ ] **Step 4: Run the verdict-preservation tests + full tableau suite**

Run: `cargo test -p owl-dl-tableau --lib precise_merge_deps_preserves -- --nocapture` → PASS (both).
Run: `cargo test -p owl-dl-tableau` → ALL pass. This is the flag-OFF byte-identical check (the existing `≤n` tests like `at_most_clash_precheck_collapses_branching`, the InterestingPizza-style merge tests, must be unchanged).

- [ ] **Step 5: fmt + clippy + commit**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
git add crates/owl-dl-tableau/src/hyper.rs
git commit  # "feat(arch): ≤n rule dependency-directed backjumping (solve_at_most/partition_rec)" + trailers
```

---

### Task 4: ≠-participating fallback test (the FP hole guard)

**Files:**
- Test: `crates/owl-dl-tableau/src/hyper.rs` (`mod tests`)

**Interfaces:** Consumes the precise path (Tasks 2–3). Produces no code, one guard test.

- [ ] **Step 1: Write the test**

A scenario where a `≠` participates in the `≤n` merge, asserting (a) the verdict is preserved ON vs OFF, and (b) the precise path declined (so exhaustion used `DepSet::ALL`). The clean signal for (b): build the `≥2 ⊓ ≤1`-with-forced-`≠` unsat shape and assert `merge_precise_declined`-driven fallback via a test accessor:
```rust
#[cfg(test)]
pub(crate) fn merge_precise_declined_for_test(&self) -> bool { self.merge_precise_declined }
```
```rust
#[test]
fn precise_merge_deps_declines_when_neq_participates() {
    // ≥2 distinct (≠-forced) R-successors under ≤1 ⇒ unsat with NO valid merge.
    // The precise path must DECLINE (≠-provenance untracked) and preserve Unsat.
    let role = Role::Named(RoleId::new(0));
    let (a, b, c) = (cls(0), cls(1), cls(2));
    let clauses = vec![
        DlClause { body: vec![Atom::Class(a, X)], head: vec![Atom::Exists(role, b, X)] },
        DlClause { body: vec![Atom::Class(a, X)], head: vec![Atom::Exists(role, c, X)] },
        DlClause { body: vec![Atom::Class(b, X), Atom::Class(c, X)], head: vec![] }, // b,c disjoint ⇒ ≠
        DlClause { body: vec![Atom::Class(a, X)], head: vec![Atom::AtMost(role, None, 1, X)] },
    ];
    let off = HyperEngine::new(&clauses, a).decide(64);
    let on = HyperEngine::new(&clauses, a).with_precise_merge_deps().decide(64);
    assert_eq!(off, HyperResult::Unsat);
    assert_eq!(on, off, "≠-participating precise merge must preserve Unsat");
}
```
(If the `forced_distinct_exceeds` pre-check at hyper.rs:1763 short-circuits this before `solve_at_most`, the test still validates verdict preservation; adapt the shape with a third successor or a non-pre-checked path so `solve_at_most` is actually entered with a `≠` — read `forced_distinct_exceeds` ~1763 and `must_be_distinct` to construct a case that reaches `partition_rec` with a `≠`. Document which path the test exercises in a comment.)

- [ ] **Step 2: Run it**

Run: `cargo test -p owl-dl-tableau --lib precise_merge_deps_declines -- --nocapture` → PASS.

- [ ] **Step 3: fmt + clippy + commit**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
git add crates/owl-dl-tableau/src/hyper.rs
git commit  # "test(arch): ≠-participating ≤n merge declines to DepSet::ALL (FP hole guard)" + trailers
```

---

### Task 5: Backjumping-improvement canary (the architecture-works evidence)

**Files:**
- Test: `crates/owl-dl-tableau/src/hyper.rs` (`mod tests`)

**Interfaces:** Consumes the precise path. Produces one canary asserting the precise path actually backjumps (fewer branches than OFF) on a ⊔-above-an-independent-≤n-merge shape.

- [ ] **Step 1: Write the canary**

Construct a satisfiable ontology where a top-level disjunction `a ⊑ d1 ⊔ d2` is INDEPENDENT of a `≤n` merge deeper down, such that flag-OFF explores both ⊔ branches × the merge partitions (because the merge clash reports `DepSet::ALL`, defeating backjumping above it), while flag-ON backjumps past the irrelevant ⊔. Assert branch counts via `decide_with_stats`:
```rust
#[test]
fn precise_merge_deps_backjumps_past_independent_disjunction() {
    // Shape: a ⊑ d1 ⊔ d2 (irrelevant ⊔); a ⊑ ∃r.b, a ⊑ ∃r.c, a ⊑ ≤1 r,
    // b ⊓ c ⊑ ⊥ (the ≤1 merge clashes regardless of the d1/d2 choice).
    // OFF: the ≤1-merge clash reports DepSet::ALL ⇒ no backjump ⇒ the d1/d2
    // disjunction is re-explored. ON: clash deps exclude the ⊔ decision ⇒
    // backjump ⇒ fewer branches.
    let role = Role::Named(RoleId::new(0));
    let (a, b, c, d1, d2) = (cls(0), cls(1), cls(2), cls(3), cls(4));
    let clauses = vec![
        DlClause { body: vec![Atom::Class(a, X)], head: vec![Atom::Class(d1, X), Atom::Class(d2, X)] },
        DlClause { body: vec![Atom::Class(a, X)], head: vec![Atom::Exists(role, b, X)] },
        DlClause { body: vec![Atom::Class(a, X)], head: vec![Atom::Exists(role, c, X)] },
        DlClause { body: vec![Atom::Class(a, X)], head: vec![Atom::AtMost(role, None, 1, X)] },
        DlClause { body: vec![Atom::Class(b, X), Atom::Class(c, X)], head: vec![] },
    ];
    let (off_res, off_stats) = HyperEngine::new(&clauses, a).decide_with_stats(64, None);
    let (on_res, on_stats) =
        HyperEngine::new(&clauses, a).with_precise_merge_deps().decide_with_stats(64, None);
    assert_eq!(off_res, on_res, "verdict must match");
    assert_eq!(on_res, HyperResult::Unsat);
    assert!(
        on_stats.branches_taken < off_stats.branches_taken,
        "precise merge deps must backjump (fewer branches): off={} on={}",
        off_stats.branches_taken, on_stats.branches_taken
    );
}
```
CHECK the `decide_with_stats` signature (grep it in hyper.rs — it may be `decide_with_stats(&self, sub, sup, depth, deadline)` on `HyperCache`, NOT on `HyperEngine`). If `HyperEngine` exposes only `decide(depth)`, add the stats via `engine.decide(64)` then read `engine.stats_for_test()` — add a `#[cfg(test)] pub(crate) fn stats_for_test(&self) -> SearchStats { self.stats.clone() }` accessor (or read `branches_taken` directly). Use whichever the engine actually provides; the assertion is `on.branches_taken < off.branches_taken`.

If the chosen shape does NOT show a reduction (the engine's existing pre-checks may already collapse it), iterate the shape (add a second independent ⊔, deepen the merge) until ON < OFF, OR if no reduction is achievable document it: that means the precise path is sound-but-inert on synthetics and the Task 6 corpus measurement becomes the sole evidence. Do NOT fake the assertion.

- [ ] **Step 2: Run + fmt + clippy + commit**

Run: `cargo test -p owl-dl-tableau --lib precise_merge_deps_backjumps -- --nocapture` → PASS.
```bash
cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings
git add crates/owl-dl-tableau/src/hyper.rs
git commit  # "test(arch): ≤n precise backjump canary (fewer branches vs OFF)" + trailers
```

---

### Task 6: FP=0 corpus gate + branch measurement + (on success) flip default-ON

**Files:**
- Modify (only on success): `crates/owl-dl-reasoner/src/lib.rs` (`hyper_precise_merge_deps_enabled` default)
- Create: `docs/precise-merge-deps-gate-results-2026-06-23.md` (durable verdict)

- [ ] **Step 1: Flag-OFF baseline byte-identical**

Run (flag unset = OFF): `cargo test --workspace` → all green. This confirms the OFF path is byte-identical to `main`.

- [ ] **Step 2: FP=0 corpus gate, flag ON**

Run the closure-diff with the flag ON over the oracled fixtures (cardinality-bearing ones load-bearing):
```bash
export RUSTDL_PRECISE_MERGE_DEPS=1
RUSTDL_TEST_PAIR_MS=1000 cargo test -p owl-dl-reasoner --test konclude_closure_diff -- --ignored --nocapture \
  wine_ sio_ pizza_ ro_ bibtex_ galen_ notgalen_ ore_15672 ore_10908
```
Expected: every fixture `FP=0 MISSED=0`, closures byte-identical to the OFF/oracle. **A single FP is a NO-GO** → revert the branch, record in the verdict doc, STOP.

- [ ] **Step 3: Branch/bjgap measurement (evidence)**

With `RUSTDL_PRECISE_MERGE_DEPS` OFF then ON, run `decide_pair_probe` on cardinality-heavy wine pairs (reuse the pattern from `crates/owl-dl-reasoner/tests/decide_pair_probe.rs` / `sat_guide_gate.rs`: a big-stack thread, adaptive-budget OFF, the 4 wine pairs `AlsatianWine⊓¬AmericanWine`, `SweetWine`, `Zinfandel`, `RedWine`, depth 256, 60s deadline). Record `branches_taken`/`restores`/wall OFF vs ON. A reduction is the architecture-working evidence; no reduction with FP=0 held = sound-but-inert (report honestly).

- [ ] **Step 4: Write the verdict doc**

`docs/precise-merge-deps-gate-results-2026-06-23.md`: the FP=0 corpus table (per fixture), the verdict-preservation/≠-decline/backjump-canary test results, the wine branch OFF-vs-ON measurement, and the GO/NO-GO call (GO = FP=0 corpus-wide AND a measurable branch improvement somewhere; sound-but-inert = FP=0 but no improvement → keep default OFF, bank as a sound-but-currently-inert precision improvement; NO-GO = any FP → reverted).

- [ ] **Step 5: On GO — flip default-ON**

If GO: change `hyper_precise_merge_deps_enabled` to default-ON (mirror `hyper_precise_card_deps_enabled`'s `is_none_or(|v| v != "0" && !v.is_empty())`), re-run `cargo test --workspace` + the corpus gate with the new default, confirm still FP=0, and commit. If sound-but-inert or NO-GO: leave default OFF (or revert).

- [ ] **Step 6: Commit the verdict (+ flip if GO)**

```bash
git add docs/precise-merge-deps-gate-results-2026-06-23.md crates/owl-dl-reasoner/src/lib.rs
git commit  # "docs(arch): precise-merge-deps gate verdict + (GO) default-ON flip" + trailers
```

---

## Self-Review

**1. Spec coverage:** decision level `d` + cause threading + accumulate/backjump/`combined.remove(d)` (Task 3) ✓; `cause = at_most_dep ∪ {d}` fold-not-taint (Task 2) ✓; ≠-provenance decline → `DepSet::ALL` (Tasks 2 are_neq + 3 `merge_precise_declined` fallback) ✓; `RUSTDL_PRECISE_MERGE_DEPS` gate default OFF + flag-OFF byte-identical (Task 1) ✓; `card_clash_deps` guards retained (untouched — no task modifies 838–866) ✓; verdict-preservation tests (Task 3) ✓; ≠-fallback test (Task 4) ✓; backjump-improvement canary (Task 5) ✓; FP=0 corpus gate + measurement + default-ON-on-success (Task 6) ✓.

**2. Placeholder scan:** Two implementer-verification points are flagged explicitly with the grep/read to resolve them (the `decide_with_stats` location in Task 5; the `≠`-reaching-`partition_rec` shape in Task 4) — these are "confirm the exact API/shape against the code," not unspecified logic. No "TBD"/"handle errors".

**3. Type consistency:** `precise_merge_deps: bool`, `merge_precise_declined: bool`, `with_precise_merge_deps`, `hyper_precise_merge_deps_enabled`, `solve_at_most(node, succs, n, depth)` (new `node` param threaded from the ~1755 call site), `partition_rec(.., cause, d, combined)` — consistent across Tasks 1–6. `cause = at_most_dep.insert(d)` and exhaustion `combined.remove(d)` use the same `DepSet` API the ⊔ rule uses (`insert`/`remove`/`union`/`contains`), verified against hyper.rs:1714/1750.
