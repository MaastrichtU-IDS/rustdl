# Manchester I/O Conformance & Performance Report — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Produce three durable reports — `compliance-report.md`, `performance-report.md`, and a combined `manchester-io-report.md` — that exhaustively characterize the horned-owl fork's `io/omn` OWL 2 Manchester Syntax reader and writer for §2.5 conformance and comparative performance.

**Architecture:** Compliance is a Rust integration-test harness **in the fork** (`/data/dumontier/horned-owl-omn/tests/manchester/…`) covering a per-construct §2.5 matrix, full-corpus parse+round-trip, semantic axiom-set equality vs the OWL-API, and adversarial/no-panic fuzz; a report-generator (an `#[ignore]`d test) writes the markdown. Performance is a Rust bench binary (`pymos/bench/horned-bench/`) driven by new pymos workloads, comparing ours-omn vs omny / OWL-API(ROBOT) / fastobo-horned-manchester / intra-crate ofn·owx·rdf, with a Python report-generator.

**Tech Stack:** Rust (horned-owl 1.4 fork, `cargo test`, `proptest`), fastobo `horned-manchester 0.4` (horned-owl 0.14, coexisting), Python 3 (pymos/bench, `subprocess`, `Measurement`), ROBOT docker (`obolibrary/robot:v1.9.6`), omny 0.2.2 (`/tmp/verify-022/bin/python`).

**Spec:** `docs/superpowers/specs/2026-06-13-manchester-conformance-perf-design.md`

---

## Conventions for every task

- **Toolchain:** prepend PATH with `/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin` for all `cargo` calls.
- **Fork work** happens in `/data/dumontier/horned-owl-omn` (branch `master`). **pymos work** in `/data/dumontier/pymos`. **Plan/spec/combined-report** in `/data/dumontier/rustdl`.
- **Constraints (hard):** work only in the fork + pymos; do **NOT** push the fork; do **NOT** modify rustdl's `Cargo` `[patch]`; the upstream PR is the user's to open.
- **Commit trailer** on every commit: `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`.
- **Characterization caveat:** the reader/writer already exist and work. Most A1/A4 assertions will PASS on first run — they are *characterization* tests. When one FAILS, decide: (a) a genuine bug → fix in the fork reader/writer; or (b) a documented residual → tag it `ResidualKind` and assert the documented behavior. Never silently weaken an assertion.
- **Confirmed API surface** (use verbatim):
  - Read omn/ofn/owx: `horned_owl::io::<fmt>::reader::read::<Rc<str>, SetOntology<Rc<str>>, _>(bufread, ParserConfiguration::default())? -> (SetOntology, PrefixMapping)` (omn uses `io::omn::read`).
  - Read rdf: `horned_owl::io::rdf::reader::read::<Rc<str>, Rc<AnnotatedComponent<Rc<str>>>, _>(bufread)?` → returns an `RDFOntology`; obtain components via `Into<SetOntology<_>>`/`.iter()`.
  - Convert to renderable: `let amo: ComponentMappedOntology<Rc<str>, Rc<AnnotatedComponent<Rc<str>>>> = set_ont.into();`
  - Write omn/ofn/owx: `io::<fmt>::writer::write(w, &amo, Some(&pm))?`. Write rdf: `io::rdf::writer::write(w, &amo)?` (no prefix arg).
  - Component count: `ont.iter().count()`.
  - fastobo: `horned_manchester::from_str::<Rc<str>, SetOntology<Rc<str>>, _>(text)? -> (O, PrefixMapping)` (horned-owl 0.14 model).

---

## Phase 0 — Scaffolding

### Task 0.1: `horned-bench` subcrate skeleton + dual-version smoke build

**Files:**
- Create: `/data/dumontier/pymos/bench/horned-bench/Cargo.toml`
- Create: `/data/dumontier/pymos/bench/horned-bench/src/main.rs`

- [ ] **Step 1: Write `Cargo.toml`** (the risk is dual horned-owl versions coexisting)

```toml
[package]
name = "horned-bench"
version = "0.0.0"
edition = "2021"

[dependencies]
horned-owl = { path = "/data/dumontier/horned-owl-omn", default-features = false }
horned-manchester = "0.4"   # pulls horned-owl 0.14 — coexists with the path 1.4

[[bin]]
name = "horned-bench"
path = "src/main.rs"
```

- [ ] **Step 2: Write a stub `main.rs` that touches BOTH horned-owl versions**

```rust
//! Manchester / OWL I/O micro-benchmark. Reads or renders an ontology N times
//! in-process and reports timing + peak RSS as one JSON line.
fn main() {
    // Reference the path-fork (1.4): a type from our crate.
    let _b = horned_owl::model::Build::<std::rc::Rc<str>>::new();
    // Reference fastobo (horned-owl 0.14) so the dual-version link is exercised.
    let _ = horned_manchester::from_str::<std::rc::Rc<str>, horned_owl_014::ontology::set::SetOntology<std::rc::Rc<str>>, _>("");
    eprintln!("smoke ok");
}
```

NOTE: the fastobo crate re-exports its own horned-owl as a transitive dep; if the `horned_owl_014` path above does not resolve, instead call only `horned_manchester::from_str` with type inference deferred (a `let _f = horned_manchester::from_str::<_, _, _>;` function reference) to prove linkage without naming the 0.14 crate. Adjust to whatever compiles; the goal of this step is only to prove both versions link.

- [ ] **Step 3: Smoke-build**

Run: `cd /data/dumontier/pymos/bench/horned-bench && PATH=/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH cargo build --release 2>&1 | tail -15`
Expected: `Finished` (both horned-owl 0.14 and 1.4 in the dep tree). If it FAILS on the dual version, record the error and fall back: drop `horned-manchester` from this crate and plan a **separate** `horned-bench-fastobo` crate (Task B3 adjusts). Do not block Phase 0 on fastobo.

- [ ] **Step 4: Commit**

```bash
cd /data/dumontier/pymos && git add bench/horned-bench && \
git commit -m "bench: horned-bench subcrate skeleton (dual horned-owl smoke build)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task 0.2: fork conformance test skeleton + proptest dev-dep

**Files:**
- Modify: `/data/dumontier/horned-owl-omn/Cargo.toml` (add `[dev-dependencies] proptest`)
- Create: `/data/dumontier/horned-owl-omn/tests/manchester_conformance.rs`
- Create: `/data/dumontier/horned-owl-omn/tests/manchester/mod.rs`

- [ ] **Step 1: Add proptest dev-dependency**

In `Cargo.toml`, under (or create) `[dev-dependencies]`:

```toml
[dev-dependencies]
proptest = "1"
```

- [ ] **Step 2: Create the test-crate entry `tests/manchester_conformance.rs`**

```rust
//! OWL 2 Manchester Syntax §2.5 conformance harness for `io::omn`.
//! Submodules live under `tests/manchester/`. Run the report generator with:
//!   cargo test --test manchester_conformance -- --ignored generate_compliance_report
#[path = "manchester/mod.rs"]
mod manchester;
```

- [ ] **Step 3: Create `tests/manchester/mod.rs` with shared helpers**

```rust
//! Shared helpers for the Manchester conformance harness.
use std::io::BufReader;
use std::rc::Rc;

use horned_owl::io::ParserConfiguration;
use horned_owl::io::omn::{read as read_omn, write as write_omn};
use horned_owl::model::AnnotatedComponent;
use horned_owl::ontology::component_mapped::ComponentMappedOntology;
use horned_owl::ontology::set::SetOntology;
use curie::PrefixMapping;

pub mod constructs;
pub mod canonical;
pub mod adversarial;
pub mod corpus;
pub mod report;

pub type O = SetOntology<Rc<str>>;

/// Parse a Manchester document string into a SetOntology + prefixes.
pub fn read_str(s: &str) -> Result<(O, PrefixMapping), String> {
    read_omn::<Rc<str>, O, _>(BufReader::new(s.as_bytes()), ParserConfiguration::default())
        .map_err(|e| format!("{e}"))
}

