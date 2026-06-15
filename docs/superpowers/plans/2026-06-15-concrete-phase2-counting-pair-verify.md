# Concrete Phase 2: counting-pair verification — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the measured classify miss where a named⊑named data-counting subsumption (`C ⊑ ≥5 p.int` ⟹ `C ⊑ D≡≥3 p.int`) is dropped because the classifier trusts the wedge's `NotSubsumed`; route counting-relevant pairs to the main tableau (which has the sound `concrete_domain_clash`) instead.

**Architecture:** A guard inside `subsumes_via_tableau`'s `NotSubsumed if trust_sat` arm (`crates/owl-dl-reasoner/src/classify.rs`): when the pair `(sub,sup)` is counting-relevant, fall through to the existing main-tableau probe instead of trusting the wedge. A precomputed `counting_relevant` set (subsumer-expanded `data_counting_classes`) is threaded into `subsumes_via_tableau` via its callers. Reuses the sound main-tableau clash — NO wedge surgery. Empty corpus-wide ⇒ zero behavioral change.

**Tech Stack:** Rust (edition 2024), the existing classify pipeline (`Subsumers` closure, `PreparedOntology`, `subsumes_via_tableau`, the defined-sup sweep).

**Spec:** `docs/superpowers/specs/2026-06-15-concrete-phase2-counting-pair-verify-design.md`

**Build/test prelude (no `cargo` on PATH by default):**
```sh
export PATH="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"
```

**Soundness contract:** This touches classify's trust boundary. It must never produce a false subsumption (FP=0). The guard only swaps a *trusted wedge `Sat`* for the *complete main-tableau verdict* on counting pairs — the main tableau's `concrete_domain_clash` is refute-only and already ships sound, so a tableau `Subsumed` is a genuine entailment. No backjumping/merge/wedge code is touched.

**Key facts verified during design (do not re-derive):**
- The miss reproduces: `C ⊑ DataMinCardinality(5 :p xsd:integer)` + `EquivalentClasses(:D DataMinCardinality(3 :p xsd:integer))` ⇒ default classify omits `C⊑D`; `RUSTDL_HYPERTABLEAU_TRUST_SAT=0` finds it.
- `D` (a complex `EquivalentClasses`) lands in `defined_sups` (classify.rs:1367), so the **defined-sup sweep** (line ~1402) tests `(C,D)` via `subsumes_via_tableau` with `trust_sat=true` (line 1449) — that is where the fix lands. The single guard also covers the main top-down walk's two `subsumes_via_tableau` calls (lines 1711, 1735).
- `prepared.decide` (the fall-through at lines 1907-1917) runs the main tableau with `concrete_domain_clash` + `dkey_ranges` threaded — confirmed to decide `C⊑D`.
- `data_counting_classes` is empty across the whole corpus ⇒ `counting_relevant` empty ⇒ guard never fires ⇒ walls/verdicts byte-identical.

---

### Task 1: Env gate + stat field (scaffolding)

**Files:**
- Modify: `crates/owl-dl-reasoner/src/lib.rs` (add `counting_pair_verify_enabled()` near `label_heuristic_enabled` ~line 700)
- Modify: `crates/owl-dl-reasoner/src/classify.rs` (add `counting_verified_pairs` field to `ClassificationStats` ~line 138)

- [ ] **Step 1: Add the gate function**

In `crates/owl-dl-reasoner/src/lib.rs`, after `label_heuristic_enabled` (~line 702):

```rust
/// Concrete Phase 2 — counting-pair verification. When ON, a wedge
/// `NotSubsumed` verdict on a subsumption pair where either side is
/// data-counting-constrained (or has a counting subsumer) is NOT trusted;
/// the pair falls through to the main tableau (`concrete_domain_clash`).
/// Sound (only swaps a trusted wedge `Sat` for the complete path). On by
/// default; `RUSTDL_COUNTING_PAIR_VERIFY=0` reverts to trusting the wedge.
#[must_use]
pub fn counting_pair_verify_enabled() -> bool {
    std::env::var_os("RUSTDL_COUNTING_PAIR_VERIFY").is_none_or(|v| v != "0" && !v.is_empty())
}
```

- [ ] **Step 2: Add the stat field**

