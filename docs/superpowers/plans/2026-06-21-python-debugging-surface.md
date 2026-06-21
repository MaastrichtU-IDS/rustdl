# Python debugging surface Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expose the shipped explanation/debugging suite in the Python API (`justify`, `diagnose`, `repair`, a one-call `debug`) and fix the broken `materialize_*` re-exports — so the documented `rustdl.X` surface actually works.

**Architecture:** A shared `justify::parse_query` lifted from the CLI; a new `owl-dl-py/src/explain.rs` of thin PyO3 wrappers (string/tuple forms, Manchester full-IRI rendering); `__init__.py` re-export fixes + a pure-Python `debug()` convenience.

**Tech Stack:** Rust (PyO3/maturin), `owl-dl-reasoner`, Python 3.10+.

**Spec:** `docs/superpowers/specs/2026-06-21-python-debugging-surface-design.md`
**Branch:** `feat/python-debugging-surface`

## Environment notes

- cargo prefix: `export RUSTUP_HOME=/home/dumontier/.rustup; export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"`.
- **owl-dl-py is a PyO3 cdylib**: `cargo build/clippy -p owl-dl-py` works locally (extension-module defers Python symbol resolution). But **the pytest suite needs a maturin build** which is NOT available in this environment — so the Python tests are WRITTEN here and RUN by `.github/workflows/python-ci.yml` (maturin + pytest, 3.10/3.13). Local gate = the Rust side + static `__init__.py` validation (`python3 -m py_compile`).

## Key facts (verified)

- `owl-dl-cli/src/main.rs`: `parse_justify_query(parts: &[String]) -> anyhow::Result<Entailment>` (~line 632) + `parse_literal_arg` (~line 650). Used by the `Justify`/`Repair` handlers.
- `owl-dl-py/src/load.rs`: `load_path(path: &str) -> PyResult<SetOntology<RcStr>>`.
- `owl-dl-py/src/errors.rs`: `reason_error_to_py(ReasonError) -> PyErr`.
- Reasoner: `justify::{find_one_justification, find_all_justifications, Entailment}`, `diagnose` (`Diagnosis{consistent, roots: Vec<String>, derived: Vec<DerivedClass{iri,roots}>, …}`), `find_repairs` (`Repairs{repairs: Vec<Repair{remove: Vec<Component>}>}`).
- Manchester rendering: `Component::as_manchester_with_prefixes(&PrefixMapping)` (returns a Display wrapper → `.to_string()`); needs `use horned_owl::io::omn::AsManchester;` and `horned_owl::curie::PrefixMapping`.
- `_native` registered in `owl-dl-py/src/lib.rs` (`mod`s + `register` calls).
- `python/rustdl/__init__.py` re-exports a subset from `rustdl._native` and defines `__all__`.

## File structure

- **Modify** `crates/owl-dl-reasoner/src/justify.rs` — add `pub fn parse_query` (+ `parse_literal_arg`).
- **Modify** `crates/owl-dl-cli/src/main.rs` — use `justify::parse_query`; drop the local copies.
- **Create** `crates/owl-dl-py/src/explain.rs` — `justify`/`justify_all`/`diagnose`/`repair` bindings + `render`.
- **Modify** `crates/owl-dl-py/src/lib.rs` — `mod explain; explain::register(m)?;`.
- **Modify** `crates/owl-dl-py/python/rustdl/__init__.py` — re-export fixes, `debug()`, `__all__`.
- **Create** `crates/owl-dl-py/tests/python/test_explain.py` — pytest (runs in CI).
- **Modify** `README.md`, `CLAUDE.md`.

---

### Task 1: Lift the query parser into the reasoner

**Files:** Modify `crates/owl-dl-reasoner/src/justify.rs`, `crates/owl-dl-cli/src/main.rs`

- [ ] **Step 1: Branch**

```bash
cd /data/dumontier/rustdl
git checkout main
git checkout -b feat/python-debugging-surface
```

- [ ] **Step 2: Read the CLI's `parse_justify_query` + `parse_literal_arg`** (`crates/owl-dl-cli/src/main.rs`, ~lines 632–700). Copy their full bodies.

- [ ] **Step 3: Add `parse_query` + `parse_literal_arg` to `crates/owl-dl-reasoner/src/justify.rs`** as `pub` functions, returning `Result<Entailment, String>` (the error is the human message; callers wrap it). Convert the CLI's `anyhow::bail!("…")` into `return Err(format!("…"))` and the final `_ => bail!` into `_ => return Err(...)`. Keep the exact same query grammar (all 14 forms + the literal parsing). `Entailment` is already defined in this file. Example skeleton (fill in the arms from the CLI verbatim, swapping the error style):

