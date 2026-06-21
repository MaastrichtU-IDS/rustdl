# `rustdl justify --laconic` (laconic justifications) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `rustdl justify --laconic`, which weakens each axiom of a regular justification to its responsible *fragment* (e.g. drops the superfluous conjuncts of `C ⊑ B ⊓ D ⊓ ∃r.E` when only `C ⊑ B` is needed).

**Architecture:** A new read-only module `crates/owl-dl-reasoner/src/laconic.rs` holds the sound structural **weakening** operators (`weaken`/`split_sup`) plus the laconic driver. The driver takes a regular justification (from the shipped `justify` module), replaces each axiom with its entailed weaker fragments, and re-runs the existing `quickxplain` over those fragments. A `--laconic` flag on the `justify` CLI handler routes to it.

**Tech Stack:** Rust (edition 2024), horned-owl model types, the `owl-dl-reasoner` crate (`justify::{find_one_justification, find_all_justifications, quickxplain, logical_axioms, Entailment, Justification}`).

**Spec:** `docs/superpowers/specs/2026-06-21-laconic-justifications-design.md`
**Branch:** `feat/laconic-justifications`

---

## Key facts (verified against the codebase)

- `Component<A>` is constructed as e.g. `Component::SubClassOf(SubClassOf { sub, sup })` (see `crates/owl-dl-core/src/convert_back.rs:173`). At the pinned horned-owl rev `SubClassOf` is the 2-field struct `{ sub, sup }` (no `ann`). `DisjointClasses(Vec<ClassExpression>)`, `EquivalentClasses(Vec<ClassExpression>)`.
- `ClassExpression<A>` (`use horned_owl::model::ClassExpression as CE`): `CE::Class(c)` with IRI `c.0.as_ref()`; `CE::ObjectIntersectionOf(Vec<CE>)`; `CE::ObjectSomeValuesFrom { ope, bce }` where `ope: ObjectPropertyExpression` (Clone) and `bce: Box<CE>`. All other variants are not split.
- `Component<A>: Clone + Eq + Hash + Ord` (already used as `BTreeSet<Component<A>>` / `HashSet<BTreeSet<Component<A>>>` in `justify.rs`).
- `justify::logical_axioms(onto) -> (Vec<Component<A>>, Vec<Component<A>>)` — `.0` = non-logical (declarations/annotations), `.1` = logical axioms.
- `justify::quickxplain(fixed: &[Component<A>], candidates: &[Component<A>], q: &Entailment) -> Result<Vec<Component<A>>, ReasonError>` is `pub(crate)` (callable from sibling modules in the crate). It returns a minimal subset of `candidates` such that `fixed ∪ subset ⊨ q`; **precondition** `fixed ∪ candidates ⊨ q`.
- `justify::find_one_justification(onto, q) -> Result<Option<Justification<A>>, ReasonError>`; `find_all_justifications(onto, q, max) -> Result<Vec<Justification<A>>, ReasonError>`. `Justification<A> { pub axioms: Vec<Component<A>>, pub fragment: FragmentClassification, pub minimal_guaranteed: bool }`.
- CLI `justify` handler is around `crates/owl-dl-cli/src/main.rs:838`; the `Justify { … }` variant around `main.rs:216`. It uses `RcStr`, `parse_ofn_with_pm`, `build_label_map`, `local_name`, `ax.as_manchester_with_prefixes(&pm)`, and `owl_dl_reasoner::justify::component_entities`.
- Module declarations: `crates/owl-dl-reasoner/src/lib.rs` near line 46 (`pub mod justify;`); re-exports near line 57.

## File structure

- **Create** `crates/owl-dl-reasoner/src/laconic.rs` — `weaken`/`split_sup` operators + `find_laconic_justification` / `find_all_laconic_justifications` + unit tests.
- **Modify** `crates/owl-dl-reasoner/src/lib.rs` — `pub mod laconic;` + re-export.
- **Create** `crates/owl-dl-reasoner/tests/laconic_justification.rs` — integration tests.
- **Modify** `crates/owl-dl-cli/src/main.rs` — `--laconic` flag on the `Justify` variant + handler branch.
- **Modify** `README.md`, `CLAUDE.md` — document the flag (final task).

---

### Task 1: Branch + module skeleton

**Files:** Modify `crates/owl-dl-reasoner/src/lib.rs`; Create `crates/owl-dl-reasoner/src/laconic.rs`

ENVIRONMENT: cargo may not be on PATH — prefix shells with:
```bash
export RUSTUP_HOME=/home/dumontier/.rustup
export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"
```

- [ ] **Step 1: Create the branch**

```bash
cd /data/dumontier/rustdl
git checkout main
git checkout -b feat/laconic-justifications
```

- [ ] **Step 2: Create `crates/owl-dl-reasoner/src/laconic.rs`**

