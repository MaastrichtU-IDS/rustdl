# `rustdl diagnose` (root/derived unsatisfiability) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `rustdl diagnose` command that partitions an ontology's unsatisfiable classes into **root** (genuine causes) and **derived** (collateral), justifies the roots, and on an inconsistent ontology reports the responsible axioms.

**Architecture:** A new read-only module `crates/owl-dl-reasoner/src/diagnose.rs` computes the partition from (a) the already-sound classified unsatisfiable-class set and (b) a *stingy* structural dependency graph over the ontology's logical axioms (edges only for unsat-*forcing* positions: told-subsumption, `∃r.D`, conjuncts). Roots are the sink nodes of that graph (a node whose every reachable node reaches it back). A new `owl-dl-cli` subcommand orchestrates consistency → classify → partition → per-root justification, reusing the shipped `justify` machinery for rendering.

**Tech Stack:** Rust (edition 2024), horned-owl model types, the existing `owl-dl-reasoner` crate (`classify`, `is_consistent`, `justify::{find_one_justification, find_all_justifications, logical_axioms, Entailment, Justification}`).

**Spec:** `docs/superpowers/specs/2026-06-21-diagnose-root-derived-unsat-design.md`

**Branch:** `feat/diagnose-root-derived-unsat`

---

## Key facts the implementer must know (verified against the codebase)

- `crate::classify::classify(&onto) -> Result<Classification, ReasonError>`; `Classification::unsatisfiable_classes(&self) -> Vec<&str>` returns the sorted IRIs of classes equivalent to `⊥`.
- `crate::is_consistent(&onto) -> Result<bool, ReasonError>` (defined in `lib.rs`).
- `crate::justify::logical_axioms(&onto) -> (Vec<Component<A>>, Vec<Component<A>>)` — the **second** tuple element is the logical-axiom set (the first is declarations/annotations).
- `crate::justify::Entailment::Unsatisfiable { class: String }` and `Entailment::Inconsistent`.
- `crate::justify::find_one_justification(&onto, &Entailment) -> Result<Option<Justification<A>>, ReasonError>`; `find_all_justifications(&onto, &q, max) -> Result<Vec<Justification<A>>, ReasonError>`.
- `Justification<A> { pub axioms: Vec<Component<A>>, pub fragment: FragmentClassification, pub minimal_guaranteed: bool }`.
- horned-owl `ClassExpression<A>` variants used here: `Class(c)` where the IRI is `c.0.as_ref().to_string()`; `ObjectIntersectionOf(Vec<ClassExpression>)`; `ObjectSomeValuesFrom { ope, bce }` (recurse into `bce`). All other variants are NOT unsat-forcing and must NOT be recursed.
- horned-owl `Component<A>` variants used here: `SubClassOf(a)` with `a.sub` / `a.sup` (both `ClassExpression`); `EquivalentClasses(a)` with `a.0: Vec<ClassExpression>`.
- The CLI uses the type alias `RcStr` for `A` and these helpers (already in `crates/owl-dl-cli/src/main.rs`): `parse_ofn_with_pm(&file) -> Result<(SetOntology<RcStr>, PrefixMapping)>`, `build_label_map(&onto)`, `local_name(&iri)`, and `Component::as_manchester_with_prefixes(&pm)`. The `justify` handler (around `main.rs:838`) is the rendering template to mirror.
- Module declarations live near `crates/owl-dl-reasoner/src/lib.rs:46` (`pub mod justify;`).

## File structure

- **Create** `crates/owl-dl-reasoner/src/diagnose.rs` — the partition engine. Responsibilities: dependency-edge extraction, reachability, root/derived partition, and the top-level `diagnose()` orchestration returning a non-generic `Diagnosis` (all `String`s).
- **Modify** `crates/owl-dl-reasoner/src/lib.rs` — add `pub mod diagnose;` and re-export `diagnose::{diagnose, Diagnosis, DerivedClass}`.
- **Create** `crates/owl-dl-reasoner/tests/diagnose_partition.rs` — integration tests (cascade fixture, inconsistency, conservation invariant on corpus).
- **Modify** `crates/owl-dl-cli/src/main.rs` — add the `Diagnose` subcommand variant + handler.
- **Modify** `CLAUDE.md` and `README.md` — document the new command (final task).

---

### Task 1: Branch + empty module wired in

**Files:**
- Modify: `crates/owl-dl-reasoner/src/lib.rs:46`
- Create: `crates/owl-dl-reasoner/src/diagnose.rs`

- [ ] **Step 1: Create the branch**

```bash
cd /data/dumontier/rustdl
git checkout main
git checkout -b feat/diagnose-root-derived-unsat
```

- [ ] **Step 2: Create the module file with the public data types (no logic yet)**

Create `crates/owl-dl-reasoner/src/diagnose.rs`:

```rust
//! Root/derived unsatisfiability diagnosis: partition the classified
//! unsatisfiable classes into *root* causes and *derived* collateral, using a
//! stingy structural dependency graph (edges only for unsat-forcing positions).
//! Read-only over classification — adds no entailments, so FP=0 is untouched.

use std::collections::{BTreeMap, BTreeSet};

use horned_owl::model::{ClassExpression, Component, ForIRI};
use horned_owl::ontology::set::SetOntology;

use crate::ReasonError;

/// A derived unsatisfiable class and the root cause(s) it depends on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DerivedClass {
    /// IRI of the derived (collateral) unsatisfiable class.
    pub iri: String,
    /// IRIs of the root class(es) it transitively depends on.
    pub roots: Vec<String>,
}

/// The result of diagnosing an ontology.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnosis {
    /// Whether the ontology is consistent. When `false`, `roots`/`derived` are
    /// empty and the caller should justify the inconsistency directly.
    pub consistent: bool,
    /// Root unsatisfiable classes (IRIs), sorted. Empty if consistent and coherent.
    pub roots: Vec<String>,
    /// Derived unsatisfiable classes, each with its root(s), sorted by IRI.
    pub derived: Vec<DerivedClass>,
    /// Every unsatisfiable class (roots ∪ derived), sorted — the conservation set.
    pub all_unsat: Vec<String>,
    /// For each root IRI, the derived classes that depend on it, sorted.
    pub root_derives: BTreeMap<String, Vec<String>>,
}
```

- [ ] **Step 3: Wire the module into lib.rs**

In `crates/owl-dl-reasoner/src/lib.rs`, immediately after the line `pub mod justify;` (around line 46), add:

```rust
pub mod diagnose;
```

And after the `pub use realize::{...};` block (around line 57), add:

```rust
pub use diagnose::{DerivedClass, Diagnosis, diagnose};
```

- [ ] **Step 4: Add a placeholder `diagnose` fn so the re-export resolves**

Append to `crates/owl-dl-reasoner/src/diagnose.rs`:

```rust
/// Diagnose `onto`: consistency, then the root/derived unsatisfiability partition.
///
/// Read-only over classification; never mutates the ontology.
pub fn diagnose<A: ForIRI>(onto: &SetOntology<A>) -> Result<Diagnosis, ReasonError> {
    // Filled in by Task 4.
    let _ = onto;
    Ok(Diagnosis {
        consistent: true,
        roots: Vec::new(),
        derived: Vec::new(),
        all_unsat: Vec::new(),
        root_derives: BTreeMap::new(),
    })
}
```

- [ ] **Step 5: Build to verify it compiles**

Run: `cargo build -p owl-dl-reasoner`
Expected: compiles (warnings about unused imports `ClassExpression`, `Component`, `BTreeSet` are acceptable for now — they are used in Task 2).

- [ ] **Step 6: Commit**

```bash
git add crates/owl-dl-reasoner/src/diagnose.rs crates/owl-dl-reasoner/src/lib.rs
git commit -m "feat(diagnose): module skeleton + Diagnosis types"
```

---

### Task 2: Dependency-edge extractor

The core soundness unit: from the logical axioms, build edges `C -> D` (meaning "C depends on unsat class D") **only** for unsat-forcing positions.

**Files:**
- Modify: `crates/owl-dl-reasoner/src/diagnose.rs`

- [ ] **Step 1: Write the failing tests**

Append to `crates/owl-dl-reasoner/src/diagnose.rs`:

```rust
#[cfg(test)]
mod edge_tests {
    use super::*;
    use horned_owl::model::Build;
    use horned_owl::ontology::set::SetOntology;
    use horned_owl::model::MutableOntology;

    type B = Build<std::rc::Rc<str>>;

    fn b() -> B {
        Build::new_rc()
    }

    fn class(b: &B, iri: &str) -> ClassExpression<std::rc::Rc<str>> {
        ClassExpression::Class(b.class(iri))
    }

    // C SubClassOf D  (told subsumption) → edge C->D
    #[test]
    fn told_subsumption_is_edge() {
        let b = b();
        let mut o = SetOntology::new();
        o.insert(horned_owl::model::SubClassOf {
            sub: class(&b, "urn:C"),
            sup: class(&b, "urn:D"),
            ann: Default::default(),
        });
        let unsat: BTreeSet<String> = ["urn:C", "urn:D"].iter().map(|s| s.to_string()).collect();
        let edges = build_dep_edges(&unsat, &logical(&o));
        assert_eq!(edges.get("urn:C"), Some(&set(&["urn:D"])));
    }

    // C SubClassOf exists r.D → edge C->D
    #[test]
    fn existential_filler_is_edge() {
        let b = b();
        let sup = ClassExpression::ObjectSomeValuesFrom {
            ope: b.object_property("urn:r").into(),
            bce: Box::new(class(&b, "urn:D")),
        };
        let mut o = SetOntology::new();
        o.insert(horned_owl::model::SubClassOf { sub: class(&b, "urn:C"), sup, ann: Default::default() });
        let unsat = set(&["urn:C", "urn:D"]);
        let edges = build_dep_edges(&unsat, &logical(&o));
        assert_eq!(edges.get("urn:C"), Some(&set(&["urn:D"])));
    }

    // C SubClassOf (D and E) → edges C->D, C->E (both unsat)
    #[test]
    fn conjunct_is_edge() {
        let b = b();
        let sup = ClassExpression::ObjectIntersectionOf(vec![class(&b, "urn:D"), class(&b, "urn:E")]);
        let mut o = SetOntology::new();
        o.insert(horned_owl::model::SubClassOf { sub: class(&b, "urn:C"), sup, ann: Default::default() });
        let unsat = set(&["urn:C", "urn:D", "urn:E"]);
        let edges = build_dep_edges(&unsat, &logical(&o));
        assert_eq!(edges.get("urn:C"), Some(&set(&["urn:D", "urn:E"])));
    }

    // NEGATIVE: C SubClassOf forall r.D → NO edge (∀ does not force C unsat)
    #[test]
    fn forall_is_not_edge() {
        let b = b();
        let sup = ClassExpression::ObjectAllValuesFrom {
            ope: b.object_property("urn:r").into(),
            bce: Box::new(class(&b, "urn:D")),
        };
        let mut o = SetOntology::new();
        o.insert(horned_owl::model::SubClassOf { sub: class(&b, "urn:C"), sup, ann: Default::default() });
        let unsat = set(&["urn:C", "urn:D"]);
        let edges = build_dep_edges(&unsat, &logical(&o));
        assert_eq!(edges.get("urn:C"), None);
    }

    // NEGATIVE: DisjointClasses(C, D) → NO edge
    #[test]
    fn disjoint_is_not_edge() {
        let b = b();
        let mut o = SetOntology::new();
        o.insert(horned_owl::model::DisjointClasses(vec![class(&b, "urn:C"), class(&b, "urn:D")]));
        let unsat = set(&["urn:C", "urn:D"]);
        let edges = build_dep_edges(&unsat, &logical(&o));
        assert!(edges.is_empty());
    }

    // NEGATIVE: target D not unsat → NO edge (only edges to unsat classes count)
    #[test]
    fn edge_target_must_be_unsat() {
        let b = b();
        let mut o = SetOntology::new();
        o.insert(horned_owl::model::SubClassOf {
            sub: class(&b, "urn:C"),
            sup: class(&b, "urn:D"),
            ann: Default::default(),
        });
        let unsat = set(&["urn:C"]); // D is satisfiable
        let edges = build_dep_edges(&unsat, &logical(&o));
        assert!(edges.get("urn:C").is_none());
    }

    // NEGATIVE: self-edge suppressed (C SubClassOf (C and D))
    #[test]
    fn self_edge_suppressed() {
        let b = b();
        let sup = ClassExpression::ObjectIntersectionOf(vec![class(&b, "urn:C"), class(&b, "urn:D")]);
        let mut o = SetOntology::new();
        o.insert(horned_owl::model::SubClassOf { sub: class(&b, "urn:C"), sup, ann: Default::default() });
        let unsat = set(&["urn:C", "urn:D"]);
        let edges = build_dep_edges(&unsat, &logical(&o));
        assert_eq!(edges.get("urn:C"), Some(&set(&["urn:D"]))); // C->C dropped
    }

    // EquivalentClasses(C, D) where both unsat → edge C->D and D->C
    #[test]
    fn equivalent_classes_both_directions() {
        let b = b();
        let mut o = SetOntology::new();
        o.insert(horned_owl::model::EquivalentClasses(vec![class(&b, "urn:C"), class(&b, "urn:D")]));
        let unsat = set(&["urn:C", "urn:D"]);
        let edges = build_dep_edges(&unsat, &logical(&o));
        assert_eq!(edges.get("urn:C"), Some(&set(&["urn:D"])));
        assert_eq!(edges.get("urn:D"), Some(&set(&["urn:C"])));
    }

    // helpers
    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }
    fn logical(o: &SetOntology<std::rc::Rc<str>>) -> Vec<Component<std::rc::Rc<str>>> {
        crate::justify::logical_axioms(o).1
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p owl-dl-reasoner --lib edge_tests`
Expected: FAIL — `build_dep_edges` is not defined.

- [ ] **Step 3: Implement the extractor**

Append to `crates/owl-dl-reasoner/src/diagnose.rs` (before the `#[cfg(test)]` block):

