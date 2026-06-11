# Anytime-under-deadline experiment Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Produce the paper's central evidence — that rustdl is a *sound anytime* OWL classifier with *calibrated incompleteness* — by measuring precision/recall/silent-miss vs deadline across hard-SROIQ fixtures, plus a Konclude all-or-nothing contrast.

**Architecture:** Phase 1 sweeps the existing per-pair timeout (one small read-only exposure of the undecided pair set + a sweep harness in the trusted closure-diff test). Phase 2 builds a global wall-clock deadline and re-runs. Metrics computed against the existing HermiT/Konclude oracle closures.

**Tech Stack:** Rust 2024, `owl-dl-reasoner` (`classify.rs`), the `konclude_closure_diff.rs` integration test (oracle loading + closure diff), native Konclude binary for the wall contrast.

Spec: `docs/superpowers/specs/2026-06-11-anytime-deadline-experiment-design.md`.

---

## File structure

- **Modify** `crates/owl-dl-reasoner/src/classify.rs`
  - Phase 1: add `timed_out_pair_ids: Vec<(u32, u32)>` to `ClassificationStats`; populate at the two timeout sites; store on `Classification`; add `pub fn undecided_pairs(&self) -> Vec<(&str, &str)>`.
  - Phase 2: add a global wall-clock deadline path (`classify_top_down_with_global_deadline`) threading one shared absolute `Instant` to the probe sites + a tier-walk short-circuit.
- **Modify** `crates/owl-dl-reasoner/tests/konclude_closure_diff.rs`
  - Add the `#[ignore]`d sweep harness (Phase 1 per-pair; Phase 2 global) reusing the file's oracle-load + rustdl-closure-extract helpers.
- **Create** `docs/anytime-results-2026-06-11.csv` and `docs/anytime-results-2026-06-11.md` (the committed evidence artifacts).

---

# PHASE 1 — per-pair sweep (ships independently)

### Task 1: Expose the undecided (timed-out) pair set

**Files:**
- Modify: `crates/owl-dl-reasoner/src/classify.rs`

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` in `crates/owl-dl-reasoner/src/classify.rs` (mirror the imports an existing test in that module uses for OFN parsing + `classify_with_timeout`):

```rust
#[test]
fn undecided_pairs_reports_timed_out_subsumptions() {
    // A pathological SROIQ ontology with a tiny per-pair budget should
    // time out at least one pair, and that pair must appear in
    // undecided_pairs() (the calibration signal). We assert the API
    // exists and is consistent with the timed_out_pairs count.
    use std::time::Duration;
    // pizza is the canonical pathological fixture; use a tiny inline
    // ontology that forces a hard pair instead, to keep the unit test
    // self-contained and fast:
    let src = "Prefix(:=<http://t/>)\n\
Ontology(\n\
  Declaration(Class(:A)) Declaration(Class(:B)) Declaration(ObjectProperty(:r))\n\
  SubClassOf(:A ObjectAllValuesFrom(:r :B))\n\
  SubClassOf(:A ObjectSomeValuesFrom(:r owl:Thing))\n)\n";
    let onto = parse_ofn(src); // helper already in this test module; if named
                               // differently, use the module's parse helper.
    let h = classify_with_timeout(&onto, Duration::from_millis(1)).expect("classify");
    // The API exists and its length matches the count stat.
    assert_eq!(h.undecided_pairs().len(), h.stats().timed_out_pairs);
}
```

(If the test module lacks a `parse_ofn` helper, copy the 3-line OFN-parse idiom from a neighboring test in the same module. The assertion that matters is `undecided_pairs().len() == stats().timed_out_pairs` — it pins the set/count consistency regardless of whether this tiny ontology actually times out.)

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p owl-dl-reasoner --lib undecided_pairs_reports_timed_out_subsumptions 2>&1 | tail -15`
Expected: FAIL to compile — `undecided_pairs` and `timed_out_pair_ids` don't exist.

- [ ] **Step 3: Add the field to `ClassificationStats`**

In the `ClassificationStats` struct (the `pub timed_out_pairs: usize` field is around line 162), add after it:

```rust
    /// The `(sub, sup)` class-index pairs whose subsumption probe timed
    /// out (defaulted to "not subsumed"). Parallel to `timed_out_pairs`
    /// (the count) — this is the *set*, used to verify calibration
    /// (every miss is a flagged-undecided pair). Populated at the same
    /// sites that bump `timed_out_pairs`.
    pub timed_out_pair_ids: Vec<(u32, u32)>,
```

