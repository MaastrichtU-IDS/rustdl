# SP0 — Value-Partition Saturation Spike (GATING) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans. Steps use checkbox (`- [ ]`). **This is a THROWAWAY RESEARCH SPIKE — the deliverable is a go/no-go measurement + verdict doc, then revert. Do NOT polish, do NOT add exhaustive unit tests, do NOT merge the code.**

**Goal:** Determine — in weeks, not months — whether a saturation that pre-resolves wine's value-partition structure can *collapse* the wedge's per-pair branching (rustdl DNF → sub-second) while keeping FP=0 on the coupled system. This gates the entire coupled-saturation–tableau project.

**Architecture:** A wine-specific, env-gated (`RUSTDL_SAT_SPIKE`) prototype: (1) identify value partitions in the IR, (2) compute the deterministic "every-model" consequence of the `∀R.{partition-subset}` restrictions, (3) seed the wedge with those facts via the existing `HyperEngine::new_seeded` path, (4) measure the matched hard test + corpus FP. Wine-specific / soundness-relaxed-internally is acceptable — it is reverted after measuring.

**Tech Stack:** Rust; `crates/owl-dl-core` (IR), `crates/owl-dl-saturation`, `crates/owl-dl-tableau` (wedge), `crates/owl-dl-reasoner` (classify/orchestrator). Konclude binary + `docs/konclude-vs-rustdl-wine-2026-06-23.md` for reference numbers.

## Global Constraints

- **FP=0 is SACRED.** Gate = byte-identical classification closures (md5 of sorted `direct`/`equiv` edges) vs the pre-spike binary, **on the coupled (seeded) system** — never inferred from saturator soundness (the snapshot-cache precedent: looked sound, ORE found 30+ FP).
- **Spike is throwaway and OPT-IN.** Everything behind `RUSTDL_SAT_SPIKE` (default OFF). The off-path must be byte-identical to current `main`. After the verdict, revert all code; keep only the verdict doc.
- **Pre-committed verdict rule (GATE):** proceed to SP1+ ONLY IF (1) `sat(AlsatianWine ⊓ ¬AmericanWine)` and ≥2 other rustdl-DNF wine pairs drop from 60s-DNF to **sub-second** (branching collapses), AND (2) the tuned corpus stays **FP=0** with the spike ON. If branching does NOT collapse even with the wine-specific hack → **project DEAD**, record and stop.
- Toolchain: `export RUSTUP_HOME=/home/dumontier/.rustup CARGO_HOME=/home/dumontier/.cargo; export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$CARGO_HOME/bin:$PATH"`.
- Branch: `spike/sat-valuepartition-spike` (off `main`). Scratchpad harness + `/tmp/wine-probe.ofn` (Probe ≡ AlsatianWine ⊓ ¬AmericanWine) already exist.

## Matched reference (the thing to beat)

`sat(AlsatianWine ⊓ ¬AmericanWine)`: Konclude **1ms** (after 39ms precompute); rustdl **DNF 60s**. Wine partitions: `WineBody≡{Full,Light,Medium}`, `WineColor≡{Red,Rose,White}`, `WineFlavor≡{Delicate,Moderate,Strong}`, `WineSugar≡{Dry,OffDry,Sweet}`; varietals constrain via `∀hasBody.{Full,Medium}` etc.

---

### Task 1: Identify value partitions in the IR (concrete, bounded)

**Files:**
- Create: `crates/owl-dl-core/src/value_partition_spike.rs`
- Modify: `crates/owl-dl-core/src/lib.rs` (add `pub mod value_partition_spike;`)

**Interfaces:**
- Produces: `pub struct ValuePartition { pub partition_class: ClassId, pub members: Vec<ClassId> }` and `pub fn detect_value_partitions(onto: &InternalOntology) -> Vec<ValuePartition>` — a partition is a class `P` with `EquivalentClasses(P, ObjectOneOf(m1..mn))` (nominals) whose members are pairwise disjoint (from told-disjoint or `DifferentIndividuals`).

- [ ] **Step 1: Write a focused test fixture + test**

In `value_partition_spike.rs` `#[cfg(test)]`: build a tiny `InternalOntology` (or load `ontologies/real/wine.ofn` via the convert pipeline in a test helper) and assert `detect_value_partitions` finds the 4 wine partitions (WineBody/Color/Flavor/Sugar) with their member counts (3 each).

