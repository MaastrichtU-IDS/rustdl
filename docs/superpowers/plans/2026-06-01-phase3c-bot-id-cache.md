# Phase 3c — ConceptPool::bot_id Cache Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate the `apply_role_axioms` / `bot_id` / `find_map` flame cluster (24.66% on SIO post-Phase-3b) by caching `bot_id` in `ConceptPool` rather than re-scanning the pool on every call, while preserving FP=0 + the post-Phase-3b baseline.

**Architecture:** Single-crate change in `crates/owl-dl-core/src/ir.rs` (`ConceptPool`). `bot_id()` at `ir.rs:230` currently does `iter_with_ids().find_map(|...| matches!(e, ConceptExpr::Bot))` — a linear scan over EVERY interned ConceptExpr (thousands on GALEN). The Phase 3b post-flame attributed 24.66% to the `apply_role_axioms` cluster, and `bot_id()` is the inner hot work. Cache it as a `Cell<Option<ConceptId>>` populated lazily on first call (or eagerly at construction). All 6 call sites in `crates/owl-dl-tableau/` and `crates/owl-dl-reasoner/` get O(1) lookup unchanged.

**Tech Stack:** Rust (edition 2024), `owl-dl-core` crate, existing per-Phase counter pattern in `owl-dl-tableau::counters`.

---

## Background the executor needs

- The flamegraph attribution: post-Phase-3b SIO + GALEN flame both showed `apply_role_axioms` / `bot_id` / `find_map` cluster at 24.66% (the largest non-search frame after Phase 3b eliminated the inverse-pair scan). See `docs/flamegraphs/sio-classify-2026-06-01-post-phase3b-findings.md` and the Phase 3b results doc for the exact percentages.
- The hot function: `crates/owl-dl-core/src/ir.rs:230-238`:
  ```rust
  pub fn bot_id(&self) -> Option<ConceptId> {
      self.iter_with_ids().find_map(|(id, e)| {
          if matches!(e, ConceptExpr::Bot) { Some(id) } else { None }
      })
  }
  ```
