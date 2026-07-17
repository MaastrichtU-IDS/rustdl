# Anonymous Individuals Support — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let rustdl parse and reason over ontologies containing anonymous individuals (currently rejected at conversion), recovering the 23 % of ORE that is unreadable today, with the curated soundness net unchanged.

**Architecture:** Intern each anonymous individual as a first-class `IndividualId` under a reserved `urn:rustdl-anon:<label>` namespace — a single change in `convert_individual`, through which every axiom position routes, so anon support threads everywhere automatically. Interned anon individuals participate in SameAs/DifferentFrom/`≤n`-merge exactly as named ones (rustdl assumes no-UNA). They are reasoning-internal only: filtered from named-individual output surfaces by the reserved prefix.

**Tech Stack:** Rust (edition 2024), horned-owl (OWL functional-syntax parsing), the rustdl workspace (`owl-dl-core` conversion, `owl-dl-reasoner` classify/consistency/realize).

## Global Constraints

- Build/test with `RUSTUP_TOOLCHAIN=stable` (the pinned 1.95.0 toolchain often lacks `cargo`; a bare `cargo` silently reuses a stale binary). Prefix commands: `RUSTUP_TOOLCHAIN=stable cargo …`.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` must pass (pedantic on; warnings are errors). `cargo fmt --all -- --check` must pass (max_width 100).
- **FP=0 is the non-negotiable soundness contract.** No change may introduce a false-positive subsumption or a false inconsistency on any curated fixture.
- Reserved prefix, verbatim: `urn:rustdl-anon:` (mirrors the existing `urn:rustdl-dkey:` = `DKEY_IRI_PREFIX` at `crates/owl-dl-core/src/convert.rs:51`).
- OWL functional-syntax writes an anonymous individual as `_:label`; horned-owl parses it to `Individual::Anonymous(AnonymousIndividual(<label>))`.

---

### Task 1: Intern anonymous individuals in `convert_individual`

**Files:**
- Modify: `crates/owl-dl-core/src/convert.rs` (add `ANON_IRI_PREFIX` near line 51; change the `Individual::Anonymous` arm of `convert_individual`, ~line 1670; replace the anon-reject unit test, ~line 2667)

**Interfaces:**
- Consumes: `Vocabulary::intern_individual(&str) -> IndividualId` (dedupes by string); `Individual::Anonymous(AnonymousIndividual<A>)` where the label is `anon.0.as_ref(): &str`.
- Produces: `pub const ANON_IRI_PREFIX: &str = "urn:rustdl-anon:";`. `convert_individual` now returns `Ok(IndividualId)` for anonymous individuals, interned at `urn:rustdl-anon:<label>`.

- [ ] **Step 1: Write the failing test** — in the `#[cfg(test)] mod tests` of `convert.rs`, replacing the existing `AnonymousIndividual`-reject test (find it via `grep -n "AnonymousIndividual" crates/owl-dl-core/src/convert.rs` — the test around line 2667 that asserts `err == ConversionError::AnonymousIndividual`):

```rust
#[test]
fn anonymous_individual_is_interned_under_reserved_prefix() {
    use horned_owl::model::{AnonymousIndividual, Individual, RcStr};
    use std::rc::Rc;
    let mut vocab = Vocabulary::default();
    let a: Individual<RcStr> = Individual::Anonymous(AnonymousIndividual(Rc::from("blank-0")));
    let id_a = convert_individual(&a, &mut vocab).expect("anon individual interns");
    // same label → same id (blank-node identity within a document)
    let id_a2 = convert_individual(&a, &mut vocab).expect("anon individual interns");
    assert_eq!(id_a, id_a2, "same anon label must intern to the same IndividualId");
    // distinct label → distinct id
    let b: Individual<RcStr> = Individual::Anonymous(AnonymousIndividual(Rc::from("blank-1")));
    let id_b = convert_individual(&b, &mut vocab).expect("anon individual interns");
    assert_ne!(id_a, id_b, "distinct anon labels must intern to distinct IndividualIds");
    // interned under the reserved prefix
    assert!(vocab.individual_iri(id_a).starts_with(ANON_IRI_PREFIX));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-core anonymous_individual_is_interned -- --nocapture`
Expected: FAIL — currently `convert_individual` returns `Err(ConversionError::AnonymousIndividual)`, so `.expect("anon individual interns")` panics.

