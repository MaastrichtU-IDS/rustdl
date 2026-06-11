# classify-level concrete-domain verify (per-class) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `classify` report classes that are unsatisfiable solely by a concrete-domain *counting* clash (e.g. `C ⊑ DataMinCardinality(3 :p [0,1])`), by verifying counting-constrained classes on the main tableau instead of trusting the wedge's `Sat`.

**Architecture:** A targeted override in classify's existing per-class unsat probe (`classify.rs`). `PreparedOntology` gains `data_counting_classes` (named classes carrying a `Min`/`Max`-over-DKey constraint, built from the un-mutated IR). When the wedge says `Sat` for such a class (or a class whose saturation-subsumers include one), the probe falls through to `prepared.decide_with_deadline` (the main tableau, which already runs `concrete_domain_clash`). Sound: only swaps a wedge `Sat` for the complete path — never a false positive.

**Tech Stack:** Rust 2024, `owl-dl-reasoner` (classify orchestrator + `PreparedOntology`), `owl-dl-core` (`Axiom`/`ConceptExpr`/`ConceptPool`), `owl-dl-datatypes` (`CardRange`), horned-owl OFN parser, `cargo test`.

Spec: `docs/superpowers/specs/2026-06-11-classify-concrete-domain-verify-design.md`.

---

## File structure

- **Modify** `crates/owl-dl-reasoner/src/lib.rs`
  - Add free fns `concept_has_dkey_counting` + `build_data_counting_classes` (next to the existing `build_dkey_range_map`).
  - Add field `data_counting_classes: HashSet<ClassId>` to `PreparedOntology`; populate in `from_internal`.
  - Add a `#[cfg(test)]` unit test for the builder (alongside `prepared_builds_integer_dkey_range_map`).
- **Modify** `crates/owl-dl-reasoner/src/classify.rs`
  - The unsat-probe `Some(LabelOracle::Sat(_))` arm (~line 1095): override for counting-constrained classes.
- **Create** `crates/owl-dl-reasoner/tests/classify_concrete_domain.rs`
  - classify-level canaries: counting clashes → unsat; satisfiable nodes stay sat; inheritance; D11b membership probe.
- **Update** the spec status + memory after verification.

---

### Task 1: `data_counting_classes` builder + field (pure, no behavior change)

**Files:**
- Modify: `crates/owl-dl-reasoner/src/lib.rs` (free fns near `build_dkey_range_map`; field in `PreparedOntology` struct + `from_internal`)

- [ ] **Step 1: Write the failing unit test**