- 6 call sites (all in hot saturation paths, called per-node per-saturation):
  - `crates/owl-dl-tableau/src/rules.rs:929` (apply_max merge-failed path).
  - `crates/owl-dl-tableau/src/rules.rs:1072` (nominal merge-failed path).
  - `crates/owl-dl-tableau/src/rules.rs:1333` (apply_choose).
  - `crates/owl-dl-tableau/src/rules.rs:1385` (apply_role_axioms — the flame's main attribution).
  - `crates/owl-dl-reasoner/src/lib.rs:1950` (classify).
  - Plus a doc-comment reference at `crates/owl-dl-reasoner/src/lib.rs:1366` describing the call as "cheap" — that comment is empirically wrong post-Phase-3b and needs to be either fixed or made accurate by the cache.
- `ConceptPool` is in `crates/owl-dl-core/src/ir.rs`. Constructor is `ConceptPool::new()` (line 192-ish per Step-1's grep). The pool interns `ConceptExpr` values; `ConceptExpr::Bot` is one variant (a unit, no payload).
- Existing counter infrastructure mirror: `crates/owl-dl-tableau/src/counters.rs` `RuleCounters` has `inverse_pair_fast_hits` (Phase 3b), `needs_deferred_or_bloom_rejects` (Phase 3a), all `Cell<u64>`. Phase 3c adds `bot_id_cache_hits` matching that pattern. NOTE: ConceptPool lives in `owl-dl-core`, not `owl-dl-tableau`, so the counter is a separate concern — see Task 1.
- The Phase 3a/3b cadence: TDD canary first (forces gap; gates fix); fix; re-flame + measure; docs.

---

## Task 1: TDD canary + counter

**Files:**
- Modify: `crates/owl-dl-core/src/ir.rs` (the canary test module).
- Modify (for the structural counter): TBD — see Step 1's decision.

The structural canary asserts the cache is consulted, not the scan. The verdict canary asserts `bot_id()` returns the same value before/after the cache (a basic equivalence test).

- [ ] **Step 1: Decide where the structural counter lives**

`bot_id()` is in `owl-dl-core::ir`; the counter infrastructure is in `owl-dl-tableau::counters`. Three options:

(a) Add a `Cell<u64>` field directly on `ConceptPool` (e.g. `bot_id_cache_hits: Cell<u64>`). The pool tracks its own metric. Reasoner / tableau read it via a `bot_id_cache_hits()` accessor. Simplest; keeps the metric local to the cache.

(b) Add the counter to `owl-dl-tableau::counters::RuleCounters` and bump from the callers (apply_role_axioms etc.). Bumps "tableau-caller-side counts of bot_id calls" — different semantic than "pool cache hits" but matches existing counter conventions.

(c) Skip the structural canary; rely on the verdict canary + the re-flamegraph to confirm the cache is consulted. The verdict canary still verifies semantic equivalence (cache returns the same answer as scan).

**Recommended: (a)** — keep the metric where the data structure lives. Counter is owl-dl-core-local; no cross-crate coupling for the test. If (a) is awkward (e.g. ConceptPool doesn't already use `Cell`), fall back to (c) and rely on the re-flamegraph as the structural verification.

- [ ] **Step 2: Find the existing `mod tests` in `crates/owl-dl-core/src/ir.rs`**

```bash
grep -nE "^#\\[cfg\\(test\\)\\]|^mod tests" crates/owl-dl-core/src/ir.rs | head -3
```
Identify the test module. If none exists, add one at the bottom of the file (mirror other `owl-dl-core` modules' test pattern).

- [ ] **Step 3: Write the verdict canary (passes pre-fix; semantic equivalence)**

```rust
#[test]
fn phase3c_bot_id_returns_same_before_and_after_cache_population() {
    let mut pool = ConceptPool::new();
    // Pre-fix: bot_id() linearly scans. Post-fix: first call scans
    // + caches; subsequent calls hit the cache. Either way, the
    // returned ConceptId (or None) is the same.
    let first = pool.bot_id();
    // Intern some non-Bot concepts so the pool has multiple entries.
    let c0 = pool.atomic(ClassId::new(0));
    let c1 = pool.atomic(ClassId::new(1));
    let _and = pool.and(vec![c0, c1]);
    // Without explicitly interning Bot, both calls should agree.
    let second = pool.bot_id();
    assert_eq!(first, second, "bot_id() must return same value before/after pool growth");

    // Now intern Bot explicitly. Both calls should return the same Some(id).
    let bot_first = {
        let b = pool.bot();
        pool.bot_id()
    };
    let bot_second = pool.bot_id();
    assert!(bot_first.is_some(), "after pool.bot(), bot_id() must be Some");
    assert_eq!(bot_first, bot_second, "subsequent bot_id() calls must return the cached value");
}
```

Confirm the `pool.bot()` constructor (or whatever creates a `ConceptExpr::Bot`) exists with `grep -nE "pub fn bot\b" crates/owl-dl-core/src/ir.rs`. If it's named differently, adapt.

- [ ] **Step 4: Write the structural canary (gated; option (a) shape)**

If option (a) was chosen in Step 1, the structural canary asserts `bot_id_cache_hits()` accessor increments correctly:

```rust
#[cfg(feature = "counters")]  // mirror the owl-dl-tableau gating if owl-dl-core has a counters feature; otherwise drop the cfg
#[test]
fn phase3c_bot_id_cache_hits_counter_bumps_on_repeat_calls() {
    let mut pool = ConceptPool::new();
    let _ = pool.bot();
    let before = pool.bot_id_cache_hits();
    let _ = pool.bot_id();
    let _ = pool.bot_id();
    let _ = pool.bot_id();
    let after = pool.bot_id_cache_hits();
    assert!(
        after > before,
        "bot_id_cache_hits should increment on cached calls; before={before} after={after}"
    );
}
```

If `owl-dl-core` doesn't have a `counters` feature (likely true), DROP the `#[cfg(feature = "counters")]` line and let the test be unconditional. If the cost of always tracking the counter is concerning, gate the COUNTER INCREMENT itself with `#[cfg(debug_assertions)]` or a runtime-toggled flag — but that's adding scope. Simplest is unconditional `Cell::set/get`; cost is negligible.

If you chose option (c) in Step 1, SKIP this step and rely on the re-flamegraph for structural verification.

- [ ] **Step 5: Run, expect partial results**

```bash
cargo test -p owl-dl-core phase3c_ -- --test-threads=1 2>&1 | tail -10
```

Expected:
- Verdict canary `phase3c_bot_id_returns_same_before_and_after_cache_population`: PASSES pre-fix (the scan returns the right value; the cache isn't needed for correctness).
- Structural canary (if option a): FAILS TO COMPILE — `no method 'bot_id_cache_hits' on ConceptPool`. That's the gap Task 2 closes.

If the verdict canary FAILS, the test itself is buggy — fix it before proceeding.

- [ ] **Step 6: CI strictness clean**

```bash
RUSTFLAGS="-D warnings" cargo test -p owl-dl-core --no-run 2>&1 | tail -3
```
Expected: clean if structural canary is gated; compile-fails if structural is unconditional. Either way, default `cargo test` passes the verdict canary.

If the structural canary is unconditional and fails to compile, that's intentional — the next task closes it. Adjust: gate the structural canary with `#[cfg(test_phase3c_post)]` or similar so the default build is clean, OR commit only the verdict canary first.

Simplest path: commit ONLY the verdict canary in this task; add the structural canary in Task 2's same commit as the cache implementation. This avoids the temporarily-broken-default-build state.

- [ ] **Step 7: Commit**

```bash
git add crates/owl-dl-core/src/ir.rs
git commit -m "test(ir): Phase 3c verdict canary for bot_id cache equivalence"
```

---

## Task 2: Implement the cache + structural canary

**Files:**
- Modify: `crates/owl-dl-core/src/ir.rs` (add cache field + update bot_id).

The fix: lazy cache in `Cell<Option<ConceptId>>`. First call scans + populates; subsequent calls read cache.

There's ONE invariant subtlety: if `bot_id()` is called BEFORE `ConceptExpr::Bot` is interned, the cache populates to `None`. If Bot is then interned LATER, the cache is stale. The simplest fix: invalidate the cache (set to `None`) whenever the pool grows a new ConceptExpr — OR, more cheaply, only populate the cache when the result is `Some`. The latter is correct: `bot_id()` returns `None` until Bot is interned, then returns and caches `Some(id)`; subsequent calls (even after more growth) read the cached `Some(id)` correctly because Bot's id never changes once assigned.

- [ ] **Step 1: Add the cache field to `ConceptPool`**

Find `pub struct ConceptPool { ... }` (around line 187 per Step-1's grep). Add a new field:

```rust
    /// Phase 3c: cached `ConceptId` of `ConceptExpr::Bot`. `Some(id)`
    /// after the first `bot_id()` call that found Bot in the pool;
    /// stays `None` until then. Bot is a unit variant interned at
    /// most once, so once cached the id is stable. Eliminates the
    /// O(n) `iter_with_ids().find_map(...)` scan that the
    /// `apply_role_axioms` cluster on GALEN/SIO attributed at
    /// 24.66% of post-Phase-3b classify cost. See
    /// `docs/phase3c-fix-target.md` (or this plan).
    bot_id_cache: std::cell::Cell<Option<ConceptId>>,
```

If `std::cell::Cell` isn't already imported, add the `use` or qualify with the full path. Most pool fields are non-Cell (mutated through `&mut self`); the cache is `Cell` because `bot_id(&self)` is a read-only borrow.

- [ ] **Step 2: Initialize the cache in `ConceptPool::new()`**

If the constructor explicitly initializes fields (not `#[derive(Default)]`), add:

```rust
    bot_id_cache: std::cell::Cell::new(None),
```

If the struct uses `#[derive(Default)]`, no constructor change is needed (`Cell<Option<_>>::default() == Cell::new(None)`).

- [ ] **Step 3: Add the cache-hits counter (option (a) from Task 1 Step 1)**

If you chose option (a) in Task 1:

```rust
    /// Phase 3c: per-call counter for bot_id cache hits. Bumped each
    /// time bot_id() returns the cached value without scanning. Used
    /// by the structural canary to confirm the cache is consulted.
    bot_id_cache_hits: std::cell::Cell<u64>,
```

Initialize to 0 in the constructor (same pattern as `bot_id_cache`).

Add the accessor:

```rust
    /// Phase 3c: read the bot_id cache-hit counter (test-facing).
    #[must_use]
    pub fn bot_id_cache_hits(&self) -> u64 {
        self.bot_id_cache_hits.get()
    }
```

- [ ] **Step 4: Update `bot_id()` to use the cache**

Replace the existing function body at `crates/owl-dl-core/src/ir.rs:230-238`:

```rust
pub fn bot_id(&self) -> Option<ConceptId> {
    // Phase 3c: cache-or-scan. Bot is a unit variant interned at
    // most once; once cached as Some(id), the id is stable forever
    // (the pool doesn't re-intern or remove). If the cache is still
    // None, we scan; if the scan finds Bot, we populate the cache
    // (subsequent calls hit). If the scan returns None (Bot not yet
    // interned), we leave the cache None — a later call after
    // interning Bot will scan again, find it, and populate then.
    if let Some(cached) = self.bot_id_cache.get() {
        self.bot_id_cache_hits.set(self.bot_id_cache_hits.get() + 1);
        return Some(cached);
    }
    let found = self.iter_with_ids().find_map(|(id, e)| {
        if matches!(e, ConceptExpr::Bot) { Some(id) } else { None }
    });
    if let Some(id) = found {
        self.bot_id_cache.set(Some(id));
    }
    found
}
```

If you chose option (c) (skip the counter), remove the `bot_id_cache_hits` lines.

- [ ] **Step 5: Add the structural canary**

In the same commit (so the default-feature build stays consistent), add:

```rust
#[test]
fn phase3c_bot_id_cache_hits_counter_bumps_on_repeat_calls() {
    let mut pool = ConceptPool::new();
    let _ = pool.bot();  // intern Bot so the cache will populate.
    let _ = pool.bot_id();  // first call: scans + populates.
    let before = pool.bot_id_cache_hits();
    let _ = pool.bot_id();
    let _ = pool.bot_id();
    let _ = pool.bot_id();
    let after = pool.bot_id_cache_hits();
    assert!(
        after >= before + 3,
        "bot_id_cache_hits should increment on cached calls; \
         before={before} after={after}"
    );
}
```

If `pool.bot()` doesn't exist, the executor needs to find how Bot gets interned (likely by interning a concept that contains Bot, or via a `bot()` factory). Adjust accordingly.

- [ ] **Step 6: Run all owl-dl-core tests**

```bash
cargo test -p owl-dl-core -- --test-threads=1 2>&1 | tail -10
```

Expected: all owl-dl-core tests pass (the existing ones + Phase 3c canaries). If a pre-existing test fails, that's a regression — the cache changed semantic. Investigate.

- [ ] **Step 7: Run all tableau + reasoner-lib tests**

```bash
cargo test -p owl-dl-tableau -- --test-threads=1 2>&1 | tail -10
cargo test -p owl-dl-reasoner --lib -- --test-threads=1 2>&1 | tail -10
RUSTFLAGS="-D warnings" cargo test --workspace --no-run 2>&1 | tail -3
cargo clippy -p owl-dl-core --all-targets -- -D warnings 2>&1 | grep -E "warning|error" | grep -v "(too_many_lines|map_unwrap_or|doc-markdown)" | head -5
```

Expected: all tests pass; CI strictness clean; no new clippy on owl-dl-core.

If any test fails, the cache is broken — investigate. Most likely failure: a test that interns Bot AFTER calling bot_id() (which would populate the cache as None, leaving it stale post-Bot-intern). The fix in Step 4 already handles this case (the cache only populates on Some, so a subsequent call after Bot is interned still scans and finds it).

- [ ] **Step 8: Commit**

```bash
git add crates/owl-dl-core/src/ir.rs
git commit -m "$(cat <<'BODY'
perf(ir): cache ConceptPool::bot_id (Phase 3c)

bot_id() was an O(n) linear scan over every interned ConceptExpr on
every call. The post-Phase-3b flame attributed 24.66% of SIO classify
cost to the `apply_role_axioms` cluster, with bot_id being the
inner hot work (called per-node per-saturation; GALEN's pool has
thousands of concepts).

Fix: lazy Cell<Option<ConceptId>> cache. First call scans + populates;
subsequent calls hit the cache. Cache only populates on Some, so calls
made before Bot is interned correctly leave the cache None and re-scan
on the next call (until Bot exists, at which point caching kicks in).

Counter bot_id_cache_hits + accessor for structural canary. Verdict
canary asserts return-value equivalence across pre/post cache;
structural canary asserts the counter bumps on repeated calls.

Verdicts unchanged across owl-dl-core + tableau + reasoner-lib tests.
See plan: docs/superpowers/plans/2026-06-01-phase3c-bot-id-cache.md.
BODY
)"
```

---

## Task 3: Re-flamegraph + corpus measurement

**Files:**
- Create: `docs/flamegraphs/sio-classify-2026-06-01-post-phase3c.svg`
- Create: `docs/flamegraphs/sio-classify-2026-06-01-post-phase3c-findings.md`

- [ ] **Step 1: Rebuild with profile feature**

```bash
cargo build -p owl-dl-bench --release --features profile 2>&1 | tail -3
```

- [ ] **Step 2: Re-flamegraph SIO**

```bash
RUSTDL_PROFILE=docs/flamegraphs/sio-classify-2026-06-01-post-phase3c.svg \
RUSTDL_PROFILE_SECONDS=60 \
    ./target/release/owl-dl-bench classify ontologies/real/sio-stripped.ofn 2>&1 | tee /tmp/p3c-sio-flame.log | tail -10
```

- [ ] **Step 3: Diff baseline (post-Phase-3b) vs post-Phase-3c**

```bash
python3 <<'EOF'
import re
def top(p):
    with open(p) as f: s = f.read()
    out = []
    for m in re.finditer(r'<title>(.+?)\s*\(([\d,]+)\s+samples,\s*([\d.]+)\s*%\)</title>', s):
        out.append((float(m.group(3)), int(m.group(2).replace(',','')), m.group(1).strip()))
    return sorted(out, reverse=True)
b = top('docs/flamegraphs/sio-classify-2026-06-01-post-phase3b.svg')
p = top('docs/flamegraphs/sio-classify-2026-06-01-post-phase3c.svg')
print("BASELINE (post-3b) top 15:")
for pct, n, f in b[:15]: print(f"  {pct:6.2f}%  {f[:80]}")
print("\nPOST-PHASE-3c top 15:")
for pct, n, f in p[:15]: print(f"  {pct:6.2f}%  {f[:80]}")
# Specifically:
print("\n=== apply_role_axioms / bot_id / find_map ===")
for label, src in [("BASELINE", b), ("POST", p)]:
    print(f"{label}:")
    for pct, n, f in src[:50]:
        if 'apply_role_axioms' in f or 'bot_id' in f or 'find_map' in f:
            print(f"  {pct:6.2f}%  {f[:90]}")
EOF
```

Save output. Look for: `bot_id` / `apply_role_axioms` cluster dropping toward 0% (the cache makes the lookup O(1)).

- [ ] **Step 4: Phase 0 net soundness gate**

```bash
cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    alehif_closure_matches_konclude ore_10908_sroiq ore_15672_shoin \
    -- --ignored --nocapture 2>&1 | tee /tmp/p3c-net.log | grep -E "^---|FP=|MISSED="
```
Hard cap 30 min. Expected: FP=0 / MISSED=0 across all 3.

- [ ] **Step 5: GALEN measurement (isolated, with --exact-style filter)**

GALEN test name substring-matches notgalen, so isolate:

```bash
timeout 2400 cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    galen_closure_matches_konclude \
    -- --exact --ignored --nocapture 2>&1 | tee /tmp/p3c-galen.log | grep -E "^--- galen|MISSED="
```

(If `--exact` doesn't take effect via that placement, the run will sequentially process galen + notgalen within the 40-min cap. Record what completes.)

Hard cap 40 min. Expected: FP=0; MISSED=17 unchanged; wall ≤ 21.1 min (post-Phase-3a, also the practical post-Phase-3b baseline since Phase 3b's measurement was contention-noisy).

- [ ] **Step 6: Write findings doc**

Create `docs/flamegraphs/sio-classify-2026-06-01-post-phase3c-findings.md`:

```markdown
# Phase 3c post-fix SIO flamegraph + findings

Re-flamegraphed 2026-06-0N against HEAD <commit> (Phase 3c bot_id cache).
Sampling: pprof-rs @ 199Hz, 60s window on `ontologies/real/sio-stripped.ofn`.

## Frame-level diff (top 15)

<paste Step 3 output>

## Hot-frame % deltas

- `apply_role_axioms`: <pre>% → <post>% (Δ <delta>pp)
- `bot_id`: <pre>% → <post>% (Δ <delta>pp)
- `find_map` (cluster): <pre>% → <post>% (Δ <delta>pp)

## Corpus measurement

| Fixture | Pre-P3c wall | Post-P3c wall | Δ | FP | MISSED |
|---|---|---|---|---|---|
| alehif | 2.72s | <new>s | <delta> | 0 | 0 |
| ore-10908-sroiq | 31.6s | <new>s | <delta> | 0 | 0 |
| ore-15672-shoin | 29.7s | <new>s | <delta> | 0 | 0 |
| galen | 21.1 min | <new> | <delta> | 0 | 17 |

FP=0 + MISSED-unchanged held across all fixtures.

## Interpretation

<one paragraph: did the cache eliminate the 24.66% cluster? What's
the next hot frame to attack in Phase 3d?>
```

- [ ] **Step 7: Commit**

```bash
git add docs/flamegraphs/sio-classify-2026-06-01-post-phase3c.svg \
        docs/flamegraphs/sio-classify-2026-06-01-post-phase3c-findings.md
git commit -m "perf(phase3c): SIO re-flamegraph + corpus measurement (post bot_id cache)"
```

---

## Task 4: Results doc + close-out

**Files:**
- Create: `docs/phase3c-results.md`
- Modify: `CLAUDE.md` (saturator/tableau perf note)
- Modify: `docs/superpowers/specs/2026-05-31-soundness-completeness-perf-design.md` (Phase 3 close-out continuation)

- [ ] **Step 1: Write `docs/phase3c-results.md`**

Mirror Phase 3a/3b results doc shape:

```markdown
# Phase 3c — ConceptPool::bot_id cache results

Run 2026-06-0N. Fix: lazy `Cell<Option<ConceptId>>` cache on
`ConceptPool::bot_id` (was an O(n) linear scan over every interned
ConceptExpr; called per-node per-saturation across 6 hot call sites).
See `docs/flamegraphs/sio-classify-2026-06-01-post-phase3c-findings.md`
for the raw measurement.

## Headline finding

<one paragraph: the bot_id cluster's % delta + soundness gate
status. Expected: 24.66% → near 0% on the bot_id cluster.>

## Soundness gate (Phase 0 net)

<table from Task 3 Step 4>

## Wall lever

<table for SIO + GALEN walls>

## Flamegraph diff

<table from Task 3 Step 3>

## What this fix does

The 6 call sites of `bot_id()` (apply_max, nominal-resolution,
apply_choose, apply_role_axioms, classify) each previously scanned
the full ConceptPool. The Phase 3b flame showed the cluster at
24.66%; the cache makes the lookup O(1) after first scan.

Soundness: Bot is a unit variant interned at most once; its
ConceptId is stable once assigned. The cache only populates on
Some (i.e. only after Bot is actually in the pool), so calls made
before Bot is interned correctly continue to scan and find None.
Verdicts unchanged across all tests.

## What's left

- Phase 3d: clash detection (`first_clash` + `clash_deps_at`,
  ~22-26% combined post-Phase-3).
- Phase 3e: heap allocations (`spec_extend` / `from_iter`).
- Phase 2c: cluster C/D EL+ approximation (44 residual MISSED).
```

- [ ] **Step 2: Update CLAUDE.md**

Find the `crates/owl-dl-tableau` bullet (or `crates/owl-dl-core` — the cache is in core). Append:

```
Phase 3c (commit <SHA>) cached ConceptPool::bot_id to eliminate
the per-call O(n) scan. SIO flamegraph: apply_role_axioms cluster
24.66% → <new>%. FP=0 + verdicts unchanged. See `docs/phase3c-results.md`.
```

- [ ] **Step 3: Update design spec**

Append to the Phase 3 section:

```
Phase 3c landed: `docs/phase3c-results.md`. ConceptPool::bot_id
cached; the apply_role_axioms / bot_id / find_map cluster
24.66% → <new>%. FP=0 + MISSED-unchanged held. Phase 3d queued for
clash detection.
```

- [ ] **Step 4: Commit**

```bash
git add docs/phase3c-results.md \
        CLAUDE.md \
        docs/superpowers/specs/2026-05-31-soundness-completeness-perf-design.md
git commit -m "docs(phase3c): results doc + envelope updates"
```

---

## Definition of done (Phase 3c)

- `ConceptPool::bot_id` is cached; structural canary confirms via the counter.
- Verdict-preservation canary asserts return-value equivalence pre/post cache.
- All owl-dl-core + tableau + reasoner-lib tests pass; CI strictness clean.
- Re-flamegraph confirms the `apply_role_axioms` / `bot_id` cluster dropped.
- Phase 0 net FP=0 + GALEN MISSED=17 unchanged.
- Results doc + CLAUDE.md + design spec updated.

## What this plan does NOT do

- Does NOT touch clash detection (Phase 3d) or heap allocations (Phase 3e).
- Does NOT change the cache's invalidation semantic (Bot is unit + stable; no invalidation needed).
- Does NOT touch the 6 call sites — the cache is transparent at the function boundary.
