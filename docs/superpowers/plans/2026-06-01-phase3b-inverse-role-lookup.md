# Phase 3b — Inverse-role lookup optimization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reduce SIO classify wall time by attacking the `are_declared_inverses` linear scan (25.76% of SIO's `apply_max` cost per the Phase 3 SIO flamegraph), while preserving FP=0 + the post-Phase-3a baseline (GALEN 21.1 min / MISSED=17 / all Phase 0 net fixtures FP=0).

**Architecture:** Single-crate change in `crates/owl-dl-tableau/src/lib.rs`. The existing `are_declared_inverses` (line 484) does `self.inverse_pairs.iter().any(|&(a, b)| a == r && b == s)` — fine for the pizza-era "0-3 pairs" comment but evidently hot on SIO. Task 1 measures SIO's actual inverse-pair count; Task 2 picks the right data structure (FxHashSet of pairs, or per-role `HashMap<RoleId, SmallVec<[RoleId; 2]>>`) based on the measurement; Tasks 3-4 implement + TDD; Tasks 5-6 measure + document.

**Tech Stack:** Rust (edition 2024), `owl-dl-tableau` crate, `owl-dl-bench --features profile` for re-flamegraphing.

---

## Background the executor needs

- SIO flamegraph (`docs/flamegraphs/sio-classify-2026-06-01.svg`, finding file
  alongside): 27.93% inclusive in `apply_max`, with **25.76%** specifically in
  `edge_satisfies` / `are_declared_inverses` (the inverse-role linear scan
  *inside* `apply_max`). EL saturator: 0.00% on SIO.
- GALEN post-Phase-3a baseline (`docs/flamegraphs/galen-classify-2026-06-01-post-phase3.svg`):
  `apply_max` 19.58%; the same `are_declared_inverses` scan contributes here
  too.
- Phase 3a results doc (`docs/phase3-results.md`): GALEN wall 24.7 min →
  21.1 min (−14.6%) via the `needs_deferred_or` bloom prefilter. Phase 3b is
  queued in the "What's left" section.
- Hot code: `crates/owl-dl-tableau/src/lib.rs:484-493` (`are_declared_inverses`,
  the linear scan); `crates/owl-dl-tableau/src/lib.rs:581-592`
  (`edge_satisfies`, the caller — only calls `are_declared_inverses` on
  the cross-polarity branch); `crates/owl-dl-tableau/src/rules.rs:805+`
  (`apply_max`, which calls `edge_satisfies` per neighbour per role).
- Existing test infrastructure: `crates/owl-dl-tableau/src/counters.rs`
  pattern (mirror `needs_deferred_or_bloom_rejects` added in Phase 3a) for
  any new counter.
- The Phase 3a fix (commit 64bee92) extended a bloom prefilter; the same
  perf-doc culture applies here — measurement-gated, single fix, verdicts
  unchanged.
- Soundness gate: `scripts/run-soundness-diff.sh` Phase 0 fixtures + GALEN at
  the post-2b baseline. FP=0 / MISSED=17 (GALEN) / MISSED=0 (Phase 0 net) all
  must hold.

---

## Task 1: Measure SIO's inverse-pair count + pick the fix

**Files:**
- Create: `docs/phase3b-fix-target.md`

The hypothesis: SIO has more than the "0-3 inverse pairs" the pizza-era comment assumed. The fix's right shape depends on the actual count.

- [ ] **Step 1: Count SIO's inverse-pair declarations**

```bash
grep -cE "InverseObjectProperties|inverseOf" ontologies/real/sio-stripped.ofn
```

Run on:
- `ontologies/real/sio-stripped.ofn` (the post-Phase-0 stripped SIO).
- `ontologies/external/galen.ofn` (for comparison; GALEN may have few inverse pairs).
- `ontologies/external/notgalen.ofn` (similar comparison).

Record the counts.

- [ ] **Step 2: Identify how InverseObjectProperties axioms reach `inverse_pairs`**

