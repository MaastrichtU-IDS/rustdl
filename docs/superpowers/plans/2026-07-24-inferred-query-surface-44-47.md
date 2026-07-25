# Inferred Query Surface (issues #44–#47) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expose four families of inferred reasoner queries — property hierarchy (#44), property values (#45), same/different individuals (#46), and disjointness (#47) — on the reasoner API + Python + CLI `--json` surface, as sound (FP=0) queries.

**Architecture:** Each query = a structural sound lower bound (told/hierarchy closures, the abox-saturation fixpoint) optionally extended by budgeted *entailment* checks that reduce to (un)satisfiability/consistency probes on the existing sound engines, reusing one `PreparedOntology` snapshot. Every entailment is concluded **only** from an UNSAT/inconsistent verdict (never from a satisfying model). Surface wiring mirrors the existing `materialize_*` / `classify --json` precedents exactly.

**Tech Stack:** Rust (edition 2024, workspace), horned-owl (OWL object model), PyO3 (Python bindings), clap (CLI), serde_json (`--json`), HermiT/ROBOT offline oracle fixtures.

## Global Constraints

- **Build/test with `RUSTUP_TOOLCHAIN=stable cargo …`** (the pinned 1.95.0 toolchain often lacks `cargo`; a failed build silently reuses a stale binary). Always confirm a fresh `target/release/rustdl` before benchmarking.
- **Clippy pedantic is `-D warnings` workspace-wide**; `unwrap_used`/`dbg_macro` are warn-level (allowed only in `#[cfg(test)]`, gated by `#![allow(clippy::unwrap_used)]` at test-file top). `rustfmt` `max_width = 100`, run `cargo fmt --all`.
- **Soundness invariant (non-negotiable):** infer an entailment ONLY from an `unsatisfiable`/`inconsistent` verdict. Never read a satisfying model, a wedge `Sat`, or a cached completion to add a disjointness/sameness/difference/value. A `Sat` is a MISS at worst, never a false positive.
- **Inconsistent-KB guard (mandatory in every new reasoner query):** run `abox_saturation::saturate_abox_consistency(&internal)` first; on `.clash` return `Err(ReasonError::Inconsistent)` — exactly like `materialize_object_property_assertions` (`crates/owl-dl-reasoner/src/lib.rs:104-106`).
- **`schema_version`** of each NEW `--json` subcommand is `1`. Do NOT bump the existing `classify`/`consistent`/`realize` schema (`SCHEMA_VERSION` in `json_out.rs` stays `1`; new subcommands carry their own version so the existing plugin contract stays byte-stable).
- **`incomplete: bool`** on every result: set `true` if any budgeted probe timed out **or** the answer rests on a non-complete-by-construction verdict (trusted-`Sat` / outside the saturator-complete fragment). Structural-only queries (#44, disjoint-properties) report `incomplete: false` within their fragment.
- **One commit/PR per issue.** Phase 0 (shared infra) lands first in the #47 PR (its first consumer) or its own prep PR — see Phase 0 note.
- Object-vs-data property distinction is **lost** after `convert.rs` (both lower to object roles). Any query that must distinguish them walks the **horned-owl `SetOntology`** directly, as `materialize_subobjectproperty_axioms` does (`lib.rs:400-491`).

---

## File Structure

**Reasoner (`crates/owl-dl-reasoner/src/`)**
- `lib.rs` — new public query fns + result structs; `PreparedOntology` gains a `vocabulary` field + compound-probe/augment helpers; `fn decide` gains extra-fact threading.
- `abox_saturation.rs` — `SaturationResult` gains a `derived_same` field (functional-collapse merges) for the #46 seed.
- `property_classify.rs` *(new)* — `PropertyClassification` struct + `classify_object_property_hierarchy` / `classify_data_property_hierarchy` (#44).
- `disjointness.rs` *(new)* — `disjoint_classes` / `disjoint_object_properties` / `disjoint_data_properties` (#47).
- `individuals.rs` *(new)* — `same_individuals` / `different_individuals` (#46).
- `property_values.rs` *(new)* — `inferred_object_property_values` / `inferred_data_property_values` (#45).

**CLI (`crates/owl-dl-cli/src/`)**
- `main.rs` — new `Command` variants + dispatch arms.
- `json_out.rs` — new `#[derive(Serialize)]` result structs + `build_*_json` fns.
- `tests/json_output.rs` — end-to-end golden tests.
- `tests/fixtures/json/` — new fixtures.

**Python (`crates/owl-dl-py/`)**
- `src/queries.rs` — new `#[pyfunction]`s (queries module already exists and is registered).
- `python/rustdl/__init__.py` — `_native` import block + `__all__`.
- `python/rustdl/__init__.pyi` — stubs.

**Tests (`crates/owl-dl-reasoner/tests/`)**
- `disjoint_oracle.rs`, `individuals_oracle.rs`, `property_values_oracle.rs` *(new)* — HermiT-oracle diff tests + hand-authored canaries.
- `tests/fixtures/` — new `.ofn` inputs + committed `*-materialized.owx` oracles.

---

## Phase 0 — Shared infrastructure

> **Note:** Phase 0 has no user-facing deliverable; commit it as the first commits of the #47 PR (its first consumer). Tasks 0.1–0.3 are independently testable.

### Task 0.1: `derived_same` on `SaturationResult` (functional-collapse merges for #46 seed)

**Files:**
- Modify: `crates/owl-dl-reasoner/src/abox_saturation.rs` (struct ~82-107; Rule-7 functional loop ~893-910; result build ~1069-1071)
- Test: same file, `#[cfg(test)]` module.

**Interfaces:**
- Produces: `SaturationResult.derived_same: Vec<(IndividualId, IndividualId)>` — pairs `(b, c)` with `b < c` (by `.index()`) proven equal because a functional/inverse-functional role `R` has both `R(a,b)` and `R(a,c)` derived. Empty on clash.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)]` module in `abox_saturation.rs`:

```rust
#[test]
fn functional_role_forces_same_individuals() {
    // Functional(r); r(a,b); r(a,c)  ⟹  b = c.
    let src = r#"Prefix(:=<http://ex/#>)
      Ontology(<http://ex/>
        Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
        Declaration(NamedIndividual(:c)) Declaration(ObjectProperty(:r))
        FunctionalObjectProperty(:r)
        ObjectPropertyAssertion(:r :a :b)
        ObjectPropertyAssertion(:r :a :c))"#;
    let internal = parse_internal(src); // existing test helper in this module
    let res = saturate_abox_consistency(&internal);
    assert!(!res.clash);
    let iri = |i: IndividualId| internal.vocabulary.individual_iri(i).to_string();
    let pairs: Vec<(String, String)> =
        res.derived_same.iter().map(|&(x, y)| (iri(x), iri(y))).collect();
    assert!(
        pairs.contains(&("http://ex/#b".into(), "http://ex/#c".into()))
            || pairs.contains(&("http://ex/#c".into(), "http://ex/#b".into())),
        "expected b=c, got {pairs:?}"
    );
}
```

If no `parse_internal` helper exists in the module, add one mirroring the top of `crates/owl-dl-reasoner/tests/materialize_oracle.rs` (read_ofn → `convert_ontology`).

- [ ] **Step 2: Run test to verify it fails**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --lib functional_role_forces_same_individuals`
Expected: FAIL — `no field derived_same on SaturationResult`.

- [ ] **Step 3: Add the field + populate it**

In the struct (after `edges`, ~line 105):

```rust
    /// Pairs `(b, c)` (b.index() < c.index()) proven equal because a
    /// functional / inverse-functional role has both `R(a,b)` and `R(a,c)`
    /// derived. Sound under-approximation of entailed `SameIndividual`.
    /// Empty on clash.
    pub derived_same: Vec<(IndividualId, IndividualId)>,
```

Right after the fixpoint loop closes (~line 1020, before `result.edges` is built at 1069), add a pass over the same `functional` set + working `edges` set that Rule 7 uses (~893):

```rust
    let mut derived_same: Vec<(IndividualId, IndividualId)> = Vec::new();
    for &(func_rid, func_inv) in &functional {
        let mut fillers_by_subj: HashMap<IndividualId, Vec<IndividualId>> = HashMap::new();
        for &(rid, a, b) in &edges {
            if rid == func_rid {
                let (subj, filler) = if func_inv { (b, a) } else { (a, b) };
                fillers_by_subj.entry(subj).or_default().push(filler);
            }
        }
        for fillers in fillers_by_subj.values() {
            for i in 0..fillers.len() {
                for j in (i + 1)..fillers.len() {
                    let (mut x, mut y) = (fillers[i], fillers[j]);
                    if x == y {
                        continue;
                    }
                    if y.index() < x.index() {
                        std::mem::swap(&mut x, &mut y);
                    }
                    derived_same.push((x, y));
                }
            }
        }
    }
    derived_same.sort_unstable();
    derived_same.dedup();
```

Add `derived_same,` to the non-clash `SaturationResult { … }` literal (~1069) and `derived_same: Vec::new()` to the clash-path literal (search for the `clash: true` construction near the early clash returns).

- [ ] **Step 4: Run test to verify it passes**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --lib functional_role_forces_same_individuals`
Expected: PASS.

- [ ] **Step 5: Full crate test + clippy**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --lib && RUSTUP_TOOLCHAIN=stable cargo clippy -p owl-dl-reasoner --all-targets -- -D warnings`
Expected: PASS, no warnings. (Every other `SaturationResult { … }` construction site now needs the field — the compiler lists them; add `derived_same: Vec::new()`.)

- [ ] **Step 6: Commit**

```bash
git add crates/owl-dl-reasoner/src/abox_saturation.rs
git commit -m "feat(reasoner): derive functional-forced same-individual pairs in abox saturation"
```

### Task 0.2: `vocabulary` on `PreparedOntology` + compound-probe helper

**Files:**
- Modify: `crates/owl-dl-reasoner/src/lib.rs` (`PreparedOntology` struct ~4168; `from_internal` ~4455; add `impl` method)
- Test: `lib.rs` `#[cfg(test)]`.

**Interfaces:**
- Produces:
  - `PreparedOntology.vocabulary: owl_dl_core::vocab::Vocabulary` (`pub(crate)`).
  - `pub(crate) fn PreparedOntology::pair_disjoint_with_deadline(&self, a: ClassId, b: ClassId, deadline: Option<Instant>) -> Result<Option<bool>, ReasonError>` — `Some(true)` = `a ⊓ b` unsatisfiable (disjoint), `Some(false)` = satisfiable (not proven disjoint), `None` = timed out. Reuses `decide_with_deadline`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn pair_disjoint_detects_told_disjoint() {
    let internal = parse_internal_lib(
        r#"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(Class(:A)) Declaration(Class(:B))
            DisjointClasses(:A :B))"#,
    );
    let a = internal.vocabulary.class_id("http://ex/#A").unwrap();
    let b = internal.vocabulary.class_id("http://ex/#B").unwrap();
    let prepared = PreparedOntology::from_internal(internal).unwrap();
    assert_eq!(prepared.pair_disjoint_with_deadline(a, b, None).unwrap(), Some(true));
    // A vs A is satisfiable (A is not unsat here) ⇒ not disjoint.
    assert_eq!(prepared.pair_disjoint_with_deadline(a, a, None).unwrap(), Some(false));
}
```

Add a `parse_internal_lib` helper in the test module (read_ofn → `owl_dl_core::convert::convert_ontology`) if absent.

- [ ] **Step 2: Run to verify it fails**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --lib pair_disjoint_detects_told_disjoint`
Expected: FAIL — `no method pair_disjoint_with_deadline` / `no field vocabulary`.

- [ ] **Step 3: Add the field**

In `PreparedOntology` (after `pool`, ~4169):

```rust
    pub(crate) vocabulary: owl_dl_core::vocab::Vocabulary,
```

In `from_internal` (~4455), clone the vocabulary before `internal.concepts` is moved into `pool` (near the top, after the signature line):

```rust
        let vocabulary = internal.vocabulary.clone();
```

Add `vocabulary,` to the returned struct literal (~4558).

- [ ] **Step 4: Add the probe method**

In `impl PreparedOntology` (near `decide`, ~4675):

```rust
    /// `Some(true)` iff `a ⊓ b` is unsatisfiable (the two named classes are
    /// entailed disjoint); `Some(false)` if satisfiable; `None` on timeout.
    /// Sound: only unsat ⇒ disjoint (never a false positive).
    pub(crate) fn pair_disjoint_with_deadline(
        &self,
        a: owl_dl_core::ir::ClassId,
        b: owl_dl_core::ir::ClassId,
        deadline: Option<std::time::Instant>,
    ) -> Result<Option<bool>, ReasonError> {
        let sat = self.decide_with_deadline(deadline, |pool| {
            let ca = pool.atomic(a);
            let cb = pool.atomic(b);
            pool.and([ca, cb])
        })?;
        Ok(sat.map(|s| !s)) // unsat ⇒ disjoint
    }
```

(Confirm `decide_with_deadline` at ~4744 has signature `fn decide_with_deadline<F>(&self, deadline: Option<Instant>, build: F) -> Result<Option<bool>, ReasonError>`; if it takes the deadline in a different position, match it.)

- [ ] **Step 5: Run to verify it passes + clippy**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --lib pair_disjoint_detects_told_disjoint && RUSTUP_TOOLCHAIN=stable cargo clippy -p owl-dl-reasoner --all-targets -- -D warnings`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/owl-dl-reasoner/src/lib.rs
git commit -m "feat(reasoner): PreparedOntology carries vocabulary + pair-disjoint probe"
```

### Task 0.3: Augment-and-recheck plumbing in `fn decide` (for #46-same, #45)

**Files:**
- Modify: `crates/owl-dl-reasoner/src/lib.rs` (`fn decide` ~5282; seed loop ~5375-5414; `PreparedOntology::decide*` wrappers)
- Test: `lib.rs` `#[cfg(test)]`.

**Interfaces:**
- Produces: `pub(crate) fn PreparedOntology::consistent_with_extra(&self, extra_distinct: &[(IndividualId, IndividualId)], extra_neg_prop: &[(IndividualId, RoleId, IndividualId)], deadline: Option<Instant>) -> Result<Option<bool>, ReasonError>` — `Some(true)` = KB + extra facts consistent (tableau found a model), `Some(false)` = inconsistent (clash), `None` = timeout. Reuses the frozen snapshot; injects the extra facts into the per-probe tableau seed.

**Design:** The tableau reads `different_pairs` (→ `mark_distinct`) and `negative_property_assertions` (→ `∀role.¬{obj}` label), seeded in `fn decide` at `lib.rs:5375-5407`. We thread two extra slices into `fn decide` and seed them immediately after the existing `different_pairs` loop (before the `same_pairs` merges — the "mark before merges" ordering at 5361). The probe's test concept is `⊤` (`pool.top()`), so `decide` returns satisfiable-of-the-seeded-graph = consistency.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn consistent_with_extra_distinct_detects_forced_same() {
    // Functional(r); r(a,b); r(a,c) ⇒ b=c. Adding b≠c ⇒ inconsistent.
    let internal = parse_internal_lib(
        r#"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
            Declaration(NamedIndividual(:c)) Declaration(ObjectProperty(:r))
            FunctionalObjectProperty(:r)
            ObjectPropertyAssertion(:r :a :b) ObjectPropertyAssertion(:r :a :c))"#,
    );
    let b = internal.vocabulary.individual_id("http://ex/#b").unwrap();
    let c = internal.vocabulary.individual_id("http://ex/#c").unwrap();
    let prepared = PreparedOntology::from_internal(internal).unwrap();
    // Base KB is consistent…
    assert_eq!(prepared.consistent_with_extra(&[], &[], None).unwrap(), Some(true));
    // …but KB ∪ {b≠c} is inconsistent ⇒ b=c entailed.
    assert_eq!(prepared.consistent_with_extra(&[(b, c)], &[], None).unwrap(), Some(false));
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --lib consistent_with_extra_distinct_detects_forced_same`
Expected: FAIL — `no method consistent_with_extra`.

- [ ] **Step 3: Thread extra facts into `fn decide`**

Add two parameters to the free `fn decide` (~5282), after `abox`:

```rust
    extra_distinct: &[(IndividualId, IndividualId)],
    extra_neg_prop: &[(IndividualId, RoleId, IndividualId)],
```

In the seed loop, immediately after the existing `different_pairs` loop ends (~5381), add:

```rust
    for &(left, right) in extra_distinct {
        if let (Some(&nl), Some(&nr)) = (roots.get(&left), roots.get(&right)) {
            let nl = ctx.resolve(nl);
            let nr = ctx.resolve(nr);
            ctx.mark_distinct(nl, nr);
        }
    }
    for &(subj, role, obj) in extra_neg_prop {
        if let (Some(&ns), Some(_)) = (roots.get(&subj), roots.get(&obj)) {
            // Encode ¬R(subj,obj) as the label ∀role.¬{obj} on subj — the same
            // form collect_abox builds for NegativeObjectPropertyAssertion (lib.rs:4914).
            let nom = pool.nominal(obj);
            let neg = pool.not(nom);
            let all = pool.all(Role::Named(role), neg);
            let n = ctx.resolve(ns);
            ctx.add_label(n, all);
        }
    }
```

> Note: `pool` is the cloned per-probe pool inside `decide` (mutable). If the neg-prop encoding must be built before the tableau context borrows the pool, build the `ConceptId`s in the `build_test_concept` closure region — match the exact borrow structure of the surrounding code; the `extra_distinct` path (no pool mutation) is the one #46-same needs and is lower-risk. **Land `extra_distinct` first; defer `extra_neg_prop` wiring to Phase 4 (#45) where it's first consumed, if the borrow structure needs care.**

Update every call site of `fn decide` (the `PreparedOntology::decide*` wrappers ~4675/4708/4744/4772) to pass `&[]`, `&[]` for the two new slices. Add the public helper in `impl PreparedOntology`:

```rust
    pub(crate) fn consistent_with_extra(
        &self,
        extra_distinct: &[(IndividualId, IndividualId)],
        extra_neg_prop: &[(IndividualId, RoleId, IndividualId)],
        deadline: Option<std::time::Instant>,
    ) -> Result<Option<bool>, ReasonError> {
        decide(
            &self.pool, &self.tbox, &self.hierarchy, &self.inverse_pairs,
            &self.chain_axioms, &self.asymmetric_roles, &self.disjoint_role_pairs,
            &self.complements, &self.abox, extra_distinct, extra_neg_prop,
            &self.dkey_ranges, deadline, |pool| pool.top(),
        )
    }
```

(Match the exact argument order of `fn decide` after your edit.)

- [ ] **Step 4: Run to verify it passes + full crate test**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --lib && RUSTUP_TOOLCHAIN=stable cargo clippy -p owl-dl-reasoner --all-targets -- -D warnings`
Expected: PASS (all existing `decide` callers updated; no warnings).

- [ ] **Step 5: Commit**

```bash
git add crates/owl-dl-reasoner/src/lib.rs
git commit -m "feat(reasoner): snapshot-preserving augment-and-recheck (extra distinct/neg-prop) in decide"
```

---

## Phase 1 — Issue #47: disjointness (PR: `feat/47-disjointness`)

### Task 1.1: `disjoint_classes` reasoner fn (entailment-extended)

**Files:**
- Create: `crates/owl-dl-reasoner/src/disjointness.rs`
- Modify: `crates/owl-dl-reasoner/src/lib.rs` (add `mod disjointness; pub use disjointness::{...};`)
- Test: `crates/owl-dl-reasoner/tests/disjoint_oracle.rs` (Task 1.4)

**Interfaces:**
- Produces:
  ```rust
  pub struct Disjointness { pairs: Vec<(String, String)>, incomplete: bool }
  impl Disjointness { pub fn pairs(&self) -> &[(String, String)]; pub fn incomplete(&self) -> bool; }
  pub fn disjoint_classes<A: horned_owl::model::ForIRI>(onto: &SetOntology<A>, pair_deadline: Option<Duration>) -> Result<Disjointness, ReasonError>;
  ```
  `pairs` sorted, each `(c, d)` with `c < d`, over named satisfiable classes; excludes owl:Thing/Nothing, unsat classes, self-pairs. `incomplete` = a probe timed out OR classification was not complete-by-construction.

- [ ] **Step 1: Write the failing test** (in `crates/owl-dl-reasoner/tests/disjoint_oracle.rs`, new file)

```rust
#![allow(clippy::unwrap_used)]
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::io::ParserConfiguration;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;
use owl_dl_reasoner::disjoint_classes;

fn onto(src: &str) -> SetOntology<RcStr> {
    read_ofn(&mut Cursor::new(src.to_owned()), ParserConfiguration::default()).unwrap().0
}

#[test]
fn disjoint_classes_inherits_through_subclass() {
    // A,B told disjoint; C⊑A, D⊑B ⇒ C,D entailed disjoint (not told).
    let o = onto(
        r#"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(Class(:A)) Declaration(Class(:B))
            Declaration(Class(:C)) Declaration(Class(:D))
            DisjointClasses(:A :B) SubClassOf(:C :A) SubClassOf(:D :B))"#,
    );
    let r = disjoint_classes(&o, None).unwrap();
    let has = |x: &str, y: &str| {
        r.pairs().iter().any(|(a, b)| (a == x && b == y) || (a == y && b == x))
    };
    assert!(has("http://ex/#A", "http://ex/#B"), "told pair present");
    assert!(has("http://ex/#C", "http://ex/#D"), "inherited pair inferred: {:?}", r.pairs());
}

#[test]
fn disjoint_classes_errors_on_inconsistent() {
    let o = onto(
        r#"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(Class(:A)) Declaration(Class(:B)) Declaration(NamedIndividual(:x))
            DisjointClasses(:A :B) ClassAssertion(:A :x) ClassAssertion(:B :x))"#,
    );
    assert!(matches!(disjoint_classes(&o, None), Err(owl_dl_reasoner::ReasonError::Inconsistent)));
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test disjoint_oracle`
Expected: FAIL — `disjoint_classes` unresolved.

- [ ] **Step 3: Implement `disjointness.rs`**

```rust
//! Inferred disjointness queries (issue #47). Sound: a disjoint pair is
//! reported only when `C ⊓ D` is proven unsatisfiable (or told disjoint).
use crate::{PreparedOntology, ReasonError};
use horned_owl::model::ForIRI;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::convert::convert_ontology;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct Disjointness {
    pairs: Vec<(String, String)>,
    incomplete: bool,
}
impl Disjointness {
    #[must_use] pub fn pairs(&self) -> &[(String, String)] { &self.pairs }
    #[must_use] pub fn incomplete(&self) -> bool { self.incomplete }
}

const THING: &str = "http://www.w3.org/2002/07/owl#Thing";
const NOTHING: &str = "http://www.w3.org/2002/07/owl#Nothing";

/// Entailed disjoint named-class pairs. `pair_deadline` bounds each `C ⊓ D`
/// probe; `None` = unbounded.
///
/// # Errors
/// [`ReasonError::Inconsistent`] if inconsistent; [`ReasonError::Conversion`].
pub fn disjoint_classes<A: ForIRI>(
    onto: &SetOntology<A>,
    pair_deadline: Option<Duration>,
) -> Result<Disjointness, ReasonError> {
    let internal = convert_ontology(onto)?;
    if crate::abox_saturation::saturate_abox_consistency(&internal).clash {
        return Err(ReasonError::Inconsistent);
    }
    // Candidate named classes = declared classes minus unsat/Thing/Nothing.
    // Use classify to get unsat set + the class list.
    let classification = crate::classify_internal(internal.clone())?;
    let unsat: std::collections::HashSet<&str> =
        classification.unsatisfiable_classes().into_iter().collect();
    let mut names: Vec<(String, owl_dl_core::ir::ClassId)> = Vec::new();
    for c in classification.classes() {
        if c == THING || c == NOTHING || unsat.contains(c.as_str()) {
            continue;
        }
        if let Some(id) = internal.vocabulary.class_id(c) {
            names.push((c.clone(), id));
        }
    }
    let prepared = PreparedOntology::from_internal(internal)?;
    let mut incomplete = !classification.stats().completeness_guaranteed();
    let mut pairs: Vec<(String, String)> = Vec::new();
    for i in 0..names.len() {
        for j in (i + 1)..names.len() {
            let deadline = pair_deadline.map(|d| Instant::now() + d);
            match prepared.pair_disjoint_with_deadline(names[i].1, names[j].1, deadline)? {
                Some(true) => {
                    let (a, b) = (&names[i].0, &names[j].0);
                    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
                    pairs.push((lo.clone(), hi.clone()));
                }
                Some(false) => {}
                None => incomplete = true,
            }
        }
    }
    pairs.sort();
    pairs.dedup();
    Ok(Disjointness { pairs, incomplete })
}
```

Wire in `lib.rs`: add `mod disjointness;` and `pub use disjointness::{disjoint_classes, Disjointness};`. Confirm `classify_internal` and `ClassificationStats::completeness_guaranteed()` exist (the Explore report cited `completeness_guaranteed()` at `classify.rs:~550`); if the name differs, use `stats().fragment` to decide `incomplete` (non-`PureEl`/`Horn` ⇒ incomplete). Make `abox_saturation` reachable from `disjointness.rs` (`pub(crate) mod abox_saturation;` if not already).

- [ ] **Step 4: Run to verify it passes**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test disjoint_oracle`
Expected: PASS both tests.

- [ ] **Step 5: Commit**

```bash
git add crates/owl-dl-reasoner/src/disjointness.rs crates/owl-dl-reasoner/src/lib.rs crates/owl-dl-reasoner/tests/disjoint_oracle.rs
git commit -m "feat(reasoner): disjoint_classes entailment query (#47)"
```

### Task 1.2: `disjoint_object_properties` / `disjoint_data_properties` (structural)

**Files:**
- Modify: `crates/owl-dl-reasoner/src/disjointness.rs`
- Test: `crates/owl-dl-reasoner/tests/disjoint_oracle.rs`

**Interfaces:**
- Produces: `pub fn disjoint_object_properties<A: ForIRI>(onto: &SetOntology<A>) -> Result<Vec<(String, String)>, ReasonError>` and `disjoint_data_properties` (same signature). Told-disjoint pairs (each `(a,b)`, `a < b`), read from the horned-owl ontology directly (object vs data distinguished there).

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn disjoint_object_properties_told() {
    let o = onto(
        r#"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(ObjectProperty(:p)) Declaration(ObjectProperty(:q))
            DisjointObjectProperties(:p :q))"#,
    );
    let pairs = owl_dl_reasoner::disjoint_object_properties(&o).unwrap();
    assert_eq!(pairs, vec![("http://ex/#p".to_string(), "http://ex/#q".to_string())]);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test disjoint_oracle disjoint_object_properties_told`
Expected: FAIL — unresolved.

- [ ] **Step 3: Implement**

Add to `disjointness.rs` (walk components directly, à la `materialize_subobjectproperty_axioms`):

```rust
use horned_owl::model::Component as C;

pub fn disjoint_object_properties<A: ForIRI>(
    onto: &SetOntology<A>,
) -> Result<Vec<(String, String)>, ReasonError> {
    let internal = convert_ontology(onto)?;
    if crate::abox_saturation::saturate_abox_consistency(&internal).clash {
        return Err(ReasonError::Inconsistent);
    }
    let mut out: Vec<(String, String)> = Vec::new();
    for ac in onto {
        if let C::DisjointObjectProperties(ax) = &ac.component {
            // ax.0: Vec<ObjectPropertyExpression>; keep only named (non-inverse) props.
            let names: Vec<String> = ax.0.iter().filter_map(|ope| match ope {
                horned_owl::model::ObjectPropertyExpression::ObjectProperty(op) =>
                    Some(op.0.as_ref().to_string()),
                horned_owl::model::ObjectPropertyExpression::InverseObjectProperty(_) => None,
            }).collect();
            for i in 0..names.len() {
                for j in (i + 1)..names.len() {
                    let (a, b) = (&names[i], &names[j]);
                    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
                    out.push((lo.clone(), hi.clone()));
                }
            }
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

pub fn disjoint_data_properties<A: ForIRI>(
    onto: &SetOntology<A>,
) -> Result<Vec<(String, String)>, ReasonError> {
    let internal = convert_ontology(onto)?;
    if crate::abox_saturation::saturate_abox_consistency(&internal).clash {
        return Err(ReasonError::Inconsistent);
    }
    let mut out: Vec<(String, String)> = Vec::new();
    for ac in onto {
        if let C::DisjointDataProperties(ax) = &ac.component {
            let names: Vec<String> = ax.0.iter().map(|dp| dp.0.as_ref().to_string()).collect();
            for i in 0..names.len() {
                for j in (i + 1)..names.len() {
                    let (a, b) = (&names[i], &names[j]);
                    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
                    out.push((lo.clone(), hi.clone()));
                }
            }
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}
```

Add both to the `pub use` in `lib.rs`. Confirm the horned-owl field shapes (`DisjointObjectProperties(Vec<ObjectPropertyExpression>)`, `DisjointDataProperties(Vec<DataProperty>)`) against the version in `Cargo.lock`; adjust `.0` access if the struct wraps a named field.

- [ ] **Step 4: Run to verify it passes**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test disjoint_oracle disjoint_object_properties_told`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/owl-dl-reasoner/src/disjointness.rs crates/owl-dl-reasoner/src/lib.rs crates/owl-dl-reasoner/tests/disjoint_oracle.rs
git commit -m "feat(reasoner): disjoint object/data property queries (#47, structural)"
```

### Task 1.3: CLI `disjoint --json`

**Files:**
- Modify: `crates/owl-dl-cli/src/main.rs` (Command enum ~50; dispatch), `crates/owl-dl-cli/src/json_out.rs`
- Test: `crates/owl-dl-cli/tests/json_output.rs` + fixture `tests/fixtures/json/disjoint_tiny.ofn`

**Interfaces:**
- Produces CLI `rustdl disjoint --json <file>` →
  ```json
  { "schema_version": 1, "incomplete": false,
    "disjoint_classes": [["<iri>","<iri>"], …],
    "disjoint_object_properties": [["<iri>","<iri>"], …],
    "disjoint_data_properties": [["<iri>","<iri>"], …] }
  ```

- [ ] **Step 1: Write the failing test + fixture**

Fixture `crates/owl-dl-cli/tests/fixtures/json/disjoint_tiny.ofn`:

```
Prefix(:=<http://ex/#>)
Ontology(<http://ex/>
  Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))
  DisjointClasses(:A :B) SubClassOf(:C :A))
```

Test in `tests/json_output.rs` (mirror `realize_json_reports_types`, lines 71-93):

```rust
#[test]
fn disjoint_json_reports_class_pairs() {
    let out = rustdl()
        .args(["disjoint", "--json", fixture("disjoint_tiny.ofn")])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["schema_version"], 1);
    let dc = v["disjoint_classes"].as_array().unwrap();
    let has = |x: &str, y: &str| dc.iter().any(|p| {
        let a = p.as_array().unwrap();
        (a[0] == x && a[1] == y) || (a[0] == y && a[1] == x)
    });
    assert!(has("http://ex/#A", "http://ex/#B"));
}
```

Add a `fn fixture(name: &str) -> String` helper if the file uses per-fixture fns; follow the existing `tiny_abox()` pattern (`CARGO_MANIFEST_DIR` + `tests/fixtures/json/`).

- [ ] **Step 2: Run to verify it fails**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-cli --test json_output disjoint_json_reports_class_pairs`
Expected: FAIL — `disjoint` is not a subcommand.

- [ ] **Step 3: Add the JSON struct + builder**

In `json_out.rs`:

```rust
#[derive(Serialize)]
pub(crate) struct DisjointJson {
    pub(crate) schema_version: u32,
    pub(crate) incomplete: bool,
    pub(crate) disjoint_classes: Vec<[String; 2]>,
    pub(crate) disjoint_object_properties: Vec<[String; 2]>,
    pub(crate) disjoint_data_properties: Vec<[String; 2]>,
}

#[must_use]
pub(crate) fn build_disjoint_json(
    classes: &owl_dl_reasoner::Disjointness,
    obj: Vec<(String, String)>,
    data: Vec<(String, String)>,
) -> DisjointJson {
    let to_arr = |v: Vec<(String, String)>| {
        let mut a: Vec<[String; 2]> = v.into_iter().map(|(x, y)| [x, y]).collect();
        a.sort();
        a
    };
    let mut dc: Vec<[String; 2]> =
        classes.pairs().iter().map(|(x, y)| [x.clone(), y.clone()]).collect();
    dc.sort();
    DisjointJson {
        schema_version: SCHEMA_VERSION,
        incomplete: classes.incomplete(),
        disjoint_classes: dc,
        disjoint_object_properties: to_arr(obj),
        disjoint_data_properties: to_arr(data),
    }
}
```

- [ ] **Step 4: Add the Command variant + dispatch**

In `main.rs` `Command` enum (mirror `Consistent`, lines 51-59):

```rust
    /// Inferred disjointness: disjoint class pairs (entailment) + disjoint
    /// object/data property pairs (structural). `--json` for tooling.
    Disjoint {
        /// Path to an ontology file.
        file: PathBuf,
        /// Per-pair `C ⊓ D` probe deadline in ms (0 = unbounded). Default 1000.
        #[arg(long, default_value_t = 1000)]
        pair_timeout_ms: u64,
        /// Emit a single machine-readable JSON object on stdout (schema v1).
        #[arg(long)]
        json: bool,
    },
```

Dispatch arm (mirror `Command::Consistent`, lines 725-743):

```rust
        Command::Disjoint { file, pair_timeout_ms, json } => {
            let onto = parse_ofn(&file)?;
            let deadline = (pair_timeout_ms > 0)
                .then(|| std::time::Duration::from_millis(pair_timeout_ms));
            let classes = owl_dl_reasoner::disjoint_classes(&onto, deadline)
                .context("disjoint_classes")?;
            let obj = owl_dl_reasoner::disjoint_object_properties(&onto)
                .context("disjoint_object_properties")?;
            let data = owl_dl_reasoner::disjoint_data_properties(&onto)
                .context("disjoint_data_properties")?;
            if json {
                println!("{}", serde_json::to_string_pretty(
                    &json_out::build_disjoint_json(&classes, obj, data))?);
                return Ok(());
            }
            println!("# disjoint classes");
            for (a, b) in classes.pairs() { println!("{a}\t{b}"); }
            if classes.incomplete() {
                eprintln!("warning: disjointness incomplete (budget/fragment) — sound under-approximation");
            }
            println!("# disjoint object properties");
            for (a, b) in &obj { println!("{a}\t{b}"); }
            println!("# disjoint data properties");
            for (a, b) in &data { println!("{a}\t{b}"); }
        }
```

- [ ] **Step 5: Run to verify it passes + clippy**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-cli --test json_output disjoint_json_reports_class_pairs && RUSTUP_TOOLCHAIN=stable cargo clippy -p owl-dl-cli --all-targets -- -D warnings`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/owl-dl-cli/src/main.rs crates/owl-dl-cli/src/json_out.rs crates/owl-dl-cli/tests/json_output.rs crates/owl-dl-cli/tests/fixtures/json/disjoint_tiny.ofn
git commit -m "feat(cli): disjoint --json subcommand (#47)"
```

### Task 1.4: Python bindings for #47

**Files:**
- Modify: `crates/owl-dl-py/src/queries.rs`, `python/rustdl/__init__.py`, `python/rustdl/__init__.pyi`
- Test: `crates/owl-dl-py/tests/python/test_queries.py` (add cases)

**Interfaces:**
- Produces Python: `disjoint_classes(path) -> list[tuple[str,str]]`, `disjoint_object_properties(path) -> list[tuple[str,str]]`, `disjoint_data_properties(path) -> list[tuple[str,str]]`.

- [ ] **Step 1: Write the failing test**

In `crates/owl-dl-py/tests/python/test_queries.py` (create if absent, mirror existing test files):

```python
import rustdl

def test_disjoint_classes(tmp_path):
    p = tmp_path / "o.ofn"
    p.write_text(
        "Prefix(:=<http://ex/#>)\n"
        "Ontology(<http://ex/>\n"
        "  Declaration(Class(:A)) Declaration(Class(:B))\n"
        "  DisjointClasses(:A :B))\n"
    )
    pairs = rustdl.disjoint_classes(str(p))
    assert ("http://ex/#A", "http://ex/#B") in pairs or ("http://ex/#B", "http://ex/#A") in pairs
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd crates/owl-dl-py && RUSTUP_TOOLCHAIN=stable maturin develop && python -m pytest tests/python/test_queries.py::test_disjoint_classes -q`
Expected: FAIL — `module 'rustdl' has no attribute 'disjoint_classes'`.

- [ ] **Step 3: Add the `#[pyfunction]`s**

In `crates/owl-dl-py/src/queries.rs` (mirror `materialize_inferred_subobjectproperty_axioms`, materialize.rs:91-99):

```rust
#[pyfunction]
pub(crate) fn disjoint_classes(path: &str) -> PyResult<Vec<(String, String)>> {
    let ontology = crate::load::load_path(path)?;
    owl_dl_reasoner::disjoint_classes(&ontology, Some(std::time::Duration::from_millis(1000)))
        .map(|d| d.pairs().to_vec())
        .map_err(crate::errors::reason_error_to_py)
}

#[pyfunction]
pub(crate) fn disjoint_object_properties(path: &str) -> PyResult<Vec<(String, String)>> {
    let ontology = crate::load::load_path(path)?;
    owl_dl_reasoner::disjoint_object_properties(&ontology).map_err(crate::errors::reason_error_to_py)
}

#[pyfunction]
pub(crate) fn disjoint_data_properties(path: &str) -> PyResult<Vec<(String, String)>> {
    let ontology = crate::load::load_path(path)?;
    owl_dl_reasoner::disjoint_data_properties(&ontology).map_err(crate::errors::reason_error_to_py)
}
```

Add to `queries::register` (mirror the `wrap_pyfunction!` block in materialize.rs:125-146):

```rust
    m.add_function(wrap_pyfunction!(disjoint_classes, m)?)?;
    m.add_function(wrap_pyfunction!(disjoint_object_properties, m)?)?;
    m.add_function(wrap_pyfunction!(disjoint_data_properties, m)?)?;
```

- [ ] **Step 4: Export in `__init__.py` + stub in `.pyi`**

In `python/rustdl/__init__.py`, add to the `from rustdl._native import (` block:

```python
    disjoint_classes as disjoint_classes,
    disjoint_object_properties as disjoint_object_properties,
    disjoint_data_properties as disjoint_data_properties,
```

and to `__all__`:

```python
    "disjoint_classes",
    "disjoint_object_properties",
    "disjoint_data_properties",
```

In `python/rustdl/__init__.pyi` (under a new `# ── inferred disjointness ──` header):

```python
def disjoint_classes(path: str) -> list[tuple[str, str]]:
    """Entailed disjoint named-class pairs (C ⊓ D unsatisfiable)."""
    ...

def disjoint_object_properties(path: str) -> list[tuple[str, str]]:
    """Told-disjoint object property pairs."""
    ...

def disjoint_data_properties(path: str) -> list[tuple[str, str]]:
    """Told-disjoint data property pairs."""
    ...
```

- [ ] **Step 5: Run tests (incl. stub-drift guard)**

Run: `cd crates/owl-dl-py && RUSTUP_TOOLCHAIN=stable maturin develop && python -m pytest tests/python/test_queries.py tests/python/test_stubs.py -q`
Expected: PASS (test_stubs confirms `__all__` ↔ `.pyi` ↔ module sync).

- [ ] **Step 6: Commit**

```bash
git add crates/owl-dl-py/src/queries.rs crates/owl-dl-py/python/rustdl/__init__.py crates/owl-dl-py/python/rustdl/__init__.pyi crates/owl-dl-py/tests/python/test_queries.py
git commit -m "feat(python): disjoint_classes / disjoint_{object,data}_properties (#47)"
```

### Task 1.5: HermiT oracle test for `disjoint_classes`

**Files:**
- Create: `crates/owl-dl-reasoner/tests/fixtures/disjoint/dj.ofn` + committed `dj-disjoint.owx` oracle
- Modify: `crates/owl-dl-reasoner/tests/disjoint_oracle.rs`; `docker/robot/property-oracle.sh` (add a disjoint-classes generator variant)

**Interfaces:** Consumes `disjoint_classes` (Task 1.1). Diffs rustdl output against HermiT's `InferredDisjointClassesAxiomGenerator` (bidirectional MISSED/FP asserts) — the FP direction is the soundness guard.

- [ ] **Step 1: Author the input fixture** `dj.ofn` — a small consistent TBox with told + inheritance-derived disjointness (e.g. `DisjointClasses(Animal Plant)`, `Dog ⊑ Animal`, `Tree ⊑ Plant`, plus an unsatisfiable class to confirm exclusion).

- [ ] **Step 2: Generate the oracle** — extend `docker/robot/property-oracle.sh` to run ROBOT's `reason --axiom-generators "DisjointClasses"` (HermiT) and emit `dj-disjoint.owx`. Run it once; commit the `.owx`. Document the regenerate command in the test file header (mirror `materialize_oracle.rs:15-17`).

- [ ] **Step 3: Write the oracle diff test** (mirror `materialize_matches_hermit_oracle`, materialize_oracle.rs:63-87):

```rust
#[test]
fn disjoint_classes_matches_hermit_oracle() {
    let dir = std::path::Path::new("tests/fixtures/disjoint");
    let o = { /* read_ofn dir.join("dj.ofn") */ };
    let mut got: std::collections::BTreeSet<(String, String)> =
        disjoint_classes(&o, None).unwrap().pairs().iter().cloned().collect();
    // normalise: oracle pairs sorted lo<hi too
    let oracle = oracle_disjoint_pairs(&dir.join("dj-disjoint.owx"));
    let missed: Vec<_> = oracle.difference(&got).collect();
    let fp: Vec<_> = got.difference(&oracle).collect();
    assert!(fp.is_empty(), "FP — rustdl reports, HermiT does not: {fp:?}");
    // MISSED allowed to be non-empty ONLY if disjoint_classes(..).incomplete(); else assert empty
    assert!(missed.is_empty(), "MISSED — HermiT infers, rustdl omits: {missed:?}");
    let _ = &mut got;
}
```

Add `oracle_disjoint_pairs` helper (walk `Component::DisjointClasses` in the `.owx`, emit sorted named-class pairs, filter Thing/Nothing) mirroring `oracle_edges` (materialize_oracle.rs:37-61).

- [ ] **Step 4: Run**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test disjoint_oracle`
Expected: PASS (FP empty is the hard requirement).

- [ ] **Step 5: Commit**

```bash
git add crates/owl-dl-reasoner/tests/disjoint_oracle.rs crates/owl-dl-reasoner/tests/fixtures/disjoint/ docker/robot/property-oracle.sh
git commit -m "test(reasoner): HermiT oracle for disjoint_classes (#47)"
```

### Task 1.6: FP=0 corpus re-validation + close #47

- [ ] **Step 1:** Run `RUSTUP_TOOLCHAIN=stable cargo test --workspace` (full suite green).
- [ ] **Step 2:** Sanity-run `disjoint --json` on a curated fixture (e.g. `ontologies/real/pizza.ofn` if present) and eyeball that reported pairs are genuine (spot-check a couple against known disjointness). No FP.
- [ ] **Step 3:** Open PR `feat/47-disjointness` referencing "Closes #47". PR body notes: entailment for classes, structural for properties, sound under-approximation with `incomplete` flag.

---

## Phase 2 — Issue #44: property hierarchy (PR: `feat/44-property-hierarchy`)

### Task 2.1: `PropertyClassification` + `classify_{object,data}_property_hierarchy`

**Files:**
- Create: `crates/owl-dl-reasoner/src/property_classify.rs`
- Modify: `crates/owl-dl-reasoner/src/lib.rs`
- Test: `crates/owl-dl-reasoner/tests/property_hierarchy.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct PropertyClassification { equivalent_groups: Vec<Vec<String>>, direct_subsumptions: Vec<(String, String)> }
  impl PropertyClassification { pub fn equivalent_groups(&self) -> &[Vec<String>]; pub fn direct_subsumptions(&self) -> &[(String, String)]; }
  pub fn classify_object_property_hierarchy<A: ForIRI>(onto: &SetOntology<A>) -> Result<PropertyClassification, ReasonError>;
  pub fn classify_data_property_hierarchy<A: ForIRI>(onto: &SetOntology<A>) -> Result<PropertyClassification, ReasonError>;
  ```
  Built by post-processing the transitive `(sub,sup)` closure from `materialize_subobjectproperty_axioms` / `materialize_subdataproperty_axioms`: `equivalent_groups` = strongly-connected sets (mutual `sub⊑sup ∧ sup⊑sub`); `direct_subsumptions` = Hasse edges between equivalence-group representatives.

- [ ] **Step 1: Write the failing test**

```rust
#![allow(clippy::unwrap_used)]
// read_ofn helpers as in disjoint_oracle.rs
use owl_dl_reasoner::classify_object_property_hierarchy;

#[test]
fn object_property_direct_and_equiv() {
    // r ⊑ s, s ⊑ t (⇒ direct r⊑s, s⊑t; r⊑t is transitive, NOT direct);
    // p ≡ q (equiv group).
    let o = onto(
        r#"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))
            Declaration(ObjectProperty(:t)) Declaration(ObjectProperty(:p))
            Declaration(ObjectProperty(:q))
            SubObjectPropertyOf(:r :s) SubObjectPropertyOf(:s :t)
            EquivalentObjectProperties(:p :q))"#,
    );
    let h = classify_object_property_hierarchy(&o).unwrap();
    let direct: Vec<(&str, &str)> =
        h.direct_subsumptions().iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    assert!(direct.contains(&("http://ex/#r", "http://ex/#s")));
    assert!(direct.contains(&("http://ex/#s", "http://ex/#t")));
    assert!(!direct.contains(&("http://ex/#r", "http://ex/#t")), "transitive edge must not be direct");
    assert!(h.equivalent_groups().iter().any(|g| {
        g.contains(&"http://ex/#p".to_string()) && g.contains(&"http://ex/#q".to_string())
    }));
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test property_hierarchy`
Expected: FAIL — unresolved.

- [ ] **Step 3: Implement `property_classify.rs`**

```rust
//! Inferred property hierarchy (issue #44). Structural closure — complete for
//! the fragment the reasoner reasons about (told + equivalent + inverse for
//! object properties, told + equivalent for data). No entailment probe.
use crate::ReasonError;
use horned_owl::model::ForIRI;
use horned_owl::ontology::set::SetOntology;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
pub struct PropertyClassification {
    equivalent_groups: Vec<Vec<String>>,
    direct_subsumptions: Vec<(String, String)>,
}
impl PropertyClassification {
    #[must_use] pub fn equivalent_groups(&self) -> &[Vec<String>] { &self.equivalent_groups }
    #[must_use] pub fn direct_subsumptions(&self) -> &[(String, String)] { &self.direct_subsumptions }
}

/// Turn a transitive `(sub, sup)` closure into equivalence groups + Hasse edges.
fn from_closure(closure: Vec<(String, String)>) -> PropertyClassification {
    let sub_of: BTreeSet<(String, String)> = closure.into_iter().collect();
    // Equivalence: a≡b iff (a,b) and (b,a) both present.
    let mut nodes: BTreeSet<String> = BTreeSet::new();
    for (a, b) in &sub_of { nodes.insert(a.clone()); nodes.insert(b.clone()); }
    // Group by mutual reachability.
    let mut group_of: BTreeMap<String, usize> = BTreeMap::new();
    let mut groups: Vec<Vec<String>> = Vec::new();
    for n in &nodes {
        if group_of.contains_key(n) { continue; }
        let mut grp = vec![n.clone()];
        for m in &nodes {
            if m != n
                && sub_of.contains(&(n.clone(), m.clone()))
                && sub_of.contains(&(m.clone(), n.clone()))
            { grp.push(m.clone()); }
        }
        grp.sort(); grp.dedup();
        let idx = groups.len();
        for g in &grp { group_of.insert(g.clone(), idx); }
        groups.push(grp);
    }
    // Representative = lexicographically smallest member.
    let rep = |g: usize| groups[g][0].clone();
    // Direct subsumption between DISTINCT groups: rep_a ⊑ rep_b with no rep_c strictly between.
    let mut strict: BTreeSet<(usize, usize)> = BTreeSet::new();
    for (a, b) in &sub_of {
        let (ga, gb) = (group_of[a], group_of[b]);
        if ga != gb { strict.insert((ga, gb)); }
    }
    let mut direct: Vec<(String, String)> = Vec::new();
    for &(ga, gb) in &strict {
        let redundant = strict.iter().any(|&(gx, gy)| gx == ga && gy != gb
            && strict.contains(&(gy, gb)));
        if !redundant { direct.push((rep(ga), rep(gb))); }
    }
    direct.sort(); direct.dedup();
    let mut equivalent_groups: Vec<Vec<String>> =
        groups.into_iter().filter(|g| g.len() > 1).collect();
    equivalent_groups.sort();
    PropertyClassification { equivalent_groups, direct_subsumptions: direct }
}

/// # Errors
/// [`ReasonError::Inconsistent`] / [`ReasonError::Conversion`].
pub fn classify_object_property_hierarchy<A: ForIRI>(
    onto: &SetOntology<A>,
) -> Result<PropertyClassification, ReasonError> {
    Ok(from_closure(crate::materialize_subobjectproperty_axioms(onto)?))
}

/// # Errors
/// [`ReasonError::Inconsistent`] / [`ReasonError::Conversion`].
pub fn classify_data_property_hierarchy<A: ForIRI>(
    onto: &SetOntology<A>,
) -> Result<PropertyClassification, ReasonError> {
    Ok(from_closure(crate::materialize_subdataproperty_axioms(onto)?))
}
```

Wire `mod property_classify; pub use property_classify::{PropertyClassification, classify_object_property_hierarchy, classify_data_property_hierarchy};` in `lib.rs`.

- [ ] **Step 4: Run to verify it passes**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test property_hierarchy`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/owl-dl-reasoner/src/property_classify.rs crates/owl-dl-reasoner/src/lib.rs crates/owl-dl-reasoner/tests/property_hierarchy.rs
git commit -m "feat(reasoner): classify_{object,data}_property_hierarchy (#44)"
```

### Task 2.2: CLI `property-hierarchy --json`

**Files:** `crates/owl-dl-cli/src/main.rs`, `json_out.rs`, `tests/json_output.rs`, fixture `tests/fixtures/json/prophier_tiny.ofn`.

**Interfaces:** `rustdl property-hierarchy --json <file>` →
```json
{ "schema_version": 1, "incomplete": false,
  "object_properties": { "equivalent_groups": [[…]], "direct_subsumptions": [["<sub>","<sup>"], …] },
  "data_properties":   { "equivalent_groups": [[…]], "direct_subsumptions": [[…]] } }
```

- [ ] **Step 1: Write failing test + fixture** (fixture: two object props `r ⊑ s`, two data props `d1 ⊑ d2`). Test asserts `v["object_properties"]["direct_subsumptions"]` contains `["r","s"]`.
- [ ] **Step 2: Run** → FAIL (`property-hierarchy` not a subcommand).
- [ ] **Step 3: Add `json_out` structs + builder:**

```rust
#[derive(Serialize)]
pub(crate) struct PropHierSide {
    pub(crate) equivalent_groups: Vec<Vec<String>>,
    pub(crate) direct_subsumptions: Vec<[String; 2]>,
}
#[derive(Serialize)]
pub(crate) struct PropHierJson {
    pub(crate) schema_version: u32,
    pub(crate) incomplete: bool,
    pub(crate) object_properties: PropHierSide,
    pub(crate) data_properties: PropHierSide,
}
fn side(c: &owl_dl_reasoner::PropertyClassification) -> PropHierSide {
    let mut ds: Vec<[String; 2]> =
        c.direct_subsumptions().iter().map(|(a, b)| [a.clone(), b.clone()]).collect();
    ds.sort();
    let mut eg: Vec<Vec<String>> = c.equivalent_groups().to_vec();
    eg.sort();
    PropHierSide { equivalent_groups: eg, direct_subsumptions: ds }
}
#[must_use]
pub(crate) fn build_prophier_json(
    obj: &owl_dl_reasoner::PropertyClassification,
    data: &owl_dl_reasoner::PropertyClassification,
) -> PropHierJson {
    PropHierJson {
        schema_version: SCHEMA_VERSION, incomplete: false,
        object_properties: side(obj), data_properties: side(data),
    }
}
```

- [ ] **Step 4: Add Command variant + dispatch** (mirror `Command::Consistent`): load, call `classify_object_property_hierarchy` + `classify_data_property_hierarchy`, `if json { print build_prophier_json; return Ok(()) }`, else print `# object property hierarchy` / `# data property hierarchy` tab lines.
- [ ] **Step 5: Run** → PASS + clippy clean.
- [ ] **Step 6: Commit** `feat(cli): property-hierarchy --json subcommand (#44)`.

### Task 2.3: Python bindings for #44

**Files:** `crates/owl-dl-py/src/queries.rs`, `__init__.py`, `__init__.pyi`, `tests/python/test_queries.py`.

**Interfaces:** `object_property_hierarchy(path) -> tuple[list[list[str]], list[tuple[str,str]]]` (equivalent_groups, direct_subsumptions); `data_property_hierarchy(path)` same shape.

- [ ] **Step 1:** Test: build an ontology with `r ⊑ s`, assert `("http://ex/#r","http://ex/#s")` in `object_property_hierarchy(path)[1]`.
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Add `#[pyfunction]`s:**

```rust
#[pyfunction]
#[allow(clippy::type_complexity)]
pub(crate) fn object_property_hierarchy(
    path: &str,
) -> PyResult<(Vec<Vec<String>>, Vec<(String, String)>)> {
    let o = crate::load::load_path(path)?;
    let c = owl_dl_reasoner::classify_object_property_hierarchy(&o)
        .map_err(crate::errors::reason_error_to_py)?;
    Ok((c.equivalent_groups().to_vec(), c.direct_subsumptions().to_vec()))
}
// data_property_hierarchy: identical, calling classify_data_property_hierarchy.
```

Register both.
- [ ] **Step 4:** Add to `__init__.py` import block + `__all__`; add `.pyi` stubs:

```python
def object_property_hierarchy(path: str) -> tuple[list[list[str]], list[tuple[str, str]]]:
    """(equivalent_groups, direct_subsumptions) for object properties."""
    ...
def data_property_hierarchy(path: str) -> tuple[list[list[str]], list[tuple[str, str]]]:
    """(equivalent_groups, direct_subsumptions) for data properties."""
    ...
```

- [ ] **Step 5: Run** `maturin develop && pytest test_queries.py test_stubs.py -q` → PASS.
- [ ] **Step 6: Commit** `feat(python): object/data_property_hierarchy (#44)`.

### Task 2.4: Close #44

- [ ] **Step 1:** `RUSTUP_TOOLCHAIN=stable cargo test --workspace` green.
- [ ] **Step 2:** PR `feat/44-property-hierarchy`, "Closes #44"; note structural-complete-for-fragment semantics.

---

## Phase 3 — Issue #46: same / different individuals (PR: `feat/46-same-different`)

### Task 3.1: `same_individuals` / `different_individuals` reasoner fns

**Files:**
- Create: `crates/owl-dl-reasoner/src/individuals.rs`
- Modify: `crates/owl-dl-reasoner/src/lib.rs`
- Test: `crates/owl-dl-reasoner/tests/individuals_oracle.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct SameIndividuals { groups: Vec<Vec<String>>, incomplete: bool }
  pub struct DifferentIndividuals { pairs: Vec<(String, String)>, incomplete: bool }
  // accessors: groups()/pairs() + incomplete()
  pub fn same_individuals<A: ForIRI>(onto: &SetOntology<A>, pair_deadline: Option<Duration>) -> Result<SameIndividuals, ReasonError>;
  pub fn different_individuals<A: ForIRI>(onto: &SetOntology<A>, pair_deadline: Option<Duration>) -> Result<DifferentIndividuals, ReasonError>;
  ```

**Design:**
- `same_individuals`: seed union-find from asserted `SameIndividual` axioms + `SaturationResult.derived_same` (Task 0.1). Extend: for each candidate pair `(a,b)` not yet same, `a=b` entailed iff `consistent_with_extra(&[(a,b)], &[], deadline) == Some(false)` (adding `a≠b` → inconsistent). Union on `Some(false)`; `None` → `incomplete`. Emit equivalence groups (size ≥ 2).
- `different_individuals`: seed from told `DifferentIndividuals`/`AllDifferent`. Extend: `a≠b` entailed iff `{a} ⊓ {b}` unsat, via a `PreparedOntology::pair_individuals_disjoint_with_deadline` probe (analogous to Task 0.2's class probe but building `pool.and([pool.nominal(a), pool.nominal(b)])`).

- [ ] **Step 1:** Add `PreparedOntology::pair_individuals_disjoint_with_deadline` (in `lib.rs`, mirror Task 0.2's `pair_disjoint_with_deadline` but with nominals):

```rust
    pub(crate) fn pair_individuals_disjoint_with_deadline(
        &self, a: IndividualId, b: IndividualId, deadline: Option<std::time::Instant>,
    ) -> Result<Option<bool>, ReasonError> {
        let sat = self.decide_with_deadline(deadline, |pool| {
            let na = pool.nominal(a);
            let nb = pool.nominal(b);
            pool.and([na, nb])
        })?;
        Ok(sat.map(|s| !s)) // {a}⊓{b} unsat ⇒ a≠b
    }
```

Unit test in `lib.rs`: `DifferentIndividuals(:a :b)` ⇒ `pair_individuals_disjoint_with_deadline(a,b,None) == Some(true)`.

- [ ] **Step 2:** Write the failing integration tests in `individuals_oracle.rs`:

```rust
#[test]
fn same_from_functional_role() {
    // Functional(r); r(a,b); r(a,c) ⇒ b=c.
    let o = onto(/* … as Task 0.1 fixture … */);
    let s = owl_dl_reasoner::same_individuals(&o, None).unwrap();
    assert!(s.groups().iter().any(|g| g.contains(&"http://ex/#b".to_string())
        && g.contains(&"http://ex/#c".to_string())));
}

#[test]
fn different_from_disjoint_types() {
    // A,B disjoint; a:A, b:B ⇒ a≠b.
    let o = onto(
        r#"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(Class(:A)) Declaration(Class(:B))
            Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
            DisjointClasses(:A :B) ClassAssertion(:A :a) ClassAssertion(:B :b))"#,
    );
    let d = owl_dl_reasoner::different_individuals(&o, None).unwrap();
    assert!(d.pairs().iter().any(|(x, y)|
        (x == "http://ex/#a" && y == "http://ex/#b") || (x == "http://ex/#b" && y == "http://ex/#a")));
}
```

- [ ] **Step 3:** Run → FAIL. Then implement `individuals.rs`:
  - Convert; inconsistent-KB guard.
  - Enumerate named individuals from `internal.vocabulary.individuals()` (skip anon `ANON_IRI_PREFIX`).
  - `same_individuals`: build union-find from `Axiom::SameIndividual` + `saturate_abox_consistency(&internal).derived_same`; `PreparedOntology::from_internal`; for each not-yet-same pair, `consistent_with_extra(&[(a,b)], &[], deadline)` → union on `Some(false)`, `incomplete` on `None`; emit groups (IRIs, sorted). `incomplete` also true if the fragment isn't complete-by-construction (consistency here is a trusted-`Sat` under-approx — set `incomplete = true` whenever any extension probe returned `Some(true)`, i.e. we relied on a "consistent" verdict; the *derived_same* + asserted seed is always sound-complete, so if NO probe was needed `incomplete` stays false).
  - `different_individuals`: seed told-different pairs; for each remaining pair, `pair_individuals_disjoint_with_deadline` → collect on `Some(true)`, `incomplete` on `None`.
  - Wire `mod individuals; pub use …` in `lib.rs`.
- [ ] **Step 4:** Run → PASS.
- [ ] **Step 5: Commit** `feat(reasoner): same_individuals / different_individuals (#46)`.

### Task 3.2: CLI `individuals --json`

**Files:** `main.rs`, `json_out.rs`, `tests/json_output.rs`, fixtures.

**Interfaces:** `rustdl individuals --json <file>` →
```json
{ "schema_version": 1, "incomplete": false,
  "same_groups": [["<iri>", …], …], "different_pairs": [["<iri>","<iri>"], …] }
```
`incomplete` = `same.incomplete() || different.incomplete()`.

- [ ] **Step 1:** Failing test + fixture (`different_from_disjoint_types` fixture reused). Assert `different_pairs` contains `[a,b]`.
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** `json_out` struct `IndividualsJson { schema_version, incomplete, same_groups: Vec<Vec<String>>, different_pairs: Vec<[String;2]> }` + `build_individuals_json(&SameIndividuals, &DifferentIndividuals)` (sort all).
- [ ] **Step 4:** `Command::Individuals { file, pair_timeout_ms (default 1000), json }` + dispatch calling both reasoner fns.
- [ ] **Step 5:** Run → PASS + clippy.
- [ ] **Step 6: Commit** `feat(cli): individuals --json subcommand (#46)`.

### Task 3.3: Python bindings for #46

**Interfaces:** `same_individuals(path) -> list[list[str]]`, `different_individuals(path) -> list[tuple[str,str]]`.

- [ ] **Step 1:** Test (functional-role same; disjoint-type different).
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** `#[pyfunction]`s in `queries.rs` (1000ms default deadline), returning `.groups().to_vec()` / `.pairs().to_vec()`; register.
- [ ] **Step 4:** `__init__.py` + `__all__` + `.pyi` stubs.
- [ ] **Step 5:** `maturin develop && pytest test_queries.py test_stubs.py -q` → PASS.
- [ ] **Step 6: Commit** `feat(python): same_individuals / different_individuals (#46)`.

### Task 3.4: HermiT oracle for #46 + close

- [ ] **Step 1:** Fixture `individuals/inds.ofn` (functional-forced same + disjoint-type different + a same-as chain). Generate oracle via ROBOT `reason --axiom-generators "SameIndividual DifferentIndividuals"` → `inds-materialized.owx`; commit.
- [ ] **Step 2:** Oracle diff test (FP=0 hard requirement; MISSED allowed only if `incomplete`).
- [ ] **Step 3:** `cargo test --workspace` green; PR `feat/46-same-different`, "Closes #46".

---

## Phase 4 — Issue #45: property values (PR: `feat/45-property-values`)

### Task 4.1: `inferred_object_property_values` / `inferred_data_property_values`

**Files:**
- Create: `crates/owl-dl-reasoner/src/property_values.rs`
- Modify: `crates/owl-dl-reasoner/src/lib.rs`
- Test: `crates/owl-dl-reasoner/tests/property_values_oracle.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct ObjectPropertyValues { triples: Vec<(String, String, String)>, incomplete: bool } // (subject, property, object)
  pub struct DataPropertyValues { quads: Vec<(String, String, String, String)>, incomplete: bool } // (subject, property, lexical, datatype)
  pub fn inferred_object_property_values<A: ForIRI>(onto: &SetOntology<A>, pair_deadline: Option<Duration>) -> Result<ObjectPropertyValues, ReasonError>;
  pub fn inferred_data_property_values<A: ForIRI>(onto: &SetOntology<A>) -> Result<DataPropertyValues, ReasonError>;
  ```

**Design:**
- **Seed** = `materialize_object_property_assertions` / `materialize_data_property_assertions` (already sound lower bounds).
- **Object extension (bounded):** candidate `(a, R, b)` = seed edges' *transitive/inverse neighborhood* only — do NOT enumerate `|I|²×|R|`. For each candidate not already in the seed, `R(a,b)` entailed iff `consistent_with_extra(&[], &[(a, R_id, b)], deadline) == Some(false)` (the `extra_neg_prop` path from Task 0.3). `None` → `incomplete`. **If the Task 0.3 `extra_neg_prop` borrow wiring was deferred, complete it here first** (it's this task's first consumer).
- **Data values:** structural only for v1 (the negative-data-assertion reduction over concrete domains is out of scope) — `inferred_data_property_values` returns the `materialize_data_property_assertions` closure (drop the `lang` element to a 4-tuple), `incomplete: false` within that fragment. (Documented boundary; matches the #45 "structural seed" decision — the budgeted extension applies to object values.)

- [ ] **Step 1:** Complete Task 0.3 `extra_neg_prop` wiring if deferred; unit-test in `lib.rs`: KB with `Functional(r)`, `r(a,b)`, and a TBox forcing `a` to have an `r`-successor equal to `b` — adding `¬r(a,b)` → inconsistent. (Author a minimal fixture where a negative property assertion is known to clash; if hard to construct minimally, assert the simpler property: adding `¬r(a,b)` to a KB that *asserts* `r(a,b)` is inconsistent.)

```rust
#[test]
fn extra_neg_prop_contradicts_asserted_edge() {
    let internal = parse_internal_lib(
        r#"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
            Declaration(ObjectProperty(:r)) ObjectPropertyAssertion(:r :a :b))"#,
    );
    let a = internal.vocabulary.individual_id("http://ex/#a").unwrap();
    let b = internal.vocabulary.individual_id("http://ex/#b").unwrap();
    let r = internal.vocabulary.role_id("http://ex/#r").unwrap();
    let prepared = PreparedOntology::from_internal(internal).unwrap();
    assert_eq!(prepared.consistent_with_extra(&[], &[(a, r, b)], None).unwrap(), Some(false));
}
```

- [ ] **Step 2:** Write failing integration test in `property_values_oracle.rs`:

```rust
#[test]
fn object_values_include_asserted_and_symmetric() {
    // Symmetric(r); r(a,b) ⇒ r(b,a) entailed.
    let o = onto(
        r#"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
            Declaration(ObjectProperty(:r)) SymmetricObjectProperty(:r)
            ObjectPropertyAssertion(:r :a :b))"#,
    );
    let v = owl_dl_reasoner::inferred_object_property_values(&o, None).unwrap();
    let has = |s: &str, p: &str, ob: &str|
        v.triples().iter().any(|(x, y, z)| x == s && y == p && z == ob);
    assert!(has("http://ex/#a", "http://ex/#r", "http://ex/#b"));
    assert!(has("http://ex/#b", "http://ex/#r", "http://ex/#a"));
}
```

(The symmetric edge is already in the materialize seed — this test confirms the seed is surfaced; the entailment-extension path is exercised by the oracle test in Task 4.4.)

- [ ] **Step 3:** Run → FAIL. Implement `property_values.rs`: inconsistent-KB guard; `triples` = seed from `materialize_object_property_assertions`; candidate generation bounded to the seed neighborhood; `PreparedOntology::from_internal`; extension via `consistent_with_extra(&[], &[(a, r, b)], deadline)`; `incomplete` on `None`/trusted-consistency. Data path = structural passthrough. Wire in `lib.rs`.
- [ ] **Step 4:** Run → PASS.
- [ ] **Step 5: Commit** `feat(reasoner): inferred_object/data_property_values (#45)`.

### Task 4.2: CLI `property-values --json`

**Interfaces:** `rustdl property-values --json <file>` →
```json
{ "schema_version": 1, "incomplete": false,
  "object_property_values": [["<subj>","<prop>","<obj>"], …],
  "data_property_values": [["<subj>","<prop>","<lexical>","<datatype>"], …] }
```

- [ ] **Step 1:** Failing test + fixture (symmetric-role) — assert `object_property_values` contains `[b,r,a]`.
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** `json_out` struct + `build_property_values_json` (sort all).
- [ ] **Step 4:** `Command::PropertyValues { file, pair_timeout_ms (default 1000), json }` + dispatch.
- [ ] **Step 5:** Run → PASS + clippy.
- [ ] **Step 6: Commit** `feat(cli): property-values --json subcommand (#45)`.

### Task 4.3: Python bindings for #45

**Interfaces:** `object_property_values(path) -> list[tuple[str,str,str]]`, `data_property_values(path) -> list[tuple[str,str,str,str]]`.

- [ ] **Step 1–2:** Test (symmetric-role object values) → FAIL.
- [ ] **Step 3:** `#[pyfunction]`s (1000ms deadline for object), register.
- [ ] **Step 4:** `__init__.py` + `__all__` + `.pyi` stubs (`#[allow(clippy::type_complexity)]` on the wide-tuple wrappers).
- [ ] **Step 5:** `maturin develop && pytest test_queries.py test_stubs.py -q` → PASS.
- [ ] **Step 6: Commit** `feat(python): object/data_property_values (#45)`.

### Task 4.4: HermiT oracle for #45 + close

- [ ] **Step 1:** Fixture `property_values/pv.ofn` (symmetric + transitive + inverse + sub-property chains producing entailed-but-not-asserted edges). Generate oracle via ROBOT's `InferredPropertyAssertionGenerator` → `pv-materialized.owx`; commit. (Reuse `materialize_oracle.rs`'s existing generator note — this is the same HermiT generator.)
- [ ] **Step 2:** Oracle diff test — FP=0 hard requirement; MISSED allowed only if `incomplete`.
- [ ] **Step 3:** `cargo test --workspace` green; PR `feat/45-property-values`, "Closes #45"; note object=seed+bounded-entailment, data=structural.

---

## Self-Review

**1. Spec coverage:**
- §2.1 invariant 1 (unsat-direction-only) → enforced by every probe returning disjoint/same/different only on `Some(true)`-unsat / `Some(false)`-inconsistent (Tasks 0.2, 0.3, 1.1, 3.1, 4.1). ✓
- §2.1 invariant 2 (inconsistent-KB guard) → every reasoner fn's Step 3 opens with `saturate_abox_consistency(&internal).clash → Err(Inconsistent)` (Tasks 1.1, 1.2, 3.1, 4.1); regression test `disjoint_classes_errors_on_inconsistent` (1.1). ✓
- §3 mechanism (seed + budgeted extension via `decide`) → Phase 0 helpers + per-issue extension. ✓
- §4.1 #47 classes (entailment) + properties (structural) → Tasks 1.1, 1.2. Unsat-class + self-pair exclusion in 1.1 Step 3. ✓
- §4.2 #46 same (derived_same + augment-recheck) + different (`{a}⊓{b}`) → Tasks 0.1, 3.1. ✓
- §4.3 #45 object (bounded entailment) + data (structural) → Task 4.1. Hard-bounded candidate set called out. ✓
- §4.4 #44 structural role closure → Task 2.1. ✓
- §5 structural boundaries (#44, disjoint-props) → Tasks 2.1, 1.2 documented. ✓
- §6 `incomplete` (budget OR non-complete fragment) → carried on every result struct; set on `None` timeout + fragment/trusted-consistency (1.1, 3.1, 4.1). ✓
- §7 three-layer surface → CLI + Python tasks per issue. ✓
- §8 testing (oracle + canaries + golden json + stub drift + FP=0 corpus) → Tasks *.4/*.5 oracle, golden json tasks, test_stubs auto, 1.6 corpus. ✓
- §9 risks (ABox-seed injection, 45 candidate bounding, 47 O(n²) budget, derived-same scope, schema-version) → Tasks 0.3, 4.1, 1.1, 0.1; schema-version fixed in Global Constraints. ✓

**2. Placeholder scan:** No "TBD"/"add error handling"/"write tests for the above". Phases 2–4 CLI/Python steps reference the concrete builder/struct code shown once per layer but repeat the distinctive code (struct shapes, signatures) inline. The two genuinely investigation-dependent points (horned-owl `DisjointObjectProperties` field shape; `decide_with_deadline` arg order; `completeness_guaranteed()` name) are flagged with the fallback to use, not left blank.

**3. Type consistency:** `Disjointness`/`PropertyClassification`/`SameIndividuals`/`DifferentIndividuals`/`ObjectPropertyValues`/`DataPropertyValues` result structs are used identically across reasoner→CLI→Python. `pair_disjoint_with_deadline` / `pair_individuals_disjoint_with_deadline` / `consistent_with_extra` signatures match between definition (Phase 0/3.1) and call sites. `incomplete()` accessor consistent. `build_*_json` names match their dispatch call sites.
