# Konclude snapshot cache — Phase 2b Implementation Plan (Horn short-circuit)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** For Horn-fragment ontologies (GALEN, notgalen, alehif, etc.), dispatch classify to the existing `classify_saturation_only_internal` fast path instead of the per-pair verification loop. Per Phase 2a recon (`docs/phase2a-recon.md`), this brings GALEN wall from **155.95s → ~0.48s** (325× speedup; closure 27,997 = Konclude).

**Architecture:** One-line change to the existing pure-EL short-circuit at `classify.rs:479` and `:817`. Change the gating predicate from `is_pure_el(internal)` to "PureEl OR Horn" via the existing `FragmentClassification` enum. Plus an env gate `RUSTDL_HORN_SHORTCIRCUIT` (default ON) for A/B isolation.

**Tech Stack:** Rust 1.88+. No new deps. Pure orchestrator change.

**Spec:** [docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md](../specs/2026-06-03-konclude-style-global-classification-design.md) §5 (re-framed per Phase 2a recon) + §6 Phase 2b.

**Predecessor:** Phase 2a recon (commit `e5f0519`). Empirical validation: `rustdl classify --saturation-only` on GALEN produces 27,997 subsumptions in 0.48s. The existing `classify_pure_el` function emits the closure-derived classification; it's sound on any Horn-complete saturator output.

---

## Why this is small (and why the recon kept it from being multi-month)

The Phase 2a recon caught the spec §5 misframing: label-cache-build is 0.2% of wall, not ~30%. The dominant cost is the per-pair replay path (1.86M calls × ~1.44ms CPU). Killing the per-pair loop entirely on Horn ontologies — via the existing saturator-complete short-circuit — gives massive wall savings with a tiny code change.

Spec §5's original Layer 2 design (a new TBox-wide saturation candidate filter) would have been multi-session work. The recon's empirical validation showed the existing saturator ALREADY produces the full Horn closure; we just don't use it. Phase 2b is "use it".

---

## Soundness contract

`classify_pure_el(internal, classes, index, closure)` already reads only from the saturation closure and emits classification. Its soundness depends entirely on the closure being **complete** for the ontology.

- For `FragmentClassification::PureEl`: closure is complete by saturator construction (existing invariant).
- For `FragmentClassification::Horn`: closure is complete by the hyper Horn fixpoint's soundness-and-completeness on Horn DL-clauses. The existing `# fragment: Horn (sound by construction; hyper Horn fixpoint is complete)` banner line is the proof.

Both fragments are sound. The change is correct by composition: the gating predicate widens from PureEl-only to PureEl-or-Horn; the body (`classify_pure_el`) is unchanged.

**Inv-2b:** corpus diff verdicts match between (a) `RUSTDL_HORN_SHORTCIRCUIT=1` (Phase 2b default) and (b) `RUSTDL_HORN_SHORTCIRCUIT=0` (Phase 1c default) on every fixture with a pinned Konclude closure. Tested via the existing `tests/konclude_closure_diff.rs` suite.

---

## File structure

**Modified files:**
- `crates/owl-dl-reasoner/src/classify.rs` — widen the pure-EL short-circuit predicate to PureEl-or-Horn (two call sites: lines ~479 and ~817).
- `crates/owl-dl-reasoner/src/lib.rs` — add `horn_shortcircuit_enabled()` env helper (default ON; `RUSTDL_HORN_SHORTCIRCUIT=0` opts out).
- `CLAUDE.md` — soundness contract bullet documenting the new env default.

**New files:**
- `docs/phase2b-results.md` — project-headline results doc.

**Test files extended:**
- `crates/owl-dl-reasoner/tests/snapshot_phase0_canary.rs` — add a Phase 2b assertion that GALEN-shaped Horn classifies via the saturation fast path under the new default (verifies `stats.pure_el_mode` or new equivalent flag).

---

### Task 1: Wire the Horn short-circuit + env gate

**Goal:** the load-bearing change. Widen the pure-EL gating; add env helper; verify Phase 0 net soundness; verify GALEN soundness; verify A/B toggle works.

**Files:**
- Modify: `crates/owl-dl-reasoner/src/classify.rs` (two call sites)
- Modify: `crates/owl-dl-reasoner/src/lib.rs` (env helper)