- [ ] **Step 3: Add the constant.** In `crates/owl-dl-core/src/convert.rs`, immediately after the `DKEY_IRI_PREFIX` definition (line 51):

```rust
/// Reserved IRI namespace for anonymous individuals interned during conversion.
/// Anonymous individuals are first-class `IndividualId`s under this prefix; they
/// participate in all ABox/identity reasoning but are filtered from named-individual
/// output surfaces (they have no real IRI). Cannot collide with an input individual IRI.
pub const ANON_IRI_PREFIX: &str = "urn:rustdl-anon:";
```

- [ ] **Step 4: Change the `convert_individual` anonymous arm** (~line 1670). Replace:

```rust
        Individual::Anonymous(_) => Err(ConversionError::AnonymousIndividual),
```

with:

```rust
        Individual::Anonymous(anon) => {
            let label: &str = anon.0.as_ref();
            let synthetic = format!("{ANON_IRI_PREFIX}{label}");
            Ok(vocab.intern_individual(&synthetic))
        }
```

Also update the doc comment on `convert_individual` (line ~1660) from "(named only — anonymous is rejected)" to "(named individuals by IRI; anonymous individuals interned under `ANON_IRI_PREFIX`)".

- [ ] **Step 5: Run test to verify it passes**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-core anonymous_individual_is_interned`
Expected: PASS.

- [ ] **Step 6: Remove the now-dead error variant if unreferenced.** Run `grep -rn "AnonymousIndividual" crates/`. If `ConversionError::AnonymousIndividual` is referenced only in its own `#[error(...)]` definition (line ~805) and nowhere else, delete the variant and its `#[error]` line. If anything else matches it, leave it. Run `RUSTUP_TOOLCHAIN=stable cargo build -p owl-dl-core` to confirm no unused-variant warning (warnings are errors).

- [ ] **Step 7: fmt + clippy + commit**

```bash
RUSTUP_TOOLCHAIN=stable cargo fmt --all
RUSTUP_TOOLCHAIN=stable cargo clippy -p owl-dl-core --all-targets -- -D warnings
git add crates/owl-dl-core/src/convert.rs
git commit -m "feat(core): intern anonymous individuals under urn:rustdl-anon: prefix"
```

---

### Task 2: Filter anonymous individuals from named-individual output surfaces

**Files:**
- Modify: `crates/owl-dl-reasoner/src/realize.rs` (`instances_of_internal` push site, ~line 288; and `instances_of_saturation_only_internal`)
- Modify: `crates/owl-dl-reasoner/src/lib.rs` (the three `materialize_*` output builders: `materialize_object_property_assertions` ~line 110, `materialize_data_property_assertions` ~line 138, `materialize_existential_successors` ~line 294 — each builds its result tuples here via `vocab.individual_iri(...)`)
- Test: `crates/owl-dl-reasoner/tests/anonymous_individuals_reporting.rs` (new)

**Interfaces:**
- Consumes: `owl_dl_core::convert::ANON_IRI_PREFIX` (from Task 1); `Vocabulary::individual_iri(IndividualId) -> &str`; `instances_of(&SetOntology, class_iri) -> Result<Vec<String>>`.
- Produces: no anonymous individual (IRI starting with `ANON_IRI_PREFIX`) appears in `instances_of` or any `materialize_*` output.

- [ ] **Step 1: Write the failing test** — create `crates/owl-dl-reasoner/tests/anonymous_individuals_reporting.rs`:

```rust
//! Anonymous individuals reason but are never reported on named-individual
//! output surfaces (decision (a) in the anon-individuals spec).
use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::instances_of;
use std::io::Cursor;

fn onto(body: &str) -> SetOntology<RcStr> {
    let src = format!("Prefix(:=<http://e#>)\nOntology(\n{body}\n)");
    let mut r = Cursor::new(src);
    read_ofn(&mut r, ParserConfiguration::default()).expect("parse ofn").0
}

#[test]
fn instances_of_excludes_anonymous_individuals() {
    // Named :a and anonymous _:x are both asserted to be :A.
    let o = onto(
        "Declaration(Class(:A)) Declaration(NamedIndividual(:a))\n\
         ClassAssertion(:A :a)\n\
         ClassAssertion(:A _:x)",
    );
    let members = instances_of(&o, "http://e#A").expect("instances_of");
    assert!(members.iter().any(|m| m == "http://e#a"), "named :a must be reported");
    assert!(
        members.iter().all(|m| !m.starts_with("urn:rustdl-anon:")),
        "anonymous individuals must NOT appear in instances_of output: {members:?}"
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test anonymous_individuals_reporting`
Expected: FAIL — `_:x` interns as `urn:rustdl-anon:x`, is an instance of `:A`, and is currently pushed into the `instances_of` result.

