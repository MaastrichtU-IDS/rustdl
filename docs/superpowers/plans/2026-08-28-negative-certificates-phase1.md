# Negative Certificates Phase 1 — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `crates/owl-dl-verify` — a verified finite model of a pure-EL ontology plus an
independent axiom evaluator — so rustdl can detect D10 defects (fragment gate says COMPLETE while
the engine drops an axiom) with no peer reasoner.

**Architecture:** Saturate the ontology, build ONE canonical model whose elements are interned
label sets, then check every admitted axiom against that model with an evaluator that is generic
over an `Interpretation` trait and therefore cannot reach the engine it checks. A `Violated` verdict
means the reported closure admits no model, i.e. the engine dropped something.

**Tech Stack:** Rust (edition 2024, rust-version 1.88), `owl-dl-core`, `owl-dl-saturation`,
`horned-owl` (dev only), `hashbrown`.

**Spec:** `docs/superpowers/specs/2026-08-27-negative-certificates-phase1-design.md` (v3). Read it
before Task 1; this plan argues from it and does not restate its reasoning.

## Global Constraints

**Key-extractor closures, not function items.** `sort_unstable_by_key` and
`binary_search_by_key` pass the key extractor a REFERENCE (`for<'a> fn(&'a ClassId) -> _`), so
`ClassId::index` — whose signature is `fn(self) -> u32` — does **not** compile (E0631, verified).
Every key extractor in this plan is written as a closure (`|c| c.index()`) for that reason. Do not
"tidy" them back into function items.



- Rust edition **2024**, `rust-version` **1.88**; build with `RUSTUP_TOOLCHAIN=stable cargo …`
  (the pinned 1.95.0 toolchain often lacks `cargo`; a failed build **silently reuses a stale
  binary**).
- CI sets `RUSTFLAGS: -D warnings` and clippy `pedantic` is on workspace-wide with `unwrap_used`
  and `dbg_macro` at warn. **No `.unwrap()` in this crate.** `[lints] workspace = true` is
  MANDATORY in the manifest or the crate silently escapes all of it.
- `crates/owl-dl-verify` depends on **`owl-dl-core` and `owl-dl-saturation` ONLY**. Never
  `owl-dl-reasoner` (dependency cycle).
- **`src/eval.rs` must contain no reference to `owl_dl_saturation`.** It is generic over
  `Interpretation` and resolves concepts only through `ConceptPool`. This is the crate's reason to
  exist; Task 1 adds a test that enforces it.
- **No wildcard match arm (`_ => …`) over `Axiom` or `ConceptExpr` anywhere in `eval.rs`.** An
  unhandled form yields `Unresolved`, never a skip.
- **Phase 1 changes no reasoning behaviour.** Nothing is wired into `classify`, `consistent`, or
  `realize`.
- Internal workspace deps use the frozen `version = "0.4.5"` (not `workspace.package.version`).
- OFN fixture syntax: property chains are `ObjectPropertyChain`, **never**
  `SubObjectPropertyChain`; every `Ontology(` needs its closing `)`; declare every entity used.

## File Structure

| file | responsibility |
|---|---|
| `crates/owl-dl-verify/Cargo.toml` | manifest; workspace lints; core + saturation deps |
| `src/lib.rs` | `Bounds`, `Verdict`, `Violation`, `UnresolvedReason`, `build_model`, `verify` |
| `src/interp.rs` | `Element`, `Interpretation` trait — no dependency on the model |
| `src/model.rs` | `FiniteModel`, `VerifiedModel`, label interning, seeding, expansion, injection, chain/transitive closure, the §4 refusal |
| `src/eval.rs` | `eval_concept`, `check_axiom` — generic, engine-blind |
| `tests/fixtures/*.ofn` | committed reproducers (already present) + `*.oracle` verdicts |
| `tests/model.rs` | construction tests (Tasks 2–6) |
| `tests/evaluator.rs` | per-variant sabotage matrix (Tasks 7–9) |
| `tests/acceptance.rs` | the six live-defect invariant tests (Task 12) |

---

### Task 1: Crate scaffold, `Element`, `Interpretation`, and the independence guard

**Files:**
- Create: `crates/owl-dl-verify/Cargo.toml`, `src/lib.rs`, `src/interp.rs`, `src/eval.rs`, `src/model.rs`
- Modify: `Cargo.toml` (root) — `members`, `default-members`, `[workspace.dependencies]`
- Test: `crates/owl-dl-verify/tests/independence.rs`

**Interfaces:**
- Produces: `Element`, `Interpretation` (used by every later task); crate name `owl_dl_verify`.

- [ ] **Step 1: Create the manifest**

```toml
[package]
name = "owl-dl-verify"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
authors.workspace = true
repository.workspace = true

[dependencies]
owl-dl-core = { workspace = true }
owl-dl-saturation = { workspace = true }
hashbrown = { workspace = true }

[dev-dependencies]
horned-owl = { workspace = true }

[lints]
workspace = true
```

- [ ] **Step 2: Wire the workspace**

In the root `Cargo.toml`, add `"crates/owl-dl-verify",` to `members` after `"crates/owl-dl-cb",`
and the identical line to `default-members` after the same entry. In `[workspace.dependencies]`
add:

```toml
owl-dl-verify = { path = "crates/owl-dl-verify", version = "0.4.5" }
```

- [ ] **Step 3: Write `src/interp.rs`**

```rust
//! The interpretation interface the evaluator sees.
//!
//! Deliberately knows nothing about how a model is built: `eval.rs` is generic
//! over this trait so it cannot reach the saturation engine it checks.

use owl_dl_core::{ClassId, RoleId};

/// An element of an interpretation's domain.
#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct Element(u32);

impl Element {
    #[must_use]
    pub const fn new(index: u32) -> Self {
        Self(index)
    }
    #[must_use]
    pub const fn index(self) -> u32 {
        self.0
    }
}

/// A finite first-order interpretation over rustdl's class and role ids.
pub trait Interpretation {
    fn domain_size(&self) -> usize;
    fn elements(&self) -> impl Iterator<Item = Element> + '_;
    /// Is `e` in the extension of atomic class `c`?
    fn in_concept(&self, e: Element, c: ClassId) -> bool;
    /// Successors of `e` under `r`, INCLUDING edges held by any sub-role of `r`.
    ///
    /// Returns an owned `Vec` rather than a slice because the sub-role union is
    /// not a stored contiguous run; promising `&[Element]` would force
    /// materialising the sub-role closure, which the builder deliberately avoids.
    fn successors(&self, e: Element, r: RoleId) -> Vec<Element>;
    fn has_edge(&self, from: Element, r: RoleId, to: Element) -> bool;
    /// Every edge of `r` (incl. sub-role edges), for whole-extension axioms.
    fn edges(&self, r: RoleId) -> Vec<(Element, Element)>;
    fn num_roles(&self) -> usize;
}
```

- [ ] **Step 4: Stub `src/eval.rs` and `src/model.rs`, and write `src/lib.rs`**

`src/eval.rs` and `src/model.rs` start as `//! placeholder — see plan Task 7 / Task 2` plus nothing
else. `src/lib.rs`:

```rust
//! Verified canonical models for pure-EL ontologies, and an engine-blind
//! axiom evaluator over them.
//!
//! See `docs/superpowers/specs/2026-08-27-negative-certificates-phase1-design.md`.

pub mod eval;
pub mod interp;
pub mod model;

pub use interp::{Element, Interpretation};
```

- [ ] **Step 5: Write the independence guard test**

`crates/owl-dl-verify/tests/independence.rs`:

```rust
//! `eval.rs` must not reference the saturation engine. This is the crate's
//! reason to exist: an evaluator sharing code with the engine could hide the
//! very bug it is built to find.

#[test]
fn eval_module_does_not_reference_the_saturation_engine() {
    let src = include_str!("../src/eval.rs");
    assert!(
        !src.contains("owl_dl_saturation"),
        "eval.rs must stay engine-blind; found an owl_dl_saturation reference"
    );
}

#[test]
fn eval_module_has_no_wildcard_match_arm() {
    let src = include_str!("../src/eval.rs");
    assert!(
        !src.contains("_ =>"),
        "a wildcard arm silently skips axiom/concept forms; unhandled forms must \
         yield Unresolved instead"
    );
}
```

- [ ] **Step 6: Verify the guards actually guard (sabotage)**

Temporarily add `use owl_dl_saturation as _;` to `src/eval.rs` and run
`RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-verify --test independence`.
Expected: `eval_module_does_not_reference_the_saturation_engine` **FAILS**. Then add `_ => {}`
inside a dummy `match 1u8 { 1 => {}, _ => {} }` and confirm the second test **FAILS**. Remove both.
A guard that cannot fail is not protecting anything.

- [ ] **Step 7: Build, lint, test**

```bash
RUSTUP_TOOLCHAIN=stable cargo build -p owl-dl-verify
RUSTUP_TOOLCHAIN=stable cargo clippy -p owl-dl-verify --all-targets -- -D warnings
RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-verify
```
Expected: all pass, 2 tests.

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml crates/owl-dl-verify
git commit -m "feat(verify): crate scaffold, Interpretation trait, independence guards"
```

---

### Task 2: `FiniteModel` — label interning and seeding

**Files:**
- Modify: `crates/owl-dl-verify/src/model.rs`
- Test: `crates/owl-dl-verify/tests/model.rs`

**Interfaces:**
- Consumes: `Element`, `Interpretation` (Task 1).
- Produces: `FiniteModel`, `FiniteModel::intern(&mut self, label: Vec<ClassId>) -> Element`,
  `FiniteModel::seed(internal: &InternalOntology, subs: &Subsumers, facts: &[(ClassId, RoleId, ClassId)]) -> Self`,
  `FiniteModel::element_of_class(&self, c: ClassId) -> Option<Element>`,
  `FiniteModel::label(&self, e: Element) -> &[ClassId]`.

- [ ] **Step 1: Write the failing test**

`crates/owl-dl-verify/tests/model.rs`:

```rust
use owl_dl_core::{convert_ontology, ClassId};
use owl_dl_verify::model::FiniteModel;
use owl_dl_verify::Interpretation;

fn load(ofn: &str) -> owl_dl_core::InternalOntology {
    let (onto, _) = horned_owl::io::ofn::reader::read(&mut ofn.as_bytes(), Default::default())
        .expect("parse fixture");
    convert_ontology(&onto).expect("convert fixture")
}

const CHAIN: &str = r#"Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/t>
Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))
SubClassOf(:A :B)
SubClassOf(:B :C)
)
"#;

