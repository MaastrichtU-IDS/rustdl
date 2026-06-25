# Adaptive Label-Cache Deadline Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Make the per-class label-cache build deadline adaptive — `clamp(n_classes × per_pair, 1s floor, 30s ceiling)` — so MRV-tractable hard nominal classes get labeled (build-once: O(n) builds prune O(n²) pairs), capturing the validated ~13% wine wall win, soundly.

**Architecture:** Replace the fixed `label_cache_timeout_ms()` (default 1000ms) at the classify label-cache build site with an adaptive value computed from `n` (class count) and `per_pair_timeout` (both already in scope). Sound by construction (same Phase-7 label oracle, more complete → closure byte-identical). Env override `RUSTDL_LABEL_CACHE_TIMEOUT_MS` retained and still wins.

**Tech Stack:** Rust (edition 2024), crate `owl-dl-reasoner` (`classify.rs` build site + `lib.rs` consts/helper).

## Global Constraints

- Branch `feat/adaptive-label-cache` off `feat/build-once-redesign`.
- **Sound by construction** (perf-only): the change alters only how long the per-class label build runs, not what it computes. The label oracle prune (`D∉labels(C)` ⟹ counter-model ⟹ `C⋢D`) is the existing FP=0-validated mechanism; building more labels only moves genuine non-subsumptions from per-pair-refuted to oracle-pruned ⟹ **classification closure byte-identical (FP=0/MISSED=0 unchanged)**. The corpus gate (Task 2) confirms byte-identity.
- Ships as default behaviour (no flag — sound, floored strict improvement). Env override `RUSTDL_LABEL_CACHE_TIMEOUT_MS` retained (explicit value always wins).
- `cargo fmt --all -- --check` + `cargo clippy --workspace --all-targets --all-features -- -D warnings` clean; `cargo test --workspace` green.
- Toolchain (prefix every cargo command): `export RUSTUP_HOME=/home/dumontier/.rustup CARGO_HOME=/home/dumontier/.cargo; export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$CARGO_HOME/bin:$PATH"`
- Commit only when asked. Messages end with a blank line then:
  `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`
  `Claude-Session: https://claude.ai/code/session_01HSzon7V2wkhrudxBNAJduh`

---

## File Structure

- `crates/owl-dl-reasoner/src/lib.rs` — `pub(crate) fn adaptive_label_cache_ms(n: usize, per_pair: Option<std::time::Duration>, env_override: Option<u64>) -> u64` (pure, unit-tested) + consts `LABEL_CACHE_FLOOR_MS=1000`, `LABEL_CACHE_CEILING_MS=30_000`. Keep `label_cache_timeout_ms()` parsing for the env-override input.
- `crates/owl-dl-reasoner/src/classify.rs` — at the build site (~1221), use `adaptive_label_cache_ms(n, per_pair_timeout, env)` instead of the fixed `label_cache_timeout_ms()`.
- `docs/adaptive-label-cache-gate-results-2026-06-25.md` — Task 2 verdict.

---

### Task 1: `adaptive_label_cache_ms` pure function + wire it in + unit test

**Files:**
- Modify: `crates/owl-dl-reasoner/src/lib.rs` (near `label_cache_timeout_ms` ~1380)
- Modify: `crates/owl-dl-reasoner/src/classify.rs` (build site ~1221)
- Test: `crates/owl-dl-reasoner/src/lib.rs` (`#[cfg(test)]`)

**Interfaces:**
- Produces: `pub(crate) fn adaptive_label_cache_ms(n: usize, per_pair: Option<std::time::Duration>, env_override: Option<u64>) -> u64`. Semantics: if `env_override` is `Some(v)` → return `v` (explicit env wins, incl. `0` = unbounded sentinel preserved). Else `base = per_pair.map(|d| d.as_millis() as u64).unwrap_or(CEILING)`; return `(n as u64).saturating_mul(base).clamp(FLOOR, CEILING)`.

- [ ] **Step 1: Write the failing unit test**

