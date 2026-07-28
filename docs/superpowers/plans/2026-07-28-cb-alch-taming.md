# CB SP-A: resurrect ALCH engine + tame the disjunctive blowup — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Resurrect the retired `owl-dl-cb` ALCH consequence-based engine and determine, empirically, whether a taming refinement kills its disjunctive-antichain blowup while preserving completeness (`tamed-S1 ≡ B1`) at FP=0 — the go/no-go for the whole CB pursuit.

**Architecture:** `owl-dl-cb` has two engines: `engine.rs` (B1, unordered, **directly complete** → the completeness oracle) and `seq_engine.rs`+`seq_order.rs` (S1, ordered, single-maximal eligibility → where the taming goes). The blowup is the `∏ᵢ|supports(pᵢ)|` cross-product over an antichain of incomparable disjunctive clauses. SP-A adds candidate taming(s) behind a flag and lets the gate pick the winner.

**Tech Stack:** Rust (edition 2024), `owl-dl-cb` crate (dep: `owl-dl-core` only). Reference: KM at `/data/dumontier/kobayashi-marust/engine` (`engine.rs` branch_ordered `3442-3453`, `clause.rs` max_head_mask), Bate et al. SRIQ-CB (JAIR 63, 2018), Tena Cucala Sequoia (DL 2019).

## Global Constraints

- Build/test: `export PATH="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"; export RUSTUP_TOOLCHAIN=stable`, then `cargo test -p owl-dl-cb`.
- **FP=0 is absolute.** Any false subsumption (tamed-S1 asserts a subsumption B1/oracle does not) is stop-and-diagnose, never ship. This is the crown jewel.
- **Completeness is validated differentially against B1** (unordered, directly complete): `tamed-S1 ≡ B1` on every ontology where B1 terminates. B1 is the oracle; do not weaken B1.
- The taming is default-OFF behind an env flag; flag-off must be byte-identical to the current retired S1.
- `cargo clippy -p owl-dl-cb --all-targets -- -D warnings` clean; `cargo fmt`.
- Commit trailers end with:
  `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`
  `Claude-Session: https://claude.ai/code/session_01BPU4DH5DXn2jmpuXdfijF7`
- Branch: `feat/cb-alch-taming` (off `main`, spec `5f7041c`).

## File structure

- `crates/owl-dl-cb/src/{engine,seq_engine,seq_order,seq_model,model,normalize,classify,seq_classify,lib}.rs` — restored from branch `feat/cb-b1-integration` (Task 1). Taming lands in `seq_order.rs::eligible` (+ maybe `seq_engine.rs::apply_hyper`).
- `crates/owl-dl-cb/tests/cb_sequoia_diff.rs` — the B1≡S1 differential (extended in Tasks 3/5).
- `crates/owl-dl-cb/tests/cb_blowup.rs` — NEW: adversarial ∀-disjunctive ALCH blowup baseline + tamed-terminates regression (Task 2).
- Konclude-oracle FP=0 harness — a scratch script (Task 6), not committed to the repo.

---

### Task 1: Un-stub `owl-dl-cb` (restore B1+S1) and confirm it builds + passes

**Files:**
- Replace: `crates/owl-dl-cb/src/*` (main's 162-LOC stub → branch `feat/cb-b1-integration`'s full engine)
- Restore: `crates/owl-dl-cb/tests/*`

**Interfaces:**
- Produces (for later tasks): `owl_dl_cb::classify_unordered(&InternalOntology) -> CbOutcome` (B1 oracle) and `owl_dl_cb::classify_sequoia(&InternalOntology) -> CbOutcome` (S1); `CbOutcome::{Classified(hier), OutOfFragment(reason)}`.

- [ ] **Step 1: Restore the retired crate + tests**

```bash
cd /data/dumontier/rustdl
git checkout feat/cb-b1-integration -- crates/owl-dl-cb
```

- [ ] **Step 2: Build**

Run: `cargo build -p owl-dl-cb`
Expected: compiles, 0 errors (verified 2026-07-28: builds clean against current `owl-dl-core`).

