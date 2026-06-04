# Phase 3e — `apply_role_rules` inner-cost Performance Plan

> **Outcome: SHIPPED-AND-REVERTED.** GALEN +2.34% wall regression despite SIO flame win; reverted at commit `a2a4d7f`. See `docs/phase3e-results.md` and dead-end §16.

---

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reduce `apply_role_rules`'s contribution to GALEN classify wall (currently 16.36% of SIO post-3c per `docs/flamegraphs/sio-classify-2026-06-01-post-phase3c-findings.md`, likely similar post-3d after the apply_deferred drop redistributed the denominator) while preserving FP=0 + MISSED=17 on GALEN and FP=0 / MISSED=0 on the Phase 0 net.

**Architecture:** Single-crate change to `crates/owl-dl-tableau/src/rules.rs::apply_role_rules` (lines 313-405). Unlike Phase 3d, `apply_role_rules` ALREADY has the dispatch-vs-fallback hoist done (line 325-326's `use_index` gate). The 16.36% is inner work — likely the `matching_edges` closure (line 346-359) doing repeated edge scans per rule, the `edge_satisfies` predicate (10.65% leaf frame per post-3c flame), or the `guards_present` HashMap build (line 334-342). T1 recon identifies the dominant inner sub-path; T3 designs the surgical fix; T4 implements; T5 measures.

**Tech Stack:** Rust (edition 2024), `owl-dl-tableau` crate, existing `unguarded_role_rules` / `guarded_role_rules_by_guard` partitioned indices, `pprof-rs` for flamegraph drill-down.

---

## Background the executor needs

- Phase 3 sequence to date:
  - **3a** (64bee92): bloom prefilter on `needs_deferred_or`. SIO `apply_deferred…` 31.40% → 22.28%.
  - **3b** (cf05e22): O(1) HashSet for `are_declared_inverses`. SIO `apply_max` 27.93% → 6.51%.
  - **3c** (0b5ed36): `OnceLock` cache for `ConceptPool::bot_id`. SIO `apply_role_axioms` cluster 24.66% → 0.45%; GALEN wall 24.8 → 12.2 min.
  - **3d** (32aeda6 + d4f85f8): hoisted apply_deferred linear-scan fallback. SIO top-frame 18.16% → 3.23%; GALEN wall 12.43 → 11.87 min (−4.5%).
- Post-3c top non-search frames on SIO (likely shifted post-3d as denominator redistributed):
  | Rank | Frame | Post-3c % |
  |---|---|---|
  | 5 | `apply_role_rules` | **16.36%** ← Phase 3e target |
  | 6 | `apply_max` | 14.34% |
  | 7 | `{closure#1}` | 13.41% (suspected = `matching_edges`) |
  | 8 | `edge_satisfies` | 10.65% (leaf) |
  | 9 | `eq` | 8.81% |
  | 12 | `apply_deferred_or_residuals` | 6.70% |
  | 15 | `are_declared_inverses` | 6.43% (Phase 3b'd, now O(1) but still warm) |
- Phase 0 soundness net: `alehif`, `ore_10908_sroiq`, `ore_15672_shoin` in `crates/owl-dl-reasoner/tests/konclude_closure_diff.rs`.
- GALEN baseline (post-3d): MISSED=17, wall 11.87 min (clean), FP=0.
- The function at `crates/owl-dl-tableau/src/rules.rs:313-405`. Key inner structure:
  ```rust
  pub fn apply_role_rules(ctx, node) -> RuleOutcome {
      // ... index-vs-fallback gate already in place at line 325-326 ...
      let pending: Vec<(NodeId, ConceptId, DepSet)> = {
          // Build guards_present: HashMap<ClassId, DepSet> from atomic labels (alloc)
          let guards_present: HashMap<ClassId, DepSet> = n.labels()
              .iter().enumerate()
              .filter_map(|(pos, &c)| match pool.get(c) {
                  ConceptExpr::Atomic(cls) => Some((*cls, n.label_deps[pos].clone())),
                  _ => None,
              })
              .collect();
          // matching_edges closure: linear scan of n.edges + n.in_edges per rule.role
          let matching_edges = |rule_role: Role| {
              let mut triples = Vec::new();
              for &(role, neighbour) in &n.edges { if edge_satisfies(Role::Named(role), rule_role) { ... } }
              for &(role, neighbour) in &n.in_edges { if edge_satisfies(Role::Inverse(role), rule_role) { ... } }
              triples
          };
          if use_index {
              for rule in &tbox.unguarded_role_rules {
                  for triple in matching_edges(rule.role) { out.push(...) }     // alloc per call
              }
              for (g, guard_deps) in &guards_present {
                  if let Some(rules) = tbox.guarded_role_rules_by_guard.get(g) {
                      for rule in rules {
                          for triple in matching_edges(rule.role) { out.push(...) }
                      }
                  }
              }
          } else { /* pre-finalize fallback retained */ }
          out
      };
      for (target, c, deps) in pending { ctx.add_label_with_deps(target, c, &deps); }
  }
  ```
- The candidate inner hot paths (T1's enumeration target):
  | Candidate | Why hot | Fix shape |
  |---|---|---|
  | **A: `matching_edges` allocates a Vec per call** | Called once per `unguarded_role_rule` + once per (guard × rule). Each call linear-scans `n.edges` + `n.in_edges` + allocs a `Vec<(Role, NodeId, DepSet)>`. | Convert to iterator (avoid allocation); or hoist edge-scan once per node (group rules by role first). |
  | **B: `edge_satisfies` per-edge-per-rule cost** | Called twice per edge per rule (`n.edges` * `n.in_edges` * rules). Inner call traverses role hierarchy. | Pre-compute satisfied role set per edge once; or hoist role-set lookup outside inner loop. |
  | **C: `guards_present` HashMap build cost** | Re-builds the full guard→deps map every call (per node) by filter_mapping all labels + cloning DepSets. | Cache per-node; rebuild on label set change; OR keep as Vec/SmallVec if guards are usually few. |
  | **D: per-rule HashMap iteration over `guarded_role_rules_by_guard`** | Iterates `guards_present` then HashMap-gets per guard. Cheap individually but cumulative. | Likely fine; not a target. |
  | **E: `add_label_with_deps` per `pending` entry** | Mutation path; trail-recording, blocking-sig update. Counted under apply_role_rules if it inlines. | Same as Phase 3d candidate D — batching could help but bigger surface. |
- Counter pattern: `crates/owl-dl-tableau/src/counters.rs` (Phase 3a `needs_deferred_or_bloom_rejects`, Phase 3d `apply_deferred_concept_or_skip_missing_trigger`). Mirror.
- Diagnostics: `RUSTDL_TRACE=1`, `RUSTDL_COUNTERS=1` + `--features counters`.

**ENV NOTE for executor**: cargo and rustc are NOT on the default shell PATH. Always:
```bash
export PATH=/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH
```

---

## Task 1: Drill-down recon

**Files:**
- Read: `docs/flamegraphs/sio-classify-2026-06-01-post-phase3d.svg` (just-archived post-3d flame).
- Read: `crates/owl-dl-tableau/src/rules.rs:313-405` (the function).
- (Maybe) capture: `/tmp/p3e-recon.svg` — fresh focused flame, if the existing one lacks depth.
- Create: `docs/phase3e-recon.md` (committed).

- [ ] **Step 1: Inspect post-3d SVG for `apply_role_rules` inner frames**

```bash
grep -oE '<title>[^<]*apply_role_rules[^<]*</title>' docs/flamegraphs/sio-classify-2026-06-01-post-phase3d.svg | head -20
grep -oE '<title>[^<]*matching_edges[^<]*</title>' docs/flamegraphs/sio-classify-2026-06-01-post-phase3d.svg | head -10
grep -oE '<title>[^<]*edge_satisfies[^<]*</title>' docs/flamegraphs/sio-classify-2026-06-01-post-phase3d.svg | head -10
```

Each `<title>` tag has `(<N> samples, <P>%)` — extract attribution for each candidate.

If the SVG doesn't have enough depth, capture a fresh focused flame:

```bash
export PATH=/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH
RUSTDL_PROFILE=/tmp/p3e-recon-sio.svg RUSTDL_PROFILE_SECONDS=90 \
    ./target/release/owl-dl-bench classify ontologies/real/sio-fp2-module.ofn 2>&1 | tail -5
```

(Build with profile feature first if needed: `cargo build --features profile --release -p owl-dl-bench`.)

- [ ] **Step 2: Code-trace `apply_role_rules`**

Read `crates/owl-dl-tableau/src/rules.rs:313-405` line-by-line. For each candidate (A-E above), estimate its share from the flame + cross-check with code-traced cost.

Open question to resolve: does `guards_present` get rebuilt every call (per node)? If so, candidate C may dominate when nodes have many atomic labels.

- [ ] **Step 3: Write `docs/phase3e-recon.md`**

```markdown
# Phase 3e recon — apply_role_rules internals

Source: post-Phase-3d SIO flamegraph
(`docs/flamegraphs/sio-classify-2026-06-01-post-phase3d.svg`) + code-trace.
HEAD: d4f85f8.

## Flamegraph drill-down

<per-child sample counts / % for apply_role_rules>

## Identified dominant inner cost

<which candidate (A-E) the flame supports; quantitatively>

## Code-trace evidence

<file:line refs>

## Discarded candidates

<the others; brief justification>

## Proposed fix shape (handoff to T3)

<one paragraph>
```

- [ ] **Step 4: Commit**

```bash
git add docs/phase3e-recon.md
git commit -m "perf(phase3e): apply_role_rules drill-down recon"
```

If the recon shows the 16.36% is split across 3+ sub-paths with no clear winner, REPORT DONE_WITH_CONCERNS — Phase 3e may need to be split or pivoted to the `from_iter/collect` heap-alloc cluster (post-3c: 6.51%) which has a cleaner target.

---

## Task 2: Lock baseline

Same as Phase 3d T2.

- [ ] **Step 1: Run GALEN at current HEAD**

```bash
export PATH=/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH
timeout 1500 cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    galen_closure_matches_konclude \
    -- --exact --ignored --nocapture 2>&1 | tee /tmp/p3e-baseline-galen.log | grep -E "^--- galen|MISSED=|FP=|finished"
```

Expected: FP=0, MISSED=17, wall ~11.5-12.5 min (post-3d range).

- [ ] **Step 2: Record exact values**

No commit; measurement only.

---

## Task 3: Design

- [ ] **Step 1: Write `docs/phase3e-fix-target.md`**

Structure per Phase 3d's design doc template (`docs/phase3d-fix-target.md`):

```markdown
# Phase 3e — fix target

Per Phase 3e recon (`docs/phase3e-recon.md`), the dominant inner cost
of `apply_role_rules` is <X>. This doc specifies the fix.

## Target code

<file:line + quoted current block>

## Fix shape

<exact restructuring; example code>

## Soundness invariant

<what stays the same; specifically: does the change preserve role-hierarchy semantics
(Role::Named vs Role::Inverse, edge_satisfies's check)? does it preserve DepSet propagation
including the guard-deps union?>

## Predicted impact

<expected % reduction; honest range>

## What this design does NOT change

<other sub-paths; future Phase 3f+ candidates>
```

- [ ] **Step 2: Commit**

```bash
git add docs/phase3e-fix-target.md
git commit -m "perf(phase3e): chosen fix target + soundness preconditions"
```

---

## Task 4: Implement + structural canary

Per Phase 3d T4 pattern.

- [ ] **Step 1: Add Phase 3e counter (if a sensible event exists)**

Mirror Phase 3a/3d pattern in `crates/owl-dl-tableau/src/counters.rs`. Gate on `cfg(feature = "counters")`.

- [ ] **Step 2: Implement the fix**

Per T3's design. Surgical — limit changes to the identified sub-path.

- [ ] **Step 3: Structural canary**

Mirror Phase 3d's `phase3d_indexed_branch_skips_missing_triggers`. Construct minimal positive case that exercises the optimized path; assert counter bumps.

- [ ] **Step 4: Regression sweep**

```bash
export PATH=/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH
cargo test -p owl-dl-tableau --features counters -- --test-threads=1 2>&1 | tail -10
cargo test -p owl-dl-tableau -- --test-threads=1 2>&1 | tail -10
cargo test -p owl-dl-saturation -- --test-threads=1 2>&1 | tail -5
cargo test -p owl-dl-reasoner --lib -- --test-threads=1 2>&1 | tail -5
RUSTFLAGS="-D warnings" cargo test -p owl-dl-tableau --no-run 2>&1 | tail -3
cargo clippy -p owl-dl-tableau --all-targets -- -D warnings 2>&1 | grep -E "warning|error" | head -10
```

All green; CI strict clean. If any pre-existing test fails, STOP and investigate.

- [ ] **Step 5: Commit**

```bash
git add crates/owl-dl-tableau/src/rules.rs \
        crates/owl-dl-tableau/src/counters.rs
git commit -m "perf(tableau): <Phase 3e specific change> (Phase 3e)

<one paragraph: what changed, soundness invariant, predicted impact.
See docs/phase3e-fix-target.md.>"
```

---

## Task 5: Corpus measurement

Per Phase 3d T5 pattern.

**ENV NOTE**: always `export PATH=/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH` before bash commands.

- [ ] **Step 1: Phase 0 net soundness gate (30 min cap)**

```bash
timeout 1800 cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    alehif_closure_matches_konclude ore_10908_sroiq ore_15672_shoin \
    -- --ignored --nocapture 2>&1 | tee /tmp/p3e-net.log | grep -E "^---|FP=|MISSED="
```

FP > 0 anywhere → REVERT.

- [ ] **Step 2: Fresh SIO flamegraph**

```bash
cargo build --features profile --release -p owl-dl-bench 2>&1 | tail -3
RUSTDL_PROFILE=/tmp/p3e-sio-flame.svg RUSTDL_PROFILE_SECONDS=60 \
    ./target/release/owl-dl-bench classify ontologies/real/sio-fp2-module.ofn 2>&1 | tail -5
grep -oE '<title>apply_role_rules[^<]*</title>' /tmp/p3e-sio-flame.svg | head -5
```

Record `apply_role_rules`'s new top-frame %.

- [ ] **Step 3: Clean GALEN wall (25 min cap, NO concurrent load)**

**CRITICAL**: do NOT run the SIO flamegraph capture concurrently with GALEN — Phase 3d's first measurement (+14% spurious regression) was contention from the parallel build+profile run. Run SIO flame in Step 2 FIRST, then GALEN in Step 3 only after Step 2 has completed.

```bash
timeout 1500 cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    galen_closure_matches_konclude \
    -- --exact --ignored --nocapture 2>&1 | tee /tmp/p3e-galen.log | grep -E "^--- galen|MISSED=|FP=|finished"
```

Expected: FP=0; MISSED=17; wall reduced from 11.87 min baseline.

- [ ] **Step 4: Triage**

Ship-or-revert criteria identical to Phase 3d:
- FP > 0 anywhere: REVERT.
- Wall regression > 5%: REVERT.
- Wall delta within ±3% AND flame % unchanged: REVERT (didn't fire).
- Wall delta within ±3% AND flame % dropped: SHIP with caveat.
- Wall reduction > 3%: SHIP cleanly.

No commit; T6 captures.

---

## Task 6: Results doc + envelope updates

Per Phase 3d T6 pattern.

- [ ] **Step 1: Write `docs/phase3e-results.md`** mirroring `docs/phase3d-results.md` shape.
- [ ] **Step 2: Archive SIO flame**: `cp /tmp/p3e-sio-flame.svg docs/flamegraphs/sio-classify-2026-06-01-post-phase3e.svg`.
- [ ] **Step 3: Update CLAUDE.md tableau bullet + design spec close-out**.
- [ ] **Step 4: Single commit**: `docs(phase3e): results doc + envelope updates`.

---

## Definition of done

- Structural canary passes; all regression suites pass.
- Phase 0 net FP=0 / MISSED=0 held.
- SIO `apply_role_rules` % meaningfully reduced from 16.36%.
- GALEN wall reduced (or, if unchanged, accepted with documented rationale).
- Results doc + CLAUDE.md + design spec updated.

## What this plan does NOT do

- Does NOT change verdicts on any corpus fixture.
- Does NOT touch `apply_max`, `apply_deferred_or_residuals`, `from_iter/collect` cluster.
- Does NOT change the env-flag defaults.