#[test]
fn seeding_labels_are_sorted_and_contain_the_class_itself() {
    let internal = load(CHAIN);
    let (subs, facts, _) = owl_dl_saturation::saturate_with_exists_facts(&internal);
    let model = FiniteModel::seed(&internal, &subs, &facts);

    let a = internal.vocabulary.class_id("http://ex.org/A").expect("A declared");
    let e = model.element_of_class(a).expect("A is satisfiable, so it is seeded");
    let label = model.label(e);

    assert!(label.windows(2).all(|w| w[0] <= w[1]), "labels must be sorted: {label:?}");
    assert!(label.contains(&a), "subsumers_of is reflexive, so A must be in its own label");
    assert!(model.in_concept(e, a), "in_concept must agree with the label");
}

#[test]
fn derived_equivalent_classes_share_one_element() {
    // B and C are NOT equivalent here, so they must be distinct elements.
    let internal = load(CHAIN);
    let (subs, facts, _) = owl_dl_saturation::saturate_with_exists_facts(&internal);
    let model = FiniteModel::seed(&internal, &subs, &facts);
    let b = internal.vocabulary.class_id("http://ex.org/B").expect("B");
    let c = internal.vocabulary.class_id("http://ex.org/C").expect("C");
    assert_ne!(
        model.element_of_class(b),
        model.element_of_class(c),
        "B and C have different subsumer sets, so they must not be interned together"
    );
}
```

- [ ] **Step 2: Run it and watch it fail**

`RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-verify --test model`
Expected: FAIL — `FiniteModel` has no `seed`/`label`/`element_of_class`.

- [ ] **Step 3: Implement seeding in `src/model.rs`**

```rust
//! The canonical model: elements are INTERNED LABEL SETS.
//!
//! Two classes share an element exactly when their subsumer sets coincide,
//! which (because `subsumers_of` is reflexive) happens exactly for
//! derived-equivalent classes.

use hashbrown::HashMap;
use owl_dl_core::{ClassId, InternalOntology, RoleId};
use owl_dl_saturation::Subsumers;

use crate::interp::{Element, Interpretation};

#[derive(Debug, Default)]
pub struct FiniteModel {
    labels: Vec<Box<[ClassId]>>,
    label_ix: HashMap<Box<[ClassId]>, Element>,
    /// Indexed by `RoleId`; holds edges of the DECLARED role only. Sub-role
    /// inclusion is answered on demand so that closure is never materialised.
    edges: Vec<Vec<(Element, Element)>>,
    class_of: HashMap<ClassId, Element>,
}

impl FiniteModel {
    /// Interns `label` (which MUST be sorted ascending) and returns its element.
    pub fn intern(&mut self, label: Vec<ClassId>) -> Element {
        debug_assert!(label.windows(2).all(|w| w[0] <= w[1]), "label must be sorted");
        let key: Box<[ClassId]> = label.into_boxed_slice();
        if let Some(&e) = self.label_ix.get(&key) {
            return e;
        }
        let e = Element::new(u32::try_from(self.labels.len()).unwrap_or(u32::MAX));
        self.labels.push(key.clone());
        self.label_ix.insert(key, e);
        e
    }

    #[must_use]
    pub fn label(&self, e: Element) -> &[ClassId] {
        self.labels
            .get(e.index() as usize)
            .map_or(&[], |l| l.as_ref())
    }

    #[must_use]
    pub fn element_of_class(&self, c: ClassId) -> Option<Element> {
        self.class_of.get(&c).copied()
    }

    /// Seeds one element per SATISFIABLE class, over the union of the named
    /// vocabulary and every id appearing in `facts` in either position.
    ///
    /// Unsatisfiable classes get NO element. That is inertness hygiene, not a
    /// detection mechanism: a dropped `⊑ ⊥` axiom leaves its class satisfiable
    /// and therefore seeded, and the evaluator is what catches it.
    #[must_use]
    pub fn seed(
        internal: &InternalOntology,
        subs: &Subsumers,
        facts: &[(ClassId, RoleId, ClassId)],
    ) -> Self {
        let mut model = Self {
            edges: vec![Vec::new(); internal.vocabulary.num_roles()],
            ..Self::default()
        };
        let mut population: Vec<ClassId> =
            internal.vocabulary.classes().map(|(id, _)| id).collect();
        for &(sub, _, target) in facts {
            population.push(sub);
            population.push(target);
        }
        population.sort_unstable_by_key(|c| c.index());
        population.dedup();

        for c in population {
            if subs.is_unsatisfiable(c) {
                continue;
            }
            let label = subs.subsumers_of(c);
            let e = model.intern(label);
            model.class_of.insert(c, e);
        }
        model
    }
}

impl Interpretation for FiniteModel {
    fn domain_size(&self) -> usize {
        self.labels.len()
    }
    fn elements(&self) -> impl Iterator<Item = Element> + '_ {
        (0..u32::try_from(self.labels.len()).unwrap_or(u32::MAX)).map(Element::new)
    }
    fn in_concept(&self, e: Element, c: ClassId) -> bool {
        self.label(e).binary_search_by_key(&c.index(), |k| k.index()).is_ok()
    }
    fn successors(&self, _e: Element, _r: RoleId) -> Vec<Element> {
        Vec::new() // edges arrive in Task 4
    }
    fn has_edge(&self, _from: Element, _r: RoleId, _to: Element) -> bool {
        false
    }
    fn edges(&self, _r: RoleId) -> Vec<(Element, Element)> {
        Vec::new()
    }
    fn num_roles(&self) -> usize {
        self.edges.len()
    }
}
```

Note `subsumers_of` already returns ascending ids, so no re-sort is needed — but the
`debug_assert!` in `intern` pins that dependency, because a future "proper subsumers"
representation would silently break both interning and the §3 witness argument.

- [ ] **Step 4: Run the tests**

`RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-verify --test model` → PASS (2 tests).

- [ ] **Step 5: Add the reflexivity pin**

Append to `tests/model.rs`:

```rust
#[test]
fn subsumers_of_is_reflexive_which_interning_and_the_witness_argument_both_need() {
    let internal = load(CHAIN);
    let (subs, _, _) = owl_dl_saturation::saturate_with_exists_facts(&internal);
    let a = internal.vocabulary.class_id("http://ex.org/A").expect("A");
    assert!(
        subs.subsumers_of(a).contains(&a),
        "spec §3 and the label-interning argument both consume reflexivity"
    );
}
```

- [ ] **Step 6: Lint and commit**

```bash
RUSTUP_TOOLCHAIN=stable cargo clippy -p owl-dl-verify --all-targets -- -D warnings
git add crates/owl-dl-verify
git commit -m "feat(verify): FiniteModel label interning and seeding"
```

---

### Task 3: Role hierarchy and effective ranges

**Files:**
- Modify: `crates/owl-dl-verify/src/model.rs`
- Test: `crates/owl-dl-verify/tests/model.rs`

**Interfaces:**
- Produces: `pub fn build_role_hierarchy(internal: &InternalOntology) -> RoleHierarchy`,
  `pub fn effective_ranges(internal: &InternalOntology, h: &RoleHierarchy) -> HashMap<RoleId, Vec<ClassId>>`.
  Both are consumed by Tasks 4, 5 and 6.

- [ ] **Step 1: Write the failing test**

```rust
use owl_dl_verify::model::{build_role_hierarchy, effective_ranges};

const RANGES: &str = r#"Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/r>
Declaration(Class(:F)) Declaration(Class(:G))
Declaration(ObjectProperty(:p)) Declaration(ObjectProperty(:q))
SubObjectPropertyOf(:p :q)
ObjectPropertyRange(:q :F)
ObjectPropertyRange(:p :G)
)
"#;

#[test]
fn effective_ranges_unions_over_super_roles() {
    let internal = load(RANGES);
    let h = build_role_hierarchy(&internal);
    let er = effective_ranges(&internal, &h);
    let p = internal.vocabulary.role_id("http://ex.org/p").expect("p");
    let q = internal.vocabulary.role_id("http://ex.org/q").expect("q");
    let f = internal.vocabulary.class_id("http://ex.org/F").expect("F");
    let g = internal.vocabulary.class_id("http://ex.org/G").expect("G");

    let pr = er.get(&p).cloned().unwrap_or_default();
    assert!(pr.contains(&f), "p ⊑ q, so Range(q,F) constrains p-successors");
    assert!(pr.contains(&g), "p's own range must be included (super_roles is reflexive)");
    let qr = er.get(&q).cloned().unwrap_or_default();
    assert!(qr.contains(&f));
    assert!(!qr.contains(&g), "a SUB-role's range must NOT leak upward to q");
}
```

- [ ] **Step 2: Run it, watch it fail** — FAIL, functions do not exist.

- [ ] **Step 3: Implement both**

```rust
use owl_dl_core::{Axiom, ConceptExpr, Role, RoleHierarchy, RoleHierarchyBuilder, SubRolePath};