- [ ] **Step 3: Run its existing tests**

Run: `cargo test -p owl-dl-cb`
Expected: all pass (verified 2026-07-28: 92 tests, 0 failed). If any fail, STOP — the port assumption broke; report.

- [ ] **Step 4: clippy + fmt**

Run: `cargo clippy -p owl-dl-cb --all-targets -- -D warnings && cargo fmt -p owl-dl-cb -- --check`
Expected: clean (fix trivial drift if the 691-commit gap introduced a new lint; do NOT change engine logic).

- [ ] **Step 5: Commit**

```bash
git add crates/owl-dl-cb
git commit -m "feat(cb): resurrect retired ALCH CB engine (B1 unordered + S1 ordered)

Restores owl-dl-cb from feat/cb-b1-integration (was a 162-LOC stub on main).
Builds against current owl-dl-core unchanged; 92/92 tests green. B1 is the
directly-complete completeness oracle; S1 is where the taming lands.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01BPU4DH5DXn2jmpuXdfijF7"
```

---

### Task 2: Adversarial ∀-disjunctive ALCH blowup baseline

**Files:**
- Create: `crates/owl-dl-cb/tests/cb_blowup.rs`

**Interfaces:**
- Consumes: `classify_unordered`, `classify_sequoia` (Task 1).
- Produces: a deterministic generator `fn adversarial_forall_disjunctive(seed: u64, n: usize) -> InternalOntology` and a `run_with_timeout(f, ms) -> Option<Duration>` helper used by later tasks.

The characterized failing pattern (from the retirement verdict): ∀-rich + disjunctive ALCH — GCIs of the form `C ⊑ ∃R.(A ⊔ B)`, `⊤ ⊑ ∀R.(D₁ ⊔ … ⊔ Dₖ)`, disjoint pairs among the `Dᵢ`, chained so the `∀`-rule back-propagates growing disjunctions into successor contexts, producing an antichain `{∃R.A⊔B}, {∃R.A⊔C}, …`.

- [ ] **Step 1: Write the generator + a blowup assertion (failing baseline)**

```rust
// cb_blowup.rs
use std::time::{Duration, Instant};
use owl_dl_cb::{classify_sequoia, classify_unordered, CbOutcome};

/// Build an adversarial ∀-rich disjunctive ALCH ontology parameterised by size.
/// Pattern: n disjoint atoms D0..Dn-1; ⊤ ⊑ ∀R.(D0⊔…⊔Dn-1); a chain X_i ⊑ ∃R.X_{i+1}
/// so ∀-back-propagation grows an incomparable disjunctive antichain in the
/// successor contexts (the ∏|supports| cross-product the retirement measured).
fn adversarial(n: usize) -> owl_dl_core::ontology::InternalOntology { /* build via owl-dl-core IR; see convert/normalize test helpers */ unimplemented!() }

fn walltime(f: impl FnOnce()) -> Duration { let t = Instant::now(); f(); t.elapsed() }

#[test]
#[ignore] // baseline: documents the blowup; run with --ignored
fn s1_blows_up_on_adversarial() {
    let o = adversarial(12);
    // Guard so the test suite can't hang: cap via a thread + join timeout.
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let d2 = done.clone();
    let h = std::thread::spawn(move || { let _ = classify_sequoia(&o); d2.store(true, std::sync::atomic::Ordering::SeqCst); });
    std::thread::sleep(Duration::from_secs(30));
    assert!(!done.load(std::sync::atomic::Ordering::SeqCst),
        "BASELINE: current S1 is expected to blow up (>30s) on adversarial(12); if it finished, the blowup did not reproduce — reassess before taming");
    // (h is leaked deliberately; process exits at test end.)
    let _ = h;
}
```

- [ ] **Step 2: Run the baseline**