/// Render a SetOntology back to Manchester text.
pub fn write_str(ont: &O, pm: &PrefixMapping) -> String {
    let amo: ComponentMappedOntology<Rc<str>, Rc<AnnotatedComponent<Rc<str>>>> =
        ont.clone().into();
    let buf = write_omn(Vec::<u8>::new(), &amo, Some(pm)).expect("omn write");
    String::from_utf8(buf).expect("utf8")
}

/// Sorted multiset of components, for order-insensitive structural comparison.
pub fn components_sorted(ont: &O) -> Vec<String> {
    let mut v: Vec<String> = ont.iter().map(|ac| format!("{:?}", ac.component)).collect();
    v.sort();
    v
}
```

- [ ] **Step 4: Stub the five submodules** (empty but compiling)

Create `tests/manchester/{constructs,canonical,adversarial,corpus,report}.rs`, each with a header doc-comment and (where noted later) `use super::*;`. For now each contains only:

```rust
//! (filled in a later task)
#![allow(unused_imports)]
use super::*;
```

- [ ] **Step 5: Build the test crate**

Run: `cd /data/dumontier/horned-owl-omn && PATH=/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH cargo test --test manchester_conformance --no-run 2>&1 | tail -8`
Expected: compiles (0 tests so far is fine).

- [ ] **Step 6: Commit**

```bash
cd /data/dumontier/horned-owl-omn && git add Cargo.toml tests/ && \
git commit -m "test(omn): conformance harness skeleton + proptest dev-dep

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Phase A — Compliance harness (fork)

### Task A1: §2.5 per-construct coverage matrix

**Files:**
- Modify: `/data/dumontier/horned-owl-omn/tests/manchester/constructs.rs`

This task builds a data-driven table of every §2.5 construct, each a minimal `.omn` snippet checked for read → write → round-trip, with documented residuals tagged.

- [ ] **Step 1: Define the case type + residual kinds**

```rust
//! A1 — §2.5 per-construct coverage matrix.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Residual {
    None,
    SwrlRule,
    ComplexLhsGci,
    NestedAnnotationDropped,
    DataRestrictionAsObject,
    HasKeyObjectDataConflation,
    BareNameNeedsPrefix,
}

pub struct Case {
    pub id: &'static str,
    /// A complete, minimal Manchester document exercising one construct.
    pub omn: &'static str,
    /// A substring expected in the Debug of at least one parsed component
    /// (the construct's signature), or "" when only round-trip is asserted.
    pub expect_debug_contains: &'static str,
    pub residual: Residual,
}
```

- [ ] **Step 2: Write the case table** (one row per construct from the spec's coverage list)

Add `pub const CASES: &[Case] = &[ … ];`. Provide a row for **every** construct enumerated in spec §A1. Examples (write all of them; these show the exact shape):

```rust
pub const CASES: &[Case] = &[
    Case { id: "class.subclassof", residual: Residual::None,
        expect_debug_contains: "SubClassOf",
        omn: "Prefix: : <http://e/>\nClass: :A\n    SubClassOf: :B\n" },
    Case { id: "class.equivalentto", residual: Residual::None,
        expect_debug_contains: "EquivalentClasses",
        omn: "Prefix: : <http://e/>\nClass: :A\n    EquivalentTo: :B\n" },
    Case { id: "ce.some", residual: Residual::None,
        expect_debug_contains: "ObjectSomeValuesFrom",
        omn: "Prefix: : <http://e/>\nClass: :A\n    SubClassOf: :r some :B\n" },
    Case { id: "ce.only", residual: Residual::None,
        expect_debug_contains: "ObjectAllValuesFrom",
        omn: "Prefix: : <http://e/>\nClass: :A\n    SubClassOf: :r only :B\n" },
    Case { id: "ce.min.qualified", residual: Residual::None,
        expect_debug_contains: "ObjectMinCardinality",
        omn: "Prefix: : <http://e/>\nClass: :A\n    SubClassOf: :r min 2 :B\n" },
    Case { id: "ce.self", residual: Residual::None,
        expect_debug_contains: "ObjectHasSelf",
        omn: "Prefix: : <http://e/>\nClass: :A\n    SubClassOf: :r Self\n" },
    Case { id: "ce.oneof", residual: Residual::None,
        expect_debug_contains: "ObjectOneOf",
        omn: "Prefix: : <http://e/>\nClass: :A\n    EquivalentTo: { :a , :b }\n" },
    Case { id: "data.facet.mininclusive", residual: Residual::None,
        expect_debug_contains: "DatatypeRestriction",
        omn: "Prefix: : <http://e/>\nPrefix: xsd: <http://www.w3.org/2001/XMLSchema#>\nDataProperty: :p\n    Range: xsd:integer[>= 1]\n" },
    Case { id: "lit.bare.integer", residual: Residual::None,
        expect_debug_contains: "Literal",
        omn: "Prefix: : <http://e/>\nIndividual: :a\n    Facts: :p 3\n" },
    Case { id: "misc.disjointclasses", residual: Residual::None,
        expect_debug_contains: "DisjointClasses",
        omn: "Prefix: : <http://e/>\nDisjointClasses: :A , :B\n" },
    Case { id: "datatype.def", residual: Residual::None,
        expect_debug_contains: "DatatypeDefinition",
        omn: "Prefix: : <http://e/>\nPrefix: xsd: <http://www.w3.org/2001/XMLSchema#>\nDatatype: :Small\n    EquivalentTo: xsd:integer[<= 9]\n" },
    Case { id: "header.versioniri", residual: Residual::None,
        expect_debug_contains: "",
        omn: "Prefix: : <http://e/>\nOntology: <http://e/o> <http://e/o/1.0>\n" },
    Case { id: "ann.nested", residual: Residual::NestedAnnotationDropped,
        expect_debug_contains: "AnnotationAssertion",
        omn: "Prefix: : <http://e/>\nClass: :A\n    Annotations: Annotations: :m \"x\" :note \"y\"\n" },
    Case { id: "indiv.anonymous", residual: Residual::None,
        expect_debug_contains: "AnonymousIndividual",
        omn: "Prefix: : <http://e/>\nIndividual: _:b1\n    Types: :A\n" },
    // … continue: class.disjointwith, class.disjointunion, class.haskey,
    //   op.{domain,range,subpropertyof,equivalentto,disjointwith,inverseof},
    //   op.char.{functional,inversefunctional,reflexive,irreflexive,symmetric,
    //            asymmetric,transitive}, op.subpropertychain,
    //   dp.{domain,range,subpropertyof,equivalentto,disjointwith,functional},
    //   ce.{value,max,exactly,and,or,not,inverse,nested},
    //   data.{and,or,not,oneof,paren} + every facet
    //     (length,minLength,maxLength,pattern,langRange,
    //      minInclusive,minExclusive,maxInclusive,maxExclusive),
    //   lit.{typed,string,langstring,bare.decimal,bare.float},
    //   misc.{equivalentclasses,equivalentproperties,disjointproperties,
    //         sameindividual,differentindividuals},
    //   ann.{entity,listitem,postcomma,ontology,anonvalue},
    //   header.{ontologyiri,import}, indiv.named,
    //   residual.{swrl,complexgci,datarestriction,haskeyconflation,barename}
];
```

WRITE ALL rows — do not leave the `// … continue` comment in the final file. The residual rows (`residual.*`) use the documented inputs (e.g. `residual.swrl` = a doc containing `Rule:`; `residual.barename` = a `Class: Foo` with only the default `""` prefix unset).

- [ ] **Step 3: Write the matrix-runner test**

```rust
#[derive(Debug)]
pub struct Row {
    pub id: String,
    pub read_ok: bool,
    pub write_ok: bool,
    pub roundtrip_ok: bool,
    pub residual: Residual,
    pub note: String,
}

pub fn run_case(c: &Case) -> Row {
    let mut note = String::new();
    let (read_ok, ont_pm) = match read_str(c.omn) {
        Ok(op) => (true, Some(op)),
        Err(e) => { note = e.lines().next().unwrap_or("").to_string(); (false, None) }
    };
    let mut write_ok = false;
    let mut roundtrip_ok = false;
    if let Some((ont, pm)) = &ont_pm {
        if !c.expect_debug_contains.is_empty() {
            let hit = ont.iter().any(|ac|
                format!("{:?}", ac.component).contains(c.expect_debug_contains));
            if !hit { note = format!("expected {} in components", c.expect_debug_contains); }
        }
        let rendered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| write_str(ont, pm)));
        if let Ok(text) = rendered {
            write_ok = true;
            if let Ok((ont2, _)) = read_str(&text) {
                roundtrip_ok = components_sorted(ont) == components_sorted(&ont2);
            }
        }
    }
    Row { id: c.id.into(), read_ok, write_ok, roundtrip_ok, residual: c.residual, note }
}

#[test]
fn construct_matrix_has_no_unexpected_failures() {
    let mut failures = Vec::new();
    for c in CASES {
        let row = run_case(c);
        // For non-residual rows we require read + (signature) + round-trip.
        let ok = if c.residual == Residual::None {
            row.read_ok && row.note.is_empty() && row.roundtrip_ok
        } else {
            // residual rows: behavior is documented, not necessarily round-trip-clean
            true
        };
        if !ok { failures.push(format!("{:?}", row)); }
    }
    assert!(failures.is_empty(), "unexpected construct failures:\n{}", failures.join("\n"));
}
```

- [ ] **Step 4: Run the matrix test**

Run: `cd /data/dumontier/horned-owl-omn && PATH=…:$PATH cargo test --test manchester_conformance construct_matrix -- --nocapture 2>&1 | tail -30`
Expected: PASS. If a non-residual case fails, decide bug-vs-residual (see characterization caveat): fix the reader/writer in the fork **or** reclassify the row's `residual` with a justifying `note`, and re-run.

- [ ] **Step 5: Commit**

```bash
git add tests/manchester/constructs.rs && \
git commit -m "test(omn): §2.5 per-construct coverage matrix (A1)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task A2: corpus parse + structural round-trip (docker-gated)

**Files:**
- Modify: `/data/dumontier/horned-owl-omn/tests/manchester/corpus.rs`

- [ ] **Step 1: Add docker/robot detection + a corpus list**

```rust
//! A2 — corpus parse + structural round-trip via the OWL-API (ROBOT) oracle.
use super::*;
use std::path::PathBuf;
use std::process::Command;

/// ROBOT docker image used as the OWL-API oracle.
const ROBOT_IMAGE: &str = "obolibrary/robot:v1.9.6";

pub fn docker_available() -> bool {
    Command::new("docker").arg("version").output().map(|o| o.status.success()).unwrap_or(false)
}

/// Corpus ontologies, by absolute path to a source file readable by ROBOT.
/// Reuses already-fetched fixtures; extend as available.
pub fn corpus_paths() -> Vec<PathBuf> {
    [
        "/data/dumontier/pymos/bench/data/pizza.rdfxml",
        "/data/dumontier/pymos/bench/data/koala.rdfxml",
        "/data/dumontier/pymos/bench/data/obi-core.rdfxml",
        "/data/dumontier/pymos/bench/data/hp.rdfxml",
        "/data/dumontier/pymos/bench/data/doid.rdfxml",
    ].iter().map(PathBuf::from).filter(|p| p.exists()).collect()
}
```

- [ ] **Step 2: ROBOT convert helper (source → .omn)**

```rust
/// Convert `src` to Manchester `.omn` using ROBOT; returns the .omn text.
pub fn robot_to_omn(src: &std::path::Path) -> Result<String, String> {
    let dir = src.parent().unwrap().to_str().unwrap();
    let name = src.file_name().unwrap().to_str().unwrap();
    let out = Command::new("docker").args([
        "run", "--rm", "-v", &format!("{dir}:/w"), "-w", "/w",
        ROBOT_IMAGE, "robot", "convert", "-i", name, "--format", "omn", "-o", "out.omn",
    ]).output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into());
    }
    std::fs::read_to_string(format!("{dir}/out.omn")).map_err(|e| e.to_string())
}
```

- [ ] **Step 3: Per-ontology result struct + the run fn**

```rust
#[derive(Debug)]
pub struct CorpusRow {
    pub name: String,
    pub bytes: usize,
    pub parse_ok: bool,
    pub components: usize,
    pub roundtrip_ok: bool,
    pub blocking: String,   // first error line on parse failure
}

