# Konclude snapshot cache — Phase 2a recon plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Measure where GALEN's 153.70s classify wall actually goes post-Phase-1c. Spec §5 hypothesized Phase 7 label-cache build is ~30% of wall (~50s); we now ALSO build a per-class snapshot cache (added in Phase 1b) — total per-class wedge work has potentially doubled. Recon decides whether Layer 2 (global saturation candidate filter) is the right lever, or whether a different optimization (e.g., shared per-class wedge between label + snapshot caches) is cheaper.

**Architecture (recon-driven):** Add per-component wall-time instrumentation to `classify_top_down_internal`: separately measure (a) label-cache build, (b) snapshot-cache build, (c) snapshot-replay calls (cumulative), (d) tier walk / orchestrator overhead. Run GALEN; emit breakdown via CLI banner. Recon doc: where does the 153.70s actually go? Decision frame: ship Phase 2 Layer 2 as scoped, pivot to "share per-class wedge across caches", or close as dead-end §19.

**Tech Stack:** Rust 1.88+. No new deps. Pure instrumentation; revertable depending on outcome.

**Spec:** [docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md](../specs/2026-06-03-konclude-style-global-classification-design.md) §5 (Layer 2 design intent) + §6 (Phase 2a recon row).

**Predecessor:** Phase 1c shipped at `5db819a..1090595`. Results: `docs/phase1c-results.md`. GALEN default-on wall: 153.70s, FP=0/MISSED=0. Spec §6 outcome band: 150-300s → "Ship + mandatory Phase 2 build" (Phase 2 is committed; recon just decides the SHAPE).

---

## Why recon-first

