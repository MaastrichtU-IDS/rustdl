# `rustdl repair` (repair suggestions) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `rustdl repair`, which lists minimal sets of axioms to remove so an unwanted entailment (unsatisfiable class, inconsistency, or any `justify` query) no longer holds — each repair verified to actually break the entailment.

**Architecture:** A new read-only module `crates/owl-dl-reasoner/src/repair.rs` computes repairs as the minimal hitting sets over all justifications (from the shipped `find_all_justifications`), then verifies each by removing it and re-checking the entailment. A new `repair` CLI subcommand reuses justify's query parser + renderer.

**Tech Stack:** Rust (edition 2024), horned-owl model types, `owl-dl-reasoner` crate (`justify::{find_all_justifications, logical_axioms, ontology_from, entails, Entailment, Justification}`).

**Spec:** `docs/superpowers/specs/2026-06-21-repair-suggestions-design.md`
**Branch:** `feat/repair-suggestions`

---

## Key facts (verified against the codebase)

- `justify::find_all_justifications(onto, q, max) -> Result<Vec<Justification<A>>, ReasonError>`. Returns `[]` iff `q` is not entailed. Each `Justification<A> { axioms: Vec<Component<A>>, fragment, minimal_guaranteed }`; all carry the same `minimal_guaranteed` (true iff EL/Horn → the justification set is complete).
- `justify::logical_axioms(onto) -> (Vec<Component<A>>, Vec<Component<A>>)` — `.0` = non-logical (declarations), `.1` = logical axioms.
- `justify::ontology_from(fixed: &[Component<A>], subset: &[Component<A>]) -> SetOntology<A>` (pub).
- `justify::entails(onto: &SetOntology<A>, q: &Entailment) -> Result<bool, ReasonError>` (pub).
- `Component<A>: Clone + Eq + Hash + Ord` (used in `BTreeSet`/`HashSet` in justify.rs). `BTreeSet` has `is_disjoint`, `is_subset`.
- CLI: `parse_justify_query(parts: &[String]) -> anyhow::Result<Entailment>` (`main.rs:632`); the `Justify` variant/handler (`main.rs:216`/`:838`) is the template for the new `repair` arm — it uses `RcStr`, `parse_ofn_with_pm`, `build_label_map`, `local_name`, `ax.as_manchester_with_prefixes(&pm)`, `justify::component_entities`.
- Module declarations: `crates/owl-dl-reasoner/src/lib.rs` near line 46; re-exports near line 57.
- Convention from prior sub-features: model-API-built test ontologies need explicit `Declaration` axioms (`o.insert(DeclareClass(b.class(iri)))`) or you get `UnknownClass`.

## File structure

- **Create** `crates/owl-dl-reasoner/src/repair.rs` — `Repair`/`Repairs` types, `minimal_hitting_sets`, `find_repairs`, unit tests.
- **Modify** `crates/owl-dl-reasoner/src/lib.rs` — `pub mod repair;` + re-export.
- **Create** `crates/owl-dl-reasoner/tests/repair_suggestions.rs` — integration tests.
- **Modify** `crates/owl-dl-cli/src/main.rs` — `repair` subcommand + handler.
- **Modify** `README.md`, `CLAUDE.md` — document (final task).

---

### Task 1: Branch + module skeleton

**Files:** Modify `crates/owl-dl-reasoner/src/lib.rs`; Create `crates/owl-dl-reasoner/src/repair.rs`

ENVIRONMENT: cargo may not be on PATH — prefix shells with:
```bash
export RUSTUP_HOME=/home/dumontier/.rustup
export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"
```

- [ ] **Step 1: Branch**

```bash
cd /data/dumontier/rustdl
git checkout main
git checkout -b feat/repair-suggestions
```

- [ ] **Step 2: Create `crates/owl-dl-reasoner/src/repair.rs`**

