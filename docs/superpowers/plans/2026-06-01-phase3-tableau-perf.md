# Phase 3 — Tableau Perf (Post-2b Hot-Path Attack) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reduce GALEN classify wall time from the post-Phase-2b 24.7 min back toward the pre-2b 12.5 min baseline by attacking the top 1–2 hot frames in the tableau's deferred-OR + cardinality / clash paths, while preserving FP=0 + the 92-pair MISSED recovery Phase 2b delivered.

**Architecture:** Single-crate change in `crates/owl-dl-tableau/src/`. The GALEN flamegraph (`docs/flamegraphs/galen-classify-2026-06-01.svg` + findings) names the top hot frames; this plan starts with a SIO flamegraph to confirm/refute the spec's "saturator dominates on SIO" claim, reads the hot tableau code (`rules.rs::needs_deferred_or` at line 612, `rules.rs::apply_deferred_concept_or_rules` at 546, `rules.rs::apply_max` at 805, `saturate.rs::first_clash` at 175) to identify the most cost-effective single fix, applies it with TDD (a perf canary asserting verdicts unchanged + a wall-time gate on a known fixture), re-flamegraphs to confirm the % dropped, and measures the corpus impact.

**Tech Stack:** Rust (edition 2024), `owl-dl-tableau` crate, `owl-dl-bench --features profile` (pprof-rs sampling per `docs/flamegraphs/README.md`).

---

## Background the executor needs

