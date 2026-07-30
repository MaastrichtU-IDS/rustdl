# Reduced-input `abox_check` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stop the classify fast path from building a full `PreparedOntology` — `HyperCache`, NNF, absorb, `ConsistencyCache` — solely to read `abox_verdict()` and then discard it.

**Architecture:** Extract the eight fields `abox_check::check` actually reads into an owned `AboxCheckInputs` struct built by a new function, and have `from_internal` use that same function so the two can never diverge. Then change the fast-path call sites only. The genuine classify builds are untouched: they need the full object anyway, and `abox_verdict()` is already lazy, so there is nothing to save there.

> **CORRECTION, applied during execution (2026-07-30).** Wherever this plan says "the fast-path
> call site" singular, or cites `classify.rs:785-798` as the only one, read **two sites**. There is
> a second, structurally identical fast-path block inside `classify_top_down_internal` that also
> ends in `return Ok(classify_pure_el(...))` and was wasting the same build. I mistook it for the
> top-down path because it sits directly above it. Task 3's implementer found and converted both,
> correctly. The two *genuine* `from_internal` calls (one per function, used to classify) remain
> untouched. Identify these sites **by function**, not by line number — the line numbers drifted
> under this branch's own edits, which is exactly how the mis-attribution happened. The
> authoritative map is in the spec's "§ The waste".

**Tech Stack:** Rust (edition 2024), `cargo test` / `clippy` / `fmt`.

**Spec:** `docs/superpowers/specs/2026-07-30-abox-check-reduced-input-design.md`

## Global Constraints

- **Toolchain.** `cargo` is NOT on `PATH`, and `rust-toolchain.toml` pins a `cargo`-less 1.95.0. Every cargo command must be prefixed with
  `export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"` and run as `RUSTUP_TOOLCHAIN=stable cargo …`.
- **Warnings are errors.** CI sets `RUSTFLAGS: -D warnings`; clippy `pedantic` is workspace-wide; `unwrap_used`/`dbg_macro` are warn-level. Test files here open with `#![allow(clippy::unwrap_used)]`.
- **Formatting.** `cargo fmt --all -- --check`; `max_width = 100`.
- **The `pyo3` crate does not build in this sandbox** (missing Python dev headers) — pre-existing. Use `--exclude owl-dl-py` for workspace runs.
- **This change must be verdict-identical.** It passes `abox_check` the same eight values it reads today. Any verdict change is a bug, not a trade-off.
- **CI does not run the FP=0 net** (`closure-diff soundness net` is a `workflow_dispatch` stub with unprovisioned fixtures). `./scripts/run-soundness-diff.sh` locally is the only FP=0 evidence this change will get. It takes ~4 min.
- Do not change what `abox_check` decides. P1–P9 are untouched.

---

## File Structure

| file | responsibility |
|---|---|
| `crates/owl-dl-reasoner/src/abox_check.rs` | `check` takes `&AboxCheckInputs` instead of `&PreparedOntology`; the `AboxCheckInputs` type lives here, next to its only consumer |
| `crates/owl-dl-reasoner/src/lib.rs` | new `build_abox_check_inputs`; `from_internal` and `abox_verdict` use it |
| `crates/owl-dl-reasoner/src/classify.rs` | fast-path call site (`~785-798`) builds inputs directly instead of a full `PreparedOntology` |
| `crates/owl-dl-reasoner/tests/abox_check_reduced_input.rs` | **new** — verdict identity + fast-path inertness canaries |

---

### Task 1: Prove verdict identity is achievable before refactoring anything

The whole change rests on "the same eight values give the same verdict". `collect_abox` interns a
nominal concept per individual into the pool, so building the ABox *before* `absorb` /
`precompute_max_complements` assigns different concept ids than building it after. Class ids are
unchanged, so `closure`/`told` lookups are unaffected — but that is an argument, not a measurement.
Measure it first; if it fails, the rest of the plan is void.

**Files:**
- Test: `crates/owl-dl-reasoner/tests/abox_check_reduced_input.rs` (create)

**Interfaces:**
- Consumes: `owl_dl_reasoner::classify` (public), `owl_dl_core::convert::convert_ontology`.
- Produces: nothing consumed later (test-only).

- [ ] **Step 1: Write a differential canary over ABox fixtures**