```rust
//! Repair suggestions: minimal sets of axioms whose removal makes an unwanted
//! entailment `η` no longer hold. Repairs are the minimal hitting sets over all
//! justifications of `η` (Reiter diagnoses). Every reported repair is VERIFIED by
//! removing it and confirming `η` no longer holds — sound even when the
//! justification set is incomplete. Read-only; never mutates the ontology.

use std::collections::BTreeSet;

use horned_owl::model::{Component, ForIRI};
use horned_owl::ontology::set::SetOntology;

use crate::ReasonError;
use crate::justify::{Entailment, entails, find_all_justifications, logical_axioms, ontology_from};

/// Cap on justifications discovered for repair (independent of the user-facing
/// `max` on repairs). Generous so the hitting sets are computed over as complete a
/// justification set as the fragment allows; on EL/Horn this finds them all.
const REPAIR_JUSTIFICATION_CAP: usize = 100;

/// A single repair: the axioms to remove to break the entailment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repair<A: ForIRI> {
    /// Axioms to remove (sorted, minimal).
    pub remove: Vec<Component<A>>,
}

/// The result of a repair query.
#[derive(Debug, Clone)]
pub struct Repairs<A: ForIRI> {
    /// Whether `η` was entailed at all (`false` → nothing to repair).
    pub entailed: bool,
    /// Verified minimal repairs, smallest first, capped by the user `max`.
    pub repairs: Vec<Repair<A>>,
    /// Whether the repair set is complete (all minimal repairs found) — true iff
    /// the underlying justification set is complete (EL/Horn).
    pub complete: bool,
    /// Candidate hitting sets discarded because they failed verification (an
    /// unfound justification survived). >0 signals the reported set may be partial.
    pub dropped_unverified: usize,
}

/// Compute repairs for `q` in `onto`. Filled in by Task 3.
pub fn find_repairs<A: ForIRI>(
    onto: &SetOntology<A>,
    q: &Entailment,
    max: usize,
) -> Result<Repairs<A>, ReasonError> {
    let _ = (onto, q, max, REPAIR_JUSTIFICATION_CAP);
    Ok(Repairs {
        entailed: false,
        repairs: Vec::new(),
        complete: true,
        dropped_unverified: 0,
    })
}
```

- [ ] **Step 3: Wire into `lib.rs`**

Add `pub mod repair;` next to `pub mod justify;`, and `pub use repair::{Repair, Repairs, find_repairs};` next to the other `pub use` re-exports. READ the surrounding lines first to place them correctly.

- [ ] **Step 4: Build**

Run: `cargo build -p owl-dl-reasoner`
Expected: compiles. Unused-import warnings for `entails`, `find_all_justifications`, `logical_axioms`, `ontology_from`, `BTreeSet` are EXPECTED (used in Tasks 2–3) — keep them. Do NOT run clippy in this task (it would deny them).

- [ ] **Step 5: Commit**

```bash
git add crates/owl-dl-reasoner/src/repair.rs crates/owl-dl-reasoner/src/lib.rs
git commit -m "feat(repair): module skeleton + Repair/Repairs types"
```

---

### Task 2: Minimal hitting set core

**Files:** Modify `crates/owl-dl-reasoner/src/repair.rs`

- [ ] **Step 1: Write the failing tests** — append to `crates/owl-dl-reasoner/src/repair.rs`:

```rust
#[cfg(test)]
mod mhs_tests {
    use super::*;
    use horned_owl::model::{Build, SubClassOf};
    use horned_owl::model::ClassExpression as CE;

    type Rc = std::rc::Rc<str>;

    // Build a distinct dummy axiom per label so sets compare by content.
    fn ax(b: &Build<Rc>, name: &str) -> Component<Rc> {
        Component::SubClassOf(SubClassOf {
            sub: CE::Class(b.class(&format!("urn:{name}sub"))),
            sup: CE::Class(b.class(&format!("urn:{name}sup"))),
        })
    }
    fn set(items: &[Component<Rc>]) -> BTreeSet<Component<Rc>> {
        items.iter().cloned().collect()
    }

    // One justification {a, b}: minimal hitting sets are {a} and {b}.
    #[test]
    fn single_justification_each_axiom_is_a_repair() {
        let b = Build::new_rc();
        let (a, c) = (ax(&b, "a"), ax(&b, "b"));
        let js = vec![set(&[a.clone(), c.clone()])];
        let mhs = minimal_hitting_sets(&js);
        let got: BTreeSet<BTreeSet<Component<Rc>>> = mhs.into_iter().collect();
        let want: BTreeSet<BTreeSet<Component<Rc>>> =
            [set(&[a]), set(&[c])].into_iter().collect();
        assert_eq!(got, want);
    }

    // Two disjoint justifications {a},{b}: the only hitting set is {a,b}.
    #[test]
    fn disjoint_justifications_need_both() {
        let b = Build::new_rc();
        let (a, c) = (ax(&b, "a"), ax(&b, "b"));
        let js = vec![set(&[a.clone()]), set(&[c.clone()])];
        let mhs = minimal_hitting_sets(&js);
        assert_eq!(mhs, vec![set(&[a, c])]);
    }

    // Overlapping {a,b},{a,c}: shared axiom {a} is the minimal repair; {b,c} is also
    // a hitting set but is NOT minimal-cardinality — both {a} and {b,c} are minimal
    // transversals, but {a} must appear and no returned set is a superset of another.
    #[test]
    fn overlapping_justifications_share_repair() {
        let b = Build::new_rc();
        let (a, c, d) = (ax(&b, "a"), ax(&b, "b"), ax(&b, "c"));
        let js = vec![set(&[a.clone(), c.clone()]), set(&[a.clone(), d.clone()])];
        let mhs: BTreeSet<BTreeSet<Component<Rc>>> =
            minimal_hitting_sets(&js).into_iter().collect();
        // {a} hits both; {b,c} hits both; neither is a subset of the other.
        assert!(mhs.contains(&set(&[a.clone()])), "shared axiom {{a}} must be a repair");
        assert!(mhs.contains(&set(&[c, d])), "{{b,c}} is also a minimal transversal");
        // minimality: no returned set is a strict superset of another
        for x in &mhs {
            for y in &mhs {
                if x != y {
                    assert!(!x.is_subset(y), "no repair may be a superset of another");
                }
            }
        }
    }

    // No justifications → no hitting sets.
    #[test]
    fn empty_justifications_no_repairs() {
        let js: Vec<BTreeSet<Component<Rc>>> = Vec::new();
        assert!(minimal_hitting_sets(&js).is_empty());
    }
}
```

- [ ] **Step 2: Run to confirm FAIL** — `cargo test -p owl-dl-reasoner --lib mhs_tests`
Expected: FAIL (`minimal_hitting_sets` undefined). If `SubClassOf { sub, sup }` / `ClassExpression::Class` don't compile, match the shapes used in `crates/owl-dl-reasoner/src/laconic.rs` / `diagnose.rs`. Report adjustments.

- [ ] **Step 3: Implement `minimal_hitting_sets`** — insert into `crates/owl-dl-reasoner/src/repair.rs` (before the `#[cfg(test)]` block):

```rust
/// Enumerate the minimal hitting sets (minimal transversals) over `justifications`:
/// the ⊆-minimal sets that intersect every justification. These are the minimal
/// repairs. Cheap for the small justification sets seen in practice; the
/// dominated-branch prune below bounds the search.
fn minimal_hitting_sets<A: ForIRI>(
    justifications: &[BTreeSet<Component<A>>],
) -> Vec<BTreeSet<Component<A>>> {
    let mut results: Vec<BTreeSet<Component<A>>> = Vec::new();
    if justifications.is_empty() {
        return results;
    }
    let mut seen: std::collections::HashSet<BTreeSet<Component<A>>> =
        std::collections::HashSet::new();
    let mut worklist: Vec<BTreeSet<Component<A>>> = vec![BTreeSet::new()];

    while let Some(h) = worklist.pop() {
        if !seen.insert(h.clone()) {
            continue;
        }
        // Prune: if some known minimal repair is already ⊆ h, h can't be minimal.
        if results.iter().any(|r| r.is_subset(&h)) {
            continue;
        }
        // First justification not hit by h.
        match justifications.iter().find(|j| j.is_disjoint(&h)) {
            None => {
                // h hits all → it is a minimal hitting set (prune above guaranteed
                // no subset already present). Drop any existing superset of h.
                results.retain(|r| !h.is_subset(r));
                results.push(h);
            }
            Some(ju) => {
                for a in ju {
                    let mut next = h.clone();
                    next.insert(a.clone());
                    worklist.push(next);
                }
            }
        }
    }
    results
}
```

- [ ] **Step 4: Run** — `cargo test -p owl-dl-reasoner --lib mhs_tests` → 4 passed. Paste the `test result:` line.

