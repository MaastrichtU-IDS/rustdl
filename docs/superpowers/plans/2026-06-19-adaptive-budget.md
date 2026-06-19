# Adaptive per-pair budget (Lever #1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Cut a *diverging* wedge search early (return `Stalled` once it's provably making no progress toward a model) instead of burning the full per-pair deadline — reclaiming the wall on the disjunctive-branching outliers (ore-15672 138s → target ~15–30s) with **identical verdicts** (FP=0, MISSED unchanged).

**Architecture:** A pure `is_diverging(...)` predicate over the wedge's existing `SearchStats` (branches/restores/max_branch_depth + node count), evaluated every `N` branches at the top of `HyperEngine::solve` (alongside the deadline check at `hyper.rs:1617`). When it fires → `return HyperResult::Stalled` — the same value the deadline returns, so the orchestrator records "not subsumed" exactly as before. Gated by `RUSTDL_ADAPTIVE_BUDGET`.

**Tech Stack:** Rust; `owl-dl-tableau/src/hyper.rs` (engine + `solve`); `owl-dl-reasoner/src/lib.rs` (wedge construction sites); `konclude_closure_diff` corpus net.

**Spec:** `docs/superpowers/specs/2026-06-19-adaptive-budget-design.md`

**Soundness law:** FP=0 is **structural** — an early cut only ever yields `Stalled` → "not subsumed" (a MISS at worst, never an invented subsumption). The *entire* risk is recall. The gate is **corpus MISSED unchanged** (byte-identical closures): if any closure shrinks, the predicate cut a real subsumption → retune (raise `N`/`θ`) or revert. Tune conservatively; loosen only while MISSED stays 0.

---

## Conventions

- Toolchain: `export PATH="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"`.
- Build: `cargo build --release -p owl-dl-cli`.
- Branch is `feat/perf-levers` (specs committed `dac0f67`). Do NOT touch main.

---

## Task 1: Divergence predicate + plumbing (flag OFF by default)

**Files:**
- Modify: `crates/owl-dl-tableau/src/hyper.rs` (predicate helper, engine fields, `with_adaptive_budget`, the `solve` hook)
- Test: `crates/owl-dl-tableau/src/hyper.rs` `#[cfg(test)]`

- [ ] **Step 1: Write the failing unit test for the pure predicate**

In `hyper.rs` tests module:
```rust
#[test]
fn is_diverging_fires_only_on_no_progress() {
    // window: 5000 branches, 4990 restores (≈all failing), depth saturated, model grew
    assert!(is_diverging(/*db*/ 5000, /*dr*/ 4990, /*depth_saturated*/ true, /*model_grew*/ true));
    // progressing: many branches succeeded (restores ≪ branches) → not diverging
    assert!(!is_diverging(5000, 1000, true, true));
    // depth not saturated (still room) → not diverging
    assert!(!is_diverging(5000, 4990, false, true));
    // model stabilized (no growth) → not diverging
    assert!(!is_diverging(5000, 4990, true, false));
}
```

- [ ] **Step 2: Run it; expect failure**

Run: `cargo test -p owl-dl-tableau is_diverging_fires_only_on_no_progress`
Expected: FAIL (`is_diverging` undefined).

- [ ] **Step 3: Implement the pure predicate**

Free function in `hyper.rs` (module-level):
```rust
/// Divergence predicate (Lever #1): a wedge search is making no progress toward a
/// satisfying completion when, over a window of `db` branches, ~all failed
/// (`dr`/`db` ≥ θ), the branch depth has saturated the cap, and the model is still
/// growing (∃-generation manufacturing successors, not stabilizing). Pure +
/// unit-tested so the threshold logic is testable in isolation. θ = 0.98.
fn is_diverging(db: u64, dr: u64, depth_saturated: bool, model_grew: bool) -> bool {
    depth_saturated && model_grew && db > 0 && dr.saturating_mul(100) >= db.saturating_mul(98)
}
```