Run: `cargo test -p owl-dl-cb --test cb_blowup -- --ignored s1_blows_up_on_adversarial`
Expected: PASS (S1 does NOT finish in 30 s — the blowup reproduces). If it FAILS (S1 finished), the adversarial pattern is wrong or too small — increase `n` / adjust the ∀-disjunction shape until the blowup reproduces, because Task 3's whole point is to fix a real blowup. Document the smallest `n` that blows up.

- [ ] **Step 3: Commit**

```bash
git add crates/owl-dl-cb/tests/cb_blowup.rs
git commit -m "test(cb): adversarial ∀-disjunctive ALCH blowup baseline (S1 hangs >30s)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01BPU4DH5DXn2jmpuXdfijF7"
```

---

### Task 3: Candidate 1 — second-maximal eligibility (behind a flag)

**Files:**
- Modify: `crates/owl-dl-cb/src/seq_order.rs` (`eligible`, ~259-266)
- Modify: `crates/owl-dl-cb/tests/cb_blowup.rs`, `crates/owl-dl-cb/tests/cb_sequoia_diff.rs`

**Interfaces:**
- Consumes: `PerContextOrder::eligible(&self, pool, delta: &[ConceptId], a: ConceptId) -> bool`; `self.atom_gt`; `Self::is_atomic`.
- Produces: env flag `RUSTDL_CB_SECOND_MAXIMAL` (default off = current single-maximal behaviour).

The current `eligible` requires **zero** atoms of `delta` to exceed `a` (single-maximal). Candidate 1 relaxes it to **≤1** (maximal or second-maximal) — the Sequoia 2ⁿ→n³ refinement in rustdl's total-order frame.

- [ ] **Step 1: Differential test that MUST hold under the flag (write first)**

Add to `cb_sequoia_diff.rs` (reuse its `assert_parity(name, body)` which compares `classify_unordered` vs `classify_sequoia` for FP + MISSED). Add a wrapper that sets the flag for the whole test and asserts parity on the existing ALCH fixtures **plus** a small ∀-disjunctive by-cases fixture:

```rust
#[test]
fn second_maximal_preserves_b1_parity() {
    // SAFETY: single-threaded test; set+remove the env around the parity checks.
    unsafe { std::env::set_var("RUSTDL_CB_SECOND_MAXIMAL", "1"); }
    // every fixture used by the existing parity tests must still give B1 ≡ S1:
    assert_parity("bycases", "SubClassOf(:C ObjectUnionOf(:A :B)) SubClassOf(:A :D) SubClassOf(:B :D) ClassAssertion? ..."); // fill with a real by-cases ALCH fixture in OFN
    // ... re-run the existing parity fixtures under the flag ...
    unsafe { std::env::remove_var("RUSTDL_CB_SECOND_MAXIMAL"); }
}
```

- [ ] **Step 2: Run it — expect FAIL (flag not wired)**

Run: `cargo test -p owl-dl-cb --test cb_sequoia_diff second_maximal_preserves_b1_parity`
Expected: FAIL (env flag has no effect yet, or MISSED if the fixture needs the relaxation).

- [ ] **Step 3: Implement the relaxation**

In `seq_order.rs::eligible`:

```rust
pub(crate) fn eligible(&self, pool: &ConceptPool, delta: &[ConceptId], a: ConceptId) -> bool {
    let greater = delta.iter().filter(|&&l| Self::is_atomic(pool, l) && self.atom_gt(l, a)).count();
    // Single-maximal (default): NO atom of delta may exceed a. Second-maximal
    // (RUSTDL_CB_SECOND_MAXIMAL): at most ONE may — completeness-preserving
    // (Bate et al. SRIQ-CB), bounds the derived-clause antichain 2ⁿ→n³.
    let allow = if second_maximal_enabled() { 1 } else { 0 };
    greater <= allow
}
```

Add near the order/engine config:
```rust
fn second_maximal_enabled() -> bool {
    std::env::var_os("RUSTDL_CB_SECOND_MAXIMAL").is_some_and(|v| v != "0" && !v.is_empty())
}
```
(Cache it once if `eligible` is hot — a `OnceLock<bool>` or a field threaded from engine construction; a per-call `var_os` is acceptable for the first cut, optimise only if profiling shows it.)