- [ ] **Step 2: Run — expect FAIL** (`detect_value_partitions` undefined). `cargo test -p owl-dl-core value_partition_spike -- --nocapture`.

- [ ] **Step 3: Implement `detect_value_partitions`**

Scan the IR for `EquivalentClasses(P, ObjectOneOf(members…))` (the IR's nominal/`ObjectOneOf` representation — check `ir.rs` for the `OneOf`/nominal concept variant and how `EquivalentClasses` is stored after `convert.rs`/`absorb.rs`). Collect `(P, members)`. Keep only those whose members are pairwise told-disjoint (reuse `told.rs` told-disjoint table). Return the list.

- [ ] **Step 4: Run — expect PASS** (finds 4 wine partitions). Commit: `git commit -m "spike: detect value partitions (SP0 Task1, throwaway)"`.

---

### Task 2: Compute + seed the deterministic value-partition consequence (THE SPIKE CORE — exploratory)

**Files:**
- Create: `crates/owl-dl-reasoner/src/sat_spike.rs` (the seed builder + env gate)
- Modify: `crates/owl-dl-reasoner/src/classify.rs` (~line 1991 per-pair decide path: when `RUSTDL_SAT_SPIKE=1`, build the spike seed and use `new_seeded`)
- Modify: `crates/owl-dl-tableau/src/hyper.rs` only if `AboxSeed` needs an extra field for class-label seeds (it currently seeds nominals/property-assertions/same-pairs; **the spike likely needs a `class_labels: Vec<(u32 node, ClassId)>` seed channel** — add it minimally if absent).

**Interfaces:**
- Consumes: `detect_value_partitions` (Task 1); `HyperEngine::new_seeded(clauses, &AboxSeed)` (hyper.rs:1591); `AboxSeed` (hyper.rs:330).
- Produces: `pub fn build_spike_seed(onto: &InternalOntology, partitions: &[ValuePartition]) -> AboxSeed` + the env-gated wiring.

**This is the spike's exploratory heart. Hypothesis (first concrete attempt):** the wedge branches over *which* partition value each `∀R.{subset}` successor takes, even when the choice is irrelevant to the test. The deterministic all-model consequence is: a `∀R.{Full,Medium}`-restricted `R`-successor is a single `WineBody` whose value is *don't-care* — represent it with **one deterministic representative successor** carrying the partition class (`WineBody`) instead of `n` branched value-nodes. Seed the engine so the value-partition successors are pre-materialized as deterministic representatives (class-label seed = the partition class), removing the disjunctive `ObjectOneOf` branch.

- [ ] **Step 1: Build the seed (first attempt)**

In `sat_spike.rs`: `build_spike_seed` walks the test concept's `∀R.{partition-subset}` restrictions; for each, allocate a representative successor node seeded with the partition class label and the subset members' shared constraints, and record the `R`-edge in `AboxSeed.property_assertions`. Add a `class_labels` channel to `AboxSeed` if needed (seed node `i` with `ClassId` directly). Gate the whole thing on `std::env::var("RUSTDL_SAT_SPIKE")==Ok("1")`.

- [ ] **Step 2: Wire into the per-pair decide** (classify.rs ~1991): when the env flag is on, construct the spike seed and call `new_seeded` instead of the normal engine build for the pair. Off-flag path unchanged.

- [ ] **Step 3: Build** `cargo build --release -p owl-dl-cli`. Expected: compiles. (No unit test here — the measurement in Task 3 is the test; this is a spike.)

- [ ] **Step 4: Commit** `git commit -m "spike: value-partition deterministic seed + wedge coupling (SP0 Task2, throwaway)"`.

---

### Task 3: Measure the matched hard test (the branch-collapse gate)

**Files:** none (measurement). Reuse `/tmp/wine-probe.ofn`.

- [ ] **Step 1: Run the matched test, spike OFF vs ON**

```bash
R=./target/release/rustdl; P="http://www.w3.org/TR/2003/PR-owl-guide-20031209/wine#Probe"
echo "OFF:"; timeout 90 "$R" sat /tmp/wine-probe.ofn "$P"; 
echo "ON:";  RUSTDL_SAT_SPIKE=1 timeout 90 "$R" sat /tmp/wine-probe.ofn "$P"
```
Expected OFF: DNF/timeout. Target ON: returns `satisfiable` sub-second (or at least finishes ≪ 90s).

- [ ] **Step 2: Repeat on ≥2 more rustdl-DNF wine pairs** — build `/tmp/wine-probe2.ofn` (`Probe ≡ AmericanWine ⊓ ¬Anjou`) and `/tmp/wine-probe3.ofn` (`Probe ≡ Beaujolais ⊓ ¬Bordeaux`) the same way (these all DNF'd at 12s in the earlier sweep). Run spike ON, record walls + verdicts.

- [ ] **Step 3: Branch-collapse gate.** If all (or ≥2/3) drop DNF→sub-second AND return the correct verdict (`satisfiable` — these are non-subsumptions; confirm against Konclude `satisfiability -x` on the same Probe ontologies), the collapse criterion is met. If they still DNF/timeout → **STOP, project dead** (jump to Task 5 verdict=DEAD).

---

### Task 4: Corpus FP=0 gate (coupled system — the sacred tripwire)

**Files:** none (measurement). Reuse the closure-md5 A/B harness pattern.

- [ ] **Step 1: Capture BEFORE closures** with spike OFF (== current main) on the tuned corpus: `galen` (EL control — must be byte-identical, spike must not touch the saturator fast path), `notgalen`, `sio`, `pizza`, `ore-10908-sroiq`, `ore-15516-alchoiq`, `alehif-test`. For each: `rustdl classify --pair-timeout-ms 1000 <f> | grep -E '^(direct|equiv)' | sort | md5sum`.

- [ ] **Step 2: Capture AFTER closures** with `RUSTDL_SAT_SPIKE=1` on the same set.

- [ ] **Step 3: FP gate.** Diff the md5s. **Any difference = the spike's seed/coupling is unsound (FP risk) — record it (this is a real finding about the coupling FP surface) and treat as a soundness obstacle in the verdict.** galen MUST be byte-identical (the spike must not perturb the EL fast path). Note: closures changing toward *more* edges that are genuinely entailed would need oracle-checking, but for a throwaway spike, ANY closure change with the flag on is a red flag to investigate, since the seed should only make hard tests faster, not change verdicts.

---

### Task 5: Verdict + revert

**Files:**
- Create: `docs/sp0-saturation-spike-results-2026-06-23.md` (committed — the durable verdict).

- [ ] **Step 1: Write the verdict doc** with: the matched-test before/after walls (Task 3), the corpus FP result (Task 4), and the **verdict per the pre-committed rule**: GO (branching collapsed + FP=0 → SP1 is justified), or NO-GO (didn't collapse, or FP obstacle → project dead/blocked), with the reason.

- [ ] **Step 2: Revert the spike code** (keep only the verdict doc): `git checkout main -- crates/` equivalent, or cherry-pick just the doc onto a clean branch. The throwaway code does NOT merge to main.

- [ ] **Step 3: Commit the verdict doc** and hand back for the user decision on whether to start SP1.

---

## Self-Review

**Spec coverage:** SP0 from the spec (throwaway spike, value-partition saturation, seed via new_seeded, env-gated, branch-collapse + FP=0-on-coupled gate, pre-committed verdict, revert) — all covered (Tasks 1–5). The spec's SP1–SP3 are explicitly out of scope for this plan (gated on SP0's GO).

**Placeholder scan:** Task 2 is legitimately exploratory (a research spike's core) — it carries a *concrete first hypothesis* (deterministic representative for `∀R.{partition-subset}` successors) + concrete seed wiring + the measurement that decides, not a hand-wave. The exact deterministic-consequence rule may need 1–2 iterations within Task 2; that is the nature of a spike, and the Task-3 gate bounds it.

**Type consistency:** `ValuePartition{partition_class:ClassId, members:Vec<ClassId>}`, `detect_value_partitions(&InternalOntology)->Vec<ValuePartition>`, `build_spike_seed(&InternalOntology,&[ValuePartition])->AboxSeed`, `new_seeded(clauses,&AboxSeed)` — consistent across tasks. `AboxSeed` gains a `class_labels` channel in Task 2 if needed (flagged, not assumed).

**Spike discipline:** the plan repeatedly marks throwaway/env-gated/revert; no merge to main; deliverable is the verdict doc. This matches the session's P0-gate pattern.
