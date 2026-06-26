# NomKey-DifferentIndividuals Disjointness (M2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Feed `DifferentIndividuals` into the EL saturator's `disjoint_pairs` as NomKey-disjointness (env-gated, default-OFF), so `≤1`/functional witness-merge over distinct value nominals derives class-unsat — the determinism Konclude has and rustdl's class-level saturator lacks.

**Architecture:** One additive block at the end of `collect_el_rules` (crates/owl-dl-saturation/src/lib.rs), gated by `RUSTDL_NOMKEY_DIFF=1`. For each `Axiom::DifferentIndividuals(inds)`, look up each individual's already-allocated NomKey in `tseitin.nominal_by_ind` (lookup only — no allocation, so `total_classes` is unaffected) and push every distinct pair into `rules.disjoint_pairs`. All downstream machinery (functional witness-merge → Tseitin synthetic `S = NomKey(a) ⊓ NomKey(b)` → existing disjointness check → `process_unsat` ancestor+subclass propagation → B2a forced-disjunct) is unchanged. Flag-OFF path is byte-identical.

**Tech Stack:** Rust (edition 2024), owl-dl-saturation crate, horned-owl OFN test fixtures.

## Global Constraints

- Toolchain: `export RUSTUP_HOME=/home/dumontier/.rustup CARGO_HOME=/home/dumontier/.cargo; export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$CARGO_HOME/bin:$PATH"`
- `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets --all-features -- -D warnings` (pedantic, warnings = errors); `cargo test --workspace` green.
- Branch `feat/nomkey-diff-disjoint` (already created off `main`); `main` stays pristine.
- Flag `RUSTDL_NOMKEY_DIFF` **default OFF**; only `=1` enables. Flag-OFF path byte-identical.
- Commit only when asked; trailers `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>` + `Claude-Session: https://claude.ai/code/session_01HSzon7V2wkhrudxBNAJduh`.
- Soundness of the measured config is verified by the corpus closure-diff (Gate, after Task 1), NOT by unit tests alone — the three prior nominal-pruning FP escapes all passed their canaries.

---

### Task 1: Env-gated DifferentIndividuals → NomKey disjointness rule + canary