In `crates/owl-dl-reasoner/src/classify.rs`, add to `pub struct ClassificationStats` (~line 138), next to `hyper_refuted_pairs`:

```rust
    /// Phase 2: subsumption pairs recovered by counting-pair verification —
    /// a wedge `NotSubsumed` that the main-tableau `concrete_domain_clash`
    /// flipped to `Subsumed` because the pair was data-counting-relevant.
    pub counting_verified_pairs: usize,
```

- [ ] **Step 3: Verify it builds**

Run: `cargo build -p owl-dl-reasoner 2>&1 | tail -2`
Expected: Finished, no errors. (`ClassificationStats` derives `Default`, so the new field needs no other change. If a manual aggregation site lists every field, add `counting_verified_pairs` there — search `hyper_refuted_pairs +=` and mirror.)

- [ ] **Step 4: Commit**

```bash
git add crates/owl-dl-reasoner/src/lib.rs crates/owl-dl-reasoner/src/classify.rs
git commit -m "feat(classify): counting_pair_verify gate + counting_verified_pairs stat (Phase 2 scaffolding)"
```

---

### Task 2: counting_relevant precompute + threading + the guard (the core)

**Files:**
- Modify: `crates/owl-dl-reasoner/src/classify.rs` (driver precompute; thread param through `find_direct_parents_top_down` + the defined-sup sweep + `subsumes_via_tableau`; the guard)
- Test: `crates/owl-dl-reasoner/tests/classify_concrete_domain.rs` (headline canary)

- [ ] **Step 1: Write the failing headline canary**