- [ ] **Step 3: Filter in `instances_of_internal`.** In `crates/owl-dl-reasoner/src/realize.rs`, in the loop (~line 285-289), guard the push:

```rust
        if instance_check_with_closure(internal, &closure, &prepared, class_id, individual_id)? {
            let iri = internal.vocabulary.individual_iri(individual_id);
            if !iri.starts_with(owl_dl_core::convert::ANON_IRI_PREFIX) {
                out.push(iri.to_owned());
            }
        }
```

Apply the identical guard to `instances_of_saturation_only_internal` (same file — find the analogous push loop).

- [ ] **Step 4: Filter in the three `materialize_*` builders (in `lib.rs`).** Each builds its output via a `.map(...).filter(...).collect()` (or a per-row push); add an `ANON_IRI_PREFIX` guard to that filter.

  **`materialize_object_property_assertions`** (~line 120): the current filter is `.filter(|(_, p, _)| p != TOP && p != BOT)`. Replace with:

```rust
        .filter(|(s, p, o)| {
            p != TOP
                && p != BOT
                && !s.starts_with(owl_dl_core::convert::ANON_IRI_PREFIX)
                && !o.starts_with(owl_dl_core::convert::ANON_IRI_PREFIX)
        })
```

  **`materialize_data_property_assertions`** (5-tuple `(subject, prop, lexical, datatype, lang)`; the object is a literal, so guard the **subject only**). This function's output-building is complex (union-find over individual IRIs), so filter robustly on the final result vector: immediately before the function's `Ok(<vec>)` return, add (using the vec's actual local name — e.g. `out`):

```rust
        out.retain(|(s, ..)| !s.starts_with(owl_dl_core::convert::ANON_IRI_PREFIX));
```

  **`materialize_existential_successors`** (4-tuple `(subject, property, witness_blank, filler_class)`; guard the **subject only** — `witness_blank` is a separate synthetic and is expected in output). Same robust approach — immediately before its `Ok(<vec>)` return:

```rust
        out.retain(|(s, ..)| !s.starts_with(owl_dl_core::convert::ANON_IRI_PREFIX));
```

- [ ] **Step 5: Add a materialize reporting test** — append to `anonymous_individuals_reporting.rs`:

```rust
#[test]
fn materialize_object_property_excludes_anonymous_subjects_and_objects() {
    use owl_dl_reasoner::materialize_object_property_assertions;
    let o = onto(
        "Declaration(ObjectProperty(:r)) Declaration(NamedIndividual(:a))\n\
         ObjectPropertyAssertion(:r :a _:x)\n\
         ObjectPropertyAssertion(:r _:x :a)",
    );
    let rows = materialize_object_property_assertions(&o).expect("materialize");
    for (s, _p, ob) in &rows {
        assert!(!s.starts_with("urn:rustdl-anon:"), "anon subject leaked: {s}");
        assert!(!ob.starts_with("urn:rustdl-anon:"), "anon object leaked: {ob}");
    }
}
```

(`materialize_object_property_assertions` returns `Result<Vec<(String, String, String)>, ReasonError>` = `(subject_iri, property_iri, object_iri)`, so the `(s, _p, ob)` destructuring above is exact.)

- [ ] **Step 6: Run tests to verify they pass**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test anonymous_individuals_reporting`
Expected: PASS (both tests).

- [ ] **Step 7: fmt + clippy + commit**

```bash
RUSTUP_TOOLCHAIN=stable cargo fmt --all
RUSTUP_TOOLCHAIN=stable cargo clippy -p owl-dl-reasoner --all-targets -- -D warnings
git add crates/owl-dl-reasoner/src/realize.rs crates/owl-dl-reasoner/src/lib.rs crates/owl-dl-reasoner/tests/anonymous_individuals_reporting.rs
git commit -m "feat(reasoner): filter anonymous individuals from named-individual output surfaces"
```

---

### Task 3: Anonymous-individual identity soundness fixtures

**Files:**
- Test: `crates/owl-dl-reasoner/tests/anonymous_individuals_identity.rs` (new)

**Interfaces:**
- Consumes: `is_consistent(&SetOntology) -> Result<bool>`; `instances_of(&SetOntology, &str) -> Result<Vec<String>>`; the `onto(body)` helper pattern from Task 2.
- Produces: end-to-end validation that interned anon individuals participate correctly in SameAs / DifferentFrom / functional-`≤1` merge / disjointness — the advisor-flagged identity soundness edge. Expected verdicts are hand-derived and MUST match a HermiT/Konclude oracle (see Task 4).

- [ ] **Step 1: Write the failing tests** — create `crates/owl-dl-reasoner/tests/anonymous_individuals_identity.rs`:

```rust
//! Soundness of anonymous-individual IDENTITY reasoning: interned anon
//! individuals participate in SameAs / DifferentFrom / functional-≤1 merge /
//! disjointness exactly as named individuals. Verdicts are oracle-adjudicated
//! (HermiT/Konclude); see the anon-individuals plan Task 4.
use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{instances_of, is_consistent};
use std::io::Cursor;

fn onto(body: &str) -> SetOntology<RcStr> {
    let src = format!("Prefix(:=<http://e#>)\nOntology(\n{body}\n)");
    let mut r = Cursor::new(src);
    read_ofn(&mut r, ParserConfiguration::default()).expect("parse ofn").0
}

// A) anon instance of two disjoint classes ⇒ inconsistent.
#[test]
fn anon_in_two_disjoint_classes_is_inconsistent() {
    let o = onto(
        "Declaration(Class(:A)) Declaration(Class(:B)) DisjointClasses(:A :B)\n\
         ClassAssertion(:A _:x) ClassAssertion(:B _:x)",
    );
    assert!(!is_consistent(&o).expect("consistency"), "A⊓B on _:x must be inconsistent");
}

// B) functional r + two anon witnesses of a's r, one A one B (A,B disjoint) ⇒
//    merge forces A⊓B ⇒ inconsistent.
#[test]
fn functional_merges_anon_witnesses_into_clash() {
    let o = onto(
        "Declaration(Class(:A)) Declaration(Class(:B)) DisjointClasses(:A :B)\n\
         Declaration(ObjectProperty(:r)) FunctionalObjectProperty(:r)\n\
         Declaration(NamedIndividual(:a))\n\
         ObjectPropertyAssertion(:r :a _:x) ObjectPropertyAssertion(:r :a _:y)\n\
         ClassAssertion(:A _:x) ClassAssertion(:B _:y)",
    );
    assert!(!is_consistent(&o).expect("consistency"), "functional merge of _:x,_:y into A⊓B must clash");
}

// C) functional r + two anon witnesses asserted DifferentIndividuals ⇒ ≤1 clash.
#[test]
fn functional_plus_different_anon_witnesses_is_inconsistent() {
    let o = onto(
        "Declaration(ObjectProperty(:r)) FunctionalObjectProperty(:r)\n\
         Declaration(NamedIndividual(:a))\n\
         ObjectPropertyAssertion(:r :a _:x) ObjectPropertyAssertion(:r :a _:y)\n\
         DifferentIndividuals(_:x _:y)",
    );
    assert!(!is_consistent(&o).expect("consistency"), "functional + ≠ anon witnesses must clash");
}

// D) control: the same as C without DifferentIndividuals ⇒ consistent (they merge).
#[test]
fn functional_anon_witnesses_without_diff_is_consistent() {
    let o = onto(
        "Declaration(ObjectProperty(:r)) FunctionalObjectProperty(:r)\n\
         Declaration(NamedIndividual(:a))\n\
         ObjectPropertyAssertion(:r :a _:x) ObjectPropertyAssertion(:r :a _:y)",
    );
    assert!(is_consistent(&o).expect("consistency"), "functional anon witnesses without ≠ must merge (consistent)");
}

// E) SameIndividual(:a, _:x): _:x∈A propagates to the NAMED :a ⇒ :a reported ∈ A.
#[test]
fn sameas_from_anon_propagates_to_named() {
    let o = onto(
        "Declaration(Class(:A)) Declaration(NamedIndividual(:a))\n\
         ClassAssertion(:A _:x) SameIndividual(:a _:x)",
    );
    let members = instances_of(&o, "http://e#A").expect("instances_of");
    assert!(members.iter().any(|m| m == "http://e#a"), "SameAs(:a,_:x) must make :a ∈ A: {members:?}");
    assert!(members.iter().all(|m| !m.starts_with("urn:rustdl-anon:")), "anon still filtered");
}
```

- [ ] **Step 2: Run to verify they compile and reveal current behaviour**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test anonymous_individuals_identity`
Expected: with Task 1+2 already merged, tests A–E should PASS (the identity machinery is IndividualId-based and inherited). If any FAILS, that is a genuine threading gap — do NOT weaken the test; instead fix the reasoning path (most likely a spot that assumed named-only individuals) and re-run. Record any such fix in the commit message.

- [ ] **Step 3: Commit**

```bash
git add crates/owl-dl-reasoner/tests/anonymous_individuals_identity.rs
git commit -m "test(reasoner): anonymous-individual identity soundness fixtures (SameAs/≠/functional-merge/disjoint)"
```

---

### Task 4: Acceptance — curated non-regression + ORE coverage

**Files:**
- None (verification task). Optionally add a short note to `docs/2026-07-16-ore-sweep.md` or a new results file recording the ERR1 drop.

**Interfaces:**
- Consumes: the full workspace test suite; the ORE `pool_sample` corpus at `/data/dumontier/ore-run/pool_sample/files`; the fresh `target/release/rustdl` binary.

- [ ] **Step 1: Curated non-regression — full workspace tests pass**

Run: `RUSTUP_TOOLCHAIN=stable cargo test --workspace`
Expected: all green (no new failures vs the pre-change baseline). The curated closure-diff / oracle tests (`konclude_closure_diff.rs`, `completeness_contract.rs`, `materialize_oracle.rs`, etc.) must still pass — anon-free ontologies never reach the changed arm, so their results are unchanged.

- [ ] **Step 2: Build a fresh release binary**

Run: `RUSTUP_TOOLCHAIN=stable cargo build --release -p owl-dl-cli`
Expected: builds; confirm `target/release/rustdl` mtime is fresh (stale-binary trap).

- [ ] **Step 3: ORE coverage — the anon-error subset no longer errors.** Re-run the ORE sweep's ERR1 subset and confirm the count collapses. Using the sweep TSV from `bench-results/ore-perf-sweep-20260716.tsv` (rows with status `ERR1`):

```bash
POOL=/data/dumontier/ore-run/pool_sample/files
# sample 30 previously-anon-erroring onts; each should now return a verdict, not the anon error
for ont in $(awk -F'\t' '$4=="ERR1"{print $1}' bench-results/ore-perf-sweep-20260716.tsv | head -30); do
  out=$(timeout -s KILL 60 ./target/release/rustdl classify "$POOL/$ont" 2>&1 | tail -1)
  echo "$ont :: $out"
done | tee /tmp/anon-coverage-check.txt
grep -c "anonymous individuals are not supported" /tmp/anon-coverage-check.txt
```

Expected: the `grep -c` for the old error message is **0** — none of the sampled ontologies still reject on anonymous individuals. (Some may now DNF or hit a *different* unsupported construct; that is acceptable — this feature's success is the removal of the anon rejection, not finishing every ont.)

- [ ] **Step 4: Record the result + commit**

Append a short "Anonymous individuals (D1) — shipped" note to `docs/2026-07-16-ore-sweep.md` (or a new `docs/2026-07-17-anon-individuals-results.md`) with the pre/post ERR1 count (446 → measured residual) and the coverage-check output summary. Commit:

```bash
git add docs/
git commit -m "docs: anonymous-individuals coverage result (ORE ERR1 446 -> residual)"
```

---

## Notes for the implementer

- The whole feature is deliberately small: one interning arm (Task 1), one filter applied at a handful of output sites (Task 2), and soundness fixtures (Task 3) that should pass *without* further code changes if the threading is truly automatic — Task 3 failing means a real named-only assumption to fix, not a test to weaken.
- If Task 3 exposes a reasoning path that assumed real IRIs on individuals (e.g. a `has_abox_axioms` gate or an ABox-check site), fix it there and add a one-line comment referencing this plan.
- Do not add anon-individual *reporting* (blank-node output) — that is explicitly out of scope (decision (a) in the spec).