- [ ] **Step 4: Run the differential — expect PASS**

Run: `cargo test -p owl-dl-cb --test cb_sequoia_diff`
Expected: PASS — `tamed-S1 ≡ B1` on all fixtures (no FP, no MISSED). If MISSED appears, the relaxation is unsound-for-completeness as written — STOP, do not weaken the parity assertion; re-derive from the Bate second-maximal condition.

- [ ] **Step 5: Blowup-tamed regression**

Add to `cb_blowup.rs`:
```rust
#[test]
fn second_maximal_tames_adversarial() {
    unsafe { std::env::set_var("RUSTDL_CB_SECOND_MAXIMAL", "1"); }
    let o = adversarial(12);
    let t = Instant::now();
    let out = classify_sequoia(&o);           // must return, fast
    let dt = t.elapsed();
    unsafe { std::env::remove_var("RUSTDL_CB_SECOND_MAXIMAL"); }
    assert!(matches!(out, CbOutcome::Classified(_)));
    assert!(dt < Duration::from_secs(5), "second-maximal must tame adversarial(12) to <5s, got {dt:?}");
}
```

- [ ] **Step 6: Run it**

Run: `cargo test -p owl-dl-cb --test cb_blowup second_maximal_tames_adversarial`
Expected: PASS if Candidate 1 tames the blowup. **If it FAILS (still slow), that is a real result** — Candidate 1 under-tames; proceed to Task 5 (Candidate 2). Do not force it.

- [ ] **Step 7: Commit**

```bash
git add crates/owl-dl-cb/src/seq_order.rs crates/owl-dl-cb/tests/
git commit -m "feat(cb): Candidate-1 second-maximal eligibility (RUSTDL_CB_SECOND_MAXIMAL, default off)

Relaxes S1 eligibility to allow ≤1 delta atom above the resolved atom (maximal OR
second-maximal; Bate SRIQ-CB, completeness-preserving). Differential: tamed-S1 ≡ B1
on all ALCH fixtures. Blowup regression records whether it tames adversarial(12).

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01BPU4DH5DXn2jmpuXdfijF7"
```

---

### Task 4: Decision — did Candidate 1 tame it? (controller-run)