```rust
#[test]
fn adaptive_label_cache_ms_branches() {
    use std::time::Duration;
    // env override always wins (incl. 0 = unbounded sentinel)
    assert_eq!(adaptive_label_cache_ms(137, Some(Duration::from_millis(200)), Some(7777)), 7777);
    assert_eq!(adaptive_label_cache_ms(137, None, Some(0)), 0);
    // n × per_pair, clamped to [1000, 30000]
    assert_eq!(adaptive_label_cache_ms(137, Some(Duration::from_millis(200)), None), 27_400); // 137*200
    assert_eq!(adaptive_label_cache_ms(137, Some(Duration::from_millis(1000)), None), 30_000); // 137000→ceiling
    assert_eq!(adaptive_label_cache_ms(2, Some(Duration::from_millis(200)), None), 1_000);     // 400→floor
    // None per_pair → base = ceiling, then ×n clamps to ceiling
    assert_eq!(adaptive_label_cache_ms(137, None, None), 30_000);
    assert_eq!(adaptive_label_cache_ms(1, None, None), 30_000); // 1*30000=30000
}
```

- [ ] **Step 2: Run it — expect compile failure** (`adaptive_label_cache_ms` absent).

Run: `cargo test -p owl-dl-reasoner --lib adaptive_label_cache_ms_branches -- --nocapture`

- [ ] **Step 3: Implement the function + consts**

In `lib.rs` near `label_cache_timeout_ms`:
```rust
pub(crate) const LABEL_CACHE_FLOOR_MS: u64 = 1000;
pub(crate) const LABEL_CACHE_CEILING_MS: u64 = 30_000;

/// Adaptive per-class label-cache build deadline (build-once tuning, 2026-06-25).
/// `env_override` (RUSTDL_LABEL_CACHE_TIMEOUT_MS) always wins — incl. `0` (unbounded).
/// Else `n × per_pair` (the refute-the-row break-even: labeling C is worth it iff its
/// `sat` costs less than refuting C's ~n pairs at the per-pair cap), clamped to
/// [floor, ceiling]. `per_pair == None` (unbounded refutations) → base = ceiling.
/// See docs/superpowers/specs/2026-06-25-adaptive-label-cache-deadline-design.md.
pub(crate) fn adaptive_label_cache_ms(
    n: usize,
    per_pair: Option<std::time::Duration>,
    env_override: Option<u64>,
) -> u64 {
    if let Some(v) = env_override {
        return v;
    }
    let base = per_pair
        .map_or(LABEL_CACHE_CEILING_MS, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
    (n as u64)
        .saturating_mul(base)
        .clamp(LABEL_CACHE_FLOOR_MS, LABEL_CACHE_CEILING_MS)
}
```
Add a parsing helper for the explicit env override (distinct from the legacy `label_cache_timeout_ms` which folds in the 1000 default). Provide `fn label_cache_env_override() -> Option<u64>`:
```rust
fn label_cache_env_override() -> Option<u64> {
    std::env::var("RUSTDL_LABEL_CACHE_TIMEOUT_MS").ok().and_then(|s| s.parse().ok())
}
```
(`label_cache_timeout_ms()` itself stays for any other callers, unchanged.)

- [ ] **Step 4: Wire it at the build site (`classify.rs` ~1221)**

Replace:
```rust
        let cache_ms = crate::label_cache_timeout_ms();
        let per_class_cache_dur = if cache_ms == 0 {
            None
        } else {
            Some(std::time::Duration::from_millis(cache_ms))
        };
```
with:
```rust
        // Adaptive build-once deadline (2026-06-25): scale to n × per_pair, clamped
        // [1s,30s]; env RUSTDL_LABEL_CACHE_TIMEOUT_MS overrides (0 = unbounded).
        let cache_ms = crate::adaptive_label_cache_ms(n, per_pair_timeout, crate::label_cache_env_override());
        let per_class_cache_dur = if cache_ms == 0 {
            None
        } else {
            Some(std::time::Duration::from_millis(cache_ms))
        };
```
(`n` and `per_pair_timeout` are both in scope in `classify_top_down_internal`.) If `adaptive_label_cache_ms`/`label_cache_env_override` need `pub(crate)` visibility from `classify.rs`, they already are (same crate).

- [ ] **Step 5: Run unit test + workspace**