Append to `crates/owl-dl-reasoner/tests/classify_concrete_domain.rs` (follow the file's existing parse/classify helper; the pattern is `classify(&parse(SRC)).unwrap()` then `.is_subclass(sub, sup)`, as in `tests/label_heuristic_canary.rs`):

```rust
/// Phase 2 headline: `C ⊑ ≥5 p.int` entails `C ⊑ D` where `D ≡ ≥3 p.int`
/// (cardinality monotonicity `≥5 ⟹ ≥3`). The default classifier trusted the
/// wedge `NotSubsumed` and missed this; counting-pair verification routes it
/// to the main tableau's `concrete_domain_clash`.
#[test]
fn phase2_cardinality_monotonicity_subsumption_is_found() {
    let src = r#"Prefix(:=<http://t/>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://t/x>
  Declaration(Class(:C)) Declaration(Class(:D)) Declaration(DataProperty(:p))
  SubClassOf(:C DataMinCardinality(5 :p xsd:integer))
  EquivalentClasses(:D DataMinCardinality(3 :p xsd:integer))
)
"#;
    let result = owl_dl_reasoner::classify(&parse(src)).expect("classify");
    assert!(
        result.is_subclass("http://t/C", "http://t/D"),
        "C ⊑ D must be found via counting-pair verification (≥5 ⟹ ≥3)"
    );
}
```
If `classify_concrete_domain.rs` has no `parse` helper, copy the 6-line one from `tests/label_heuristic_canary.rs` (Cursor + `read_ofn` + `.0`).

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p owl-dl-reasoner --test classify_concrete_domain phase2_cardinality_monotonicity`
Expected: FAIL — `C ⊑ D` not found (the current miss).

- [ ] **Step 3: Precompute `counting_relevant` in the classify driver**

In `classify.rs`, in the driver function, AFTER `closure` and `prepared` are available and `n` is known, and BEFORE the tier walk (the `for tier in &tiers` loop ~line 1293) AND before the defined-sup sweep (~line 1402), add:

```rust
    // Phase 2: classes whose subsumption pairs must be counting-verified —
    // a data-counting class, or one with a counting subsumer. Empty (the
    // whole corpus) ⇒ the guard in `subsumes_via_tableau` never fires.
    let counting_relevant: std::collections::HashSet<owl_dl_core::ClassId> =
        if !crate::counting_pair_verify_enabled() || prepared.data_counting_classes.is_empty() {
            std::collections::HashSet::new()
        } else {
            (0..n)
                .map(|i| owl_dl_core::ClassId::new(u32::try_from(i).expect("class index fits in u32")))
                .filter(|&c| {
                    prepared.data_counting_classes.contains(&c)
                        || closure
                            .subsumers_of(c)
                            .iter()
                            .any(|s| prepared.data_counting_classes.contains(s))
                })
                .collect()
        };
```
(`closure.subsumers_of(c) -> Vec<ClassId>` exists at owl-dl-saturation lib.rs:1063.)

- [ ] **Step 4: Add the parameter to `subsumes_via_tableau` and the guard**

Change the signature (`classify.rs:1787`) to add a final param:
```rust
fn subsumes_via_tableau(
    prepared: &PreparedOntology,
    sub: owl_dl_core::ClassId,
    sup: owl_dl_core::ClassId,
    per_pair_timeout: Option<std::time::Duration>,
    global_deadline: Option<Instant>,
    trust_sat: bool,
    counting_relevant: &std::collections::HashSet<owl_dl_core::ClassId>,
    stats: &mut ClassificationStats,
) -> Result<Option<bool>, ReasonError> {
```

In the `HyperVerdict::NotSubsumed if trust_sat && crate::hyper_trust_sat_enabled()` arm (~line 1887), BEFORE the existing `hyper_trust_sat_min_ms` logic, add the counting guard. Replace the arm body:

```rust
        crate::HyperVerdict::NotSubsumed if trust_sat && crate::hyper_trust_sat_enabled() => {
            // Phase 2: counting-pair verification. If either side is
            // data-counting-relevant, the wedge `NotSubsumed` is untrusted
            // (the wedge has no `card_sat`); fall through to the main
            // tableau, which runs `concrete_domain_clash`. Sound: only
            // swaps a trusted wedge `Sat` for the complete path.
            if counting_relevant.contains(&sub) || counting_relevant.contains(&sup) {
                counting_verified = true;
                // fall through to the tableau probe (no early return).
            } else {
                // Phase 1 selective verification (unchanged): trust a
                // `NotSubsumed` returned at/after the min-ms threshold.
                let threshold = crate::hyper_trust_sat_min_ms();
                if threshold == 0 || wedge_elapsed_ms >= threshold {
                    stats.hyper_refuted_pairs += 1;
                    return Ok(Some(false));
                }
                stats.hyper_refuted_fast_pairs += 1;
                was_fast_refuted = true;
            }
        }
```

Add the new local flag near `let mut was_fast_refuted = false;` (~line 1881):
```rust
    let mut counting_verified = false;
```

In the tableau-probe results (both the `None` deadline arm ~line 1917-1923 and the `Some(deadline)` arm ~line 1932-1939), after computing `subsumed`, bump the new stat. Add to BOTH `subsumed`-computing branches, right after `let subsumed = !sat;`:
```rust
            if counting_verified && subsumed {
                stats.counting_verified_pairs += 1;
            }
```

- [ ] **Step 5: Thread `counting_relevant` through the call sites**

`subsumes_via_tableau` is called at 3 sites; add `&counting_relevant` as the new second-to-last arg (before `stats`):
1. Defined-sup sweep (~line 1449): the closure passed to `par_iter().map(...)` must capture `&counting_relevant`. Add the argument to the call.
2. & 3. Inside `find_direct_parents_top_down` (lines 1711 and 1735): these need `counting_relevant` in scope — add it as a parameter to `find_direct_parents_top_down` (signature ~line 1656, add `counting_relevant: &std::collections::HashSet<owl_dl_core::ClassId>` before `stats`), and pass `counting_relevant` at both internal calls.

Then update the caller of `find_direct_parents_top_down` (driver ~line 1302) to pass `&counting_relevant`. NOTE: the tier walk is `tier.par_iter().map(|&c| ... find_direct_parents_top_down(..., &counting_relevant, ...))` — `&counting_relevant` is captured by the rayon closure (it's `Sync`), fine.

- [ ] **Step 6: Run the headline canary**

Run: `cargo test -p owl-dl-reasoner --test classify_concrete_domain phase2_cardinality_monotonicity`
Expected: PASS (`C ⊑ D` now found).

- [ ] **Step 7: fmt + clippy under -D warnings**

Run: `cargo fmt -p owl-dl-reasoner && cargo clippy -p owl-dl-reasoner --tests -- -D warnings 2>&1 | tail -2`
Expected: Finished, no error.

- [ ] **Step 8: Commit**

```bash
git add crates/owl-dl-reasoner/src/classify.rs crates/owl-dl-reasoner/tests/classify_concrete_domain.rs
git commit -m "feat(classify): Phase 2 counting-pair verification — route counting pairs to the main tableau"
```

---

### Task 3: FP-guard, subsumer-inheritance, and gate canaries

**Files:**
- Test: `crates/owl-dl-reasoner/tests/classify_concrete_domain.rs` (append)

- [ ] **Step 1: Write the canaries**

```rust
/// FP GUARD: `≥3` does NOT entail `≥5`, so `C ⊑ D` must NOT be reported.
#[test]
fn phase2_weaker_lower_bound_is_not_subsumed() {
    let src = r#"Prefix(:=<http://t/>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://t/x>
  Declaration(Class(:C)) Declaration(Class(:D)) Declaration(DataProperty(:p))
  SubClassOf(:C DataMinCardinality(3 :p xsd:integer))
  EquivalentClasses(:D DataMinCardinality(5 :p xsd:integer))
)
"#;
    let result = owl_dl_reasoner::classify(&parse(src)).expect("classify");
    assert!(
        !result.is_subclass("http://t/C", "http://t/D"),
        "≥3 must NOT be reported ⊑ ≥5 (false subsumption = FP)"
    );
}

/// FP GUARD: different property ⇒ no subsumption.
#[test]
fn phase2_different_property_is_not_subsumed() {
    let src = r#"Prefix(:=<http://t/>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://t/x>
  Declaration(Class(:C)) Declaration(Class(:D))
  Declaration(DataProperty(:p)) Declaration(DataProperty(:q))
  SubClassOf(:C DataMinCardinality(5 :p xsd:integer))
  EquivalentClasses(:D DataMinCardinality(3 :q xsd:integer))
)
"#;
    let result = owl_dl_reasoner::classify(&parse(src)).expect("classify");
    assert!(
        !result.is_subclass("http://t/C", "http://t/D"),
        "≥5 p must NOT be reported ⊑ ≥3 q (different property)"
    );
}