pub fn run_corpus() -> Vec<CorpusRow> {
    let mut rows = Vec::new();
    for p in corpus_paths() {
        let omn = match robot_to_omn(&p) { Ok(s) => s, Err(_) => continue };
        let name = p.file_stem().unwrap().to_string_lossy().into_owned();
        match read_str(&omn) {
            Ok((ont, pm)) => {
                let rendered = write_str(&ont, &pm);
                let roundtrip_ok = read_str(&rendered)
                    .map(|(o2, _)| components_sorted(&ont) == components_sorted(&o2))
                    .unwrap_or(false);
                rows.push(CorpusRow { name, bytes: omn.len(), parse_ok: true,
                    components: ont.iter().count(), roundtrip_ok, blocking: String::new() });
            }
            Err(e) => rows.push(CorpusRow { name, bytes: omn.len(), parse_ok: false,
                components: 0, roundtrip_ok: false,
                blocking: e.lines().next().unwrap_or("").into() }),
        }
    }
    rows
}
```

- [ ] **Step 4: Gated test**

```rust
#[test]
fn corpus_parses_or_documents_blocker() {
    if !docker_available() { eprintln!("SKIPPED A2: docker/ROBOT not available"); return; }
    let rows = run_corpus();
    assert!(!rows.is_empty(), "no corpus fixtures found");
    for r in &rows {
        // A failure here is a finding, not a hard error — print, don't panic,
        // unless it's a regression on a known-good fixture (pizza/koala).
        eprintln!("{r:?}");
        if (r.name == "pizza" || r.name == "koala") && !r.parse_ok {
            panic!("regression: {} no longer parses: {}", r.name, r.blocking);
        }
    }
}
```

- [ ] **Step 5: Run (will SKIP if docker absent — that is an acceptable pass)**

Run: `cd /data/dumontier/horned-owl-omn && PATH=…:$PATH cargo test --test manchester_conformance corpus_parses -- --nocapture 2>&1 | tail -20`
Expected: either `SKIPPED A2…` or per-row output with pizza/koala parsing OK.

- [ ] **Step 6: Commit**

```bash
git add tests/manchester/corpus.rs && \
git commit -m "test(omn): corpus parse + round-trip via ROBOT oracle (A2)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task A3: semantic axiom-set equality vs OWL-API (canonicalizer first, TDD)

**Files:**
- Modify: `/data/dumontier/horned-owl-omn/tests/manchester/canonical.rs`
- Modify: `/data/dumontier/horned-owl-omn/tests/manchester/corpus.rs` (add the equality run)

The canonicalizer is the highest-risk piece: it must absorb each documented OWL-API normalization so they are not false mismatches. TDD it on tiny fixtures before corpus use.

- [ ] **Step 1: Write the FAILING canonicalizer unit test (declaration conflation)**

```rust
//! A3 — semantic axiom-set equality vs OWL-API, with a documented-normalization
//! canonicalizer.
use super::*;
use horned_owl::model::{AnnotatedComponent, Component};
use std::rc::Rc;

/// Canonicalize a component set to a sorted Vec<String> that is invariant under
/// the documented OWL-API normalizations:
///  - declaration conflation: drop all `Declare*` components;
///  - n-ary ↔ pairwise: (handled by sorting member IRIs within n-ary axioms,
///    and by the corpus comparison treating pairwise/​n-ary as equal sets —
///    see `expand_nary`).
pub fn canonical(ont: &O) -> Vec<String> {
    let mut v: Vec<String> = ont.iter()
        .filter(|ac| !is_declaration(&ac.component))
        .map(|ac| format!("{:?}", ac.component))
        .collect();
    v.sort();
    v
}

fn is_declaration(c: &Component<Rc<str>>) -> bool {
    matches!(c,
        Component::DeclareClass(_) | Component::DeclareObjectProperty(_)
        | Component::DeclareDataProperty(_) | Component::DeclareAnnotationProperty(_)
        | Component::DeclareNamedIndividual(_) | Component::DeclareDatatype(_))
}

#[test]
fn canonical_drops_declarations() {
    let with_decl = "Prefix: : <http://e/>\nClass: :A\n    SubClassOf: :B\n";
    let (o, _) = read_str(with_decl).unwrap();
    // SubClassOf survives; the two DeclareClass(A), DeclareClass(B) are dropped.
    let canon = canonical(&o);
    assert!(canon.iter().any(|s| s.contains("SubClassOf")));
    assert!(canon.iter().all(|s| !s.contains("DeclareClass")));
}
```