```rust
//! Canaries for the reduced-input `abox_check` change.
//!
//! The change passes `abox_check` the same eight values it reads today, so every
//! verdict must be identical. These tests pin that from the outside, via the
//! classification the verdict drives: an ABox-inconsistent ontology classifies
//! every class unsatisfiable; a consistent one does not.
//!
//! Run: `cargo test -p owl-dl-reasoner --test abox_check_reduced_input`

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

const PFX: &str = "Prefix(:=<http://t/>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n";

fn parse(body: &str) -> SetOntology<RcStr> {
    let src = format!("{PFX}Ontology(<http://t/x>\n{body}\n)\n");
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    onto
}

fn unsat_count(body: &str) -> usize {
    let c = owl_dl_reasoner::classify(&parse(body)).expect("classify");
    c.unsatisfiable_classes().len()
}

/// P2-shaped ABox clash (disjoint types on one individual) on an ontology that
/// takes the FAST path — the only path this change touches. Every class must be
/// reported unsatisfiable, which is `classify_inconsistent`'s behaviour and is
/// reachable only if the ABox verdict still fires.
#[test]
fn fastpath_abox_clash_still_marks_all_classes_unsat() {
    let body = "    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    Declaration(NamedIndividual(:i))
    DisjointClasses(:A :B)
    ClassAssertion(:A :i)
    ClassAssertion(:B :i)
    SubClassOf(:C :A)";
    assert!(
        unsat_count(body) >= 3,
        "an ABox clash must make every class unsatisfiable on the fast path"
    );
}

/// NEGATIVE control: a consistent ABox on the fast path must NOT mark anything
/// unsatisfiable. Guards against the change turning the verdict into a blanket
/// `Inconsistent`.
#[test]
fn fastpath_consistent_abox_marks_nothing_unsat() {
    let body = "    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(NamedIndividual(:i))
    ClassAssertion(:A :i)
    SubClassOf(:A :B)";
    assert_eq!(
        unsat_count(body), 0,
        "a consistent ABox must not make any class unsatisfiable"
    );
}

/// ABox-free fast-path ontology: entirely inert — `has_abox_axioms` short-circuits
/// before any of this code runs.
#[test]
fn fastpath_no_abox_is_inert() {
    let body = "    Declaration(Class(:A))
    Declaration(Class(:B))
    SubClassOf(:A :B)";
    assert_eq!(unsat_count(body), 0);
}
```

- [ ] **Step 2: Run them against unmodified code — all three must PASS**

```bash
export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"
RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test abox_check_reduced_input
```

Expected: 3 passed. These are **invariants**, not red tests — they must hold before and after.
If `fastpath_abox_clash_still_marks_all_classes_unsat` fails on unmodified code, the fixture is
not reaching the fast path or not reaching `abox_check`; fix the fixture before continuing (add a
`SubClassOf` to keep it in EL, and confirm with
`./target/release/rustdl classify <file>` showing `# mode: pure EL`).

- [ ] **Step 3: Record the baseline for the measurement gate**

```bash
RUSTUP_TOOLCHAIN=stable cargo build --release -p owl-dl-cli
P=/data/dumontier/ore-run/pool_sample/files
for n in 1043 1115 10965 11110; do
  printf "%s " $n
  /usr/bin/time -f "%es %MkB" env RAYON_NUM_THREADS=1 \
    ./target/release/rustdl classify $P/ore_ont_$n.owl >/dev/null
done
```

Expected order of magnitude (from the spec): `1043` ~2.4 s, `1115` ~0.8 s, `10965` ~1.2 s,
`11110` ~4.5 s. Save the numbers; Task 4 compares against them.

- [ ] **Step 4: Commit the invariant canaries**

```bash
git add crates/owl-dl-reasoner/tests/abox_check_reduced_input.rs
git commit -m "test(abox-check): fast-path verdict invariants before the reduced-input change

Three canaries that must hold before AND after: a P2-shaped ABox clash on a
fast-path ontology marks every class unsatisfiable; a consistent ABox marks
nothing; an ABox-free ontology is inert. They pin the verdict from the outside,
via the classification it drives."
```

---

### Task 2: Extract `AboxCheckInputs` and route `abox_check::check` through it

Pure refactor — **no behaviour change**. Doing the extraction separately from the fast-path change
isolates the risky part: after this task, byte-identity everywhere is expected, so any diff is
this task's bug.