Run: `cargo test -p owl-dl-reasoner --lib adaptive_label_cache_ms_branches -- --nocapture` → PASS.
Run: `cargo test --workspace` → green. (Existing tests: the default classify path now uses the adaptive deadline; closure must be unchanged — if any test asserts a closure and it changed, that's a soundness regression to investigate, but by construction it should be byte-identical.)

- [ ] **Step 6: fmt + clippy + commit**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
git add crates/owl-dl-reasoner/src/lib.rs crates/owl-dl-reasoner/src/classify.rs
git commit  # "feat(build-once): adaptive label-cache deadline (n×per_pair, clamped [1s,30s])" + trailers
```

---

### Task 2: Corpus FP=0 byte-identical gate + wine improvement + no-regression — controller-run

**Files:**
- Create: `docs/adaptive-label-cache-gate-results-2026-06-25.md`

- [ ] **Step 1: Corpus FP=0 byte-identical (soundness confirmation)**

Build release CLI. Run `konclude_closure_diff` across all oracled fixtures (the adaptive deadline is now the default — no env needed). Fast fixtures at 1s/pair; heavy (sio/ore-10908/wine) at 25ms/pair:
```bash
RUSTDL_TEST_PAIR_MS=1000 cargo test -p owl-dl-reasoner --test konclude_closure_diff -- --ignored --nocapture bibtex_ pizza_ ro_ sulo_ galen_ notgalen_ ore_15672
RUSTDL_TEST_PAIR_MS=25 cargo test -p owl-dl-reasoner --test konclude_closure_diff -- --ignored --nocapture sio_ ore_10908 wine_
```
Expected: every fixture **FP=0/MISSED=0 byte-identical** (the closure must NOT change — only test counts do). Any closure change is a stop (it would mean the label-build change altered soundness, which it must not).

- [ ] **Step 2: Wine net-wall improvement**

Wine classify wall, adaptive (default now) vs the pre-change fixed-1s baseline (`RUSTDL_LABEL_CACHE_TIMEOUT_MS=1000` to force old behaviour), same per-pair (200ms):
```bash
RUSTDL_LABEL_CACHE_TIMEOUT_MS=1000 /usr/bin/time -f "%es" ./target/release/rustdl classify --pair-timeout-ms 200 ontologies/real/wine.ofn >/dev/null   # baseline
/usr/bin/time -f "%es" ./target/release/rustdl classify --pair-timeout-ms 200 ontologies/real/wine.ofn >/dev/null                                      # adaptive default
```
Confirm adaptive ≤ baseline (expect ~13%, ~343→~300s) and read the banner `label heuristic: misses` dropping.

- [ ] **Step 3: No fast-fixture regression**

Classify wall on galen + ore-10908 + sio, baseline (`RUSTDL_LABEL_CACHE_TIMEOUT_MS=1000`) vs adaptive default. Confirm no material build-time increase (their label-`sat`s are ≪ floor, so the larger ceiling shouldn't bind).

- [ ] **Step 4: Verdict doc**

`docs/adaptive-label-cache-gate-results-2026-06-25.md`: corpus FP=0 byte-identical table; wine baseline-vs-adaptive wall + misses; fast-fixture no-regression table. GO iff closure byte-identical everywhere AND wine improves AND no regression.

- [ ] **Step 5: Commit verdict + merge to integration**

```bash
git add docs/adaptive-label-cache-gate-results-2026-06-25.md
git commit  # "docs(build-once): adaptive label-cache gate verdict" + trailers
```
On GO: merge `feat/adaptive-label-cache` → `feat/build-once-redesign` (`--no-ff`). If any closure changed or regressed: do NOT merge, record, diagnose.

---

## Self-Review

**1. Spec coverage:** adaptive formula `clamp(n×per_pair, 1s, 30s)` + env-override-wins + None→ceiling (Task 1) ✓; wire at build site (Task 1 Step 4) ✓; sound-by-construction → corpus FP=0 byte-identical gate (Task 2 Step 1) ✓; wine improvement (Task 2 Step 2) ✓; no fast-fixture regression (Task 2 Step 3) ✓; ships default + env override retained (Global Constraints) ✓; unit test of the pure function (Task 1) ✓.

**2. Placeholder scan:** all code is concrete (the formula, the consts, the wiring). The unit-test values are computed (137×200=27400, 137×1000→30000 ceiling, 2×200=400→1000 floor). No "TBD".

**3. Type consistency:** `adaptive_label_cache_ms(n: usize, per_pair: Option<Duration>, env_override: Option<u64>) -> u64`, `label_cache_env_override() -> Option<u64>`, consts `LABEL_CACHE_FLOOR_MS`/`LABEL_CACHE_CEILING_MS` — consistent across Tasks 1–2. `n` + `per_pair_timeout` confirmed in scope at `classify_top_down_internal`'s build site.