```rust
/// Parse a justify/repair query from CLI-style tokens into an [`Entailment`].
/// Shared by the CLI and the Python bindings. `Err` is a human-readable message.
pub fn parse_query(parts: &[String]) -> Result<Entailment, String> {
    let kind = parts.first().map_or("", String::as_str);
    Ok(match (kind, parts.len()) {
        ("subclass", 3) => Entailment::SubClassOf { sub: parts[1].clone(), sup: parts[2].clone() },
        // … all other arms copied verbatim from the CLI …
        ("data-value", 4) => {
            let (value_lexical, value_datatype) = parse_literal_arg(&parts[3]);
            Entailment::DataPropertyValue {
                source: parts[1].clone(), prop: parts[2].clone(), value_lexical, value_datatype,
            }
        }
        _ => return Err(format!("unrecognized query: {parts:?}")),
    })
}

/// Parse `"lex"^^<dt>` / `"lex"^^xsd:type` / `"lex"` into `(lexical, datatype_iri)`.
pub fn parse_literal_arg(s: &str) -> (String, String) {
    // … verbatim from the CLI …
}
```

- [ ] **Step 4: Switch the CLI to the shared parser.** In `main.rs`, delete the local `parse_justify_query` + `parse_literal_arg`, and replace call sites `parse_justify_query(&query)?` with
  `owl_dl_reasoner::justify::parse_query(&query).map_err(|e| anyhow::anyhow!(e))?`.
  (There are call sites in the `Justify` and `Repair` handlers.)

- [ ] **Step 5: Build + CLI smoke (parser-move regression)**
```bash
cargo build -p owl-dl-reasoner -p owl-dl-cli --release
cat > /tmp/pq.ofn <<'EOF'
Prefix(:=<urn:>)
Ontology(Declaration(Class(:A)) Declaration(Class(:Bad))
  SubClassOf(:Bad ObjectIntersectionOf(:A ObjectComplementOf(:A))) )
EOF
./target/release/rustdl justify /tmp/pq.ofn unsat urn:Bad
```
Expected: a justification prints (CLI behaviour unchanged). Then:
```bash
cargo clippy -p owl-dl-reasoner -p owl-dl-cli --all-targets -- -D warnings
cargo fmt -p owl-dl-reasoner -p owl-dl-cli
cargo test -p owl-dl-reasoner --lib justify 2>&1 | grep -E 'test result' | tail -3
```
All green.

- [ ] **Step 6: Commit**
```bash
git add crates/owl-dl-reasoner/src/justify.rs crates/owl-dl-cli/src/main.rs
git commit -m "refactor(justify): lift parse_query into the reasoner (shared by CLI + Python)"
```

---

### Task 2: `explain.rs` native bindings

**Files:** Create `crates/owl-dl-py/src/explain.rs`; Modify `crates/owl-dl-py/src/lib.rs`

- [ ] **Step 1: Create `crates/owl-dl-py/src/explain.rs`**