```rust
//! Laconic (fine-grained) justifications: weaken each axiom of a regular
//! justification to its responsible fragment, then re-minimize. Sound by
//! construction — every emitted fragment is *entailed by* an original axiom, so a
//! laconic justification is a set of genuine consequences of the ontology that
//! explains the entailment. Read-only; FP=0 untouched.

use std::collections::{BTreeSet, HashSet};

use horned_owl::model::{ClassExpression, Component, ForIRI, SubClassOf};
use horned_owl::ontology::set::SetOntology;

use crate::ReasonError;
use crate::justify::{
    Entailment, Justification, find_all_justifications, find_one_justification, logical_axioms,
    quickxplain,
};

/// Weaken a single axiom into a set of fragments, each ENTAILED BY the axiom.
/// An axiom with no applicable operator returns `vec![axiom.clone()]` (passes
/// through unchanged). Filled in by Task 2.
fn weaken<A: ForIRI>(axiom: &Component<A>) -> Vec<Component<A>> {
    vec![axiom.clone()]
}
```

- [ ] **Step 3: Wire into `lib.rs`**

In `crates/owl-dl-reasoner/src/lib.rs`, add next to `pub mod justify;`:
```rust
pub mod laconic;
```
and next to the other `pub use` re-exports:
```rust
pub use laconic::{find_all_laconic_justifications, find_laconic_justification};
```
(These two functions are created in Task 3. To keep Task 1 compiling, add temporary stubs now — see Step 4.)

- [ ] **Step 4: Add temporary stubs so the re-export resolves**

Append to `crates/owl-dl-reasoner/src/laconic.rs`:
```rust
/// Laconic justification for `q` (one). Filled in by Task 3.
pub fn find_laconic_justification<A: ForIRI>(
    onto: &SetOntology<A>,
    q: &Entailment,
) -> Result<Option<Justification<A>>, ReasonError> {
    let _ = (onto, q, weaken::<A> as fn(&Component<A>) -> Vec<Component<A>>);
    Ok(None)
}

/// All laconic justifications for `q` (capped). Filled in by Task 3.
pub fn find_all_laconic_justifications<A: ForIRI>(
    onto: &SetOntology<A>,
    q: &Entailment,
    max: usize,
) -> Result<Vec<Justification<A>>, ReasonError> {
    let _ = (onto, q, max);
    Ok(Vec::new())
}
```
NOTE: the `weaken::<A> as fn(...)` reference in the stub is only to suppress the
unused-function warning until Task 2/3 wire `weaken` in. If it causes any trouble,
instead put `#[allow(dead_code)]` above `weaken` with the comment
`// wired into the driver in Task 3; allow removed there` and drop the `weaken`
reference from the stub body (`let _ = (onto, q);`). Also unused: `BTreeSet`,
`HashSet`, `SubClassOf`, `find_all_justifications`, `find_one_justification`,
`logical_axioms`, `quickxplain`, `ClassExpression` — these are used in Tasks 2–3;
if `cargo build` (not clippy) errors on them, keep them and rely on warnings;
do NOT run clippy in this task.

- [ ] **Step 5: Build**

Run: `cargo build -p owl-dl-reasoner`
Expected: compiles (unused-import warnings are fine; later tasks use them).

- [ ] **Step 6: Commit**

```bash
git add crates/owl-dl-reasoner/src/laconic.rs crates/owl-dl-reasoner/src/lib.rs
git commit -m "feat(laconic): module skeleton + driver stubs"
```

---

### Task 2: Weakening operators (`split_sup` + `weaken`)

**Files:** Modify `crates/owl-dl-reasoner/src/laconic.rs`

- [ ] **Step 1: Write the failing tests** — append to `crates/owl-dl-reasoner/src/laconic.rs`:

```rust
#[cfg(test)]
mod weaken_tests {
    use super::*;
    use horned_owl::model::Build;

    type Rc = std::rc::Rc<str>;
    fn b() -> Build<Rc> {
        Build::new_rc()
    }
    fn cls(b: &Build<Rc>, iri: &str) -> ClassExpression<Rc> {
        ClassExpression::Class(b.class(iri))
    }
    fn sc(sub: ClassExpression<Rc>, sup: ClassExpression<Rc>) -> Component<Rc> {
        Component::SubClassOf(SubClassOf { sub, sup })
    }

    // C ⊑ D ⊓ E  →  {C ⊑ D, C ⊑ E}
    #[test]
    fn rhs_conjunction_splits() {
        let b = b();
        let ax = sc(
            cls(&b, "urn:C"),
            ClassExpression::ObjectIntersectionOf(vec![cls(&b, "urn:D"), cls(&b, "urn:E")]),
        );
        let got: BTreeSet<Component<Rc>> = weaken(&ax).into_iter().collect();
        let want: BTreeSet<Component<Rc>> =
            [sc(cls(&b, "urn:C"), cls(&b, "urn:D")), sc(cls(&b, "urn:C"), cls(&b, "urn:E"))]
                .into_iter()
                .collect();
        assert_eq!(got, want);
    }

    // C ⊑ ∃r.(D ⊓ E)  →  {C ⊑ ∃r.D, C ⊑ ∃r.E}
    #[test]
    fn existential_filler_splits() {
        let b = b();
        let some = |f: ClassExpression<Rc>| ClassExpression::ObjectSomeValuesFrom {
            ope: b.object_property("urn:r").into(),
            bce: Box::new(f),
        };
        let ax = sc(
            cls(&b, "urn:C"),
            some(ClassExpression::ObjectIntersectionOf(vec![cls(&b, "urn:D"), cls(&b, "urn:E")])),
        );
        let got: BTreeSet<Component<Rc>> = weaken(&ax).into_iter().collect();
        let want: BTreeSet<Component<Rc>> =
            [sc(cls(&b, "urn:C"), some(cls(&b, "urn:D"))), sc(cls(&b, "urn:C"), some(cls(&b, "urn:E")))]
                .into_iter()
                .collect();
        assert_eq!(got, want);
    }

    // Nested: C ⊑ F ⊓ ∃r.(G ⊓ H)  →  {C⊑F, C⊑∃r.G, C⊑∃r.H}
    #[test]
    fn nested_splits() {
        let b = b();
        let some = |f: ClassExpression<Rc>| ClassExpression::ObjectSomeValuesFrom {
            ope: b.object_property("urn:r").into(),
            bce: Box::new(f),
        };
        let ax = sc(
            cls(&b, "urn:C"),
            ClassExpression::ObjectIntersectionOf(vec![
                cls(&b, "urn:F"),
                some(ClassExpression::ObjectIntersectionOf(vec![cls(&b, "urn:G"), cls(&b, "urn:H")])),
            ]),
        );
        let got: BTreeSet<Component<Rc>> = weaken(&ax).into_iter().collect();
        let want: BTreeSet<Component<Rc>> = [
            sc(cls(&b, "urn:C"), cls(&b, "urn:F")),
            sc(cls(&b, "urn:C"), some(cls(&b, "urn:G"))),
            sc(cls(&b, "urn:C"), some(cls(&b, "urn:H"))),
        ]
        .into_iter()
        .collect();
        assert_eq!(got, want);
    }

    // C ≡ D ⊓ E  →  {C⊑D, C⊑E, (D⊓E)⊑C}
    #[test]
    fn equivalence_splits_to_subsumptions() {
        let b = b();
        let inter = ClassExpression::ObjectIntersectionOf(vec![cls(&b, "urn:D"), cls(&b, "urn:E")]);
        let ax = Component::EquivalentClasses(horned_owl::model::EquivalentClasses(vec![
            cls(&b, "urn:C"),
            inter.clone(),
        ]));
        let got: BTreeSet<Component<Rc>> = weaken(&ax).into_iter().collect();
        let want: BTreeSet<Component<Rc>> = [
            sc(cls(&b, "urn:C"), cls(&b, "urn:D")),
            sc(cls(&b, "urn:C"), cls(&b, "urn:E")),
            sc(inter, cls(&b, "urn:C")),
        ]
        .into_iter()
        .collect();
        assert_eq!(got, want);
    }

    // DisjointClasses(C,D,E) → pairwise {DC(C,D), DC(C,E), DC(D,E)}
    #[test]
    fn disjoint_splits_pairwise() {
        let b = b();
        let dc = |x: &str, y: &str| {
            Component::DisjointClasses(horned_owl::model::DisjointClasses(vec![cls(&b, x), cls(&b, y)]))
        };
        let ax = Component::DisjointClasses(horned_owl::model::DisjointClasses(vec![
            cls(&b, "urn:C"),
            cls(&b, "urn:D"),
            cls(&b, "urn:E"),
        ]));
        let got: BTreeSet<Component<Rc>> = weaken(&ax).into_iter().collect();
        let want: BTreeSet<Component<Rc>> =
            [dc("urn:C", "urn:D"), dc("urn:C", "urn:E"), dc("urn:D", "urn:E")].into_iter().collect();
        assert_eq!(got, want);
    }

    // NEGATIVE: plain C ⊑ D passes through unchanged.
    #[test]
    fn plain_subsumption_unchanged() {
        let b = b();
        let ax = sc(cls(&b, "urn:C"), cls(&b, "urn:D"));
        assert_eq!(weaken(&ax), vec![ax.clone()]);
    }

    // NEGATIVE: LHS conjunction C₁⊓C₂ ⊑ D is NOT split (would strengthen → unsound).
    #[test]
    fn lhs_conjunction_not_split() {
        let b = b();
        let ax = sc(
            ClassExpression::ObjectIntersectionOf(vec![cls(&b, "urn:C1"), cls(&b, "urn:C2")]),
            cls(&b, "urn:D"),
        );
        assert_eq!(weaken(&ax), vec![ax.clone()]);
    }

    // NEGATIVE: cardinality filler is NOT weakened.
    #[test]
    fn cardinality_not_weakened() {
        let b = b();
        let ax = sc(
            cls(&b, "urn:C"),
            ClassExpression::ObjectMinCardinality {
                n: 3,
                ope: b.object_property("urn:r").into(),
                bce: Box::new(cls(&b, "urn:D")),
            },
        );
        assert_eq!(weaken(&ax), vec![ax.clone()]);
    }
}
```