```rust
/// Collect the named classes that occur in *unsat-forcing* positions of `ce`.
///
/// Forcing positions (a `⊥` here forces the whole expression to `⊥`):
/// - a named class at top level (`C ⊑ D`),
/// - every conjunct of an intersection (`C ⊑ D ⊓ …`),
/// - the filler of an existential (`C ⊑ ∃r.D`, since `∃r.⊥ ≡ ⊥`),
/// - recursively, the same inside nested `⊓`/`∃`.
///
/// Every other constructor (`∀`, `⊔`, `¬`, cardinality, `hasValue`, data) is
/// deliberately NOT recursed: its filler being `⊥` does not force the parent
/// `⊥`. Omitting such an edge only ever over-reports roots (safe); including a
/// spurious one would hide a root (the dangerous failure we avoid).
fn unsat_forcing_classes<A: ForIRI>(ce: &ClassExpression<A>, out: &mut BTreeSet<String>) {
    use ClassExpression as CE;
    match ce {
        CE::Class(c) => {
            out.insert(c.0.as_ref().to_string());
        }
        CE::ObjectIntersectionOf(cs) => {
            for c in cs {
                unsat_forcing_classes(c, out);
            }
        }
        CE::ObjectSomeValuesFrom { bce, .. } => {
            unsat_forcing_classes(bce, out);
        }
        // All other constructors are not unsat-forcing — stop.
        _ => {}
    }
}

/// Build the dependency graph over the unsatisfiable set `unsat`.
///
/// `edges[C]` = the set of unsatisfiable classes `D ≠ C` such that `C`'s
/// unsatisfiability is *forced* by `D`'s (an edge `C → D`). Only `SubClassOf`
/// (named-class LHS) and `EquivalentClasses` axioms contribute; only targets in
/// `unsat` are kept; self-edges are dropped.
fn build_dep_edges<A: ForIRI>(
    unsat: &BTreeSet<String>,
    logical: &[Component<A>],
) -> BTreeMap<String, BTreeSet<String>> {
    let mut edges: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    let mut add = |src: &str, targets: BTreeSet<String>| {
        if !unsat.contains(src) {
            return;
        }
        for t in targets {
            if t != src && unsat.contains(&t) {
                edges.entry(src.to_string()).or_default().insert(t);
            }
        }
    };

    for c in logical {
        match c {
            Component::SubClassOf(a) => {
                if let ClassExpression::Class(sub) = &a.sub {
                    let mut forced = BTreeSet::new();
                    unsat_forcing_classes(&a.sup, &mut forced);
                    add(sub.0.as_ref(), forced);
                }
            }
            Component::EquivalentClasses(a) => {
                // Each named-class member C ⊑ every other member: edges from C to
                // the forcing classes of every other member.
                for (i, member) in a.0.iter().enumerate() {
                    let ClassExpression::Class(c) = member else {
                        continue;
                    };
                    let mut forced = BTreeSet::new();
                    for (j, other) in a.0.iter().enumerate() {
                        if i != j {
                            unsat_forcing_classes(other, &mut forced);
                        }
                    }
                    add(c.0.as_ref(), forced);
                }
            }
            _ => {}
        }
    }

    edges
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p owl-dl-reasoner --lib edge_tests`
Expected: PASS (9 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/owl-dl-reasoner/src/diagnose.rs
git commit -m "feat(diagnose): stingy dependency-edge extractor + negative controls"
```

---

### Task 3: Reachability + root/derived partition

Root = a node whose every forward-reachable node reaches it back (sink SCC member). No external SCC library — O(n²) reachability is fine (the unsat set is small).

**Files:**
- Modify: `crates/owl-dl-reasoner/src/diagnose.rs`

- [ ] **Step 1: Write the failing tests**

Append a new test module to `crates/owl-dl-reasoner/src/diagnose.rs`:

```rust
#[cfg(test)]
mod partition_tests {
    use super::*;

    fn edges(pairs: &[(&str, &[&str])]) -> BTreeMap<String, BTreeSet<String>> {
        pairs
            .iter()
            .map(|(s, ts)| (s.to_string(), ts.iter().map(|t| t.to_string()).collect()))
            .collect()
    }
    fn unsat(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    // Chain C->D->E: E is the only root; C,D derived ⇐ E.
    #[test]
    fn chain_has_single_root() {
        let u = unsat(&["C", "D", "E"]);
        let g = edges(&[("C", &["D"]), ("D", &["E"])]);
        let p = partition(&u, &g);
        assert_eq!(p.roots, vec!["E".to_string()]);
        let derived: BTreeMap<_, _> = p
            .derived
            .iter()
            .map(|d| (d.iri.clone(), d.roots.clone()))
            .collect();
        assert_eq!(derived["C"], vec!["E".to_string()]);
        assert_eq!(derived["D"], vec!["E".to_string()]);
    }

    // Independent roots: A and B both depend on nobody.
    #[test]
    fn independent_roots() {
        let u = unsat(&["A", "B"]);
        let g = edges(&[]);
        let p = partition(&u, &g);
        assert_eq!(p.roots, vec!["A".to_string(), "B".to_string()]);
        assert!(p.derived.is_empty());
    }

    // Cycle C<->D with no outside edge: both are co-roots.
    #[test]
    fn cycle_members_are_co_roots() {
        let u = unsat(&["C", "D"]);
        let g = edges(&[("C", &["D"]), ("D", &["C"])]);
        let p = partition(&u, &g);
        assert_eq!(p.roots, vec!["C".to_string(), "D".to_string()]);
        assert!(p.derived.is_empty());
    }

    // Cycle C<->D that both depend on root E: C,D derived ⇐ E; E root.
    #[test]
    fn cycle_depending_on_root() {
        let u = unsat(&["C", "D", "E"]);
        let g = edges(&[("C", &["D", "E"]), ("D", &["C"])]);
        let p = partition(&u, &g);
        assert_eq!(p.roots, vec!["E".to_string()]);
        let iris: Vec<String> = p.derived.iter().map(|d| d.iri.clone()).collect();
        assert_eq!(iris, vec!["C".to_string(), "D".to_string()]);
    }

    // Conservation: roots ∪ derived == all_unsat, always.
    #[test]
    fn conservation_holds() {
        let u = unsat(&["A", "B", "C", "D"]);
        let g = edges(&[("A", &["B"]), ("C", &["D"])]);
        let p = partition(&u, &g);
        let mut union: BTreeSet<String> = p.roots.iter().cloned().collect();
        union.extend(p.derived.iter().map(|d| d.iri.clone()));
        assert_eq!(union, u);
    }

    // root_derives: root B is depended on by A.
    #[test]
    fn root_derives_populated() {
        let u = unsat(&["A", "B"]);
        let g = edges(&[("A", &["B"])]);
        let p = partition(&u, &g);
        assert_eq!(p.root_derives.get("B"), Some(&vec!["A".to_string()]));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p owl-dl-reasoner --lib partition_tests`
Expected: FAIL — `partition` is not defined.

- [ ] **Step 3: Implement reachability + partition**

Append to `crates/owl-dl-reasoner/src/diagnose.rs` (before the test modules):

```rust
/// Forward-reachable set of `start` (including `start`) over `edges`.
fn reachable(start: &str, edges: &BTreeMap<String, BTreeSet<String>>) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut stack = vec![start.to_string()];
    while let Some(n) = stack.pop() {
        if seen.insert(n.clone()) {
            if let Some(succ) = edges.get(&n) {
                for s in succ {
                    if !seen.contains(s) {
                        stack.push(s.clone());
                    }
                }
            }
        }
    }
    seen
}

/// Partition `unsat` into roots and derived using the dependency `edges`.
///
/// A class `C` is a **root** iff every node reachable from `C` reaches `C` back
/// (i.e. `C`'s reachable set is contained in its strongly-connected component —
/// `C` is a sink-SCC member, depending on nothing strictly more root). Cycle
/// members with no outside dependency are therefore all roots.
fn partition(
    unsat: &BTreeSet<String>,
    edges: &BTreeMap<String, BTreeSet<String>>,
) -> Diagnosis {
    // Precompute forward reachability for every unsat node.
    let reach: BTreeMap<String, BTreeSet<String>> = unsat
        .iter()
        .map(|c| (c.clone(), reachable(c, edges)))
        .collect();

    let is_root = |c: &str| -> bool {
        let rc = &reach[c];
        rc.iter().all(|d| reach[d].contains(c))
    };

    let mut roots: Vec<String> = Vec::new();
    let mut derived: Vec<DerivedClass> = Vec::new();

    for c in unsat {
        if is_root(c) {
            roots.push(c.clone());
        } else {
            // Roots this derived class depends on = reachable roots (excluding self).
            let mut dr: Vec<String> = reach[c]
                .iter()
                .filter(|d| d.as_str() != c.as_str() && is_root(d))
                .cloned()
                .collect();
            dr.sort();
            derived.push(DerivedClass {
                iri: c.clone(),
                roots: dr,
            });
        }
    }

    roots.sort();
    derived.sort_by(|a, b| a.iri.cmp(&b.iri));

    // Invert: for each root, the derived classes that reach it.
    let mut root_derives: BTreeMap<String, Vec<String>> =
        roots.iter().map(|r| (r.clone(), Vec::new())).collect();
    for d in &derived {
        for r in &d.roots {
            root_derives.entry(r.clone()).or_default().push(d.iri.clone());
        }
    }
    for v in root_derives.values_mut() {
        v.sort();
    }

    let all_unsat: Vec<String> = unsat.iter().cloned().collect();

    Diagnosis {
        consistent: true,
        roots,
        derived,
        all_unsat,
        root_derives,
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p owl-dl-reasoner --lib partition_tests`
Expected: PASS (6 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/owl-dl-reasoner/src/diagnose.rs
git commit -m "feat(diagnose): reachability + sink-SCC root/derived partition"
```

---

### Task 4: `diagnose()` orchestration

Wire consistency + classify + edges + partition into the public `diagnose()`.

**Files:**
- Modify: `crates/owl-dl-reasoner/src/diagnose.rs`
- Create: `crates/owl-dl-reasoner/tests/diagnose_partition.rs`

- [ ] **Step 1: Write the failing integration test**

Create `crates/owl-dl-reasoner/tests/diagnose_partition.rs`:

```rust
//! Integration tests for `diagnose`: cascade fixture, inconsistency, conservation.

use horned_owl::model::{Build, MutableOntology};
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::diagnose;

type Rc = std::rc::Rc<str>;

fn b() -> Build<Rc> {
    Build::new_rc()
}

// Root = Bad (Bad ⊑ A ⊓ ¬A); Derived = SubBad (SubBad ⊑ Bad).
#[test]
fn root_and_derived_cascade() {
    let b = b();
    let mut o = SetOntology::new();
    use horned_owl::model::ClassExpression as CE;
    // Bad ⊑ A ⊓ ¬A  → Bad unsat (a root: depends on no other unsat class)
    o.insert(horned_owl::model::SubClassOf {
        sub: CE::Class(b.class("urn:Bad")),
        sup: CE::ObjectIntersectionOf(vec![
            CE::Class(b.class("urn:A")),
            CE::ObjectComplementOf(Box::new(CE::Class(b.class("urn:A")))),
        ]),
        ann: Default::default(),
    });
    // SubBad ⊑ Bad  → SubBad unsat (derived from Bad)
    o.insert(horned_owl::model::SubClassOf {
        sub: CE::Class(b.class("urn:SubBad")),
        sup: CE::Class(b.class("urn:Bad")),
        ann: Default::default(),
    });

    let d = diagnose(&o).expect("diagnose");
    assert!(d.consistent, "ontology is consistent (no ABox clash)");
    assert_eq!(d.roots, vec!["urn:Bad".to_string()]);
    assert_eq!(d.derived.len(), 1);
    assert_eq!(d.derived[0].iri, "urn:SubBad");
    assert_eq!(d.derived[0].roots, vec!["urn:Bad".to_string()]);
    // conservation
    let mut union: std::collections::BTreeSet<String> = d.roots.iter().cloned().collect();
    union.extend(d.derived.iter().map(|x| x.iri.clone()));
    let all: std::collections::BTreeSet<String> = d.all_unsat.iter().cloned().collect();
    assert_eq!(union, all);
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p owl-dl-reasoner --test diagnose_partition root_and_derived_cascade`
Expected: FAIL — `diagnose` still returns the empty placeholder.

- [ ] **Step 3: Implement the orchestration**

Replace the placeholder `diagnose` function in `crates/owl-dl-reasoner/src/diagnose.rs` with:

```rust
/// Diagnose `onto`: consistency, then the root/derived unsatisfiability partition.
///
/// Read-only over classification; never mutates the ontology. On an inconsistent
/// ontology returns `consistent: false` with empty partition (the caller should
/// justify the inconsistency directly).
pub fn diagnose<A: ForIRI>(onto: &SetOntology<A>) -> Result<Diagnosis, ReasonError> {
    if !crate::is_consistent(onto)? {
        return Ok(Diagnosis {
            consistent: false,
            roots: Vec::new(),
            derived: Vec::new(),
            all_unsat: Vec::new(),
            root_derives: BTreeMap::new(),
        });
    }

    let classification = crate::classify::classify(onto)?;
    let unsat: BTreeSet<String> = classification
        .unsatisfiable_classes()
        .into_iter()
        .map(str::to_string)
        .collect();

    if unsat.is_empty() {
        return Ok(Diagnosis {
            consistent: true,
            roots: Vec::new(),
            derived: Vec::new(),
            all_unsat: Vec::new(),
            root_derives: BTreeMap::new(),
        });
    }

    let logical = crate::justify::logical_axioms(onto).1;
    let edges = build_dep_edges(&unsat, &logical);
    Ok(partition(&unsat, &edges))
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p owl-dl-reasoner --test diagnose_partition root_and_derived_cascade`
Expected: PASS.

- [ ] **Step 5: Run the whole module's tests + build**

Run: `cargo test -p owl-dl-reasoner --lib diagnose && cargo build -p owl-dl-reasoner`
Expected: PASS, clean build.

- [ ] **Step 6: Commit**

```bash
git add crates/owl-dl-reasoner/src/diagnose.rs crates/owl-dl-reasoner/tests/diagnose_partition.rs
git commit -m "feat(diagnose): diagnose() orchestration (consistency + classify + partition)"
```

---

### Task 5: CLI `diagnose` subcommand

**Files:**
- Modify: `crates/owl-dl-cli/src/main.rs` (subcommand enum near line 244; handler near line 891; query/render helpers near line 838)

- [ ] **Step 1: Add the `Diagnose` subcommand variant**

In `crates/owl-dl-cli/src/main.rs`, in the `enum Command { … }`, add a new variant after the `Prove { … }` variant (the variant block ends around line 252):

```rust
    /// Diagnose a broken ontology: partition unsatisfiable classes into ROOT
    /// (genuine causes) and DERIVED (collateral), justify the roots, and on an
    /// inconsistent ontology report the responsible axioms.
    Diagnose {
        /// Path to the ontology (.ofn / .owx / .owl / .rdf).
        file: PathBuf,
        /// Print ALL minimal justifications per root (capped by --max), not just one.
        #[arg(long)]
        all: bool,
        /// Cap on the number of justifications printed with --all.
        #[arg(long, default_value_t = 10)]
        max: usize,
        /// Gloss each axiom with the rdfs:label of the entities it mentions.
        #[arg(long)]
        labels: bool,
    },
```

- [ ] **Step 2: Add the handler**

In the `match command { … }` block, add a `Command::Diagnose { … } => { … }` arm after the `Command::Prove { … } => { … }` arm. Insert:

```rust
        Command::Diagnose {
            file,
            all,
            max,
            labels,
        } => {
            use owl_dl_reasoner::justify::{
                Entailment, component_entities, find_all_justifications, find_one_justification,
            };
            let (onto, pm) = parse_ofn_with_pm(&file)?;
            let label_map = labels.then(|| build_label_map(&onto));

            // Shared renderer for a justification (mirrors the `justify` handler).
            let render = |j: &owl_dl_reasoner::justify::Justification<RcStr>, indent: &str| {
                let note = if j.minimal_guaranteed {
                    format!("minimal ({})", j.fragment)
                } else {
                    format!("entailing; minimality NOT guaranteed ({})", j.fragment)
                };
                println!("{indent}justification ({} axioms) — {note}", j.axioms.len());
                for ax in &j.axioms {
                    println!("{indent}  {}", ax.as_manchester_with_prefixes(&pm));
                    if let Some(lm) = &label_map {
                        let glosses: Vec<String> = component_entities(ax)
                            .into_iter()
                            .filter_map(|iri| {
                                lm.get(&iri).map(|l| format!("{} = \"{l}\"", local_name(&iri)))
                            })
                            .collect();
                        if !glosses.is_empty() {
                            println!("{indent}      label: {}", glosses.join("; "));
                        }
                    }
                }
            };

            // Render either one or all justifications for an entailment.
            let render_q = |q: &Entailment, indent: &str| -> anyhow::Result<()> {
                if all {
                    let js = find_all_justifications(&onto, q, max)
                        .context("find_all_justifications")?;
                    if js.is_empty() {
                        println!("{indent}(no justification found)");
                    }
                    for j in &js {
                        render(j, indent);
                    }
                } else {
                    match find_one_justification(&onto, q).context("find_one_justification")? {
                        Some(j) => render(&j, indent),
                        None => println!("{indent}(no justification found)"),
                    }
                }
                Ok(())
            };

            let d = owl_dl_reasoner::diagnose(&onto).context("diagnose")?;
            println!("# diagnose: {}", file.display());

            if !d.consistent {
                println!("# consistency: INCONSISTENT");
                println!("## responsible axioms:");
                render_q(&Entailment::Inconsistent, "  ")?;
                return Ok(());
            }

            println!("# consistency: consistent");
            if d.all_unsat.is_empty() {
                println!("# coherent: no unsatisfiable classes");
                return Ok(());
            }
            println!(
                "# unsatisfiable: {}  ({} root, {} derived)",
                d.all_unsat.len(),
                d.roots.len(),
                d.derived.len()
            );

            println!("\n## ROOT unsatisfiable classes (fix these first)");
            for r in &d.roots {
                println!("ROOT  {r}");
                render_q(&Entailment::Unsatisfiable { class: r.clone() }, "  ")?;
                if let Some(deps) = d.root_derives.get(r) {
                    if !deps.is_empty() {
                        println!("  derives: {}", deps.join(", "));
                    }
                }
            }

            if !d.derived.is_empty() {
                println!("\n## DERIVED unsatisfiable classes (likely resolve once roots are fixed)");
                for dc in &d.derived {
                    println!("DERIVED {}   <= {}", dc.iri, dc.roots.join(", "));
                }
            }
        }
```

- [ ] **Step 3: Build the CLI**

Run: `cargo build -p owl-dl-cli`
Expected: compiles. (If `RcStr` / `parse_ofn_with_pm` / `build_label_map` / `local_name` are not in scope in the match arm, they already are — they are used by the adjacent `Justify` handler in the same file.)

- [ ] **Step 4: Smoke-test on a crafted ontology**

```bash
cat > /tmp/diag-smoke.ofn <<'EOF'
Prefix(:=<urn:>)
Ontology(
  Declaration(Class(:A))
  Declaration(Class(:Bad))
  Declaration(Class(:SubBad))
  SubClassOf(:Bad ObjectIntersectionOf(:A ObjectComplementOf(:A)))
  SubClassOf(:SubBad :Bad)
)
EOF
./target/release/rustdl diagnose /tmp/diag-smoke.ofn
```

Expected output contains:
```
# consistency: consistent
# unsatisfiable: 2  (1 root, 1 derived)
## ROOT unsatisfiable classes (fix these first)
ROOT  urn:Bad
  justification (1 axioms) — ...
  derives: urn:SubBad
## DERIVED unsatisfiable classes (likely resolve once roots are fixed)
DERIVED urn:SubBad   <= urn:Bad
```

(Build the release binary first if needed: `cargo build -p owl-dl-cli --release`.)

- [ ] **Step 5: Commit**

```bash
git add crates/owl-dl-cli/src/main.rs
git commit -m "feat(diagnose): rustdl diagnose CLI subcommand"
```

---

### Task 6: Inconsistency path test (family)

**Files:**
- Modify: `crates/owl-dl-reasoner/tests/diagnose_partition.rs`

- [ ] **Step 1: Write the failing test**

Append to `crates/owl-dl-reasoner/tests/diagnose_partition.rs`:

```rust
// An ABox clash makes the ontology inconsistent: diagnose reports it, partition empty.
#[test]
fn inconsistent_ontology_flagged() {
    let b = b();
    let mut o = SetOntology::new();
    use horned_owl::model::ClassExpression as CE;
    // A DisjointWith B ; individual i is both A and B → inconsistent.
    o.insert(horned_owl::model::DisjointClasses(vec![
        CE::Class(b.class("urn:A")),
        CE::Class(b.class("urn:B")),
    ]));
    o.insert(horned_owl::model::ClassAssertion {
        ce: CE::Class(b.class("urn:A")),
        i: b.named_individual("urn:i").into(),
        ann: Default::default(),
    });
    o.insert(horned_owl::model::ClassAssertion {
        ce: CE::Class(b.class("urn:B")),
        i: b.named_individual("urn:i").into(),
        ann: Default::default(),
    });

    let d = diagnose(&o).expect("diagnose");
    assert!(!d.consistent, "ontology must be flagged inconsistent");
    assert!(d.roots.is_empty());
    assert!(d.derived.is_empty());
    assert!(d.all_unsat.is_empty());
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test -p owl-dl-reasoner --test diagnose_partition inconsistent_ontology_flagged`
Expected: PASS (the orchestration's `is_consistent` short-circuit already handles this).

If the `ClassAssertion` / `named_individual` constructor names differ, check the existing usage pattern in `crates/owl-dl-reasoner/src/justify.rs` (functions `named`, `ind_iri`) and match the horned-owl API exactly.

- [ ] **Step 3: Commit**

```bash
git add crates/owl-dl-reasoner/tests/diagnose_partition.rs
git commit -m "test(diagnose): inconsistent ontology is flagged, partition empty"
```

---

### Task 7: Corpus conservation invariant

The merge-gate safety test: on every real fixture, `roots ∪ derived` must equal the classifier's unsatisfiable set exactly.

**Files:**
- Modify: `crates/owl-dl-reasoner/tests/diagnose_partition.rs`

- [ ] **Step 1: Write the corpus conservation test (ignored by default; needs the fetched corpus)**

Append to `crates/owl-dl-reasoner/tests/diagnose_partition.rs`:

```rust
use owl_dl_reasoner::classify;

// Conservation invariant on a real fixture: roots ∪ derived == classified-unsat.
// Ignored by default (needs the fetched corpus); run with `-- --ignored`.
#[test]
#[ignore = "needs fetched corpus (scripts/fetch-real-ontologies.sh)"]
fn corpus_conservation_invariant() {
    for path in ["ontologies/real/sio.ofn", "ontologies/real/wine.ofn"] {
        let p = std::path::Path::new(path);
        if !p.exists() {
            eprintln!("skip {path} (not fetched)");
            continue;
        }
        let onto = read_ofn_fixture(p);
        let classification = classify(&onto).expect("classify");
        let classified: std::collections::BTreeSet<String> = classification
            .unsatisfiable_classes()
            .into_iter()
            .map(str::to_string)
            .collect();

        let d = diagnose(&onto).expect("diagnose");
        if !d.consistent {
            // Inconsistent ⇒ partition deliberately empty; nothing to conserve.
            continue;
        }
        let mut union: std::collections::BTreeSet<String> = d.roots.iter().cloned().collect();
        union.extend(d.derived.iter().map(|x| x.iri.clone()));
        assert_eq!(
            union, classified,
            "{path}: roots ∪ derived must equal the classified unsat set"
        );
        // Sanity: roots non-empty whenever there is any unsat class.
        if !classified.is_empty() {
            assert!(!d.roots.is_empty(), "{path}: unsat classes but no root reported");
        }
    }
}

// Minimal .ofn reader for the test (mirrors the CLI's OFN read path).
fn read_ofn_fixture(p: &std::path::Path) -> SetOntology<Rc> {
    use horned_owl::io::ofn::reader::read;
    let mut f = std::io::BufReader::new(std::fs::File::open(p).expect("open fixture"));
    let (o, _) = read(&mut f, Default::default()).expect("parse ofn");
    o.into()
}
```

If `horned_owl::io::ofn::reader::read`'s exact path/signature differs, copy the OFN read used by `crates/owl-dl-cli/src/main.rs` (`read_ofn`, around line 379) — match it exactly.

- [ ] **Step 2: Run it against the corpus (fetch first if needed)**

```bash
./scripts/fetch-real-ontologies.sh   # if ontologies/real is empty
cargo test -p owl-dl-reasoner --test diagnose_partition corpus_conservation_invariant -- --ignored --nocapture
```

Expected: PASS (or "skip … (not fetched)" lines if a fixture is absent). The conservation `assert_eq!` must hold on every present fixture.

- [ ] **Step 3: Commit**

```bash
git add crates/owl-dl-reasoner/tests/diagnose_partition.rs
git commit -m "test(diagnose): corpus conservation invariant (roots ∪ derived == classified-unsat)"
```

---

### Task 8: Docs + final gate

**Files:**
- Modify: `README.md` (CLI section, around line 102)
- Modify: `CLAUDE.md` (the `owl-dl-cli` bullet)

- [ ] **Step 1: Document the command in README**

In `README.md`, in the CLI block, add a line after the `prove` line:

```
rustdl diagnose  ontology.ofn               # root vs derived unsatisfiable classes (where to start fixing)
```

- [ ] **Step 2: Note it in CLAUDE.md**

In `CLAUDE.md`, in the `owl-dl-cli` bullet, append a sentence:

```
`diagnose` partitions unsatisfiable classes into root (causes) vs derived
(collateral) via a stingy structural dependency graph and justifies the roots;
read-only over classification, so FP=0 is untouched (see
`docs/superpowers/specs/2026-06-21-diagnose-root-derived-unsat-design.md`).
```

- [ ] **Step 3: Run the full local gate**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```

Expected: all green. (If clippy flags the `render`/`render_q` closures capturing `all`/`max`, or a `needless_borrow`, fix inline — these are mechanical.)

- [ ] **Step 4: Closure byte-identical sanity (FP=0 unchanged)**

`diagnose` is read-only over `classify`, but confirm nothing changed the classify path. Pick one fixture and confirm classify output is unchanged vs `main`:

```bash
./target/release/rustdl classify ontologies/real/sio.ofn > /tmp/sio-after.txt 2>/dev/null || true
git stash; cargo build -p owl-dl-cli --release 2>/dev/null
./target/release/rustdl classify ontologies/real/sio.ofn > /tmp/sio-before.txt 2>/dev/null || true
git stash pop; cargo build -p owl-dl-cli --release 2>/dev/null
diff /tmp/sio-before.txt /tmp/sio-after.txt && echo "CLOSURE IDENTICAL"
```

Expected: `CLOSURE IDENTICAL` (no diff). (Skip if the corpus is not fetched — the read-only property is structural.)

- [ ] **Step 5: Commit**

```bash
git add README.md CLAUDE.md
git commit -m "docs(diagnose): document the diagnose command (README + CLAUDE.md)"
```

---

## Self-review notes (author)

- **Spec coverage:** edge-set table → Task 2 (incl. all negative controls: `∀`, disjointness, non-unsat target, self-edge); cycles → Task 3 (`cycle_members_are_co_roots`, `cycle_depending_on_root`); transitive root attribution → Task 3 (`chain_has_single_root`); both scenarios (consistent/inconsistent) → Task 4 + Task 6; CLI `diagnose` + `--all`/`--max`/`--labels` → Task 5; conservation invariant → Task 3 (unit) + Task 7 (corpus); soundness/read-only → Task 8 Step 4. `≥n r.D` is intentionally treated as non-forcing (omission over-reports roots = safe), matching the spec's silence on it.
- **No placeholders:** every code step is complete and copy-pasteable.
- **Type consistency:** `Diagnosis` / `DerivedClass` field names (`consistent`, `roots`, `derived`, `all_unsat`, `root_derives`; `iri`, `roots`) are identical across Tasks 1–5. `build_dep_edges` / `unsat_forcing_classes` / `partition` / `reachable` signatures match their call sites.
- **API risk flagged inline:** horned-owl constructor names (`ClassAssertion`, `named_individual`, OFN `read`) — Tasks 6/7 point at existing in-repo usage to copy if a name differs.