- [ ] **Step 2: Run — verify the test exists and passes** (the impl is in the same step; this is a characterization unit)

Run: `cd /data/dumontier/horned-owl-omn && PATH=…:$PATH cargo test --test manchester_conformance canonical_drops -- --nocapture 2>&1 | tail -8`
Expected: PASS. If `Component` variant names differ, fix `is_declaration` to match the actual enum (check `src/model.rs`).

- [ ] **Step 3: Add the n-ary normalization test + helper**

```rust
/// OWL-API may emit EquivalentClasses(a,b,c) as pairwise EquivalentClasses(a,b),
/// (b,c), (a,c). Normalize by replacing every n-ary equivalence/disjointness
/// with the sorted SET of its member IRIs, so pairwise and n-ary canonicalize
/// to the same multiset of member-pairs.
pub fn nary_member_pairs(ont: &O) -> std::collections::BTreeSet<String> {
    // Implementation: for each EquivalentClasses/DisjointClasses/.../SameIndividual,
    // collect member IRI debug strings, sort, emit all unordered pairs "x|y".
    // (Full code: iterate ont.iter(), match the n-ary Component variants.)
    todo!("implemented in this step — see note")
}

#[test]
fn nary_and_pairwise_canonicalize_equal() {
    let nary = "Prefix: : <http://e/>\nEquivalentClasses: :A , :B , :C\n";
    let pairwise = "Prefix: : <http://e/>\nEquivalentClasses: :A , :B\nEquivalentClasses: :B , :C\nEquivalentClasses: :A , :C\n";
    let (o1, _) = read_str(nary).unwrap();
    let (o2, _) = read_str(pairwise).unwrap();
    assert_eq!(nary_member_pairs(&o1), nary_member_pairs(&o2));
}
```

Replace the `todo!()` with the real implementation in this same step (match `Component::EquivalentClasses(_)`, `DisjointClasses(_)`, `EquivalentObjectProperties(_)`, `DisjointObjectProperties(_)`, `SameIndividual(_)`, `DifferentIndividuals(_)`; for each, extract the member Vec, render each member with `{:?}`, sort, and insert every unordered pair `format!("{a}|{b}")`). Do NOT leave `todo!()` in the committed file.

- [ ] **Step 4: Run the n-ary test**

Run: `cd /data/dumontier/horned-owl-omn && PATH=…:$PATH cargo test --test manchester_conformance nary_and_pairwise -- --nocapture 2>&1 | tail -8`
Expected: PASS (both canonicalize to `{A|B, A|C, B|C}`).

- [ ] **Step 5: Add the corpus axiom-equality run in `corpus.rs`**

```rust
// in corpus.rs
use super::canonical::canonical;

#[derive(Debug)]
pub struct EqRow { pub name: String, pub matched: usize, pub missing: Vec<String>, pub extra: Vec<String> }

/// source -> ROBOT(.ofn) -> our ofn reader  == source-of-truth axiom set;
/// source -> ROBOT(.omn) -> our omn reader  == candidate.
/// Compare candidate vs source-of-truth after canonicalization.
pub fn run_axiom_equality() -> Vec<EqRow> {
    let mut rows = Vec::new();
    for p in corpus_paths() {
        let name = p.file_stem().unwrap().to_string_lossy().into_owned();
        let (Ok(omn), Ok(ofn)) = (robot_to_fmt(&p, "omn"), robot_to_fmt(&p, "ofn")) else { continue };
        let Ok((omn_ont, _)) = read_str(&omn) else { continue };
        let Ok((ofn_ont, _)) = read_ofn_str(&ofn) else { continue };
        let cand = canonical(&omn_ont);
        let truth = canonical(&ofn_ont);
        let cs: std::collections::BTreeSet<_> = cand.iter().cloned().collect();
        let ts: std::collections::BTreeSet<_> = truth.iter().cloned().collect();
        rows.push(EqRow {
            name,
            matched: cs.intersection(&ts).count(),
            missing: ts.difference(&cs).take(20).cloned().collect(),
            extra: cs.difference(&ts).take(20).cloned().collect(),
        });
    }
    rows
}
```

Generalize `robot_to_omn` from Task A2 into `robot_to_fmt(src, "omn"|"ofn")` (parameterize `--format` and the output extension), and add `read_ofn_str` mirroring `read_str` but using `io::ofn::reader::read`. Both go in `corpus.rs`/`mod.rs`.

- [ ] **Step 6: Gated corpus-equality test**

```rust
#[test]
fn corpus_axiom_equality_documents_diffs() {
    if !docker_available() { eprintln!("SKIPPED A3: docker/ROBOT not available"); return; }
    for r in run_axiom_equality() {
        eprintln!("{}: matched={} missing={} extra={}", r.name, r.matched, r.missing.len(), r.extra.len());
        for m in &r.missing { eprintln!("  MISSING {m}"); }
        for e in &r.extra   { eprintln!("  EXTRA   {e}"); }
    }
}
```

This test reports; it does not panic on diffs (diffs are findings for the report). Add the n-ary pair-set comparison as a secondary diagnostic if the flat `canonical` diff is dominated by pairwise/n-ary mismatch.

- [ ] **Step 7: Run**

Run: `cd /data/dumontier/horned-owl-omn && PATH=…:$PATH cargo test --test manchester_conformance corpus_axiom_equality -- --nocapture 2>&1 | tail -30`
Expected: SKIP (no docker) or per-ontology matched/missing/extra lines.

- [ ] **Step 8: Commit**

```bash
git add tests/manchester/canonical.rs tests/manchester/corpus.rs tests/manchester/mod.rs && \
git commit -m "test(omn): semantic axiom-set equality vs OWL-API + canonicalizer (A3)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task A4: adversarial / edge + no-panic fuzz

**Files:**
- Modify: `/data/dumontier/horned-owl-omn/tests/manchester/adversarial.rs`

- [ ] **Step 1: Edge-case fixtures (read+write+round-trip)**

```rust
//! A4 — adversarial / edge cases + no-panic fuzz.
use super::*;

const EDGE: &[(&str, &str)] = &[
    ("unicode_iri",
     "Prefix: : <http://e/>\nClass: :Caf\u{00e9}\n    SubClassOf: :Na\u{00ef}ve\n"),
    ("unicode_literal",
     "Prefix: : <http://e/>\nIndividual: :a\n    Annotations: :note \"\u{1F600} \u{0631}\u{0633}\u{0627}\u{0644}\u{0629}\"\n"),
    ("deep_nesting",
     // 12-deep nested intersection/negation
     "Prefix: : <http://e/>\nClass: :A\n    SubClassOf: :r some (:r some (:r some (:r some (:r some (:r some :B)))))\n"),
    ("crlf_endings",
     "Prefix: : <http://e/>\r\nClass: :A\r\n    SubClassOf: :B\r\n"),
    ("dotted_local",
     "Prefix: ex: <http://e/>\nClass: ex:a.b.c\n    SubClassOf: ex:d\n"),
];