**Files:**
- Modify: `crates/owl-dl-reasoner/src/abox_check.rs` (the `check` signature at `:137`, and every `prepared.` access in the body)
- Modify: `crates/owl-dl-reasoner/src/lib.rs` (`abox_verdict` at `~4692`)

**Interfaces:**
- Consumes: `PreparedOntology`'s fields `abox`, `axioms`, `told`, `pool`, `inverse_pairs`, `hierarchy`, `disjoint_role_pairs`, `closure`.
- Produces: `pub(crate) struct AboxCheckInputs<'a>` (borrowed form) and
  `pub(crate) fn check(inputs: &AboxCheckInputs<'_>) -> AboxVerdict`, used by Task 3.

- [ ] **Step 1: Add the borrowed input type in `abox_check.rs`**

Place it immediately above `check`. Field names match `PreparedOntology`'s so the mechanical
rename in step 2 is a straight substitution.

```rust
/// Exactly the values [`check`] reads. Extracted so the dependency set is
/// compiler-checked: `abox_check` must never grow a dependency on the expensive
/// parts of `PreparedOntology` (`hyper`, `tbox`) without this struct changing,
/// which is what lets the classify fast path skip building them. See
/// `docs/superpowers/specs/2026-07-30-abox-check-reduced-input-design.md`.
pub(crate) struct AboxCheckInputs<'a> {
    pub(crate) abox: &'a crate::Abox,
    pub(crate) axioms: &'a [owl_dl_core::ontology::Axiom],
    pub(crate) told: &'a owl_dl_core::told::ToldTables,
    pub(crate) pool: &'a owl_dl_core::ir::ConceptPool,
    pub(crate) inverse_pairs: &'a [(owl_dl_core::ir::Role, owl_dl_core::ir::Role)],
    pub(crate) hierarchy: &'a crate::RoleHierarchy,
    pub(crate) disjoint_role_pairs: &'a [(owl_dl_core::ir::Role, owl_dl_core::ir::Role)],
    pub(crate) closure: &'a owl_dl_saturation::SaturationResult,
}
```

The exact type paths must match the field declarations on `PreparedOntology` (around
`lib.rs:4270-4300`). Read them and copy; do not guess. If a field is a `Vec<T>`, borrow it as
`&'a [T]`.

- [ ] **Step 2: Change `check`'s signature and mechanically rename accesses**

```rust
pub(crate) fn check(inputs: &AboxCheckInputs<'_>) -> AboxVerdict {
```

Then replace every `prepared.` in the body with `inputs.` — there are 30 occurrences across the
eight fields (`abox` ×20, `axioms` ×4, and one each of `told`, `pool`, `inverse_pairs`,
`hierarchy`, `disjoint_role_pairs`, `closure`). Where the body took `&prepared.x`, the field is
already a reference, so drop the `&` (e.g. `let told = &prepared.told;` becomes
`let told = inputs.told;`).

- [ ] **Step 3: Build the inputs inside `abox_verdict`**

In `lib.rs`, `abox_verdict` (`~4692`) currently calls `abox_check::check(self)`. Change to:

```rust
    pub(crate) fn abox_verdict(&self) -> &abox_check::AboxVerdict {
        self.abox_verdict.get_or_init(|| {
            if crate::abox_check_enabled() {
                abox_check::check(&abox_check::AboxCheckInputs {
                    abox: &self.abox,
                    axioms: &self.axioms,
                    told: &self.told,
                    pool: &self.pool,
                    inverse_pairs: &self.inverse_pairs,
                    hierarchy: &self.hierarchy,
                    disjoint_role_pairs: &self.disjoint_role_pairs,
                    closure: &self.closure,
                })
            } else {
                abox_check::AboxVerdict::Unknown
            }
```

- [ ] **Step 4: Build and run the full reasoner suite**

```bash
export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"
RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner
RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test abox_check_reduced_input
```

Expected: all groups 0 failed; the 3 Task-1 canaries still pass. This task changes no behaviour,
so **any** failure is a mechanical-rename error — most likely a dropped or doubled `&`.

- [ ] **Step 5: fmt, clippy, commit**