Find where the IR `Axiom::InverseObjectProperties` (or equivalent) is consumed and pushed onto `TableauContext::inverse_pairs`. Likely in `crates/owl-dl-reasoner/src/lib.rs` (the build path) or the preprocessing pass.

```bash
grep -rn "inverse_pairs\|declare_inverse_pair\|InverseObjectProperties" crates/owl-dl-tableau/src/ crates/owl-dl-reasoner/src/ | head -15
```

This confirms what the `inverse_pairs` Vec actually contains (axiom-declared inverses) and whether the count matches what `grep` showed.

- [ ] **Step 3: Look at the existing counter pattern**

```bash
grep -nA3 "is_blocked_prefilter_rejects\|needs_deferred_or_bloom_rejects" crates/owl-dl-tableau/src/counters.rs | head -20
```

Confirms the field shape (`Cell<u64>`), the `dump()` entry registration, and the `bump_counter!` macro. Phase 3b adds a counter for "inverse-pair lookup hits" or similar.

- [ ] **Step 4: Pick the fix based on Step 1's data**

If SIO has > 10 inverse pairs, the linear scan's O(N) cost is real. Three candidate fixes:

**Option A (FxHashSet of pairs).** Replace `inverse_pairs: Vec<(RoleId, RoleId)>` with `inverse_pairs_set: FxHashSet<(RoleId, RoleId)>` for O(1) lookup. Keep the Vec too (for iteration in declarations / rollback), or replace it fully. Simple, low-risk.

**Option B (per-role `HashMap<RoleId, Vec<RoleId>>`).** Index by first role: `inverse_of: HashMap<RoleId, SmallVec<[RoleId; 2]>>`. `are_declared_inverses(r, s)` becomes `inverse_of.get(&r).map_or(false, |v| v.contains(&s))`. Faster per-role indexing; preserves the existing `inverse_pairs` Vec for declaration order.

**Option C (sorted Vec + binary_search).** Sort `inverse_pairs` at engine-build time; `are_declared_inverses` does binary_search. No new state, but requires the Vec to stay sorted (or sort once at build).

For SIO at, say, 20-50 inverse pairs: Option A's FxHashSet wins (hash on `(u32, u32)` is cheap). For > 100, definitely A or B. For < 10, the existing linear scan is already optimal — bail out and pick a different Phase 3b target.

Note: `FxHashSet` (from `rustc_hash`) is the saturator's hot-path hash impl elsewhere in the codebase if used; otherwise default `HashSet` is fine. Confirm with `grep -rn "use rustc_hash\|FxHashSet" crates/owl-dl-tableau/src/` whether the crate already uses it.

- [ ] **Step 5: Write `docs/phase3b-fix-target.md`**

Structure:

```markdown
# Phase 3b — first fix target

Based on Phase 3 SIO flamegraph (`docs/flamegraphs/sio-classify-2026-06-01.svg`)
hot frame `are_declared_inverses` at 25.76% of `apply_max`'s inclusive cost.

## Inverse-pair counts

- SIO (sio-stripped.ofn): N inverse-pair declarations.
- GALEN (galen.ofn): M.
- notgalen (notgalen.ofn): K.

The existing comment at `lib.rs:485-488` assumed "0-3 pairs" (pizza-era).
SIO has <N> pairs; linear scan is O(<N>) per `are_declared_inverses` call.

## Chosen fix

Option <A|B|C|other> per the inverse-pair count: <one-paragraph
description of the chosen data structure + the rationale>.

## Implementation surface

- `crates/owl-dl-tableau/src/lib.rs` (the `are_declared_inverses` body
  + the `declare_inverse_pair` setter, if any).
- `crates/owl-dl-tableau/src/counters.rs` (new counter `inverse_pair_lookups`
  or `inverse_pair_fast_hits` — name per the chosen option).

## Expected impact

SIO flame: `are_declared_inverses` 25.76% → <estimated>%. If the
fix succeeds, `apply_max` overall drops correspondingly (27.93% → ~18%).
GALEN wall: post-Phase-3a 21.1 min → target ~19-20 min (5-10% reduction).
SIO wall: baseline ~68s; post-fix target ~50-55s.

## Soundness considerations

The fix changes ONLY the data structure for inverse-pair lookup. Logic
unchanged: same boolean returned for same inputs. Verdicts preserved on
all 87 tableau + 78 reasoner-lib tests + Phase 0 net + GALEN.
```

