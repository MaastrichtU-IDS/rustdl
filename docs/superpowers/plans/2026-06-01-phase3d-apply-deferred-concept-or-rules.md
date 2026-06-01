# Phase 3d — `apply_deferred_concept_or_rules` Performance Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reduce `apply_deferred_concept_or_rules`'s contribution to GALEN classify wall (currently 18.16% of SIO post-3c per `docs/flamegraphs/sio-classify-2026-06-01-post-phase3c-findings.md`) while preserving FP=0 + MISSED-unchanged across the Phase 0 soundness net + GALEN.

**Architecture:** Single-crate change to `crates/owl-dl-tableau/src/rules.rs::apply_deferred_concept_or_rules` (lines 546-617). The function is a per-node tableau rule that snapshots atomic-trigger labels, looks up matching concept rules via `concept_rules_by_trigger` (HashMap), runs the Phase 3a bloom-prefiltered `needs_deferred_or` per conclusion, then adds qualifying labels via `add_label_with_deps`. T1 identifies which inner sub-path actually dominates (trigger snapshotting / `pending` Vec allocation / `DepSet::clone` / `add_label_with_deps`); T3 designs the surgical fix per the candidate that matches; T4 implements; T5 measures.

**Tech Stack:** Rust (edition 2024), `owl-dl-tableau` crate, existing Phase 3a bloom prefilter (`label_sig: u64`), `concept_rules_by_trigger: HashMap<ClassId, Vec<ConceptId>>` finalized index, `pprof-rs` for flamegraph drill-down.

---

## Background the executor needs

- Phase 3 sequence to date:
  - **Phase 3a** (commit 64bee92): bloom prefilter (`args_mask & label_sig`) on `needs_deferred_or`. SIO `apply_deferred_concept_or_rules` 31.40% → 22.28%.
  - **Phase 3b** (commit cf05e22): O(1) `hashbrown::HashSet` for `are_declared_inverses`. SIO `apply_max` 27.93% → 6.51%.
  - **Phase 3c** (commit 0b5ed36): `OnceLock<ConceptId>` cache for `ConceptPool::bot_id`. SIO `apply_role_axioms`/`bot_id`/`find_map` cluster 24.66% → 0.45%; GALEN wall 24.8 → 12.2 min (~50% reduction).
- Post-3c top non-search frames on SIO (per `docs/flamegraphs/sio-classify-2026-06-01-post-phase3c-findings.md`):
  | Rank | Frame | % |
  |---|---|---|
  | 4 | `apply_deferred_concept_or_rules` | **18.16%** ← Phase 3d target |
  | 5 | `apply_role_rules` | 16.36% (Phase 3e candidate) |
  | 6 | `apply_max` | 14.34% |
  | 12 | `apply_deferred_or_residuals` | 6.70% |
  | 13/14 | `from_iter` / `collect` cluster | 6.51% (Phase 3e heap-alloc target) |
- Phase 0 soundness net (FP=0 / MISSED=0 gate): `alehif_closure_matches_konclude`, `ore_10908_sroiq`, `ore_15672_shoin` in `crates/owl-dl-reasoner/tests/konclude_closure_diff.rs`.
- GALEN baseline at HEAD (post-Phase-2c-revert, equivalent to post-3c): MISSED=17, wall ~12.2 min, FP=0.
- The function lives at `crates/owl-dl-tableau/src/rules.rs:546-617`. Its essential shape:
  ```rust
  pub fn apply_deferred_concept_or_rules(ctx, node) -> RuleOutcome {
      let pending: Vec<(ConceptId, DepSet)> = {
          let triggers: Vec<(ClassId, DepSet)> = n.labels()
              .iter().enumerate()
              .filter_map(|(pos, &c)| match pool.get(c) {
                  ConceptExpr::Atomic(cls) => Some((*cls, n.label_deps[pos].clone())),
                  _ => None,
              })
              .collect();                                  // alloc #1
          let mut out = Vec::new();                        // alloc #2
          for (trigger, deps) in &triggers {
              for &c in concept_rules_by_trigger.get(trigger).iter().flatten() {
                  if needs_deferred_or(...).0 {
                      out.push((c, deps.clone()));         // DepSet clone, alloc #3 each push
                  }
              }
          }
          out
      };
      for (c, deps) in pending {
          ctx.add_label_with_deps(node, c, &deps);
      }
  }
  ```