/// Subsumer-inheritance: C ⊑ X, X ⊑ ≥5 p.int, D ≡ ≥3 p.int ⇒ C ⊑ D found
/// (exercises the `counting_relevant` subsumer expansion, not just direct
/// membership — C itself carries no counting axiom).
#[test]
fn phase2_inherited_counting_subsumption_is_found() {
    let src = r#"Prefix(:=<http://t/>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://t/x>
  Declaration(Class(:C)) Declaration(Class(:X)) Declaration(Class(:D))
  Declaration(DataProperty(:p))
  SubClassOf(:C :X)
  SubClassOf(:X DataMinCardinality(5 :p xsd:integer))
  EquivalentClasses(:D DataMinCardinality(3 :p xsd:integer))
)
"#;
    let result = owl_dl_reasoner::classify(&parse(src)).expect("classify");
    assert!(
        result.is_subclass("http://t/C", "http://t/D"),
        "C ⊑ D must be found (C inherits ≥5 from X)"
    );
}

/// Gate: with RUSTDL_COUNTING_PAIR_VERIFY=0 the headline miss returns
/// (verifies the gate disables cleanly). Serialized env mutation — set,
/// classify, then unset; runs in its own test to avoid env races.
#[test]
fn phase2_gate_off_restores_the_miss() {
    let src = r#"Prefix(:=<http://t/>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://t/x>
  Declaration(Class(:C)) Declaration(Class(:D)) Declaration(DataProperty(:p))
  SubClassOf(:C DataMinCardinality(5 :p xsd:integer))
  EquivalentClasses(:D DataMinCardinality(3 :p xsd:integer))
)
"#;
    // SAFETY: single-threaded test; restore immediately after classify.
    unsafe { std::env::set_var("RUSTDL_COUNTING_PAIR_VERIFY", "0") };
    let result = owl_dl_reasoner::classify(&parse(src)).expect("classify");
    let found = result.is_subclass("http://t/C", "http://t/D");
    unsafe { std::env::remove_var("RUSTDL_COUNTING_PAIR_VERIFY") };
    assert!(!found, "with the gate off, the wedge Sat is trusted and C⊑D is missed");
}
```

> NOTE on the gate test: the repo uses an env-guard convention for tests that
> mutate process env (see `test_env_lock`/`EnvGuard` referenced in CLAUDE.md and
> existing flag tests). Prefer that convention if present in the reasoner test
> crate (search for `EnvGuard`/`env_lock`); it serializes env-mutating tests so
> they don't race the parallel test runner. If you use it, wrap this test with
> the guard instead of the raw `set_var`/`remove_var`. Do NOT leave the env var
> set on any exit path.

- [ ] **Step 2: Run all Phase 2 canaries**

Run: `cargo test -p owl-dl-reasoner --test classify_concrete_domain phase2`
Expected: all 5 (`phase2_*`) pass. If the gate test races (env), apply the `EnvGuard` convention.

- [ ] **Step 3: fmt + clippy**

Run: `cargo fmt -p owl-dl-reasoner && cargo clippy -p owl-dl-reasoner --tests -- -D warnings 2>&1 | tail -2`
Expected: no error.

- [ ] **Step 4: Commit**

```bash
git add crates/owl-dl-reasoner/tests/classify_concrete_domain.rs
git commit -m "test(classify): Phase 2 FP-guard, subsumer-inheritance, and gate canaries"
```

---

### Task 4: Corpus regression + opus review gate

**Files:** none (verification). Build release: `cargo build --release -p owl-dl-cli`.

- [ ] **Step 1: Full reasoner + classify suites green**

Run: `cargo test -p owl-dl-reasoner --test classify_concrete_domain --test datatype_value_membership --test datatype_inconsistency --test label_heuristic_canary --test wedge_consistency`
Expected: all PASS (the label-heuristic canaries especially — they pin the trust-boundary behaviour the guard sits next to).

- [ ] **Step 2: Consistent fixtures + classify closure unchanged (the FP/MISSED gate)**

```sh
for f in pizza ro family wine sulo bibtex; do
  printf "%-12s " "$f:"; ./target/release/rustdl consistent ontologies/real/$f.ofn