- [ ] **Step 6: Commit**

```bash
git add docs/phase3b-fix-target.md
git commit -m "perf(phase3b): inverse-pair count measurement + chosen fix design"
```

---

## Task 2: TDD canaries

**Files:**
- Modify: `crates/owl-dl-tableau/src/lib.rs` (or wherever `mod tests` is for the lib).

Two canaries: verdict preservation on an inverse-role-heavy synthetic + structural assertion that the new fast-path data structure is consulted.

- [ ] **Step 1: Build the inverse-heavy synthetic**

A small ontology with: a handful of roles, several `InverseObjectProperties` declarations, and a `Max(1, R, C)`-style axiom that triggers `apply_max` to call `are_declared_inverses` repeatedly.

Skeleton:

```rust
// Class A has ≤1 successor via R; B has predecessor via inverse(R).
// 5+ inverse-pair declarations to push past the "0-3 pairs" linear-scan
// regime.
let src = "\
Prefix(:=<http://rustdl.test/p3b/>)
Ontology(<http://rustdl.test/p3b/test>
    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    Declaration(ObjectProperty(:r1)) Declaration(ObjectProperty(:s1))
    Declaration(ObjectProperty(:r2)) Declaration(ObjectProperty(:s2))
    Declaration(ObjectProperty(:r3)) Declaration(ObjectProperty(:s3))
    Declaration(ObjectProperty(:r4)) Declaration(ObjectProperty(:s4))
    Declaration(ObjectProperty(:r5)) Declaration(ObjectProperty(:s5))
    InverseObjectProperties(:r1 :s1)
    InverseObjectProperties(:r2 :s2)
    InverseObjectProperties(:r3 :s3)
    InverseObjectProperties(:r4 :s4)
    InverseObjectProperties(:r5 :s5)
    SubClassOf(:A ObjectMaxCardinality(1 :r1 :C))
    SubClassOf(:A ObjectSomeValuesFrom(:r1 :B))
    SubClassOf(:A ObjectSomeValuesFrom(:r1 :C))
)
";
```

The ontology asserts A has ≤1 R1-successor with C, but two R1-successors with C are forced (one via :B which may or may not be in :C). The classification verdict for `A ⊑ B` or `B ⊑ C` may or may not derive; the exact assertions depend on what's actually derivable. KEEP IT SIMPLE: assert just that classification COMPLETES without crashing and that A is consistent (not in unsat).

- [ ] **Step 2: Write the verdict-preservation canary**

In `crates/owl-dl-tableau/src/lib.rs::mod tests` (or `crates/owl-dl-reasoner/src/lib.rs::tests` if cross-crate is easier — check where existing apply_max tests live first):

```rust
#[test]
fn phase3b_inverse_heavy_classification_completes() {
    // The synthetic above; classify and assert A is consistent.
    let src = "...";  // the source from Step 1
    let onto = parse(src);
    let h = classify(&onto).expect("classification");
    let iri_a = "http://rustdl.test/p3b/A";
    assert!(
        !h.unsatisfiable_classes().contains(&iri_a),
        "A should be consistent under the inverse-heavy synthetic"
    );
}
```

- [ ] **Step 3: Write the structural canary**

This asserts the new fast-path lookup is used. The implementation choice from Task 1 dictates the counter shape — likely `inverse_pair_fast_hits: Cell<u64>` on `RuleCounters`. The canary references this counter (currently nonexistent) and expects compilation to FAIL until Task 3 adds it.

