# Phase 1 — Selective trust-sat verification Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When the hyper wedge returns `NotSubsumed` in less than a threshold (env-controlled, default 50 ms), distrust it and tableau-verify — recovering the ~109 GALEN / ~27 notgalen MISSED that the handoff and dead-end #12 trace to "wedge skipped a check the tableau could answer," while holding FP=0 across the Phase 0 net.

**Architecture:** Single chokepoint change in `crates/owl-dl-reasoner/src/classify.rs::subsumes_via_tableau`. Add an env-controlled threshold function in `crates/owl-dl-reasoner/src/lib.rs` next to the existing `hyper_*_enabled()` family. Add two stats counters in `ClassificationStats` to make the new code path visible. The "candidate filtering" hard constraint (dead-end #3: unfiltered defined-sup sweep at 1.9 M pair-tests blew up at 8000 CPU-min) is satisfied by construction — fast-`NotSubsumed` pairs are a small fraction of total pairs, so the filter IS the threshold. Measure: empirical sweep on GALEN + notgalen + Phase 0 net.

**Tech Stack:** Rust (edition 2024), the existing `owl-dl-reasoner` crate, the closure-diff harness from Phase 0.

---

## Background the executor needs

- The hyper wedge is consulted in one place: `subsumes_via_tableau` at `crates/owl-dl-reasoner/src/classify.rs:1063`. The `NotSubsumed` arm at `:1095-1100`:
  ```rust
  crate::HyperVerdict::NotSubsumed
      if trust_sat && crate::hyper_trust_sat_enabled() =>
  {
      stats.hyper_refuted_pairs += 1;
      return Ok(Some(false));
  }
  ```
  This is the gate to extend. The two call sites are `find_direct_parents_top_down` (`:1023`) and the defined-sup sweep (`:904`), both passing `trust_sat=true`.
- The per-call `trust_sat` parameter on `subsumes_via_tableau` exists since commit `b8d8695` — read its message; it explains the rationale and why the defined-sup sweep currently passes `true` (the unfiltered `false`-sweep was killed after 8000 CPU-min — dead-end #3).
- The three env-controlled `hyper_*_enabled()` functions are at `crates/owl-dl-reasoner/src/lib.rs:622-651`. The new threshold function lives next to them.
- `ClassificationStats` (`crates/owl-dl-reasoner/src/lib.rs`, search the symbol) carries the existing counters (`hyper_proven_pairs`, `hyper_refuted_pairs`, `tableau_subsumption_calls`, `timed_out_pairs`). New counters are added there; the per-pair `local_stats` aggregator at `classify.rs:919-923` must add them to the global accumulator.
- The Phase 0 net is `scripts/run-soundness-diff.sh` (runs every `#[ignore]` test in `crates/owl-dl-reasoner/tests/konclude_closure_diff.rs`). Phase 1's measurable success is "GALEN MISSED 109 → ≤ 40 AND 0 FP across the net AND wall +1–3 min, not +hours."
- The handoff at `docs/handoff-2026-05-30.md` documents `ErosionOfStomach ⊑ GastricPathology` as a known-tableau-resolves-in-0.994s pair where the wedge fast-NotSubsumes — an excellent integration-test target for the empirical sweep.
- `RUSTFLAGS: -D warnings` is set in CI (also clippy pedantic on workspace-wide). Any new code must clear `cargo clippy --workspace --all-targets --all-features -- -D warnings`.

---

## Task 1: Threshold env-var reader

**Files:**
- Modify: `crates/owl-dl-reasoner/src/lib.rs` (add new function near `hyper_trust_sat_enabled` at `:649-651`; add a unit test in the existing `mod tests` if one exists in that file, otherwise put the test directly above the `#[cfg(test)]` block)

- [ ] **Step 1: Read existing test surface in lib.rs**

Run: `grep -n "^#\[cfg(test)\]\|fn hyper_trust_sat_enabled\|mod tests\b" crates/owl-dl-reasoner/src/lib.rs`
Expected output identifies (a) the existing `hyper_trust_sat_enabled` function location, (b) whether there is an existing `mod tests`, (c) any `#[cfg(test)]` block.

- [ ] **Step 2: Write the failing test**

Open `crates/owl-dl-reasoner/src/lib.rs`. Find the existing `mod tests` (or end of file if none). Add this test (if no `mod tests` exists at module level in lib.rs, add a new one at the bottom of the file just before any closing `}` of an outer module):

```rust
#[cfg(test)]
mod hyper_trust_sat_min_ms_tests {
    use super::hyper_trust_sat_min_ms;
    // Note: env-var-driven tests run sequentially under one test process
    // by default in `cargo test`, but they share the global env. We
    // mutate only this one variable and restore it; if multiple env-var
    // tests are added later, switch to a serial_test crate guard.

    fn with_env<F: FnOnce()>(key: &str, val: Option<&str>, f: F) {
        let prev = std::env::var_os(key);
        match val {
            Some(v) => unsafe { std::env::set_var(key, v) },
            None => unsafe { std::env::remove_var(key) },
        }
        f();
        match prev {
            Some(v) => unsafe { std::env::set_var(key, v) },
            None => unsafe { std::env::remove_var(key) },
        }
    }

    #[test]
    fn default_is_50ms() {
        with_env("RUSTDL_HYPER_TRUST_SAT_MIN_MS", None, || {
            assert_eq!(hyper_trust_sat_min_ms(), 50);
        });
    }

    #[test]
    fn env_overrides_value() {
        with_env("RUSTDL_HYPER_TRUST_SAT_MIN_MS", Some("200"), || {
            assert_eq!(hyper_trust_sat_min_ms(), 200);
        });
    }

    #[test]
    fn zero_disables_selective_verification() {
        with_env("RUSTDL_HYPER_TRUST_SAT_MIN_MS", Some("0"), || {
            assert_eq!(hyper_trust_sat_min_ms(), 0);
        });
    }

    #[test]
    fn empty_string_uses_default() {
        with_env("RUSTDL_HYPER_TRUST_SAT_MIN_MS", Some(""), || {
            assert_eq!(hyper_trust_sat_min_ms(), 50);
        });
    }

    #[test]
    fn garbage_uses_default() {
        with_env("RUSTDL_HYPER_TRUST_SAT_MIN_MS", Some("not-a-number"), || {
            assert_eq!(hyper_trust_sat_min_ms(), 50);
        });
    }
}
```

- [ ] **Step 3: Run to verify failure (compile error: function not defined)**

Run: `cargo test -p owl-dl-reasoner --lib hyper_trust_sat_min_ms -- --test-threads=1 2>&1 | tail -20`
Expected: compile error — `cannot find function 'hyper_trust_sat_min_ms' in this scope` (or similar).

- [ ] **Step 4: Implement the function**

Add this immediately after `pub fn hyper_trust_sat_enabled()` in `crates/owl-dl-reasoner/src/lib.rs` (after `:651`):

```rust
/// Minimum wedge wall-time threshold (in milliseconds) below which a
/// `NotSubsumed` verdict is **distrusted** and the tableau is asked to
/// verify. A wedge `NotSubsumed` returned in < threshold ms is more
/// likely "didn't try hard enough" than a genuine satisfying model.
///
/// **Default: 50 ms.** Setting to `0` disables selective verification
/// (restores pre-Phase-1 behaviour: trust every `NotSubsumed` verdict
/// when [`hyper_trust_sat_enabled`] is on). Empty / garbage values
/// also fall back to the default.
///
/// Rationale: GALEN's 109 MISSED and notgalen's 27 (see
/// `docs/handoff-2026-05-30.md`) are mostly cases where the wedge
/// returned `NotSubsumed` in single-digit milliseconds and the tableau,
/// asked directly via `rustdl explain`, finds the entailment in under a
/// second. The dead-end #3 unfiltered "always tableau-verify" sweep
/// (`docs/hypertableau-dead-ends.md` §3) was killed at 8000 CPU-min;
/// the threshold is the filter that makes the verification tractable
/// (fast-`NotSubsumed` pairs are a small fraction of all pairs).
#[must_use]
pub fn hyper_trust_sat_min_ms() -> u64 {
    std::env::var("RUSTDL_HYPER_TRUST_SAT_MIN_MS")
        .ok()
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(50)
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p owl-dl-reasoner --lib hyper_trust_sat_min_ms -- --test-threads=1 2>&1 | tail -20`
Expected: 5 tests pass.

- [ ] **Step 6: Clippy clean check**

Run: `cargo clippy -p owl-dl-reasoner --lib -- -D warnings 2>&1 | tail -10`
Expected: no warnings.

- [ ] **Step 7: Commit**

```bash
git add crates/owl-dl-reasoner/src/lib.rs
git commit -m "feat(reasoner): RUSTDL_HYPER_TRUST_SAT_MIN_MS threshold (default 50ms)"
```

---

## Task 2: New stats fields for the selective-verify path

**Files:**
- Modify: `crates/owl-dl-reasoner/src/classify.rs` (the `ClassificationStats` struct and its `Default` impl if explicit, plus the per-pair merger at `:919-923`)

Background: we need two new counters to make the new code path visible in the diff harness output and CI regression checks:
- `hyper_refuted_fast_pairs` — count of wedge `NotSubsumed` verdicts that fell through to tableau because the wedge was fast (the new path; previously these would have returned `Some(false)` directly).
- `hyper_refuted_fast_flipped_pairs` — count of cases where the new path's tableau actually returned `Subsumed` (the recovered MISSED). This is the explicit completeness-gain counter.

- [ ] **Step 1: Locate `ClassificationStats`**

Run: `grep -nE "pub struct ClassificationStats|impl Default for ClassificationStats|hyper_refuted_pairs" crates/owl-dl-reasoner/src/classify.rs crates/owl-dl-reasoner/src/lib.rs | head -10`
Expected: identifies the file + line where the struct is defined and where existing counters live (use this to keep the new fields in the same shape as the existing ones — derived `Default`, public, `u64`).

- [ ] **Step 2: Write the failing unit test**

In `crates/owl-dl-reasoner/src/classify.rs`'s `mod tests` (the one starting at `:1138`), add:

```rust
#[test]
fn stats_carry_selective_verify_counters_by_default() {
    let s = ClassificationStats::default();
    assert_eq!(s.hyper_refuted_fast_pairs, 0);
    assert_eq!(s.hyper_refuted_fast_flipped_pairs, 0);
}
```

- [ ] **Step 3: Run — fail with missing fields**

Run: `cargo test -p owl-dl-reasoner --lib stats_carry_selective_verify_counters_by_default 2>&1 | tail -10`
Expected: compile error — `no field 'hyper_refuted_fast_pairs' on type 'ClassificationStats'`.

- [ ] **Step 4: Add the fields**

In the `ClassificationStats` struct (from Step 1's grep result), add these two fields, placed immediately after `hyper_refuted_pairs` so semantically-related counters cluster:

```rust
/// Wedge returned `NotSubsumed` in < `hyper_trust_sat_min_ms()` and
/// the verdict was therefore distrusted: the tableau was asked
/// instead. Counts each fall-through, regardless of the tableau's
/// answer. Zero when [`hyper_trust_sat_min_ms`] returns 0.
pub hyper_refuted_fast_pairs: u64,
/// Subset of `hyper_refuted_fast_pairs` where the tableau actually
/// returned `Subsumed` — the entailment the wedge would have dropped
/// as MISSED but the slow path recovered. Directly tracks Phase 1's
/// completeness lever.
pub hyper_refuted_fast_flipped_pairs: u64,
```

If the struct uses `#[derive(Default)]`, no changes are needed there — the `u64` defaults to 0. If it has a manual `impl Default`, add `hyper_refuted_fast_pairs: 0` and `hyper_refuted_fast_flipped_pairs: 0`.

- [ ] **Step 5: Run — test passes**

Run: `cargo test -p owl-dl-reasoner --lib stats_carry_selective_verify_counters_by_default 2>&1 | tail -10`
Expected: 1 test passes.

- [ ] **Step 6: Wire into the per-pair merge in the defined-sup sweep**

Open `crates/owl-dl-reasoner/src/classify.rs`. Find the per-pair stats merger that currently reads (around `:919-923`):

```rust
stats.saturation_subsumption_hits += sd.saturation_subsumption_hits;
stats.tableau_subsumption_calls += sd.tableau_subsumption_calls;
stats.timed_out_pairs += sd.timed_out_pairs;
stats.hyper_proven_pairs += sd.hyper_proven_pairs;
stats.hyper_refuted_pairs += sd.hyper_refuted_pairs;
```

Add two lines:

```rust
stats.hyper_refuted_fast_pairs += sd.hyper_refuted_fast_pairs;
stats.hyper_refuted_fast_flipped_pairs += sd.hyper_refuted_fast_flipped_pairs;
```

- [ ] **Step 7: Confirm compile + clippy clean**

Run: `cargo test -p owl-dl-reasoner --lib --no-run 2>&1 | tail -5 && cargo clippy -p owl-dl-reasoner -- -D warnings 2>&1 | tail -5`
Expected: both clean.

- [ ] **Step 8: Commit**

```bash
git add crates/owl-dl-reasoner/src/classify.rs
git commit -m "feat(reasoner): add hyper_refuted_fast{,_flipped}_pairs to ClassificationStats"
```

---

## Task 3: Wire the selective-verify policy into `subsumes_via_tableau`

**Files:**
- Modify: `crates/owl-dl-reasoner/src/classify.rs::subsumes_via_tableau` (`:1063-1136`)

This is the load-bearing code change. The plan: time the `hyper_decide` call; if the verdict is `NotSubsumed` AND `trust_sat` is on globally AND the wedge wall time is below the threshold, **fall through to the tableau instead of trusting the verdict**; if the tableau then says `Subsumed`, bump `hyper_refuted_fast_flipped_pairs`.

- [ ] **Step 1: Write the failing integration test**

In `crates/owl-dl-reasoner/src/classify.rs`'s `mod tests` add:

```rust
/// With `RUSTDL_HYPER_TRUST_SAT_MIN_MS=100000` (~100 s, far above any
/// realistic wedge call), every wedge `NotSubsumed` should be
/// distrusted and the tableau should be asked. We exercise this via
/// stats: `hyper_refuted_fast_pairs > 0` proves the new code path
/// was taken on at least one pair.
///
/// The synthetic ontology is a non-EL ALC sat probe that the wedge
/// returns NotSubsumed on quickly (its non-Horn fragment escapes the
/// fast Horn fixpoint).
#[test]
fn selective_verify_triggers_when_threshold_high() {
    // SAFETY: tests in the same process share env; we restore on exit.
    let key = "RUSTDL_HYPER_TRUST_SAT_MIN_MS";
    let prev = std::env::var_os(key);
    unsafe { std::env::set_var(key, "100000") };

    let onto = parse(&format!(
        "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(Class(:C))\n\
    Declaration(ObjectProperty(:r))\n\
    Declaration(ObjectProperty(:s))\n\
    DisjointObjectProperties(:r :s)\n\
    SubClassOf(:A :B)\n\
    SubClassOf(:B :C)\n\
)\n"
    ));
    let h = classify(&onto).expect("classify");
    let stats = h.stats();

    match prev {
        Some(v) => unsafe { std::env::set_var(key, v) },
        None => unsafe { std::env::remove_var(key) },
    }

    // Above the threshold => every NotSubsumed pair falls through.
    // The ontology has 3 classes and at least one pair where the
    // wedge would return NotSubsumed (e.g. C ⊑ A, which is false).
    // `hyper_refuted_fast_pairs > 0` proves the new code path fired.
    assert!(
        stats.hyper_refuted_fast_pairs > 0,
        "selective verify path never fired; stats = {stats:?}"
    );
    // Soundness: the existing entailments must still hold.
    let iri = |s: &str| format!("http://rustdl.test/{s}");
    assert!(h.is_subclass(&iri("A"), &iri("B")));
    assert!(h.is_subclass(&iri("A"), &iri("C")));
    assert!(h.is_subclass(&iri("B"), &iri("C")));
    // And spurious entailments must still NOT appear.
    assert!(!h.is_subclass(&iri("C"), &iri("A")));
}

/// Symmetric check: with `RUSTDL_HYPER_TRUST_SAT_MIN_MS=0`
/// (explicit opt-out), the selective-verify path is disabled and
/// `hyper_refuted_fast_pairs` stays at zero.
#[test]
fn selective_verify_disabled_when_threshold_zero() {
    let key = "RUSTDL_HYPER_TRUST_SAT_MIN_MS";
    let prev = std::env::var_os(key);
    unsafe { std::env::set_var(key, "0") };

    let onto = parse(&format!(
        "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(Class(:C))\n\
    Declaration(ObjectProperty(:r))\n\
    Declaration(ObjectProperty(:s))\n\
    DisjointObjectProperties(:r :s)\n\
    SubClassOf(:A :B)\n\
    SubClassOf(:B :C)\n\
)\n"
    ));
    let h = classify(&onto).expect("classify");
    let stats = h.stats();

    match prev {
        Some(v) => unsafe { std::env::set_var(key, v) },
        None => unsafe { std::env::remove_var(key) },
    }

    assert_eq!(
        stats.hyper_refuted_fast_pairs, 0,
        "selective verify fired despite threshold=0; stats = {stats:?}"
    );
}
```

(If `parse` and `HEADER` aren't in scope at this point in the test module, copy the existing usage pattern from `classify_drops_to_tableau_when_axioms_leave_el` which is the closest existing test exercising a non-EL ontology — at `:1276-1300`.)

- [ ] **Step 2: Run — expect failure (the new path doesn't exist yet)**

Run: `cargo test -p owl-dl-reasoner --lib selective_verify_triggers_when_threshold_high 2>&1 | tail -15`
Expected: test FAILS with `selective verify path never fired; stats = ...` because the current code returns from the `NotSubsumed` arm without bumping the new counter.

- [ ] **Step 3: Implement the policy**

In `crates/owl-dl-reasoner/src/classify.rs`, replace the existing wedge-consult block (currently `:1089-1102`):

```rust
let hyper_deadline = per_pair_timeout.map(|t| Instant::now() + t);
match prepared.hyper_decide(sub, sup, hyper_deadline) {
    crate::HyperVerdict::Subsumed => {
        stats.hyper_proven_pairs += 1;
        return Ok(Some(true));
    }
    crate::HyperVerdict::NotSubsumed
        if trust_sat && crate::hyper_trust_sat_enabled() =>
    {
        stats.hyper_refuted_pairs += 1;
        return Ok(Some(false));
    }
    _ => {}
}
```

with this:

```rust
let hyper_deadline = per_pair_timeout.map(|t| Instant::now() + t);
let wedge_start = Instant::now();
let verdict = prepared.hyper_decide(sub, sup, hyper_deadline);
let wedge_elapsed_ms = u64::try_from(wedge_start.elapsed().as_millis()).unwrap_or(u64::MAX);
match verdict {
    crate::HyperVerdict::Subsumed => {
        stats.hyper_proven_pairs += 1;
        return Ok(Some(true));
    }
    crate::HyperVerdict::NotSubsumed if trust_sat && crate::hyper_trust_sat_enabled() => {
        // Phase 1 selective verification: a wedge `NotSubsumed`
        // returned in < `RUSTDL_HYPER_TRUST_SAT_MIN_MS` is more
        // likely "didn't try hard enough" than a genuine satisfying
        // model. Fall through to the tableau in that case; trust
        // the verdict only when the wedge took at least the
        // threshold. Setting the env var to 0 restores pre-Phase-1
        // behaviour (trust every NotSubsumed verdict).
        let threshold = crate::hyper_trust_sat_min_ms();
        if threshold == 0 || wedge_elapsed_ms >= threshold {
            stats.hyper_refuted_pairs += 1;
            return Ok(Some(false));
        }
        stats.hyper_refuted_fast_pairs += 1;
        // Fall through to the tableau probe below; if it flips to
        // Subsumed, bump `hyper_refuted_fast_flipped_pairs`.
    }
    _ => {}
}
```

Then, in the tableau-probe block immediately below (`:1103-1135`), bump the flipped-counter on a `true` result. Replace the existing match-on-`per_pair_timeout` with:

```rust
let build = move |pool: &mut ConceptPool| {
    let sub_concept = pool.atomic(sub);
    let super_concept = pool.atomic(sup);
    let not_super = pool.not(super_concept);
    pool.and(vec![sub_concept, not_super])
};
let was_fast_refuted = matches!(
    verdict,
    crate::HyperVerdict::NotSubsumed,
) && trust_sat
    && crate::hyper_trust_sat_enabled()
    && {
        let threshold = crate::hyper_trust_sat_min_ms();
        threshold != 0 && wedge_elapsed_ms < threshold
    };
match per_pair_timeout {
    None => {
        let sat = prepared.decide(build)?;
        stats.tableau_subsumption_calls += 1;
        let subsumed = !sat;
        if was_fast_refuted && subsumed {
            stats.hyper_refuted_fast_flipped_pairs += 1;
        }
        Ok(Some(subsumed))
    }
    Some(timeout) => {
        let deadline = Instant::now() + timeout;
        match prepared.decide_with_deadline(deadline, build) {
            Ok(Some(sat)) => {
                stats.tableau_subsumption_calls += 1;
                let subsumed = !sat;
                if was_fast_refuted && subsumed {
                    stats.hyper_refuted_fast_flipped_pairs += 1;
                }
                Ok(Some(subsumed))
            }
            Ok(None) | Err(crate::ReasonError::NoVerdict) => {
                stats.timed_out_pairs += 1;
                Ok(None)
            }
            Err(other) => Err(other),
        }
    }
}
```

Note: `was_fast_refuted` is computed before the tableau call so the increment site has the information it needs. Yes, `matches!(verdict, ...)` lets us re-inspect `verdict` after the wedge-consult `match` already moved past it — `verdict` is `Copy` if `HyperVerdict` derives `Copy` (it does — see `lib.rs:656`). Confirm with: `grep -nE "#\[derive.*Copy.*\] *$|^pub.* enum HyperVerdict" crates/owl-dl-reasoner/src/lib.rs | head -3`.

- [ ] **Step 4: Run the two new tests — expect pass**

Run: `cargo test -p owl-dl-reasoner --lib selective_verify -- --test-threads=1 2>&1 | tail -15`
Expected: both tests pass.

- [ ] **Step 5: Run the full lib tests — soundness regression check**

Run: `cargo test -p owl-dl-reasoner --lib 2>&1 | tail -10`
Expected: all lib tests pass. The existing `classify_*` tests must still pass — they assert specific verdicts that the new policy must not change.

- [ ] **Step 6: Clippy + fmt clean (only on changes; don't fix pre-existing debt)**

```bash
cargo clippy -p owl-dl-reasoner -- -D warnings 2>&1 | tail -10
cargo fmt -p owl-dl-reasoner -- --check 2>&1 | head -20
```
Expected: clippy clean. If `cargo fmt -- --check` reports diffs, run `cargo fmt -p owl-dl-reasoner -- crates/owl-dl-reasoner/src/classify.rs crates/owl-dl-reasoner/src/lib.rs` to fix ONLY the files this task touched (do not run `cargo fmt --all`, which would touch pre-existing fmt debt outside Phase 1 scope — Phase 0's final review explicitly left that debt to its owners).

- [ ] **Step 7: Commit**

```bash
git add crates/owl-dl-reasoner/src/classify.rs
git commit -m "feat(classify): selective trust-sat verification (fast NotSubsumed -> tableau)

Wedge NotSubsumed verdicts returned in < RUSTDL_HYPER_TRUST_SAT_MIN_MS
(default 50ms) are now distrusted and tableau-verified. The fast-refuted
fall-through is counted in hyper_refuted_fast_pairs; tableau-Subsumed
flips are counted in hyper_refuted_fast_flipped_pairs. Set the env var
to 0 to restore pre-Phase-1 behaviour."
```

---

## Task 4: Soundness gate — re-run Phase 0 net

**Files:**
- No new files. Uses `scripts/run-soundness-diff.sh` from Phase 0 Task 7.

Fixtures required on disk: the full set wired in `crates/owl-dl-reasoner/tests/konclude_closure_diff.rs` (alehif-test, ore-10908-sroiq, ore-15672-shoin — galen and notgalen tests will SKIP cleanly if missing; we handle them explicitly in Task 5). The Phase 0 fixtures are NOT gitignored from this branch's perspective; they're on disk under `ontologies/external/`.

- [ ] **Step 1: Run the closure-diff suite with selective verification ON (default)**

Run: `scripts/run-soundness-diff.sh 2>&1 | tee /tmp/phase1-soundness.log`
Expected per-fixture lines of shape `--- <slug> (...) --- rustdl_closure=... konclude_closure=... FP=... MISSED=... (unsat: ... thing-equiv: ...)`. **Every `FP` value MUST be 0.** A FP > 0 is a real soundness regression — STOP, do not commit any Phase 1 follow-on, file the failing pair with its layer (run `rustdl classify --saturation-only` on the failing input — if the FP persists, the bug is in saturation, not the new wedge policy).

- [ ] **Step 2: Compare against Phase 0 baseline**

Phase 0 results (from `docs/phase0-soundness-results.md`): SHOIN FP=0 MISSED=0; SROIQ FP=0 MISSED=0. The Phase 1 run should show the same FP=0 across the board; MISSED **may decrease** (recovered by the new path) on any ontology where it was non-zero pre-Phase-1, and **must not increase** on any ontology.

If MISSED *increased* on any ontology: that's the dead-end #7 pattern (sound rule → search blowup → budget → Stalled → MISSED). Confirm with the `timed_out_pairs` counter (visible in the stats but not in the per-fixture line; re-run with `--features ...` or stats-printing if needed). Recommendation if this happens: bump the per-pair timeout for that fixture, OR raise `RUSTDL_HYPER_TRUST_SAT_MIN_MS` to be more conservative (e.g. 100ms). Document the finding.

- [ ] **Step 3: Capture the harness output for the results doc**

Save `/tmp/phase1-soundness.log` — Task 6 quotes per-fixture lines from it.

No commit yet — Task 5 + Task 6 build the empirical evidence; the results land together in Task 6.

---

## Task 5: Empirical sweep on GALEN + notgalen (the lever's payoff)

**Files:**
- No new files. Uses the existing harness tests `galen_closure_matches_konclude` (`konclude_closure_diff.rs:284`) and `notgalen_closure_matches_konclude` (`:310`).

Fixtures required: `ontologies/external/galen.ofn` + `galen-classified.owx` + `notgalen.ofn` + `notgalen-classified.owx`. Check first:
```bash
ls -lh ontologies/external/galen* ontologies/external/notgalen* 2>&1 | head -10
```
If missing, the tests skip — record the skip and the absent files, do not attempt to fetch them in this task.

- [ ] **Step 1: Baseline (`RUSTDL_HYPER_TRUST_SAT_MIN_MS=0`) — pre-Phase-1 behaviour**

Run:
```bash
RUSTDL_HYPER_TRUST_SAT_MIN_MS=0 cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    galen_closure_matches_konclude notgalen_closure_matches_konclude \
    -- --ignored --nocapture 2>&1 | tee /tmp/phase1-galen-baseline.log
```
Expected wall: GALEN ~2–12 min, notgalen ~10 min, per the existing `#[ignore]` reasons. **If either exceeds 20 min, kill it and record the timeout** — we still get a valid before/after if both runs use the same kill criterion.

Record: the `FP`, `MISSED`, and `(... s)` wall values from the harness line for each fixture.

- [ ] **Step 2: Phase 1 default (`RUSTDL_HYPER_TRUST_SAT_MIN_MS=50`) — the active policy**

Run (no env override; 50 is the default):
```bash
cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    galen_closure_matches_konclude notgalen_closure_matches_konclude \
    -- --ignored --nocapture 2>&1 | tee /tmp/phase1-galen-50ms.log
```
Same kill criterion: 20 min cap per test.

Record: `FP`, `MISSED`, wall — and, if the harness output includes stats (the existing format is the per-fixture line only, so stats aren't visible here; that's fine, we read them indirectly from the MISSED delta).

- [ ] **Step 3: Phase 1 conservative (`RUSTDL_HYPER_TRUST_SAT_MIN_MS=200`) — fallback threshold**

If Step 2's wall blew past the +1–3 min spec target, run Step 2 again with a higher threshold to see if a more conservative policy still recovers most MISSED at lower wall cost:
```bash
RUSTDL_HYPER_TRUST_SAT_MIN_MS=200 cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    galen_closure_matches_konclude notgalen_closure_matches_konclude \
    -- --ignored --nocapture 2>&1 | tee /tmp/phase1-galen-200ms.log
```

If Step 2 met the spec target, this step is OPTIONAL — note as "default 50ms met target; conservative threshold sweep not needed."

- [ ] **Step 4: Triage**

For each fixture, compare:
- **Baseline (0) → Phase 1 default (50):** MISSED should drop substantially. The spec target is GALEN 109 → ≤ 40 (and notgalen 27 → comparable, e.g. ≤ 10). Wall should grow by 1–3 min, not by an order of magnitude.
- If MISSED decreased AND `FP=0` is held (re-verify FP from the harness line): success.
- If wall blew up: report what 200 ms gave (Step 3); pick the threshold that achieves the best MISSED-reduction-per-extra-second; document the choice.
- If FP > 0 on EITHER run: STOP, file. This is the soundness regression case (extremely unlikely given Phase 0 net pre-checked it, but the new path could theoretically expose a latent tableau bug under the larger flow).

Save the three harness lines (or two, if Step 3 was skipped) per fixture for Task 6.

---

## Task 6: Results doc + threshold default lock-in

**Files:**
- Create: `docs/phase1-results.md`
- Modify: `docs/fragment-completeness.md` (one-line update: the "Validated corpus envelope" section should note that Phase 1 was run with selective verification on and FP=0 still held — this keeps the doc accurate per its own discipline).
- Modify: `CLAUDE.md` (Soundness contract section: add a note that `trust_sat`-default-on now means "trust only when the wedge took ≥ threshold ms" with one-line pointer to the env var).

- [ ] **Step 1: Write `docs/phase1-results.md`**

Create the file with this structure, filling values from the Task 4 and Task 5 logs:

```markdown
# Phase 1 — Selective trust-sat verification results

Run on 2026-05-31 against the Phase 0 soundness net (Task 4) plus the
GALEN and notgalen sweeps (Task 5). Threshold mechanism:
`RUSTDL_HYPER_TRUST_SAT_MIN_MS` (default 50 ms; 0 restores pre-Phase-1
behaviour). See `docs/superpowers/plans/2026-05-31-phase1-selective-trust-sat.md`.

## Soundness net (Phase 0 fixtures, default threshold)

<table per fixture from /tmp/phase1-soundness.log — exact columns:
Fixture | FP | MISSED | Wall | Outcome. Outcome = PASS if FP=0,
filed FP if not.>

**Soundness gate:** <one sentence — FP=0 held / regression filed>.

## Completeness lever (GALEN, notgalen)

| Fixture | Threshold | FP | MISSED | Wall | Delta vs baseline |
|---|---|---|---|---|---|
| galen | 0 (baseline) | <fp> | <missed> | <wall> | — |
| galen | 50 (default) | <fp> | <missed> | <wall> | MISSED -X (-Y%) , wall +Zs |
| galen | 200 (conservative, if run) | ... | ... | ... | ... |
| notgalen | 0 (baseline) | ... | ... | ... | — |
| notgalen | 50 (default) | ... | ... | ... | ... |
| notgalen | 200 (conservative, if run) | ... | ... | ... | ... |

**Interpretation:** <one paragraph — did the spec target (GALEN 109 → ≤40
at +1–3 min wall) land? If yes: state which threshold gave the best
MISSED-per-second ratio. If no: explain what threshold the data
recommends and why.>

## Threshold default

<one sentence: based on the data, the default of 50 ms is kept / raised
to N ms / lowered to M ms. If changed from 50, note the code change is
in `crates/owl-dl-reasoner/src/lib.rs::hyper_trust_sat_min_ms`.>

## Cross-cutting confirmation

- 0 FP held across the Phase 0 net under the default threshold ✓ / ✗
- `hyper_refuted_fast_pairs > 0` on at least one fixture (proves the new code path fired) ✓ / ✗
- `hyper_refuted_fast_flipped_pairs > 0` on GALEN or notgalen (proves real MISSED were recovered) ✓ / ✗

## How to re-run

```bash
# Soundness net (default threshold):
scripts/run-soundness-diff.sh

# Lever sweep (GALEN + notgalen):
cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    galen_closure_matches_konclude notgalen_closure_matches_konclude \
    -- --ignored --nocapture
```
```

- [ ] **Step 2: Update `docs/fragment-completeness.md`**

Find the "Validated corpus envelope" section. Append at the END of that section (after the existing prose, before the next `##` heading):

```markdown
Phase 1 (selective trust-sat verification, default threshold 50 ms) re-ran
the above corpus on 2026-05-31; FP=0 held across every fixture (see
[`phase1-results.md`](phase1-results.md)). The validated envelope is
unchanged; the new code path narrows when `Sat` verdicts are trusted (only
when the wedge took ≥ threshold ms) but does not widen what counts as
sound.
```

(If FP>0 occurred and Phase 1 was reverted before this task ran, do NOT make this edit — the user will not have reached Task 6 in that case.)

- [ ] **Step 3: Update `CLAUDE.md` Soundness contract section**

Open `CLAUDE.md`. Find the bullet starting `With trust_sat on, the wedge concludes "not subsumed" from its own Sat verdict without consulting the tableau.` Append the following at the end of that bullet:

```
  Phase 1 narrows this: `trust_sat` now trusts a `NotSubsumed` verdict
  only when the wedge took at least `RUSTDL_HYPER_TRUST_SAT_MIN_MS` ms
  (default 50; 0 disables, restoring the pre-Phase-1 always-trust
  behaviour). See `docs/phase1-results.md` for the empirical sweep.
```

- [ ] **Step 4: Commit**

```bash
git add docs/phase1-results.md docs/fragment-completeness.md CLAUDE.md
git commit -m "docs(phase1): results doc + envelope/CLAUDE.md updates for selective trust-sat"
```

---

## Definition of done (Phase 1)

- `RUSTDL_HYPER_TRUST_SAT_MIN_MS` env var honoured by `hyper_trust_sat_min_ms()` with default 50 (Task 1).
- `ClassificationStats` carries `hyper_refuted_fast_pairs` and `hyper_refuted_fast_flipped_pairs` (Task 2).
- `subsumes_via_tableau` distrusts fast wedge `NotSubsumed` verdicts and falls through to the tableau; flipped Subsumed verdicts increment the flipped counter (Task 3).
- Lib unit tests cover the threshold reader (5 tests) and the policy (2 tests) — all pass.
- Phase 0 net runs FP=0 under the default threshold (Task 4).
- GALEN + notgalen sweep shows MISSED dropping while FP stays 0 — empirical evidence the lever works, recorded in `docs/phase1-results.md` (Tasks 5 + 6).
- `CLAUDE.md` and `docs/fragment-completeness.md` describe the new behaviour accurately (Task 6).

This unblocks Phase 2 (deep completeness calculus). Phase 1 is the keystone — Phase 2 builds on its measured baseline, so the `phase1-results.md` numbers become the reference Phase 2 must not regress.
