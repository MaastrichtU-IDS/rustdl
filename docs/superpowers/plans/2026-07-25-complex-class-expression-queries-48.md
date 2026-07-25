# Complex Class-Expression Queries (#48) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Accept an anonymous Manchester class expression as a query target for `isSatisfiable(CE)`, `isEntailed(SubClassOf(CE₁,CE₂))`, and `getInstances(CE)`, on the reasoner API + CLI `--json` + Python.

**Architecture:** Sound-by-construction reduction — parse the Manchester CE, inject a fresh `EquivalentClasses(Q, CE)` definitional axiom into a clone of the ontology, and answer via the existing named-class queries on `Q` (`is_class_satisfiable_with_stats` / `is_subclass_of_with_stats` / `instances_of`). This is the exact pattern `justify::entails` already uses for its `DisjointClasses` arm, generalized to an arbitrary parsed CE body.

**Tech Stack:** Rust (edition 2024), horned-owl (`parse_class_expression`, `Build`, `SetOntology::insert`), PyO3, clap, serde_json, ROBOT/HermiT oracle.

## Global Constraints

- **Build/test with `RUSTUP_TOOLCHAIN=stable cargo …`** (pinned 1.95.0 often lacks cargo; a bare cargo silently reuses a stale binary).
- **Clippy pedantic `-D warnings`** workspace-wide; `unwrap_used`/`dbg_macro` only under `#[cfg(test)]` (test files gate with `#![allow(clippy::unwrap_used)]`). `rustfmt max_width = 100` (`cargo fmt --all`). Raw string with no inner `#`/`"` → `r"..."` (clippy `needless_raw_string_hashes`).
- **Soundness:** the reduction is sound by construction (`Q ≡ CE` is a conservative definitional extension over a fresh name — it adds no entailment about the original signature). Answer soundness = that of the underlying named query. Never claim complete when it might miss: `incomplete = !QueryStats.pure_el_mode` (true whenever the tableau/wedge — possibly trust-`Sat` — was consulted rather than the complete EL closure).
- **Fresh-probe guarantee:** the probe IRI(s) MUST NOT already occur as a class in the ontology; a collision is a `ReasonError`, not a silent overwrite.
- **CLI `--json`:** exactly one JSON object on stdout, diagnostics to stderr; `schema_version` = the `SCHEMA_VERSION` const in `json_out.rs` (stays `1`); arrays sorted.
- **Python:** bare-value returns; emit `IncompleteQueryWarning` when incomplete (mirror the `_warn_if_query_incomplete` wrapper added for #44–#47). Keep `__all__`/`.pyi`/module in sync (`test_stubs`).
- **Manchester input only** for the CE argument; the ontology file stays any supported format.
- Single PR closing #48.

---

## File Structure
- **Reasoner:** `crates/owl-dl-reasoner/src/class_expr_query.rs` (new) — the three query fns + `CeVerdict`/`CeInstances` + shared probe helper; wired into `lib.rs`.
- **CLI:** `crates/owl-dl-cli/src/main.rs` (3 `Command` variants + dispatch, Manchester parse), `json_out.rs` (3 result structs + builders), `tests/json_output.rs` + `tests/fixtures/json/ce_tiny.ofn`.
- **Python:** `crates/owl-dl-py/src/queries.rs` (3 `#[pyfunction]`s), `src/load.rs` (a `load_path_with_pm` helper), `python/rustdl/__init__.py` (+ `__all__` + wrappers), `python/rustdl/__init__.pyi`, `tests/python/test_queries.py`.
- **Oracle:** `crates/owl-dl-reasoner/tests/class_expr_oracle.rs` + `tests/fixtures/class_expr/` (input `.ofn` + committed HermiT `.owx`), reusing `docker/robot/*-oracle.sh` conventions.

---

## Task 1: Reasoner `class_expr_query.rs` — the three CE queries

**Files:**
- Create: `crates/owl-dl-reasoner/src/class_expr_query.rs`
- Modify: `crates/owl-dl-reasoner/src/lib.rs` (add `mod class_expr_query; pub use …`)
- Test: `crates/owl-dl-reasoner/tests/class_expr.rs` (new)

**Interfaces:**
- Produces:
  ```rust
  pub struct CeVerdict { holds: bool, incomplete: bool }
  impl CeVerdict { pub fn holds(&self) -> bool; pub fn incomplete(&self) -> bool; }
  pub struct CeInstances { individuals: Vec<String>, incomplete: bool }
  impl CeInstances { pub fn individuals(&self) -> &[String]; pub fn incomplete(&self) -> bool; }
  pub fn class_expression_satisfiable<A: horned_owl::model::ForIRI>(onto: &SetOntology<A>, ce: &ClassExpression<A>) -> Result<CeVerdict, ReasonError>;
  pub fn class_expression_entailed_subclass<A: horned_owl::model::ForIRI>(onto: &SetOntology<A>, sub_ce: &ClassExpression<A>, sup_ce: &ClassExpression<A>) -> Result<CeVerdict, ReasonError>;
  pub fn class_expression_instances<A: horned_owl::model::ForIRI>(onto: &SetOntology<A>, ce: &ClassExpression<A>) -> Result<CeInstances, ReasonError>;
  ```
- Consumes: `crate::{is_class_satisfiable_with_stats, is_subclass_of_with_stats, instances_of, ReasonError, QueryStats}`; the `justify::entails` `DisjointClasses` arm (`justify.rs`, the `probed.insert(Component::EquivalentClasses(...))` block) is the template.

- [ ] **Step 1: Write the failing tests** (`crates/owl-dl-reasoner/tests/class_expr.rs`)

```rust
#![allow(clippy::unwrap_used)]
use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::{Build, ClassExpression, RcStr};
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;
use owl_dl_reasoner::{class_expression_satisfiable, class_expression_entailed_subclass, class_expression_instances};

fn onto(src: &str) -> SetOntology<RcStr> {
    read_ofn(&mut Cursor::new(src.to_owned()), ParserConfiguration::default()).unwrap().0
}
fn cls(b: &Build<RcStr>, iri: &str) -> ClassExpression<RcStr> { ClassExpression::Class(b.class(iri)) }

const TBOX: &str = r"Prefix(:=<http://ex/#>)
  Ontology(<http://ex/>
    Declaration(Class(:A)) Declaration(Class(:B))
    Declaration(NamedIndividual(:x)) Declaration(NamedIndividual(:y))
    ClassAssertion(:A :x) ClassAssertion(:B :y))";

#[test]
fn ce_satisfiable_and_unsatisfiable() {
    let o = onto(TBOX);
    let b = Build::<RcStr>::new();
    // A ⊔ B satisfiable
    let union = ClassExpression::ObjectUnionOf(vec![cls(&b,"http://ex/#A"), cls(&b,"http://ex/#B")]);
    assert!(class_expression_satisfiable(&o, &union).unwrap().holds());
    // A ⊓ ¬A unsatisfiable
    let contradiction = ClassExpression::ObjectIntersectionOf(vec![
        cls(&b,"http://ex/#A"),
        ClassExpression::ObjectComplementOf(Box::new(cls(&b,"http://ex/#A"))),
    ]);
    assert!(!class_expression_satisfiable(&o, &contradiction).unwrap().holds());
}

#[test]
fn ce_entailed_subclass_positive_and_negative() {
    let o = onto(TBOX);
    let b = Build::<RcStr>::new();
    let a_and_b = ClassExpression::ObjectIntersectionOf(vec![cls(&b,"http://ex/#A"), cls(&b,"http://ex/#B")]);
    // A ⊓ B ⊑ A  (entailed)
    assert!(class_expression_entailed_subclass(&o, &a_and_b, &cls(&b,"http://ex/#A")).unwrap().holds());
    // A ⊑ B  (NOT entailed)
    assert!(!class_expression_entailed_subclass(&o, &cls(&b,"http://ex/#A"), &cls(&b,"http://ex/#B")).unwrap().holds());
}

#[test]
fn ce_instances_of_union() {
    let o = onto(TBOX);
    let b = Build::<RcStr>::new();
    let union = ClassExpression::ObjectUnionOf(vec![cls(&b,"http://ex/#A"), cls(&b,"http://ex/#B")]);
    let inds = class_expression_instances(&o, &union).unwrap();
    let set: std::collections::HashSet<&str> = inds.individuals().iter().map(String::as_str).collect();
    assert!(set.contains("http://ex/#x")); // x:A ⇒ x:(A⊔B)
    assert!(set.contains("http://ex/#y")); // y:B ⇒ y:(A⊔B)
    // the synthetic probe IRI must NOT leak into instances:
    assert!(!inds.individuals().iter().any(|i| i.starts_with("urn:rustdl-ce-probe")));
}

#[test]
fn ce_probe_iri_collision_errors() {
    // An ontology that already declares the probe IRI as a class ⇒ error, not silent overwrite.
    let o = onto(r#"Prefix(:=<http://ex/#>)
      Ontology(<http://ex/> Declaration(Class(<urn:rustdl-ce-probe:q>)) Declaration(Class(:A)))"#);
    let b = Build::<RcStr>::new();
    assert!(class_expression_satisfiable(&o, &cls(&b,"http://ex/#A")).is_err());
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test class_expr`
Expected: FAIL — the three fns are unresolved.

- [ ] **Step 3: Implement `class_expr_query.rs`**

```rust
//! Complex (anonymous) class-expression queries (issue #48). A parsed
//! `ClassExpression` is answered by minting a fresh probe class `Q`, adding
//! `EquivalentClasses(Q, CE)`, and delegating to the named-class queries — the
//! same reduction `justify::entails` uses for its DisjointClasses arm. Sound by
//! construction (a definitional extension over a fresh name adds no entailment
//! about the original signature). Read-only on the input.
use crate::{
    QueryStats, ReasonError, instances_of, is_class_satisfiable_with_stats,
    is_subclass_of_with_stats,
};
use horned_owl::model::{
    Build, ClassExpression, Component, EquivalentClasses, ForIRI,
};
use horned_owl::ontology::set::SetOntology;

const PROBE_IRI: &str = "urn:rustdl-ce-probe:q";
const PROBE_IRI_2: &str = "urn:rustdl-ce-probe:q2";

#[derive(Debug, Clone, Copy)]
pub struct CeVerdict { holds: bool, incomplete: bool }
impl CeVerdict {
    #[must_use] pub fn holds(&self) -> bool { self.holds }
    #[must_use] pub fn incomplete(&self) -> bool { self.incomplete }
}
#[derive(Debug, Clone)]
pub struct CeInstances { individuals: Vec<String>, incomplete: bool }
impl CeInstances {
    #[must_use] pub fn individuals(&self) -> &[String] { &self.individuals }
    #[must_use] pub fn incomplete(&self) -> bool { self.incomplete }
}

fn incomplete_of(stats: QueryStats) -> bool { !stats.pure_el_mode }

/// Error if `iri` already occurs as a class in the ontology signature.
fn ensure_fresh<A: ForIRI>(onto: &SetOntology<A>, iri: &str) -> Result<(), ReasonError> {
    for ac in onto {
        if let Component::DeclareClass(dc) = &ac.component {
            if dc.0.0.as_ref() == iri {
                return Err(ReasonError::UnknownClass(format!(
                    "probe IRI {iri} collides with a declared class"
                )));
            }
        }
    }
    Ok(())
}

fn probe_axiom<A: ForIRI>(build: &Build<A>, iri: &str, ce: &ClassExpression<A>) -> Component<A> {
    Component::EquivalentClasses(EquivalentClasses(vec![
        ClassExpression::Class(build.class(iri)),
        ce.clone(),
    ]))
}

/// # Errors
/// [`ReasonError::UnknownClass`] on probe collision; propagates reasoner errors.
pub fn class_expression_satisfiable<A: ForIRI>(
    onto: &SetOntology<A>,
    ce: &ClassExpression<A>,
) -> Result<CeVerdict, ReasonError> {
    ensure_fresh(onto, PROBE_IRI)?;
    let mut probed = onto.clone();
    let build: Build<A> = Build::new();
    probed.insert(probe_axiom(&build, PROBE_IRI, ce));
    let (holds, stats) = is_class_satisfiable_with_stats(&probed, PROBE_IRI)?;
    Ok(CeVerdict { holds, incomplete: incomplete_of(stats) })
}

/// # Errors
/// As above.
pub fn class_expression_entailed_subclass<A: ForIRI>(
    onto: &SetOntology<A>,
    sub_ce: &ClassExpression<A>,
    sup_ce: &ClassExpression<A>,
) -> Result<CeVerdict, ReasonError> {
    ensure_fresh(onto, PROBE_IRI)?;
    ensure_fresh(onto, PROBE_IRI_2)?;
    let mut probed = onto.clone();
    let build: Build<A> = Build::new();
    probed.insert(probe_axiom(&build, PROBE_IRI, sub_ce));
    probed.insert(probe_axiom(&build, PROBE_IRI_2, sup_ce));
    let (holds, stats) = is_subclass_of_with_stats(&probed, PROBE_IRI, PROBE_IRI_2)?;
    Ok(CeVerdict { holds, incomplete: incomplete_of(stats) })
}

/// # Errors
/// As above.
pub fn class_expression_instances<A: ForIRI>(
    onto: &SetOntology<A>,
    ce: &ClassExpression<A>,
) -> Result<CeInstances, ReasonError> {
    ensure_fresh(onto, PROBE_IRI)?;
    let mut probed = onto.clone();
    let build: Build<A> = Build::new();
    probed.insert(probe_axiom(&build, PROBE_IRI, ce));
    // completeness signal via a companion sat query on the probe (cheap vs realize):
    let (_sat, stats) = is_class_satisfiable_with_stats(&probed, PROBE_IRI)?;
    let mut individuals = instances_of(&probed, PROBE_IRI)?;
    individuals.retain(|i| !i.starts_with("urn:rustdl-ce-probe"));
    individuals.sort();
    individuals.dedup();
    Ok(CeInstances { individuals, incomplete: incomplete_of(stats) })
}
```

Wire in `lib.rs`: `mod class_expr_query;` + `pub use class_expr_query::{CeInstances, CeVerdict, class_expression_satisfiable, class_expression_entailed_subclass, class_expression_instances};`.

**Verify against the real horned-owl API** (adjust field access if needed): `DeclareClass`'s inner path to the IRI (`dc.0.0.as_ref()` — a `Class` wraps an `IRI`; match how `justify.rs`/`convert.rs` read a class IRI), `SetOntology::insert(Component)` (confirm it takes a `Component` vs an `AnnotatedComponent` — `justify::entails` calls `probed.insert(Component::EquivalentClasses(...))`, so mirror that exactly), and that `instances_of` returns `Result<Vec<String>, ReasonError>`.

- [ ] **Step 4: Run to verify it passes**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test class_expr`
Expected: PASS (4 tests). Then `--lib` + `cargo clippy -p owl-dl-reasoner --all-targets -- -D warnings` + `cargo fmt --all`.

- [ ] **Step 5: Commit**

```bash
git add crates/owl-dl-reasoner/src/class_expr_query.rs crates/owl-dl-reasoner/src/lib.rs crates/owl-dl-reasoner/tests/class_expr.rs
git commit -m "feat(reasoner): complex class-expression queries via probe reduction (#48)"
```

## Task 2: CLI `sat-expr` / `subclass-expr` / `instances-expr` (Manchester + `--json`)

**Files:**
- Modify: `crates/owl-dl-cli/src/main.rs` (3 `Command` variants + dispatch), `crates/owl-dl-cli/src/json_out.rs`
- Test: `crates/owl-dl-cli/tests/json_output.rs` + fixture `crates/owl-dl-cli/tests/fixtures/json/ce_tiny.ofn`

**Interfaces:**
- `rustdl sat-expr <file> <ce> [--json]` → `{ "schema_version":1, "incomplete":bool, "satisfiable":bool }`
- `rustdl subclass-expr <file> <sub-ce> <sup-ce> [--json]` → `{ …, "entailed":bool }`
- `rustdl instances-expr <file> <ce> [--json]` → `{ …, "instances":[<iri>,…] }`
- Consumes: Task 1 reasoner fns; `parse_ofn_with_pm` (main.rs); `horned_owl::io::omn::reader::parse_class_expression`.

- [ ] **Step 1: Write the failing test + fixture**

Fixture `crates/owl-dl-cli/tests/fixtures/json/ce_tiny.ofn`:
```
Prefix(:=<http://ex/#>)
Ontology(<http://ex/>
  Declaration(Class(:A)) Declaration(Class(:B))
  Declaration(NamedIndividual(:x)) ClassAssertion(:A :x))
```

Tests in `tests/json_output.rs` (mirror the existing `disjoint_json_*` harness with `rustdl()`/`CARGO_BIN_EXE_rustdl`; the CE arg is Manchester — `:A` resolves via the file's prefix map):
```rust
#[test]
fn sat_expr_json_reports_satisfiable() {
    let out = rustdl().args(["sat-expr","--json", ce_tiny(), ":A and not :A"]).output().unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["satisfiable"], false); // A ⊓ ¬A unsat
}
#[test]
fn subclass_expr_json_reports_entailed() {
    let out = rustdl().args(["subclass-expr","--json", ce_tiny(), ":A and :B", ":A"]).output().unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["entailed"], true); // A⊓B ⊑ A
}
#[test]
fn instances_expr_json_lists_instances() {
    let out = rustdl().args(["instances-expr","--json", ce_tiny(), ":A"]).output().unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let insts: Vec<&str> = v["instances"].as_array().unwrap().iter().map(|s| s.as_str().unwrap()).collect();
    assert!(insts.contains(&"http://ex/#x"));
}
```
Add a `ce_tiny()` fixture-path helper mirroring `disjoint_tiny()`.

- [ ] **Step 2: Run → FAIL** (`sat-expr` unknown subcommand).
Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-cli --test json_output`