```bash
RUSTUP_TOOLCHAIN=stable cargo fmt --all -- --check
RUSTUP_TOOLCHAIN=stable cargo clippy -p owl-dl-reasoner --all-targets --all-features -- -D warnings
git add crates/owl-dl-reasoner/src/abox_check.rs crates/owl-dl-reasoner/src/lib.rs
git commit -m "refactor(abox-check): take AboxCheckInputs instead of &PreparedOntology

Pure refactor, no behaviour change. Makes abox_check's dependency set
compiler-checked: it reads exactly eight fields and never hyper or tbox, which is
what lets the classify fast path skip building those. Verified by the unchanged
reasoner suite."
```

---

### Task 3: Build the inputs directly on the classify fast path

**Files:**
- Modify: `crates/owl-dl-reasoner/src/lib.rs` (new `build_abox_check_inputs`)
- Modify: `crates/owl-dl-reasoner/src/classify.rs:785-798` (the fast-path call site)

**Interfaces:**
- Consumes: `abox_check::AboxCheckInputs` and `abox_check::check` from Task 2.
- Produces: `pub(crate) fn build_abox_check_inputs(internal: &InternalOntology, closure: &SaturationResult) -> OwnedAboxCheckInputs` — an owned holder the caller borrows from.

- [ ] **Step 1: Add an owned holder plus its builder in `lib.rs`**

The borrowed `AboxCheckInputs<'a>` needs its values to live somewhere. Add next to
`PreparedOntology`:

```rust
/// Owned backing store for [`abox_check::AboxCheckInputs`], for callers that need
/// only the inconsistency verdict and not a full [`PreparedOntology`]. Built by
/// [`build_abox_check_inputs`]; borrow with [`Self::as_inputs`].
///
/// This exists so the classify fast path stops building `HyperCache`, NNF, absorb
/// and `ConsistencyCache` solely to read `abox_verdict()` and then discard them —
/// measured at 0.62 s / 185 MB on `ore_ont_1043`.
pub(crate) struct OwnedAboxCheckInputs {
    pool: owl_dl_core::ir::ConceptPool,
    abox: Abox,
    axioms: Vec<owl_dl_core::ontology::Axiom>,
    told: owl_dl_core::told::ToldTables,
    hierarchy: RoleHierarchy,
    inverse_pairs: Vec<(owl_dl_core::ir::Role, owl_dl_core::ir::Role)>,
    disjoint_role_pairs: Vec<(owl_dl_core::ir::Role, owl_dl_core::ir::Role)>,
}

impl OwnedAboxCheckInputs {
    pub(crate) fn as_inputs<'a>(
        &'a self,
        closure: &'a owl_dl_saturation::SaturationResult,
    ) -> abox_check::AboxCheckInputs<'a> {
        abox_check::AboxCheckInputs {
            abox: &self.abox,
            axioms: &self.axioms,
            told: &self.told,
            pool: &self.pool,
            inverse_pairs: &self.inverse_pairs,
            hierarchy: &self.hierarchy,
            disjoint_role_pairs: &self.disjoint_role_pairs,
            closure,
        }
    }
}

/// Build only what [`abox_check::check`] reads. Mirrors the corresponding prefix of
/// [`PreparedOntology::from_internal`] — `expand_role_characteristics`, the role-side
/// collectors, `build_told_tables`, `collect_abox` — and deliberately omits
/// `nnf_axioms`, `absorb`, `precompute_max_complements`, `HyperCache::build`,
/// `ConsistencyCache::build` and `snapshot_cache`, none of which `check` reads.
///
/// `collect_abox` only reads `internal.axioms` and interns one nominal concept per
/// individual, so running it before `absorb` yields different *individual* concept
/// ids but identical *class* ids — and `check` compares ids only within one input
/// set, so the verdict is unchanged. Task 1's canaries pin that.
pub(crate) fn build_abox_check_inputs(internal: &InternalOntology) -> OwnedAboxCheckInputs {
    let mut internal = internal.clone();
    let told = owl_dl_core::told::build_told_tables(&internal);
    let axioms = internal.axioms.clone();
    expand_role_characteristics(&mut internal);
    let hierarchy = build_role_hierarchy(&internal);
    let inverse_pairs = collect_inverse_pairs(&internal);
    let disjoint_role_pairs = collect_disjoint_role_pairs(&internal);
    let abox = collect_abox(&mut internal);
    OwnedAboxCheckInputs {
        pool: internal.concepts,
        abox,
        axioms,
        told,
        hierarchy,
        inverse_pairs,
        disjoint_role_pairs,
    }
}
```

