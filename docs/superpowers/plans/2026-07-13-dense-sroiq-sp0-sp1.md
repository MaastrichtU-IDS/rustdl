# Dense-SROIQ Tractability — SP0 + SP1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Measure why the hypertableau wedge stalls on dense-SROIQ classes (SP0), then make its per-branch cost incremental so it decides `ore_ont_10019` within budget instead of re-saturating the whole graph on every branch (SP1).

**Architecture:** SP0 exposes the *already-collected* blocking counters (`SearchStats.blocks_fired/block_eligible/is_blocked_calls`) per class in the `hyper-sat` probe and adds inverse/nominal/`=n` feature detection — a findings note, no behavior change. SP1 adds an env-gated (default-OFF) incremental mode to `HyperEngine::horn_fixpoint`: instead of clearing + re-seeding every node's labels/edges at the top of every `solve` frame (`hyper.rs:1532`), it drains only the worklist delta the branch decision added, with the worklist snapshotted in `save`/`restore` so a backtrack rolls it back. Correctness is guarded by a differential verdict-identity harness (flag OFF vs ON must produce byte-identical closures).

**Tech Stack:** Rust (workspace crates `owl-dl-tableau`, `owl-dl-reasoner`, `owl-dl-cli` bin `rustdl`, `owl-dl-bench`). horned-owl. rayon. Tests via `cargo test`.

## Global Constraints

- Toolchain: **always** prefix cargo with `RUSTUP_TOOLCHAIN=stable` (the pinned 1.95.0 lacks cargo). Rebuild BOTH `-p owl-dl-cli -p owl-dl-bench` before any CLI/matrix run.
- Soundness invariant: **FP = 0 must never regress**, verified on the curated matrix AND a non-Horn adversarial FP oracle (`ore_ont_13723`-class vs Konclude). (SP0/SP1 are verdict-preserving so this is a regression guard, not a new risk — but it is still a required gate.)
- Completeness invariant: **MISSED = 0 / byte-identical curated closures.** This is the load-bearing gate for SP1 — its only real failure mode (a dropped clause-firing on incremental restore) is a silent MISS, not an FP.
- Every shipped behavior change lands behind a default-OFF env flag first, is validated FP=0 AND MISSED=0 + no curated wall regression, then flipped default-ON in a **separate reviewed commit**.
- `clippy -D warnings` and `cargo fmt --check` clean on every commit.
- Branch: do this work on a new branch off `main` (e.g. `feat/wedge-incremental-fixpoint`); do not pile onto `feat/matrix-ore-tier`.

## File Structure

- `crates/owl-dl-tableau/src/hyper.rs` — engine. SP1 modifies: `Snapshot` (312), `SearchStats` (461, SP0 counters already present), `HyperEngine` fields (~520), builders (~1010), constructors (~931/981/2047), `horn_fixpoint` (1526), `save`/`restore` (2317/2328). SP0 reads `is_blocked` (1436) counters.
- `crates/owl-dl-reasoner/src/lib.rs` — flag fns + probe. SP1 adds `incremental_fixpoint_enabled()` (beside `adaptive_budget_enabled`, ~1647) and applies `with_incremental_fixpoint()` at the 4 `HyperEngine::new` sites (685, 1077, 1216, 1372). SP0 extends `hyper_sat_probe` (~675) output.
- `crates/owl-dl-tableau/tests/incremental_fixpoint_identity.rs` — **create**: SP1 differential harness.
- `docs/known-limitations/` or `docs/` — SP0 findings note (create).

---

## SP0 — Measurement probe

### Task 0.1: Expose blocking counters + feature detection in `hyper-sat`, run, write findings

**Files:**
- Modify: `crates/owl-dl-reasoner/src/lib.rs` — `hyper_sat_probe` (~675) output block.
- Modify: `crates/owl-dl-tableau/src/hyper.rs:1436` `is_blocked` — ONLY if the counters are not already incremented (verify first).
- Create: `docs/2026-07-13-ore_ont_10019-stall-findings.md`.

**Interfaces:**
- Consumes: `SearchStats { blocks_fired, block_eligible, is_blocked_calls, block_compares }` (already defined, `hyper.rs:461-500`).
- Produces: a findings note (no code interface).

- [ ] **Step 1: Verify the blocking counters are actually incremented.**

Run: `grep -n 'blocks_fired\|block_eligible\|is_blocked_calls' crates/owl-dl-tableau/src/hyper.rs`
Expected: increments inside `is_blocked` (near line 1436). If any counter has only its struct definition and no `+= 1`, add the increment in `is_blocked`: `self.stats.is_blocked_calls += 1;` at entry; `self.stats.block_eligible += 1;` when the node has a parent; `self.stats.blocks_fired += 1;` immediately before each `return true`. If already incremented, no code change — proceed.

