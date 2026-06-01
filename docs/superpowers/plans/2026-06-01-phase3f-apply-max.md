# Phase 3f — `apply_max` Performance Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reduce `apply_max`'s contribution to GALEN classify wall (currently 14.34% of SIO post-3c per `docs/flamegraphs/sio-classify-2026-06-01-post-phase3c-findings.md`, likely similar post-3d) while preserving FP=0 + MISSED=17 on GALEN and FP=0 / MISSED=0 on the Phase 0 net. Hold to the lesson from Phase 3e dead-end §16: avoid optimizations whose break-even is workload-dependent in ways that regress GALEN.

**Architecture:** `apply_max` lives at `crates/owl-dl-tableau/src/rules.rs:882-966`. Phase 3b already attacked its inverse-edge path (`are_declared_inverses` O(N) → O(1) via HashSet); SIO went 27.93% → 6.51% then. The 14.34% post-3c residual is on the rest of the function: `maxes` Vec collection (filter_map labels), per-max `c_neighbours` collection (edge scan with `edge_satisfies` + `has_label` + linear dedup), the O(n²) pair-wise mergeability loop, and `compute_max_merge_deps`. T1 identifies the dominant inner sub-path; T3 designs the surgical fix; T4 implements; T5 measures.

**Tech Stack:** Rust (edition 2024), `owl-dl-tableau` crate, existing Phase 3b `inverse_pairs_set` HashSet, `pprof-rs` for flamegraph drill-down.

---

## Background the executor needs

- Phase 3 to date:
  - **3a** (64bee92): bloom prefilter on `needs_deferred_or`. SIO `apply_deferred_concept_or_rules` 31.40% → 22.28%.
  - **3b** (cf05e22): O(1) HashSet for `are_declared_inverses`. SIO `apply_max` 27.93% → 6.51% (the inverse-edge path). This is the most relevant precedent — Phase 3f targets what 3b left on the table.
  - **3c** (0b5ed36): `OnceLock` cache for `ConceptPool::bot_id`. GALEN wall 24.8 → 12.2 min.
  - **3d** (32aeda6 + d4f85f8): hoisted apply_deferred linear-scan fallback. SIO top-frame 18.16% → 3.23%; GALEN wall −4.5%. **Shipped.**
  - **3e** (89317e0, reverted at a2a4d7f): edge-keyed role-rule indexing. SIO win but GALEN +2.34%. **Reverted; dead-end §16.**
- Post-3c top non-search frames on SIO (likely shifted post-3d):
  | Rank | Frame | Post-3c % |
  |---|---|---|
  | 5 | `apply_role_rules` | 16.36% (Phase 3e reverted) |
  | 6 | `apply_max` | **14.34%** ← Phase 3f target |
  | 7 | `{closure#1}` | 13.41% (was matching_edges in apply_role_rules) |
  | 8 | `edge_satisfies` | 10.65% (leaf, multi-caller) |
  | 9 | `eq` | 8.81% |