Spec §5 explicitly defers detailed Layer 2 design: "The shape of this filter depends on what Phase 1 measurements show." Phase 1 results put GALEN at 153.70s (the middle case in spec §5's branch: "150-300s and recon shows the residual wall is in label-cache-build → the filter is the obvious next lever").

But Phase 1b added a second per-class wedge call (snapshot-cache build) that didn't exist when spec §5 was written. Spec §5's ~30% label-cache-build estimate is now possibly an UNDERESTIMATE — both caches together could be ~60% of wall, OR the snapshot-build and label-build phases overlap on something the recon can identify and share.

The recon answers two questions:
1. **What's the per-component wall breakdown on GALEN today?** Need this to confirm spec §5's hypothesis or update it.
2. **Can Layer 2 actually replace both per-class wedge calls?** Or does it only replace one (label cache), leaving the snapshot-cache build cost in place?

Without the breakdown, we'd be flying blind into a multi-session implementation — same risk as Phase 1b.5 recon caught.

---

## File structure (this plan)

**Modified files (temporary instrumentation, kept or reverted post-recon):**
- `crates/owl-dl-reasoner/src/classify.rs` — add 4 wall-time fields to `ClassificationStats`: `label_cache_build_wall_ms`, `snapshot_cache_build_wall_ms`, `snapshot_replay_wall_ms`, `tier_walk_wall_ms`. Populate via `Instant::now()` at the right call sites.
- `crates/owl-dl-cli/src/main.rs` — emit a `# wall breakdown:` banner line in the classify output.

**New files (kept):**
- `docs/phase2a-recon.md` — recon doc with breakdown table + go/no-go/pivot recommendation.

If outcome is GO: follow-up Phase 2b implementation plan written (separate doc).
If outcome is PIVOT (e.g., "share wedge across caches"): follow-up Phase 2-alt plan.
If outcome is DEAD-END: dead-end ledger §19 entry.

---

### Task 1: Wall-time instrumentation

**Goal:** add 4 per-component wall-time counters to `ClassificationStats`. Populate at call sites. Surface in CLI banner.

**Files:**
- Modify: `crates/owl-dl-reasoner/src/classify.rs`
- Modify: `crates/owl-dl-cli/src/main.rs`

- [ ] **Step 1: Add fields to `ClassificationStats`**

In `crates/owl-dl-reasoner/src/classify.rs`, near the Phase 1b.5 recon counters (`pairs_per_sub`, `wedge_cost_histogram_ms`), add:

```rust
/// Phase 2a recon: cumulative wall time spent building the Phase 7
/// label cache (per-class wedge calls + risk classification +
/// post-processing). Measured at the call site in
/// `classify_top_down_internal`. Diagnostic only.
pub label_cache_build_wall_ms: u64,
/// Phase 2a recon: cumulative wall time spent building the Phase 1b
/// snapshot cache (per-class wedge calls at `SnapshotCache::build`
/// + lazy per-sub snapshot population via `get_or_build_snapshot`).
/// Diagnostic only.
pub snapshot_cache_build_wall_ms: u64,
/// Phase 2a recon: cumulative wall time spent inside
/// `replay_with_neg_sup` calls (per-pair replay work, lazy or
/// full-rerun). Sum over all pairs reaching subsumes_via_tableau
/// with the snapshot path active. Diagnostic only.
pub snapshot_replay_wall_ms: u64,
/// Phase 2a recon: cumulative wall time spent inside the top-down
/// tier walk + per-pair orchestration (excluding the four cache
/// builds + replay measured above). Captures the residual
/// orchestrator overhead — pair enumeration, label-cache lookup,
/// wedge invocations that DON'T hit the snapshot path, etc.
/// Diagnostic only.
pub tier_walk_wall_ms: u64,
```

### Step 2: Populate at the right call sites

Locate `classify_top_down_internal` in `classify.rs`. Identify the four phases:

1. **Label cache build**: the existing `(0..n).into_par_iter().map(|i| { ... prepared.classify_labels(class_id, deadline) ... }).collect()` block. Wrap with `Instant::now()`/`elapsed()` and add to `stats.label_cache_build_wall_ms`.

2. **Snapshot cache build**: triggered eagerly inside `PreparedOntology::from_internal` via `SnapshotCache::build(internal)`. Measure inside `from_internal` and thread the value back through `prepared` (or add a recon-specific field to `PreparedOntology`). Alternative: measure the snapshot-build's hot loop in `SnapshotCache::get_or_build_snapshot` and aggregate into the stats post-classify.

   **Simpler approach**: measure at the orchestrator's entry into the `find_direct_parents_top_down` loop, take "before" timestamp, subtract label-cache-build time and tier-walk time at the end. But this is fragile.

   **Cleanest approach**: add a `pub(crate) build_wall_ms: AtomicU64` field to `SnapshotCache`; bump it inside `get_or_build_snapshot` whenever a snapshot is built (not when it's a cache hit). Expose via accessor; read at classify end into `stats.snapshot_cache_build_wall_ms`.

3. **Snapshot replay**: similarly, add an `AtomicU64` to `SnapshotCache` tracking cumulative `replay_with_neg_sup`/`replay_with_neg_sup_full_rerun` wall. Each `try_replay` call wraps the replay invocation with timing and bumps.

4. **Tier walk wall**: the entire `classify_top_down_internal` body's wall, minus the three above. Measure top-level wall at entry/exit; subtract the others to derive tier-walk wall.

Implementer judgment: pick whichever combination of these patterns is least invasive. `AtomicU64` per-cache is the cleanest fit because the caches are already `Arc`-shared across rayon workers. Measurement overhead must be cheap (one `Instant::now()` per call site).

### Step 3: Surface in CLI banner

In `crates/owl-dl-cli/src/main.rs`, find the `write_classification` function (where existing banner lines like `# label heuristic:` are emitted). Add:

```rust
let s = result.stats();
println!(
    "# wall breakdown ms: label_cache_build={} snapshot_cache_build={} snapshot_replay={} tier_walk={}",
    s.label_cache_build_wall_ms,
    s.snapshot_cache_build_wall_ms,
    s.snapshot_replay_wall_ms,
    s.tier_walk_wall_ms
);
```

### Step 4: Smoke check on alehif

```bash
export PATH=/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH
cargo build --release -p owl-dl-cli
./target/release/rustdl classify --pair-timeout-ms 200 ontologies/external/alehif-test.ofn 2>&1 | grep "wall breakdown"
```

Expected: a banner line with non-zero numbers in at least 2 of 4 fields. Sum should be roughly ≤ total wall (some overhead in tier_walk is normal; if the sum noticeably EXCEEDS total wall, the same time is being double-counted somewhere).

### Step 5: Soundness gate on Phase 0 net

```bash
cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    -- --ignored --nocapture alehif_closure_matches_konclude \
    ore_10908_sroiq ore_15672_shoin 2>&1 | tee /tmp/p2a/soundness-instrumented.log
```

Expected: FP=0/MISSED=0 across all three. Instrumentation is additive; behavior must not change.

### Step 6: Commit

```bash
mkdir -p /tmp/p2a
git add crates/owl-dl-reasoner/src/classify.rs crates/owl-dl-reasoner/src/lib.rs crates/owl-dl-cli/src/main.rs
git commit -m "$(cat <<'EOF'
recon(snapshot): per-component wall-time instrumentation (Phase 2a)

Adds four cumulative wall-time counters to ClassificationStats so
the Phase 2a recon can attribute GALEN's 153.70s classify wall to
its components:
- label_cache_build_wall_ms (Phase 7 wedge-per-class build)
- snapshot_cache_build_wall_ms (Phase 1b wedge-per-class snapshot build)
- snapshot_replay_wall_ms (Phase 1b.5 per-pair replay cumulative)
- tier_walk_wall_ms (residual orchestrator overhead)

SnapshotCache adds AtomicU64 timers, bumped at get_or_build_snapshot
and try_replay sites. classify_top_down_internal wraps the label-
cache-build loop with Instant timing. CLI surfaces all four in a
`# wall breakdown ms:` banner line.

Temporary instrumentation — kept or removed based on Phase 2a
recon outcome.

Spec: docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md §5

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: Measure GALEN + write recon doc

**Goal:** run the instrumented build on GALEN; extract per-component breakdown; write recon doc with go/no-go/pivot recommendation for Phase 2 implementation.

**Files:**
- Create: `docs/phase2a-recon.md`

- [ ] **Step 1: Run GALEN classify with instrumentation**

```bash
export PATH=/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH
mkdir -p /tmp/p2a
/usr/bin/time -f "wall=%es" ./target/release/rustdl classify --pair-timeout-ms 200 \
    ontologies/external/galen.ofn 2>&1 | tee /tmp/p2a/galen-instrumented.log
```

Foreground, wait for completion (~150s).

Extract the `# wall breakdown ms:` line. Compute percentages of total wall (recall: wall is multi-threaded; component CPU times sum to much more than wall when divided by concurrency).

### Step 2: Run with `RUSTDL_SNAPSHOT_CAPTURE=0` for comparison

```bash
RUSTDL_SNAPSHOT_CAPTURE=0 /usr/bin/time -f "wall=%es" \
    ./target/release/rustdl classify --pair-timeout-ms 200 \
    ontologies/external/galen.ofn 2>&1 | tee /tmp/p2a/galen-noflag.log
```

With snapshot OFF, the snapshot_cache_build_wall_ms + snapshot_replay_wall_ms fields should be 0; this isolates the label-cache-build cost as a fraction of pre-snapshot wall.

### Step 3: Compute the recon answer

Build a small table:

| Component | Wall (default-on, ms) | Wall (flag-OFF, ms) | % of total |
|---|---:|---:|---:|
| label_cache_build | <a> | <b> | <a/total>% |
| snapshot_cache_build | <c> | 0 | <c/total>% |
| snapshot_replay | <d> | 0 | <d/total>% |
| tier_walk + other | <e> | <f> | <e/total>% |
| **Total wall** | **<total>** | **<noflag_total>** | 100% |

Key questions to answer:
- Is `label_cache_build` ~30% of wall as spec §5 estimated? If yes, the spec's hypothesis stands.
- Is `snapshot_cache_build` significant (say > 10% of wall)? If yes, Phase 2 should target BOTH cache builds, not just label cache.
- Is `tier_walk + other` the dominant cost? If yes, Layer 2 saturation filter won't help; need a different lever.

### Step 4: Write recon doc

Create `docs/phase2a-recon.md`. Structure mirrors `docs/phase1b5-recon.md`:

- Headline: GO / PIVOT / DEAD-END.
- Per-component breakdown table.
- Comparison: default-on vs flag-OFF (isolates snapshot cost).
- Break-even projection for Layer 2 saturation filter (vs. label-cache-build cost).
- Recommendation with concrete next-plan path:
  - **GO**: Phase 2b implementation plan replaces label_cache_build with one global saturation pass. Optionally also replaces snapshot_cache_build if the saturation closure subsumes both.
  - **PIVOT to share wedge**: if label-cache-build and snapshot-cache-build are both significant, propose sharing the per-class wedge call between them. Saves one cache-build pass without a saturation rewrite. Smaller scope, lower risk.
  - **DEAD-END**: if tier_walk dominates and cache-build is < 10% of wall, Layer 2 can't deliver the ≤150s headline. Close project; dead-end §19 entry.

### Step 5: Commit recon doc

```bash
git add docs/phase2a-recon.md
git commit -m "$(cat <<'EOF'
docs(phase2a): recon — where does GALEN's 153.70s actually go?

Measurement-driven recon for Phase 2 implementation shape. Per-
component wall breakdown on GALEN (default-on vs flag-OFF) shows
<headline>. Spec §5's Layer 2 design hypothesized label-cache-build
is ~30% of wall; actual breakdown is <breakdown summary>.

Recommendation: <GO / PIVOT / DEAD-END>.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## After all tasks

Recon outcome determines next plan:

- **GO → Phase 2b implementation plan**: ship Layer 2 global saturation filter. Replace per-class wedge calls in label-cache-build with a single TBox saturation pass that produces per-class candidate sup-sets.
- **PIVOT to share-wedge**: smaller-scope plan to merge the label-cache and snapshot-cache wedge calls into one per class (savings = ~half of cache-build cost without saturation rewrite).
- **DEAD-END**: project closes here. Phase 1c is the headline; Phase 2 doesn't pay off. Dead-end ledger §19 captures the conclusion.

Phase 3 (loosen `BackPropRisk` classifier for SROIQ workloads) is independent of Phase 2's outcome — can be scoped separately whenever the SROIQ wall gap is the priority.
