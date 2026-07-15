# Wedge Backjump-Precision — Phase 1 Probe — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Measure whether `ore_ont_10019`'s disjunctive-DFS stall (diagnosis: H2, established) is driven by **backjump degradation** — the real `clash_deps` collapsing to `DepSet::ALL` where a precise dep-set exists — so the fix direction (Fix #1 backjump-repair vs Fix #2 absorption / bound-tail) is chosen from data, not guessed.

**Architecture:** Reuse the shipped, read-only `RUSTDL_SHADOW_DEP_PROBE` (already wired into the classify path) which records per-clash `ClashRecord { branch_depth, real: DepSetSnapshot, shadow: DepSetSnapshot }`, and `shadow_measures::analyze` which already computes `bjgap_real`/`bjgap_shadow` histograms. Add only a `HydroxylGroup`-focused harness that runs it on `ore_ont_10019`'s stalled classes and reports the real-vs-shadow backjump gap + real-`ALL` frequency. No engine change. Deliverable: a findings note + the selected Phase-2 fix direction.

**Tech Stack:** Rust (`owl-dl-tableau`, `owl-dl-reasoner`, bin `rustdl`). Tests via `cargo test`.

## Global Constraints

- Toolchain: **always** prefix cargo with `RUSTUP_TOOLCHAIN=stable`. Rebuild `-p owl-dl-cli` before any probe run (stale-binary trap).
- Branch: `feat/wedge-backjump-precision` (already off `main`; spec committed). Do NOT work on `main`.
- **No engine/behavior change in this phase** — it is a read-only measurement (the probe already exists and is verdict-neutral by construction). Any new code is a test harness + a small `shadow_measures` reporting helper at most.
- `clippy -D warnings` + `cargo fmt --all -- --check` clean on every commit. Test modules may carry `#![allow(clippy::unwrap_used)]` per repo precedent.
- This is Phase 1 only. **Phase 2 (the fix) is a separate spec+plan written after this probe selects the direction.** Do NOT implement a fix here.

## What already exists (reuse — do NOT rebuild)

- `RUSTDL_SHADOW_DEP_PROBE` / `hyper_shadow_dep_probe_enabled()` wired into the classify path (`reasoner/src/lib.rs:2607` `decide_with_stats`, `:2670` `classify_labels`); populates `SearchStats.clash_records`.
- `ClashRecord { branch_depth: u32, real: DepSetSnapshot, shadow: DepSetSnapshot, clash_label_key: u64 }` (`hyper.rs:~435`). `DepSetSnapshot { highest: Option<u32>, count: u32, levels: Vec<u32> }` — **`ALL`/overflow ⇔ `highest == Some(127) && count == 0`**.
- `shadow_measures::analyze(&[ClashRecord]) -> ShadowReport { n_clashes, bjgap_real, bjgap_shadow, reusable_nogood_frac, distinct_nogoods, revisit_frac, revisit_context_shared_frac }`; `bjgap = branch_depth - highest + 1` (1 = useless/chronological; large = deep jump).
- Harness template: `crates/owl-dl-reasoner/tests/shadow_dep_gate.rs` (its `print_report` already computes `real_all` = `real.highest==Some(127) && real.count==0` and `shadow_differs`, and prints `bjgap_real` vs `bjgap_shadow`). Uses `sat_class_probe` (`lib.rs:1439`).
- Data: `~/data/ore-run/input/ore_ont_10019.ofn`. Stalled classes (SP0): `HydroxylGroup`, `EtherGroup`, `SulfoxideGroup`, `OxygenAtom`, `SulfonicAcidGroup`, `EsterGroup`, … (namespace `http://ontology.dumontierlab.com/`).

---

### Task 1: HydroxylGroup backjump-precision probe + findings + fix selection

**Files:**
- Create: `crates/owl-dl-reasoner/tests/backjump_precision_gate.rs` (model on `shadow_dep_gate.rs`).
- Create: `docs/2026-07-15-backjump-precision-findings.md`.
- (Optional, only if `analyze` lacks a needed aggregate) Modify: `crates/owl-dl-tableau/src/shadow_measures.rs`.

**Interfaces:**
- Consumes: `owl_dl_reasoner::sat_class_probe`, `owl_dl_tableau::shadow_measures::{analyze, ShadowReport}`, `owl_dl_tableau::hyper::ClashRecord`, `RUSTDL_SHADOW_DEP_PROBE`.
- Produces: a findings note stating, per stalled class + aggregate, the **real-`ALL` frequency**, **`bjgap_real` vs `bjgap_shadow`**, and the go-to Phase-2 fix.

- [ ] **Step 1: Confirm the stalled classes + namespace.**

Run: `RUSTUP_TOOLCHAIN=stable cargo build --release -p owl-dl-cli && ./target/release/rustdl hyper-sat ~/data/ore-run/input/ore_ont_10019.ofn --per-class-timeout-ms 300 2>&1 | grep -i stalled | head`
Record the stalled class IRIs (confirm `HydroxylGroup` is among them; grab ~5 of the deepest for the harness). Confirm the ontology namespace by grepping the `.ofn` for a class IRI.

- [ ] **Step 2: Write the probe harness (`#[ignore]`d gate, manual run).**