- [ ] **Step 3: `json_out.rs` structs + builders**

```rust
#[derive(Serialize)]
pub(crate) struct SatExprJson { pub(crate) schema_version: u32, pub(crate) incomplete: bool, pub(crate) satisfiable: bool }
#[derive(Serialize)]
pub(crate) struct SubclassExprJson { pub(crate) schema_version: u32, pub(crate) incomplete: bool, pub(crate) entailed: bool }
#[derive(Serialize)]
pub(crate) struct InstancesExprJson { pub(crate) schema_version: u32, pub(crate) incomplete: bool, pub(crate) instances: Vec<String> }

#[must_use] pub(crate) fn build_sat_expr_json(v: &owl_dl_reasoner::CeVerdict) -> SatExprJson {
    SatExprJson { schema_version: SCHEMA_VERSION, incomplete: v.incomplete(), satisfiable: v.holds() }
}
#[must_use] pub(crate) fn build_subclass_expr_json(v: &owl_dl_reasoner::CeVerdict) -> SubclassExprJson {
    SubclassExprJson { schema_version: SCHEMA_VERSION, incomplete: v.incomplete(), entailed: v.holds() }
}
#[must_use] pub(crate) fn build_instances_expr_json(r: &owl_dl_reasoner::CeInstances) -> InstancesExprJson {
    let mut instances = r.individuals().to_vec(); instances.sort();
    InstancesExprJson { schema_version: SCHEMA_VERSION, incomplete: r.incomplete(), instances }
}
```