- [ ] **Step 4: Run unit test; expect pass**

Run: `cargo test -p owl-dl-tableau is_diverging_fires_only_on_no_progress` → PASS.

- [ ] **Step 5: Add engine fields + `with_adaptive_budget`**

Add to `struct HyperEngine`: `adaptive_budget: bool,` and `div_checkpoint: (u64, u64, usize),` (last `(branches_taken, restores, nodes.len())` snapshot). Initialize `adaptive_budget: false, div_checkpoint: (0, 0, 0),` in **every** constructor (`new`, `new_with_prebuilt`, `new_seeded`, and any `from_snapshot*`). Add:
```rust
/// Opt into adaptive early-cut of diverging searches (Lever #1). Off by default
/// (preserves existing deadline-only behavior + test calibration).
#[must_use]
pub fn with_adaptive_budget(mut self) -> Self {
    self.adaptive_budget = true;
    self
}
```

- [ ] **Step 6: Hook the check into `solve`**

In `fn solve(&mut self, depth: usize)` (`hyper.rs:1616`), immediately AFTER the existing deadline check (the `if let Some(dl) = self.deadline && Instant::now() >= dl { return HyperResult::Stalled; }` at ~1617-1620), add:
```rust
        if self.adaptive_budget {
            const DIV_WINDOW: u64 = 5_000;
            let (b0, r0, n0) = self.div_checkpoint;
            let db = self.stats.branches_taken.saturating_sub(b0);
            if db >= DIV_WINDOW {
                let dr = self.stats.restores.saturating_sub(r0);
                // depth saturated: the deepest branch reached the recursion cap.
                // `init_depth` is the `max_depth` passed to decide_with_deadline
                // (HYPER_WEDGE_DEPTH); `max_branch_depth` is the deepest level hit.
                // CONFIRM the comparison matches how `max_branch_depth` is recorded
                // at hyper.rs:1712 (it stores `level`); depth_saturated should mean
                // "reached the cap". Adjust the comparison to the real semantics.
                let depth_saturated = self.stats.max_branch_depth as usize >= self.init_depth;
                let model_grew = self.nodes.len() > n0;
                if is_diverging(db, dr, depth_saturated, model_grew) {
                    return HyperResult::Stalled;
                }
                self.div_checkpoint =
                    (self.stats.branches_taken, self.stats.restores, self.nodes.len());
            }
        }
```
IMPORTANT (open question §6.2 of the spec): verify `max_branch_depth` is live-updated during the search (it is — set at `hyper.rs:1712` on each deepening) and that `>= self.init_depth` correctly means "hit the cap". If `level` counts DOWN (remaining depth) rather than up, invert (`<= 0` / `<= small`). Read lines around 1700-1715 and 1616 to confirm the depth convention before finalizing; the predicate must mean "search went as deep as allowed and is still failing".

- [ ] **Step 7: Build + existing suite green (flag off → no behavior change)**

Run: `cargo build --release -p owl-dl-cli && cargo test -p owl-dl-tableau`
Expected: all pass (adaptive_budget is false everywhere → solve unchanged). Clippy `-p owl-dl-tableau --all-targets -- -D warnings` clean; fmt.

- [ ] **Step 8: Commit**