- [ ] **Step 4: Populate it at the two timeout sites**

Site A — `subsumes_via_tableau` (the top-down path). Find where it handles the `decide_with_deadline` timeout and bumps `timed_out_pairs` (the `Ok(None)` / `NoVerdict` branch, ~line 1793-1800; it returns `Ok(None)`). At that branch, before returning `Ok(None)`, add:

```rust
            stats.timed_out_pair_ids.push((sub.index(), sup.index()));
```

(The function has `sub: ClassId`, `sup: ClassId`, `stats: &mut ClassificationStats` in scope — `ClassId::index()` returns the `u32`/usize index; cast to `u32` if needed: `u32::try_from(sub.index()).unwrap()`. Match the existing index-cast idiom in the file, e.g. `as u32` is avoided — use `u32::try_from(...).expect(...)` like the rest of classify.rs.)

Site B — the n² path. At `classify_internal_with_timeout`, the pair loop produces `(i, j, is_entailed, used_saturation, timed_out)` and bumps `stats.timed_out_pairs += 1` when `timed_out` (around line 632-634). In that `if timed_out {` block add:

```rust
            stats.timed_out_pair_ids.push((
                u32::try_from(i).expect("class index fits in u32"),
                u32::try_from(j).expect("class index fits in u32"),
            ));
```

- [ ] **Step 5: Store the set on `Classification` + expose it**

The tier results merge per-tier `ClassificationStats` into the final `stats` (search for `stats.timed_out_pairs += sd.timed_out_pairs;`, ~line 1229). Immediately after that line add:

```rust
            stats.timed_out_pair_ids.extend(sd.timed_out_pair_ids.iter().copied());
```

`Classification` already stores `stats: ClassificationStats` (field at ~line 65), so the set is already retained. Add the accessor in `impl Classification` (near `unsatisfiable_classes`, ~line 338):

```rust
    /// The `(sub, sup)` IRI pairs whose subsumption probe timed out at the
    /// configured deadline — the *flagged-undecided* set. Sound anytime
    /// contract: a timed-out pair is reported "not subsumed" but recorded
    /// here, so a consumer knows exactly which subsumptions are unverified.
    #[must_use]
    pub fn undecided_pairs(&self) -> Vec<(&str, &str)> {
        self.stats
            .timed_out_pair_ids
            .iter()
            .map(|&(i, j)| {
                (
                    self.classes[i as usize].as_str(),
                    self.classes[j as usize].as_str(),
                )
            })
            .collect()
    }
```

- [ ] **Step 6: Run the test to verify it passes**

Run: `cargo test -p owl-dl-reasoner --lib undecided_pairs_reports_timed_out_subsumptions 2>&1 | tail -8`
Expected: PASS.

- [ ] **Step 7: Guard against regression in the existing classify tests + clippy**

Run: `cargo test -p owl-dl-reasoner --lib 2>&1 | tail -4` (all green — additive change) and `cargo clippy -p owl-dl-reasoner --all-targets --all-features -- -D warnings 2>&1 | tail -4` (clean). `Vec` default is fine; `ClassificationStats` derives `Default` already — confirm the new field doesn't break the derive (it won't; `Vec` is `Default`).

- [ ] **Step 8: Commit**

```bash
git add crates/owl-dl-reasoner/src/classify.rs
git commit -m "feat(classify): expose undecided (timed-out) pair set for anytime calibration

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Per-pair sweep harness (metrics vs oracle)

**Files:**
- Modify: `crates/owl-dl-reasoner/tests/konclude_closure_diff.rs`

- [ ] **Step 1: Read the harness's existing helpers**

Read `crates/owl-dl-reasoner/tests/konclude_closure_diff.rs` and identify, by name: (a) the per-fixture oracle-closure loader (the fn that yields the HermiT/Konclude subsumption set, e.g. how `sio_closure_matches_konclude` obtains `konclude_closure`), (b) how it extracts rustdl's closure as comparable IRI pairs, (c) the fixture path resolution, and (d) the `RUSTDL_TEST_PAIR_MS` read. The sweep reuses these — do NOT reimplement oracle loading.

- [ ] **Step 2: Write the sweep harness (an `#[ignore]`d test)**