- Phase 0 net (FP=0 / MISSED=0 gate): `alehif_closure_matches_konclude`, `ore_10908_sroiq`, `ore_15672_shoin`.
- GALEN baseline (post-3d clean + revert of 3e): ~12.33 min (per Phase 3e T2 baseline), MISSED=17, FP=0.
- The function `apply_max` at `crates/owl-dl-tableau/src/rules.rs:882-966`. Essential shape:
  ```rust
  pub fn apply_max(ctx, node) -> RuleOutcome {
      if ctx.is_blocked(node) { return NoChange; }
      let maxes: Vec<(u32, Role, ConceptId, DepSet)> = n.labels()
          .iter().enumerate()
          .filter_map(|(pos, &c)| match pool.get(c) {
              ConceptExpr::Max(count, role, body) => Some((*count, *role, *body, n.label_deps[pos].clone())),
              _ => None,
          })
          .collect();                                        // ALLOC #1
      for (n, role, body, max_deps) in maxes {
          let mut c_neighbours: Vec<NodeId> = Vec::new();    // ALLOC #2 (per max)
          for (seen, w) in ctx.graph().node(node).neighbours() {
              if ctx.edge_satisfies(seen, role)              // CALL site for edge_satisfies (10.65% leaf)
                  && ctx.graph().node(w).has_label(body)
                  && !c_neighbours.contains(&w)              // O(c_neighbours.len()) linear dedup
              {
                  c_neighbours.push(w);
              }
          }
          if c_neighbours.len() <= n { continue; }
          // O(c² ) pair loop:
          'pairs: for i in 0..c_neighbours.len() {
              for j in (i + 1)..c_neighbours.len() {
                  if !ctx.are_distinct(a, b) {               // distinctness check
                      let merge_deps = compute_max_merge_deps(...);  // separate function
                      if ctx.merge_into_with_deps(...) { applied=true; merged=true; break 'pairs; }
                  }
              }
          }
          if !merged && let Some(bot) = ctx.pool().bot_id() {
              // Clash insertion + active_branches binary_search insertions
          }
      }
  }
  ```