- [ ] **Step 5: clippy + fmt** —
```bash
cargo clippy -p owl-dl-reasoner --all-targets -- -D warnings
cargo fmt -p owl-dl-reasoner
```
If clippy flags `minimal_hitting_sets` as dead (only used by tests until Task 3), add `#[allow(dead_code)] // wired into find_repairs in Task 3; allow removed there` above it (and any still-unused import). Re-run mhs_tests after fmt.

- [ ] **Step 6: Commit**

```bash
git add crates/owl-dl-reasoner/src/repair.rs
git commit -m "feat(repair): minimal hitting set core"
```

---

### Task 3: `find_repairs` driver

**Files:** Modify `crates/owl-dl-reasoner/src/repair.rs`; Create `crates/owl-dl-reasoner/tests/repair_suggestions.rs`

- [ ] **Step 1: Write the failing integration test** — create `crates/owl-dl-reasoner/tests/repair_suggestions.rs`:

```rust
//! Integration tests for repair suggestions.

use horned_owl::model::{Build, ClassExpression as CE, DeclareClass, MutableOntology, SubClassOf};
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::find_repairs;
use owl_dl_reasoner::justify::{Entailment, entails, logical_axioms, ontology_from};

type Rc = std::rc::Rc<str>;

// X unsat via TWO independent justifications:
//   J1 = { X ⊑ A, X ⊑ B }   (A,B disjoint)
//   J2 = { X ⊑ C }          (C ⊑ ⊥ via C ⊑ D ⊓ ¬D)
// Minimal repairs (each verified to make X satisfiable):
//   removing one of {X⊑A, X⊑B} breaks J1, and removing {X⊑C} breaks J2 → a repair
//   must hit BOTH: e.g. {X⊑A, X⊑C} or {X⊑B, X⊑C}.
#[test]
fn repairs_hit_every_justification_and_verify() {
    let b = Build::new_rc();
    let cls = |iri: &str| CE::Class(b.class(iri));
    let mut o = SetOntology::new();
    for c in ["urn:X", "urn:A", "urn:B", "urn:C", "urn:D"] {
        o.insert(DeclareClass(b.class(c)));
    }
    // A and B disjoint
    o.insert(horned_owl::model::DisjointClasses(vec![cls("urn:A"), cls("urn:B")]));
    // X ⊑ A, X ⊑ B  (→ X unsat, justification J1)
    o.insert(SubClassOf { sub: cls("urn:X"), sup: cls("urn:A") });
    o.insert(SubClassOf { sub: cls("urn:X"), sup: cls("urn:B") });
    // C ⊑ D ⊓ ¬D  (C unsat), and X ⊑ C  (→ X unsat, justification J2)
    o.insert(SubClassOf {
        sub: cls("urn:C"),
        sup: CE::ObjectIntersectionOf(vec![cls("urn:D"), CE::ObjectComplementOf(Box::new(cls("urn:D")))]),
    });
    o.insert(SubClassOf { sub: cls("urn:X"), sup: cls("urn:C") });

    let q = Entailment::Unsatisfiable { class: "urn:X".to_string() };
    let r = find_repairs(&o, &q, 10).expect("repair");
    assert!(r.entailed, "X is unsatisfiable → entailed");
    assert!(!r.repairs.is_empty(), "must find at least one repair");

    // Every reported repair must actually make X satisfiable (verification).
    let (fixed, logical) = logical_axioms(&o);
    for rep in &r.repairs {
        let kept: Vec<_> = logical
            .iter()
            .filter(|a| !rep.remove.contains(a))
            .cloned()
            .collect();
        let o2 = ontology_from(&fixed, &kept);
        assert!(
            !entails(&o2, &q).expect("entails"),
            "repair {:?} must break the unsatisfiability",
            rep.remove
        );
        // Each repair must touch BOTH justifications → contains X⊑C plus one of X⊑A/X⊑B.
        assert!(rep.remove.len() >= 2, "must hit both independent justifications");
    }
}

// Not entailed → entailed=false, no repairs.
#[test]
fn not_entailed_nothing_to_repair() {
    let b = Build::new_rc();
    let cls = |iri: &str| CE::Class(b.class(iri));
    let mut o = SetOntology::new();
    for c in ["urn:A", "urn:B"] {
        o.insert(DeclareClass(b.class(c)));
    }
    o.insert(SubClassOf { sub: cls("urn:A"), sup: cls("urn:B") });
    let q = Entailment::Unsatisfiable { class: "urn:A".to_string() };
    let r = find_repairs(&o, &q, 10).expect("repair");
    assert!(!r.entailed);
    assert!(r.repairs.is_empty());
}
```

