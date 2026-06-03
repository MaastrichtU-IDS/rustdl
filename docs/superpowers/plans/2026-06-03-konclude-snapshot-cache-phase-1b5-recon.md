# Konclude snapshot cache — Phase 1b.5 recon plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Determine whether lazy expansion of the snapshot replay path can deliver meaningful wall savings on GALEN. Phase 1b shipped full-re-run with +8.3% overhead vs flag-OFF baseline; Phase 1b.5's scoped intent is to drive flag-ON wall *below* flag-OFF by skipping redundant rule firings. Before committing 4-6 sessions to the implementation, **prove the architectural lever is viable** with measurement.

**Architecture (recon-driven):** Instrument the classify pipeline to extract (a) pairs-per-sub distribution reaching `subsumes_via_tableau` on GALEN, (b) per-call cost breakdown (cold-wedge vs snapshot-build vs full-re-run replay), then project lazy-expansion savings against the actual distribution. Decision criterion: lazy expansion ships if projected wall is ≤ flag-OFF baseline; otherwise pivot to a different lever (per-sup `neg_sup_clauses` caching, or dead-end ledger entry).

**Tech Stack:** Rust 1.88+. No new deps. Instrumentation is temporary — either kept (telemetry counters) or reverted after recon.

**Spec:** [docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md](../specs/2026-06-03-konclude-style-global-classification-design.md) §4.1 (lazy expansion design) + §6 Phase 1c outcome bands.

**Predecessor:** Phase 1b landed at `b4eab21..cb47751`. Results: `docs/phase1b-results.md`. GALEN flag-OFF baseline (this host): **148.95 s**. GALEN flag-ON: **161.31 s**. Closure: 27,997=Konclude.

---

## Why recon-first

The savings ceiling for Phase 1b.5 is small (~12s on a 149s baseline → ~8% reduction at best, ~0% at worst). The implementation cost is large (4-6 sessions: per-node fingerprints, gated re-seeding, per-sup caching, parent/parent_role capture, GALEN gate per task). Before committing that budget, we need evidence the lever exists.

The break-even logic: for lazy expansion to beat cold wedge, the savings from skipping redundant firings per replay must exceed the snapshot-build amortization cost. Specifically:

```
snapshot_build_cost(sub)     <  (pairs_per_sub - 1) × wedge_per_call_cost
                                  - pairs_per_sub × replay_per_call_cost
```

If `pairs_per_sub` is low (many subs only get 1-2 deep-path queries because Phase 7's label cache prunes the rest), the snapshot build doesn't amortize. We need the actual distribution to know.

---

## File structure (this plan)

**Modified files (temporary instrumentation):**
- `crates/owl-dl-reasoner/src/classify.rs` — add per-sub pair counter + cold-wedge cost histogram via `ClassificationStats`; emit in the CLI's classify banner.
- `crates/owl-dl-cli/src/main.rs` — surface the distribution stats in the `classify` subcommand output.

**New files (kept):**
- `docs/phase1b5-recon.md` — recon doc with distribution stats, cost projection, go/no-go recommendation.

If the recon outcome is "go", a follow-up `docs/superpowers/plans/2026-06-XX-konclude-snapshot-cache-phase-1b5.md` implementation plan is written. If "no-go", recon doc + dead-end ledger entry §19 close out the work.

---

### Task 1: Instrument pairs-per-sub + wedge-cost histogram

**Goal:** add minimal telemetry to `subsumes_via_tableau` so we can reconstruct (a) how many (sub, *) pairs reach the deep path for each sub on GALEN, (b) the cold-wedge cost distribution.

**Files:**
- Modify: `crates/owl-dl-reasoner/src/classify.rs` (extend `ClassificationStats` with two new fields; populate in `subsumes_via_tableau`; accumulate in the two merge loops)
- Modify: `crates/owl-dl-cli/src/main.rs` (emit distribution summary in the classify banner)

- [ ] **Step 1: Add fields to `ClassificationStats`**

In `crates/owl-dl-reasoner/src/classify.rs`, near the existing snapshot counters (added in Phase 1b), add:

```rust
/// Phase 1b.5 recon: per-sub count of pairs reaching
/// `subsumes_via_tableau`. Keyed by sub ClassId index. Used to
/// derive the pairs-per-sub distribution that determines whether
/// snapshot caching can amortize on a workload.
///
/// Temporary instrumentation — will be removed or formalized
/// depending on the recon outcome.
pub pairs_per_sub: std::collections::HashMap<u32, u32>,
/// Phase 1b.5 recon: cold-wedge per-call cost histogram, in
/// milliseconds. Bucket boundaries: 0, 1, 2, 5, 10, 20, 50, 100, ∞.
/// Reset per classify run.
pub wedge_cost_histogram_ms: [u64; 9],
```

Lean on the existing `HashMap` reasoning style — no new deps. The histogram is fixed-size (9 buckets), so it merges trivially.

- [ ] **Step 2: Populate in `subsumes_via_tableau`**

In `subsumes_via_tableau`, right after the wedge call returns (around line 1296, where `wedge_elapsed_ms` is computed):

```rust
// Phase 1b.5 recon telemetry. Bump the per-sub pair count and
// place wedge_elapsed_ms in the appropriate histogram bucket.
*stats.pairs_per_sub.entry(sub.index()).or_insert(0) += 1;
let bucket = match wedge_elapsed_ms {
    0 => 0,
    1 => 1,
    2..=4 => 2,
    5..=9 => 3,
    10..=19 => 4,
    20..=49 => 5,
    50..=99 => 6,
    100..=999 => 7,
    _ => 8,
};
stats.wedge_cost_histogram_ms[bucket] += 1;
```

The increment must happen ON EVERY pair that reaches the wedge — i.e., AFTER the snapshot-replay shortcut (which we want to bypass for measurement purposes). Important: set `RUSTDL_SNAPSHOT_CAPTURE=0` (default) for the recon run so the snapshot path doesn't pre-empt the wedge — we want the wedge cost distribution as if no snapshot existed.

Make sure the snapshot-replay shortcut at the top of `subsumes_via_tableau` doesn't increment this counter (it'd skew the per-sub distribution). Place the counter increment AFTER the snapshot-replay shortcut block but BEFORE the wedge call OR right after.