- The candidate inner hot paths (T1's enumeration target):
  | Candidate | Why hot | Fix shape |
  |---|---|---|
  | **A: `c_neighbours` per-max edge scan** (line 909-916) | One full edges-iter + `edge_satisfies` + `has_label` + linear `contains` per max constraint. Likely fires repeatedly per node when multiple maxes exist. | Hoist edge_satisfies once per (node, role) and reuse across maxes; OR collect candidates by role-AND-body once per node. |
  | **B: `edge_satisfies` per-edge-per-max** (10.65% summed leaf) | Same call cost as in apply_role_rules. Phase 3e tried HashMap indexing here and regressed on GALEN. | Workload-adaptive variant (per §16) OR a smaller scope: cache `edge_satisfies` results per (edge, role) for the duration of this `apply_max` call. |
  | **C: `maxes` Vec collection alloc** (line 887-900) | Allocates a Vec per call (per node). Cardinality typically small (max constraints per node). | Iterate inline (closure-driven); likely small win since Vec is short. |
  | **D: O(c²) pair-wise mergeability loop** (line 926-941) | Quadratic in `c_neighbours.len()`. Each pair checks `are_distinct` (O(1)? — verify) + possibly computes merge_deps + merges. | Order-of-magnitude depends on typical c_neighbours size; if c is usually ≤ 4, the loop is fine. If c is often large, restructuring (e.g. union-find) could help. |
  | **E: `compute_max_merge_deps`** (separate function at line 977) | DepSet union work. Called only when `!are_distinct` and a merge happens — likely modest aggregate. | Probably not target. |
  | **F: `c_neighbours.contains(&w)` linear dedup** (line 912) | O(c_neighbours.len()) per push. If c is large, O(c²). | HashSet for membership check; cheap if c can be > ~8. |

- The 3e dead-end §16 warning: any per-edge HashMap indexing has workload-dependent break-even. apply_max's edge-scan candidates A/B are RISKY by the same reasoning. Prefer candidates with no per-classify HashMap overhead.
- Counter pattern: `crates/owl-dl-tableau/src/counters.rs` already has `apply_max` (Phase 3b counter at line 882). Mirror.
- Diagnostics: `RUSTDL_TRACE=1`, `RUSTDL_COUNTERS=1` + `--features counters`.

**ENV NOTE for executor**: cargo and rustc are NOT on the default shell PATH:
```bash
export PATH=/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH
```

---

## Task 1: Drill-down recon

**Files:**
- Read: `docs/flamegraphs/sio-classify-2026-06-01-post-phase3d.svg`.
- Read: `crates/owl-dl-tableau/src/rules.rs:882-1020` (apply_max + compute_max_merge_deps).
- Create: `docs/phase3f-recon.md` (committed).

- [ ] **Step 1: Inspect post-3d SVG for `apply_max` inner frames**

```bash
grep -oE '<title>apply_max[^<]*</title>' docs/flamegraphs/sio-classify-2026-06-01-post-phase3d.svg | head -10
grep -oE '<title>[^<]*compute_max_merge_deps[^<]*</title>' docs/flamegraphs/sio-classify-2026-06-01-post-phase3d.svg | head -5
grep -oE '<title>[^<]*are_distinct[^<]*</title>' docs/flamegraphs/sio-classify-2026-06-01-post-phase3d.svg | head -5
grep -oE '<title>[^<]*merge_into_with_deps[^<]*</title>' docs/flamegraphs/sio-classify-2026-06-01-post-phase3d.svg | head -5
```

If the SVG doesn't have enough depth, capture a fresh focused flame:

```bash
export PATH=/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH
RUSTDL_PROFILE=/tmp/p3f-recon-sio.svg RUSTDL_PROFILE_SECONDS=90 \
    ./target/release/owl-dl-bench classify ontologies/real/sio-fp2-module.ofn 2>&1 | tail -5
```

(Build with `--features profile` first; that's already done from Phase 3d.)

- [ ] **Step 2: Code-trace `apply_max`**

Read `crates/owl-dl-tableau/src/rules.rs:882-1020` line-by-line. For each candidate (A-F), estimate share from flame + code-trace cost.

Specifically check:
- Typical `c_neighbours.len()` on GALEN — is it usually small (≤4) or sometimes large (~50)? Determines whether candidate D or F could dominate.
- Cost of `ctx.are_distinct(a, b)` — what does it do? (Read it.)
- Cost of `ctx.merge_into_with_deps()` — likely big when it fires but conditional on `!are_distinct`.

- [ ] **Step 3: Avoid the §16 trap**

If the dominant candidate is A or B (per-edge HashMap indexing), explicitly evaluate whether the fix would have workload-dependent break-even like Phase 3e did. The §16 warning: per-classify HashMap structures with per-edge probe costs can regress on GALEN's edge-heavy pattern.

If the dominant candidate is C, D, E, or F — the fix is more straightforward (alloc removal, dedup HashSet swap, loop-restructure). PROCEED.

If the dominant candidate is A or B — REPORT in the recon doc with explicit workload-dependence risk analysis. T3 may pivot to a different candidate or design the fix with workload-adaptive dispatch.

- [ ] **Step 4: Write `docs/phase3f-recon.md`**

```markdown
# Phase 3f recon — apply_max internals

Source: post-Phase-3d SIO flamegraph
(`docs/flamegraphs/sio-classify-2026-06-01-post-phase3d.svg`) + code-trace.
HEAD: 6b8a081.

## Flamegraph drill-down

<per-child sample counts / % for apply_max>

## Identified dominant inner cost

<which candidate (A-F) the flame supports; quantitatively>

## §16 risk analysis

<if dominant is A or B, explicit workload-dependence assessment;
otherwise note "candidate X is workload-neutral, no §16 risk">

## Code-trace evidence

<file:line refs; cardinality estimates for c_neighbours / maxes-per-node>

## Discarded candidates

<the others; brief justification>

## Proposed fix shape (handoff to T3)

<one paragraph; explicit about workload-adaptiveness if relevant>
```

- [ ] **Step 5: Commit**

```bash
git add docs/phase3f-recon.md
git commit -m "perf(phase3f): apply_max drill-down recon"
```

If the recon shows the 14.34% is split across 3+ sub-paths with no clear winner, OR if the dominant candidate is A/B and §16 risk is high, REPORT DONE_WITH_CONCERNS. Phase 3f may pivot to the `from_iter/collect` cluster (6.51%) which has a cleaner low-risk shape.

---

## Task 2: Lock baseline

```bash
export PATH=/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH
timeout 1500 cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    galen_closure_matches_konclude \
    -- --exact --ignored --nocapture 2>&1 | tee /tmp/p3f-baseline-galen.log | grep -E "^--- galen|MISSED=|FP=|finished"
```

Expected: FP=0, MISSED=17, wall ~12-13 min (post-3d + Phase-3e-revert range).

No commit.

---

## Task 3: Design

Mirror Phase 3d/3e design-doc shape. Create `docs/phase3f-fix-target.md` with target code, fix shape, soundness invariant, predicted impact (honest range), and what's NOT changed.

Explicit requirement from §16: if the fix involves new per-classify HashMap structures, the soundness section must include a workload-dependence risk subsection arguing why GALEN won't regress. If you can't make that argument, switch candidates.

Commit: `perf(phase3f): chosen fix target + soundness preconditions`.

---

## Task 4: Implement + structural canary

Mirror Phase 3d/3e T4 pattern.

- [ ] Counter in `counters.rs` (gate `cfg(feature = "counters")`).
- [ ] Surgical implementation per T3.
- [ ] Structural canary asserting counter bumps on minimal positive.
- [ ] Full regression sweep:
  ```bash
  export PATH=/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH
  cargo test -p owl-dl-tableau --features counters -- --test-threads=1 2>&1 | tail -10
  cargo test -p owl-dl-tableau -- --test-threads=1 2>&1 | tail -10
  cargo test -p owl-dl-saturation -- --test-threads=1 2>&1 | tail -5
  cargo test -p owl-dl-reasoner --lib -- --test-threads=1 2>&1 | tail -5
  RUSTFLAGS="-D warnings" cargo test -p owl-dl-tableau --no-run 2>&1 | tail -3
  cargo clippy -p owl-dl-tableau --all-targets -- -D warnings 2>&1 | grep -E "warning|error" | head -10
  ```
  All green; CI strict clean.
- [ ] Commit: `perf(tableau): <Phase 3f specific> (Phase 3f)`.

---

## Task 5: Corpus measurement (mirror Phase 3d T5)

**CRITICAL**: sequence SIO flame BEFORE GALEN. Do NOT run them concurrently (Phase 3d had +14% spurious from contention). Phase 3e's TWO consecutive GALEN samples is the right verification pattern — if first sample shows regression, run a second to confirm signal vs noise.

- [ ] Phase 0 net (30 min cap). FP > 0 → REVERT.
- [ ] SIO flamegraph capture: `RUSTDL_PROFILE=/tmp/p3f-sio-flame.svg RUSTDL_PROFILE_SECONDS=60`.
- [ ] Extract `apply_max` top-frame % from SVG.
- [ ] GALEN clean wall (25 min cap). If first sample is +>1.5%, run a second for noise-vs-signal disambiguation.
- [ ] Triage per criteria:
  - FP > 0 anywhere → REVERT.
  - Wall regression > 5% → REVERT.
  - Wall regression 2-5% on TWO samples → REVERT (per §16 lesson; signal, not noise).
  - Wall delta within ±2% AND flame % unchanged → REVERT (didn't fire).
  - Wall delta within ±2% AND flame % dropped → SHIP with caveat.
  - Wall reduction > 2% → SHIP cleanly.

No commit; T6 captures.

---

## Task 6: Results doc + envelope updates (or revert)

If T5 verdict is SHIP: mirror Phase 3d T6.
If T5 verdict is REVERT: mirror Phase 2c T6 / Phase 3e T6 — revert commit + docs commit + dead-end ledger §17.

Single docs commit shape per Phase 3a/b/c/d precedent. Update CLAUDE.md tableau bullet + design spec close-out.

---

## Definition of done

- Phase 0 net FP=0 / MISSED=0 held.
- GALEN MISSED=17 unchanged.
- Either SHIP cleanly (wall reduction >2%, flame win) or REVERT with dead-end ledger entry.
- Results doc + CLAUDE.md + design spec updated.

## What this plan does NOT do

- Does NOT touch the saturator or wedge engines.
- Does NOT change verdicts on any corpus fixture.
- Does NOT touch `from_iter/collect` cluster (6.51%, separate phase candidate).
- Does NOT change env-flag defaults.