Append to `konclude_closure_diff.rs`. Replace `load_oracle_pairs(fixture)`, `rustdl_closure_pairs(&classification)`, and `fixture_ontology(fixture)` below with the actual helper names found in Step 1 (co-located in this file, so private helpers are in scope):

```rust
/// Anytime per-pair sweep: for each fixture and per-pair deadline, record
/// precision / recall / silent-miss / wall vs the oracle closure. Writes a
/// CSV. Ignored (run explicitly):
/// `RUSTDL_ANYTIME_CSV=docs/anytime-results-2026-06-11.csv \
///  cargo test -p owl-dl-reasoner --release --test konclude_closure_diff \
///  -- --ignored --nocapture anytime_per_pair_sweep`
#[test]
#[ignore]
fn anytime_per_pair_sweep() {
    use std::collections::HashSet;
    use std::time::{Duration, Instant};

    let fixtures = ["galen", "alehif", "sio", "wine", "ore-10908", "ore-15672"];
    let deadlines_ms = [5u64, 25, 100, 250, 1000];
    let csv_path = std::env::var("RUSTDL_ANYTIME_CSV")
        .unwrap_or_else(|_| "/tmp/anytime-per-pair.csv".to_string());
    let mut csv = String::from(
        "fixture,phase,deadline_ms,recall,precision,silent_miss,wall_ms,undecided,true_pairs\n",
    );

    for fx in fixtures {
        // `true_pairs`: oracle closure as a set of (sub,sup) IRI strings,
        // EXCLUDING reflexive i==j (match whatever the existing diff does).
        let true_pairs: HashSet<(String, String)> = load_oracle_pairs(fx);
        let onto = fixture_ontology(fx);
        for &ms in &deadlines_ms {
            let t0 = Instant::now();
            let h = owl_dl_reasoner::classify_with_timeout(&onto, Duration::from_millis(ms))
                .expect("classify");
            let wall_ms = t0.elapsed().as_millis();
            let reported: HashSet<(String, String)> = rustdl_closure_pairs(&h);
            let undecided: HashSet<(String, String)> = h
                .undecided_pairs()
                .into_iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect();

            let tp = reported.intersection(&true_pairs).count();
            let fp = reported.difference(&true_pairs).count();
            let missed: HashSet<(String, String)> =
                true_pairs.difference(&reported).cloned().collect();
            let precision = if reported.is_empty() {
                1.0
            } else {
                tp as f64 / reported.len() as f64
            };
            let recall = if true_pairs.is_empty() {
                1.0
            } else {
                tp as f64 / true_pairs.len() as f64
            };
            // silent-miss: missed pairs NOT flagged undecided. Calibration
            // claim ⇒ 0. (A non-zero value is a real finding — see the spec
            // gates — investigate, do not paper over.)
            let silent_miss = missed.difference(&undecided).count();

            // SOUNDNESS GATE: precision must be 1.0 (FP=0) at every deadline.
            assert_eq!(
                fp, 0,
                "FP at {fx}@{ms}ms = {fp} (precision {precision}); a deadline must \
                 never produce an unsound subsumption"
            );

            use std::fmt::Write;
            let _ = writeln!(
                csv,
                "{fx},per_pair,{ms},{recall:.6},{precision:.6},{silent_miss},{wall_ms},{},{}",
                undecided.len(),
                true_pairs.len()
            );
            println!(
                "{fx} @ {ms}ms: recall={recall:.4} precision={precision:.4} \
                 silent_miss={silent_miss} wall={wall_ms}ms undecided={} missed={}",
                undecided.len(),
                missed.len()
            );
        }
    }
    std::fs::write(&csv_path, csv).expect("write CSV");
    println!("wrote {csv_path}");
}
```

- [ ] **Step 3: Compile-check (don't run the full sweep yet)**

Run: `cargo test -p owl-dl-reasoner --release --test konclude_closure_diff --no-run 2>&1 | tail -6`
Expected: compiles. Fix any helper-name mismatches from Step 1.

- [ ] **Step 4: Smoke-run one fast fixture**

Temporarily narrow `fixtures` to `["galen"]` (or add an env override), run:
`cargo test -p owl-dl-reasoner --release --test konclude_closure_diff -- --ignored --nocapture anytime_per_pair_sweep 2>&1 | tail -15`
Expected: galen shows recall≈1.0, precision=1.0, silent_miss=0 at all deadlines (EL → instant). Restore the full `fixtures` list.

- [ ] **Step 5: Commit**

```bash
git add crates/owl-dl-reasoner/tests/konclude_closure_diff.rs
git commit -m "test(anytime): per-pair deadline sweep harness (precision/recall/silent-miss vs oracle)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: Run the per-pair sweep + write the results doc