- Counters: `crates/owl-dl-tableau/src/counters.rs` already has `needs_deferred_or_bloom_rejects` (Phase 3a). Phase 3d can add a counter mirroring the same pattern.
- Diagnostics: `RUSTDL_TRACE=1` (one stderr line per branch decision; off-path is one atomic load) + `RUSTDL_COUNTERS=1` with `--features counters` (per-rule call counts dumped on `TableauContext::drop`).
- pprof flamegraph profiling: `cargo build --features profile -p owl-dl-bench` then `owl-dl-bench classify --profile <output.svg> <ontology>`.

---

## Task 1: Drill-down recon — identify the dominant inner sub-path

**Files:**
- Read: `docs/flamegraphs/sio-classify-2026-06-01-post-phase3c.svg` (existing post-3c flamegraph).
- Read: `crates/owl-dl-tableau/src/rules.rs:546-617` (the function).
- Read: `crates/owl-dl-tableau/src/graph.rs` (NodeId, label_deps definitions if needed).
- (Maybe) capture: `/tmp/p3d-recon.svg` if the existing flame lacks depth.
- Create: `docs/phase3d-recon.md` (committed) — recon findings.

T1 is the analytical heart. Phase 3a/3b/3c each had a clear leaf-frame target (`needs_deferred_or`, `are_declared_inverses`, `bot_id`); Phase 3d's target is the function ITSELF at 18.16%, which means the dominant cost is a SUM of internal work, not a single leaf. T1 identifies which sub-path actually dominates before T3 designs anything.

- [ ] **Step 1: Inspect the existing post-3c SVG to identify inner frames**

```bash
ls -la docs/flamegraphs/sio-classify-2026-06-01-post-phase3c.svg
# Open in browser to drill into apply_deferred_concept_or_rules; or grep
# for child frames:
grep -oE 'apply_deferred_concept_or_rules[^<]*' docs/flamegraphs/sio-classify-2026-06-01-post-phase3c.svg | head -20
```

If the existing SVG has sufficient depth to identify children of `apply_deferred_concept_or_rules` (filter_map closure, push, collect, add_label_with_deps, etc.), use it. If not, capture a fresh one:

```bash
cargo build --features profile --release -p owl-dl-bench
RUSTUP_PATH=/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin
./target/release/owl-dl-bench classify --profile /tmp/p3d-recon.svg ontologies/external/sio-stripped.ofn
# If sio-stripped doesn't exist, use sio-fp2-module.ofn from ontologies/real/
```

- [ ] **Step 2: Trace the function manually to enumerate candidate hot paths**

Read `crates/owl-dl-tableau/src/rules.rs:546-617` carefully. The candidate inner hot paths (per pattern-spotting):

| Candidate | Why hot | Fix shape |
|---|---|---|
| **A: trigger collection alloc** | Builds a fresh `Vec<(ClassId, DepSet)>` from a filter_map over all labels every call. DepSet cloned per atomic. | Iterate inline; avoid the intermediate Vec entirely (process each trigger directly into `out`). |
| **B: per-conclusion `needs_deferred_or` call** | Already Phase 3a-prefiltered, but `pool.get(c)` + the bloom mask + the disjunct walk fire per conclusion. May dominate when `concept_rules_by_trigger` has long lists. | Hoist common subexpressions; precompute Or-aware metadata at TBox finalize() time. |
| **C: `DepSet::clone` cost** | Each `out.push((c, deps.clone()))` clones a SmallVec. The post-3a flame attributed 4.08% to `clone SmallVec<[u32;1]>`. | Share via `Rc` / `Arc`; or batch by trigger so the clone happens once per trigger, not per conclusion. |
| **D: `add_label_with_deps` overhead** | The actual mutation path; trail-recording, blocking-sig update, label-set insertion. Counted under apply_deferred_* if it inlines. | Batch updates; defer the label_sig update to end of batch. |
| **E: `concept_rules_by_trigger.get()` HashMap miss/hit** | HashMap lookup per trigger. Could re-hash for every label-derived trigger. | Already O(1); likely not the dominant cost. |