- [ ] **Step 1: Add `horn_shortcircuit_enabled` env helper**

In `crates/owl-dl-reasoner/src/lib.rs`, near `snapshot_capture_enabled` / `snapshot_lazy_enabled`:

```rust
/// Phase 2b: for ontologies classified as `Horn` fragment (the
/// hyper Horn fixpoint is complete by construction), dispatch
/// classify to the saturation-only fast path instead of the
/// per-pair verification loop. **Default ON** as of Phase 2b
/// (project-headline landing); set `RUSTDL_HORN_SHORTCIRCUIT=0`
/// to revert to the pre-Phase-2b per-pair loop for Horn
/// ontologies (A/B isolation).
///
/// Sibling-style env helper: any non-empty, non-`"0"` value
/// (`=1`/`=true`/`=yes`/`=on`) keeps it ON; only `=0` or empty
/// disables.
///
/// Spec: `docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md` §5
/// Recon: `docs/phase2a-recon.md`
#[must_use]
pub fn horn_shortcircuit_enabled() -> bool {
    std::env::var_os("RUSTDL_HORN_SHORTCIRCUIT").map_or(true, |v| v != "0" && !v.is_empty())
}
```

- [ ] **Step 2: Widen the gating in `classify_internal_with_timeout`**

In `crates/owl-dl-reasoner/src/classify.rs`, find the existing pure-EL check at approximately line 479 (use grep `is_pure_el(internal)` to locate exact line; T1 instrumentation may have shifted line numbers slightly):

```rust
// before:
if is_pure_el(internal) {
    return Ok(classify_pure_el(internal, &classes, &index, &closure));
}

// after:
let fragment = analyze_fragment(internal);
if matches!(fragment, FragmentClassification::PureEl)
    || (matches!(fragment, FragmentClassification::Horn)
        && crate::horn_shortcircuit_enabled())
{
    return Ok(classify_pure_el(internal, &classes, &index, &closure));
}
```

Apply the SAME change to the second site at approximately line 817 (the duplicate in `classify_top_down_internal`). Both sites guard the pure-EL fast path; both must widen consistently.