```sh
git add crates/owl-dl-tableau/src/hyper.rs
git commit -m "feat(wedge): divergence predicate + adaptive-budget plumbing (Lever #1, flag off)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: Wire into the reasoner wedge paths (env-gated) + early-cut behavior test

**Files:**
- Modify: `crates/owl-dl-reasoner/src/lib.rs` (flag helper + `.with_adaptive_budget()` on wedge engines)
- Test: `crates/owl-dl-reasoner/tests/adaptive_budget.rs` (new)

- [ ] **Step 1: Add the flag helper**

In `crates/owl-dl-reasoner/src/lib.rs`, mirroring the sibling flag helpers (`label_heuristic_enabled` etc.):
```rust
/// Lever #1: adaptive early-cut of diverging wedge searches. Default OFF until the
/// corpus MISSED-unchanged gate confirms it (then flip to default-ON — strictly
/// faster, identical verdicts). Set `RUSTDL_ADAPTIVE_BUDGET=1`.
pub(crate) fn adaptive_budget_enabled() -> bool {
    std::env::var("RUSTDL_ADAPTIVE_BUDGET").map_or(false, |v| v == "1")
}
```

- [ ] **Step 2: Apply it to every wedge-engine construction**

In the wedge paths that run per-pair / per-class searches, chain `.with_adaptive_budget()` when the flag is on. Sites (search for `decide_with_deadline` callers + `HyperEngine::new`): `HyperCache::decide`, `HyperCache::classify_labels`, the `ConsistencyCache` engine (~lib.rs:1334), and the per-class unsat-probe (`HyperEngine::new` at ~lib.rs:194). Pattern:
```rust
        if crate::adaptive_budget_enabled() {
            engine = engine.with_adaptive_budget();
        }
```
(Place it next to the existing `with_double_blocking()`/`with_precise_card_deps()` conditional chains.)

- [ ] **Step 3: Early-cut behavior test**

Create `crates/owl-dl-reasoner/tests/adaptive_budget.rs`. With `RUSTDL_ADAPTIVE_BUDGET=1` (use the `ENV_MUTEX`/`SetEnvGuard` serialization pattern from `inverse_symmetric_domain.rs`), classify a small ontology with a known-diverging satisfiable class and assert (a) the verdict is **unchanged** vs flag-off, and (b) it completes. Simplest robust assertion: classify `ontologies/external/ore-15672-shoin.ofn` (the real diverging case) with the flag on and a generous global deadline, assert the closure equals the flag-off closure (same 142 subsumptions). If the full fixture is too slow for a unit test, mark `#[ignore]` and rely on the corpus gate (Task 3) for behavior verification; prefer a tiny synthetic if one can reproduce divergence cheaply.

- [ ] **Step 4: Build + tests**

Run: `cargo build --release -p owl-dl-cli && cargo test -p owl-dl-reasoner --test adaptive_budget && cargo clippy -p owl-dl-reasoner --all-targets -- -D warnings && cargo fmt --all`
Expected: pass; clean.

- [ ] **Step 5: Commit**