Create `crates/owl-dl-reasoner/tests/backjump_precision_gate.rs` modeled on `shadow_dep_gate.rs`. For each stalled class call `sat_class_probe(&ont, &iri(local), 256, Some(Duration::from_secs(30)))`, then from `stats.clash_records` compute + print:
```rust
let r = analyze(&records);
let n = records.len().max(1);
let real_all = records.iter()
    .filter(|c| c.real.highest == Some(127) && c.real.count == 0).count();
// disjunctive clashes where the PRECISE (shadow) dep would have allowed a real
// backjump (bjgap_shadow > 1) but the real dep-set is ALL (bjgap_real == 1):
let crippled = records.iter().filter(|c| {
    let real_all = c.real.highest == Some(127) && c.real.count == 0;
    let shadow_bjgap = c.shadow.highest.map_or(c.branch_depth + 1,
        |h| c.branch_depth.saturating_sub(h).saturating_add(1));
    real_all && shadow_bjgap > 1
}).count();
println!("[{class}] clashes={} real_ALL={}/{} ({:.1}%)  crippled_backjumps={}  \
    bjgap_real(med={} p90={} max={})  bjgap_shadow(med={} p90={} max={})",
    r.n_clashes, real_all, n, 100.0*real_all as f64/n as f64, crippled,
    r.bjgap_real.median, r.bjgap_real.p90, r.bjgap_real.max,
    r.bjgap_shadow.median, r.bjgap_shadow.p90, r.bjgap_shadow.max);
```
The file's module doc must state this is a read-only fix-selecting probe (H3b): it decides whether backjump degradation (real deps → `ALL` where shadow is precise) is the stall driver. (If `crippled` needs a helper not in `shadow_measures`, add it there; but the above is computable inline from `ClashRecord`.)

- [ ] **Step 3: Run it (asymptotic, probe on).**

```bash
RUSTUP_TOOLCHAIN=stable cargo build --release -p owl-dl-cli
RUSTDL_SHADOW_DEP_PROBE=1 RUSTDL_ADAPTIVE_BUDGET=0 RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --release --test backjump_precision_gate -- --ignored --nocapture 2>&1 | tee /tmp/bj-probe.txt | tail -40
```
Expected: per-class lines with `real_ALL%`, `crippled_backjumps`, and `bjgap_real` vs `bjgap_shadow`.

- [ ] **Step 4: Write findings + select the Phase-2 fix.**

Create `docs/2026-07-15-backjump-precision-findings.md` recording the per-class + aggregate numbers, then the verdict:
- **Fix #1 (backjump-precision repair)** if a **material fraction of disjunctive clashes are real-`ALL` while the shadow dep is precise** (`crippled_backjumps` non-trivial; `bjgap_shadow.median` >> `bjgap_real.median` ≈ 1). This confirms backjumping is crippled → Phase 2 tightens those dep-sets (FP-critical: sound over-approximation only). Name the widening site(s) implicated (`card_clash_deps` `≤n` / merge-taint) from the data.
- **Fix #2 (absorption/BCP) or bound-the-tail** if **real deps are already precise** (`real_ALL` low, `bjgap_real ≈ bjgap_shadow`) yet the DFS still can't prune — the disjunctive breadth is intrinsic; backjump repair won't help.
- State the confidence and any caveat (e.g. `merge=0` on the top-33 ⇒ if real-`ALL` is high there, it comes from cardinality-lowered disjunctive clauses, not merge-taint — note which).

- [ ] **Step 5: fmt + clippy + commit.**

```bash
RUSTUP_TOOLCHAIN=stable cargo fmt --all -- --check
RUSTUP_TOOLCHAIN=stable cargo clippy -p owl-dl-reasoner --all-targets --all-features -- -D warnings
git add crates/owl-dl-reasoner/tests/backjump_precision_gate.rs docs/2026-07-15-backjump-precision-findings.md
git commit -m "test(reasoner): H3b backjump-precision probe on ore_ont_10019 + findings (Phase 1)"
```

---

## Self-review notes

- **Spec coverage:** Phase 1 (spec §Phase 1 probe) → Task 1. The decisive signal (real-`ALL` frequency + `bjgap_real` vs `bjgap_shadow`) is the spec's fix-selecting criterion. Phase 2 explicitly deferred (spec §Phase 2).
- **Reuse-first:** the probe is the shipped `RUSTDL_SHADOW_DEP_PROBE` + `analyze`; new code is one `#[ignore]`d harness + a findings doc (+ optional 1 helper). No engine change → FP=0 trivially (read-only).
- **Residual-H1 (optional, secondary):** H1 is already ruled out (advisor: code gates ⊔ + SP2 `revisit≈1.0`). A `(depth, nodes.len())` correlation would nail the coffin but requires adding a field to the shared `ClashRecord` — out of scope unless the bjgap data is ambiguous; note it as a follow-up if so.
- **Open confirmations the implementer resolves in-task:** exact `sat_class_probe` signature + `ore_ont_10019` namespace (Step 1–2); whether `crippled_backjumps` is computable inline (it is, from `ClashRecord`) or needs a `shadow_measures` helper.