**Files:**
- Create: `docs/anytime-results-2026-06-11.csv`, `docs/anytime-results-2026-06-11.md`

- [ ] **Step 1: Ensure the corpus is present**

Run: `ls ontologies/real/ 2>/dev/null | head` — if empty, run `./scripts/fetch-real-ontologies.sh` (the closure-diff tests need the fixtures; if a fixture is missing the loader will error — note which ran).

- [ ] **Step 2: Run the full per-pair sweep**

Run:
```bash
RUSTDL_ANYTIME_CSV=docs/anytime-results-2026-06-11.csv \
  cargo test -p owl-dl-reasoner --release --test konclude_closure_diff -- \
  --ignored --nocapture anytime_per_pair_sweep 2>&1 | tee /tmp/anytime-per-pair.log | tail -40
```
Expected: no panic (the `assert_eq!(fp, 0)` gate holds at every deadline — if it fires, STOP: a real soundness bug surfaced; report it, do not continue). CSV written to `docs/`.

- [ ] **Step 3: Write the results doc**

Create `docs/anytime-results-2026-06-11.md` with: (a) a per-fixture table (deadline_ms → recall, precision, silent_miss, wall_ms, undecided) from the CSV; (b) the headline observations stated factually — precision=1.0 at every deadline across all fixtures (the soundness claim), recall rising with deadline (with the actual numbers), silent_miss=0 everywhere (the calibration claim), galen flat-at-1.0 (zero anytime overhead on EL); (c) any anomaly (e.g. a fixture where silent_miss > 0 — report honestly with the count and which pairs). 2–3 sentences of interpretation per fixture, written for direct reuse in the paper's evaluation section. Do not invent numbers — transcribe from the CSV/log.

- [ ] **Step 4: Commit**

```bash
git add docs/anytime-results-2026-06-11.csv docs/anytime-results-2026-06-11.md
git commit -m "docs(anytime): per-pair sweep results (Phase 1 evidence)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

# PHASE 2 — global wall-clock deadline + Konclude contrast

### Task 4: Build the global wall-clock deadline

**Files:**
- Modify: `crates/owl-dl-reasoner/src/classify.rs`

The mechanism: a single shared **absolute** `Instant` deadline for the whole run. Every probe uses that absolute deadline (not a fresh `now()+per_pair`), so a probe reached late has little/no budget and times out → undecided. A tier-walk short-circuit skips probes once the deadline has passed (they would instant-timeout anyway; the short-circuit just avoids the churn).

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` in `classify.rs`:

```rust
#[test]
fn global_deadline_is_sound_and_bounded() {
    use std::time::{Duration, Instant};
    // Reuse a pathological-enough inline ontology; a 50ms global budget
    // must return quickly (bounded) and soundly (no FP — checked via the
    // hierarchy being a subset of the untimed hierarchy is overkill here;
    // assert it returns Ok and respects the wall within a slack factor).
    let src = "Prefix(:=<http://t/>)\n\
Ontology(\n  Declaration(Class(:A)) Declaration(Class(:B))\n  SubClassOf(:A :B)\n)\n";
    let onto = parse_ofn(src);
    let t0 = Instant::now();
    let h = classify_with_global_deadline(&onto, Duration::from_millis(50)).expect("classify");
    assert!(t0.elapsed() < Duration::from_secs(5), "global deadline must bound the wall");
    // Trivial subsumption A⊑B is told/saturator-decided (not probe-gated),
    // so it survives even a tiny global budget:
    assert!(h.is_subclass("http://t/A", "http://t/B"));
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p owl-dl-reasoner --lib global_deadline_is_sound_and_bounded 2>&1 | tail -10`
Expected: FAIL — `classify_with_global_deadline` undefined.

- [ ] **Step 3: Add the public entry point + thread the absolute deadline**

Add a public fn mirroring `classify_with_timeout` (near it, ~line 379):