```sh
git add crates/owl-dl-reasoner/src/lib.rs crates/owl-dl-reasoner/tests/adaptive_budget.rs
git commit -m "feat(wedge): wire adaptive-budget into reasoner wedge paths (RUSTDL_ADAPTIVE_BUDGET, off)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: Corpus MISSED gate + wall + tune (the sacred gate)

**Files:** none (measurement + possible threshold tweak in `hyper.rs`).

- [ ] **Step 1: Wall with flag ON vs OFF**

```sh
export PATH="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"
echo -n "ore-15672 OFF: "; { /usr/bin/time -v ./target/release/rustdl classify ontologies/external/ore-15672-shoin.ofn >/dev/null ; } 2>&1 | grep -oE 'Elapsed.*: [0-9:.]+' | grep -oE '[0-9:.]+$'
echo -n "ore-15672 ON:  "; { RUSTDL_ADAPTIVE_BUDGET=1 /usr/bin/time -v ./target/release/rustdl classify ontologies/external/ore-15672-shoin.ofn >/dev/null ; } 2>&1 | grep -oE 'Elapsed.*: [0-9:.]+' | grep -oE '[0-9:.]+$'
```
Expected: OFF ≈138s, ON materially lower (target ~15–30s). Record both.

- [ ] **Step 2: Corpus closure-IDENTITY net with flag ON (sacred)**

```sh
RUSTDL_ADAPTIVE_BUDGET=1 cargo test --release -p owl-dl-reasoner --test konclude_closure_diff -- --include-ignored --nocapture 2>&1 | grep -iE 'rustdl_closure=|FP=|MISSED=|test result'
```
Expected: every `*_closure_matches_*` fixture **FP=0 AND closure byte-identical to baseline** (galen 27997, notgalen 32739, sio 8904, wine 653, ore-10908 6001, ore-15672 142, alehif 247, ro 158, pizza 499, bibtex 16). `family_inconsistency_detected` failing under `--include-ignored` is expected. **If ANY closure shrinks → the predicate cut a real subsumption → STOP, raise `DIV_WINDOW` (e.g. 20_000) and/or θ, re-run.** FP=0 holds structurally regardless.

- [ ] **Step 3: Tune if needed; record final thresholds**

If a closure shrank, raise `DIV_WINDOW`/θ (more conservative) until MISSED=0, re-measuring the wall (more conservative = less wall gain). Record the final `(DIV_WINDOW, θ)` + the ore-15672/wine wall in Results. If MISSED=0 holds at the starting (5000, 0.98), keep them.

---

## Task 4: Flip default + docs

- [ ] **Step 1: Flip default per the gate**

If Task 3 shows MISSED-unchanged + wall-drop, flip `adaptive_budget_enabled()` to **default ON** (`map_or(true, |v| v != "0")`) — it's strictly better (faster, identical verdicts). If borderline, leave default OFF (opt-in) and document. Re-run the corpus net at the new default to confirm.

- [ ] **Step 2: Full suite + CLAUDE.md + Results**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo fmt --all -- --check` (green/clean). Add a CLAUDE.md entry (`owl-dl-tableau`/`owl-dl-reasoner`): wedge now early-cuts diverging searches (`is_diverging` predicate, gated `RUSTDL_ADAPTIVE_BUDGET`), ore-15672 `<baseline>→<new>`, closures byte-identical (FP=0, MISSED unchanged); reference spec + plan. Fill Results.

- [ ] **Step 3: Commit**

```sh
git add -A
git commit -m "feat(wedge): adaptive-budget default <ON|opt-in> — ore-15672 <baseline>→<new>, MISSED unchanged

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Results (2026-06-19) — SHIPPED, default ON

**Final tuning:** `DIV_WINDOW = 500`, `θ = 0.98`. The `model_grew` clause was **dropped**
during Task 1/3 — the #2 reuse probe showed the divergence is *thrashing through a tiny
state set at stable node count*, not growth, so a growth clause never fired. Predicate:
`depth_saturated && restores≈branches (≥98%)` over a 500-branch window; the window size is
the discriminator vs a converging Unsat proof (real proofs terminate within 500 branches;
only a search still all-failing-at-cap after 500 is cut).

**Wall:** ore-15672 **138s → 91s (~34%)**, verdict-preserving. (N=5000 gave zero gain — the
window matched the per-pair branch budget, firing only at the deadline; N=500 cuts early.)

**Soundness gate: PASS.** Corpus closure net at `RUSTDL_ADAPTIVE_BUDGET=1`, N=500:
**FP=0, MISSED=0, every closure byte-identical** to baseline (galen 27997, notgalen 32739,
sio 8904, wine 653, ore-10908 6001, ore-15672 142, alehif 247, ro 158, pizza 499, bibtex 16;
net wall 454s ≤ baseline). No real subsumption proof is cut — they all complete within 500
branches. FP=0 is structural (early-cut only yields `Stalled`/"not subsumed").

**Default ON** (`adaptive_budget_enabled` → `map_or(true, v != "0")`): strictly
verdict-preserving + faster, so no reason for opt-in; `RUSTDL_ADAPTIVE_BUDGET=0` reverts.
Full workspace suite green; clippy/fmt clean.

**Headroom (future tuning knob):** the gain is modest (34%) because many hard pairs reach
`depth_saturated` only after >500 branches, so they're cut later than ~100ms. Lower
`DIV_WINDOW` and/or a relaxed depth threshold would gain more — each step gated by a fresh
corpus MISSED net (the convergence-risk curve). N=500 is the validated safe point shipped now.