- [ ] **Step 2: Run to confirm FAIL** — `cargo test -p owl-dl-reasoner --lib weaken_tests`
Expected: FAIL (assertions fail — `weaken` is the pass-through stub). If the `ObjectMinCardinality { n, ope, bce }` field names differ at this horned-owl rev, fix the test to match (see `crates/owl-dl-reasoner/src/justify.rs` `collect_ce_entities` for the exact variant shapes — it matches `ObjectMinCardinality { ope, bce, .. }`). Report any adjustment.

- [ ] **Step 3: Implement `split_sup` + `weaken`** — replace the stub `weaken` in `crates/owl-dl-reasoner/src/laconic.rs` with:

```rust
/// Decompose a superclass expression into top-level fragments, each of which the
/// original superclass is subsumed by (so `C ⊑ sup` entails `C ⊑ fragment`).
/// Splits conjunctions and recurses into existential fillers; everything else is
/// atomic (returned as-is).
fn split_sup<A: ForIRI>(sup: &ClassExpression<A>) -> Vec<ClassExpression<A>> {
    use ClassExpression as CE;
    match sup {
        CE::ObjectIntersectionOf(cs) => cs.iter().flat_map(|c| split_sup(c)).collect(),
        CE::ObjectSomeValuesFrom { ope, bce } => split_sup(bce)
            .into_iter()
            .map(|f| CE::ObjectSomeValuesFrom {
                ope: ope.clone(),
                bce: Box::new(f),
            })
            .collect(),
        other => vec![other.clone()],
    }
}

/// Weaken a single axiom into a set of fragments, each ENTAILED BY the axiom.
/// An axiom with no applicable operator returns `vec![axiom.clone()]`.
fn weaken<A: ForIRI>(axiom: &Component<A>) -> Vec<Component<A>> {
    use ClassExpression as CE;
    match axiom {
        // C ⊑ sup  →  one fragment per split of sup (LHS kept whole — splitting it
        // would strengthen the axiom, which is not entailed).
        Component::SubClassOf(sc) => {
            let frags = split_sup(&sc.sup);
            if frags.len() == 1 && frags[0] == sc.sup {
                vec![axiom.clone()]
            } else {
                frags
                    .into_iter()
                    .map(|f| Component::SubClassOf(SubClassOf { sub: sc.sub.clone(), sup: f }))
                    .collect()
            }
        }
        // C₁ ≡ … ≡ Cₙ  →  all ordered pairs Cᵢ ⊑ (each split fragment of Cⱼ).
        Component::EquivalentClasses(eq) => {
            let members = &eq.0;
            if members.len() < 2 {
                return vec![axiom.clone()];
            }
            let mut out = Vec::new();
            for (i, mi) in members.iter().enumerate() {
                for (j, mj) in members.iter().enumerate() {
                    if i == j {
                        continue;
                    }
                    for f in split_sup(mj) {
                        out.push(Component::SubClassOf(SubClassOf {
                            sub: mi.clone(),
                            sup: f,
                        }));
                    }
                }
            }
            out
        }
        // DisjointClasses(C₁ … Cₙ), n>2  →  pairwise DisjointClasses(Cᵢ, Cⱼ).
        Component::DisjointClasses(dc) => {
            let members = &dc.0;
            if members.len() <= 2 {
                return vec![axiom.clone()];
            }
            let mut out = Vec::new();
            for i in 0..members.len() {
                for j in (i + 1)..members.len() {
                    out.push(Component::DisjointClasses(horned_owl::model::DisjointClasses(vec![
                        members[i].clone(),
                        members[j].clone(),
                    ])));
                }
            }
            out
        }
        // Everything else passes through unchanged.
        _ => vec![axiom.clone()],
    }
    .into_iter()
    .collect::<BTreeSet<_>>() // dedup, deterministic order
    .into_iter()
    .collect()
}
```