**Files:**
- Modify: `crates/owl-dl-saturation/src/lib.rs` (insert the rule just before `collect_el_rules`'s return at line 2927 `(rules, tseitin, total_classes)`; add tests in the existing `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes (already exist): `Axiom::DifferentIndividuals(Vec<IndividualId>)` (owl-dl-core `ontology.rs:88`); `tseitin.nominal_by_ind: HashMap<IndividualId, ClassId>` (forward ind→NomKey map, `lib.rs:2210`, fully populated by line 2897); `rules.disjoint_pairs: Vec<(ClassId, ClassId)>` (`lib.rs:2046`); `internal.axioms`. Test helpers in the same file's test module: `parse_internal(&str) -> InternalOntology`, `saturate(&internal) -> Subsumers`, `class(&internal, "Name") -> ClassId`, `Subsumers::is_unsatisfiable(ClassId) -> bool`, and the `HEADER` prefix const (see existing test `existential_with_unsat_body_propagates_to_source` ~line 4350).
- Produces: behavior only (no new public API). Under `RUSTDL_NOMKEY_DIFF=1`, asserted-distinct nominal fillers become pairwise-disjoint synthetic classes.

- [ ] **Step 1: Write the failing canary tests**

Add to the `#[cfg(test)] mod tests` block in `crates/owl-dl-saturation/src/lib.rs`. The tests set the env var; serialize them with a test-local mutex so a parallel test never observes the var mid-flight.

```rust
    /// M2 canary (negatives-first): `≤1`/functional `r` with two distinct
    /// nominal fillers must be unsat ONLY when `DifferentIndividuals(a,b)` is
    /// asserted AND the flag is on. Without the assertion, a and b may be the
    /// same individual (no UNA) ⇒ NOT unsat. Flag-off ⇒ NOT unsat (byte-identical).
    static M2_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn m2_src(with_different: bool) -> String {
        let diff = if with_different {
            "    DifferentIndividuals(:a :b)\n"
        } else {
            ""
        };
        format!(
            "{HEADER}\
Ontology(<http://rustdl.test/m2>\n\
    Declaration(Class(:C))\n\
    Declaration(ObjectProperty(:r))\n\
    Declaration(NamedIndividual(:a))\n\
    Declaration(NamedIndividual(:b))\n\
    FunctionalObjectProperty(:r)\n\
    SubClassOf(:C ObjectHasValue(:r :a))\n\
    SubClassOf(:C ObjectHasValue(:r :b))\n\
{diff}\
)\n"
        )
    }

    #[test]
    fn m2_distinct_nominal_fillers_clash_when_flag_on() {
        let _g = M2_ENV_LOCK.lock().unwrap();
        // SAFETY: serialized by M2_ENV_LOCK; var removed before unlock.
        unsafe { std::env::set_var("RUSTDL_NOMKEY_DIFF", "1") };
        let internal = parse_internal(&m2_src(true));
        let subs = saturate(&internal);
        let unsat = subs.is_unsatisfiable(class(&internal, "C"));
        unsafe { std::env::remove_var("RUSTDL_NOMKEY_DIFF") };
        assert!(
            unsat,
            "flag-on + DifferentIndividuals(a,b) + ≤1 r + ∃r.{{a}} ⊓ ∃r.{{b}} ⇒ C unsat"
        );
    }

    #[test]
    fn m2_no_clash_without_different_individuals() {
        let _g = M2_ENV_LOCK.lock().unwrap();
        // SAFETY: serialized by M2_ENV_LOCK; var removed before unlock.
        unsafe { std::env::set_var("RUSTDL_NOMKEY_DIFF", "1") };
        let internal = parse_internal(&m2_src(false));
        let subs = saturate(&internal);
        let unsat = subs.is_unsatisfiable(class(&internal, "C"));
        unsafe { std::env::remove_var("RUSTDL_NOMKEY_DIFF") };
        assert!(
            !unsat,
            "flag-on but NO DifferentIndividuals ⇒ a,b may be same ⇒ C NOT unsat (sound, no UNA)"
        );
    }

    #[test]
    fn m2_no_clash_when_flag_off() {
        let _g = M2_ENV_LOCK.lock().unwrap();
        // Flag off (var absent): byte-identical pre-M2 behaviour.
        let internal = parse_internal(&m2_src(true));
        let subs = saturate(&internal);
        assert!(
            !subs.is_unsatisfiable(class(&internal, "C")),
            "flag-off ⇒ DifferentIndividuals not fed to disjoint_pairs ⇒ C NOT unsat"
        );
    }
```

- [ ] **Step 2: Run the canary to verify the first test fails**

Run: `cargo test -p owl-dl-saturation m2_distinct_nominal_fillers_clash_when_flag_on -- --exact`
Expected: FAIL (assert `unsat` is false) — the rule doesn't exist yet, so the flag does nothing.
(The other two tests should already PASS, since flag-on-without-rule == flag-off; that is expected and fine.)

- [ ] **Step 3: Implement the rule**

In `crates/owl-dl-saturation/src/lib.rs`, insert this block immediately before the final `(rules, tseitin, total_classes)` return of `collect_el_rules` (currently line 2927). It only reads `tseitin.nominal_by_ind`, so it must come after all nominal allocation (it does — `total_classes` is already captured at 2897):

```rust
    // M2 (env-gated, default OFF): DifferentIndividuals → NomKey disjointness.
    // `DifferentIndividuals(a,b)` ⟹ `{a} ⊓ {b} = ⊥` ⟹ `NomKey(a) ⊓ NomKey(b)`
    // unsat (NomKey is a 1:1 identity representative). Registering the disjoint
    // pair lets the existing functional/≤1 witness-merge derive `C ⊑ ⊥` on the
    // nominal value-partition pattern (`≤1 R` over distinct value nominals) —
    // the saturation-time determinism Konclude has and our class-level saturator
    // lacked (see docs/stage4-konclude-representation-study-2026-06-26.md).
    // SOUND: only entailed disjointness is added; never UNA-wide. Lookup-only
    // (no NomKey allocation), so `total_classes` above is unaffected. Only pairs
    // for individuals actually used as nominal fillers (those with a NomKey).
    if std::env::var("RUSTDL_NOMKEY_DIFF").as_deref() == Ok("1") {
        for ax in &internal.axioms {
            if let Axiom::DifferentIndividuals(inds) = ax {
                let keys: Vec<ClassId> = inds
                    .iter()
                    .filter_map(|i| tseitin.nominal_by_ind.get(i).copied())
                    .collect();
                for i in 0..keys.len() {
                    for j in (i + 1)..keys.len() {
                        rules.disjoint_pairs.push((keys[i], keys[j]));
                    }
                }
            }
        }
    }

```

- [ ] **Step 4: Run the canary tests to verify they pass**

Run: `cargo test -p owl-dl-saturation m2_ -- --exact 2>&1 | tail -20`
(Run the three `m2_*` tests.) Expected: all three PASS — flag-on+DifferentIndividuals ⇒ C unsat; flag-on without DifferentIndividuals ⇒ C sat; flag-off ⇒ C sat.

- [ ] **Step 5: Verify the flag-OFF path is byte-identical (no accidental default change)**

Run: `cargo test -p owl-dl-saturation 2>&1 | tail -15`
Expected: the full saturation suite is green (the rule is behind `RUSTDL_NOMKEY_DIFF=1`, which is unset for all other tests, so nothing else changes).

- [ ] **Step 6: fmt + clippy**

Run: `cargo fmt --all && cargo clippy -p owl-dl-saturation --all-targets --all-features -- -D warnings 2>&1 | tail -10`
Expected: no diff from fmt, no clippy warnings.

- [ ] **Step 7: Commit (only if the controller has authorized committing)**

```bash
git add crates/owl-dl-saturation/src/lib.rs
git commit -m "feat(saturation): M2 — DifferentIndividuals→NomKey disjointness (RUSTDL_NOMKEY_DIFF, default OFF)

Feed DifferentIndividuals into the EL saturator's disjoint_pairs as
NomKey(a)⊓NomKey(b) disjointness so ≤1/functional witness-merge over distinct
value nominals derives class-unsat. Sound (only entailed disjointness; no UNA).
Default-OFF, flag-off byte-identical. 3 negatives-first canaries.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HSzon7V2wkhrudxBNAJduh"
```

---

## Gate (controller-run after Task 1 — NOT a subagent task)

This is the decisive measurement and the pre-committed NO-GO bar. Run it directly, in order. Build the CLI + the closure-diff harness correctly first (`cargo build --release -p owl-dl-cli -p owl-dl-bench` — note each crate needs its own `-p`, a malformed `-p a b` silently leaves a stale binary).

1. **Wine FIRST — soundness.** Run the `konclude_closure_diff` on `wine` with `RUSTDL_NOMKEY_DIFF=1`. Require **FP=0 / MISSED=0, byte-identical (653=653, unsat:0)**. A single spurious subsumption or spurious-unsat class ⇒ **NO-GO: revert the rule, the increment is wrong as built** (record the failing classes for diagnosis).
2. **Full corpus — soundness.** Same closure-diff, `RUSTDL_NOMKEY_DIFF=1`, byte-identical across the oracled fixtures: sio (8904), galen (27997), notgalen (32739), ore-10908 (6001), ore-15672 (142), pizza, alehif, ro, shoiq-knowledge. Any FP ⇒ NO-GO.
3. **Then wall.** Measure wine classify wall + label-cache misses + per-class labeling on the 8 genuinely-hard classes (Burgundy, Chardonnay, Gamay, Meursault, PinotBlanc, Port, Tours, WhiteBurgundy), `RUSTDL_NOMKEY_DIFF=1` vs unset. Report whether any of the 8 now label/collapse and the net wall delta.

**Verdict rule (pre-committed):**
- FP=0 corpus-wide AND wall measurably improves (≥1 of the 8 collapses, or material miss reduction) ⇒ **GO**: write the results doc, then a follow-up to flip default-ON (controller's call).
- FP=0 but inert on the wall ⇒ the determinism gap is elsewhere (M1 absorption, or the dense wall): **document and stop; do NOT ship an inert default-on.** Decide M1 next.
- Any FP ⇒ **NO-GO: revert.** (Per det-pruning/marker-saturator/precise-merge, the corpus oracle is the only ground truth; do not rationalize a single FP.)

---

## Self-Review

**Spec coverage:** Spec's rule (DifferentIndividuals→NomKey disjoint, gated, lookup-only) → Task 1 Step 3. Spec's negatives-first canary (flag-on clash, no-DifferentIndividuals control, flag-off control) → Task 1 Step 1. Spec's gate (wine-first FP=0 → corpus FP=0 → wall; pre-committed verdict) → Gate section. Spec's default-OFF/byte-identical → flag pattern + Step 5. All covered.

**Placeholder scan:** No TBD/TODO; all code blocks complete; exact insertion line (2927) and file given; gate commands concrete.

**Type consistency:** `Axiom::DifferentIndividuals(Vec<IndividualId>)`, `tseitin.nominal_by_ind: HashMap<IndividualId, ClassId>`, `rules.disjoint_pairs: Vec<(ClassId, ClassId)>` — all match the code read at lib.rs:2046/2210 and ontology.rs:88. Test helpers (`parse_internal`/`saturate`/`class`/`is_unsatisfiable`/`HEADER`) match the existing test at lib.rs:4350.