- [ ] **Step 4: `Command` variants + dispatch + Manchester parse helper**

Add a parse helper near `parse_ofn_with_pm` in `main.rs`:
```rust
fn parse_ce(pm: &PrefixMapping, s: &str) -> Result<horned_owl::model::ClassExpression<RcStr>> {
    let build: horned_owl::model::Build<RcStr> = horned_owl::model::Build::new();
    horned_owl::io::omn::reader::parse_class_expression(s, pm, &build)
        .map_err(|e| anyhow::anyhow!("parsing class expression '{s}': {e}"))
}
```
(Confirm the `parse_class_expression` path is exported and the `pm` type matches `parse_ofn_with_pm`'s `PrefixMapping` — both are `horned_owl::curie::PrefixMapping`.)

`Command` variants (mirror `Consistent`'s shape; `--json` a plain bool):
```rust
    /// Satisfiability of a Manchester class expression.
    SatExpr { file: PathBuf, ce: String, #[arg(long)] json: bool },
    /// Whether SubClassOf(sub-ce, sup-ce) is entailed (Manchester).
    SubclassExpr { file: PathBuf, sub_ce: String, sup_ce: String, #[arg(long)] json: bool },
    /// Named individuals entailed to be instances of a Manchester class expression.
    InstancesExpr { file: PathBuf, ce: String, #[arg(long)] json: bool },
```

Dispatch arms (mirror `Command::Disjoint`; `--json` → `to_string_pretty` + `return Ok(())`, else human line; parse errors propagate via `?` to stderr+nonzero exit):
```rust
        Command::SatExpr { file, ce, json } => {
            let (onto, pm) = parse_ofn_with_pm(&file)?;
            let ce = parse_ce(&pm, &ce)?;
            let v = owl_dl_reasoner::class_expression_satisfiable(&onto, &ce).context("sat-expr")?;
            if json { println!("{}", serde_json::to_string_pretty(&json_out::build_sat_expr_json(&v))?); return Ok(()); }
            println!("{}", if v.holds() { "satisfiable" } else { "unsatisfiable" });
            if v.incomplete() { eprintln!("warning: verdict is a sound under-approximation (incomplete)"); }
        }
        // SubclassExpr / InstancesExpr analogous (subclass parses two CEs; instances prints one IRI per line).
```

- [ ] **Step 5: Run → PASS + clippy + fmt**
Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-cli --test json_output && RUSTUP_TOOLCHAIN=stable cargo clippy -p owl-dl-cli --all-targets -- -D warnings && cargo fmt --all`

- [ ] **Step 6: Commit**
```bash
git add crates/owl-dl-cli/src/main.rs crates/owl-dl-cli/src/json_out.rs crates/owl-dl-cli/tests/json_output.rs crates/owl-dl-cli/tests/fixtures/json/ce_tiny.ofn
git commit -m "feat(cli): sat-expr / subclass-expr / instances-expr subcommands (#48)"
```

## Task 3: Python bindings

**Files:**
- Modify: `crates/owl-dl-py/src/queries.rs`, `crates/owl-dl-py/src/load.rs` (add `load_path_with_pm`), `python/rustdl/__init__.py`, `python/rustdl/__init__.pyi`
- Test: `crates/owl-dl-py/tests/python/test_queries.py`

**Interfaces:**
- `class_expression_satisfiable(path, ce) -> bool`, `class_expression_entailed_subclass(path, sub_ce, sup_ce) -> bool`, `class_expression_instances(path, ce) -> list[str]`. Each emits `IncompleteQueryWarning` when incomplete (via the existing `_warn_if_query_incomplete` wrapper).

- [ ] **Step 1: Write failing tests** (`test_queries.py`)
```python
def test_ce_satisfiable(tmp_path):
    p = tmp_path/"o.ofn"; p.write_text(
        "Prefix(:=<http://ex/#>)\nOntology(<http://ex/>\n"
        "  Declaration(Class(:A)))\n")
    assert rustdl.class_expression_satisfiable(str(p), ":A and not :A") is False
    assert rustdl.class_expression_satisfiable(str(p), ":A") is True

def test_ce_instances(tmp_path):
    p = tmp_path/"o.ofn"; p.write_text(
        "Prefix(:=<http://ex/#>)\nOntology(<http://ex/>\n"
        "  Declaration(Class(:A)) Declaration(NamedIndividual(:x)) ClassAssertion(:A :x))\n")
    assert "http://ex/#x" in rustdl.class_expression_instances(str(p), ":A")
```

- [ ] **Step 2: Run → FAIL** (`AttributeError`).
Run: `cd crates/owl-dl-py && export VIRTUAL_ENV="$PWD/.venv" PATH="$PWD/.venv/bin:$PATH" RUSTUP_TOOLCHAIN=stable && maturin develop && python -m pytest tests/python/test_queries.py -q`

- [ ] **Step 3: `load_path_with_pm` + native fns**

In `src/load.rs`, add (mirror `load_path` but return the prefix map — read via the OFN/OWX/OMN reader that yields a `PrefixMapping`; for RDF/XML return `PrefixMapping::default()`):
```rust
pub(crate) fn load_path_with_pm(path: &str) -> PyResult<(SetOntology<RcStr>, horned_owl::curie::PrefixMapping)> { /* … */ }
```

In `src/queries.rs` (native returns `(value, incomplete)` so the Python wrapper can warn — mirror the #44–#47 pattern):
```rust
#[pyfunction]
pub(crate) fn _class_expression_satisfiable(path: &str, ce: &str) -> PyResult<(bool, bool)> {
    let (o, pm) = crate::load::load_path_with_pm(path)?;
    let build: horned_owl::model::Build<horned_owl::model::RcStr> = horned_owl::model::Build::new();
    let ce = horned_owl::io::omn::reader::parse_class_expression(ce, &pm, &build)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("class expression: {e}")))?;
    let v = owl_dl_reasoner::class_expression_satisfiable(&o, &ce).map_err(crate::errors::reason_error_to_py)?;
    Ok((v.holds(), v.incomplete()))
}
// _class_expression_entailed_subclass(path, sub, sup) -> (bool,bool); _class_expression_instances(path, ce) -> (Vec<String>, bool)
```
Register all three; expose them as private `_native` names.

- [ ] **Step 4: Python wrappers + `__all__` + `.pyi`**

In `__init__.py`, add public wrappers that unpack + warn (mirror the `same_individuals` wrapper):
```python
def class_expression_satisfiable(path, ce):
    holds, incomplete = _class_expression_satisfiable(path, ce)
    _warn_if_query_incomplete(incomplete)
    return holds