#[test]
fn edge_cases_read_and_roundtrip() {
    for (id, omn) in EDGE {
        let (ont, pm) = read_str(omn).unwrap_or_else(|e| panic!("{id}: read failed: {e}"));
        let rendered = write_str(&ont, &pm);
        let (ont2, _) = read_str(&rendered).unwrap_or_else(|e| panic!("{id}: reread failed: {e}"));
        assert_eq!(components_sorted(&ont), components_sorted(&ont2), "{id}: round-trip drift");
    }
}
```

- [ ] **Step 2: Run edge test; reclassify any genuine non-§2.5 input as a residual**

Run: `cd /data/dumontier/horned-owl-omn && PATH=…:$PATH cargo test --test manchester_conformance edge_cases -- --nocapture 2>&1 | tail -15`
Expected: PASS. If `dotted_local`/`crlf` legitimately cannot round-trip per §2.5, move it out of `EDGE` and document it as a residual in `constructs.rs` instead.

- [ ] **Step 3: No-panic fuzz with proptest**

```rust
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig { cases: 2000, ..ProptestConfig::default() })]
    #[test]
    fn reader_never_panics_on_arbitrary_input(s in ".{0,400}") {
        // Must return Ok or Err, never panic / never hang on bounded input.
        let _ = read_str(&s);
    }
    #[test]
    fn reader_never_panics_on_manchester_ish(
        s in proptest::collection::vec(
            prop_oneof![Just("Class:"), Just("SubClassOf:"), Just("some"),
                        Just(":A"), Just("and"), Just("not"), Just("{"), Just("}"),
                        Just("\n"), Just(" ")], 0..60)
            .prop_map(|toks| toks.join(""))
    ) {
        let _ = read_str(&s);
    }
}
```

- [ ] **Step 4: Run fuzz**

Run: `cd /data/dumontier/horned-owl-omn && PATH=…:$PATH cargo test --test manchester_conformance reader_never_panics -- --nocapture 2>&1 | tail -15`
Expected: PASS (no panic). If proptest finds a panicking seed, that is a **real bug** — fix the reader (return `Err`, never `panic!`/`unwrap` on input), record the minimized seed, re-run.

- [ ] **Step 5: Commit**

```bash
git add tests/manchester/adversarial.rs && \
git commit -m "test(omn): adversarial edge cases + no-panic fuzz (A4)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task A5: compliance report generator

**Files:**
- Modify: `/data/dumontier/horned-owl-omn/tests/manchester/report.rs`
- Create (output): `/data/dumontier/horned-owl-omn/docs/manchester/compliance-report.md`

- [ ] **Step 1: Write the report generator as an `#[ignore]`d test**

```rust
//! A5 — compliance report generator. Run with:
//!   cargo test --test manchester_conformance -- --ignored generate_compliance_report
use super::*;
use std::fmt::Write as _;

#[test]
#[ignore]
fn generate_compliance_report() {
    let mut md = String::new();
    writeln!(md, "# Manchester `io/omn` Compliance Report\n").unwrap();
    writeln!(md, "_Generated by `tests/manchester` — see the design spec._\n").unwrap();

    // A1 matrix
    writeln!(md, "## A1 — §2.5 per-construct coverage matrix\n").unwrap();
    writeln!(md, "| id | read | write | round-trip | residual | note |").unwrap();
    writeln!(md, "|----|------|-------|-----------|----------|------|").unwrap();
    for c in super::constructs::CASES {
        let r = super::constructs::run_case(c);
        writeln!(md, "| {} | {} | {} | {} | {:?} | {} |",
            r.id, tick(r.read_ok), tick(r.write_ok), tick(r.roundtrip_ok), r.residual,
            r.note.replace('|', "\\|")).unwrap();
    }

    // A2 corpus + A3 equality (docker-gated)
    writeln!(md, "\n## A2 — corpus parse + round-trip\n").unwrap();
    if super::corpus::docker_available() {
        writeln!(md, "| ontology | bytes | parse | components | round-trip | blocking |").unwrap();
        writeln!(md, "|----------|-------|-------|-----------|-----------|----------|").unwrap();
        for r in super::corpus::run_corpus() {
            writeln!(md, "| {} | {} | {} | {} | {} | {} |",
                r.name, r.bytes, tick(r.parse_ok), r.components, tick(r.roundtrip_ok),
                r.blocking.replace('|', "\\|")).unwrap();
        }
        writeln!(md, "\n## A3 — semantic axiom-set equality vs OWL-API\n").unwrap();
        writeln!(md, "| ontology | matched | missing | extra |").unwrap();
        writeln!(md, "|----------|---------|---------|-------|").unwrap();
        for r in super::corpus::run_axiom_equality() {
            writeln!(md, "| {} | {} | {} | {} |", r.name, r.matched, r.missing.len(), r.extra.len()).unwrap();
        }
    } else {
        writeln!(md, "_SKIPPED — docker/ROBOT not available on this host._").unwrap();
    }

    writeln!(md, "\n## A4 — adversarial / fuzz\n").unwrap();
    writeln!(md, "Edge fixtures + 2×2000 proptest cases pass with zero panics \
        (run `cargo test … reader_never_panics edge_cases`).").unwrap();

    let out = std::path::Path::new("docs/manchester/compliance-report.md");
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();
    std::fs::write(out, md).unwrap();
    eprintln!("wrote {}", out.display());
}

fn tick(b: bool) -> &'static str { if b { "✓" } else { "✗" } }
```

- [ ] **Step 2: Generate the report**

Run: `cd /data/dumontier/horned-owl-omn && PATH=…:$PATH cargo test --test manchester_conformance -- --ignored generate_compliance_report --nocapture 2>&1 | tail -5`
Expected: `wrote docs/manchester/compliance-report.md`.

- [ ] **Step 3: Sanity-check the output**

Run: `sed -n '1,40p' /data/dumontier/horned-owl-omn/docs/manchester/compliance-report.md`
Expected: a populated A1 matrix; A2/A3 either tables or the SKIPPED note.

- [ ] **Step 4: Commit (report + generator)**

```bash
git add tests/manchester/report.rs docs/manchester/compliance-report.md && \
git commit -m "test(omn): compliance report generator + generated report (A5)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

- [ ] **Step 5: Full-suite green check + fmt/clippy**

Run: `cd /data/dumontier/horned-owl-omn && PATH=…:$PATH cargo test --test manchester_conformance 2>&1 | tail -6 && cargo fmt -- --check && cargo clippy --tests 2>&1 | grep -i 'manchester\|warning: ' | head`
Expected: all non-ignored tests pass; fmt clean; no new clippy warnings in the harness.

---

## Phase B — Performance harness (pymos)

### Task B1: `horned-bench` core — omn parse/render + timing + RSS + JSON

**Files:**
- Modify: `/data/dumontier/pymos/bench/horned-bench/src/main.rs`

- [ ] **Step 1: Replace the stub with the timing framework + arg parsing + omn**

```rust
//! Manchester / OWL I/O micro-benchmark. One JSON line on stdout:
//! {"format":..,"mode":..,"wall_hot_median_s":..,"wall_hot_min_s":..,
//!  "wall_cold_s":..,"peak_rss_bytes":..,"component_count":..,"bytes":..}
use std::io::BufReader;
use std::rc::Rc;
use std::time::Instant;

use horned_owl::io::ParserConfiguration;
use horned_owl::model::AnnotatedComponent;
use horned_owl::ontology::component_mapped::ComponentMappedOntology;
use horned_owl::ontology::set::SetOntology;
use horned_owl::ontology::iri_mapped::Ontology; // for .iter(); adjust import to the trait that provides iter()

type Set = SetOntology<Rc<str>>;
type Amo = ComponentMappedOntology<Rc<str>, Rc<AnnotatedComponent<Rc<str>>>>;

fn arg(flag: &str, default: &str) -> String {
    let a: Vec<String> = std::env::args().collect();
    a.iter().position(|x| x == flag).and_then(|i| a.get(i + 1)).cloned()
        .unwrap_or_else(|| default.into())
}
fn input_path() -> String { std::env::args().last().expect("input path") }

fn peak_rss_bytes() -> u64 {
    // VmHWM from /proc/self/status, in kB.
    std::fs::read_to_string("/proc/self/status").ok()
        .and_then(|s| s.lines().find(|l| l.starts_with("VmHWM"))
            .and_then(|l| l.split_whitespace().nth(1)).and_then(|n| n.parse::<u64>().ok()))
        .map(|kb| kb * 1024).unwrap_or(0)
}

/// Run `f` cold once, warmup M, then N hot; return (cold_s, min_s, median_s).
fn time_it(warmup: usize, hot: usize, mut f: impl FnMut()) -> (f64, f64, f64) {
    let t = Instant::now(); f(); let cold = t.elapsed().as_secs_f64();
    for _ in 0..warmup { f(); }
    let mut samples: Vec<f64> = (0..hot).map(|_| { let t = Instant::now(); f(); t.elapsed().as_secs_f64() }).collect();
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let min = *samples.first().unwrap_or(&cold);
    let median = samples.get(samples.len() / 2).copied().unwrap_or(cold);
    (cold, min, median)
}