`analyze_fragment` is already imported (it's a sibling fn in the same module).

**Naming note**: `classify_pure_el` is now misnamed (it handles Horn too). Consider renaming to `classify_via_saturation_closure` in a follow-up commit. NOT required for Phase 2b T1 — the function body is identical regardless of fragment, and renaming risks merge conflicts with other in-flight work. Defer.

- [ ] **Step 3: Verify Phase 0 net soundness with new default**

```bash
export PATH=/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH
mkdir -p /tmp/p2b
cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    -- --ignored --nocapture alehif_closure_matches_konclude \
    ore_10908_sroiq ore_15672_shoin 2>&1 | tee /tmp/p2b/phase0-default-on.log
```

Expected: FP=0/MISSED=0 across all three.
- alehif (Horn) hits the new fast path; closure should stay at 247=Konclude.
- ore-10908 (out-of-EL) stays on the per-pair path; closure 6001=Konclude unchanged.
- ore-15672 (out-of-EL) stays on the per-pair path; closure 142=Konclude unchanged.

If alehif regresses on FP/MISSED, the saturator's Horn closure differs from the per-pair-derived closure. Investigate before commit (would indicate a long-standing latent bug in the Horn-completeness claim, or a clausifier edge case).

- [ ] **Step 4: GALEN soundness gate (load-bearing)**

```bash
timeout 1500 cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    galen_closure_matches_konclude -- --exact --ignored --nocapture 2>&1 | \
    tee /tmp/p2b/galen-default-on.log
```

Expected: FP=0/MISSED=0, closure 27,997=Konclude, wall **< 5s** (vs Phase 1c 155.95s). The recon validated 0.48s for `--saturation-only`; the closure-diff test adds parsing + comparison overhead, but total should still be < 5s.

If wall is anywhere near pre-project (155s), the gate didn't actually fire — debug `analyze_fragment(GALEN)` returns `Horn`, and trace which branch the orchestrator took.

If FP > 0 or MISSED > 0, **STOP** — Horn-fragment classification produces a verdict that doesn't match the per-pair loop's verdict. This is unexpected per the recon's empirical run; investigate before commit.

- [ ] **Step 5: A/B verification with `RUSTDL_HORN_SHORTCIRCUIT=0`**

```bash
RUSTDL_HORN_SHORTCIRCUIT=0 timeout 1500 cargo test -p owl-dl-reasoner \
    --test konclude_closure_diff --release \
    galen_closure_matches_konclude -- --exact --ignored --nocapture 2>&1 | \
    tee /tmp/p2b/galen-shortcircuit-off.log
```

Expected: FP=0/MISSED=0, wall ~155s (matches Phase 1c). This proves the toggle works — Phase 2b can be reverted via env for debugging.

- [ ] **Step 6: notgalen soundness gate**

```bash
timeout 2400 cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    notgalen_closure_matches_konclude -- --exact --ignored --nocapture 2>&1 | \
    tee /tmp/p2b/notgalen-default-on.log
```

Expected: FP=0 / MISSED ≤ 18 (matches Phase 7 baseline; the 18 MISSED are dl-approximation artifacts pre-dating this project). Wall: should drop dramatically (notgalen is Horn; recon-style projection predicts ≤2s based on closure size).

If MISSED jumps above 18, the Horn closure is incomplete on notgalen's structure somehow — this would invalidate the recon's "Horn = sound + complete" assumption. STOP and investigate before commit.

- [ ] **Step 7: Canary tests still pass**

```bash
cargo test -p owl-dl-reasoner --test snapshot_phase0_canary 2>&1 | tail -5
```

Expected: 4/4 pass. The canary tests don't depend on the Horn-fragment gating directly; should be unaffected.

- [ ] **Step 8: Clippy + fmt (crate-scope) + commit**

```bash
cargo clippy -p owl-dl-reasoner --lib --tests -- -D warnings 2>&1 | grep "^error" | grep -v saturation | head -3
cargo fmt -p owl-dl-reasoner -- --check
git add crates/owl-dl-reasoner/src/lib.rs crates/owl-dl-reasoner/src/classify.rs
git commit -m "$(cat <<'EOF'
feat(snapshot): Horn fragment short-circuit to saturation fast path (Phase 2b T1)

Widens the existing pure-EL short-circuit (classify.rs:479, :817) to
also dispatch Horn-fragment ontologies (GALEN, notgalen, alehif) to
classify_pure_el (which reads the saturation closure directly).

For Horn fragments, the hyper Horn fixpoint is complete by
construction (per existing # fragment: Horn (sound + complete)
banner). The per-pair verification loop the orchestrator was
running on every Horn ontology was therefore redundant work —
1.86M pair calls on GALEN × ~1.44ms CPU each = 2,686 CPU-seconds
of wasted effort.

GALEN wall (default-on): <wall>s vs Phase 1c 155.95s
(<delta>× speedup; closure 27,997=Konclude).
notgalen wall: <wall>s vs Phase 1c 342.15s.
alehif/ore-10908/ore-15672 unchanged (out-of-EL stays on per-pair path).

RUSTDL_HORN_SHORTCIRCUIT (default ON) provides A/B isolation.
RUSTDL_HORN_SHORTCIRCUIT=0 reverts to Phase 1c per-pair loop for
Horn fragments; verified the toggle works.

Spec: docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md §5
Recon: docs/phase2a-recon.md

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

Fill `<wall>` and `<delta>` from the logs.

---

### Task 2: Full-corpus matrix + Phase 2b results doc + CLAUDE.md update

**Goal:** measure every fixture under the new default; verify spec §7 acceptance; write project-headline-followup doc.

**Files:**
- Create: `docs/phase2b-results.md`
- Modify: `CLAUDE.md` (add `RUSTDL_HORN_SHORTCIRCUIT` to the soundness contract section)

- [ ] **Step 1: Full-corpus matrix run**

Run every fixture in the corpus under default ON. Capture wall + soundness banner lines. Compare against Phase 1c baseline (`docs/phase1c-results.md` has the table).

Use the script pattern from Phase 1c T2 (the matrix runner). Both flag-on and flag-off (RUSTDL_HORN_SHORTCIRCUIT=0) for each fixture, so we have a clean delta.

Specifically pay attention to:
- **Horn fixtures** (alehif, GALEN, notgalen): big wall reductions expected.
- **PureEl fixtures**: unchanged (they already hit the existing pure-EL short-circuit pre-Phase-2b).
- **OutOfFragment fixtures** (pizza, ore-10908, ore-15672, etc.): unchanged (Horn check returns false; stays on per-pair path).

Any wall regression > 10% on a non-Horn fixture would be a surprise (and a bug — the new gating shouldn't affect non-Horn ontologies). Investigate if found.

- [ ] **Step 2: Write `docs/phase2b-results.md`**

Mirror the shape of `docs/phase1c-results.md`. Sections:

- Headline: GALEN <wall>s vs 155.95s, notgalen <wall>s vs 342.15s; spec §7 acceptance.
- Per-fixture matrix table.
- Spec §7 acceptance verification (the 4 bullets — FP=0/MISSED=0, all tests pass, clippy clean for changed crates, no fixture > 10% regression).
- Spec §6 outcome-band attribution: GALEN now likely below 150s → "Ship + proceed to Phase 2a" band (originally placed in 150-300s mandatory-Phase-2 band by Phase 1c results; Phase 2b moves it into the ship+proceed band).
- Recon validation: empirical wall matches recon projection? (Recon predicted ~0.48s saturator-only; closure-diff overhead adds parsing).
- Project arc summary table (extended with Phase 2b).
- Carry-overs for Phase 3 (SROIQ classifier loosening per spec §6).

- [ ] **Step 3: Update CLAUDE.md**

In the soundness contract section, add the new env default:

```markdown
- **New as of Phase 2b**: `RUSTDL_HORN_SHORTCIRCUIT` defaults ON.
  For ontologies classified as `Horn` fragment, the classifier
  dispatches directly to the saturation closure (sound + complete on
  Horn by construction). Set `RUSTDL_HORN_SHORTCIRCUIT=0` to revert
  to the Phase 1c per-pair loop for Horn fragments. See
  `docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md` §5
  + `docs/phase2a-recon.md`.
```

Place alongside the existing snapshot-cache + snapshot-lazy bullets.

- [ ] **Step 4: Commit**

```bash
git add docs/phase2b-results.md CLAUDE.md
git commit -m "$(cat <<'EOF'
docs(phase2b): project-headline followup — Horn short-circuit shipped

Phase 2b dispatches Horn-fragment ontologies (GALEN, notgalen,
alehif) to the saturation-only fast path. Full-corpus matrix:
GALEN <wall>s (vs Phase 1c 155.95s, <delta>× speedup), notgalen
<wall>s (vs 342.15s), all non-Horn fixtures unchanged.

Spec §7 acceptance: MET. Spec §6 outcome band: GALEN now in
≤150s "Ship + proceed to Phase 2a" band (vs Phase 1c's 150-300s
"mandatory Phase 2 build" band — Phase 2b IS the Phase 2 build,
just smaller-than-expected per recon empirical kicker).

CLAUDE.md: added RUSTDL_HORN_SHORTCIRCUIT to the env defaults bullet
list in the soundness contract section.

Project arc: Phase 0+1a → 1b → 1b.5 → 1c → 2a recon → 2b shipped.
Phase 3 (loosen BackPropRisk classifier for SROIQ workloads) is
the queued next work.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## After all tasks

Phase 2b complete. The project's "snapshot cache" arc is now substantially complete:
- Phase 1a/1b/1b.5/1c: snapshot infrastructure (sound on SROIQ; modest perf change on Horn).
- Phase 2b: Horn short-circuit (huge perf win on Horn; pre-existing saturator-complete fact made shippable).

Remaining work:
- **Phase 3**: loosen `BackPropRisk` classifier for SROIQ workloads (ore-15672, pizza, ore-10908). Targets the residual SROIQ wall gaps. The runtime sentinel (Phase 1b T3) is the safety net. Multi-session work per spec §6.
- **Code cleanup carry-overs**:
  - Rename `classify_pure_el` → `classify_via_saturation_closure` (the name is now misleading since Horn calls it too).
  - Consider revising `pure_el_mode: bool` on ClassificationStats to `via_saturation_closure: bool` or similar.
- **Phase 2a recon instrumentation**: 4 wall-breakdown fields landed; keep as profiling telemetry per recon doc's decision.

Phase 2b project-headline status: **substantial speedup with minimal code change** — the project's payoff materialized after the recon caught the spec §5 misframing. Recon-first discipline saved a multi-session implementation that would have re-built the wrong lever.