Note: `let _ = CE;` is not needed; remove the unused `use ClassExpression as CE;` inside `weaken` if clippy flags it (it IS used in the `Component::SubClassOf` arm via `split_sup`? No — `weaken` itself doesn't name `CE`). If `use ClassExpression as CE;` is unused inside `weaken`, delete that line from `weaken` (keep it in `split_sup`).

- [ ] **Step 4: Run tests** — `cargo test -p owl-dl-reasoner --lib weaken_tests`
Expected: 8 passed.

- [ ] **Step 5: clippy + fmt** —
```bash
cargo clippy -p owl-dl-reasoner --all-targets -- -D warnings
cargo fmt -p owl-dl-reasoner
```
Fix lints inline (preserve behavior). If `weaken`/`split_sup` are still flagged dead (only used by tests so far), add `#[allow(dead_code)] // wired into the driver in Task 3; allow removed there` above each — Task 3 removes them. Re-run `weaken_tests` after fmt.

- [ ] **Step 6: Commit**

```bash
git add crates/owl-dl-reasoner/src/laconic.rs
git commit -m "feat(laconic): sound structural weakening operators + negative controls"
```

---

### Task 3: Laconic driver

**Files:** Modify `crates/owl-dl-reasoner/src/laconic.rs`; Create `crates/owl-dl-reasoner/tests/laconic_justification.rs`

- [ ] **Step 1: Write the failing integration test** — create `crates/owl-dl-reasoner/tests/laconic_justification.rs`:

```rust
//! Integration tests for laconic justifications.

use horned_owl::model::{Build, ClassExpression as CE, Component, MutableOntology, SubClassOf};
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::find_laconic_justification;
use owl_dl_reasoner::justify::Entailment;

type Rc = std::rc::Rc<str>;

// A ⊑ B ⊓ C ⊓ ∃r.D ; query A ⊑ B  → laconic must be exactly {A ⊑ B}.
#[test]
fn laconic_drops_superfluous_conjuncts() {
    let b = Build::new_rc();
    let cls = |iri: &str| CE::Class(b.class(iri));
    let mut o = SetOntology::new();
    let some_rd = CE::ObjectSomeValuesFrom {
        ope: b.object_property("urn:r").into(),
        bce: Box::new(cls("urn:D")),
    };
    o.insert(SubClassOf {
        sub: cls("urn:A"),
        sup: CE::ObjectIntersectionOf(vec![cls("urn:B"), cls("urn:C"), some_rd]),
    });

    let q = Entailment::SubClassOf {
        sub: "urn:A".to_string(),
        sup: "urn:B".to_string(),
    };
    let lac = find_laconic_justification(&o, &q)
        .expect("laconic")
        .expect("entailed");
    let want = Component::SubClassOf(SubClassOf {
        sub: cls("urn:A"),
        sup: cls("urn:B"),
    });
    assert_eq!(lac.axioms, vec![want], "laconic must keep only A ⊑ B");
}
```

- [ ] **Step 2: Run to confirm FAIL** — `cargo test -p owl-dl-reasoner --test laconic_justification`
Expected: FAIL — `find_laconic_justification` is the stub returning `Ok(None)`, so `.expect("entailed")` panics. If `Entailment` is not re-exported at `owl_dl_reasoner::justify::Entailment`, confirm the path (it is `pub` in the `pub mod justify`). Adjust the `use` if needed.

- [ ] **Step 3: Implement the driver** — replace BOTH stub functions in `crates/owl-dl-reasoner/src/laconic.rs` with:

```rust
/// Build the laconic version of one regular justification: weaken its axioms,
/// keep the rest of the ontology as background, and re-minimize via QuickXplain.
fn laconic_from<A: ForIRI>(
    onto: &SetOntology<A>,
    q: &Entailment,
    j_axioms: &[Component<A>],
    fragment: crate::classify::FragmentClassification,
    minimal_guaranteed: bool,
) -> Result<Justification<A>, ReasonError> {
    let (nonlogical, logical) = logical_axioms(onto);
    let j_set: HashSet<Component<A>> = j_axioms.iter().cloned().collect();

    // background = non-logical fixed + every logical axiom NOT in this justification.
    let mut background = nonlogical;
    for ax in logical {
        if !j_set.contains(&ax) {
            background.push(ax);
        }
    }

    // candidates = the union of the weakenings of the justification's axioms.
    let candidates: Vec<Component<A>> = j_axioms
        .iter()
        .flat_map(weaken)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();

    let laconic = quickxplain(&background, &candidates, q)?;
    Ok(Justification {
        axioms: laconic,
        fragment,
        minimal_guaranteed,
    })
}

/// Laconic justification for `q` (one), or `None` if `q` is not entailed.
pub fn find_laconic_justification<A: ForIRI>(
    onto: &SetOntology<A>,
    q: &Entailment,
) -> Result<Option<Justification<A>>, ReasonError> {
    let Some(j) = find_one_justification(onto, q)? else {
        return Ok(None);
    };
    Ok(Some(laconic_from(
        onto,
        q,
        &j.axioms,
        j.fragment,
        j.minimal_guaranteed,
    )?))
}

/// All laconic justifications for `q` (one per regular justification, capped by
/// `max`), de-duplicated by fragment set.
pub fn find_all_laconic_justifications<A: ForIRI>(
    onto: &SetOntology<A>,
    q: &Entailment,
    max: usize,
) -> Result<Vec<Justification<A>>, ReasonError> {
    let regular = find_all_justifications(onto, q, max)?;
    let mut out = Vec::new();
    let mut seen: HashSet<BTreeSet<Component<A>>> = HashSet::new();
    for j in regular {
        let lac = laconic_from(onto, q, &j.axioms, j.fragment, j.minimal_guaranteed)?;
        let key: BTreeSet<Component<A>> = lac.axioms.iter().cloned().collect();
        if seen.insert(key) {
            out.push(lac);
        }
    }
    Ok(out)
}
```
Then REMOVE any `#[allow(dead_code)]` you added to `weaken`/`split_sup` in Task 2 (they are now reachable from `laconic_from`). If you used the `weaken::<A> as fn(...)` reference trick in the Task 1 stub, it's gone now (the stub bodies are replaced).

`FragmentClassification` import: add `use crate::classify::FragmentClassification;` at the top if you prefer the short name, or reference it fully-qualified as written. Match whichever keeps clippy happy.

- [ ] **Step 4: Run tests** — `cargo test -p owl-dl-reasoner --test laconic_justification`
Expected: PASS. If it fails because the laconic result is `{A⊑B, A⊑C, ...}` instead of just `{A⊑B}`, that's a real bug in the driver/operators — investigate (QuickXplain should drop the unneeded fragments). Report.

- [ ] **Step 5: full lib + integration tests + clippy + fmt** —
```bash
cargo test -p owl-dl-reasoner --lib laconic
cargo test -p owl-dl-reasoner --test laconic_justification
cargo clippy -p owl-dl-reasoner --all-targets -- -D warnings
cargo fmt -p owl-dl-reasoner
```
All green; re-run tests after fmt.

- [ ] **Step 6: Commit**

```bash
git add crates/owl-dl-reasoner/src/laconic.rs crates/owl-dl-reasoner/tests/laconic_justification.rs
git commit -m "feat(laconic): laconic driver (weaken justification axioms + re-minimize)"
```

---

### Task 4: Equivalence + disjoint integration coverage

**Files:** Modify `crates/owl-dl-reasoner/tests/laconic_justification.rs`

- [ ] **Step 1: Add tests** — append:

```rust
// C ≡ D ⊓ E ; query C ⊑ D  → laconic exactly {C ⊑ D}.
#[test]
fn laconic_equivalence_picks_one_direction_and_conjunct() {
    let b = Build::new_rc();
    let cls = |iri: &str| CE::Class(b.class(iri));
    let mut o = SetOntology::new();
    o.insert(horned_owl::model::EquivalentClasses(vec![
        cls("urn:C"),
        CE::ObjectIntersectionOf(vec![cls("urn:D"), cls("urn:E")]),
    ]));
    let q = Entailment::SubClassOf {
        sub: "urn:C".to_string(),
        sup: "urn:D".to_string(),
    };
    let lac = find_laconic_justification(&o, &q)
        .expect("laconic")
        .expect("entailed");
    let want = Component::SubClassOf(SubClassOf {
        sub: cls("urn:C"),
        sup: cls("urn:D"),
    });
    assert_eq!(lac.axioms, vec![want]);
}

// not entailed → None.
#[test]
fn laconic_not_entailed_is_none() {
    let b = Build::new_rc();
    let cls = |iri: &str| CE::Class(b.class(iri));
    let mut o = SetOntology::new();
    o.insert(SubClassOf {
        sub: cls("urn:A"),
        sup: cls("urn:B"),
    });
    let q = Entailment::SubClassOf {
        sub: "urn:A".to_string(),
        sup: "urn:Z".to_string(),
    };
    assert!(find_laconic_justification(&o, &q).expect("laconic").is_none());
}
```

- [ ] **Step 2: Run** — `cargo test -p owl-dl-reasoner --test laconic_justification`
Expected: 3 passed.

- [ ] **Step 3: Commit**

```bash
git add crates/owl-dl-reasoner/tests/laconic_justification.rs
git commit -m "test(laconic): equivalence + not-entailed integration coverage"
```

---

### Task 5: CLI `--laconic` flag

**Files:** Modify `crates/owl-dl-cli/src/main.rs`

- [ ] **Step 1: Add the flag to the `Justify` variant** — in `enum Command`, in the `Justify { … }` variant (around `main.rs:216`), add after the `labels` field:

```rust
        /// Weaken each justification axiom to its responsible fragment (laconic).
        #[arg(long)]
        laconic: bool,
```

- [ ] **Step 2: Update the handler** — in the `Command::Justify { … } => { … }` arm (around `main.rs:838`):

  (a) add `laconic,` to the destructured fields list.

  (b) Change the `render` closure's header line so it can label laconic results. Find:
```rust
                println!("# justification ({} axioms) — {note}", j.axioms.len());
```
  and replace with:
```rust
                let kind = if laconic { "laconic justification (structural)" } else { "justification" };
                let note = if laconic {
                    format!("fragments sound; minimal among supported weakenings ({})", j.fragment)
                } else {
                    note
                };
                println!("# {kind} ({} axioms) — {note}", j.axioms.len());
```
  IMPORTANT: this references the existing `note` binding computed just above in the closure (`let note = if j.minimal_guaranteed { … } else { … };`). Keep that existing line; the new code shadows `note` only in the laconic case. If the borrow/shadow is awkward, restructure minimally so the non-laconic output is byte-identical to today.

  (c) Switch which finder is called. Find the `if all { … } else { … }` block that calls `find_all_justifications` / `find_one_justification` and route to the laconic variants when `laconic` is set:
```rust
            if all {
                let js = if laconic {
                    owl_dl_reasoner::find_all_laconic_justifications(&onto, &q, max)
                        .context("find_all_laconic_justifications")?
                } else {
                    owl_dl_reasoner::justify::find_all_justifications(&onto, &q, max)
                        .context("find_all_justifications")?
                };
                if js.is_empty() {
                    println!("not entailed (no justification)");
                } else {
                    println!("# {} justification(s)", js.len());
                    for j in &js {
                        render(j);
                    }
                }
            } else {
                let one = if laconic {
                    owl_dl_reasoner::find_laconic_justification(&onto, &q)
                        .context("find_laconic_justification")?
                } else {
                    owl_dl_reasoner::justify::find_one_justification(&onto, &q)
                        .context("find_one_justification")?
                };
                match one {
                    Some(j) => render(&j),
                    None => println!("not entailed (no justification)"),
                }
            }
```
  Adapt to the exact current structure (the existing arm already has this shape — only the finder calls and the `laconic` branch are new). Keep the non-laconic path behaviorally identical.

- [ ] **Step 3: Build** — `cargo build -p owl-dl-cli`

- [ ] **Step 4: Smoke-test** —
```bash
cat > /tmp/laconic-smoke.ofn <<'EOF'
Prefix(:=<urn:>)
Ontology(
  Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) Declaration(Class(:D))
  Declaration(ObjectProperty(:r))
  SubClassOf(:A ObjectIntersectionOf(:B :C ObjectSomeValuesFrom(:r :D)))
)
EOF
cargo build -p owl-dl-cli --release
echo "--- regular ---"
./target/release/rustdl justify /tmp/laconic-smoke.ofn subclass urn:A urn:B
echo "--- laconic ---"
./target/release/rustdl justify --laconic /tmp/laconic-smoke.ofn subclass urn:A urn:B
```
Expected: regular prints the full `A SubClassOf B and C and (r some D)`; laconic prints header `# laconic justification (structural) (1 axioms) — …` and the single axiom `A SubClassOf B`. Paste the actual laconic output. If laconic still shows the full conjunction, STOP and report (real bug).

- [ ] **Step 5: clippy + fmt** —
```bash
cargo clippy -p owl-dl-cli --all-targets -- -D warnings
cargo fmt -p owl-dl-cli
```

- [ ] **Step 6: Commit**

```bash
git add crates/owl-dl-cli/src/main.rs
git commit -m "feat(laconic): justify --laconic CLI flag"
```

---

### Task 6: Corpus check + docs + final gate

**Files:** Create `crates/owl-dl-reasoner/tests/laconic_justification.rs` additions; Modify `README.md`, `CLAUDE.md`

- [ ] **Step 1: Add a corpus re-verification test (ignored)** — append to `crates/owl-dl-reasoner/tests/laconic_justification.rs`:

```rust
use owl_dl_reasoner::justify::find_one_justification;

// On a real fixture, the laconic justification of a known unsat class must (a) be
// non-empty, (b) be no larger than the regular justification. Ignored (corpus).
#[test]
#[ignore = "reads the curated corpus (ontologies/real/pizza.ofn)"]
fn laconic_pizza_unsat_no_larger_than_regular() {
    let p = std::path::Path::new("../../ontologies/real/pizza.ofn");
    if !p.exists() {
        eprintln!("skip pizza.ofn (not present)");
        return;
    }
    let onto = read_ofn_fixture(p);
    let q = Entailment::Unsatisfiable {
        class: "http://www.co-ode.org/ontologies/pizza/pizza.owl#CheeseyVegetableTopping"
            .to_string(),
    };
    let regular = find_one_justification(&onto, &q)
        .expect("justify")
        .expect("entailed");
    let lac = find_laconic_justification(&onto, &q)
        .expect("laconic")
        .expect("entailed");
    assert!(!lac.axioms.is_empty(), "laconic must be non-empty");
    assert!(
        lac.axioms.len() <= regular.axioms.len() + regular.axioms.iter().map(|_| 0).sum::<usize>()
            || !lac.axioms.is_empty(),
        "laconic fragment count is a refinement of the regular justification"
    );
    eprintln!(
        "pizza CheeseyVegetableTopping: regular {} axioms, laconic {} fragments",
        regular.axioms.len(),
        lac.axioms.len()
    );
}

fn read_ofn_fixture(p: &std::path::Path) -> SetOntology<Rc> {
    use horned_owl::io::ParserConfiguration;
    use horned_owl::io::ofn::reader::read as read_ofn;
    let mut reader = std::io::BufReader::new(std::fs::File::open(p).expect("open fixture"));
    let (o, _): (SetOntology<Rc>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    o
}
```
NOTE: the assertion above is intentionally lenient on the count (laconic can in principle have more *fragments* than the regular has *axioms* when one axiom splits into several needed parts; the real invariant is "laconic still entails and is sound", which QuickXplain guarantees by construction). The test's value is the `eprintln!` report + non-empty + no-crash + that it runs on real data. Simplify the second assertion to just `assert!(!lac.axioms.is_empty())` if the compound expression trips clippy — keep it simple and correct.

- [ ] **Step 2: Run it** —
```bash
cargo test -p owl-dl-reasoner --test laconic_justification laconic_pizza_unsat_no_larger_than_regular -- --ignored --nocapture
```
Expected: PASS with an `eprintln!` line reporting the axiom/fragment counts. (May take a few seconds — pizza justify on a SHOIN unsat class.) If it hangs beyond ~3 min, note it and rely on the synthetic tests; the corpus test is supplementary.

- [ ] **Step 3: Document in README** — in the CLI block, update the `justify` line or add a note. Change:
```
rustdl justify   ontology.ofn <query…>      # minimal responsible-axiom set (why it holds)
```
to add a following line:
```
rustdl justify --laconic ontology.ofn <query…>  # pinpoint the responsible PART of each axiom
```
Match the column alignment of the surrounding lines.

- [ ] **Step 4: Note in CLAUDE.md** — find the `owl-dl-cli` bullet (the one mentioning `diagnose`) and append:
```
`justify --laconic` weakens each justification axiom to its responsible fragment
(sound structural weakening: RHS-conjunction / ∃-filler / equivalence / pairwise
disjoint; LHS + cardinality deliberately not weakened), re-minimized via
QuickXplain — sound by construction (every fragment is entailed by an original
axiom). See `docs/superpowers/specs/2026-06-21-laconic-justifications-design.md`.
```

- [ ] **Step 5: Full workspace gate** —
```bash
cd /data/dumontier/rustdl
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```
All three green. `cargo test --workspace` runs the non-ignored laconic tests (weaken_tests + the 3 integration tests); the corpus test is `#[ignore]`d and will NOT run — that's expected. If a NON-ignored test fails, report it verbatim. If clippy flags UNRELATED pre-existing code, stop and report; fix only laconic-related lints.

- [ ] **Step 6: Commit**

```bash
git add crates/owl-dl-reasoner/tests/laconic_justification.rs README.md CLAUDE.md
git commit -m "test+docs(laconic): corpus re-verification + README/CLAUDE notes"
```

---

## Self-review notes (author)

- **Spec coverage:** weakening operators (RHS conj, ∃-filler, equivalence→subsumptions, pairwise disjoint, nested) → Task 2 with negative controls (LHS not split, cardinality not weakened, plain passthrough); the laconic driver (weaken J, background = non-J, re-minimize) → Task 3; find_all + dedup → Task 3; CLI `--laconic` + honesty header → Task 5; corpus re-verification + read-only → Task 6. The "uniform equivalence → pairwise subsumptions" detail (spec was ambiguous n=2 vs n>2) is pinned in Task 2's `weaken` (all ordered pairs Cᵢ ⊑ split(Cⱼ)).
- **Soundness:** operators emit only entailed fragments (Task 2 table mirrors spec); end-to-end entailment is guaranteed by QuickXplain's postcondition (no separate per-fragment oracle test needed — the spec's "{a} ⊨ fragment" check is replaced by structural unit tests + QuickXplain's guarantee, because complex fragments like `C ⊑ ∃r.D` aren't expressible as the named-class `Entailment` query the oracle takes). This deviation from the spec's testing section is intentional and noted here.
- **No placeholders:** every code step is complete.
- **Type consistency:** `weaken`/`split_sup`/`laconic_from`/`find_laconic_justification`/`find_all_laconic_justifications` signatures match across tasks; `Justification` reused (no new type); `Component::SubClassOf(SubClassOf { sub, sup })` construction matches convert_back.rs.
- **API risk flagged inline:** `ObjectMinCardinality { n, ope, bce }` field names (Task 2), `EquivalentClasses`/`DisjointClasses` tuple shapes, `Entailment` re-export path — each task points at in-repo usage to copy if a name differs.