The flame data drives the actual prioritization. List the children of `apply_deferred_concept_or_rules` from the SVG with sample counts (or %) and identify the dominant ONE. T3 designs the fix for that one only.

- [ ] **Step 3: Write `docs/phase3d-recon.md`**

```markdown
# Phase 3d recon — apply_deferred_concept_or_rules internals

## Flamegraph drill-down

<child-frame breakdown from the SVG; sample-count or % per child>

## Identified dominant inner cost

<the single candidate from T1 Step 2 that the flame data supports>

## Code-trace evidence

<file:line references; what the dominant cost actually does>

## Discarded candidates

<the others; why they're not the target this phase>

## Proposed fix shape

<one paragraph; T3 elaborates>
```

- [ ] **Step 4: Commit**

```bash
git add docs/phase3d-recon.md
git commit -m "perf(phase3d): apply_deferred_concept_or_rules drill-down recon"
```

If the recon shows the function's 18.16% is actually evenly split across 3+ sub-paths with no clear winner, REPORT DONE_WITH_CONCERNS — Phase 3d may need to be split into smaller plays, or skipped in favor of Phase 3e (`apply_role_rules` 16.36%) which has the same magnitude with a cleaner target.

---

## Task 2: Verify-before-build — measure the baseline

**Files:**
- Capture: `/tmp/p3d-baseline-galen.log` (GALEN wall + MISSED + FP).

Before any code change, lock in the baseline. Phase 3c results doc cites 12.2 min GALEN wall + MISSED=17 + FP=0 at HEAD 0b5ed36; current HEAD is 9b3060d (Phase 2c reverted; equivalent to 0b5ed36 modulo the kept canary + design doc). Confirm.

- [ ] **Step 1: Run GALEN at current HEAD**

```bash
RUSTUP_PATH=/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin
export PATH=$RUSTUP_PATH:$PATH
cargo build --release -p owl-dl-reasoner 2>&1 | tail -3
timeout 1500 cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    galen_closure_matches_konclude \
    -- --exact --ignored --nocapture 2>&1 | tee /tmp/p3d-baseline-galen.log | grep -E "^--- galen|MISSED=|FP=|finished"
```

Hard cap 25 min. Expected: FP=0, MISSED=17, wall ~12-13 min (Phase 2c just added 7%, then reverted).

- [ ] **Step 2: Compare to phase3c-results.md**

Record exact values. If wall differs from 12.2 min by >20%, the baseline shifted — re-read flame, re-derive Phase 3d's target.

No commit; this is measurement only.

---

## Task 3: Design — chosen fix + soundness preconditions

**Files:**
- Create: `docs/phase3d-fix-target.md` (committed).

T3's deliverable specifies the EXACT code change for the candidate T1 identified.

- [ ] **Step 1: Write `docs/phase3d-fix-target.md`**

Structure:

```markdown
# Phase 3d — fix target

Per Phase 3d recon (`docs/phase3d-recon.md`), the dominant inner cost
of `apply_deferred_concept_or_rules` is <X>. This doc specifies the
fix.

## Target code

<file:line reference + the specific block being changed>

## Fix shape

<the exact data-structure or algorithmic change>

## Soundness invariant

<what stays the same; what could go wrong; specifically: does the
change preserve the "Or-only materialization" semantic of
needs_deferred_or? does it preserve DepSet propagation for the
back-jumping driver?>

## Predicted impact

<expected % reduction in apply_deferred_concept_or_rules frame; expected
% wall reduction on GALEN. Honest range, not a point estimate.>

## What this design does NOT change

<other sub-paths of the function; future Phase 3e+ targets>
```