Actually cleaner: place it RIGHT BEFORE the wedge call (line ~1294), so it counts every pair that ACTUALLY runs the wedge. The snapshot-replay shortcut returning early means the wedge wasn't run, so no counter increment — that's the right semantics. (For the recon run with flag OFF, the shortcut never fires anyway, so this is a no-op concern.)

- [ ] **Step 3: Merge through the parallel classify shards**

Same as the d3c5598 fix for snapshot counters: the two per-shard merge loops in `classify_top_down_internal` (around lines 947 and 1077) need to fold the new fields:

```rust
for (k, v) in sd.pairs_per_sub {
    *stats.pairs_per_sub.entry(k).or_insert(0) += v;
}
for (i, c) in sd.wedge_cost_histogram_ms.iter().enumerate() {
    stats.wedge_cost_histogram_ms[i] += c;
}
```

- [ ] **Step 4: Surface in CLI classify banner**

In `crates/owl-dl-cli/src/main.rs`, find the existing `Command::Classify` handler. After the existing `# label heuristic:` line, add:

```rust
let p = &result.stats().pairs_per_sub;
if !p.is_empty() {
    let mut counts: Vec<u32> = p.values().copied().collect();
    counts.sort_unstable();
    let n = counts.len();
    let total: u64 = counts.iter().map(|&c| u64::from(c)).sum();
    let median = counts[n / 2];
    let p90 = counts[(n * 90) / 100];
    let p99 = counts[(n * 99).min(n * 100 - 1) / 100];
    let max = counts[n - 1];
    println!(
        "# pairs-per-sub: n_subs={n} total={total} median={median} p90={p90} p99={p99} max={max}"
    );
    let h = &result.stats().wedge_cost_histogram_ms;
    println!(
        "# wedge-cost-histogram ms (0|1|2-4|5-9|10-19|20-49|50-99|100-999|≥1000):"
    );
    println!(
        "#   {} | {} | {} | {} | {} | {} | {} | {} | {}",
        h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7], h[8]
    );
}
```

- [ ] **Step 5: Build + sanity-check on a small fixture**

```bash
export PATH=/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH
cargo build --release -p owl-dl-cli
./target/release/rustdl classify --pair-timeout-ms 200 ontologies/external/alehif-test.ofn 2>&1 | grep "pairs-per-sub\|wedge-cost"
```

Expected: shows distribution lines. If empty, the counter isn't firing — verify in `subsumes_via_tableau`.

- [ ] **Step 6: Commit**

