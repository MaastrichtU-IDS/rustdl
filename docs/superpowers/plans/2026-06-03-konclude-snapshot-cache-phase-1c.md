# Konclude snapshot cache — Phase 1c Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Flip `RUSTDL_SNAPSHOT_CAPTURE` default from OFF to ON; verify the full corpus matrix passes spec §7 acceptance (FP=0 + MISSED=0 + no fixture > 10% wall regression); write the project-headline results doc + handoff for Phase 2.

**Architecture:** One-line change to `snapshot_capture_enabled()`. Plus a flip of the corresponding Phase 0 canary test (which currently asserts default OFF). Plus orchestrator-side careful measurement on small workloads (alehif, pizza, ore-15516, etc.) where cache-build overhead could exceed the per-pair savings.

**Tech Stack:** Rust 1.88+. No new deps. Pure behavior change at the env-helper level.

**Spec:** [docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md](../specs/2026-06-03-konclude-style-global-classification-design.md) §6 Phase 1c row + §7 (project-level acceptance).

**Predecessors:**
- Phase 1b.5 results: `docs/phase1b5-results.md` (GALEN 154s flag-ON / lazy ON ≈ flag-OFF baseline 149s).
- Phase 1b.5 places GALEN in the spec §6 Phase 1c **150-300s outcome band → Ship + mandatory Phase 2 build**.
- Pre-project baseline: `docs/perf-2026-06-03-konclude-vs-rustdl.md`.

---

## Scope decision: default-on without further perf work

Per Phase 1b.5 results, GALEN is right at the 150-300s band boundary
(154.44s in T5 rerun, 149.92s in T3 rerun — boundary-band noise).
The spec §A revert criterion is `GALEN > 300s after recon-driven
tuning` — comfortably not firing.

The §7 project-level non-regression bound is `≤ 10%` per fixture. The
load-bearing checks for Phase 1c:

| Fixture | Phase 1b.5 wall (flag ON, lazy ON) | §7 bound |
|---|---:|---|
| GALEN (Horn, large) | 154.44 s | flag-OFF 149s; +3.7% (well inside ≤10%) |
| notgalen (Horn, large) | 337.58 s | no clean flag-OFF baseline; safely under §7 ≤400s |
| alehif (Horn, small) | 6.58 s | flag-OFF baseline TBD this phase |
| ore-10908 (SROIQ → Unsafe → no-op) | 13.05 s | flag-OFF baseline TBD; should be near-identical |
| ore-15672 (SROIQ → Unsafe → no-op) | 34.11 s | flag-OFF baseline TBD; should be near-identical |
| pizza, sio-fp2-module, etc. | unmeasured this project | needs Phase 1c measurement |

**Risk:** small Horn workloads (alehif, ro-stripped) pay eager
snapshot-build cost (one wedge per class) on top of Phase 7's label
cache wedge work — could nearly double the small-workload classify
wall. The Phase 1c measurement will catch this; if a small fixture
regresses > 10%, revert or scope the flip to "only enable on N >
some-threshold classes."

---

## File structure

**Modified files:**
- `crates/owl-dl-reasoner/src/lib.rs` — `snapshot_capture_enabled` returns true by default; doc-comment updated.
- `crates/owl-dl-reasoner/tests/snapshot_phase0_canary.rs` — the "defaults OFF" assertion flips to "defaults ON"; the unsafe-ontology no-op test still relies on `snapshot_capture_enabled` being ON, which it now is by default — so the explicit `SetEnvGuard::set("RUSTDL_SNAPSHOT_CAPTURE", "1")` calls can be removed or kept for explicit-test-vs-implicit-default isolation (recommend keeping for robustness against future flips).
- `CLAUDE.md` — `## Soundness contract (important)` section: add the new env default.

**New files:**
- `docs/phase1c-results.md` — project-level headline results doc.

---

### Task 1: Flip `snapshot_capture_enabled` default + canary updates

**Goal:** the one-line behavior change. Update the canary test to reflect the new default.

**Files:**
- Modify: `crates/owl-dl-reasoner/src/lib.rs` (the helper + docstring)
- Modify: `crates/owl-dl-reasoner/tests/snapshot_phase0_canary.rs` (the default-OFF assertion)

- [ ] **Step 1: Flip the env helper**

In `crates/owl-dl-reasoner/src/lib.rs`, locate `pub fn snapshot_capture_enabled` (line ~673 area). Change the body:

```rust
// before:
std::env::var_os("RUSTDL_SNAPSHOT_CAPTURE").is_some_and(|v| v != "0" && !v.is_empty())

// after:
std::env::var_os("RUSTDL_SNAPSHOT_CAPTURE").map_or(true, |v| v != "0" && !v.is_empty())
```

`is_some_and` returns false on unset; `map_or(true, ...)` returns true on unset. Updates the default-OFF semantics to default-ON, preserving the explicit `=0` opt-out.

Update the docstring (currently says "Default OFF; Phase 1c flips the default" — now Phase 1c has flipped it):

```rust
/// Project flag for the Konclude snapshot cache. When ON,
/// `subsumes_via_tableau` consults a per-class snapshot-replay cache
/// ahead of the wedge. Default ON as of Phase 1c (project-headline
/// landing); set `RUSTDL_SNAPSHOT_CAPTURE=0` to revert to pre-project
/// behavior (no snapshot cache; pure wedge per pair).
///
/// Sibling-style env helper: accepts any non-empty, non-`"0"` value
/// (`=1`/`=true`/`=yes`/`=on`); only `=0` or empty disables.
///
/// Spec: `docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md`
```

- [ ] **Step 2: Update Phase 0 canary's default-OFF assertion**

In `crates/owl-dl-reasoner/tests/snapshot_phase0_canary.rs`, locate `snapshot_capture_flag_defaults_off`. The test name itself is now wrong — rename to `snapshot_capture_flag_defaults_on`:

```rust
#[test]
fn snapshot_capture_flag_defaults_on() {
    // Phase 1c (project-headline): env flag defaults ON. Set
    // RUSTDL_SNAPSHOT_CAPTURE=0 to opt back into pre-project
    // pure-wedge behavior.
    assert!(
        std::env::var("RUSTDL_SNAPSHOT_CAPTURE").is_err(),
        "Phase 1c canary: RUSTDL_SNAPSHOT_CAPTURE must not be set in the test env"
    );
    assert!(snapshot_capture_enabled(), "default must be ON (Phase 1c)");
}
```

The other 3 tests (`classify_unchanged_with_flag_off`, `replay_returns_subsumed_on_horn_chain_with_flag_on`, `replay_no_op_on_unsafe_ontology_with_flag_on`) need scrutiny:

- `classify_unchanged_with_flag_off`: this test asserts classify still produces correct verdicts. With flag now defaulting to ON, this test currently exercises the FLAG-ON path. Rename it to `classify_unchanged_with_default` or leave as-is (the test's assertion is correctness-only, not flag-state-conditional). Recommend rename for clarity.
- `replay_returns_subsumed_on_horn_chain_with_flag_on`: sets `RUSTDL_SNAPSHOT_CAPTURE=1` explicitly via guard. Now redundant but still correct. Leave as-is OR remove the explicit `set` call (test would still pass with the new default). Recommend leaving the explicit set: documents intent and survives a future default flip.
- `replay_no_op_on_unsafe_ontology_with_flag_on`: same as above. Leave.

- [ ] **Step 3: Run the canary**

```bash
export PATH=/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH
cargo test -p owl-dl-reasoner --test snapshot_phase0_canary 2>&1 | tail -10
```

Expected: 4/4 pass with the renamed/updated test.

- [ ] **Step 4: Verify Phase 0 net soundness with new default**

```bash
mkdir -p /tmp/p1c
cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    -- --ignored --nocapture alehif_closure_matches_konclude \
    ore_10908_sroiq ore_15672_shoin 2>&1 | tee /tmp/p1c/soundness-default-on.log
```

Note: no `RUSTDL_SNAPSHOT_CAPTURE=1` env prefix — the flag is now default ON. Expected: FP=0/MISSED=0 across all three. This is the Inv-2 corpus invariant under the new default.

If any fixture regresses on FP/MISSED, **STOP** — Phase 1c's flip exposed a soundness gap that Phase 1b.5's explicit-flag-ON runs missed. Investigate before commit.

- [ ] **Step 5: Crate-scope clippy + fmt + commit**

```bash
cargo clippy -p owl-dl-reasoner --lib --tests -- -D warnings 2>&1 | grep "^error" | grep -v "saturation" | head -5
cargo fmt -p owl-dl-reasoner -- --check
git add crates/owl-dl-reasoner/src/lib.rs crates/owl-dl-reasoner/tests/snapshot_phase0_canary.rs
git commit -m "$(cat <<'EOF'
feat(snapshot): flip RUSTDL_SNAPSHOT_CAPTURE default to ON (Phase 1c)

Project-headline behavior change: snapshot cache is now the default
path for `subsumes_via_tableau`. Set RUSTDL_SNAPSHOT_CAPTURE=0 to
revert to pre-project pure-wedge behavior.

Canary test renamed: snapshot_capture_flag_defaults_off →
snapshot_capture_flag_defaults_on, asserts the new default.

Phase 0 net soundness (default-on, no env): FP=0/MISSED=0 on
alehif + ore-10908 + ore-15672.

Spec: docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md §6 Phase 1c + §7

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: Full-corpus measurement matrix

**Goal:** measure every fixture in the corpus with the new default; populate the results doc with per-fixture wall deltas; identify any > 10% regressions for revert/scope decisions.

**Files:**
- (no code changes — measurement only)
- Updates `docs/phase1c-results.md` (created in Task 3 with the data from Task 2's logs)

**Fixtures to measure** (per `docs/perf-2026-06-03-konclude-vs-rustdl.md`):

| Fixture | Notes |
|---|---|
| alehif-test | small Horn (167 classes) — risk: cache-build overhead |
| anch-module | small SROIQ (12 classes) — should be unsafe, near-zero impact |
| asp-module | small SROIQ (20 classes) |
| sulo-stripped | tiny SROIQ (17 classes) |
| sio-fp2-module | small SROIQ (74) |
| ro-stripped | small SROIF (58) — Horn-ish? check BackPropRisk |
| ore-15516-alchoiq | small SROIQ (84) |
| np-module | small SROIQ (34) |
| pizza | medium SROIQ (99) — unsafe |
| ore-10908-sroiq | medium SROIQ (693) — unsafe |
| family-stripped | medium SROIF (58) |
| ore-15672-shoin | small SROIQ (82) — unsafe |
| sio-stripped | large mixed (1585) |
| **GALEN** | **large Horn (2748) — load-bearing** |
| **notgalen** | **large Horn (3087) — load-bearing** |

- [ ] **Step 1: Run small-fixture corpus matrix (default ON, no env)**

```bash
export PATH=/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH
for fixture in alehif-test anch-module asp-module sulo-stripped sio-fp2-module \
               ro-stripped ore-15516-alchoiq np-module pizza ore-15672-shoin; do
  echo "=== $fixture (default ON) ===" | tee -a /tmp/p1c/small.log
  /usr/bin/time -f "wall=%es" ./target/release/rustdl classify --pair-timeout-ms 200 \
      ontologies/external/$fixture.ofn 2>&1 | \
      grep -E "^# (classes|fragment|subsumption|label heuristic|pairs-per-sub|wedge-cost)|wall=" \
      | tee -a /tmp/p1c/small.log
  echo "" | tee -a /tmp/p1c/small.log
done
```

Build with `cargo build --release -p owl-dl-cli` first if not already current.

If a fixture isn't present, skip it (some are renamed; grep `ontologies/external/` for similar names).

- [ ] **Step 2: Run small-fixture matrix with explicit OFF (baseline)**

```bash
for fixture in alehif-test anch-module asp-module sulo-stripped sio-fp2-module \
               ro-stripped ore-15516-alchoiq np-module pizza ore-15672-shoin; do
  echo "=== $fixture (flag OFF) ===" | tee -a /tmp/p1c/small-baseline.log
  RUSTDL_SNAPSHOT_CAPTURE=0 /usr/bin/time -f "wall=%es" ./target/release/rustdl classify \
      --pair-timeout-ms 200 ontologies/external/$fixture.ofn 2>&1 | \
      grep -E "^# (classes|fragment|subsumption|label heuristic)|wall=" | \
      tee -a /tmp/p1c/small-baseline.log
  echo "" | tee -a /tmp/p1c/small-baseline.log
done
```

This gives a per-fixture wall delta. Compute: `(default_on - flag_off) / flag_off * 100` for each. Flag any > 10% in the results doc.

- [ ] **Step 3: GALEN + notgalen soundness gate with default ON**

```bash
timeout 1500 cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    galen_closure_matches_konclude -- --exact --ignored --nocapture 2>&1 | tee /tmp/p1c/galen.log
timeout 2400 cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    notgalen_closure_matches_konclude -- --exact --ignored --nocapture 2>&1 | tee /tmp/p1c/notgalen.log
```

Expected: FP=0/MISSED ≤ baseline. Wall: GALEN ≤ 165s (10% over 149s flag-OFF baseline); notgalen ≤ 400s.

- [ ] **Step 4: Identify any > 10% regressions**

From the logs, compute per-fixture (default_on - flag_off) / flag_off. Any > 10% is a Phase 1c blocker per spec §7. Three options for each blocker:
1. Revert the default flip if the regression is widespread.
2. Scope the flip to "only enable on N > X classes" via a heuristic gate in `snapshot_capture_enabled`.
3. Document the regression as acceptable trade-off if isolated and the wall delta is small absolute (e.g., 0.5s on a 5s fixture).

**No commit yet** — Task 3 writes the results doc + commits.

---

### Task 3: Phase 1c results doc + CLAUDE.md update + commit

**Goal:** project-headline results doc; CLAUDE.md update mentioning the new default; commit.

**Files:**
- Create: `docs/phase1c-results.md`
- Modify: `CLAUDE.md` — `## Soundness contract (important)` section

- [ ] **Step 1: Write results doc**

Create `docs/phase1c-results.md` mirroring the shape of `docs/phase1b5-results.md`. Sections:

- Headline: default-on shipped; project-level §7 acceptance status; any regressions called out clearly.
- Per-fixture measurement matrix (table): fixture | classes | flag-OFF wall | default-ON wall | Δ % | FP | MISSED.
- Spec §7 acceptance bullet list with verification per item.
- Honest framing of where the project landed vs spec §6 Phase 1c outcome bands:
  - "≤ 150s ship+proceed" missed by 4-5s on GALEN — landed in "150-300s mandatory Phase 2" band.
  - Phase 2 (Layer 2 global saturation filter) is now mandatory next work.
- Commits list (this phase: Task 1's flip + Task 3's docs).
- Cross-references to the project's full arc (1a → 1b → 1b.5 → 1c).
- Phase 2 plan reference (placeholder if not yet written).

- [ ] **Step 2: Update CLAUDE.md**

Locate `## Soundness contract (important)` in `CLAUDE.md`. Add the snapshot cache to the env-var list:

```markdown
* **New as of Phase 1c (project-headline)**: `RUSTDL_SNAPSHOT_CAPTURE`
  defaults ON. The classify path consults a per-class snapshot cache
  ahead of the wedge for `BackPropRisk::Safe` ontologies (Horn-only
  in the first-cut classifier). Set `RUSTDL_SNAPSHOT_CAPTURE=0` to
  revert. `RUSTDL_SNAPSHOT_LAZY` also defaults ON (Phase 1b.5 lazy
  expansion); set to `0` to revert to Phase 1b full-re-run for A/B.
  See `docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md`.
```

Also update the relevant CLAUDE.md "Commands" section if there's a `classify` command example — note that the snapshot cache is now default ON (no env var needed to enable).

- [ ] **Step 3: Commit**

```bash
git add docs/phase1c-results.md CLAUDE.md
git commit -m "$(cat <<'EOF'
docs(phase1c): project-headline results — snapshot cache default ON shipped

Phase 1c flips RUSTDL_SNAPSHOT_CAPTURE default OFF → ON; this doc
captures the project-level §7 acceptance verification across the
full corpus matrix.

Per-fixture measurements: <summary>. GALEN <wall>s / notgalen
<wall>s with default ON; <regressions or none-flagged>. Spec §A
revert criterion (GALEN > 300s after tuning) far from firing.

Spec §6 outcome band: GALEN ~154s lands in 150-300s "Ship +
mandatory Phase 2 build" — Phase 1c shipped; Phase 2 (Layer 2
global saturation filter) is now the green-lit next work to close
the residual gap.

CLAUDE.md updated: snapshot-cache env defaults now documented in
the soundness contract section.

Project arc: Phase 0+1a → 1b → 1b.5 → 1c shipped across 4 plans
(docs/superpowers/plans/2026-06-03-konclude-*). Phase 2 plan to follow.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## After all tasks

Phase 1c complete. The snapshot cache is now default ON; the project's headline acceptance is verified.

Next-up plans (per spec §6, in priority order):

1. **Phase 2a recon**: is Layer 2 (global saturation candidate filter) worth building? Same recon-first discipline as Phase 1b.5: instrument label-cache build cost on GALEN, project Layer 2 savings, go/no-go.
2. **Phase 2b implementation** (if Phase 2a is GO): build the global saturation filter; replace the per-class wedge-cache-build cost with a single ontology-wide saturation pass.
3. **Phase 3**: loosen `BackPropRisk` classifier for SROIQ workloads (ore-15672, pizza) using the runtime sentinel as the safety net. Targets the residual SROIQ wall gap.

Each gets its own brainstorm → spec → plan cycle.