/// Builds the named-role hierarchy from the lowered axioms.
///
/// `is_pure_el` admits no inverse-role USE, so inverse canonicalization (which
/// the reasoner's private builder performs) is deliberately not replicated: any
/// inverse occurrence puts the ontology out of fragment.
#[must_use]
pub fn build_role_hierarchy(internal: &InternalOntology) -> RoleHierarchy {
    let n = u32::try_from(internal.vocabulary.num_roles()).unwrap_or(u32::MAX);
    let mut b = RoleHierarchyBuilder::with_roles(n);
    for ax in &internal.axioms {
        match ax {
            Axiom::SubObjectPropertyOf { sub: SubRolePath::Role(r), sup }
                if !r.is_inverse() && !sup.is_inverse() =>
            {
                b.add_sub_role(r.role_id(), sup.role_id());
            }
            Axiom::EquivalentObjectProperties(roles) => {
                for a in roles {
                    for c in roles {
                        if !a.is_inverse() && !c.is_inverse() {
                            b.add_sub_role(a.role_id(), c.role_id());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    b.build()
}

/// `eff_ranges(r) = ⋃ { ranges(s) : s ∈ super_roles(r) }`.
///
/// SUPER-roles, because `r ⊑ s` makes an `r`-edge an `s`-edge, so `Range(s)`
/// constrains `r`-successors. `super_roles` is reflexive.
///
/// `Top` fillers are skipped (trivial). `Bot` fillers are skipped too: a label
/// cannot carry `⊥`, and the axiom check is its home — which is exactly what
/// makes the `Range(r,⊥)` case a DETECTION rather than a refusal.
#[must_use]
pub fn effective_ranges(
    internal: &InternalOntology,
    h: &RoleHierarchy,
) -> HashMap<RoleId, Vec<ClassId>> {
    let mut declared: HashMap<RoleId, Vec<ClassId>> = HashMap::new();
    for ax in &internal.axioms {
        if let Axiom::ObjectPropertyRange { role, range } = ax {
            if role.is_inverse() {
                continue;
            }
            if let ConceptExpr::Atomic(c) = internal.concepts.get(*range) {
                declared.entry(role.role_id()).or_default().push(*c);
            }
        }
    }
    let mut out: HashMap<RoleId, Vec<ClassId>> = HashMap::new();
    for r in 0..u32::try_from(h.num_roles()).unwrap_or(u32::MAX) {
        let rid = RoleId::new(r);
        let mut acc: Vec<ClassId> = Vec::new();
        for s in h.super_roles(rid) {
            if let Some(cs) = declared.get(s) {
                acc.extend_from_slice(cs);
            }
        }
        acc.sort_unstable_by_key(|c| c.index());
        acc.dedup();
        if !acc.is_empty() {
            out.insert(rid, acc);
        }
    }
    out
}
```

The `_ => {}` arms here are fine: this is `model.rs`, not `eval.rs`. The no-wildcard rule exists so
the *evaluator* cannot silently skip a form it should have judged; a builder that ignores axioms
irrelevant to the role hierarchy is correct.

- [ ] **Step 4: Run the test** → PASS.

- [ ] **Step 5: Commit**

```bash
RUSTUP_TOOLCHAIN=stable cargo clippy -p owl-dl-verify --all-targets -- -D warnings
git add crates/owl-dl-verify && git commit -m "feat(verify): role hierarchy and super-role-closed effective ranges"
```

---

### Task 4: Expansion to fixpoint, with `LabelNotClosed` REPORT-ONLY

Report-only first, per the corpus-measurement discipline: detect where a label would need closure
and *report* it, before building the machinery that closes it. That way Task 5's injection has a
measured baseline to beat and cannot silently paper over a construction bug.

**Files:**
- Modify: `crates/owl-dl-verify/src/model.rs`, `src/lib.rs`
- Test: `crates/owl-dl-verify/tests/model.rs`

**Interfaces:**
- Produces: `UnresolvedReason` (in `lib.rs`), `Bounds`,
  `FiniteModel::expand(&mut self, subs, facts, eff: &HashMap<RoleId, Vec<ClassId>>, bounds: &Bounds) -> Vec<UnresolvedReason>`,
  and working `successors`/`has_edge`/`edges`.

- [ ] **Step 1: Write the failing test — Probe B distinctness**

```rust
const PROBE_B: &str = r#"Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/pb>
Declaration(Class(:A)) Declaration(Class(:C)) Declaration(Class(:F))
Declaration(ObjectProperty(:t)) Declaration(ObjectProperty(:u))
SubClassOf(:C ObjectSomeValuesFrom(:t ObjectSomeValuesFrom(:u :A)))
ObjectPropertyRange(:u :F)
)
"#;

#[test]
fn a_as_a_class_and_a_as_a_u_successor_are_distinct_elements() {
    let internal = load(PROBE_B);
    let (subs, facts, _) = owl_dl_saturation::saturate_with_exists_facts(&internal);
    let h = build_role_hierarchy(&internal);
    let eff = effective_ranges(&internal, &h);
    let mut model = FiniteModel::seed(&internal, &subs, &facts);
    let _ = model.expand(&subs, &facts, &eff, &Bounds::default());

    let a = internal.vocabulary.class_id("http://ex.org/A").expect("A");
    let f = internal.vocabulary.class_id("http://ex.org/F").expect("F");
    let u = internal.vocabulary.role_id("http://ex.org/u").expect("u");
    let x_a = model.element_of_class(a).expect("A is seeded");

    // EXISTENTIAL, not universal: a broken expansion that produces zero
    // successors would pass a forall-phrased assertion vacuously.
    let succ: Vec<_> = model
        .elements()
        .flat_map(|e| model.successors(e, u))
        .collect();
    assert!(!succ.is_empty(), "the u-edge must exist");
    let witness = succ[0];
    assert!(model.in_concept(witness, f), "the u-successor must carry Range(u,F)");
    assert!(!model.in_concept(x_a, f), "A-as-a-class must NOT carry F — A ⊑ F is not entailed");
    assert_ne!(witness, x_a, "the two must be different elements");
}
```

- [ ] **Step 2: Run it, watch it fail** — FAIL, `expand` does not exist.

- [ ] **Step 3: Add `Bounds` and `UnresolvedReason` to `src/lib.rs`**

```rust
use owl_dl_core::{ClassId, RoleId};

/// Construction bounds. Checking is bounded separately, by a deadline passed to
/// `verify`, so no stale `Instant` is ever read off a model.
#[derive(Clone, Debug)]
pub struct Bounds {
    pub max_elements: usize,
    pub max_edges: usize,
    pub max_rounds: usize,
}

impl Default for Bounds {
    fn default() -> Self {
        Self { max_elements: 50_000, max_edges: 2_000_000, max_rounds: 8 }
    }
}

/// Why a run could not reach a verdict. NEVER treated as `Verified`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnresolvedReason {
    UnhandledAxiom { axiom_index: usize, variant: &'static str },
    UnhandledConcept { axiom_index: usize, variant: &'static str },
    /// `limit: None` means a deadline expired rather than a count being exceeded.
    BoundTripped { bound: &'static str, limit: Option<usize> },
    GuardedRoleHasEdges { role: RoleId },
    ChainRangeOutOfProfile { chain_super: RoleId },
    LabelNotClosed { class: ClassId, role: RoleId },
    /// A run-delta on an ORIGINAL class between the first and final saturation:
    /// direct evidence the shipped classification is incomplete.
    RunDelta { class: ClassId },
}
```

- [ ] **Step 4: Implement `expand` (report-only for `aug`)**

```rust
impl FiniteModel {
    /// Target label for a fact `(_, r, y)`.
    ///
    /// Returns `Err(aug)` when the label would need TBox closure this local rule
    /// cannot supply. Task 5 replaces that with injection; until then the caller
    /// reports `LabelNotClosed`, because a plain union is a FALSE-`Violated`
    /// generator: with `Range(u,F)` and `F ⊑ G` it yields `{A,F}`, missing `G`,
    /// so `SubClassOf(F,G)` reads violated on a HEALTHY ontology.
    fn target_label(
        subs: &Subsumers,
        eff: &HashMap<RoleId, Vec<ClassId>>,
        r: RoleId,
        y: ClassId,
    ) -> Result<Vec<ClassId>, Vec<ClassId>> {
        let base = subs.subsumers_of(y);
        let Some(ranges) = eff.get(&r) else { return Ok(base) };
        let aug: Vec<ClassId> = ranges
            .iter()
            .copied()
            .filter(|c| base.binary_search_by_key(&c.index(), |k| k.index()).is_err())
            .collect();
        if aug.is_empty() { Ok(base) } else { Err(aug) }
    }

    pub fn expand(
        &mut self,
        subs: &Subsumers,
        facts: &[(ClassId, RoleId, ClassId)],
        eff: &HashMap<RoleId, Vec<ClassId>>,
        bounds: &Bounds,
    ) -> Vec<UnresolvedReason> {
        let mut by_sub: HashMap<ClassId, Vec<(RoleId, ClassId)>> = HashMap::new();
        for &(s, r, t) in facts {
            by_sub.entry(s).or_default().push((r, t));
        }
        let mut reasons = Vec::new();
        let mut queue: Vec<Element> = self.elements().collect();
        let mut edge_count = 0usize;
        while let Some(e) = queue.pop() {
            let classes: Vec<ClassId> = self.label(e).to_vec();
            for x in classes {
                let Some(outs) = by_sub.get(&x).cloned() else { continue };
                for (r, y) in outs {
                    match Self::target_label(subs, eff, r, y) {
                        Ok(label) => {
                            let before = self.labels.len();
                            let t = self.intern(label);
                            if self.labels.len() > bounds.max_elements {
                                reasons.push(UnresolvedReason::BoundTripped {
                                    bound: "max_elements",
                                    limit: Some(bounds.max_elements),
                                });
                                return reasons;
                            }
                            if let Some(bucket) = self.edges.get_mut(r.index() as usize) {
                                if !bucket.contains(&(e, t)) {
                                    bucket.push((e, t));
                                    edge_count += 1;
                                }
                            }
                            if edge_count > bounds.max_edges {
                                reasons.push(UnresolvedReason::BoundTripped {
                                    bound: "max_edges",
                                    limit: Some(bounds.max_edges),
                                });
                                return reasons;
                            }
                            if self.labels.len() > before {
                                queue.push(t);
                            }
                        }
                        Err(_aug) => reasons
                            .push(UnresolvedReason::LabelNotClosed { class: y, role: r }),
                    }
                }
            }
        }
        reasons
    }
}
```

Replace the three placeholder `Interpretation` methods with:

```rust
    fn successors(&self, e: Element, r: RoleId) -> Vec<Element> {
        let mut out = Vec::new();
        for s in self.hierarchy_sub_roles(r) {
            if let Some(bucket) = self.edges.get(s.index() as usize) {
                out.extend(bucket.iter().filter(|(f, _)| *f == e).map(|(_, t)| *t));
            }
        }
        out.sort_unstable_by_key(|e| e.index());
        out.dedup();
        out
    }
    fn has_edge(&self, from: Element, r: RoleId, to: Element) -> bool {
        self.hierarchy_sub_roles(r).iter().any(|s| {
            self.edges
                .get(s.index() as usize)
                .is_some_and(|b| b.contains(&(from, to)))
        })
    }
    fn edges(&self, r: RoleId) -> Vec<(Element, Element)> {
        let mut out = Vec::new();
        for s in self.hierarchy_sub_roles(r) {
            if let Some(bucket) = self.edges.get(s.index() as usize) {
                out.extend_from_slice(bucket);
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }
```

Store the hierarchy on the model. **Do NOT change `seed`'s signature** — Task 2's tests call
`seed(&internal, &subs, &facts)` and must keep passing. Add instead:

```rust
    /// Attaches the role hierarchy, which `successors`/`has_edge`/`edges` need
    /// to answer sub-role inclusion on demand.
    #[must_use]
    pub fn with_hierarchy(mut self, h: RoleHierarchy) -> Self {
        self.hierarchy = Some(h);
        self
    }

    /// Sub-roles of `r`, or `&[]` for a role outside the hierarchy.
    ///
    /// `RoleHierarchy::sub_roles` PANICS out of range, and "the edit introduces a
    /// role" is the normal case for `still_holds_after`, so an unknown role must
    /// read as an empty extension rather than crashing. A model with no
    /// hierarchy attached yet behaves the same way.
    fn hierarchy_sub_roles(&self, r: RoleId) -> &[RoleId] {
        match &self.hierarchy {
            Some(h) if (r.index() as usize) < h.num_roles() => h.sub_roles(r),
            _ => &[],
        }
    }
```

**The field is `Option<RoleHierarchy>`, and that is forced, not stylistic.** `RoleHierarchy` derives
only `Debug, Clone` — **not `Default`** (`crates/owl-dl-core/src/role_hierarchy.rs:132`) — while
`FiniteModel` derives `Default` and `seed` builds itself with `..Self::default()`. A bare
`hierarchy: RoleHierarchy` field therefore breaks both the derive and `seed`. Wrapping in `Option`
keeps them and composes with the rule above: no hierarchy attached reads as an empty extension,
exactly like an out-of-range role. So:

```rust
    hierarchy: Option<RoleHierarchy>,
```

with `with_hierarchy` storing `Some(h)`, and callers writing
`FiniteModel::seed(..).with_hierarchy(h)`.

- [ ] **Step 5: Run the Probe B test** → PASS.

- [ ] **Step 6: Add the report-only baseline test**

```rust
#[test]
fn label_closure_case_reports_LabelNotClosed_rather_than_a_wrong_label() {
    let ofn = std::fs::read_to_string("tests/fixtures/label-closure-range-sub.ofn")
        .expect("fixture present");
    let internal = load(&ofn);
    let (subs, facts, _) = owl_dl_saturation::saturate_with_exists_facts(&internal);
    let h = build_role_hierarchy(&internal);
    let eff = effective_ranges(&internal, &h);
    let mut model = FiniteModel::seed(&internal, &subs, &facts);
    let reasons = model.expand(&subs, &facts, &eff, &Bounds::default());
    assert!(
        reasons.iter().any(|r| matches!(r, UnresolvedReason::LabelNotClosed { .. })),
        "Range(u,F)+F⊑G needs closure this local rule cannot supply; report it, \
         do not emit a truncated label. Got {reasons:?}"
    );
}
```

- [ ] **Step 7: Lint and commit**

```bash
RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-verify
RUSTUP_TOOLCHAIN=stable cargo clippy -p owl-dl-verify --all-targets -- -D warnings
git add crates/owl-dl-verify && git commit -m "feat(verify): fixpoint expansion with report-only LabelNotClosed"
```

---

### Task 4b: AXIOM-DRIVEN expansion (inserted mid-flight — see ruling)

**Why this task exists.** Task 4 expands from the saturator's fact list. A controller probe of
`saturate_with_exists_facts` showed that is not enough, and the numbers are stark:

```text
flat    C ⊑ ∃u.A  +  Range(u,F):   facts: C--u-->T#3   subsumers(T#3) = [A, F, T#3]
nested  C ⊑ ∃t.∃u.A            :   facts: C--t-->T#3   subsumers(T#3) = [T#3]
```

A **flat** existential yields a properly range-folded successor — exactly as §6 of the spec
predicted. A **nested** one yields an **opaque, empty-labelled** element with **no outgoing fact**.
So a fact-driven model is shallower than the ontology, `eval(∃t.∃u.F, x_C)` is vacuously false, and
the instrument would MISS issues #80 and #81 — its own headline prey. (The probe also located the
root cause of #80 and is posted there.)

**The fix is additive and improves independence:** derive existential structure from the AXIOMS via
`ConceptPool`, not only from engine facts.

**Files:**
- Modify: `crates/owl-dl-verify/src/model.rs`
- Test: `crates/owl-dl-verify/tests/model.rs`

**Interfaces:**
- Consumes: `target_label`, `intern`, `edges`, `Bounds`, `UnresolvedReason` (Task 4);
  `effective_ranges` (Task 3).
- Produces: `FiniteModel::expand_from_axioms(&mut self, internal: &InternalOntology, subs: &Subsumers, eff: &HashMap<RoleId, Vec<ClassId>>, bounds: &Bounds) -> Vec<UnresolvedReason>`.
  Task 5's `build_model` calls it immediately after `expand`.

- [ ] **Step 1: Write the failing test — the #80 shape must become detectable**

```rust
const NESTED_MONO: &str = r"Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/nm>
Declaration(Class(:A)) Declaration(Class(:C)) Declaration(Class(:D)) Declaration(Class(:F))
Declaration(ObjectProperty(:t)) Declaration(ObjectProperty(:u))
SubClassOf(:C ObjectSomeValuesFrom(:t ObjectSomeValuesFrom(:u :A)))
SubClassOf(:A :F)
SubClassOf(ObjectSomeValuesFrom(:t ObjectSomeValuesFrom(:u :F)) :D)
)
";

#[test]
fn axiom_driven_expansion_materialises_the_nested_chain() {
    let internal = load(NESTED_MONO);
    let (subs, facts, _) = owl_dl_saturation::saturate_with_exists_facts(&internal);
    let hier = build_role_hierarchy(&internal);
    let eff = effective_ranges(&internal, &hier);
    let mut model = FiniteModel::seed(&internal, &subs, &facts).with_hierarchy(hier);
    let bounds = Bounds::default();
    let _ = model.expand(&subs, &facts, &eff, &bounds);
    let _ = model.expand_from_axioms(&internal, &subs, &eff, &bounds);

    let c = internal.vocabulary.class_id("http://ex.org/C").expect("C");
    let a = internal.vocabulary.class_id("http://ex.org/A").expect("A");
    let f = internal.vocabulary.class_id("http://ex.org/F").expect("F");
    let t = internal.vocabulary.role_id("http://ex.org/t").expect("t");
    let u = internal.vocabulary.role_id("http://ex.org/u").expect("u");
    let x_c = model.element_of_class(c).expect("C is satisfiable");

    // EXISTENTIAL at each hop: a zero-successor model passes a forall phrasing vacuously.
    let mid = model.successors(x_c, t);
    assert!(!mid.is_empty(), "C must gain a t-successor from its own axiom");
    let leaf: Vec<_> = mid.iter().flat_map(|m| model.successors(*m, u)).collect();
    assert!(!leaf.is_empty(), "the NESTED u-successor is what the fact list omits");
    let w = leaf[0];
    assert!(model.in_concept(w, a), "the leaf must satisfy the body class A");
    assert!(
        model.in_concept(w, f),
        "and A ⊑ F must be closed INTO the leaf label — this is what makes the #80 shape detectable"
    );
}
```

- [ ] **Step 2: Run it and watch it fail**

```bash
export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"
RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-verify --test model axiom_driven
```
Expected: FAIL — `expand_from_axioms` does not exist. (Before implementing, confirm it also fails
for the RIGHT reason once the method exists but does nothing: no `u`-successor.)

- [ ] **Step 3: Implement**

```rust
impl FiniteModel {
    /// The atomic classes a concept expression directly requires of an element,
    /// or `None` if the expression is not a shape we can label from.
    ///
    /// `Some(..)` bodies contribute NO classes of their own — an element standing
    /// for `∃u.A` is opaque as a class, and its content is carried by the edge
    /// this function's caller materialises, not by its label.
    fn required_atoms(pool: &ConceptPool, ce: ConceptId, out: &mut Vec<ClassId>) {
        match pool.get(ce) {
            ConceptExpr::Atomic(c) => out.push(*c),
            ConceptExpr::And(ops) => {
                for op in ops.iter() {
                    Self::required_atoms(pool, *op, out);
                }
            }
            _ => {}
        }
    }

    /// Materialises the existential structure of axiom superclass positions.
    ///
    /// The saturator emits no fact for a NESTED existential body and gives its
    /// Tseitin marker an empty subsumer set, so a fact-driven model has no
    /// element for the nested witness at all. This walks the axioms instead:
    /// wherever an element satisfies an axiom's antecedent atoms, the axiom's
    /// consequent existential chain is built out, one element per body.
    ///
    /// Labels reuse `target_label`, so the TBox-closure gap is reported as
    /// `LabelNotClosed` here exactly as it is on the fact path — this task adds
    /// reach, not a second labelling policy.
    pub fn expand_from_axioms(
        &mut self,
        internal: &InternalOntology,
        subs: &Subsumers,
        eff: &HashMap<RoleId, Vec<ClassId>>,
        bounds: &Bounds,
    ) -> Vec<UnresolvedReason> {
        // (antecedent atoms, consequent concept) pairs, from both axiom shapes.
        let mut rules: Vec<(Vec<ClassId>, ConceptId)> = Vec::new();
        for ax in &internal.axioms {
            match ax {
                Axiom::SubClassOf { sub, sup } => {
                    let mut ante = Vec::new();
                    Self::required_atoms(&internal.concepts, *sub, &mut ante);
                    if !ante.is_empty() {
                        rules.push((ante, *sup));
                    }
                }
                Axiom::EquivalentClasses(members) => {
                    for lhs in members {
                        for rhs in members {
                            if lhs == rhs {
                                continue;
                            }
                            let mut ante = Vec::new();
                            Self::required_atoms(&internal.concepts, *lhs, &mut ante);
                            if !ante.is_empty() {
                                rules.push((ante, *rhs));
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        let mut reasons = Vec::new();
        let mut round = 0usize;
        loop {
            let mut grew = false;
            let elems: Vec<Element> = self.elements().collect();
            for e in elems {
                for (ante, sup) in &rules {
                    if !ante.iter().all(|c| self.in_concept(e, *c)) {
                        continue;
                    }
                    if self.materialise_exists(&internal.concepts, subs, eff, bounds, e, *sup, &mut reasons, &mut grew) {
                        return reasons; // a bound tripped
                    }
                }
            }
            round += 1;
            if !grew {
                return reasons;
            }
            if round >= bounds.max_rounds {
                reasons.push(UnresolvedReason::BoundTripped {
                    bound: "max_rounds",
                    limit: Some(bounds.max_rounds),
                });
                return reasons;
            }
        }
    }

    /// Builds out every positive `∃` in `ce` starting at `e`. Returns true iff a
    /// bound tripped and the caller must stop.
    #[allow(clippy::too_many_arguments)]
    fn materialise_exists(
        &mut self,
        pool: &ConceptPool,
        subs: &Subsumers,
        eff: &HashMap<RoleId, Vec<ClassId>>,
        bounds: &Bounds,
        e: Element,
        ce: ConceptId,
        reasons: &mut Vec<UnresolvedReason>,
        grew: &mut bool,
    ) -> bool {
        match pool.get(ce) {
            ConceptExpr::And(ops) => {
                for op in ops.iter() {
                    if self.materialise_exists(pool, subs, eff, bounds, e, *op, reasons, grew) {
                        return true;
                    }
                }
                false
            }
            ConceptExpr::Some(role, body) => {
                if role.is_inverse() {
                    return false;
                }
                let r = role.role_id();
                // Label the witness from the body's own required atoms, closed
                // through `target_label` so the range union and the closure
                // report are identical to the fact path.
                let mut atoms = Vec::new();
                Self::required_atoms(pool, *body, &mut atoms);
                let mut label: Vec<ClassId> = Vec::new();
                let mut unclosed = false;
                for a in &atoms {
                    match Self::target_label(subs, eff, r, *a) {
                        Ok(l) => label.extend(l),
                        Err(_) => unclosed = true,
                    }
                }
                if unclosed {
                    reasons.push(UnresolvedReason::LabelNotClosed {
                        class: *atoms.first().unwrap_or(&ClassId::new(0)),
                        role: r,
                    });
                }
                if atoms.is_empty() {
                    // Opaque body (e.g. a nested `∃`): the witness carries only
                    // the role's effective ranges, and its content comes from
                    // the edges built below.
                    if let Some(rs) = eff.get(&r) {
                        for c in rs {
                            label.extend(subs.subsumers_of(*c));
                        }
                    }
                }
                label.sort_unstable_by_key(|c| c.index());
                label.dedup();
                let before = self.labels.len();
                let w = self.intern(label);
                if self.labels.len() > bounds.max_elements {
                    reasons.push(UnresolvedReason::BoundTripped {
                        bound: "max_elements",
                        limit: Some(bounds.max_elements),
                    });
                    return true;
                }
                if self.labels.len() > before {
                    *grew = true;
                }
                if self.push_edge(r, e, w, bounds, reasons) {
                    return true;
                }
                // Recurse INTO the body at the new witness: this is the hop the
                // fact list omits.
                self.materialise_exists(pool, subs, eff, bounds, w, *body, reasons, grew)
            }
            ConceptExpr::Top
            | ConceptExpr::Bot
            | ConceptExpr::Atomic(_)
            | ConceptExpr::Nominal(_)
            | ConceptExpr::SelfRestriction(_)
            | ConceptExpr::Not(_)
            | ConceptExpr::Or(_)
            | ConceptExpr::All(_, _)
            | ConceptExpr::Min(_, _, _)
            | ConceptExpr::Max(_, _, _) => false,
        }
    }
}
```

Extract the edge-append-with-bound-check from Task 4's `expand` into
`fn push_edge(&mut self, r: RoleId, from: Element, to: Element, bounds: &Bounds, reasons: &mut Vec<UnresolvedReason>) -> bool`
(returns true iff `max_edges` tripped) and use it from both call sites, so the two expansion paths
cannot drift on bound handling.

- [ ] **Step 4: Run the test** → PASS.

- [ ] **Step 5: Un-ignore what is now satisfiable**

Task 4 `#[ignore]`d two tests because the fact list could not reach nested structure. Re-run them
with `--ignored` and, for each that now passes, **remove the `#[ignore]` and its stale reason**. For
any that still fails, leave it ignored but REPLACE the reason with what you measured — a stale
`#[ignore]` reason is an unchecked claim about the engine, and this repo has a documented history of
exactly that going unnoticed for weeks.

```bash
RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-verify --test model -- --ignored
```

- [ ] **Step 6: Commit**

```bash
RUSTUP_TOOLCHAIN=stable cargo fmt -p owl-dl-verify -- --check
RUSTUP_TOOLCHAIN=stable cargo clippy -p owl-dl-verify --all-targets -- -D warnings
RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-verify
git add crates/owl-dl-verify && git commit -m "feat(verify): axiom-driven expansion reaches nested existential witnesses"
```

---

### Task 5: Injection to a fixpoint, and building from the FINAL run

**Files:**
- Modify: `crates/owl-dl-verify/src/model.rs`, `src/lib.rs`
- Test: `crates/owl-dl-verify/tests/model.rs`

**Interfaces:**
- Produces: `pub fn build_model(internal: &InternalOntology, bounds: &Bounds) -> Result<(FiniteModel, Vec<UnresolvedReason>), UnresolvedReason>`.
  Consumed by Tasks 10–13.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn injection_closes_the_label_so_the_healthy_fixture_needs_no_LabelNotClosed() {
    let ofn = std::fs::read_to_string("tests/fixtures/label-closure-range-sub.ofn").expect("fixture");
    let internal = load(&ofn);
    let (_model, reasons) =
        owl_dl_verify::build_model(&internal, &Bounds::default()).expect("builds");
    assert!(
        !reasons.iter().any(|r| matches!(r, UnresolvedReason::LabelNotClosed { .. })),
        "injection must close the label Task 4 could only report: {reasons:?}"
    );
}

#[test]
fn cascade_needs_more_than_two_saturation_rounds() {
    // Regression pin: a conditional ∃-RHS has no anchor class, so its
    // (target, aug) pair is undiscoverable in pass 1. Two runs are NOT enough.
    let ofn = std::fs::read_to_string("tests/fixtures/cascade.ofn").expect("fixture");
    let internal = load(&ofn);
    let mut bounds = Bounds::default();
    bounds.max_rounds = 1;
    let (_m, reasons) = owl_dl_verify::build_model(&internal, &bounds).expect("builds");
    assert!(
        reasons.iter().any(|r| matches!(
            r,
            UnresolvedReason::BoundTripped { bound: "max_rounds", .. }
        )),
        "one round must be insufficient on cascade.ofn: {reasons:?}"
    );
}
```

- [ ] **Step 2: Run, watch fail** — `build_model` does not exist.

- [ ] **Step 3: Implement `build_model`**

```rust
/// Builds the canonical model.
///
/// # Why the model comes from the FINAL augmented run
///
/// `TseitinAllocator::new(internal.vocabulary.num_classes())`
/// (`owl-dl-saturation/src/lib.rs:3398`) bases marker ids at the user-class
/// count, so injecting `k` classes shifts EVERY Tseitin id by `k`, and markers
/// have no IRIs to remap by. Joining a later run's ids against an earlier run's
/// facts therefore mislabels elements arbitrarily — a constructible false
/// `Verified`. So seeds, facts and labels all come from the final run, while the
/// classification being VERIFIED is the one the user actually received (run 1).
///
/// A delta between run 1 and the final run on an ORIGINAL class is reported as
/// `RunDelta`: it is direct evidence the shipped classification is incomplete.
/// Injection is a sound MONOTONE extension — the final run can only ADD entailed
/// derivations among original classes — not an observationally inert one.
pub fn build_model(
    internal: &InternalOntology,
    bounds: &Bounds,
) -> Result<(FiniteModel, Vec<UnresolvedReason>), UnresolvedReason> {
    let (first_subs, _, _) = owl_dl_saturation::saturate_with_exists_facts(internal);
    let mut working = internal.clone();
    let mut reasons: Vec<UnresolvedReason> = Vec::new();
    let mut round = 0usize;

    loop {
        let (subs, facts, _) = owl_dl_saturation::saturate_with_exists_facts(&working);
        let h = model::build_role_hierarchy(&working);
        // Task 6 inserts the ChainRangeOutOfProfile check HERE. Do not add it in
        // this task: `chain_range_out_of_profile` does not exist yet and Task 5
        // must compile and pass on its own.
        let eff = model::effective_ranges(&working, &h);
        let mut m = FiniteModel::seed(&working, &subs, &facts)
            .with_hierarchy(model::build_role_hierarchy(&working));
        let mut step = m.expand(&subs, &facts, &eff, bounds);
        // BOTH expansion paths run. The fact path alone cannot reach a nested
        // existential witness (the saturator emits no inner fact and gives the
        // marker an empty subsumer set), which is why Task 4b exists.
        step.extend(m.expand_from_axioms(&working, &subs, &eff, bounds));

        let pending: Vec<(ClassId, RoleId)> = step
            .iter()
            .filter_map(|r| match r {
                UnresolvedReason::LabelNotClosed { class, role } => Some((*class, *role)),
                _ => None,
            })
            .collect();

        if pending.is_empty() {
            reasons.extend(step.into_iter().filter(|r| {
                !matches!(r, UnresolvedReason::LabelNotClosed { .. })
            }));
            reasons.extend(run_deltas(internal, &first_subs, &subs));
            return Ok((m, reasons));
        }
        round += 1;
        if round >= bounds.max_rounds {
            reasons.push(UnresolvedReason::BoundTripped {
                bound: "max_rounds",
                limit: Some(bounds.max_rounds),
            });
            reasons.extend(step);
            return Ok((m, reasons));
        }
        for (y, r) in pending {
            model::inject_conjunction(&mut working, &subs, &eff, y, r);
        }
    }
}

/// Reports every ORIGINAL class whose satisfiability changed between the first
/// and final saturation. Measured on `unsatnested.ofn`: injection flips `X` from
/// satisfiable to unsatisfiable, and HermiT agrees `X` is unsat — so this is a
/// defect signal, not noise.
fn run_deltas(
    internal: &InternalOntology,
    first: &Subsumers,
    final_: &Subsumers,
) -> Vec<UnresolvedReason> {
    internal
        .vocabulary
        .classes()
        .filter(|(c, _)| first.is_unsatisfiable(*c) != final_.is_unsatisfiable(*c))
        .map(|(c, _)| UnresolvedReason::RunDelta { class: c })
        .collect()
}
```

And in `model.rs`:

```rust
/// Adds `Q ≡ Y ⊓ ⨅aug` to `working`, with an IRI carrying
/// `SYNTHETIC_CLASS_IRI_PREFIX` so reporting filters it.
///
/// A fresh defined class is a conservative extension in the SEMANTIC sense: it
/// cannot make a non-entailment entailed. It is NOT observationally inert on
/// derived output when the engine is incomplete — that is what `RunDelta`
/// records.
pub fn inject_conjunction(
    working: &mut InternalOntology,
    subs: &Subsumers,
    eff: &HashMap<RoleId, Vec<ClassId>>,
    y: ClassId,
    r: RoleId,
) {
    let base = subs.subsumers_of(y);
    let Some(ranges) = eff.get(&r) else { return };
    let mut operands: Vec<ConceptId> = vec![working.concepts.atomic(y)];
    for c in ranges {
        if base.binary_search_by_key(&c.index(), |k| k.index()).is_err() {
            operands.push(working.concepts.atomic(*c));
        }
    }
    if operands.len() < 2 {
        return;
    }
    let iri = format!(
        "{}verify-aug:{}:{}",
        owl_dl_core::residual_absorbability::SYNTHETIC_CLASS_IRI_PREFIX,
        y.index(),
        r.index()
    );
    let q = working.vocabulary.intern_class(&iri);
    let q_expr = working.concepts.atomic(q);
    let conj = working.concepts.and(operands);
    working
        .axioms
        .push(Axiom::EquivalentClasses(vec![q_expr, conj]));
}
```

**Unsatisfiable `Q`:** after re-saturation, if `subs.is_unsatisfiable(q)` for an injected `q`, do
NOT use its row as a label — it is truncated, not a model row. It also *means* the fact's source
class is genuinely unsatisfiable, so push `UnresolvedReason::RunDelta { class: y }` and let the
caller treat it as a defect signal.

- [ ] **Step 4: Run both tests** → PASS.

- [ ] **Step 4b: SETTLE the deferred `#[ignore]`.** Task 4b re-measured the two ignored tests and
  reported that one of them *"would actually pass if wired to `expand_from_axioms`"*, deferring the
  wiring to this task — which the step above performs. Run:

```bash
RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-verify --test model -- --ignored
```

  For each test that now passes, **delete the `#[ignore]` and its reason**. For any that still fails,
  keep it ignored but replace the reason with what you measured here. Do not leave a test ignored on
  the strength of a previous task's note: a test whose green is withheld by scope choice rather than
  by a real gap is exactly the stale-sentinel hazard
  (`docs/2026-08-18-ignored-sentinels-went-stale-unobserved.md`). Report which you un-ignored.

- [ ] **Step 5: Commit**

```bash
RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-verify
RUSTUP_TOOLCHAIN=stable cargo clippy -p owl-dl-verify --all-targets -- -D warnings
git add crates/owl-dl-verify && git commit -m "feat(verify): injection fixpoint, model from the final run, RunDelta signal"
```

---

### Task 6: Chain/transitive materialisation and the `ChainRangeOutOfProfile` refusal

**Files:**
- Modify: `crates/owl-dl-verify/src/model.rs`
- Test: `crates/owl-dl-verify/tests/model.rs`

**Interfaces:**
- Produces: `pub fn chain_range_out_of_profile(internal: &InternalOntology, h: &RoleHierarchy) -> Option<RoleId>`,
  `FiniteModel::close_chains_and_transitivity(&mut self, internal, h, bounds)`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn chain_edges_are_materialised_onto_the_declared_super_role() {
    let ofn = std::fs::read_to_string("tests/fixtures/chainpoison.ofn").expect("fixture");
    let internal = load(&ofn);
    let (m, _) = owl_dl_verify::build_model(&internal, &Bounds::default()).expect("builds");
    let r = internal.vocabulary.role_id("http://t/r").expect("r");
    assert!(!m.edges(r).is_empty(), "Chain(t,u) ⊑ r must materialise an r-edge");
}

#[test]
fn transitivity_with_a_range_is_NOT_refused() {
    // A materialised transitive edge's target was already an edge-target of the
    // same or a sub-role, so it already carries eff_ranges(r). Refusing would be
    // pure coverage loss over the dominant wild combination.
    let ofn = r#"Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/tr>
Declaration(Class(:F)) Declaration(ObjectProperty(:r))
TransitiveObjectProperty(:r)
ObjectPropertyRange(:r :F)
)
"#;
    let internal = load(ofn);
    let h = build_role_hierarchy(&internal);
    assert!(
        chain_range_out_of_profile(&internal, &h).is_none(),
        "TransitiveRole is exempt by construction"
    );
}

#[test]
fn a_chain_whose_head_range_is_not_covered_by_the_second_leg_IS_refused() {
    let ofn = r#"Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/cr>
Declaration(Class(:F))
Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:t)) Declaration(ObjectProperty(:u))
SubObjectPropertyOf(ObjectPropertyChain(:t :u) :r)
ObjectPropertyRange(:r :F)
)
"#;
    let internal = load(ofn);
    let h = build_role_hierarchy(&internal);
    assert!(chain_range_out_of_profile(&internal, &h).is_some());
}
```

- [ ] **Step 2: Run, watch fail.**

- [ ] **Step 3: Implement the refusal**

```rust
/// The OWL 2 EL profile forbids a range on a property implied by a chain
/// (Baader–Brandt–Lutz 2008), precisely because the unrestricted combination
/// breaks the canonical-model technique. `is_el_axiom` admits the two
/// constructs INDEPENDENTLY, so rustdl accepts the combination — see issue #82.
///
/// Refuse iff some admitted 2-leg chain `Chain(t,u) ⊑ v` has
/// `eff_ranges(v) ⊄ eff_ranges(u)`.
///
/// `eff_ranges` MUST be the super-role-closed set, never the declared ranges
/// alone: measured over the 1,920-ontology ORE pool, the precise predicate fires
/// on 61 ontologies and **44 of those only via a super-role of the chain head**.
/// Reading it as declared-only therefore misses the MAJORITY case, and that miss
/// is a false `Verified` (the evaluator reads ranges per declared-role edge
/// vector), not a false `Violated`.
///
/// `TransitiveRole` and the self-chain spelling `Chain(r r) ⊑ r` are exempt: the
/// `⊆ eff_ranges(u)` test passes for them by construction.
#[must_use]
pub fn chain_range_out_of_profile(
    internal: &InternalOntology,
    h: &RoleHierarchy,
) -> Option<RoleId> {
    let eff = effective_ranges(internal, h);
    for ax in &internal.axioms {
        if let Axiom::SubObjectPropertyOf { sub: SubRolePath::Chain(parts), sup } = ax {
            let [_t, u] = parts.as_slice() else { continue };
            if sup.is_inverse() || u.is_inverse() {
                continue;
            }
            let head = eff.get(&sup.role_id()).cloned().unwrap_or_default();
            let second = eff.get(&u.role_id()).cloned().unwrap_or_default();
            if !head.iter().all(|c| second.contains(c)) {
                return Some(sup.role_id());
            }
        }
    }
    None
}
```

- [ ] **Step 4: Implement chain/transitive closure**

```rust
    /// Materialises chain and transitive edges to a fixpoint, writing to the
    /// DECLARED role's vector and reading via `has_edge` (sub-role aware).
    ///
    /// Sub-role inclusion itself is never materialised: it is a lookup, whereas
    /// chains and transitivity generate NEW pairs.
    pub fn close_chains_and_transitivity(
        &mut self,
        internal: &InternalOntology,
        bounds: &Bounds,
    ) -> Vec<UnresolvedReason> {
        let mut rules: Vec<(RoleId, RoleId, RoleId)> = Vec::new();
        for ax in &internal.axioms {
            match ax {
                Axiom::SubObjectPropertyOf { sub: SubRolePath::Chain(parts), sup }
                    if !sup.is_inverse() =>
                {
                    if let [a, b] = parts.as_slice() {
                        if !a.is_inverse() && !b.is_inverse() {
                            rules.push((a.role_id(), b.role_id(), sup.role_id()));
                        }
                    }
                }
                Axiom::TransitiveRole(r) if !r.is_inverse() => {
                    rules.push((r.role_id(), r.role_id(), r.role_id()));
                }
                _ => {}
            }
        }
        let mut changed = true;
        let mut total = 0usize;
        while changed {
            changed = false;
            for &(a, b, v) in &rules {
                for (x, y) in self.edges(a) {
                    for (y2, z) in self.edges(b) {
                        if y != y2 || self.has_edge(x, v, z) {
                            continue;
                        }
                        if let Some(bucket) = self.edges.get_mut(v.index() as usize) {
                            bucket.push((x, z));
                            total += 1;
                            changed = true;
                        }
                        if total > bounds.max_edges {
                            return vec![UnresolvedReason::BoundTripped {
                                bound: "max_edges",
                                limit: Some(bounds.max_edges),
                            }];
                        }
                    }
                }
            }
        }
        Vec::new()
    }
```

Call `close_chains_and_transitivity` from `build_model` immediately after `expand`, before
returning. **Also insert the refusal now** — replace the Task 5 placeholder comment in
`build_model` with:

```rust
        if let Some(chain_super) = model::chain_range_out_of_profile(&working, &h) {
            return Err(UnresolvedReason::ChainRangeOutOfProfile { chain_super });
        }
```

- [ ] **Step 5: Run the tests** → PASS (3 new).

- [ ] **Step 6: Measure the blast radius on the inertness population**

```bash
for o in 13204 3263 11274 4918 2672 10742 2022 3102 5115 4570 3919 13752 12161 4733 16114 5487 13902 11739 16687 14879; do
  grep -c 'ObjectPropertyRange' /data/dumontier/ore-run/pool_sample/files/ore_ont_$o.owl
done | sort | uniq -c
```
Expected: `20` ontologies with count `0` — the refusal cannot fire on any of them. If any is
non-zero, re-screen the Task 13 population before relying on it.

- [ ] **Step 7: Commit**

```bash
git add crates/owl-dl-verify && git commit -m "feat(verify): chain/transitive closure and the chain-range profile refusal"
```

---

### Task 7: The concept evaluator

**Files:**
- Modify: `crates/owl-dl-verify/src/eval.rs`
- Test: `crates/owl-dl-verify/tests/evaluator.rs`

**Interfaces:**
- Produces: `pub enum Judgement { True, False, Unresolved(&'static str) }`,
  `pub fn eval_concept<I: Interpretation>(pool: &ConceptPool, i: &I, e: Element, c: ConceptId) -> Judgement`.

- [ ] **Step 1: Write the failing test**

```rust
use owl_dl_verify::eval::{eval_concept, Judgement};

#[test]
fn top_is_true_bot_is_false_everywhere() { /* build a 1-element model, assert */ }

#[test]
fn unhandled_concept_forms_are_Unresolved_not_false() {
    // All(r, X) is out of fragment: it MUST NOT silently evaluate to true or
    // false, because either would let a stub pass as a real check.
    // (construct ConceptExpr::All via pool.all(...), assert Judgement::Unresolved)
}
```

- [ ] **Step 2: Run, watch fail.**

- [ ] **Step 3: Implement**

```rust
//! Engine-blind axiom and concept evaluation.
//!
//! Generic over `Interpretation`, resolving concepts only through `ConceptPool`
//! — DATA, not saturation logic. An evaluator sharing code with the saturator
//! could hide the very bug this crate exists to find.
//!
//! NO WILDCARD MATCH ARM over `ConceptExpr` or `Axiom`. An unhandled form is
//! `Unresolved`, never a skip: otherwise "accept" can mean "ignored every form
//! I did not recognise".

use owl_dl_core::{ConceptExpr as CE, ConceptId, ConceptPool, Role};

use crate::interp::{Element, Interpretation};

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Judgement {
    True,
    False,
    Unresolved(&'static str),
}

pub fn eval_concept<I: Interpretation>(
    pool: &ConceptPool,
    interp: &I,
    e: Element,
    c: ConceptId,
) -> Judgement {
    match pool.get(c) {
        CE::Top => Judgement::True,
        CE::Bot => Judgement::False,
        CE::Atomic(cls) => {
            if interp.in_concept(e, *cls) { Judgement::True } else { Judgement::False }
        }
        CE::And(ops) => {
            let mut unresolved = None;
            for op in ops.iter() {
                match eval_concept(pool, interp, e, *op) {
                    Judgement::False => return Judgement::False,
                    Judgement::Unresolved(v) => unresolved = Some(v),
                    Judgement::True => {}
                }
            }
            unresolved.map_or(Judgement::True, Judgement::Unresolved)
        }
        CE::Some(Role::Named(r), body) => {
            let mut unresolved = None;
            for t in interp.successors(e, *r) {
                match eval_concept(pool, interp, t, *body) {
                    Judgement::True => return Judgement::True,
                    Judgement::Unresolved(v) => unresolved = Some(v),
                    Judgement::False => {}
                }
            }
            unresolved.map_or(Judgement::False, Judgement::Unresolved)
        }
        CE::Some(Role::Inverse(_), _) => Judgement::Unresolved("Some(Inverse)"),
        CE::Nominal(_) => Judgement::Unresolved("Nominal"),
        CE::SelfRestriction(_) => Judgement::Unresolved("SelfRestriction"),
        CE::Not(_) => Judgement::Unresolved("Not"),
        CE::Or(_) => Judgement::Unresolved("Or"),
        CE::All(_, _) => Judgement::Unresolved("All"),
        CE::Min(_, _, _) => Judgement::Unresolved("Min"),
        CE::Max(_, _, _) => Judgement::Unresolved("Max"),
    }
}
```

All 12 `ConceptExpr` variants are listed explicitly; the compiler enforces that a new variant
breaks the build rather than silently becoming a skip.

- [ ] **Step 4: Run** → PASS. **Step 5: Commit**
`git commit -m "feat(verify): concept evaluator, all 12 variants explicit"`

---

### Task 8: Axiom evaluator — the 8 class-shaped variants, with sabotage matrix

**Files:** Modify `src/eval.rs`; Test `tests/evaluator.rs`

**Interfaces:** Produces `pub fn check_axiom<I: Interpretation>(pool, interp, index: usize, ax: &Axiom) -> AxiomVerdict`
where `pub enum AxiomVerdict { Holds, Fails { witness: Vec<Element>, note: String }, Unresolved(UnresolvedReason) }`.

Covers `DeclareClass`, `DeclareObjectProperty`, `DeclareNamedIndividual`, `SubClassOf`,
`EquivalentClasses`, `DisjointClasses`, `ObjectPropertyDomain`, `ObjectPropertyRange`.

- [ ] **Step 1: Write the SABOTAGE MATRIX tests (one per variant)**

For each of `SubClassOf`, `EquivalentClasses`, `DisjointClasses`, `ObjectPropertyDomain`,
`ObjectPropertyRange`: a fixture where the axiom genuinely constrains the model, plus a mutation
that MUST flip `Holds → Fails` **with the axiom index and witness pinned**. Example for `Range`:

```rust
#[test]
fn sabotage_range_a_deleted_label_entry_must_be_caught_with_index_and_witness() {
    let internal = load(RANGE_FIXTURE);
    let (mut m, _) = build_model(&internal, &Bounds::default()).expect("builds");
    // baseline: the axiom holds
    let idx = range_axiom_index(&internal);
    assert!(matches!(
        check_axiom(&internal.concepts, &m, idx, &internal.axioms[idx]),
        AxiomVerdict::Holds
    ));
    // mutation: strip F from the successor's label
    m.test_only_remove_from_label(successor_element(&m), f_class(&internal));
    match check_axiom(&internal.concepts, &m, idx, &internal.axioms[idx]) {
        AxiomVerdict::Fails { witness, .. } => {
            assert_eq!(witness, vec![successor_element(&m)], "witness must be pinned");
        }
        other => panic!("mutation must be caught, got {other:?}"),
    }
}
```

`test_only_remove_from_label` is a `#[cfg(any(test, feature = "test-mutations"))]` method on
`FiniteModel`; expose it behind that cfg so production code cannot mutate a model.

**Why the matrix exists:** an earlier suite let every test pass while **10 of the 13 axiom
evaluators were `true` stubs**, and such an implementation passes an inertness sweep MORE easily
than an honest one, because stubs cannot fire spuriously. An unpinned expectation is satisfied by
any garbage violation.

- [ ] **Step 2: Run, watch all 5 fail.**
- [ ] **Step 3: Implement the 8 arms** (declarations return `Holds`; `SubClassOf` ∀e;
  `EquivalentClasses` all members agree ∀e; `DisjointClasses` at most one member ∀e;
  `Domain` over `edges(r)` sources; `Range` over `edges(r)` targets; any inverse-polarity role →
  `Unresolved`).
- [ ] **Step 4: Run** → PASS (5 sabotages + 8 baselines).
- [ ] **Step 5: Commit** `git commit -m "feat(verify): class-shaped axiom checks + 5-variant sabotage matrix"`

---

### Task 9: Axiom evaluator — the 5 role-shaped variants, plus all 12 unhandled variants

**Files:** Modify `src/eval.rs`; Test `tests/evaluator.rs`

Covers the 5 remaining checked VARIANTS — `SubObjectPropertyOf` (two arms: `Role` and `Chain`),
`EquivalentObjectProperties`, `TransitiveRole`, `SymmetricRole`, `InverseObjectProperties` — plus
the 12 `Unresolved` variants. 8 (Task 8) + 5 (here) = the 13 checked variants in spec §8.

- [ ] **Step 1: Sabotage tests, one per role-shaped variant** — delete a materialised edge and
  require `Fails` with the axiom index pinned. For `Chain`, delete the chain-composed edge.
- [ ] **Step 2: Guard tests both ways**

```rust
#[test]
fn bare_symmetric_role_with_an_empty_extension_holds() { /* … Holds */ }

#[test]
fn a_guarded_role_that_HAS_edges_is_reported_not_accepted() {
    // The gate admits SymmetricRole only when BareRoleDecls proves the role
    // unread, so it should have no edges. Verify emptiness rather than assume
    // it: a non-empty extension means the observability analysis is wrong,
    // which is itself a finding.
    // … expect AxiomVerdict::Unresolved(GuardedRoleHasEdges { .. })
}

#[test]
fn inverse_object_properties_checks_BOTH_roles() {
    // The guard requires both p and q unread; a check that looks at only one
    // would accept a model where the other has edges.
}
```

- [ ] **Step 3: The unhandled-variant LOOP (not one spot check)**

```rust
#[test]
fn every_unhandled_axiom_variant_yields_Unresolved() {
    for ax in unhandled_axiom_samples() {   // one sample per unchecked variant
        assert!(
            matches!(check_axiom(&pool, &model, 0, &ax), AxiomVerdict::Unresolved(_)),
            "unhandled variant {ax:?} must be Unresolved, never a silent pass"
        );
    }
}
```

`unhandled_axiom_samples()` must return one value for **each** of `DisjointUnion`,
`DisjointObjectProperties`, `AsymmetricRole`, `ReflexiveRole`, `IrreflexiveRole`, `FunctionalRole`,
`InverseFunctionalRole`, `ClassAssertion`, `ObjectPropertyAssertion`,
`NegativeObjectPropertyAssertion`, `SameIndividual`, `DifferentIndividuals` — 12 values. Also
`Chain` of length 1 and 3, and `EquivalentObjectProperties` containing an inverse.

- [ ] **Step 4: Implement.** **Step 5: Run** → PASS. **Step 6: Commit**

---

### Task 10: `verify`, `Verdict`, and the `VerifiedModel` type-state

**Files:** Modify `src/lib.rs`; Test `tests/evaluator.rs`

**Interfaces:** Produces `Verdict`, `Violation`, `VerifiedModel`,
`pub fn verify(model: FiniteModel, internal: &InternalOntology, deadline: Option<Instant>) -> (Verdict, Option<VerifiedModel>)`.

- [ ] **Step 1: Tests** — (a) `Violated` outranks `Unresolved` and still carries its `unresolved`
  rows; (b) `domain_size` is present on all three variants; (c) `verify` returns
  `Some(VerifiedModel)` **iff** `Verified`; (d) a `Violation` on a Tseitin element renders by
  **label**, not IRI (`Vocabulary::class_iri` panics on such ids, and the model is consumed, so
  rendering must happen inside `verify`).
- [ ] **Step 2–4: Implement, run.**
- [ ] **Step 5: Type-state compile-fail test** — `tests/compile_fail/still_holds_on_unverified.rs`
  asserting `FiniteModel::still_holds_after` does not exist. If `trybuild` is not a workspace dep,
  assert it instead by a doc-comment invariant plus a code-review note in the task's commit message;
  do NOT add a new dependency for this.
- [ ] **Step 6: Commit**

---

### Task 11: `still_holds_after`

**Files:** Modify `src/lib.rs`; Test `tests/incremental.rs`

**Interfaces:** Produces
`impl VerifiedModel { pub fn still_holds_after(&self, pool: &ConceptPool, added: &[Axiom], deadline: Option<Instant>) -> Verdict }`.

- [ ] **Step 1: Five tests, the negative one being essential**

```rust
#[test] fn delta_that_holds_in_the_model_is_Verified() { /* … */ }

#[test]
fn delta_that_genuinely_changes_the_classification_is_Violated() {
    // Δ = [SubClassOf(A, B)] where some element has A but not B.
    // WITHOUT this test, a still_holds_after that returns Verified
    // unconditionally passes every other test in the suite.
}

#[test] fn delta_with_an_unhandled_form_is_Unresolved_never_Verified() { /* FunctionalRole */ }

#[test]
fn delta_naming_a_FRESH_ROLE_does_not_panic() {
    // RoleHierarchy::{super,sub}_roles panic out of range, and "the edit
    // introduces a role" is the NORMAL case for this API. An unknown role must
    // read as an empty extension.
}

#[test] fn empty_delta_is_Verified() { /* … */ }
```

- [ ] **Step 2–4: Implement, run.**
- [ ] **Step 5: Document the caller contract on the method** — additions only; `added` is lowered
  IR, so convert against the **original** tables with
  `owl_dl_core::convert::convert_component(&component, &mut vocab, &mut pool)`
  (`crates/owl-dl-core/src/convert.rs:1889`) — re-converting the whole edited ontology yields a
  fresh pool and silently wrong `ClassId`s; and the caller must check `dropped` did not grow,
  because a grown `dropped` invalidates `Verified`.
- [ ] **Step 6: Commit**

---

### Task 12: Acceptance tests as INVARIANTS, with committed oracles

**Files:** Create `crates/owl-dl-verify/tests/acceptance.rs`,
`tests/fixtures/*.oracle`; Test as listed

- [ ] **Step 1: Write the oracle files.** One per fixture, recording the expected classification
  and its **provenance**:

```
# chainpoison.oracle
provenance: konclude-v0.7.0-1138
unsatisfiable: C
```
```
# chain-range-bot.oracle
provenance: derivation-only   # BOTH rustdl and Konclude miss this; see issue #82
unsatisfiable: C
```

Provenance matters: #80, #81 and #82-domain are Konclude-confirmed; **#82-range is
derivation-only**, and a test resting on derivation must say so in its failure message so nobody
later reads it as peer-confirmed.

- [ ] **Step 2: Write the invariant test**

```rust
/// The instrument must NOT return `Verified` while rustdl's own classification
/// disagrees with the committed oracle.
///
/// Phrased as an invariant, NOT as `assert Violated`, because the engine defects
/// these fixtures detect are filed as #80/#81/#82 and are expected to be FIXED.
/// A test asserting `Violated` would then fail because the codebase improved —
/// the `#[ignore]`d-sentinel trap in reverse. Once a defect is fixed, rustdl
/// agrees with the oracle, the antecedent is false, and this passes unchanged.
#[test]
fn instrument_never_verifies_a_classification_that_disagrees_with_the_oracle() {
    for (fixture, oracle) in acceptance_cases() {
        let internal = load_fixture(fixture);
        let rustdl_agrees = classification_matches_oracle(&internal, &oracle);
        let (verdict, _) = verify_fixture(&internal);
        if !rustdl_agrees {
            assert!(
                !matches!(verdict, Verdict::Verified { .. }),
                "{fixture}: rustdl disagrees with the {} oracle, so the instrument \
                 must not report Verified. Got {verdict:?}",
                oracle.provenance
            );
        }
    }
}
```

**Cases — MEASURED 2026-08-28 by running `build_model` on every committed fixture, so these
buckets are observed, not predicted:**

| bucket | fixtures | expectation |
|---|---|---|
| detections (buildable, rustdl disagrees with oracle) | `chainpoison.ofn` (domain 4), `chain-range-bot.ofn` (domain 4), `unsatnested.ofn`, `nested-mono.ofn` (domain 6 — issue #80's three-axiom minimal case) | not `Verified` |
| detection, but likely WEAKER | `cascade.ofn` — builds, yet carries **3 `LabelNotClosed`**, so it will probably land on `Unresolved` rather than `Violated` | not `Verified`; log which |
| REFUSED, so neither control nor detection | `chainrange.ofn`, `chainrange_ctl.ofn` — both return `Err(ChainRangeOutOfProfile)` | `Unresolved`; a known coverage loss Phase 2's fold would recover |
| healthy controls | `unsatconj.ofn`, `flat-mono.ofn`, `label-closure-range-sub.ofn` | **`Verified`** |

Two corrections embedded above, both from measurement:

* **`chainrange_ctl.ofn` is REFUSED**, so it cannot be a control. An earlier draft listed it as one,
  which was already wrong on separate evidence (rustdl misses `C ⊑ D` on it and Konclude reports it —
  issue #80's second instance), and the refusal makes it wrong a second time over.
* **`chainpoison.ofn` and `chain-range-bot.ofn` are both BUILT, not refused.** The second is safe
  specifically because `Range(r, owl:Nothing)` is a `Bot` filler that `effective_ranges` skips, so a
  range-keyed refusal cannot fire on it. Both crown jewels are reachable.

- [ ] **Step 3: Log the weaker outcome.** When the verdict is `Unresolved` rather than `Violated`
  the invariant still holds but coverage is weaker; `eprintln!` which fixtures land there so the
  degradation is visible rather than silent.
- [ ] **Step 4: Run.** **Step 5: Commit**

---

### Task 13: CLI `verify-el`, bounds tests, determinism, and the inertness sweep

**Files:** Modify `crates/owl-dl-cli/Cargo.toml`, `crates/owl-dl-cli/src/main.rs`;
Create `scripts/verify-el-inertness.sh`; Test `crates/owl-dl-cli/tests/verify_el.rs`

- [ ] **Step 1: Add the subcommand.** `rustdl verify-el <file> [--json]`; exit **0** `Verified`,
  **2** `Violated`, **3** `Unresolved`, **1** I/O and parse errors. Distinct codes so a sweep
  buckets without parsing stdout. Gate with the public
  `owl_dl_reasoner::analyze_fragment(&internal) == FragmentClassification::PureEl` —
  `is_pure_el` is `pub(crate)` and unexported, so it is unreachable even from the CLI.
- [ ] **Step 2: Tests** — exit-code mapping for all four; `verify-el ontologies/real/pizza.ofn`
  ⇒ `Unresolved`, exit 3 (out of fragment); `--json` run **twice** on `cascade.ofn` must be
  **byte-identical** (rustdl shipped exactly this bug in `justify`/`report`, #59, and `FiniteModel`
  holds hash maps — sort before emitting).
- [ ] **Step 3: Bounds tests** — `max_elements: 1`, `max_edges: 1`, `max_rounds: 1` and an expired
  deadline each yield `Unresolved { BoundTripped }` **naming the bound**, with the deadline case
  carrying `limit: None`. Without these, a builder that silently truncates at a bound and returns
  `Verified` on the truncated model passes the entire suite.
- [ ] **Step 4: The inertness sweep script**

```bash
#!/usr/bin/env bash
# Inertness: on ontologies where rustdl is believed complete, verify-el must
# return Verified. Population is ORE, NOT the curated corpus: only 1 of 15
# curated files is pure-EL (go-basic.ofn) and at 51,967 classes it exceeds
# max_elements, so a curated-corpus sweep produces ZERO Verified verdicts and
# passes on an empty set.
set -uo pipefail
POOL=${POOL:-/data/dumontier/ore-run/pool_sample/files}
BIN=${BIN:-./target/release/rustdl}
for o in 13204 3263 11274 4918 2672 10742 2022 3102 5115 4570 \
         3919 13752 12161 4733 16114 5487 13902 11739 16687 14879; do
  f="$POOL/ore_ont_$o.owl"
  [ -f "$f" ] || { echo "MISSING ore_ont_$o"; continue; }
  RAYON_NUM_THREADS=1 timeout 300 "$BIN" verify-el "$f" >/dev/null 2>&1
  printf "ore_ont_%-8s exit=%s\n" "$o" "$?"
done
```

Expected: exit **0** on all 20 (`Verified` specifically — never merely "zero `Violated`", since an
always-`Unresolved` implementation also has zero violations). `ore_ont_1357` and `ore_ont_283` are
free `BoundTripped` cases; `go-basic.ofn` is a documented `BoundTripped` case and is **not** raised
past.

- [ ] **Step 5: Injection needs SYNTHETIC coverage.** Measured: **0 injections across 6 real
  pure-EL ontologies** (no nested/⊤ fillers under range-bearing roles), so this sweep will not
  exercise Task 5 at all. Add two synthetic fixtures: one where the cheap closure-union would
  suffice, and one with a conjunctive trigger (`A ⊓ F ⊑ H`) that **only** full injection closes —
  the second is what pins Task 5 rather than a shortcut.
- [ ] **Step 6: Commit**

---

### Task 14: Documentation and the fix-in-passing

**Files:** Modify `crates/owl-dl-saturation/src/lib.rs:103`, `CLAUDE.md`, `README.md`

- [ ] **Step 1:** Fix the doc drift: line 103 documents `RUSTDL_EL_BOT_FILLER` as "Default
  **OFF**"; the predicate at line 149 is `is_none_or(|v| v != "0")` — **ON**.
- [ ] **Step 2:** Add an `owl-dl-verify` paragraph to `CLAUDE.md`'s workspace-architecture list,
  stating the scope (`is_pure_el` only), that it is diagnostic-only, and that its acceptance
  fixtures track issues #80/#81/#82.
- [ ] **Step 3:** Document `verify-el` in the README subcommand list.
- [ ] **Step 4: Full gates**

```bash
RUSTUP_TOOLCHAIN=stable cargo fmt --all -- --check
RUSTUP_TOOLCHAIN=stable cargo clippy --workspace --all-targets --all-features -- -D warnings
RUSTUP_TOOLCHAIN=stable cargo test --workspace
./scripts/run-soundness-diff.sh   # must stay FP=0; this crate touches no reasoning path
```

- [ ] **Step 5: Record what execution discovered.** These are carried forward from task reviews and
  would otherwise survive only in scratch files:

  1. **`docs/known-limitations/verify-two-expansion-paths-split-a-witness.md`** — the two expansion
     paths label the same nested-existential witness DIFFERENTLY (the axiom path from `eff_ranges`
     only, often empty; the fact path from `subsumers_of(Tseitin Q)`, non-empty), and `intern` dedups
     purely by LABEL CONTENT, so **one logical witness becomes two elements**. That contradicts the
     spec's own "one canonical interpretation" framing. Not shown unsound — an extra edge-less element
     contributes to no composed pair — but whether a future concept-level check could read a WEAKER
     answer at the under-labelled witness is untested. Add a matching comment on
     `materialise_exists`'s opaque-body branch, because this currently survives only in a task report
     and test comments that a future `model.rs` reader will miss.
  2. **`Violation`'s struct doc** is written entirely from `verify`'s perspective. Note that
     `still_holds_after` also produces `Violation`s — via a borrow rather than a consume — and that
     its `axiom_index` indexes into `added`, NOT `internal.axioms`.
  3. **Amend spec §7**: it says the crate must never depend on `owl-dl-reasoner`. Task 12 added it as
     a **dev**-dependency so the acceptance suite runs the real hybrid classifier instead of grading
     its own homework. Reword to "not a RUNTIME dependency"; the property that matters is the absence
     of a cycle, which holds (nothing in `owl-dl-reasoner`'s manifest names `owl-dl-verify`).
  4. **Record the measured coverage** in the `CLAUDE.md` paragraph, in these words or closer:
     inertness established on **16 of 20** banner-selected pure-EL ORE ontologies (4 unmeasured —
     exit-124 timeouts at 300 s, NOT passes); **5 detections** on committed fixtures tracking issues
     #80/#81/#82, with 2 fixtures REFUSED by the chain-range profile guard; and the injection fixpoint
     **past round 1 is untested machinery** (injection is corpus-rare: 0 injections across 6 real
     pure-EL ontologies, and `cascade.ofn` converges in one round).

- [ ] **Step 6: Commit**

---

## Ordering discipline (read before Task 13)

Inertness first for **interpretation** — a violation carries no information until spurious
violations are zero. But as a **work** order it is a hazard: "drive spurious violations to zero" is
a tuning loop in which weakening the evaluator also makes the sweep green. Two rules separate
repair from suppression:

1. **`axioms_checked` must never decrease across a tuning step.** A builder change may create new
   `Verified`s; an evaluator change may only move an axiom `Violated → Unresolved` — visible and
   counted — never to a silent pass.
2. **Run Task 12's acceptance tests continuously DURING the inertness phase, not after it.** The
   signature of suppression is an acceptance test flipping away from `Violated`. A calibration pair
   is only a calibration pair while it is armed.