```bash
git add crates/owl-dl-reasoner/src/classify.rs crates/owl-dl-cli/src/main.rs
git commit -m "$(cat <<'EOF'
recon(snapshot): instrument pairs-per-sub + wedge-cost histogram (Phase 1b.5)

Adds two diagnostic counters to ClassificationStats so the
Phase 1b.5 recon can decide whether lazy expansion of the snapshot
replay path will pay off on GALEN:
- pairs_per_sub: HashMap<u32 (sub ClassId), u32 (pair count)>
  — bumped on every (sub, sup) pair reaching subsumes_via_tableau.
- wedge_cost_histogram_ms: [u64; 9] — cold-wedge per-call cost
  in 9 buckets (0, 1, 2-4, 5-9, 10-19, 20-49, 50-99, 100-999, ≥1000).

Both merge through the two parallel classify shards (same pattern
as the d3c5598 snapshot-counter merge fix).

CLI classify banner surfaces n_subs/total/median/p90/p99/max for
pairs-per-sub and the 9-bucket histogram for wedge cost.

Temporary instrumentation — kept or removed depending on recon
outcome (see docs/phase1b5-recon.md).

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: Measure GALEN distribution + write recon doc

**Goal:** run the instrumented build on GALEN; extract the distribution; project lazy-expansion savings; write recon doc with go/no-go recommendation.

**Files:**
- Create: `docs/phase1b5-recon.md`

- [ ] **Step 1: Run GALEN classify with instrumentation, flag OFF**

```bash
mkdir -p /tmp/p1b5
export PATH=/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH
/usr/bin/time -f "wall=%es" ./target/release/rustdl classify --pair-timeout-ms 200 \
    ontologies/external/galen.ofn 2>&1 | tee /tmp/p1b5/galen-instrumented.log
```

Expected: classify completes (~150 s on a quiet host). The banner shows pairs-per-sub distribution + wedge-cost histogram.

Extract from the log:
- `n_subs`, `total`, `median`, `p90`, `p99`, `max` pairs-per-sub.
- 9-bucket wedge cost distribution.
- Total classify wall.

- [ ] **Step 2: Compute the break-even projection**

For each pairs-per-sub band, project whether snapshot caching saves wall:

```
For a sub with k pairs reaching subsumes_via_tableau:
- Cold wedge cost: k × wedge_cost(sub)
- Snapshot cache cost: snapshot_build(sub) + k × replay_cost(sub)

Snapshot wins iff: snapshot_build < (k - 1) × wedge_cost - k × (replay_cost - wedge_cost_per_warm_path)
```

For a first approximation: replay's full-re-run cost ≈ wedge cost (same horn fixpoint over the same labels), so Phase 1b first-cut has `replay_cost ≈ wedge_cost`. Lazy expansion targets bringing `replay_cost` down to a small fraction of `wedge_cost` by skipping re-firing on snapshot nodes. Assume optimistically that lazy expansion brings replay to 10% of wedge cost:

```
Break-even: snapshot_build < (k - 1) × wedge_cost - 0.1 × k × wedge_cost
         => snapshot_build / wedge_cost < 0.9 × k - 1
         => k > (snapshot_build / wedge_cost + 1) / 0.9
```

If `snapshot_build / wedge_cost ≈ 5` (snapshot build is ~5x more expensive than a single wedge call), break-even is `k > 6.7`. So subs with > 7 pairs each amortize.

Use the actual wedge cost histogram + assumption about `snapshot_build / wedge_cost` ratio (which can be measured separately with a small experiment if needed) to compute the projection.

- [ ] **Step 3: Write the recon doc**

Create `docs/phase1b5-recon.md`:

```markdown
# Phase 1b.5 recon — does lazy expansion pay off on GALEN?

Run 2026-06-XX at HEAD `<short-sha>`. Recon to decide whether the
Phase 1b.5 lazy-expansion implementation is worth building, given
Phase 1b's full-re-run only added +8.3% wall overhead vs flag-OFF
baseline on GALEN (148.95 → 161.31 s on this host).

## Headline

<one-sentence go/no-go>

## Pairs-per-sub distribution on GALEN

| Metric | Value |
|---|---:|
| Subs that reach subsumes_via_tableau | <n_subs> |
| Total pairs to deep path | <total> |
| Median pairs per sub | <median> |
| p90 | <p90> |
| p99 | <p99> |
| Max | <max> |

## Wedge cost distribution

| Bucket (ms) | Pair count | Cumulative % |
|---|---:|---:|
| 0 | <c0> | <pct0>% |
| 1 | <c1> | <pct1>% |
| 2-4 | <c2> | ... |
| 5-9 | <c3> | ... |
| 10-19 | <c4> | ... |
| 20-49 | <c5> | ... |
| 50-99 | <c6> | ... |
| 100-999 | <c7> | ... |
| ≥1000 | <c8> | ... |

Total wedge wall (sum of bucket-midpoint × bucket-count): <wedge_total> s.

## Break-even projection

Assume `snapshot_build_cost ≈ <ratio> × median_wedge_cost`. Optimistic
lazy-expansion ratio: `replay_cost ≈ 0.1 × wedge_cost`. Break-even
`pairs_per_sub` for amortization: <k_breakeven>.

Subs with `pairs ≥ <k_breakeven>` count: <amortizing_subs>.
Subs with `pairs < <k_breakeven>` (snapshot wasted): <wasted_subs>.

Projected wall:
- Amortizing path savings: <amortizing_subs> × avg_savings_per_sub = <save_amount> s.
- Wasted-path overhead: <wasted_subs> × snapshot_build_overhead = <waste_amount> s.
- Net projected delta: <save - waste> s.