- [ ] **Step 2: Commit**

```bash
git add docs/phase3d-fix-target.md
git commit -m "perf(phase3d): chosen fix target + soundness preconditions"
```

---

## Task 4: Implement + structural canary

**Files:**
- Modify: `crates/owl-dl-tableau/src/rules.rs` (around line 546-617 per T3's target).
- Modify: `crates/owl-dl-tableau/src/counters.rs` (add counter for fired/skipped per Phase 3d's path).
- Modify: `crates/owl-dl-tableau/src/lib.rs::mod tests` OR `crates/owl-dl-tableau/tests/` (structural canary asserting the new counter bumps on a minimal positive case).

Exact code depends on T3's design. The framework:

- [ ] **Step 1: Add the Phase 3d counter to `counters.rs`**

Mirror the Phase 3a pattern (`needs_deferred_or_bloom_rejects`). Name it descriptively (`apply_deferred_concept_or_<phase3d-specific-name>_saved` or similar — T3's design names it).

- [ ] **Step 2: Implement the fix in `apply_deferred_concept_or_rules`**

Per T3's design. Keep the change SURGICAL — Phase 3d is one specific sub-path. Do NOT refactor the surrounding function.

- [ ] **Step 3: Add structural canary**

Mirror the Phase 3a counter-bump test pattern. The canary should:
- Construct a minimal ontology that exercises the optimized path.
- Run classify (or the targeted rule directly).
- Assert the Phase 3d counter is > 0.

- [ ] **Step 4: Run structural canary + saturation suite + tableau suite**

```bash
cargo test -p owl-dl-tableau --features counters -- --test-threads=1 2>&1 | tail -10
cargo test -p owl-dl-tableau -- --test-threads=1 2>&1 | tail -10
cargo test -p owl-dl-saturation -- --test-threads=1 2>&1 | tail -5
cargo test -p owl-dl-reasoner --lib -- --test-threads=1 2>&1 | tail -5
RUSTFLAGS="-D warnings" cargo test -p owl-dl-tableau --no-run 2>&1 | tail -3
cargo clippy -p owl-dl-tableau --all-targets -- -D warnings 2>&1 | grep -E "warning|error" | head -10
```

Expected: all green; CI strictness clean.

- [ ] **Step 5: Commit**

```bash
git add crates/owl-dl-tableau/src/rules.rs \
        crates/owl-dl-tableau/src/counters.rs \
        crates/owl-dl-tableau/tests/  # or wherever the canary lives
git commit -m "perf(tableau): apply_deferred_concept_or_rules <Phase 3d fix> (Phase 3d)

<one paragraph: what changed, why, what the structural canary covers,
the soundness invariant preserved. See docs/phase3d-fix-target.md.>"
```

---

## Task 5: Corpus measurement — Phase 0 net + SIO flame + GALEN wall

**Files:**
- Capture: `/tmp/p3d-net.log`, `/tmp/p3d-sio-flame.svg`, `/tmp/p3d-galen.log`.

- [ ] **Step 1: Phase 0 net soundness gate**

```bash
timeout 1800 cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    alehif_closure_matches_konclude ore_10908_sroiq ore_15672_shoin \
    -- --ignored --nocapture 2>&1 | tee /tmp/p3d-net.log | grep -E "^---|FP=|MISSED="
```

Hard cap 30 min. Expected: FP=0 / MISSED=0 across all 3. If ANY FP > 0, the fix is UNSOUND — STOP. Investigate which fixture and which spurious subsumption.

- [ ] **Step 2: Fresh SIO flamegraph**

```bash
cargo build --features profile --release -p owl-dl-bench 2>&1 | tail -3
./target/release/owl-dl-bench classify --profile /tmp/p3d-sio-flame.svg \
    ontologies/real/sio-fp2-module.ofn  # or sio-stripped if it exists
```

Record `apply_deferred_concept_or_rules`'s new % from the SVG (grep for it). Expected: meaningful drop vs 18.16% baseline.

- [ ] **Step 3: GALEN wall**

```bash
timeout 1500 cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    galen_closure_matches_konclude \
    -- --exact --ignored --nocapture 2>&1 | tee /tmp/p3d-galen.log | grep -E "^--- galen|MISSED=|FP=|finished"
```

Hard cap 25 min. Expected: FP=0; MISSED=17 unchanged; wall reduced from 12.2 min baseline.

- [ ] **Step 4: Triage**

Tabulate: Phase 0 net (3 fixtures × FP/MISSED), SIO flame % delta, GALEN wall delta. Compute % wall reduction. Compare to T3's predicted impact.

If FP > 0 anywhere: REVERT and report.
If wall increased: REVERT — Phase 3d was a regression. Investigate.
If wall unchanged within noise (~±3%): the fix is neutral. Decide ship-or-revert per the counter-bump signal (rule fires structurally but no measurable wall delta) vs the architectural-debt question.
If wall reduced: real win, proportional to T3's prediction.

No commit; T6 captures.

---

## Task 6: Results doc + envelope updates

**Files:**
- Create: `docs/phase3d-results.md`.
- Modify: `CLAUDE.md` (tableau description).
- Modify: `docs/superpowers/specs/2026-05-31-soundness-completeness-perf-design.md` (Phase 3d close-out).

- [ ] **Step 1: Write `docs/phase3d-results.md`**

Mirror Phase 3a/3b/3c results-doc shape:

```markdown
# Phase 3d — apply_deferred_concept_or_rules <fix-name> results

Run 2026-06-0N. Fix: <one paragraph>. See `docs/phase3d-fix-target.md`
for design and `docs/phase3d-recon.md` for the recon that drove target
selection.

## Headline finding

<one paragraph: wall reduction; flame delta; FP gate status>

## Soundness gate (Phase 0 net)

<table>

## Wall lever (GALEN)

<table with pre-3d / post-3d wall + MISSED + FP>

## Flame delta (SIO)

<top frame changes>

## What's left

<residual % in apply_deferred_concept_or_rules; next-target candidates>

## Cross-references

- Phase 3d recon: `docs/phase3d-recon.md`
- Phase 3d design: `docs/phase3d-fix-target.md`
- Phase 3c results (prior baseline): `docs/phase3c-results.md`
```

- [ ] **Step 2: Update CLAUDE.md tableau description**

Append a one-paragraph Phase 3d note to the `crates/owl-dl-tableau` bullet.

- [ ] **Step 3: Update design spec**

Append Phase 3d close-out paragraph to `docs/superpowers/specs/2026-05-31-soundness-completeness-perf-design.md`.

- [ ] **Step 4: Commit**

```bash
git add docs/phase3d-results.md \
        CLAUDE.md \
        docs/superpowers/specs/2026-05-31-soundness-completeness-perf-design.md
git commit -m "docs(phase3d): results doc + envelope updates"
```

---

## Definition of done (Phase 3d)

- Structural canary passes (Phase 3d counter > 0 on positive minimum).
- All tableau + saturation + reasoner-lib tests pass; CI strictness clean.
- Phase 0 net FP=0 + MISSED-unchanged held.
- SIO `apply_deferred_concept_or_rules` % meaningfully reduced from 18.16%.
- GALEN wall reduced (or, if unchanged, accepted with documented rationale).
- Results doc + CLAUDE.md + design spec updated.

## What this plan does NOT do

- Does NOT change verdicts on any corpus fixture.
- Does NOT touch `apply_role_rules` (Phase 3e candidate, 16.36%) or `apply_max` (14.34%, post-Phase-3b).
- Does NOT touch the wedge / hypertableau / saturation engines.
- Does NOT change the env-flag defaults (`RUSTDL_HYPERTABLEAU*`).