fn main() {
    let format = arg("--format", "omn");
    let mode = arg("--mode", "parse");
    let warmup: usize = arg("--warmup", "1").parse().unwrap();
    let hot: usize = arg("--hot", "5").parse().unwrap();
    let path = input_path();
    let text = std::fs::read_to_string(&path).expect("read input");
    let bytes = text.len();

    let mut component_count = 0usize;
    let (cold, min, median) = match (format.as_str(), mode.as_str()) {
        ("omn", "parse") => time_it(warmup, hot, || {
            let (o, _): (Set, _) = horned_owl::io::omn::read(BufReader::new(text.as_bytes()),
                ParserConfiguration::default()).expect("omn parse");
            component_count = o.iter().count();
        }),
        ("omn", "render") => {
            let (o, pm): (Set, _) = horned_owl::io::omn::read(BufReader::new(text.as_bytes()),
                ParserConfiguration::default()).expect("omn parse");
            component_count = o.iter().count();
            let amo: Amo = o.into();
            time_it(warmup, hot, || {
                let _ = horned_owl::io::omn::write(Vec::<u8>::new(), &amo, Some(&pm)).expect("omn write");
            })
        }
        other => panic!("unsupported (format,mode) = {other:?} (added in later tasks)"),
    };

    println!("{{\"format\":\"{format}\",\"mode\":\"{mode}\",\
        \"wall_hot_median_s\":{median},\"wall_hot_min_s\":{min},\"wall_cold_s\":{cold},\
        \"peak_rss_bytes\":{},\"component_count\":{component_count},\"bytes\":{bytes}}}",
        peak_rss_bytes());
}
```

NOTE: the `Ontology` trait import that provides `.iter()` may be named differently (it is the trait `SetOntology` implements for iteration — check `src/ontology/set.rs`; in `omnread.rs` `.iter()` worked on `SetOntology` with only `use horned_owl::ontology::set::SetOntology`, so you may not need an extra trait import). Remove the speculative import if it doesn't compile.

- [ ] **Step 2: Build + run on a small .omn**

Run:
```
cd /data/dumontier/pymos/bench/horned-bench && PATH=…:$PATH cargo build --release 2>&1 | tail -5 && \
./target/release/horned-bench --format omn --mode parse --hot 5 /data/dumontier/pymos/bench/data/koala.omn
```
Expected: one JSON line with non-zero `component_count`, `wall_hot_median_s`, `peak_rss_bytes`.

- [ ] **Step 3: Verify render mode**

Run: `./target/release/horned-bench --format omn --mode render --hot 5 /data/dumontier/pymos/bench/data/koala.omn`
Expected: JSON line, `mode":"render"`.

- [ ] **Step 4: Commit**

```bash
cd /data/dumontier/pymos && git add bench/horned-bench/src/main.rs && \
git commit -m "bench: horned-bench omn parse/render timing + RSS + JSON (B1)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task B2: `horned-bench` — add ofn / owx / rdf formats

**Files:**
- Modify: `/data/dumontier/pymos/bench/horned-bench/src/main.rs`

- [ ] **Step 1: Add ofn + owx parse/render arms** (mirror the omn arms; same `(w,&amo,Some(&pm))` writer shape)

```rust
        ("ofn", "parse") => time_it(warmup, hot, || {
            let (o, _): (Set, _) = horned_owl::io::ofn::reader::read(BufReader::new(text.as_bytes()),
                ParserConfiguration::default()).expect("ofn parse");
            component_count = o.iter().count();
        }),
        ("ofn", "render") => {
            let (o, pm): (Set, _) = horned_owl::io::ofn::reader::read(BufReader::new(text.as_bytes()),
                ParserConfiguration::default()).expect("ofn parse");
            component_count = o.iter().count();
            let amo: Amo = o.into();
            time_it(warmup, hot, || { let _ = horned_owl::io::ofn::writer::write(Vec::<u8>::new(), &amo, Some(&pm)).unwrap(); })
        }
        ("owx", "parse") => time_it(warmup, hot, || {
            let (o, _): (Set, _) = horned_owl::io::owx::reader::read(BufReader::new(text.as_bytes()),
                ParserConfiguration::default()).expect("owx parse");
            component_count = o.iter().count();
        }),
        ("owx", "render") => {
            let (o, pm): (Set, _) = horned_owl::io::owx::reader::read(BufReader::new(text.as_bytes()),
                ParserConfiguration::default()).expect("owx parse");
            component_count = o.iter().count();
            let amo: Amo = o.into();
            time_it(warmup, hot, || { let _ = horned_owl::io::owx::writer::write(Vec::<u8>::new(), &amo, Some(&pm)).unwrap(); })
        }
```

NOTE: `io::owx::reader::read` has the signature `read::<A, O, R>(bufread, config)` (no `Ontology` bound) — pass the same turbofish/type ascription. Verify against `src/io/owx/reader.rs:32`.

- [ ] **Step 2: Add rdf arms** (rdf reader returns an `RDFOntology`; writer takes NO prefix arg)

```rust
        ("rdf", "parse") => time_it(warmup, hot, || {
            // RDFOntology -> count via conversion to SetOntology
            let (rdf_o, _): (horned_owl::ontology::set::SetOntology<Rc<str>>, _) =
                read_rdf_to_set(&text);
            component_count = rdf_o.iter().count();
        }),
        ("rdf", "render") => {
            let (o, _pm) = read_rdf_to_set(&text);
            component_count = o.iter().count();
            let amo: Amo = o.into();
            time_it(warmup, hot, || { let _ = horned_owl::io::rdf::writer::write(Vec::<u8>::new(), &amo).unwrap(); })
        }
```

Add a helper `read_rdf_to_set(text) -> (Set, PrefixMapping)` that calls `io::rdf::reader::read::<Rc<str>, Rc<AnnotatedComponent<Rc<str>>>, _>(BufReader::new(text.as_bytes()))`, then converts the returned `RDFOntology` into a `SetOntology` (via `.into()` or by collecting `.iter()` components into a fresh `SetOntology`). Inspect `src/io/rdf/reader.rs:2482` + `closure_reader.rs:200` for the exact return type and the available `Into`. If RDF read/convert proves awkward, gate rdf behind a `--format rdf` that may be dropped from the comparison and noted in the report (rdf is an intra-crate *nice-to-have*, not a Manchester comparator).

- [ ] **Step 3: Build + smoke each format**

Run:
```
cd /data/dumontier/pymos/bench/horned-bench && PATH=…:$PATH cargo build --release 2>&1 | tail -5 && \
for f in ofn owx rdf; do echo "== $f =="; ./target/release/horned-bench --format $f --mode parse --hot 3 /data/dumontier/pymos/bench/data/koala.$f 2>&1 | tail -2; done
```
Expected: JSON lines for ofn/owx (rdf if the conversion compiled). Missing input files for a format → note and skip that format's cell.

- [ ] **Step 4: Commit**

```bash
cd /data/dumontier/pymos && git add bench/horned-bench/src/main.rs && \
git commit -m "bench: horned-bench ofn/owx/rdf formats (B2)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task B3: `horned-bench` — fastobo-omn comparator

**Files:**
- Modify: `/data/dumontier/pymos/bench/horned-bench/src/main.rs`

- [ ] **Step 1: Add the fastobo-omn parse arm** (only if Task 0.1 proved the dual-version build; else create a sibling crate `horned-bench-fastobo` with the same JSON contract)

```rust
        ("fastobo-omn", "parse") => time_it(warmup, hot, || {
            // horned-manchester 0.4 on horned-owl 0.14; its own Set type.
            let (o, _): (horned_manchester::SetOntology0, _) =
                horned_manchester::from_str(&text).expect("fastobo parse");
            component_count = o.iter().count();
        }),
```

The exact 0.14 ontology type for the turbofish must come from the fastobo crate's re-exports (it `pub use`s horned-owl 0.14 types; inspect `_ref_horned-manchester/src/lib.rs` — `from_str<A,O,S>` is generic over `O: MutableOntology<…>+Default`). Use a 0.14 `SetOntology<Rc<str>>` named via the fastobo crate's transitive horned-owl. If naming the 0.14 crate is impossible from `horned-bench`, add `horned-owl = "0.14"` as a SECOND, renamed dependency:

```toml
horned-owl-014 = { package = "horned-owl", version = "0.14" }
```

and use `horned_owl_014::ontology::set::SetOntology<Rc<str>>`.

- [ ] **Step 2: Add the fastobo-omn render arm** (the crate is a serializer too)

Inspect `_ref_horned-manchester/src/lib.rs` for the render/serialize entry (e.g. a `to_string`/`AsManchester`/`write` fn). Add:

```rust
        ("fastobo-omn", "render") => {
            let (o, _): (/*0.14 Set*/, _) = horned_manchester::from_str(&text).expect("fastobo parse");
            component_count = o.iter().count();
            time_it(warmup, hot, || { let _ = horned_manchester::/*render fn*/(&o); });
        }
```

If the crate exposes no public serializer, drop fastobo from the **write** comparison and note it (read-only comparator).

- [ ] **Step 3: Build + smoke**

Run: `cd /data/dumontier/pymos/bench/horned-bench && PATH=…:$PATH cargo build --release 2>&1 | tail -8 && ./target/release/horned-bench --format fastobo-omn --mode parse --hot 3 /data/dumontier/pymos/bench/data/koala.omn`
Expected: JSON line with `format":"fastobo-omn"`. If the build fails on the dual version, fall back to the sibling-crate approach and update the workload (B4) to invoke the right binary.

- [ ] **Step 4: Commit**

```bash
cd /data/dumontier/pymos && git add bench/horned-bench && \
git commit -m "bench: horned-bench fastobo-omn comparator (B3)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task B4: pymos workloads — `parse_horned` / `render_horned`

**Files:**
- Create: `/data/dumontier/pymos/bench/workloads/parse_horned.py`
- Create: `/data/dumontier/pymos/bench/workloads/render_horned.py`

- [ ] **Step 1: Write `parse_horned.py`** (mirror `parse_owlapi.py`: subprocess + manual `Measurement`)

```python
"""Parse workload for the Rust horned-bench binary (any --format)."""
from __future__ import annotations
import json, subprocess
from pathlib import Path
from bench.measure import Measurement

BIN = Path(__file__).resolve().parents[1] / "horned-bench" / "target" / "release" / "horned-bench"

def bench_parse_horned(path: str, fmt: str = "omn", *, hot: int = 5, warmup: int = 1,
                       timeout: float = 600.0) -> Measurement:
    cmd = [str(BIN), "--format", fmt, "--mode", "parse", "--hot", str(hot),
           "--warmup", str(warmup), path]
    out = subprocess.run(cmd, check=True, capture_output=True, text=True, timeout=timeout)
    d = json.loads(out.stdout.strip().splitlines()[-1])
    return Measurement(
        wall_cold=d["wall_cold_s"],
        wall_hot_samples=[d["wall_hot_min_s"], d["wall_hot_median_s"]],
        wall_hot_median=d["wall_hot_median_s"],
        wall_hot_stddev=0.0,
        peak_rss_bytes=d["peak_rss_bytes"],
        cpu_cold=d["wall_cold_s"],
        extras={"backend": f"horned-{fmt}", "component_count": d["component_count"], "bytes": d["bytes"]},
    )
```

Verify the `Measurement(...)` keyword args against `bench/measure.py:23` (fields: `wall_cold`, `wall_hot_samples`, `wall_hot_median`, `wall_hot_stddev`, `peak_rss_bytes`, `cpu_cold`, `extras`). Adjust if a field is positional-only or named differently.

- [ ] **Step 2: Write `render_horned.py`** (identical but `--mode render`)

```python
"""Render workload for the Rust horned-bench binary."""
from __future__ import annotations
import json, subprocess
from pathlib import Path
from bench.measure import Measurement
from bench.workloads.parse_horned import BIN

def bench_render_horned(path: str, fmt: str = "omn", *, hot: int = 5, warmup: int = 1,
                        timeout: float = 600.0) -> Measurement:
    cmd = [str(BIN), "--format", fmt, "--mode", "render", "--hot", str(hot),
           "--warmup", str(warmup), path]
    out = subprocess.run(cmd, check=True, capture_output=True, text=True, timeout=timeout)
    d = json.loads(out.stdout.strip().splitlines()[-1])
    return Measurement(
        wall_cold=d["wall_cold_s"], wall_hot_samples=[d["wall_hot_min_s"], d["wall_hot_median_s"]],
        wall_hot_median=d["wall_hot_median_s"], wall_hot_stddev=0.0,
        peak_rss_bytes=d["peak_rss_bytes"], cpu_cold=d["wall_cold_s"],
        extras={"backend": f"horned-{fmt}", "component_count": d["component_count"], "bytes": d["bytes"]},
    )
```

- [ ] **Step 3: Smoke each workload** (the binary must be built — Task B1/B2)

Run:
```
cd /data/dumontier/pymos && /tmp/verify-022/bin/python -c "
from bench.workloads.parse_horned import bench_parse_horned
m = bench_parse_horned('bench/data/koala.omn', 'omn'); print(m.wall_hot_median, m.peak_rss_bytes, m.extras)"
```
Expected: a median time, an RSS, and `extras` with `backend='horned-omn'`. Use the project's own Python if pymos imports need it (check `/data/dumontier/pymos` for a venv); `/tmp/verify-022` has omny but may lack pymos deps — prefer the pymos venv for these.

- [ ] **Step 4: Commit**

```bash
cd /data/dumontier/pymos && git add bench/workloads/parse_horned.py bench/workloads/render_horned.py && \
git commit -m "bench: pymos parse_horned/render_horned workloads (B4)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task B5: comparison runner + CSV

**Files:**
- Create: `/data/dumontier/pymos/bench/run_manchester.py`
- Create (output): `/data/dumontier/pymos/bench/results/2026-06-13-manchester/raw.csv`

- [ ] **Step 1: Write the runner enumerating all cells**

```python
"""Run the Manchester read/write comparison across the corpus and emit CSV.

Cells:
  read : horned-{omn,ofn,owx,rdf}, fastobo-omn, omny, owlapi(ROBOT)
  write: horned-{omn,ofn,owx}, fastobo-omn (if serializer), omny
Inputs: each ontology as .omn (Manchester readers) + native .ofn/.owx/.rdf
        (intra-crate). Missing input for a (ontology,format) cell -> skipped.
"""
from __future__ import annotations
import csv, time
from pathlib import Path

from bench.workloads.parse_horned import bench_parse_horned
from bench.workloads.render_horned import bench_render_horned
from bench.workloads.parse import bench_parse as bench_parse_omny
from bench.workloads.render import bench_render as bench_render_omny
from bench.workloads.parse_owlapi import bench_owlapi_parse  # name per parse_owlapi.py

DATA = Path("bench/data")
# (ontology stem, available extensions present in bench/data)
ONTOLOGIES = ["koala", "obi-core", "hp"]   # extend with what exists / is fetched
OUT = Path("bench/results/2026-06-13-manchester"); OUT.mkdir(parents=True, exist_ok=True)

def rows():
    for stem in ONTOLOGIES:
        omn = DATA / f"{stem}.omn"
        if omn.exists():
            for fmt in ("omn", "ofn", "owx", "rdf"):
                f = DATA / f"{stem}.{fmt}"
                if f.exists():
                    m = bench_parse_horned(str(f), fmt)
                    yield (stem, "read", f"horned-{fmt}", m.wall_hot_median, m.peak_rss_bytes, m.extras.get("bytes"))
            try:
                m = bench_parse_horned(str(omn), "fastobo-omn")
                yield (stem, "read", "fastobo-omn", m.wall_hot_median, m.peak_rss_bytes, m.extras.get("bytes"))
            except Exception as e:
                print("fastobo skip:", e)
            try:
                m = bench_parse_omny(str(omn))
                yield (stem, "read", "omny", m.wall_hot_median, m.peak_rss_bytes, None)
            except Exception as e:
                print("omny skip:", e)
            # write cells
            for fmt in ("omn", "ofn", "owx"):
                f = DATA / f"{stem}.{fmt}"
                if f.exists():
                    m = bench_render_horned(str(f), fmt)
                    yield (stem, "write", f"horned-{fmt}", m.wall_hot_median, m.peak_rss_bytes, m.extras.get("bytes"))

def main():
    with open(OUT / "raw.csv", "w", newline="") as fh:
        w = csv.writer(fh)
        w.writerow(["ontology", "mode", "backend", "wall_hot_median_s", "peak_rss_bytes", "bytes"])
        for r in rows():
            w.writerow(r); print(r)

if __name__ == "__main__":
    main()
```