# entailed_subclass, instances analogous
```
Add the three public names to `__all__`; add `.pyi` stubs (`-> bool`, `-> bool`, `-> list[str]`) under a `# ── complex class-expression queries ──` header.

- [ ] **Step 5: Run → PASS** (test_queries + test_stubs), fmt, clippy.
Run: `python -m pytest tests/python/test_queries.py tests/python/test_stubs.py -q`

- [ ] **Step 6: Commit** `feat(python): class_expression_satisfiable / entailed_subclass / instances (#48)`

## Task 4: HermiT oracle + close #48

**Files:**
- Create: `crates/owl-dl-reasoner/tests/class_expr_oracle.rs`, `tests/fixtures/class_expr/ce.ofn`, committed `ce-materialized.owx`; optionally `docker/robot/class-expr-oracle.sh`.

**Interface:** Consumes Task 1 fns. FP=0 guard for the CE reduction.

- [ ] **Step 1:** Author `ce.ofn` — a consistent TBox+ABox with a probe-materializable expectation, e.g. `A ⊑ C`, `B ⊑ C`, individuals typed to A/B, so `A ⊔ B ⊑ C` is entailed and instances of `A ⊔ B` are known.
- [ ] **Step 2:** Generate the oracle: append `EquivalentClasses(<urn:probe> ObjectUnionOf(:A :B))` to a copy of `ce.ofn` and run `robot reason --reasoner hermit --axiom-generators "SubClass ClassAssertion"` (reuse the `docker/robot` pattern from Task 1.5/4.4 of the #44–#47 plan) → `ce-materialized.owx`; commit it. Document the regenerate command in the test header.
- [ ] **Step 3:** Write `class_expr_oracle.rs`:
  - `class_expression_entailed_subclass(&o, A⊔B, C)` must equal HermiT's verdict (probe `⊑ C` present in the oracle).
  - `class_expression_instances(&o, A⊔B)` ⊆ HermiT's instances of the probe (**FP-direction unconditional**); MISSED gated on `incomplete()`.
- [ ] **Step 4:** Run `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test class_expr_oracle`; `--lib`; clippy; fmt.
- [ ] **Step 5:** Commit `test(reasoner): HermiT oracle for complex class-expression queries (#48)`.
- [ ] **Step 6:** Full validation (`cargo test --workspace --exclude owl-dl-py`, workspace clippy `--all-features -D warnings`, Python suite). PR closing #48.

---

## Self-Review

**1. Spec coverage:**
- §2 probe-reduction (all three ops) → Task 1 (`class_expression_satisfiable`/`entailed_subclass`/`instances`), using the `justify::entails` template. ✓
- §2 fresh-probe guarantee → `ensure_fresh` + `ce_probe_iri_collision_errors` test. ✓
- §3 Manchester parsing at front-end via `parse_class_expression` + ontology prefix map → Task 2 `parse_ce`/`parse_ofn_with_pm`, Task 3 `load_path_with_pm`. Parse error → clean error (Task 2 `?`, Task 3 `PyValueError`). ✓
- §4 three-layer surface (reasoner/CLI-json/Python) → Tasks 1/2/3. ✓
- §5 testing (unit incl. negative control + collision + probe-not-leaking; HermiT oracle; golden json; stub-drift) → Tasks 1/2/3/4. ✓
- §6 scope (object CEs focus; data-range CEs reduce identically, inherit existing datatype under-approx) → no special-casing needed; reduction is CE-agnostic. ✓
- §7 open items: `incomplete` = `!QueryStats.pure_el_mode` (resolved, from the `_with_stats` variants; instances via a companion sat-stats call); `SetOntology::insert(Component::EquivalentClasses)` (resolved — the `justify::entails` precedent); probe freshness across Builds (IRIs compare by string — `ensure_fresh` + collision test). ✓

**2. Placeholder scan:** No TBD/vague steps. The "verify against the real horned-owl API" notes in Task 1 Step 3 name the exact things to confirm (`DeclareClass` inner path, `insert` arg type) with the `justify::entails`/`convert.rs` precedents to copy — not blank placeholders.

**3. Type consistency:** `CeVerdict.holds()`/`incomplete()` and `CeInstances.individuals()`/`incomplete()` used identically across reasoner→CLI (`build_*_expr_json`)→Python. `parse_ce`/`parse_class_expression` signature `(s, &pm, &build)` consistent between CLI and Python. Native `(value, incomplete)` tuple → Python wrapper unpack matches the #44–#47 mechanism. Probe IRIs (`urn:rustdl-ce-probe:q`/`:q2`) consistent between the fns and the not-leaking test assertion.