```rust
/// Classify under a single **global** wall-clock budget: the whole run
/// shares one absolute deadline. Pairs not confirmed by the deadline are
/// reported "not subsumed" and recorded in `undecided_pairs()` (sound
/// under-approximation; nothing is asserted on timeout, only omitted).
pub fn classify_with_global_deadline<A: ForIRI>(
    ontology: &SetOntology<A>,
    budget: std::time::Duration,
) -> Result<Classification, ReasonError> {
    let internal = convert_ontology(ontology)?;
    let deadline = std::time::Instant::now() + budget;
    classify_top_down_internal_global(&internal, deadline)
}
```

Add `classify_top_down_internal_global(internal, deadline: Instant)` as a thin wrapper over the existing `classify_top_down_internal` that passes the deadline as a *global absolute* deadline. The minimal-touch implementation: give `classify_top_down_internal` an extra `global_deadline: Option<Instant>` parameter (default `None` for existing callers — update them to pass `None`), and at the probe sites use it:
- In `subsumes_via_tableau`, the deadline is currently `Instant::now() + per_pair_timeout`. Add a `global_deadline: Option<Instant>` parameter; the effective deadline becomes `global_deadline.unwrap_or_else(|| Instant::now() + per_pair_timeout)` (and if both, take the min). Thread `global_deadline` through `find_direct_parents_top_down` to `subsumes_via_tableau`.
- In `find_direct_parents_top_down`, at the top of the `while let Some(d) = frontier.pop()` loop, add a short-circuit: `if let Some(gd) = global_deadline { if Instant::now() >= gd { stats.timed_out_pair_ids.push((c as u32, d as u32)); stats.timed_out_pairs += 1; continue; } }` — so once the budget is spent, remaining candidates are flagged undecided without probing. (Use the file's `u32::try_from` idiom, not `as u32`.)
- The per-class unsat-probe pass (the `prepared.decide_with_deadline` at ~1107) should likewise use `global_deadline` when set.

Keep `classify_top_down_internal_global` = `classify_top_down_internal(internal, None /*per_pair*/, Some(deadline))`. Match the actual existing signature when adding the parameter.

- [ ] **Step 4: Run the test + the full classify suite**

Run: `cargo test -p owl-dl-reasoner --lib global_deadline 2>&1 | tail -6` (new test passes) then `cargo test -p owl-dl-reasoner --lib 2>&1 | tail -4` (existing tests green — the new param defaulted `None` is behavior-preserving) and `cargo clippy -p owl-dl-reasoner --all-targets --all-features -- -D warnings 2>&1 | tail -4`.

- [ ] **Step 5: Commit**

```bash
git add crates/owl-dl-reasoner/src/classify.rs
git commit -m "feat(classify): global wall-clock deadline (anytime classification)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: Global-deadline differential gate

**Files:**
- Modify: `crates/owl-dl-reasoner/tests/konclude_closure_diff.rs`

- [ ] **Step 1: Write the differential test (an `#[ignore]`d test)**

A global deadline ≥ the fixture's untimed wall must drop nothing — the hierarchy must equal the untimed hierarchy, FP=0/MISSED=0. Append:

```rust
/// At a generous global deadline, the global-deadline classifier must
/// produce the SAME hierarchy as the untimed classifier (no spurious
/// drops). Run: `... --ignored --nocapture global_deadline_differential`.
#[test]
#[ignore]
fn global_deadline_differential() {
    use std::collections::HashSet;
    use std::time::Duration;
    // alehif: fully classifiable well under 30s; a 30s global budget must
    // equal the untimed result.
    for fx in ["galen", "alehif"] {
        let onto = fixture_ontology(fx);
        let untimed: HashSet<(String, String)> =
            rustdl_closure_pairs(&owl_dl_reasoner::classify(&onto).expect("classify"));
        let timed: HashSet<(String, String)> = rustdl_closure_pairs(
            &owl_dl_reasoner::classify_with_global_deadline(&onto, Duration::from_secs(30))
                .expect("classify"),
        );
        assert_eq!(timed, untimed, "{fx}: 30s global deadline must equal untimed hierarchy");
    }
}
```

- [ ] **Step 2: Run it**

Run: `cargo test -p owl-dl-reasoner --release --test konclude_closure_diff -- --ignored --nocapture global_deadline_differential 2>&1 | tail -10`
Expected: PASS (galen + alehif identical). If it fails, the global-deadline mechanism is dropping confirmable pairs — STOP and fix Task 4 before proceeding.

- [ ] **Step 3: Commit**

```bash
git add crates/owl-dl-reasoner/tests/konclude_closure_diff.rs
git commit -m "test(anytime): global-deadline differential gate (=untimed at generous budget)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 6: Global sweep + Konclude contrast + final results

**Files:**
- Modify: `crates/owl-dl-reasoner/tests/konclude_closure_diff.rs`
- Modify: `docs/anytime-results-2026-06-11.csv`, `docs/anytime-results-2026-06-11.md`

- [ ] **Step 1: Add the global sweep (clone Task 2's harness, global deadline)**

Append a second `#[ignore]`d test `anytime_global_sweep` identical to `anytime_per_pair_sweep` EXCEPT: deadlines are global wall-clock `[100u64, 1000, 10_000, 30_000]` ms, it calls `classify_with_global_deadline(&onto, Duration::from_millis(ms))`, and the CSV `phase` column is `global`. It appends to the same CSV file (open in append mode, or write a separate CSV `docs/anytime-results-2026-06-11-global.csv` and merge in Step 4). Keep the same `assert_eq!(fp, 0)` soundness gate.

- [ ] **Step 2: Measure Konclude's per-fixture wall (`W_k`)**

Locate the native-Konclude invocation used to produce the oracle (search the repo: `grep -rin "konclude" scripts/ xtask/ docs/perf-2026-06-08-konclude-vs-rustdl.md | head`). Time `Konclude classification` on each fixture (galen/alehif/sio/wine/ore-10908/ore-15672), e.g.:
```bash
for f in <fixture owl/ofn paths>; do /usr/bin/time -v <konclude classify cmd> "$f" 2>&1 | grep -iE "wall clock|Elapsed"; done
```
Record `W_k` (seconds) per fixture in a small table. If the native binary/path can't be located, note it and record `W_k` as "n/a" — the rustdl curves still stand; the contrast is then qualitative.

- [ ] **Step 3: Run the global sweep**

Run:
```bash
cargo test -p owl-dl-reasoner --release --test konclude_closure_diff -- \
  --ignored --nocapture anytime_global_sweep 2>&1 | tee /tmp/anytime-global.log | tail -40
```
Expected: no FP-gate panic. Note recall at each global deadline per fixture.

- [ ] **Step 4: Extend the results doc with Phase 2 + the contrast**

Append to `docs/anytime-results-2026-06-11.md`: (a) the global-deadline recall/precision/silent-miss table; (b) the Konclude contrast table — `W_k` per fixture vs rustdl's recall at `T ∈ {100ms, 1s, 10s, 30s}`, stated as "for `T < W_k` Konclude returns nothing; rustdl returns recall=X% sound"; (c) one paragraph synthesizing the paper claim from the actual numbers. Merge the global CSV rows into `docs/anytime-results-2026-06-11.csv` (single file, `phase ∈ {per_pair, global}`). Transcribe real numbers only.

- [ ] **Step 5: fmt + clippy + final commit**

Run: `cargo fmt --all -- --check` (rc 0; run `cargo fmt --all` if needed) and `cargo clippy -p owl-dl-reasoner --all-targets --all-features -- -D warnings 2>&1 | tail -4` (clean).

```bash
git add -A
git commit -m "docs+test(anytime): global sweep + Konclude all-or-nothing contrast (Phase 2 evidence)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Notes for the implementer

- **The `assert_eq!(fp, 0)` gate in the sweeps is the cardinal invariant**, not a formality: precision must be 1.0 at every deadline. If it ever fires, you've found a real soundness bug — STOP, report it, do not "fix" it by relaxing the assert.
- **silent-miss > 0 is a finding, not a failure** of the harness: it would mean a miss source that isn't a flagged timeout (e.g. a trust_sat wedge miss). Report it honestly in the results doc with the offending pairs; do not hide it. (Expected 0 given corpus MISSED=0 at infinite budget.)
- **Do not push** unless the user asks (CI runs on push to main now; these are test + docs additions, but the push decision is the user's).
- Reuse the closure-diff harness's oracle helpers; do not reimplement oracle loading (it's the trusted surface).
- ore-15672 is slow; the global sweep at 30s × that fixture is the long pole — expect the run to take minutes.