Phase 1b baseline (flag ON full-re-run): +12 s vs flag-OFF.
Phase 1c target (per spec §6): GALEN ≤ 150 s — net delta must be
≤ +1 s from current 149 s flag-OFF baseline.

## Recommendation

<one of:>

**GO: ship Phase 1b.5 lazy expansion.** Projected savings (<save> s)
clearly beat the +12 s Phase 1b overhead; even with pessimistic
assumptions about lazy-expansion ratio (replay at 30% of wedge cost
instead of 10%), savings remain positive. Implementation plan:
`docs/superpowers/plans/2026-06-XX-konclude-snapshot-cache-phase-1b5.md`
(written after this recon lands).

OR

**NO-GO: pivot to per-sup `neg_sup_clauses` caching.** Most subs
have only <median> pairs, so snapshot caching wastes more on
build-amortization than it saves on replay. Per-sup caching of the
`fresh_q ⊓ sup → ⊥` clause across all subs (one cache entry per
sup column instead of per sub row) is the cheaper lever. Plan:
`docs/superpowers/plans/2026-06-XX-konclude-per-sup-caching.md`.

OR

**DEAD-END: snapshot caching cannot deliver Phase 1c headline on
this engine architecture.** Pairs-per-sub distribution doesn't
support amortization; cold-wedge wall is already too low (~149 s
on GALEN under low contention) for the algorithmic improvement
to matter. Dead-end ledger §19: snapshot caching is sound and
shippable for SROIQ workloads where wedge cost dominates pair
count, but for Horn-fragment performance on GALEN-scale workloads
the per-pair wedge is already near-optimal. Recommend leaving
Phase 1b snapshot infrastructure in place for SROIQ Phase 3 work
but defer the Phase 1c default-on flip indefinitely.

## What's deferred

- Empirical measurement of `snapshot_build / wedge_cost` ratio (used
  the rough estimate above; can be measured precisely with a small
  microbenchmark if the projection is borderline).
- notgalen + sio-stripped distributions (run the same instrumentation
  if GALEN projection is borderline; they may have very different
  pairs-per-sub shapes).

## Cross-references

- Project spec: `docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md`.
- Phase 1b results: `docs/phase1b-results.md`.
- Phase 1b.5 recon plan (this work): `docs/superpowers/plans/2026-06-03-konclude-snapshot-cache-phase-1b5-recon.md`.
```

Fill in all `<placeholders>` with real values from the log + projection calculation. The recommendation section picks ONE of the three options.

- [ ] **Step 4: Commit recon doc**

```bash
git add docs/phase1b5-recon.md
git commit -m "$(cat <<'EOF'
docs(phase1b5): recon — does lazy expansion pay off on GALEN?

Measurement-driven recon for the Phase 1b.5 lazy-expansion
implementation decision. Instrumented GALEN classify (commit
<task-1-sha>) extracts pairs-per-sub distribution + cold-wedge
cost histogram. Break-even projection vs Phase 1c target.

Recommendation: <go / no-go / dead-end>.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: Follow-up based on recommendation

**Goal:** depending on the recon's conclusion, either (a) write the Phase 1b.5 implementation plan, (b) write the per-sup caching plan, or (c) write the dead-end §19 entry.

Single step:

- [ ] **Step 1: Take the action the recon doc recommends**

- If **GO**: write `docs/superpowers/plans/2026-06-XX-konclude-snapshot-cache-phase-1b5.md` covering per-node fingerprints, gated re-seeding, neg_sup_clauses caching, parent/parent_role capture. ~5-6 tasks. Dispatch via subagent-driven development.
- If **NO-GO** (pivot to per-sup caching): write `docs/superpowers/plans/2026-06-XX-konclude-per-sup-caching.md`. Smaller scope (~3 tasks): factor `clauses_for_sub` into per-sup pre-extension, cache the result, route through `try_replay`. Re-measure GALEN.
- If **DEAD-END**: write the `docs/hypertableau-dead-ends.md` §19 entry capturing the pairs-per-sub blocker. Optionally also revert the instrumentation from Task 1 (or keep it as kept telemetry for future ontology profiling).

Commit the resulting artifact and report back so the user can authorize the next phase.

---

## After all tasks

Recon complete. The recon doc + follow-up artifact give the user a clear decision frame for whether to continue investing in snapshot caching for Phase 1c, pivot to a different lever, or close the project.

Phase 1b's snapshot infrastructure (types, capture, replay, sentinel, cache) is sound and tested — it remains shippable for the SROIQ Phase 3 work regardless of the recon outcome, since per-class snapshot reuse on Unsafe-classified seeds is what Phase 3 unlocks. The recon only governs the Phase 1c default-on decision on Horn workloads.