Note `build_told_tables` and the `axioms` clone happen **before** `expand_role_characteristics`,
matching `from_internal`'s order (`lib.rs:4567-4568` precede `:4619`). Preserving that order is
what makes the values identical.

- [ ] **Step 2: Rewrite the fast-path call site**

`classify.rs`, currently:

```rust
        if crate::abox_check_enabled() && has_abox_axioms(internal) {
            let prepared = PreparedOntology::from_internal(internal.clone())?;
            if let crate::abox_check::AboxVerdict::Inconsistent { reason } = prepared.abox_verdict()
            {
```

becomes:

```rust
        if crate::abox_check_enabled() && has_abox_axioms(internal) {
            // Build ONLY what abox_check reads, reusing the closure the caller
            // already computed — the full PreparedOntology built here previously
            // was discarded immediately (this branch either returns
            // classify_inconsistent or falls through to classify_pure_el, and
            // neither uses it). Measured 0.62 s / 185 MB on ore_ont_1043.
            let owned = crate::build_abox_check_inputs(internal);
            let verdict = crate::abox_check::check(&owned.as_inputs(&closure));
            if let crate::abox_check::AboxVerdict::Inconsistent { reason } = &verdict {
```

`closure` is already in scope here — it is passed to `classify_pure_el` on the next line — so this
also removes the second full EL saturation that `from_internal` performed.

Adjust the `reason` binding to the reference form (`&verdict`) so the `eprintln!` still compiles.

- [ ] **Step 3: Build and run the suites**

```bash
export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"
RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test abox_check_reduced_input
RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner
RUSTUP_TOOLCHAIN=stable cargo test --workspace --exclude owl-dl-py
```

Expected: all 0 failed, and the 3 canaries still pass. If
`fastpath_abox_clash_still_marks_all_classes_unsat` now fails, the verdict changed — that is the
id-ordering risk in the doc comment, and it means the ABox must be collected after `absorb`
after all. Report it rather than working around it.

- [ ] **Step 4: fmt, clippy, commit**

```bash
RUSTUP_TOOLCHAIN=stable cargo fmt --all -- --check
RUSTUP_TOOLCHAIN=stable cargo clippy --workspace --exclude owl-dl-py --all-targets --all-features -- -D warnings
git add crates/owl-dl-reasoner/src/lib.rs crates/owl-dl-reasoner/src/classify.rs
git commit -m "perf(classify): build only abox_check's inputs on the fast path

The fast path built a full PreparedOntology (HyperCache + NNF + absorb +
ConsistencyCache) solely to read abox_verdict(), then discarded it — it either
returns classify_inconsistent or falls through to classify_pure_el, neither of
which uses the object. Now builds only the eight values abox_check reads and
reuses the caller's closure, removing a second full EL saturation too.

Verdict-identical by construction: same eight inputs, same verdict. The top-down
path is untouched — it needs the full object anyway and abox_verdict() is lazy,
so there was never anything to save there."
```

---

### Task 4: Gates — soundness net, byte-identity, and per-ontology measurement

**Files:** none (measurement); results recorded in the spec.

- [ ] **Step 1: FP=0 net — mandatory, and the only FP=0 evidence this change gets**

```bash
export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"
./scripts/run-soundness-diff.sh 2>&1 | grep -E '^\[fp0\]|test result:'
```

Expected: `16 passed; 0 failed`, and the `[fp0]` manifest showing VERIFIED with these closures —
galen 27997, notgalen 32739, sio 8904, ore-10908 6001, wine 653, pizza 499, alehif 247, ro 158,
ore-15672 142, sulo 51, bibtex 16. Any deviation is a stop-and-diagnose.

- [ ] **Step 2: Byte-identity on ABox-bearing fixtures**

```bash
S=/tmp/abox-gate; mkdir -p $S
for f in alehif-test:external wine:real family:real pizza:real ro:real; do
  n=${f%%:*}; d=${f##*:}
  ./target/release/rustdl classify ontologies/$d/$n.ofn 2>/dev/null | grep -v '^#' | sort > $S/$n.after
done
```

Compare each against the same command on the pre-change build (`git stash` or a second checkout).
Expected: **byte-identical for all five.** These all exercise the changed path (ABox-bearing), and
unlike the retired ABox-filter lever there is no nominal-free precondition, so nominal-bearing
fixtures like `wine`/`family`/`pizza` count too.