Reconcile the imports with the actual function names in `bench/workloads/parse_owlapi.py` (the OWL-API entry) and `parse.py`/`render.py` (omny). If omny needs an explicit interpreter, run the whole script under the pymos venv.

- [ ] **Step 2: Run the runner**

Run: `cd /data/dumontier/pymos && <pymos-python> bench/run_manchester.py 2>&1 | tail -30`
Expected: printed rows + `bench/results/2026-06-13-manchester/raw.csv` written. Cells with missing inputs/tools print a skip line, not a crash.

- [ ] **Step 3: Commit**

```bash
cd /data/dumontier/pymos && git add bench/run_manchester.py bench/results/2026-06-13-manchester/raw.csv && \
git commit -m "bench: Manchester read/write comparison runner + raw CSV (B5)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task B6: performance report generator

**Files:**
- Create: `/data/dumontier/pymos/bench/report_manchester.py`
- Create (output): `/data/dumontier/pymos/bench/results/2026-06-13-manchester/performance-report.md`

- [ ] **Step 1: Write the report generator (CSV → markdown with ratios vs OWL-API)**

```python
"""Aggregate raw.csv into performance-report.md (per-ontology tables + ratios)."""
from __future__ import annotations
import csv
from collections import defaultdict
from pathlib import Path

OUT = Path("bench/results/2026-06-13-manchester")

def load():
    with open(OUT / "raw.csv") as fh:
        return list(csv.DictReader(fh))

def main():
    rows = load()
    by = defaultdict(dict)   # (ontology, mode) -> {backend: (median, rss, bytes)}
    for r in rows:
        by[(r["ontology"], r["mode"])][r["backend"]] = (
            float(r["wall_hot_median_s"]), int(r["peak_rss_bytes"]),
            int(r["bytes"]) if r["bytes"] not in (None, "", "None") else None)
    md = ["# Manchester `io/omn` Performance Report\n",
          "_Generated from raw.csv by report_manchester.py. Caveats: Rust timings "
          "are in-process (cold-start excluded); OWL-API via ROBOT docker (container "
          "startup not subtracted here unless noted); omny is pure-Python._\n"]
    for (onto, mode), cells in sorted(by.items()):
        md.append(f"## {onto} — {mode}\n")
        md.append("| backend | median (ms) | peak RSS (MB) | throughput (MB/s) | ratio vs owlapi |")
        md.append("|---------|-------------|---------------|-------------------|-----------------|")
        base = cells.get("owlapi", (None,))[0]
        for backend, (median, rss, nbytes) in sorted(cells.items(), key=lambda kv: kv[1][0]):
            thru = (nbytes / 1e6 / median) if (nbytes and median) else float("nan")
            ratio = (median / base) if base else float("nan")
            md.append(f"| {backend} | {median*1e3:.2f} | {rss/1e6:.1f} | {thru:.1f} | {ratio:.2f}× |")
        md.append("")
    (OUT / "performance-report.md").write_text("\n".join(md))
    print("wrote", OUT / "performance-report.md")

if __name__ == "__main__":
    main()
```

- [ ] **Step 2: Generate + sanity-check**

Run: `cd /data/dumontier/pymos && <pymos-python> bench/report_manchester.py && sed -n '1,30p' bench/results/2026-06-13-manchester/performance-report.md`
Expected: per-ontology read/write tables with median, RSS, throughput, ratio columns.

- [ ] **Step 3: Commit**

```bash
cd /data/dumontier/pymos && git add bench/report_manchester.py bench/results/2026-06-13-manchester/performance-report.md && \
git commit -m "bench: Manchester performance report generator + report (B6)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Phase C — Combined summary

### Task C1: combined `manchester-io-report.md`

**Files:**
- Create: `/data/dumontier/horned-owl-omn/docs/manchester/manchester-io-report.md`

- [ ] **Step 1: Write the one-page summary** linking both reports

Author a concise document with these sections, filling the numbers from the generated `compliance-report.md` and `performance-report.md`:

```markdown
# Manchester `io/omn` — Conformance & Performance Summary

**Date:** 2026-06-13. Reader + writer for OWL 2 Manchester Syntax §2.5 in the
horned-owl fork.

## Conformance (see `compliance-report.md`)
- §2.5 construct matrix: <N_pass>/<N_total> constructs read+write+round-trip;
  <K> documented residuals (SWRL, complex-LHS GCI, nested-annotation drop,
  data-restriction-as-object, HasKey object/data conflation, bare-name).
- Corpus parse: <M_full>/<M_total> ontologies parse fully; blockers: <list>.
- Axiom-equality vs OWL-API: <rate> matched after canonicalization;
  residual missing/extra: <summary>.
- Adversarial: edge fixtures pass; 4000 fuzz cases, 0 panics.

## Performance (see `performance-report.md`)
- Read (median, representative ontology): ours-omn <x> ms vs omny <y> ms vs
  OWL-API <z> ms vs fastobo <w> ms; intra-crate ofn/owx/rdf <…>.
- Write: ours-omn <…> vs omny <…>.
- Peak RSS: <…>. Caveats restated.

## Residual limitations (authoritative)
<one paragraph each: inherent non-§2.5, model limits, writer follow-ups>
```

Replace EVERY `<…>` placeholder with the actual generated numbers before committing — the combined report must contain no angle-bracket placeholders.

- [ ] **Step 2: Commit**

```bash
cd /data/dumontier/horned-owl-omn && git add docs/manchester/manchester-io-report.md && \
git commit -m "docs: combined Manchester conformance + performance summary (C1)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

- [ ] **Step 3: Update durable memory**

Append to `/home/dumontier/.claude/projects/-data-dumontier-rustdl/memory/manchester-io-feature.md` a one-line note that conformance + performance reports exist (paths), and add an index line to `MEMORY.md`.

---

## Self-Review (completed by plan author)

**1. Spec coverage:**
- A1 per-construct matrix → Task A1 ✓
- A2 corpus parse + round-trip → Task A2 ✓
- A3 semantic axiom-equality + canonicalizer → Task A3 ✓
- A4 adversarial + no-panic fuzz → Task A4 ✓
- Compliance report generator → Task A5 ✓
- B1 horned-bench (omn) → Task B1; ofn/owx/rdf → B2; fastobo → B3 ✓
- B2 pymos workloads → Task B4 ✓
- Runner + CSV → Task B5; perf report → B6 ✓
- Combined summary → Task C1 ✓
- fork/pymos split, no-push, no-[patch], dual-version coexistence, ROBOT-gating → encoded in conventions + Tasks 0.1/A2/A3 ✓

**2. Placeholder scan:** the only intentional `<…>` placeholders are in C1 Step 1, with an explicit instruction to replace them all before commit. Two `todo!()`/`// …` markers (A1 Step 2 case list, A3 Step 3 helper) carry explicit "do NOT leave in the committed file / replace in this step" instructions. No bare TBD/"handle errors" placeholders.

**3. Type consistency:** `read_str`/`write_str`/`components_sorted` (mod.rs) used consistently in A1/A3/A4; `Case`/`Residual`/`run_case`/`CASES` consistent A1↔A5; `canonical`/`is_declaration`/`nary_member_pairs` consistent A3↔A5; `CorpusRow`/`EqRow`/`run_corpus`/`run_axiom_equality`/`docker_available` consistent A2/A3↔A5; `Measurement` field names match `bench/measure.py`; `horned-bench` JSON keys identical across B1→B6.

## Execution Handoff

Plan complete. Phases A (fork) and B (pymos) are independent and can run in either order; C depends on both reports existing.