```rust
//! Python bindings for the explanation/debugging suite (justify, diagnose, repair).
//! String/tuple forms; axioms rendered as Manchester with full IRIs.

use horned_owl::curie::PrefixMapping;
use horned_owl::io::omn::AsManchester;
use horned_owl::model::{Component, RcStr};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use crate::errors::reason_error_to_py;
use crate::load;

fn render(ax: &Component<RcStr>) -> String {
    ax.as_manchester_with_prefixes(&PrefixMapping::default()).to_string()
}

/// One minimal justification for a query (CLI-style tokens, e.g.
/// `["subclass", sub, sup]`, `["unsat", c]`, `["inconsistent"]`) as Manchester
/// axiom strings. Empty list if not entailed.
#[pyfunction]
pub(crate) fn justify(path: &str, query: Vec<String>) -> PyResult<Vec<String>> {
    let onto = load::load_path(path)?;
    let q = owl_dl_reasoner::justify::parse_query(&query).map_err(PyValueError::new_err)?;
    let j = owl_dl_reasoner::justify::find_one_justification(&onto, &q).map_err(reason_error_to_py)?;
    Ok(j.map(|j| j.axioms.iter().map(render).collect()).unwrap_or_default())
}

/// All minimal justifications (capped by `max`).
#[pyfunction]
#[pyo3(signature = (path, query, max = 10))]
pub(crate) fn justify_all(path: &str, query: Vec<String>, max: usize) -> PyResult<Vec<Vec<String>>> {
    let onto = load::load_path(path)?;
    let q = owl_dl_reasoner::justify::parse_query(&query).map_err(PyValueError::new_err)?;
    let js = owl_dl_reasoner::justify::find_all_justifications(&onto, &q, max).map_err(reason_error_to_py)?;
    Ok(js.into_iter().map(|j| j.axioms.iter().map(render).collect()).collect())
}

/// Root/derived unsatisfiability partition:
/// `(consistent, roots, [(derived_iri, [root_iri, …]), …])`.
#[pyfunction]
pub(crate) fn diagnose(path: &str) -> PyResult<(bool, Vec<String>, Vec<(String, Vec<String>)>)> {
    let onto = load::load_path(path)?;
    let d = owl_dl_reasoner::diagnose(&onto).map_err(reason_error_to_py)?;
    let derived = d.derived.into_iter().map(|dc| (dc.iri, dc.roots)).collect();
    Ok((d.consistent, d.roots, derived))
}

/// Minimal repairs for a query: each is a list of Manchester axioms to remove.
#[pyfunction]
#[pyo3(signature = (path, query, max = 10))]
pub(crate) fn repair(path: &str, query: Vec<String>, max: usize) -> PyResult<Vec<Vec<String>>> {
    let onto = load::load_path(path)?;
    let q = owl_dl_reasoner::justify::parse_query(&query).map_err(PyValueError::new_err)?;
    let r = owl_dl_reasoner::find_repairs(&onto, &q, max).map_err(reason_error_to_py)?;
    Ok(r.repairs.into_iter().map(|rep| rep.remove.iter().map(render).collect()).collect())
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(justify, m)?)?;
    m.add_function(wrap_pyfunction!(justify_all, m)?)?;
    m.add_function(wrap_pyfunction!(diagnose, m)?)?;
    m.add_function(wrap_pyfunction!(repair, m)?)?;
    Ok(())
}
```
If `as_manchester_with_prefixes` doesn't resolve, confirm the trait path `horned_owl::io::omn::AsManchester` (the CLI's `render` in `main.rs` shows the exact usage — match it). If `find_one_justification`'s closure `.map(render)` complains about `&Component` vs `Component`, use `.map(|a| render(a))`.

- [ ] **Step 2: Register in `lib.rs`** — add `mod explain;` and `explain::register(m)?;` in the `_native` `#[pymodule]` fn (next to the other `::register(m)?` calls).

- [ ] **Step 3: Build + clippy + fmt**
```bash
cargo build -p owl-dl-py
cargo clippy -p owl-dl-py --all-targets -- -D warnings
cargo fmt -p owl-dl-py
```
Green. If clippy `type_complexity` fires on the tuple return of `diagnose`, add `#[allow(clippy::type_complexity)]` (as the materialize bindings do). If `doc_markdown` fires, backtick.

- [ ] **Step 4: Commit**
```bash
git add crates/owl-dl-py/src/explain.rs crates/owl-dl-py/src/lib.rs
git commit -m "feat(py): native bindings for justify / justify_all / diagnose / repair"
```

---

### Task 3: `__init__.py` — fix re-exports + `debug()`

**Files:** Modify `crates/owl-dl-py/python/rustdl/__init__.py`

- [ ] **Step 1: Fix the materialize re-exports + import the new explain functions.** In the `from rustdl._native import ( … )` block, ADD:
```python
    materialize_inferred_property_assertions as materialize_inferred_property_assertions,
    materialize_inferred_data_property_assertions as materialize_inferred_data_property_assertions,
    materialize_inferred_subobjectproperty_axioms as materialize_inferred_subobjectproperty_axioms,
    materialize_inferred_subdataproperty_axioms as materialize_inferred_subdataproperty_axioms,
    materialize_existential_successors as materialize_existential_successors,
    justify as justify,
    justify_all as justify_all,
    diagnose as diagnose,
    repair as repair,
```

- [ ] **Step 2: Add the `debug()` convenience** (pure Python, after the `classify` helpers):