- [ ] **Step 3: `realize` / `is_consistent` unchanged**

```bash
for n in wine family pizza; do
  ./target/release/rustdl realize --json ontologies/real/$n.ofn > /tmp/abox-gate/$n.realize.after 2>/dev/null
  ./target/release/rustdl consistent ontologies/real/$n.ofn > /tmp/abox-gate/$n.cons.after 2>/dev/null
done
```

Expected: byte-identical to pre-change. These consumers still use `PreparedOntology` unchanged, so
this should hold by construction; the test exists to pin it.

- [ ] **Step 4: Per-ontology recovery, reported individually**

```bash
P=/data/dumontier/ore-run/pool_sample/files
for n in 1043 1115 10965 11110 10068 10073; do
  printf "%s " $n
  /usr/bin/time -f "%es %MkB" env RAYON_NUM_THREADS=1 \
    ./target/release/rustdl classify $P/ore_ont_$n.owl >/dev/null
done
```

Compare against Task 1 Step 3's baseline. Expect a saving **less than** the
`RUSTDL_ABOX_CHECK=0` upper bound (that skips the check entirely; this still builds the eight
fields). Report the fraction of the bound achieved per ontology — do not quote the bound as the
result.

- [ ] **Step 5: Confirm the two known non-winners are unaffected**

```bash
for n in 10127 10838; do
  printf "%s " $n
  /usr/bin/time -f "%es" env RAYON_NUM_THREADS=1 \
    ./target/release/rustdl classify $P/ore_ont_$n.owl >/dev/null
done
```

Expected: unchanged (~19.1 s, ~4.6 s). Both take the **hybrid** path, which this change does not
touch. A change here means the fast-path branch is being entered when it should not be.

- [ ] **Step 6: Record results in the spec and commit**

Append a `## Measured results (YYYY-MM-DD)` section to
`docs/superpowers/specs/2026-07-30-abox-check-reduced-input-design.md` with: the FP=0 manifest
outcome, byte-identity results for the five fixtures plus realize/consistent, per-ontology
before/after wall and RSS for the six winners, and confirmation the two hybrid ontologies are
unchanged.

```bash
git add docs/superpowers/specs/2026-07-30-abox-check-reduced-input-design.md
git commit -m "docs: measured results for the reduced-input abox_check change"
```

---

## Self-Review

**1. Spec coverage.**

| spec requirement | task |
|---|---|
| `AboxCheckInputs` extraction | Task 2 |
| fast-path call site only; top-down untouched | Task 3 Step 2 |
| reuse the caller's closure | Task 3 Step 2 (`closure` already in scope) |
| `ConsistencyCache` off the fast path | Task 3 Step 1 — falls out, since the fast path stops calling `from_internal` |
| gate 1 verdict identity | Task 1 (invariants) + Task 3 Step 3 |
| gate 2 FP=0 net | Task 4 Step 1 |
| gate 3 byte-identity on ABox fixtures | Task 4 Step 2 |
| gate 4 realize/consistent unchanged | Task 4 Step 3 |
| gate 5 per-ontology recovery | Task 4 Steps 4–5 |
| no flag needed | Global Constraints + no flag anywhere in the plan |

Gate 6 (the 50k–180k band) was already run before this plan was written and is recorded in the
spec — no task needed.

**2. Placeholder scan.** The `YYYY-MM-DD` in Task 4 Step 6 is a date to fill at run time, and the
baseline numbers in Task 1 Step 3 are to be recorded, not invented. Every code step contains the
actual code. No "add error handling" or "similar to Task N".

**3. Type consistency.** `AboxCheckInputs<'a>` (borrowed, Task 2) and `OwnedAboxCheckInputs`
(owned, Task 3) are distinct names used consistently; `as_inputs(&closure)` is the only bridge.
`build_abox_check_inputs` takes `&InternalOntology` and returns `OwnedAboxCheckInputs` in both its
definition and its call site. `check(&AboxCheckInputs<'_>)` matches both call sites (`abox_verdict`
in Task 2, the fast path in Task 3).

Two steps deliberately begin by reading real declarations rather than trusting this plan's types
(Task 2 Step 1 on `PreparedOntology`'s field types, Task 3 Step 2 on the `reason` binding) — the
field types here were transcribed from a reading and a mismatch would surface as a compile error
rather than a silent bug, but confirming is cheaper than debugging.