- [ ] **Step 2: Add per-class blocking + feature columns to the probe output.**

In `hyper_sat_probe` (`lib.rs`, the per-class stat line and the summary), extend the existing `# --- top classes by branching ---` line format to also print `blk_fired=<n>/<eligible>`. In the summary header add:
```rust
// after the existing total_branches / match_attempts lines:
eprintln!("# total_is_blocked_calls: {}", agg.is_blocked_calls);
eprintln!("# total_blocks_fired:    {}", agg.blocks_fired);
eprintln!("# total_block_eligible:  {}", agg.block_eligible);
```
and on each top-class line append `blk={}/{}`, `stats.blocks_fired`, `stats.block_eligible`. (Match the existing `eprintln!` style in that function; do not restructure it.)

- [ ] **Step 3: Add ontology feature detection to the probe.**

In `hyper_sat_probe`, after loading the ontology, scan the axioms once and print:
```rust
let mut has_inverse = false; let mut has_nominal = false; let mut has_card = false;
for c in onto.iter().map(|ac| &ac.component) {
    let s = format!("{c:?}");
    if s.contains("InverseObjectProperties") || s.contains("ObjectInverseOf") { has_inverse = true; }
    if s.contains("ObjectOneOf") || s.contains("ObjectHasValue") { has_nominal = true; }
    if s.contains("ObjectMinCardinality") || s.contains("ObjectMaxCardinality") || s.contains("ObjectExactCardinality") { has_card = true; }
}
eprintln!("# features: inverse={has_inverse} nominal={has_nominal} card={has_card}");
```
(This is a coarse syntactic scan — adequate for the SP2-soundness gate question "are inverse/nominal present?". Do not over-engineer.)

- [ ] **Step 4: Build and run on ore_ont_10019.**

Run:
```bash
RUSTUP_TOOLCHAIN=stable cargo build --release -p owl-dl-cli
./target/release/rustdl hyper-sat ~/data/ore-run/input/ore_ont_10019.ofn --per-class-timeout-ms 300 2>&1 | tail -40
```
Expected: per-class lines now show `blk=<fired>/<eligible>`; summary shows `total_blocks_fired`, `total_block_eligible`, and `features: inverse=? nominal=? card=?`.

- [ ] **Step 5: Write the findings note.**