```python
def debug(path):
    """One-call ontology diagnosis. Returns a JSON-serializable dict:

    consistent ontology with unsatisfiable classes →
      {"consistent": True, "unsatisfiable": [iri…],
       "roots": [{"iri", "justification": [axiom…], "repairs": [[axiom…]…],
                  "derives": [iri…]}…],
       "derived": [{"iri", "roots": [iri…]}…]}

    inconsistent ontology →
      {"consistent": False, "unsatisfiable": [], "roots": [], "derived": [],
       "inconsistency": {"justification": [axiom…], "repairs": [[axiom…]…]}}

    Composes diagnose / justify / repair. Read-only; sound by construction."""
    consistent, roots, derived = diagnose(path)
    if not consistent:
        return {
            "consistent": False,
            "unsatisfiable": [],
            "roots": [],
            "derived": [],
            "inconsistency": {
                "justification": justify(path, ["inconsistent"]),
                "repairs": repair(path, ["inconsistent"], 10),
            },
        }
    root_objs = [
        {
            "iri": r,
            "justification": justify(path, ["unsat", r]),
            "repairs": repair(path, ["unsat", r], 10),
            "derives": [d for (d, rs) in derived if r in rs],
        }
        for r in roots
    ]
    return {
        "consistent": True,
        "unsatisfiable": list(roots) + [d for (d, _) in derived],
        "roots": root_objs,
        "derived": [{"iri": d, "roots": rs} for (d, rs) in derived],
    }
```

- [ ] **Step 3: Extend `__all__`** — add: `"materialize_inferred_property_assertions"`, `"materialize_inferred_data_property_assertions"`, `"materialize_inferred_subobjectproperty_axioms"`, `"materialize_inferred_subdataproperty_axioms"`, `"materialize_existential_successors"`, `"justify"`, `"justify_all"`, `"diagnose"`, `"repair"`, `"debug"`.

- [ ] **Step 4: Static validation** (no maturin needed):
```bash
python3 -m py_compile crates/owl-dl-py/python/rustdl/__init__.py && echo "PY COMPILE OK"
```
And confirm every `__all__` name is either imported from `_native`, imported as a submodule, or defined in the file (eyeball the diff). Expected: PY COMPILE OK.

- [ ] **Step 5: Commit**
```bash
git add crates/owl-dl-py/python/rustdl/__init__.py
git commit -m "feat(py): expose justify/diagnose/repair/debug + fix materialize re-exports"
```

---

### Task 4: pytest (runs in CI)

**Files:** Create `crates/owl-dl-py/tests/python/test_explain.py`

- [ ] **Step 1: Write the tests** (they run in `.github/workflows/python-ci.yml` via maturin; not runnable in this environment). Use a fixture written inline so it needs no corpus:

```python
"""Tests for the Python explanation/debugging surface + materialize re-exports."""
import rustdl

BROKEN = """Prefix(:=<urn:>)
Ontology(
  Declaration(Class(:A)) Declaration(Class(:Bad)) Declaration(Class(:SubBad))
  SubClassOf(:Bad ObjectIntersectionOf(:A ObjectComplementOf(:A)))
  SubClassOf(:SubBad :Bad)
)
"""

def _write(tmp_path, text, name="o.ofn"):
    p = tmp_path / name
    p.write_text(text)
    return str(p)


# Regression: the materialize_* functions must be reachable as rustdl.X
# (the missing __init__ re-export bug).
def test_materialize_reexports_present():
    for name in [
        "materialize_inferred_property_assertions",
        "materialize_inferred_data_property_assertions",
        "materialize_inferred_subobjectproperty_axioms",
        "materialize_inferred_subdataproperty_axioms",
        "materialize_existential_successors",
        "justify",
        "justify_all",
        "diagnose",
        "repair",
        "debug",
    ]:
        assert hasattr(rustdl, name), f"rustdl.{name} not exported"
        assert name in rustdl.__all__, f"{name} missing from __all__"


def test_justify(tmp_path):
    p = _write(tmp_path, BROKEN)
    ax = rustdl.justify(p, ["unsat", "urn:Bad"])
    assert ax, "expected a non-empty justification"
    assert any("Bad" in a for a in ax)


def test_diagnose(tmp_path):
    p = _write(tmp_path, BROKEN)
    consistent, roots, derived = rustdl.diagnose(p)
    assert consistent is True
    assert "urn:Bad" in roots
    assert any(d == "urn:SubBad" for (d, _) in derived)


def test_repair(tmp_path):
    p = _write(tmp_path, BROKEN)
    reps = rustdl.repair(p, ["unsat", "urn:Bad"], 10)
    assert reps and all(isinstance(r, list) for r in reps)


def test_debug_consistent_with_unsat(tmp_path):
    p = _write(tmp_path, BROKEN)
    d = rustdl.debug(p)
    assert d["consistent"] is True
    assert "urn:Bad" in d["unsatisfiable"]
    bad = next(r for r in d["roots"] if r["iri"] == "urn:Bad")
    assert bad["justification"] and bad["repairs"]
    assert "urn:SubBad" in bad["derives"]


def test_debug_coherent(tmp_path):
    p = _write(tmp_path, "Prefix(:=<urn:>)\nOntology(Declaration(Class(:A)))\n")
    d = rustdl.debug(p)
    assert d["consistent"] is True
    assert d["unsatisfiable"] == []


def test_materialize_property_assertions(tmp_path):
    p = _write(tmp_path, """Prefix(:=<urn:>)
Ontology(
  Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
  Declaration(ObjectProperty(:hasParent)) Declaration(ObjectProperty(:hasAncestor))
  SubObjectPropertyOf(:hasParent :hasAncestor)
  ObjectPropertyAssertion(:hasParent :a :b)
)
""")
    triples = rustdl.materialize_inferred_property_assertions(p)
    assert ("urn:a", "urn:hasAncestor", "urn:b") in triples
```