- [ ] **Step 2: Run to confirm FAIL** — `cargo test -p owl-dl-reasoner --test repair_suggestions`
Expected: FAIL (stub returns `entailed:false`). If `DisjointClasses`/`ObjectComplementOf` shapes differ, match `diagnose.rs`/`laconic.rs`. Report.

- [ ] **Step 3: Implement `find_repairs`** — replace the stub in `crates/owl-dl-reasoner/src/repair.rs` with:

```rust
/// Compute verified minimal repairs for `q` in `onto`.
pub fn find_repairs<A: ForIRI>(
    onto: &SetOntology<A>,
    q: &Entailment,
    max: usize,
) -> Result<Repairs<A>, ReasonError> {
    // All justifications (generous internal cap, independent of the repair `max`).
    let justifications = find_all_justifications(onto, q, REPAIR_JUSTIFICATION_CAP)?;
    if justifications.is_empty() {
        return Ok(Repairs {
            entailed: false,
            repairs: Vec::new(),
            complete: true,
            dropped_unverified: 0,
        });
    }
    let complete = justifications.iter().all(|j| j.minimal_guaranteed);

    // Hitting sets over the justification axiom-sets.
    let j_sets: Vec<BTreeSet<Component<A>>> = justifications
        .iter()
        .map(|j| j.axioms.iter().cloned().collect())
        .collect();
    let mut candidates = minimal_hitting_sets(&j_sets);
    // Smallest repairs first, deterministic.
    candidates.sort_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)));

    // Verify each candidate by removing it and re-checking the entailment.
    let (fixed, logical) = logical_axioms(onto);
    let mut repairs = Vec::new();
    let mut dropped_unverified = 0usize;
    for h in candidates {
        if repairs.len() >= max {
            break;
        }
        let kept: Vec<Component<A>> = logical.iter().filter(|a| !h.contains(a)).cloned().collect();
        let reduced = ontology_from(&fixed, &kept);
        if entails(&reduced, q)? {
            // An unfound justification survives — not a real repair.
            dropped_unverified += 1;
            continue;
        }
        repairs.push(Repair {
            remove: h.into_iter().collect(),
        });
    }

    Ok(Repairs {
        entailed: true,
        repairs,
        complete,
        dropped_unverified,
    })
}
```
Then REMOVE the `#[allow(dead_code)]` from `minimal_hitting_sets` if you added it in Task 2 (it is now used).

- [ ] **Step 4: Run** — `cargo test -p owl-dl-reasoner --test repair_suggestions` → 2 passed. If `repairs_hit_every_justification_and_verify` fails because a repair did NOT break unsatisfiability, that's a real bug — investigate (the verification loop should have filtered it). Paste the `test result:` line.

- [ ] **Step 5: full lib + integration + clippy + fmt** —
```bash
cargo test -p owl-dl-reasoner --lib repair
cargo test -p owl-dl-reasoner --test repair_suggestions
cargo clippy -p owl-dl-reasoner --all-targets -- -D warnings
cargo fmt -p owl-dl-reasoner
```
All green; re-run tests after fmt. Confirm `grep -c 'allow(dead_code)' crates/owl-dl-reasoner/src/repair.rs` → 0.

- [ ] **Step 6: Commit**

```bash
git add crates/owl-dl-reasoner/src/repair.rs crates/owl-dl-reasoner/tests/repair_suggestions.rs
git commit -m "feat(repair): find_repairs driver (MHS over justifications + verify each)"
```

---

### Task 4: CLI `repair` subcommand

**Files:** Modify `crates/owl-dl-cli/src/main.rs`

- [ ] **Step 1: Add the `Repair` variant** — in `enum Command`, after the `Justify { … }` variant, add:

```rust
    /// Suggest minimal axiom removals to break an unwanted entailment.
    Repair {
        /// Path to the ontology (.ofn / .owx / .owl / .rdf).
        file: PathBuf,
        /// Query (same forms as `justify`): `unsat C` | `subclass S T` |
        /// `inconsistent` | `instance I C` | … (see `justify --help`).
        #[arg(num_args = 1..)]
        query: Vec<String>,
        /// Cap on the number of repairs printed (smallest first).
        #[arg(long, default_value_t = 10)]
        max: usize,
        /// Gloss each axiom with the rdfs:label of the entities it mentions.
        #[arg(long)]
        labels: bool,
    },
```

- [ ] **Step 2: Add the handler** — in the `match command { … }` block, after the `Command::Justify { … } => { … }` arm, add:

```rust
        Command::Repair {
            file,
            query,
            max,
            labels,
        } => {
            use owl_dl_reasoner::justify::component_entities;
            let (onto, pm) = parse_ofn_with_pm(&file)?;
            let q = parse_justify_query(&query)?;
            let label_map = labels.then(|| build_label_map(&onto));
            let r = owl_dl_reasoner::find_repairs(&onto, &q, max).context("find_repairs")?;

            if !r.entailed {
                println!("not entailed; nothing to repair");
                return Ok(());
            }
            if r.repairs.is_empty() {
                println!("entailed, but no verifiable axiom removal found");
                return Ok(());
            }

            let completeness = if r.complete {
                "complete"
            } else {
                "w.r.t. found justifications (completeness not guaranteed)"
            };
            println!("# {} minimal repair(s) — {completeness}", r.repairs.len());
            for (i, rep) in r.repairs.iter().enumerate() {
                println!("repair {} (remove {} axiom(s)):", i + 1, rep.remove.len());
                for ax in &rep.remove {
                    println!("  {}", ax.as_manchester_with_prefixes(&pm));
                    if let Some(lm) = &label_map {
                        let glosses: Vec<String> = component_entities(ax)
                            .into_iter()
                            .filter_map(|iri| {
                                lm.get(&iri).map(|l| format!("{} = \"{l}\"", local_name(&iri)))
                            })
                            .collect();
                        if !glosses.is_empty() {
                            println!("      label: {}", glosses.join("; "));
                        }
                    }
                }
            }
            if r.dropped_unverified > 0 {
                println!(
                    "# note: {} candidate(s) dropped (failed verification — justification set may be incomplete)",
                    r.dropped_unverified
                );
            }
        }
```
Adapt `return Ok(())` to the enclosing fn (the `Justify` handler shows the pattern; `repair`'s early returns work the same way as `diagnose`'s did). If `RcStr` / helpers aren't in scope, they already are (used by `Justify`).

- [ ] **Step 3: Build** — `cargo build -p owl-dl-cli`

- [ ] **Step 4: Smoke-test** —
```bash
cat > /tmp/repair-smoke.ofn <<'EOF'
Prefix(:=<urn:>)
Ontology(
  Declaration(Class(:X)) Declaration(Class(:A)) Declaration(Class(:B))
  DisjointClasses(:A :B)
  SubClassOf(:X :A)
  SubClassOf(:X :B)
)
EOF
cargo build -p owl-dl-cli --release
./target/release/rustdl repair /tmp/repair-smoke.ofn unsat urn:X
```
Expected: `# N minimal repair(s) — complete`, with repairs each removing one of `X SubClassOf A` / `X SubClassOf B` (removing either makes X satisfiable). Paste the actual output. If it prints "not entailed", X wasn't unsat — STOP and report.

- [ ] **Step 5: clippy + fmt** —
```bash
cargo clippy -p owl-dl-cli --all-targets -- -D warnings
cargo fmt -p owl-dl-cli
```

- [ ] **Step 6: Commit**

```bash
git add crates/owl-dl-cli/src/main.rs
git commit -m "feat(repair): rustdl repair CLI subcommand"
```

---

### Task 5: Corpus check + docs + final gate

**Files:** Modify `crates/owl-dl-reasoner/tests/repair_suggestions.rs`, `README.md`, `CLAUDE.md`

- [ ] **Step 1: Add an ignored corpus test** — append to `crates/owl-dl-reasoner/tests/repair_suggestions.rs`:

```rust
// Real fixture: every repair of a pizza unsat class must verify (make it
// satisfiable). Ignored by default (corpus + SHOIN justify cost).
#[test]
#[ignore = "reads the curated corpus (ontologies/real/pizza.ofn)"]
fn repair_pizza_unsat_verifies() {
    let p = std::path::Path::new("../../ontologies/real/pizza.ofn");
    if !p.exists() {
        eprintln!("skip pizza.ofn (not present)");
        return;
    }
    let onto = read_ofn_fixture(p);
    let q = Entailment::Unsatisfiable {
        class: "http://www.co-ode.org/ontologies/pizza/pizza.owl#IceCream".to_string(),
    };
    let r = find_repairs(&onto, &q, 10).expect("repair");
    assert!(r.entailed && !r.repairs.is_empty(), "IceCream unsat → repairs exist");
    let (fixed, logical) = logical_axioms(&onto);
    for rep in &r.repairs {
        let kept: Vec<_> = logical.iter().filter(|a| !rep.remove.contains(a)).cloned().collect();
        assert!(
            !entails(&ontology_from(&fixed, &kept), &q).expect("entails"),
            "every reported repair must break IceCream's unsatisfiability"
        );
    }
    eprintln!("pizza IceCream: {} repair(s), complete={}", r.repairs.len(), r.complete);
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

- [ ] **Step 2: Run it** —
```bash
cargo test -p owl-dl-reasoner --test repair_suggestions repair_pizza_unsat_verifies -- --ignored --nocapture
```
Expected: PASS with `pizza IceCream: N repair(s), complete=…`. May take a few minutes (SHOIN justify). If it exceeds ~5 min, kill it and note — the synthetic tests validate correctness; the corpus test is supplementary and stays `#[ignore]`d.

- [ ] **Step 3: README** — in the CLI block, after the `justify --laconic` line, add:
```
rustdl repair    ontology.ofn <query…>      # minimal axiom removals to break an entailment
```
Match column alignment.

- [ ] **Step 4: CLAUDE.md** — append to the `owl-dl-cli` bullet:
```
`repair` lists minimal axiom-removal sets (Reiter diagnoses = minimal hitting sets
over all justifications) to break an unwanted entailment; every repair is verified
by removal (sound even when the justification set is incomplete). See
`docs/superpowers/specs/2026-06-21-repair-suggestions-design.md`.
```

- [ ] **Step 5: Full workspace gate** —
```bash
cd /data/dumontier/rustdl
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```
All three green. The corpus repair test is `#[ignore]`d (do NOT pass `--ignored`). Report any NON-ignored failure verbatim; fix only repair-related clippy, stop+report on unrelated pre-existing issues.

- [ ] **Step 6: Commit**

```bash
git add crates/owl-dl-reasoner/tests/repair_suggestions.rs README.md CLAUDE.md
git commit -m "test+docs(repair): corpus verification + README/CLAUDE notes"
```

---

## Self-review notes (author)

- **Spec coverage:** MHS core → Task 2 (with minimality + disjoint + overlap + empty cases); find_repairs (find_all → MHS → verify, completeness flag, dropped count) → Task 3; verification soundness property → Task 3 integration test (every repair re-checked to break η) + Task 5 corpus; CLI `repair` + `--max`/`--labels` + completeness header → Task 4; read-only/byte-identical → Task 5 gate.
- **Soundness:** every reported repair is verified by removal in `find_repairs` itself (`entails(reduced, q) == false`), and re-asserted independently in the tests. The `REPAIR_JUSTIFICATION_CAP` (100) decouples justification discovery from the user-facing repair `max` so hitting sets are computed over the complete justification set (on EL/Horn).
- **No placeholders:** every code step complete.
- **Type consistency:** `Repair { remove }`, `Repairs { entailed, repairs, complete, dropped_unverified }`, `minimal_hitting_sets`, `find_repairs` signatures consistent across tasks; `Component::SubClassOf(SubClassOf { sub, sup })` construction matches the repo.
- **API risk flagged inline:** `DisjointClasses`/`ObjectComplementOf`/`SubClassOf` shapes and `Declaration` requirement for model-built test ontologies — tasks point at `diagnose.rs`/`laconic.rs` to copy.