Not a code task. Assess Tasks 3.4 + 3.6:
- **Parity holds (B1 ≡ tamed-S1) AND blowup tamed (<5s):** Candidate 1 wins → skip Task 5, go to Task 6.
- **Parity holds but blowup NOT tamed:** Candidate 1 under-tames → Task 5 (Candidate 2: KM's cap + splitting).
- **Parity BROKEN (MISSED/FP):** the relaxation as written is wrong → re-derive the second-maximal side-condition from Bate et al. before proceeding; if it cannot be made complete in the total-order frame, that itself is evidence to escalate to Candidate 2.

---

### Task 5 (conditional): Candidate 2 — KM's disjunct-count cap + splitting

**Files:** Modify `crates/owl-dl-cb/src/seq_engine.rs` (`apply_hyper` resolvent construction), `seq_order.rs`, tests.

**Reference (port target):** KM `engine.rs:3442-3453` — when `branch_ordered`, suppress a resolvent if ≥2 of its premise clauses have multi-literal heads (`head.len() > 1`); the ontology-clause head does not count. **Completeness requires the accompanying splitting**: a suppressed consequence is recovered by branching a disjunctive premise down to a unit and resolving normally — so this task ports BOTH the cap AND the splitting (study KM's split path around the `branch_ordered` guard). KM's `max_head_mask` (partial-order antichain, fire on all maximal) is the other half; rustdl's total order already picks a single maximal, so the cap is the primary port.

- [ ] **Step 1: Differential test under the Candidate-2 flag (write first)** — same `assert_parity` structure as Task 3.1 but gated on `RUSTDL_CB_DISJUNCT_CAP`; must hold `tamed-S1 ≡ B1`.
- [ ] **Step 2: Run — expect FAIL** (flag unwired).
- [ ] **Step 3: Implement** the cap in `apply_hyper` (suppress ≥2-multi-head-premise resolvents) + the splitting recovery. Complete code is derived from KM's `engine.rs` branch_ordered + split path — read it and port; the acceptance is the differential, not an a-priori proof.
- [ ] **Step 4: Run differential — expect PASS** (`tamed-S1 ≡ B1`). If MISSED, the splitting is incomplete — that is the classic cap-without-split bug; fix the split, do not weaken parity.
- [ ] **Step 5: Blowup-tamed regression** under `RUSTDL_CB_DISJUNCT_CAP` (adversarial(12) < 5s).
- [ ] **Step 6: Commit.**

---

### Task 6: FP=0 Konclude-oracle gate on the ALCH corpus (controller-run)

**Files:** Create `/mnt/um-share-drive/dumontier/rustdl-scratch/cb_alch_oracle.sh` (not committed).

The winning taming must be FP=0 on real ALCH-fragment ontologies vs an external oracle.

- [ ] **Step 1:** From the ORE/curated corpus, select ontologies in the ALCH fragment (`classify_sequoia` returns `Classified`, not `OutOfFragment`). A small driver binary or test that runs `classify_sequoia` with the winning flag and emits `sub ⊑ sup` pairs.
- [ ] **Step 2:** Run the Konclude native binary (`/data/dumontier/docker/.../snapshots/485/fs/root/Konclude classification`) on the same ontologies; adjudicate FP against Konclude∩HermiT (reuse the unsat-normalize + adjudication method from `km-headtohead-rustdl-fp`).
- [ ] **Step 3:** Assert **FP=0** (no subsumption tamed-CB reports that the oracle rejects). Any FP → stop-and-diagnose.
- [ ] **Step 4:** Record per-ont wall (tamed-CB vs current rustdl classify) — the value signal (is CB fast on ALCH?).

---

### Task 7: Go/no-go verdict + results doc

**Files:** Create `docs/2026-07-28-cb-alch-taming-results.md`; commit.

- [ ] Record: which candidate won (or that both under-tame → escalate/impossibility), the blowup-tamed evidence, the B1≡tamed-S1 differential result, the Konclude FP=0 result, and the ALCH wall comparison. State the verdict: **GREEN → SP-B (+Q)** or **RED → escalate per the spec's Commitment** (deeper refinement / genuine-impossibility finding with evidence). Update memory.

---

## Self-Review

- **Spec coverage:** resurrect (T1); blowup baseline (T2); taming candidates behind flags (T3 second-maximal, T5 KM cap+splitting); completeness differential vs B1 (T3.1/T5.1, extends `cb_sequoia_diff.rs`); FP=0 Konclude gate (T6); go/no-go verdict (T7). The empirical-choice-among-candidates framing (spec correction) → T3/T4/T5. All spec sections mapped.
- **Placeholders:** `adversarial(...)` generator body is `unimplemented!()` in the plan text — the implementer writes it against the `owl-dl-core` IR following the crate's existing normalize/convert test helpers; the *pattern* (∀-disjunction chain producing an incomparable antichain) is fully specified, and T2.2 makes "reproduces the blowup" the concrete acceptance. The OFN by-cases fixture in T3.1 is likewise specified by shape; the implementer fills real IRIs. These are genuine authoring tasks, not hidden decisions.
- **Type consistency:** `classify_unordered`/`classify_sequoia`/`CbOutcome`, `eligible(pool, delta, a)`, `atom_gt`, `is_atomic`, `second_maximal_enabled`, flags `RUSTDL_CB_SECOND_MAXIMAL`/`RUSTDL_CB_DISJUNCT_CAP` used consistently.
- **Honest note:** Tasks 3/5 are research ports whose *exact* code is derived during implementation from KM + Bate/Sequoia; the plan pins the integration points, the flag interface, and the acceptance tests (differential + blowup-tamed + FP=0), which is the correct specificity for a calculus port — the gate, not an a-priori proof, is the arbiter (per spec + advisor).