Add to the `#[cfg(test)] mod tests` in `crates/owl-dl-reasoner/src/lib.rs` (the module already containing `prepared_builds_integer_dkey_range_map`, so `convert_ontology`, `read_ofn`, `SetOntology`, `RcStr`, `Cursor` are already imported there — if any import is missing, mirror the existing test's `use` lines):

```rust
#[test]
fn builds_data_counting_classes_for_integer_cardinality() {
    let src = "Prefix(:=<http://t/>)\n\
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
Ontology(\nDeclaration(Class(:C))\nDeclaration(DataProperty(:p))\n\
SubClassOf(:C DataMinCardinality(3 :p DatatypeRestriction(xsd:integer \
xsd:minInclusive \"0\"^^xsd:integer xsd:maxInclusive \"1\"^^xsd:integer)))\n)\n";
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut Cursor::new(src), ParserConfiguration::default()).expect("parse");
    let internal = convert_ontology(&onto).expect("convert");
    let dkey = build_dkey_range_map(&internal);
    let counting = build_data_counting_classes(&internal, &dkey);
    // Resolve :C's ClassId by IRI (do NOT assume index 0 — owl:Thing may take it).
    let c_id = internal
        .vocabulary
        .classes()
        .find(|(_, iri)| *iri == "http://t/C")
        .map(|(id, _)| id)
        .expect("C declared");
    assert!(
        counting.contains(&c_id),
        "C must be in data_counting_classes; got {counting:?}"
    );
}

#[test]
fn no_data_counting_classes_for_value_membership_only() {
    // DataSomeValuesFrom is value-membership (∃p.DKey), NOT counting.
    let src = "Prefix(:=<http://t/>)\n\
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
Ontology(\nDeclaration(Class(:C))\nDeclaration(DataProperty(:p))\n\
SubClassOf(:C DataSomeValuesFrom(:p DatatypeRestriction(xsd:integer \
xsd:minInclusive \"0\"^^xsd:integer xsd:maxInclusive \"10\"^^xsd:integer)))\n)\n";
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut Cursor::new(src), ParserConfiguration::default()).expect("parse");
    let internal = convert_ontology(&onto).expect("convert");
    let dkey = build_dkey_range_map(&internal);
    let counting = build_data_counting_classes(&internal, &dkey);
    assert!(counting.is_empty(), "value-membership must not be counting; got {counting:?}");
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p owl-dl-reasoner --lib builds_data_counting_classes_for_integer_cardinality 2>&1 | tail -15`
Expected: FAIL to compile — `build_data_counting_classes` not found.

- [ ] **Step 3: Implement the builder + helper**

Add immediately after the `build_dkey_range_map` fn in `crates/owl-dl-reasoner/src/lib.rs`. (Imports: `owl_dl_core::ontology::Axiom` and `owl_dl_core::ir::{ConceptExpr, ConceptId, ClassId, ConceptPool}` — use fully-qualified paths as below to avoid touching the file's `use` block.)

```rust
/// True if concept `c`'s expression contains (recursively) a `Min`/`Max`
/// cardinality whose filler is a DKey datatype-range class. Such a class's
/// satisfiability can hinge on a concrete-domain counting clash that the
/// hypertableau wedge cannot evaluate (it has no `card_sat` and does not
/// materialise DKey cardinality — see the wedge-hang fix).
fn concept_has_dkey_counting(
    pool: &owl_dl_core::ir::ConceptPool,
    c: owl_dl_core::ir::ConceptId,
    dkey_ranges: &std::collections::HashMap<owl_dl_core::ir::ClassId, owl_dl_datatypes::CardRange>,
) -> bool {
    use owl_dl_core::ir::ConceptExpr;
    match pool.get(c) {
        ConceptExpr::Min(_, _, inner) | ConceptExpr::Max(_, _, inner) => {
            if matches!(
                pool.get(*inner),
                ConceptExpr::Atomic(cls) if dkey_ranges.contains_key(cls)
            ) {
                return true;
            }
            concept_has_dkey_counting(pool, *inner, dkey_ranges)
        }
        ConceptExpr::Not(inner) => concept_has_dkey_counting(pool, *inner, dkey_ranges),
        ConceptExpr::Some(_, inner) | ConceptExpr::All(_, inner) => {
            concept_has_dkey_counting(pool, *inner, dkey_ranges)
        }
        ConceptExpr::And(ops) | ConceptExpr::Or(ops) => {
            ops.iter().any(|&o| concept_has_dkey_counting(pool, o, dkey_ranges))
        }
        _ => false,
    }
}

/// Named classes that carry a *counting* DKey constraint
/// (`DataMin/Max/ExactCardinality` over an integer range, lowered to
/// `Min`/`Max` over a DKey filler). Scanned from the *un-mutated* IR
/// (pre-absorb), where the raw `SubClassOf`/`EquivalentClasses` axioms
/// still carry the lowered concept. The classify unsat-probe verifies
/// these (and their saturation-subclasses) on the main tableau instead of
/// trusting the wedge's `Sat`. Empty unless the ontology has integer data
/// cardinality — keeps the fast wedge path for every value-membership-only
/// ontology (e.g. `sio`).
fn build_data_counting_classes(
    internal: &owl_dl_core::ontology::InternalOntology,
    dkey_ranges: &std::collections::HashMap<owl_dl_core::ir::ClassId, owl_dl_datatypes::CardRange>,
) -> std::collections::HashSet<owl_dl_core::ir::ClassId> {
    let mut set = std::collections::HashSet::new();
    if dkey_ranges.is_empty() {
        return set;
    }
    let pool = &internal.concepts;
    for ax in &internal.axioms {
        match ax {
            owl_dl_core::ontology::Axiom::SubClassOf { sub, sup } => {
                if let owl_dl_core::ir::ConceptExpr::Atomic(c) = pool.get(*sub) {
                    if concept_has_dkey_counting(pool, *sup, dkey_ranges) {
                        set.insert(*c);
                    }
                }
            }
            owl_dl_core::ontology::Axiom::EquivalentClasses(members) => {
                if members
                    .iter()
                    .any(|&m| concept_has_dkey_counting(pool, m, dkey_ranges))
                {
                    for &m in members {
                        if let owl_dl_core::ir::ConceptExpr::Atomic(c) = pool.get(m) {
                            set.insert(*c);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    set
}
```

- [ ] **Step 4: Add the field to `PreparedOntology` + populate it**

In the `PreparedOntology` struct definition (the block containing `pub(crate) dkey_ranges:` near line 1887), add after the `dkey_ranges` field:

```rust
    /// Named classes carrying a counting DKey constraint (see
    /// `build_data_counting_classes`). The classify unsat-probe
    /// main-tableau-verifies these instead of trusting the wedge's `Sat`.
    pub(crate) data_counting_classes:
        std::collections::HashSet<owl_dl_core::ir::ClassId>,
```

In `from_internal`, immediately after `let dkey_ranges = build_dkey_range_map(&internal);` (line ~1929):

```rust
        let data_counting_classes = build_data_counting_classes(&internal, &dkey_ranges);
```

In the `Ok(Self { ... })` initializer (after `dkey_ranges,` at line ~1996):

```rust
            data_counting_classes,
```

- [ ] **Step 5: Run the unit tests to verify they pass**

Run: `cargo test -p owl-dl-reasoner --lib data_counting 2>&1 | tail -10`
Expected: PASS — both `builds_data_counting_classes_for_integer_cardinality` and `no_data_counting_classes_for_value_membership_only`.

- [ ] **Step 6: Commit**

```bash
git add crates/owl-dl-reasoner/src/lib.rs
git commit -m "feat(datatypes): build data_counting_classes set in PreparedOntology

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Failing classify canary (the gap)

**Files:**
- Create: `crates/owl-dl-reasoner/tests/classify_concrete_domain.rs`

- [ ] **Step 1: Write the canary harness + first (failing) test**

```rust
//! Canaries for classify-level concrete-domain VERIFY: a class
//! unsatisfiable only by an integer counting clash (`≥3 p.[0,1]` capacity,
//! `≥3 ⊓ ≤2` conflict) must appear unsatisfiable via `classify` — not just
//! via `is_class_satisfiable`. Before this feature, classify trusted the
//! wedge's `Sat` (the wedge has no `card_sat`) and missed these.
//!
//! NEGATIVES-FIRST: the FP-critical direction is a satisfiable class wrongly
//! reported unsatisfiable. Every `assert!(sat(...))` is a genuinely
//! satisfiable data node that MUST stay satisfiable.
//!
//! Run: `cargo test -p owl-dl-reasoner --test classify_concrete_domain`.

#![allow(clippy::unwrap_used, clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::classify;
use std::io::Cursor;

const PFX: &str = "Prefix(:=<http://t/>)\nPrefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n";

/// Classify `body` and return true iff `:C` (`http://t/C`) is unsatisfiable.
fn c_unsat(body: &str) -> bool {
    let src = format!(
        "{PFX}Ontology(<http://t/o>\n  Declaration(Class(:C)) Declaration(DataProperty(:p))\n{body}\n)\n"
    );
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut Cursor::new(src), ParserConfiguration::default()).expect("parse ofn");
    classify(&onto)
        .expect("classify")
        .unsatisfiable_classes()
        .contains(&"http://t/C")
}

fn min_int(n: u32, lo: i64, hi: i64) -> String {
    format!(
        "  SubClassOf(:C DataMinCardinality({n} :p DatatypeRestriction(xsd:integer \
         xsd:minInclusive \"{lo}\"^^xsd:integer xsd:maxInclusive \"{hi}\"^^xsd:integer)))"
    )
}
fn max_int(n: u32, lo: i64, hi: i64) -> String {
    format!(
        "  SubClassOf(:C DataMaxCardinality({n} :p DatatypeRestriction(xsd:integer \
         xsd:minInclusive \"{lo}\"^^xsd:integer xsd:maxInclusive \"{hi}\"^^xsd:integer)))"
    )
}

/// Capacity: `≥3 p.[0,1]` demands 3 distinct integers, only 2 exist. UNSAT.
#[test]
fn capacity_clash_unsat_via_classify() {
    assert!(c_unsat(&min_int(3, 0, 1)));
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p owl-dl-reasoner --test classify_concrete_domain capacity_clash_unsat_via_classify 2>&1 | tail -15`
Expected: FAIL — `:C` is reported satisfiable (the wedge's trusted `Sat`), so `.contains(&"http://t/C")` is false.

- [ ] **Step 3: Commit the failing test**

```bash
git add crates/owl-dl-reasoner/tests/classify_concrete_domain.rs
git commit -m "test(datatypes): failing canary — classify misses counting clash

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: Wire the override into the unsat probe

**Files:**
- Modify: `crates/owl-dl-reasoner/src/classify.rs` (the `Some(LabelOracle::Sat(_))` arm, ~line 1095)

- [ ] **Step 1: Replace the wedge-`Sat` arm**

Find this block in the unsat-probe `into_par_iter().map(|i| { ... })` (around line 1092):

```rust
            if crate::unsat_via_labels_enabled() {
                match label_cache.get(i) {
                    Some(crate::LabelOracle::Unsat) => return Ok((i, false, true)),
                    Some(crate::LabelOracle::Sat(_)) => return Ok((i, true, true)),
                    Some(crate::LabelOracle::NoVerdict) | None => {}
                }
            }
```

Replace the `Some(crate::LabelOracle::Sat(_))` arm so it only trusts the wedge `Sat` when the class is NOT counting-constrained; otherwise it falls through to the main-tableau probe below:

```rust
            if crate::unsat_via_labels_enabled() {
                match label_cache.get(i) {
                    Some(crate::LabelOracle::Unsat) => return Ok((i, false, true)),
                    Some(crate::LabelOracle::Sat(_)) => {
                        // Concrete-domain verify: the wedge has no `card_sat`
                        // and does not materialise DKey cardinality, so it
                        // reports a counting-clash class `Sat`. For a class
                        // carrying a `Min`/`Max`-over-DKey constraint (or a
                        // saturation-subclass of one), don't trust that `Sat`
                        // — fall through to the main tableau (which runs
                        // `concrete_domain_clash`). Sound: only swaps a wedge
                        // `Sat` for the complete path. Empty set ⇒ no overhead.
                        let needs_verify = !prepared.data_counting_classes.is_empty()
                            && (prepared.data_counting_classes.contains(&class_id)
                                || closure
                                    .subsumers_of(class_id)
                                    .iter()
                                    .any(|s| prepared.data_counting_classes.contains(s)));
                        if !needs_verify {
                            return Ok((i, true, true));
                        }
                        // else: fall through to the main-tableau probe below.
                    }
                    Some(crate::LabelOracle::NoVerdict) | None => {}
                }
            }
```

- [ ] **Step 2: Run the canary to verify it passes**

Run: `cargo test -p owl-dl-reasoner --test classify_concrete_domain capacity_clash_unsat_via_classify 2>&1 | tail -10`
Expected: PASS — `:C` now appears in `unsatisfiable_classes()`.

- [ ] **Step 3: Run the full P3 + clause suites (no regression on what already worked)**

Run: `cargo test -p owl-dl-reasoner --test concrete_domain_clash 2>&1 | tail -3 && cargo test -p owl-dl-core --lib clause 2>&1 | tail -3`
Expected: both PASS (10 P3 canaries; clause tests incl. `dkey_data_cardinality_emits_no_cardinality_head`).

- [ ] **Step 4: Commit**

```bash
git add crates/owl-dl-reasoner/src/classify.rs
git commit -m "feat(datatypes): classify verifies counting-constrained classes on main tableau

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: Remaining utility + FP-gate canaries

**Files:**
- Modify: `crates/owl-dl-reasoner/tests/classify_concrete_domain.rs`

- [ ] **Step 1: Add the conflict, inheritance, and satisfiable-node tests**

Append to `crates/owl-dl-reasoner/tests/classify_concrete_domain.rs`:

```rust
/// Conflict: `≥3 p.[0,100]` with `≤2 p.[0,100]`. UNSAT via classify.
#[test]
fn min_max_conflict_unsat_via_classify() {
    assert!(c_unsat(&format!("{}\n{}", min_int(3, 0, 100), max_int(2, 0, 100))));
}

/// Inheritance: `D` carries `≥3 p.[0,1]`, `C ⊑ D`. Both unsat via classify
/// (exercises the saturation-subsumer downward check in the probe).
#[test]
fn inherited_counting_clash_unsat_via_classify() {
    let src = format!(
        "{PFX}Ontology(<http://t/o>\n  \
         Declaration(Class(:C)) Declaration(Class(:D)) Declaration(DataProperty(:p))\n  \
         SubClassOf(:D DataMinCardinality(3 :p DatatypeRestriction(xsd:integer \
         xsd:minInclusive \"0\"^^xsd:integer xsd:maxInclusive \"1\"^^xsd:integer)))\n  \
         SubClassOf(:C :D)\n)\n"
    );
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut Cursor::new(src), ParserConfiguration::default()).expect("parse ofn");
    let unsat = classify(&onto).expect("classify");
    let unsat = unsat.unsatisfiable_classes();
    assert!(unsat.contains(&"http://t/D"), "D unsat; got {unsat:?}");
    assert!(unsat.contains(&"http://t/C"), "C (⊑ D) unsat; got {unsat:?}");
}

// ─── FP GATE: satisfiable data nodes MUST stay satisfiable via classify ───

/// `∃p.[0,10]` (≥1, 11 ints). SAT.
#[test]
fn datasome_sat_via_classify() {
    assert!(!c_unsat(
        "  SubClassOf(:C DataSomeValuesFrom(:p DatatypeRestriction(xsd:integer \
         xsd:minInclusive \"0\"^^xsd:integer xsd:maxInclusive \"10\"^^xsd:integer)))"
    ));
}

/// Tight-but-feasible: `≥2 p.[0,1]` — exactly 2 ints. SAT.
#[test]
fn exactly_enough_sat_via_classify() {
    assert!(!c_unsat(&min_int(2, 0, 1)));
}

/// `≥2 p.[0,10]` with `≤5 p.[0,10]` — room to spare. SAT.
#[test]
fn min_under_max_sat_via_classify() {
    assert!(!c_unsat(&format!("{}\n{}", min_int(2, 0, 10), max_int(5, 0, 10))));
}

/// `≤1 p.[0,10]` alone — always feasible. SAT.
#[test]
fn datamax_alone_sat_via_classify() {
    assert!(!c_unsat(&max_int(1, 0, 10)));
}

/// Non-integer cardinality is not handled (dropped) — must NOT clash.
/// `≥3 p.{a,b}` (string oneOf) stays SAT.
#[test]
fn noninteger_cardinality_sat_via_classify() {
    assert!(!c_unsat("  SubClassOf(:C DataMinCardinality(3 :p DataOneOf(\"a\" \"b\")))"));
}
```

- [ ] **Step 2: Run all classify-concrete-domain canaries**

Run: `cargo test -p owl-dl-reasoner --test classify_concrete_domain 2>&1 | tail -15`
Expected: PASS — 3 unsat (capacity, conflict, inheritance) + 5 satisfiable-gate.

- [ ] **Step 3: Commit**

```bash
git add crates/owl-dl-reasoner/tests/classify_concrete_domain.rs
git commit -m "test(datatypes): classify concrete-domain canaries (conflict, inheritance, FP gate)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: D11b ∀+∃ membership-in-classify probe (the spec's test gate)

**Files:**
- Modify: `crates/owl-dl-reasoner/tests/classify_concrete_domain.rs`

- [ ] **Step 1: Add the membership-clash probe**

This tests whether the wedge already catches a `∃p.{5} ⊓ ∀p.[0,3]` membership clash (5 ∉ [0,3]) in classify. The spec predicts it does (the wedge has ∃-generation + ∀-propagation + DKey-disjointness). Append:

```rust
/// D11b probe (spec test gate): `∃p.{5} ⊓ ∀p.[0,3]`, 5 ∉ [0,3] ⟹ C unsat.
/// This is a *membership* clash (DKey disjointness), NOT counting — the
/// spec predicts the WEDGE already catches it in classify, so
/// `data_counting_classes` stays counting-only. If this FAILS, widen the
/// predicate to include ∀-over-DKey classes (see the spec).
#[test]
fn forall_exists_membership_clash_unsat_via_classify() {
    assert!(c_unsat(
        "  SubClassOf(:C DataHasValue(:p \"5\"^^xsd:integer))\n  \
         SubClassOf(:C DataAllValuesFrom(:p DatatypeRestriction(xsd:integer \
         xsd:minInclusive \"0\"^^xsd:integer xsd:maxInclusive \"3\"^^xsd:integer)))"
    ));
}
```

- [ ] **Step 2: Run the probe**

Run: `cargo test -p owl-dl-reasoner --test classify_concrete_domain forall_exists_membership_clash_unsat_via_classify 2>&1 | tail -15`
Expected: PASS (spec prediction).

- [ ] **Step 3: If it FAILS, widen the predicate (only if needed)**

Only if Step 2 fails: extend `concept_has_dkey_counting` in `crates/owl-dl-reasoner/src/lib.rs` so a `∀p.DKey` (`All` whose filler is a DKey class) also qualifies a class for verification. Replace the `All` handling — change the combined `Some | All` arm to handle `All` like `Min`/`Max`:

```rust
        ConceptExpr::Some(_, inner) => concept_has_dkey_counting(pool, *inner, dkey_ranges),
        ConceptExpr::All(_, inner) => {
            if matches!(
                pool.get(*inner),
                ConceptExpr::Atomic(cls) if dkey_ranges.contains_key(cls)
            ) {
                return true;
            }
            concept_has_dkey_counting(pool, *inner, dkey_ranges)
        }
```

Re-run Step 2 (now PASS), and re-run Task 1's unit tests (`cargo test -p owl-dl-reasoner --lib data_counting`) to confirm the value-membership test still holds — `DataSomeValuesFrom` is `Some`, not `All`, so it must still produce an empty set. Then note in the spec that the predicate was widened.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "test(datatypes): D11b ∀+∃ membership-in-classify probe (spec test gate)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 6: Non-regression gate + DoS probe + lint + docs

**Files:**
- Modify: `docs/superpowers/specs/2026-06-11-classify-concrete-domain-verify-design.md` (status)

- [ ] **Step 1: fmt + clippy (CI gates on -D warnings)**

Run:
```bash
cargo fmt --all -- --check && \
cargo clippy -p owl-dl-reasoner -p owl-dl-core --release --all-targets --all-features -- -D warnings 2>&1 | tail -6
```
Expected: fmt clean (rc 0); clippy `Finished` with no warnings.

- [ ] **Step 2: 1M-cardinality DoS probe still terminates fast**

Run:
```bash
cat > /tmp/bign.ofn <<'EOF'
Prefix(:=<http://t/>) Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology( Declaration(Class(:C)) Declaration(DataProperty(:p))
  SubClassOf(:C DataMinCardinality(1000000 :p DatatypeRestriction(xsd:integer xsd:minInclusive "0"^^xsd:integer xsd:maxInclusive "2"^^xsd:integer))))
EOF
cargo build -p owl-dl-cli --release 2>&1 | tail -1
t0=$(date +%s.%N); timeout 15 ./target/release/rustdl classify /tmp/bign.ofn 2>&1 | tail -2; t1=$(date +%s.%N)
echo "elapsed: $(echo "$t1-$t0"|bc)s"
```
Expected: terminates in well under 1s; `:C` reported unsatisfiable (`1000000` distinct ints over `{0,1,2}` is a capacity clash — now caught in classify via the verify). No timeout.

- [ ] **Step 3: Corpus closure-diff FP=0/MISSED=0 on data-bearing + general fixtures**

Run:
```bash
RUSTDL_TEST_PAIR_MS=50 cargo test -p owl-dl-reasoner --release --test konclude_closure_diff -- --ignored --nocapture \
  shoiq_knowledge_closure_matches_konclude sio_closure_matches_konclude wine_closure_matches_konclude \
  alehif_closure_matches_konclude bibtex_closure_matches_konclude 2>&1 | grep -iE "FP|MISS|test result|FAILED"
```
Expected: every line `FP=0 MISSED=0`; `test result: ok. 5 passed`.

- [ ] **Step 4: sio perf spot-check (no regression — value-membership stays on the fast path)**

Run (sio's DKeys are value-membership → `data_counting_classes` empty → no extra main-tableau runs):
```bash
./scripts/fetch-real-ontologies.sh >/dev/null 2>&1 || true
t0=$(date +%s.%N); ./target/release/rustdl classify ontologies/real/sio*.ofn >/dev/null 2>&1; t1=$(date +%s.%N)
echo "sio classify wall: $(echo "$t1-$t0"|bc)s"
```
Expected: within noise of the pre-change wall (the override never fires for sio). Record the number; if it regresses materially, the predicate is mis-classifying value-membership as counting — re-check `concept_has_dkey_counting` (only `Min`/`Max`, never `Some`).

- [ ] **Step 5: Update the spec status to "implemented"**

In `docs/superpowers/specs/2026-06-11-classify-concrete-domain-verify-design.md`, add a `## Status (implemented YYYY-MM-DD)` section near the top recording: commits, the D11b probe outcome (predicate stayed counting-only vs. widened), closure-diff FP=0/MISSED=0, sio wall, DoS-probe result.

- [ ] **Step 6: Final commit**

```bash
git add -A
git commit -m "docs(datatypes): mark classify concrete-domain verify implemented + metrics

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Notes for the implementer

- **Do NOT push.** All datatype-solver commits are local pending the user's "metrics demonstrate feasibility + utility" gate. This plan's commits join that queue.
- **Soundness is the whole point.** The override only ever replaces a wedge `Sat` with a main-tableau verdict; if you find yourself making the wedge return `Unsat` directly, you've drifted into the deferred Phase 2 (in-wedge clash) — stop and re-read the spec.
- The wedge-hang fix (`c4c61c2`, dropping DKey cardinality heads at clausify) **stays**. This plan composes with it: the wedge still safely says `Sat`; the override re-decides on the main tableau, which is suppression-guarded so it never materialises either.
- `closure.subsumers_of` allocates a `Vec`; the `!prepared.data_counting_classes.is_empty()` guard ensures it's only called when the ontology actually has integer data cardinality (corpus: never).