- The GALEN flamegraph (`docs/flamegraphs/galen-classify-2026-06-01.svg`, taken at HEAD `b588b3a` post-Phase-2b/2b.5, 24,907 samples at 199Hz over 120s) revealed the spec's named Phase 3 target ("saturator dominates SIO 68s / notgalen 10min") is *not* what dominates GALEN post-2b: the EL saturator is 0.04% (11 samples); the tableau dominates.
- Top hot frames per `docs/flamegraphs/galen-classify-2026-06-01-findings.md`:
  - 73.01% `branch`/`search` (tableau backtracking driver, `owl-dl-tableau/src/search.rs`).
  - 72.32% `saturate` (tableau's own rule-saturation sub-loop, `owl-dl-tableau/src/saturate.rs`).
  - 31.40% `apply_deferred_concept_or_rules` — the deferred-OR rule path that Phase 2b's MISSED recovery exercises more aggressively.
  - 18.13% `PartialEq::eq` (leaf) — comparing `ConceptId`s, almost certainly inside the binary-search calls in `needs_deferred_or` at `rules.rs:612`.
  - 17.08% `apply_max` (`rules.rs:805`).
  - ~22% combined `first_clash` (`saturate.rs:175`) + `clash_deps_at`.
  - ~12% `spec_extend`/`from_iter` (heap allocs in `apply_max` + `apply_deferred`).
  - 4% `SmallVec<[u32; 1]>::clone` (DepSet clone in deferred-OR path).
- Pre-Phase-2 baselines: GALEN 12.5 min; notgalen ~10 min (per spec, but reality on this hardware was 37.5 min — see Phase 1 measurement notes; the spec's perf numbers may be from a different machine).
- The existing perf-doc culture (`docs/perf-2026-05-24-new-server.md`, the existing `docs/flamegraphs/` series) is fix-then-measure with re-flamegraph between iterations. Each fix is a measurable diff (pass/revert per `docs/moms-plan.md` §A discipline).
- Existing `Node` already carries a `label_sig` bloom filter per `docs/flamegraphs/README.md` (added in the "B.4" perf work) — the pattern of "bloom prefilter for fast `labels.contains(c)` checks" is already in the codebase. Phase 3 may extend it or build on it.
- The SOUNDNESS gate is the same as Phase 0/1/2: FP=0 across `scripts/run-soundness-diff.sh`. The COMPLETENESS gate is unchanged from Phase 2b: GALEN MISSED stays at 17, notgalen at 27. Wall is the lever.

---

## Task 1: SIO flamegraph — verify or refute spec's saturator claim

**Files:**
- Create: `docs/flamegraphs/sio-classify-2026-06-01.svg`
- Create: `docs/flamegraphs/sio-classify-2026-06-01-findings.md`

The spec named SIO as saturator-dominated. The GALEN flamegraph refuted "saturator-dominated" for GALEN; we need a SIO data point to know whether the spec's claim still holds on smaller / more EL-heavy fixtures. SIO baseline wall is ~68s, so a 60s flamegraph captures most of the run.

- [ ] **Step 1: Build with profile feature (already done if Phase 3 prep was run; cached otherwise)**

```bash
cargo build -p owl-dl-bench --release --features profile 2>&1 | tail -3
```
Expected: clean, fast (cached).

- [ ] **Step 2: Run profiled classify on sio-stripped**

```bash
RUSTDL_PROFILE=docs/flamegraphs/sio-classify-2026-06-01.svg \
RUSTDL_PROFILE_SECONDS=60 \
    ./target/release/owl-dl-bench classify ontologies/real/sio-stripped.ofn 2>&1 | tee /tmp/p3-sio-flame.log | tail -10
```

Expected: ~60s sampling + SVG written. The `sio-stripped.ofn` fixture is the data-property-stripped SIO (`docs/real-ontology-corpus.md` Caveat — Phase 0 provisioned this).

If the fixture is missing, fall back to `ontologies/external/galen.ofn`-shape extracts or any other SIO variant on disk (`ls ontologies/real/sio*.ofn`).

- [ ] **Step 3: Extract top hot frames**

```bash
python3 <<'EOF'
import re
with open('docs/flamegraphs/sio-classify-2026-06-01.svg') as f:
    svg = f.read()
titles = re.findall(r'<title>([^<]+)</title>', svg)
parsed = []
for t in titles:
    m = re.match(r'(.+?)\s*\(([\d,]+)\s+samples,\s*([\d.]+)\s*%\)', t)
    if m:
        parsed.append((float(m.group(3)), int(m.group(2).replace(',', '')), m.group(1).strip()))
parsed.sort(reverse=True)
print(f"Total frames: {len(parsed)}\n\nTop 25 by % (inclusive):")
for pct, n, fname in parsed[:25]:
    short = fname if len(fname) < 100 else fname[:97] + '...'
    print(f"  {pct:6.2f}%  ({n:>6} samples)  {short}")
EOF
```

Save the output for the findings doc.

- [ ] **Step 4: Write the findings doc**

Create `docs/flamegraphs/sio-classify-2026-06-01-findings.md` with EXACTLY this structure (fill the data tables from Step 3):

```markdown
# SIO classify hot-path findings (Phase 3 prep, SIO confirmation)

Profiled 2026-06-01 against branch `plan/soundness-completeness-perf`
post-Phase-2b/2b.5 (commit b588b3a). Sampling: pprof-rs @ 199Hz,
RUSTDL_PROFILE_SECONDS=60, on `ontologies/real/sio-stripped.ofn`.

## Top hot frames

<paste top 20-25 from Step 3>

## Spec verification

Per `docs/superpowers/specs/2026-05-31-soundness-completeness-perf-design.md`
§"Phase 3", the spec named SIO as saturator-dominated. This flamegraph
<confirms / refutes> that claim. The hot path is:

- <Saturator-dominated case:> `owl-dl-saturation::*` frames account
  for N% inclusive; the Phase 3 plan should target the EL saturator
  on SIO.
- <Tableau-dominated case:> tableau frames account for N%; the EL
  saturator is M% (≤ 5%); SIO has shifted to the same regime as
  GALEN post-2b.
- <Mixed case:> saturator N%, tableau M%; specific functions to flag.

## How this informs Phase 3

<one paragraph: based on the GALEN flamegraph's tableau-dominance +
the SIO finding here, where should Phase 3 land its first fix? The
spec's "Or-body regression at commit fddf2ee" target — is it visible
in either flamegraph?>
```

- [ ] **Step 5: Commit**

```bash
git add docs/flamegraphs/sio-classify-2026-06-01.svg \
        docs/flamegraphs/sio-classify-2026-06-01-findings.md
git commit -m "perf(phase3-prep): SIO flamegraph + findings to verify spec's saturator claim"
```

---

## Task 2: Read the GALEN-hot tableau code; pick the first fix

**Files:**
- Create: `docs/phase3-fix-target.md` (committed — records the chosen fix + design)
- Read-only inspection of: `crates/owl-dl-tableau/src/rules.rs:546-620` (apply_deferred_concept_or_rules + needs_deferred_or), `crates/owl-dl-tableau/src/rules.rs:805+` (apply_max), `crates/owl-dl-tableau/src/saturate.rs:175+` (first_clash), `crates/owl-dl-tableau/src/graph.rs` (Node label_sig if it exists)

The flamegraph's top hot frame on GALEN is `apply_deferred_concept_or_rules` at 31.4% inclusive, with `needs_deferred_or`'s `PartialEq::eq` showing 18.13% leaf cost. The hypothesis: the deferred-OR rule iterates every Or-shaped concept in the absorbed TBox, checking via `binary_search` whether the Or or any of its disjuncts is already in the node's labels. On GALEN, this iteration is the dominant per-saturate cost.

- [ ] **Step 1: Read the hot block**

Read `crates/owl-dl-tableau/src/rules.rs` lines 546-620 in full. Pay attention to:
- How `apply_deferred_concept_or_rules` iterates concepts to check.
- The structure of the absorbed `ConceptOrRule` list (how many entries are there for GALEN?).
- The `labels.binary_search` calls inside `needs_deferred_or`.
- Whether labels are already sorted (the binary_search assumes so — confirm).

Then read `crates/owl-dl-tableau/src/graph.rs` to find the `Node` struct. Look for an existing `label_sig` bloom filter (the prior B.4 work mentioned in `docs/flamegraphs/README.md`). If it exists, see how it's used and whether `apply_deferred_concept_or_rules` consults it (probably NOT — that's the gap).

- [ ] **Step 2: Read apply_max + first_clash for comparison**

Read `crates/owl-dl-tableau/src/rules.rs:805+` (apply_max) and `crates/owl-dl-tableau/src/saturate.rs:175+` (first_clash). These are #2 and #3 on the hot-frame list. Look at:
- `apply_max`'s allocation pattern (the 12% heap-alloc hotspot is here).
- `first_clash`'s iteration (called per backtracking step; the 22% combined cost is here).

Don't commit to fixing these in Phase 3's first iteration — Phase 3 is one fix at a time per the perf-doc-culture discipline.

- [ ] **Step 3: Design the first fix**

Based on what you read, write `docs/phase3-fix-target.md` with EXACTLY this structure:

```markdown
# Phase 3 — first fix target

Chosen from the GALEN flamegraph (`docs/flamegraphs/galen-classify-2026-06-01.svg`)
+ SIO flamegraph (`docs/flamegraphs/sio-classify-2026-06-01.svg`) per
the Phase 3 plan Task 2.

## The hot path

<2-3 paragraph description of the chosen function (likely
needs_deferred_or + its caller apply_deferred_concept_or_rules),
what it does, why it's expensive on GALEN.>

## The fix

<one paragraph: what to change. Examples:
- "Add a `label_set_bloom: u64` field to `Node` (or extend the existing
  label_sig if any), recompute lazily on label change, consult in
  `needs_deferred_or` before the binary_search to short-circuit when
  no operand is plausibly present."
- "Replace `binary_search` with a `FixedBitSet` per node (if label
  space is bounded)."
- "Cache `needs_deferred_or`'s return value per (node, concept) and
  invalidate on label change."
- "Switch apply_deferred_concept_or_rules to iterate only the deferred
  Or rules that mention concepts the node has — via a reverse index
  (rule_by_disjunct: HashMap<ConceptId, Vec<RuleIdx>>)."
- ...whatever the read of the code justifies>

## Expected impact

<one paragraph: if the fix succeeds, what % of the flame should
drop? The GALEN measurement gate is wall-time reduction; an
intermediate gate is the re-flamegraph showing the targeted frame
shrinking.>

## Soundness considerations

<one paragraph: does the fix change VERDICTS, or just speed? The
goal is wall reduction with VERDICTS UNCHANGED — GALEN MISSED must
stay at 17, alehif/ORE/pizza/ro/sulo MISSED at 0, FP=0 everywhere.>
```

- [ ] **Step 4: Commit the target doc**

```bash
git add docs/phase3-fix-target.md
git commit -m "perf(phase3): chosen first-fix target + design"
```

---

## Task 3: TDD canary for the fix

**Files:**
- Modify: `crates/owl-dl-tableau/src/<file>.rs` (the test file or mod tests of the relevant module — TBD based on Task 2's chosen fix)

The canary is a perf test, not a verdict test. We want to assert: (a) the verdict on a known fixture is UNCHANGED, (b) the targeted hot-frame structure (e.g. the cache, the bloom, the reverse index) is being USED.

- [ ] **Step 1: Locate `mod tests` in the relevant file**

Based on Task 2's fix, identify the file. If the fix is in `rules.rs`, look at the existing `mod tests` there (or one of its sibling test files). Confirm via:

```bash
grep -n "^#\\[cfg\\(test\\)\\]\|^mod tests" crates/owl-dl-tableau/src/rules.rs crates/owl-dl-tableau/src/graph.rs | head
```

- [ ] **Step 2: Write a verdict-preservation canary**

A small synthetic ontology where the targeted code path fires (e.g. an Or-heavy TBox if the fix is in apply_deferred_concept_or_rules). The canary asserts the classification verdict is UNCHANGED post-fix. Code shape:

```rust
#[test]
fn phase3_<fix_name>_preserves_verdict_on_or_heavy_synthetic() {
    // Synthetic ontology that exercises the hot path:
    //   A ⊑ Or(B, C)
    //   D ⊑ Or(B, E)
    //   F ⊑ Or(C, E)
    //   (etc — multiple Or-shaped axioms that the deferred-OR rule
    //    iterates over)
    let src = "...";
    let onto = parse(src);
    let h = classify(&onto);
    // Assert known entailments (these should hold pre- and post-fix):
    assert!(h.is_subclass("...", "..."));
    assert!(!h.is_subclass("...", "..."));
}
```

The exact synthetic depends on the fix; cite the chosen fix from Task 2.

- [ ] **Step 3: Write a structural canary (the fix is actually used)**

This is the "test the cache/bloom/index is consulted" assertion. If the fix is a bloom filter, expose a counter (`#[cfg(test)] pub fn label_set_bloom_hits()`) and assert it's incremented during the saturation. If the fix is a reverse index, assert the per-class index list is non-empty for a class with deferred OR rules. The shape depends on the fix.

Example structural canary for a hypothetical bloom fix:

```rust
#[test]
fn phase3_label_set_bloom_is_consulted_during_deferred_or() {
    // Same Or-heavy synthetic.
    let src = "...";
    let onto = parse(src);
    classify(&onto);
    // Assert the bloom was hit, not just constructed:
    assert!(crate::rules::__test_bloom_hits() > 0);
}
```

- [ ] **Step 4: Run canaries — expect failure**

```bash
cargo test -p owl-dl-tableau phase3_ -- --test-threads=1 2>&1 | tail -10
```

Verdict-preservation canary may pass already (unchanged behavior). Structural canary FAILS (the fix isn't wired yet). That's the gap — Task 4 closes it.

- [ ] **Step 5: Commit the canaries**

```bash
git add crates/owl-dl-tableau/src/<file>.rs
git commit -m "test(tableau): Phase 3 canaries — verdict preservation + structural use"
```

---

## Task 4: Implement the fix

**Files:**
- Modify: `crates/owl-dl-tableau/src/<file>.rs` (the file Task 2 chose).

- [ ] **Step 1: Implement per Task 2's design**

The exact code depends on the chosen fix. For the most likely shape (bloom prefilter in `needs_deferred_or`), the implementation:

1. Add a `label_set_bloom: u64` field to `Node` (or use the existing `label_sig` from B.4).
2. Update it incrementally on every `add_label` / `remove_label` (or recompute lazily).
3. In `needs_deferred_or`, check `if !label_set_bloom.contains(c.hash())` and skip the binary search.

For a different fix shape, follow Task 2's design.

CRITICAL invariant: the fix MUST NOT change VERDICTS. The structural change is purely about HOW the saturator finds the same answers, not WHICH answers it finds.

- [ ] **Step 2: Run both canaries — expect pass**

```bash
cargo test -p owl-dl-tableau phase3_ -- --test-threads=1 2>&1 | tail -10
```
Expected: both pass. Verdict-preservation confirms soundness; structural confirms the fix is wired.

- [ ] **Step 3: Run all tableau tests — soundness regression check**

```bash
cargo test -p owl-dl-tableau -- --test-threads=1 2>&1 | tail -10
```
Expected: all pre-existing tableau tests pass. A regression here is a real soundness or completeness problem with the fix.

- [ ] **Step 4: Run reasoner lib tests (broader regression)**

```bash
cargo test -p owl-dl-reasoner --lib -- --test-threads=1 2>&1 | tail -10
```
Expected: 78 tests pass (the post-Phase-2b baseline).

- [ ] **Step 5: CI strictness compile**

```bash
RUSTFLAGS="-D warnings" cargo test -p owl-dl-tableau --no-run 2>&1 | tail -3
cargo clippy -p owl-dl-tableau -- -D warnings 2>&1 | grep -E "warning|error" | grep -v "(too_many_lines|map_unwrap_or|doc-markdown)" | head -5
```
Expected: clean; no new clippy on the new code.

- [ ] **Step 6: Commit**

```bash
git add crates/owl-dl-tableau/src/<file>.rs
git commit -m "perf(tableau): Phase 3 fix — <one-line summary from Task 2 doc>"
```

---

## Task 5: Re-flamegraph + corpus measurement

**Files:**
- Create: `docs/flamegraphs/galen-classify-2026-06-01-post-phase3.svg` + findings file (committed).
- Capture logs: `/tmp/p3-final-net.log`, `/tmp/p3-final-galen.log`.

- [ ] **Step 1: Rebuild with profile feature**

```bash
cargo build -p owl-dl-bench --release --features profile 2>&1 | tail -3
```

- [ ] **Step 2: Re-flamegraph GALEN under the fix**

```bash
RUSTDL_PROFILE=docs/flamegraphs/galen-classify-2026-06-01-post-phase3.svg \
RUSTDL_PROFILE_SECONDS=120 \
    ./target/release/owl-dl-bench classify ontologies/external/galen.ofn 2>&1 | tee /tmp/p3-final-flame.log | tail -10
```

- [ ] **Step 3: Extract top hot frames; compare to baseline**

```bash
python3 <<'EOF'
import re
def top_frames(path):
    with open(path) as f:
        svg = f.read()
    titles = re.findall(r'<title>([^<]+)</title>', svg)
    parsed = []
    for t in titles:
        m = re.match(r'(.+?)\s*\(([\d,]+)\s+samples,\s*([\d.]+)\s*%\)', t)
        if m:
            parsed.append((float(m.group(3)), int(m.group(2).replace(',', '')), m.group(1).strip()))
    return sorted(parsed, reverse=True)
baseline = top_frames('docs/flamegraphs/galen-classify-2026-06-01.svg')
post = top_frames('docs/flamegraphs/galen-classify-2026-06-01-post-phase3.svg')
print("BASELINE top 15:")
for pct, n, fname in baseline[:15]:
    print(f"  {pct:6.2f}%  {fname[:80]}")
print("\nPOST-PHASE-3 top 15:")
for pct, n, fname in post[:15]:
    print(f"  {pct:6.2f}%  {fname[:80]}")
EOF
```

Save the output. Expected: the targeted frame (e.g. `needs_deferred_or`'s `eq` cost, or `apply_deferred_concept_or_rules`'s % share) is meaningfully reduced — at least a few percentage points. If unchanged, the fix didn't fire on the hot path; investigate.

- [ ] **Step 4: Phase 0 net soundness gate**

```bash
cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    alehif_closure_matches_konclude ore_10908_sroiq ore_15672_shoin \
    -- --ignored --nocapture 2>&1 | tee /tmp/p3-final-net.log | grep -E "^---|FP=|MISSED="
```

Hard cap 30 min. Expected: FP=0 / MISSED=0 across all 3. Wall regressions ≤ 2× the post-2b baseline (alehif 2.72s, ORE-SROIQ 31.60s, ORE-SHOIN 29.71s).

If ANY fixture has FP > 0, that's a soundness regression. STOP — investigate the fix.

- [ ] **Step 5: GALEN wall measurement (the lever's payoff)**

```bash
timeout 2400 cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    galen_closure_matches_konclude \
    -- --ignored --nocapture 2>&1 | tee /tmp/p3-final-galen.log | grep -E "^--- galen|MISSED="
```

Hard cap 40 min. Expected:
- FP=0 (soundness).
- MISSED=17 (unchanged from post-2b).
- Wall: meaningfully reduced from 24.7 min toward 12.5 min baseline. Target: ≤20 min would be a solid first-fix win; ≤16 min would be excellent.

Record the actual wall.

- [ ] **Step 6: Write the findings + commit**

Update `docs/flamegraphs/galen-classify-2026-06-01-post-phase3.svg`'s sibling findings file with the diff. Then:

```bash
git add docs/flamegraphs/galen-classify-2026-06-01-post-phase3.svg \
        docs/flamegraphs/galen-classify-2026-06-01-post-phase3-findings.md
git commit -m "perf(phase3): GALEN re-flamegraph + measurement (post-fix)"
```

---

## Task 6: Results doc + close-out

**Files:**
- Create: `docs/phase3-results.md`.
- Modify: `CLAUDE.md` (saturator/tableau perf note).
- Modify: `docs/superpowers/specs/2026-05-31-soundness-completeness-perf-design.md` (Phase 3 close-out).

- [ ] **Step 1: Write `docs/phase3-results.md`**

Structure (fill values from Task 5):

```markdown
# Phase 3 — Tableau perf results

Run 2026-06-0N against the Phase 0 soundness net + GALEN. Fix:
<one-line summary>. See `docs/phase3-fix-target.md` for design.

## Headline finding

<one paragraph: did the fix reduce GALEN wall meaningfully? By
what percentage? Did the targeted flame frame shrink? Verdicts
unchanged?>

## Soundness gate (Phase 0 net)

| Fixture | Pre-2b MISSED | Post-2b MISSED | Post-Phase-3 MISSED | FP | Wall |
|---|---|---|---|---|---|
| alehif | 0 | 0 | 0 | 0 | <wall>s |
| ore-10908-sroiq | 0 | 0 | 0 | 0 | <wall>s |
| ore-15672-shoin | 0 | 0 | 0 | 0 | <wall>s |

FP=0 / MISSED-unchanged held across all fixtures.

## Wall lever (GALEN)

| Phase | Wall | MISSED | Δ wall vs pre |
|---|---|---|---|
| Pre-2b baseline | 12.5 min | 109 | — |
| Post-2b/2b.5 | 24.7 min | 17 | +12.2 min (~2×) |
| Post-Phase-3 fix | <wall> | 17 | <Δ> |

<one paragraph: by how much did the fix reduce the post-2b
regression? Did it close the gap to the pre-2b baseline?>

## Flamegraph diff

<paste the Step 3 comparison output from Task 5: top frames before
and after the fix>

## What's left

- The other hot frames named in the original GALEN flamegraph
  (apply_max heap allocs, first_clash re-scan, DepSet clone) remain
  un-attacked. Each is a follow-on Phase 3b/3c/3d.
- The C/D residual gaps (44 MISSED) remain for Phase 2c.
```

- [ ] **Step 2: Update CLAUDE.md**

Find the `crates/owl-dl-tableau` bullet in the "Workspace architecture" section. Append:

```
Phase 3 (commit <head SHA>) reduced GALEN classify wall by <N>%
via <one-line fix summary>; verdicts unchanged. See
`docs/phase3-results.md`.
```

- [ ] **Step 3: Update design spec**

In `docs/superpowers/specs/2026-05-31-soundness-completeness-perf-design.md`, find the `Phase 3 — Saturator performance` bullet. Append:

```
Landed: `docs/phase3-results.md`. Empirical Phase 3 target turned
out to be the TABLEAU, not the EL saturator — the spec's "saturator
dominates" inference was true pre-Phase-2 but Phase 2b's calculus
extensions shifted the hot path to `apply_deferred_concept_or_rules`
and related. GALEN wall: <pre> → <post>, FP=0 held. Phase 3b+
(remaining hot frames) queued.
```

- [ ] **Step 4: Commit**

```bash
git add docs/phase3-results.md \
        CLAUDE.md \
        docs/superpowers/specs/2026-05-31-soundness-completeness-perf-design.md
git commit -m "docs(phase3): results doc + envelope updates"
```

---

## Definition of done (Phase 3 — first fix)

- SIO flamegraph generated; spec's saturator-dominance claim verified or refuted (Task 1).
- First fix chosen + designed based on flamegraph data (Task 2).
- TDD canaries land before the fix; both pass post-fix (Tasks 3-4).
- Re-flamegraph confirms the targeted frame shrank (Task 5).
- GALEN wall reduced from 24.7 min; verdicts unchanged (MISSED=17, FP=0); Phase 0 net FP=0 held (Tasks 5-6).
- Phase 3 results doc + CLAUDE.md + design spec updated (Task 6).

Phase 3 follow-ons (3b, 3c, ...) target the remaining hot frames per the same discipline.

## What this plan does NOT do

- Does NOT attack ALL hot frames; one fix per plan, measurement-gated.
- Does NOT touch the EL saturator (the GALEN flamegraph showed it's not the bottleneck; SIO data in Task 1 will tell us if SIO is different — if yes, a separate Phase 3-saturator plan can address SIO).
- Does NOT extend Phase 2 (no MISSED recovery is in scope).
- Does NOT change verdicts on any fixture (the soundness + completeness gates are unchanged from Phase 2b).