Create `docs/2026-07-13-ore_ont_10019-stall-findings.md` recording: (a) `blocks_fired` vs `block_eligible` on the depth-75 stalled classes — **is blocking firing on the successor recursion or not?**; (b) the `features` line (inverse/nominal/card presence — this decides whether SP2's pure-label-set no-goods are admissible); (c) a one-paragraph verdict: does the evidence point to SP1 (per-branch cost) as the primary lever, a blocking fix (SP2a), or both. Include the raw probe output.

- [ ] **Step 6: Commit.**

```bash
git add crates/owl-dl-reasoner/src/lib.rs crates/owl-dl-tableau/src/hyper.rs docs/2026-07-13-ore_ont_10019-stall-findings.md
git commit -m "diag(hyper): expose blocking counters + feature detection in hyper-sat (SP0)"
```

---

## SP1 — Incremental horn_fixpoint across save/restore

### Task 1.1: Add the default-OFF `incremental_fixpoint` flag (no behavior yet)

**Files:**
- Modify: `crates/owl-dl-tableau/src/hyper.rs` — `HyperEngine` field (~570, beside `double_blocking`), constructors (931, 981, 2047), builder (~1010 beside `with_double_blocking`).
- Modify: `crates/owl-dl-reasoner/src/lib.rs` — env fn beside `adaptive_budget_enabled` (~1647), applied at the 4 `HyperEngine::new` sites (685, 1077, 1216, 1372).

**Interfaces:**
- Produces: `HyperEngine::with_incremental_fixpoint(self) -> Self`; field `incremental_fixpoint: bool`; reasoner fn `incremental_fixpoint_enabled() -> bool` reading `RUSTDL_HYPER_INCREMENTAL_FIXPOINT` (default OFF: `is_some_and(|v| v != "0" && !v.is_empty())`).

- [ ] **Step 1: Add the field + builder (mirror `double_blocking`).**

In `HyperEngine` struct add `incremental_fixpoint: bool,` next to `double_blocking: bool` (~570). In each constructor (931, 981, 2047) add `incremental_fixpoint: false,`. Add:
```rust
/// Enable incremental horn_fixpoint (SP1): process only the per-branch
/// worklist delta instead of re-seeding the whole graph each solve frame.
pub fn with_incremental_fixpoint(mut self) -> Self {
    self.incremental_fixpoint = true;
    self
}
```
next to `with_double_blocking` (~1010).

- [ ] **Step 2: Add the env flag fn in the reasoner (mirror `adaptive_budget_enabled`).**

```rust
/// SP1: opt-in incremental horn_fixpoint. Default OFF until validated
/// FP=0 AND MISSED=0. Disable/enable with `RUSTDL_HYPER_INCREMENTAL_FIXPOINT`.
pub fn incremental_fixpoint_enabled() -> bool {
    std::env::var_os("RUSTDL_HYPER_INCREMENTAL_FIXPOINT").is_some_and(|v| v != "0" && !v.is_empty())
}
```

- [ ] **Step 3: Apply it at the 4 engine-build sites.**

At each `HyperEngine::new(...)` in `lib.rs` (685, 1077, 1216, 1372), after the existing `with_double_blocking`/`with_adaptive_budget` conditionals add:
```rust
if crate::incremental_fixpoint_enabled() { engine = engine.with_incremental_fixpoint(); }
```
(At 685 the binding is `let mut engine`; ensure it is `mut`.)

- [ ] **Step 4: Build; flag is inert.**

Run: `RUSTUP_TOOLCHAIN=stable cargo build -p owl-dl-tableau -p owl-dl-reasoner 2>&1 | tail -2`
Expected: compiles; `incremental_fixpoint` is read nowhere yet (allow a `dead_code`-free build by using it in a trivial `debug_assert!` or leaving it for Task 1.4 — prefer wiring Task 1.4 immediately after so it is used).

- [ ] **Step 5: Commit.**

```bash
git add crates/owl-dl-tableau/src/hyper.rs crates/owl-dl-reasoner/src/lib.rs
git commit -m "feat(hyper): add default-OFF incremental_fixpoint flag (SP1 scaffold)"
```

### Task 1.2: Differential verdict-identity harness (the SP1 gate)

**Files:**
- Create: `crates/owl-dl-tableau/tests/incremental_fixpoint_identity.rs`.

**Interfaces:**
- Consumes: `RUSTDL_HYPER_INCREMENTAL_FIXPOINT` env flag; the `rustdl classify` CLI.
- Produces: a test that fails if flag-ON verdicts differ from flag-OFF.

- [ ] **Step 1: Write the differential test (integration-level, over the CLI).**

```rust
//! SP1 gate: classify with incremental_fixpoint OFF vs ON must produce
//! byte-identical hierarchies on every fixture. A difference means the
//! incremental drain dropped or double-fired a clause (a MISS or FP).
use std::process::Command;

fn classify(ofn: &str, incremental: bool) -> String {
    let bin = env!("CARGO_BIN_EXE_rustdl", "build rustdl first");
    let mut c = Command::new(bin);
    c.arg("classify").arg(ofn).arg("--pair-timeout-ms").arg("1000");
    c.env("RUSTDL_HYPER_INCREMENTAL_FIXPOINT", if incremental { "1" } else { "0" });
    let out = c.output().expect("run rustdl");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn incremental_matches_baseline_on_fixtures() {
    // Small, checked-in SROIQ fixtures that exercise disjunction + ≤n.
    for ofn in ["ontologies/regression/funcmerge-cyclic.ofn"] {
        if !std::path::Path::new(ofn).exists() { continue; }
        assert_eq!(classify(ofn, false), classify(ofn, true), "mismatch on {ofn}");
    }
}
```
Note: `CARGO_BIN_EXE_rustdl` only resolves if the test crate can see the `rustdl` bin; if `owl-dl-tableau` cannot, place this test in `crates/owl-dl-cli/tests/` instead and adjust the path. Verify which crate owns the `rustdl` bin (`owl-dl-cli/Cargo.toml`) and put the test there.

- [ ] **Step 2: Run it (flag still inert ⇒ trivially passes).**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-cli --test incremental_fixpoint_identity 2>&1 | tail -5`
Expected: PASS (flag does nothing yet — establishes the harness compiles and both runs agree).

- [ ] **Step 3: Commit.**

```bash
git add crates/owl-dl-cli/tests/incremental_fixpoint_identity.rs
git commit -m "test(hyper): SP1 differential verdict-identity harness"
```

### Task 1.3: Snapshot the worklist in save/restore

**Files:**
- Modify: `crates/owl-dl-tableau/src/hyper.rs` — `Snapshot` (312), `save` (2317), `restore` (2328).

**Interfaces:**
- Consumes: `HyperEngine.worklist: Vec<Event>` (544), `incremental_fixpoint` flag.
- Produces: worklist restored on backtrack when the flag is ON.

- [ ] **Step 1: Add `worklist` to `Snapshot`.**

In `struct Snapshot` (312) add field `worklist: Vec<Event>,`.

- [ ] **Step 2: Save/restore it only under the flag (keep OFF path allocation-free).**

In `save` (2317):
```rust
worklist: if self.incremental_fixpoint { self.worklist.clone() } else { Vec::new() },
```
In `restore` (2328), before `self.stats.restores += 1;`:
```rust
if self.incremental_fixpoint { self.worklist = saved.worklist; }
```
(When OFF, the field is an empty `Vec` and is never read — `horn_fixpoint` clears it anyway.)

- [ ] **Step 3: Build + run the harness (still identical — nothing drains incrementally yet).**

Run: `RUSTUP_TOOLCHAIN=stable cargo build -p owl-dl-tableau && RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-cli --test incremental_fixpoint_identity 2>&1 | tail -3`
Expected: PASS.

- [ ] **Step 4: Commit.**

```bash
git add crates/owl-dl-tableau/src/hyper.rs
git commit -m "feat(hyper): snapshot worklist in save/restore under incremental flag (SP1)"
```

### Task 1.4: Make horn_fixpoint incremental under the flag

**Files:**
- Modify: `crates/owl-dl-tableau/src/hyper.rs` — `horn_fixpoint` (1526-1567).

**Interfaces:**
- Consumes: the worklist populated by `add_label`/`add_edge`/merge (which already `worklist.push(...)`, e.g. 1331/1426/2925/3378/3418/3514) and the parent's saturated state carried via `save`/`restore` (Task 1.3).
- Produces: incremental drain — a `solve` frame processes only the delta its decision added.

- [ ] **Step 1: Guard the clear + re-seed behind `!incremental_fixpoint`.**

Change the top of `horn_fixpoint` (1528-1567) so the whole `self.worklist.clear(); for idx in 0..nodes { … re-seed … }` block runs **only** when `!self.incremental_fixpoint`. In incremental mode, do NOT clear and do NOT re-seed — drain whatever is already in `self.worklist` (the delta the branch's `apply_head_atom`/merge pushed). The drain loop (`while let Some(ev) = self.worklist.pop()`, 1569-1577) is unchanged.

Concretely:
```rust
fn horn_fixpoint(&mut self, max_iters: usize) -> HyperResult {
    self.stats.fixpoint_passes += 1;
    if !self.incremental_fixpoint {
        self.worklist.clear();
        // ... existing re-seed loop (1533-1567) unchanged ...
    }
    // else: incremental — drain the delta already on the worklist.
    let mut steps = 0usize;
    while let Some(ev) = self.worklist.pop() { /* unchanged 1569-1577 */ }
    HyperResult::Sat
}
```

- [ ] **Step 2: Seed the worklist once at the root (incremental mode).**

In incremental mode the very first `horn_fixpoint` call must see the initial graph on the worklist. `decide_with_deadline` (1730) seeds the root query before calling `solve`; confirm the root labels/edges are pushed as events (via the `add_label`/`add_edge` used during query construction — grep the constructor path). If the root graph is built by direct field writes that bypass `worklist.push`, add a one-time seed in `decide_with_deadline` under the flag: iterate nodes and push `NodeNew`/`Label`/`Edge` exactly like the OFF re-seed, ONCE, before the first `solve`.

Run: `grep -n 'fn decide_with_deadline\|add_label\|add_edge\|worklist.push' crates/owl-dl-tableau/src/hyper.rs | sed -n '1,20p'` to confirm the seeding path.

- [ ] **Step 3: Run the differential harness with the flag ON.**

Run:
```bash
RUSTUP_TOOLCHAIN=stable cargo build -p owl-dl-cli
RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-cli --test incremental_fixpoint_identity 2>&1 | tail -5
```
Expected: PASS (byte-identical). **If it FAILS**, the incremental drain dropped or double-fired an event — a mutation path that does not push its event, or a restore that leaves a stale/missing event. Debug by diffing the two closures and tracing which subsumption differs; fix the missing `worklist.push` (or the restore) until identical. This iterate-to-identical loop IS the task — do not proceed until PASS.

- [ ] **Step 4: Measure the match-attempt drop on ore_ont_10019.**

Run both and compare `match_attempts`:
```bash
for f in 0 1; do echo "incremental=$f:"; RUSTDL_HYPER_INCREMENTAL_FIXPOINT=$f ./target/release/rustdl hyper-sat ~/data/ore-run/input/ore_ont_10019.ofn --per-class-timeout-ms 300 2>&1 | grep -E 'match_attempts|stalled|sat:'; done
```
Expected: incremental=1 shows materially fewer `match_attempts` and (hopefully) fewer `stalled`. Record the numbers in the SP0 findings note (append an SP1 result section).

- [ ] **Step 5: Commit.**

```bash
git add crates/owl-dl-tableau/src/hyper.rs
git commit -m "feat(hyper): incremental horn_fixpoint drains per-branch delta under flag (SP1)"
```

### Task 1.5: Corpus gate + measure ore_ont_10019 classify + decide default

**Files:**
- Modify (if flipping default-on): `crates/owl-dl-reasoner/src/lib.rs` `incremental_fixpoint_enabled`.

- [ ] **Step 1: Curated matrix FP=0 AND MISSED=0 with the flag ON.**

Run:
```bash
RUSTUP_TOOLCHAIN=stable cargo build --release -p owl-dl-cli -p owl-dl-bench
RUSTDL_HYPER_INCREMENTAL_FIXPOINT=1 ./target/release/owl-dl-bench matrix --tier curated --out /tmp/m-inc --pair-timeout-ms 1000 --global-timeout-s 120
# then inspect: rustdl rows must be FP=0 and MISSED=0
grep -o '"reasoner":"rustdl"[^}]*' /tmp/m-inc/results.jsonl | grep -oE '"fp":[0-9]+|"missed":[0-9]+' | sort | uniq -c
```
Expected: every rustdl `fp` and `missed` is `0`. **If any MISSED>0, STOP** — the incremental path is dropping a subsumption; return to Task 1.4 Step 3.

- [ ] **Step 2: Non-Horn adversarial FP oracle.**

Run `classify` with the flag ON on the known FP regressor(s) (`ore_ont_13723` if fetched) and diff against the Konclude oracle; assert FP=0. Record result.

- [ ] **Step 3: Measure ore_ont_10019 classify (the headline).**

```bash
gtimeout -s KILL 120 env RUSTDL_HYPER_INCREMENTAL_FIXPOINT=1 RUSTDL_AGGREGATE_DEADLINE_MS=60000 \
  ./target/release/rustdl classify ~/data/ore-run/input/ore_ont_10019.ofn --pair-timeout-ms 250 2>&1 | grep -iE 'incomplete|real' ; echo exit=$?
```
Record: did the incomplete-pair count drop vs the baseline (1643 at 60s)? Did any classes newly decide? Append to findings.

- [ ] **Step 4: Decide default.** If Step 1+2 are clean and there is a wall/completeness win with no curated regression, flip `incremental_fixpoint_enabled` default-ON in a **separate** commit:
```rust
// default ON:
std::env::var_os("RUSTDL_HYPER_INCREMENTAL_FIXPOINT").is_none_or(|v| v != "0" && !v.is_empty())
```
and re-run the curated matrix default (no env) to confirm FP=0/MISSED=0. If the win is marginal or any gate fails, leave default-OFF and record why.

- [ ] **Step 5: Commit (separate from the impl).**

```bash
git add crates/owl-dl-reasoner/src/lib.rs docs/2026-07-13-ore_ont_10019-stall-findings.md
git commit -m "feat(hyper): incremental_fixpoint default-ON (FP=0/MISSED=0 verified) [or: keep OFF, record result]"
```

---

## SP2 (DEFERRED)

Not planned here. SP0's findings (blocking hit-rate + inverse/nominal presence) decide whether SP2 pursues (2a) stronger blocking or (2b) sound UNSAT memoization, and whether pure-label-set no-goods are even admissible. Write the SP2 plan only after SP0 lands.

## Self-review notes

- Spec coverage: SP0 (roadmap §SP0) → Task 0.1. SP1 (roadmap §SP1) → Tasks 1.1-1.5. Gates FP=0 AND MISSED=0 + non-Horn oracle → Task 1.5 Steps 1-2. Default-OFF-then-flip → Tasks 1.1 + 1.5 Step 4. SP2 deferred per spec.
- Risk owned: Task 1.4 Step 3 is the iterate-to-byte-identical loop; the plan states it explicitly rather than assuming first-try correctness.
- Open confirs the implementer must resolve in-task (flagged inline, not placeholders): which crate owns the `rustdl` bin for the test (Task 1.2 Step 1); whether root seeding bypasses `worklist.push` (Task 1.4 Step 2); whether blocking counters are already incremented (Task 0.1 Step 1).