done
for f in ontologies/external/ore-15672-shoin.ofn ontologies/external/shoiq-knowledge.ofn; do
  printf "%-22s " "$(basename $f):"
  ./target/release/rustdl classify "$f" 2>/dev/null | grep "^# subsumption"
done
```
Expected: all `consistent`; `ore-15672 saturation=142`, `shoiq-knowledge saturation=443` (unchanged — `data_counting_classes` empty corpus-wide ⇒ guard inert).

- [ ] **Step 3: Spot-check a counting-bearing classify wall is not pathological**

Run: `time ./target/release/rustdl classify ontologies/external/shoiq-knowledge.ofn >/dev/null 2>&1`
Expected: within noise of the pre-change wall (shoiq-knowledge has the `=1 xsd:int` axiom — confirm the guard doesn't blow up its wall).

- [ ] **Step 4: Opus review gate**

Dispatch an **opus** spec-compliance + code-quality review of the whole change (Tasks 1-3). It touches classify's trust boundary, so per the hardened rule it gets the stronger review. The reviewer must confirm: (a) the guard cannot produce a false subsumption (FP=0) — it only swaps a trusted wedge `Sat` for the complete tableau path; (b) `counting_relevant` is empty corpus-wide so behaviour is unchanged where it should be; (c) the threading is correct at all 3 call sites; (d) the headline + FP-guard + subsumer + gate canaries genuinely exercise what they claim. Address findings, re-verify Steps 1-3, then finalize.

- [ ] **Step 5: No commit** (verification; fixes land under their task's commit).

---

## Notes for the executor

- Hardened rule: classify-trust-boundary change ⇒ **opus** review (Task 4 Step 4), not sonnet.
- The corpus has ZERO counting pairs, so corpus regression proves "no harm" but NOT "it works" — the `phase2_*` canaries are the entire functional safety net for the feature working; the corpus is the safety net for it not regressing.
- If any consistent fixture flips or any closure count changes, STOP — `counting_relevant` should be empty corpus-wide; a non-empty set on a corpus ontology means `build_data_counting_classes` is broader than expected. Investigate before proceeding.
- Out of scope (documented sound miss): a counting subsumption to a PRIMITIVE super pruned by the Phase 7 label heuristic (the prune at classify.rs:1721, `D ∉ labels(C)`, is untouched). The defined-sup sweep covers the measured (defined-class) case; primitive counting supers would need label-cache counting-awareness, deferred.