- [ ] **Step 2: Syntax-check** (can't run pytest without maturin):
```bash
python3 -m py_compile crates/owl-dl-py/tests/python/test_explain.py && echo "PY COMPILE OK"
```
Note in your report that the suite runs in CI (`python-ci.yml`), not locally.

- [ ] **Step 3: Commit**
```bash
git add crates/owl-dl-py/tests/python/test_explain.py
git commit -m "test(py): explanation surface + materialize re-export regression (runs in CI)"
```

---

### Task 5: Docs + final gate

**Files:** Modify `README.md`, `CLAUDE.md`

- [ ] **Step 1: README** — in the `import rustdl` Python example block, add a debugging one-liner, e.g.:
```python
report = rustdl.debug("ontology.ofn")   # consistency + root/derived unsat + justifications + repairs
```
Match surrounding style.

- [ ] **Step 2: CLAUDE.md** — append to the `owl-dl-py` / Python documentation:
```
Python now exposes the explanation/debugging suite: `rustdl.justify` / `justify_all` /
`diagnose` / `repair` (string/tuple forms) + the one-call `rustdl.debug(path)` (structured
dict). The query parser is shared via `owl_dl_reasoner::justify::parse_query`. (The
`materialize_*` re-exports were also fixed — they were registered in `_native` but missing
from `__init__.py`.) See `docs/superpowers/specs/2026-06-21-python-debugging-surface-design.md`.
```

- [ ] **Step 3: Rust workspace gate** (the locally-verifiable gate; Python pytest runs in CI):
```bash
cd /data/dumontier/rustdl
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
python3 -m py_compile crates/owl-dl-py/python/rustdl/__init__.py crates/owl-dl-py/tests/python/test_explain.py
```
All green. After `cargo fmt --all`, `git status --short` and stage every fmt-touched file. Report the `cargo test --workspace` aggregate + the py_compile result. (Note: `cargo test --workspace` builds owl-dl-py as a cdylib but runs no Rust tests in it; the pytest job is CI-only.)

- [ ] **Step 4: Commit**
```bash
cd /data/dumontier/rustdl
git add -A
git status --short
git commit -m "docs(py): document the Python debugging surface (justify/diagnose/repair/debug)"
```

---

## Self-review notes (author)

- **Spec coverage:** re-export fix → Task 3 Step 1 (+ `test_materialize_reexports_present`); shared parser → Task 1; justify/diagnose/repair bindings → Task 2; `debug()` → Task 3 Step 2; tests → Task 4; docs → Task 5.
- **Soundness:** presentation/reach only — every binding wraps a shipped sound reasoner fn; `debug()` composes them; the parser move is behaviour-preserving (CLI smoke in Task 1).
- **Environment honesty:** the Python tests CANNOT run locally (no maturin); they run in CI (`python-ci.yml`). Local gate = Rust (`cargo`) + `python3 -m py_compile`. This is stated in Tasks 4–5.
- **No placeholders** except the explicit "copy the CLI arms verbatim" in Task 1 Step 3 (the parser body is large and already exists — copy it, swapping `bail!`→`Err`).
- **Type consistency:** binding signatures (`Vec<String>` query, `Vec<Vec<String>>` repairs, `(bool, Vec<String>, Vec<(String,Vec<String>)>)` diagnose) match the `debug()` consumer and the tests.
- **API risk flagged inline:** `AsManchester` trait path + `.to_string()`, `parse_query` error-type change (`Result<_, String>`), `type_complexity`/`doc_markdown` clippy, and the CLI call-site swap.