```rust
#[cfg(feature = "counters")]
#[test]
fn phase3b_inverse_pair_fast_path_consulted() {
    let src = "...";  // same synthetic as Step 2
    let onto = parse(src);
    let _h = classify(&onto).expect("classification");
    // After classification, the inverse-pair fast-path counter should
    // be > 0 if the new data structure is actually consulted.
    let hits = crate::counters::inverse_pair_fast_hits();
    assert!(
        hits > 0,
        "inverse-pair fast-path counter never bumped; \
         are_declared_inverses isn't consulting the new structure"
    );
}
```

(The exact counter-accessor signature depends on Task 1's design.)

- [ ] **Step 4: Run canaries — expect partial results**

```bash
cargo test -p owl-dl-tableau phase3b_ -- --test-threads=1 2>&1 | tail -10
cargo test -p owl-dl-tableau --features counters phase3b_ -- --test-threads=1 2>&1 | tail -10
```

Expected (default features): `phase3b_inverse_heavy_classification_completes` passes (verdicts already correct).
Expected (--features counters): compile FAIL on `phase3b_inverse_pair_fast_path_consulted` because the counter doesn't exist yet.

- [ ] **Step 5: CI strictness clean (default features)**

```bash
RUSTFLAGS="-D warnings" cargo test -p owl-dl-tableau --no-run 2>&1 | tail -3
```
Expected: clean (the cfg-gated counters test isn't compiled).

- [ ] **Step 6: Commit**

```bash
git add crates/owl-dl-tableau/src/lib.rs
git commit -m "test(tableau): Phase 3b canaries (verdict + fast-path counter structural)"
```

---

## Task 3: Implement the fix

**Files:**
- Modify: `crates/owl-dl-tableau/src/lib.rs` (`are_declared_inverses` body + struct fields + declare_inverse_pair if it exists).
- Modify: `crates/owl-dl-tableau/src/counters.rs` (new counter field + dump entry).

The implementation depends on Task 1's chosen option. For Option A (FxHashSet) — likely the cleanest:

- [ ] **Step 1: Add the new data structure**

Find the `TableauContext` (or wherever `inverse_pairs: Vec<(RoleId, RoleId)>` lives — likely on a static-build struct that the context borrows). Add a parallel field:

```rust
    /// O(1) lookup for `are_declared_inverses`. Built at engine-construction
    /// time from `inverse_pairs`. Phase 3b — see `docs/phase3b-fix-target.md`.
    inverse_pairs_set: FxHashSet<(RoleId, RoleId)>,
```

If `FxHashSet` isn't already imported, add `use rustc_hash::FxHashSet;` (the crate is likely already a dependency — check `Cargo.toml`).

- [ ] **Step 2: Populate it from `inverse_pairs`**

In the constructor (or `with_ontology`-style init) where `inverse_pairs` is populated, mirror the writes into `inverse_pairs_set`. If there's a `declare_inverse_pair` method, update both there.

Alternative: build the set lazily/once at the end of `set_complement`-time initialization. Pick what fits the existing init order.

- [ ] **Step 3: Update `are_declared_inverses` to use the set**

In `crates/owl-dl-tableau/src/lib.rs:484-493`, replace:

```rust
    pub fn are_declared_inverses(&self, r: RoleId, s: RoleId) -> bool {
        if self.inverse_pairs.is_empty() {
            return false;
        }
        self.inverse_pairs.iter().any(|&(a, b)| a == r && b == s)
    }
```

with:

```rust
    pub fn are_declared_inverses(&self, r: RoleId, s: RoleId) -> bool {
        // Phase 3b: O(1) hashset lookup replaces the linear `Vec::iter().any()`
        // (which was justified for the pizza-era "0-3 pairs" assumption
        // but evidently hot on SIO at 25.76% of apply_max — see
        // docs/phase3b-fix-target.md).
        if self.inverse_pairs_set.is_empty() {
            return false;
        }
        let hit = self.inverse_pairs_set.contains(&(r, s));
        crate::bump_counter!(self, inverse_pair_fast_hits);
        hit
    }
```

Note: `bump_counter!` may need a different invocation pattern depending on whether `self` exposes counters or whether `ctx` is needed. Adapt to the pattern used elsewhere in the crate.

- [ ] **Step 4: Add the counter field**

In `crates/owl-dl-tableau/src/counters.rs`, add after `needs_deferred_or_bloom_rejects`:

```rust
    /// Phase 3b: each call to `are_declared_inverses` that consulted
    /// the O(1) FxHashSet. Used by the phase3b structural canary to
    /// confirm the fast-path is wired. See `docs/phase3b-fix-target.md`.
    pub inverse_pair_fast_hits: Cell<u64>,
```

Add a `dump()` entry mirroring `needs_deferred_or_bloom_rejects`.

If the test canary calls `crate::counters::inverse_pair_fast_hits()` as a free function rather than a field access (Task 2's design), add a `pub fn inverse_pair_fast_hits() -> u64` accessor too.

- [ ] **Step 5: Run all canaries**

```bash
cargo test -p owl-dl-tableau phase3b_ -- --test-threads=1 2>&1 | tail -10
cargo test -p owl-dl-tableau --features counters phase3b_ -- --test-threads=1 2>&1 | tail -10
```
Expected: both pass.

- [ ] **Step 6: Run all tableau tests — soundness regression check**

```bash
cargo test -p owl-dl-tableau -- --test-threads=1 2>&1 | tail -10
cargo test -p owl-dl-reasoner --lib -- --test-threads=1 2>&1 | tail -10
RUSTFLAGS="-D warnings" cargo test -p owl-dl-tableau --no-run 2>&1 | tail -3
cargo clippy -p owl-dl-tableau --all-targets -- -D warnings 2>&1 | grep -E "warning|error" | grep -v "(too_many_lines|map_unwrap_or|doc-markdown)" | head -5
```
Expected: all pre-existing tests pass + 2 new phase3b ones = 89 tableau / 78 reasoner-lib; CI strictness clean; no new clippy.

If ANY pre-existing test fails, that's a regression. STOP and investigate. The fix is a pure data-structure swap; verdicts MUST be unchanged.

- [ ] **Step 7: Commit**

```bash
git add crates/owl-dl-tableau/src/lib.rs crates/owl-dl-tableau/src/counters.rs
git commit -m "perf(tableau): FxHashSet for are_declared_inverses (Phase 3b)

Replaces the O(N) linear Vec::iter().any() scan with an O(1) hashset
lookup. The pizza-era 'most ontologies have 0-3 inverse pairs'
assumption is empirically wrong on SIO (the flamegraph attributed
25.76% of apply_max to are_declared_inverses).

The new inverse_pairs_set field is populated in parallel with
inverse_pairs; declare_inverse_pair updates both. The original Vec
stays for declaration-order iteration. Verdicts unchanged across
87 tableau + 78 reasoner-lib tests + Phase 0 net + GALEN."
```

---

## Task 4: Re-flamegraph + corpus measurement

**Files:**
- Create: `docs/flamegraphs/sio-classify-2026-06-01-post-phase3b.svg`
- Create: `docs/flamegraphs/sio-classify-2026-06-01-post-phase3b-findings.md`
- Capture logs: `/tmp/p3b-final-net.log`, `/tmp/p3b-sio.log`, `/tmp/p3b-galen.log` (if room).

- [ ] **Step 1: Rebuild with profile feature**

```bash
cargo build -p owl-dl-bench --release --features profile 2>&1 | tail -3
```

- [ ] **Step 2: Re-flamegraph SIO (primary target — Phase 3b is SIO-driven)**

```bash
RUSTDL_PROFILE=docs/flamegraphs/sio-classify-2026-06-01-post-phase3b.svg \
RUSTDL_PROFILE_SECONDS=60 \
    ./target/release/owl-dl-bench classify ontologies/real/sio-stripped.ofn 2>&1 | tee /tmp/p3b-sio-flame.log | tail -10
```

- [ ] **Step 3: Diff baseline vs post-fix top frames**

```bash
python3 <<'EOF'
import re
def top(p):
    with open(p) as f: s = f.read()
    out = []
    for m in re.finditer(r'<title>(.+?)\s*\(([\d,]+)\s+samples,\s*([\d.]+)\s*%\)</title>', s):
        out.append((float(m.group(3)), int(m.group(2).replace(',','')), m.group(1).strip()))
    return sorted(out, reverse=True)
b = top('docs/flamegraphs/sio-classify-2026-06-01.svg')
p = top('docs/flamegraphs/sio-classify-2026-06-01-post-phase3b.svg')
print("BASELINE SIO top 15:")
for pct, n, f in b[:15]: print(f"  {pct:6.2f}%  {f[:80]}")
print("\nPOST-PHASE-3b SIO top 15:")
for pct, n, f in p[:15]: print(f"  {pct:6.2f}%  {f[:80]}")
EOF
```

Save output. Look for: `are_declared_inverses` 25.76% → much lower (target near 0% if the FxHashSet completely eliminates the scan cost).

- [ ] **Step 4: Phase 0 net + GALEN soundness/wall gates**

```bash
cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    alehif_closure_matches_konclude ore_10908_sroiq ore_15672_shoin \
    -- --ignored --nocapture 2>&1 | tee /tmp/p3b-final-net.log | grep -E "^---|FP=|MISSED="
```
Hard cap 30 min. Expected: FP=0 / MISSED=0 across all 3; wall ≤ post-Phase-3a baseline (alehif 2.72s, ORE-SROIQ 31.6s, ORE-SHOIN 29.71s).

```bash
timeout 2400 cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    galen_closure_matches_konclude \
    -- --ignored --nocapture 2>&1 | tee /tmp/p3b-galen.log | grep -E "^--- galen|MISSED="
```
Hard cap 40 min. Expected: FP=0; MISSED=17 unchanged; wall ≤ 21.1 min (Phase 3a baseline), ideally 19-20 min.

- [ ] **Step 5: SIO wall measurement (the lever's payoff)**

```bash
time ./target/release/owl-dl-bench classify ontologies/real/sio-stripped.ofn 2>&1 | tail -10
```

Or via the harness if it has a SIO test. SIO baseline (spec) was 68s; post-Phase-3b target is significant reduction (the 25.76% scan being eliminated suggests ~20-30% wall reduction is possible).

- [ ] **Step 6: Write findings doc**

Create `docs/flamegraphs/sio-classify-2026-06-01-post-phase3b-findings.md`:

```markdown
# Phase 3b post-fix SIO flamegraph + findings

Re-flamegraphed 2026-06-0N against HEAD <commit> (Phase 3b
FxHashSet for are_declared_inverses). Sampling: pprof-rs @ 199Hz,
60s window.

## Frame-level diff (top 15)

<paste Step 3 output>

## Hot-frame % deltas

- `are_declared_inverses`: 25.76% → <post>% (Δ <delta>pp)
- `apply_max`: 27.93% → <post>% (Δ <delta>pp)

## Corpus measurement

| Fixture | Pre-P3b wall | Post-P3b wall | Δ |
|---|---|---|---|
| sio-stripped | ~68s | <new>s | <delta> |
| galen | 21.1 min | <new> | <delta> |
| alehif | 2.72s | <new>s | <delta> |
| ore-10908-sroiq | 31.6s | <new>s | <delta> |
| ore-15672-shoin | 29.71s | <new>s | <delta> |

FP=0 held across all fixtures; MISSED unchanged.

## Interpretation

<one paragraph: did the fix hit its target? Was the wall reduction
in the expected range? What's the next hot frame to attack in
Phase 3c?>
```

- [ ] **Step 7: Commit**

```bash
git add docs/flamegraphs/sio-classify-2026-06-01-post-phase3b.svg \
        docs/flamegraphs/sio-classify-2026-06-01-post-phase3b-findings.md
git commit -m "perf(phase3b): SIO re-flamegraph + measurement (post inverse-set fix)"
```

---

## Task 5: Results doc + close-out

**Files:**
- Create: `docs/phase3b-results.md`
- Modify: `CLAUDE.md` (tableau perf note)
- Modify: `docs/superpowers/specs/2026-05-31-soundness-completeness-perf-design.md` (Phase 3 close-out continuation)

- [ ] **Step 1: Write `docs/phase3b-results.md`**

Mirror the Phase 3a results doc shape:

```markdown
# Phase 3b — Inverse-role lookup (FxHashSet) results

Run 2026-06-0N. Fix: replace `are_declared_inverses`'s O(N)
`Vec::iter().any()` linear scan with an O(1) `FxHashSet<(RoleId,
RoleId)>` lookup. See `docs/phase3b-fix-target.md` for design and
`docs/flamegraphs/sio-classify-2026-06-01-post-phase3b-findings.md`
for measurements.

## Headline finding

**SIO classify wall dropped <pre>s → <post>s (<delta>% reduction),
FP=0 + MISSED-unchanged held.** The pizza-era "0-3 pairs"
assumption was empirically wrong on SIO (<N> inverse pairs);
the hashset's O(1) lookup eliminates the 25.76% linear-scan cost.

GALEN: <pre> → <post> (<delta>) — the fix also helps GALEN
because `apply_max` is a hot frame there too.

## Soundness gate (Phase 0 net)

<table>

## Wall lever

| Fixture | Pre-P3b | Post-P3b | Δ |
|---|---|---|---|
| sio-stripped | ~68s | <new> | <delta> |
| galen | 21.1 min | <new> | <delta> |
| (others) | ... | ... | ... |

## Flamegraph diff

<table from Task 4 Step 3>

## What's left

- Phase 3c: `apply_max` itself (the remaining apply_max cost after
  the inverse-scan is removed; likely the O(n²) Vec::contains dedup
  + the distinct-pair marking).
- Phase 3d: clash detection (first_clash + clash_deps_at, ~22-26%
  combined post-Phase-3).
- Phase 3e: heap allocations.
- Phase 2c: cluster C/D EL+ approximation (44 residual MISSED).
```

- [ ] **Step 2: Update CLAUDE.md**

Find the `crates/owl-dl-tableau` bullet. Append:

```
Phase 3b (commit <SHA>) replaced are_declared_inverses's O(N) linear
scan with an O(1) FxHashSet lookup. SIO classify wall: <pre> →
<post>; verdicts unchanged. See `docs/phase3b-results.md`.
```

- [ ] **Step 3: Update design spec Phase 3 section**

Append after the existing Phase 3a "Landed" paragraph:

```
Phase 3b landed: `docs/phase3b-results.md`. SIO 25.76% inverse-scan
cost eliminated via FxHashSet swap; SIO wall <pre>s → <post>s,
GALEN wall <pre> → <post>. FP=0 + MISSED-unchanged held everywhere.
Phase 3c queued for `apply_max`'s remaining O(n²) Vec::contains
dedup + distinct-pair marking.
```

- [ ] **Step 4: Commit**

```bash
git add docs/phase3b-results.md \
        CLAUDE.md \
        docs/superpowers/specs/2026-05-31-soundness-completeness-perf-design.md
git commit -m "docs(phase3b): results doc + envelope updates"
```

---

## Definition of done (Phase 3b)

- `are_declared_inverses` uses O(1) `FxHashSet` lookup (or the chosen Option from Task 1).
- Verdict-preservation canary + structural counter canary pass.
- 87+ tableau tests + 78 reasoner-lib tests pass; CI strictness clean.
- SIO + Phase 0 + GALEN measurement recorded; FP=0 + MISSED unchanged.
- Results doc + CLAUDE.md + design spec updated.

## What this plan does NOT do

- Does NOT touch `apply_max`'s O(n²) dedup or distinct-pair marking (Phase 3c).
- Does NOT change the saturator (Phase 2c).
- Does NOT change verdicts on any fixture.
