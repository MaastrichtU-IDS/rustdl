# Python type stubs + docstrings Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the `rustdl` Python package typed and discoverable — a `py.typed` marker, a complete `__init__.pyi` stub (precise types + docstrings, incl. `debug()` TypedDicts), filled-in native docstrings, and a stub-consistency test.

**Architecture:** Hand-written `python/rustdl/__init__.pyi` (the type-checker source of truth for `import rustdl`) + `py.typed`; both ship via maturin's `python-source` tree. No runtime/engine change — `.pyi` is type-checker-only.

**Tech Stack:** Python typing (PEP 561, `.pyi`, `TypedDict`), maturin, a little Rust (docstrings).

**Spec:** `docs/superpowers/specs/2026-06-21-python-type-stubs-design.md`
**Branch:** `feat/python-type-stubs`

## Environment notes
- cargo prefix: `export RUSTUP_HOME=/home/dumontier/.rustup; export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"`.
- `python3` available (no mypy/pytest/maturin). The `.pyi` is validated by `py_compile` + an AST consistency check (both run locally, no compiled module needed); the pytest version runs in CI.
- The `.pyi` does NOT affect runtime (Python uses `__init__.py`); zero risk to the working package.

## Key facts (verified)
- `rustdl.__all__` (28 names) is in `python/rustdl/__init__.py`. The stub must declare every one.
- `Classification` `#[pyclass]` (classify.rs): getters `classes/unsatisfiable/inconsistent/timed_out_pairs/complete`; methods `is_subclass(sub,sup)`, `equivalent_classes(cls)`, `direct_subsumers(cls)`, `__repr__`; pure-Python `subclasses_of(cls)`/`superclasses_of(cls)` bound in `__init__.py`.
- `classify(path, *, per_pair_timeout_ms=1000, saturation_only=False)`; `classify_bytes(data, *, format, …)`.
- `examples.py` is already inline-typed → no stub for it.
- `queries.rs` natives lack `///` docs: `is_consistent`, `is_class_satisfiable`, `is_subclass_of`, `is_instance_of`, `instances_of`, `realize`.

## File structure
- **Create** `crates/owl-dl-py/python/rustdl/py.typed` (empty).
- **Create** `crates/owl-dl-py/python/rustdl/__init__.pyi`.
- **Modify** `crates/owl-dl-py/src/queries.rs` — add `///` docs.
- **Modify** `crates/owl-dl-py/pyproject.toml` — add `"Typing :: Typed"` classifier.
- **Create** `crates/owl-dl-py/tests/python/test_stubs.py` — consistency test.
- **Modify** `CLAUDE.md`.

---

### Task 1: `py.typed` + `__init__.pyi` + classifier

**Files:** Create `py.typed`, `__init__.pyi`; Modify `pyproject.toml`

- [ ] **Step 1: Branch**

```bash
cd /data/dumontier/rustdl
git checkout main
git checkout -b feat/python-type-stubs
```

- [ ] **Step 2: Create the empty marker** `crates/owl-dl-py/python/rustdl/py.typed` (zero bytes):
```bash
: > crates/owl-dl-py/python/rustdl/py.typed
```

- [ ] **Step 3: Create `crates/owl-dl-py/python/rustdl/__init__.pyi`** with this exact content:

```python
"""Type stubs for rustdl — sound OWL 2 DL (SROIQ) reasoner. See __init__.py."""

from typing import TypedDict

from . import examples as examples

__version__: str

# ── result types ────────────────────────────────────────────────────────────

class Classification:
    """Result of `classify` — the computed subsumption hierarchy."""

    @property
    def classes(self) -> list[str]:
        """Every declared class IRI."""
        ...
    @property
    def unsatisfiable(self) -> list[str]:
        """Classes proved equivalent to owl:Nothing."""
        ...
    @property
    def inconsistent(self) -> bool:
        """True iff the ontology is inconsistent (every class unsatisfiable)."""
        ...
    @property
    def timed_out_pairs(self) -> int:
        """Number of subsumption pairs that hit the per-pair timeout."""
        ...
    @property
    def complete(self) -> bool:
        """True iff no pair timed out (the hierarchy is exact)."""
        ...
    def is_subclass(self, sub: str, sup: str) -> bool:
        """True iff `sub ⊑ sup` is entailed."""
        ...
    def equivalent_classes(self, cls: str) -> list[str]:
        """Classes equivalent to `cls`."""
        ...
    def direct_subsumers(self, cls: str) -> list[str]:
        """Direct (Hasse-parent) super-classes of `cls`."""
        ...
    def subclasses_of(self, cls: str) -> list[str]:
        """All D with D ⊑ cls (reflexive + proper)."""
        ...
    def superclasses_of(self, cls: str) -> list[str]:
        """All D with cls ⊑ D (reflexive + proper)."""
        ...
    def __repr__(self) -> str: ...

class IncompleteClassificationWarning(UserWarning):
    """Emitted when classification hit the per-pair timeout (sound but possibly
    incomplete)."""

class RootReport(TypedDict):
    iri: str
    justification: list[str]
    repairs: list[list[str]]
    derives: list[str]

class DerivedReport(TypedDict):
    iri: str
    roots: list[str]

class InconsistencyReport(TypedDict):
    justification: list[str]
    repairs: list[list[str]]

class DebugReport(TypedDict, total=False):
    consistent: bool
    unsatisfiable: list[str]
    roots: list[RootReport]
    derived: list[DerivedReport]
    inconsistency: InconsistencyReport

# ── exceptions ──────────────────────────────────────────────────────────────

class RustdlError(Exception):
    """Base exception for all rustdl errors."""

class ParseError(RustdlError):
    """OWL parser failure."""

class UnsupportedAxiomError(RustdlError):
    """An axiom uses a construct rustdl does not support."""

class UnknownClassError(RustdlError):
    """A queried IRI is not a declared class in the ontology."""

# ── core reasoning ──────────────────────────────────────────────────────────

def classify(
    path: str, *, per_pair_timeout_ms: int = 1000, saturation_only: bool = False
) -> Classification:
    """Classify the ontology at `path` (format auto-detected)."""
    ...

def classify_bytes(
    data: bytes, *, format: str, per_pair_timeout_ms: int = 1000, saturation_only: bool = False
) -> Classification:
    """Like `classify`, from in-memory bytes with explicit `format`."""
    ...

def is_consistent(path: str) -> bool:
    """True iff the ontology is consistent."""
    ...

def is_class_satisfiable(path: str, class_iri: str) -> bool:
    """True iff `class_iri` is satisfiable (not ⊑ ⊥)."""
    ...

def is_subclass_of(path: str, sub: str, sup: str) -> bool:
    """True iff `sub ⊑ sup` is entailed."""
    ...

def is_instance_of(path: str, class_iri: str, individual_iri: str) -> bool:
    """True iff `individual_iri` is an instance of `class_iri`."""
    ...

def instances_of(path: str, class_iri: str) -> list[str]:
    """Named individuals entailed to be instances of `class_iri`."""
    ...

def realize(path: str) -> dict[str, list[str]]:
    """Map each named individual to its most-specific entailed types."""
    ...

# ── inference materialization ───────────────────────────────────────────────

def materialize_inferred_subclass_axioms(path: str) -> list[tuple[str, str]]:
    """Every entailed (sub, sup) class-subsumption pair."""
    ...

def materialize_inferred_class_assertions(path: str) -> list[tuple[str, str]]:
    """Every entailed (class, individual) most-specific class assertion."""
    ...

def materialize_inferred_property_assertions(path: str) -> list[tuple[str, str, str]]:
    """Inferred object property assertions (subject, property, object) over named
    individuals."""
    ...

def materialize_inferred_data_property_assertions(
    path: str,
) -> list[tuple[str, str, str, str, str]]:
    """Inferred data property assertions (subject, property, lexical, datatype, lang)."""
    ...

def materialize_inferred_subobjectproperty_axioms(path: str) -> list[tuple[str, str]]:
    """Inferred object property subsumption pairs (sub, sup)."""
    ...

def materialize_inferred_subdataproperty_axioms(path: str) -> list[tuple[str, str]]:
    """Inferred data property subsumption pairs (sub, sup)."""
    ...

def materialize_existential_successors(path: str) -> list[tuple[str, str, str, str]]:
    """Entailed existential successors as (subject, property, witness_blank, filler_class)
    — a blank-node representation of entailed `a : ∃R.C` (not ground triples)."""
    ...

# ── explanation & debugging ─────────────────────────────────────────────────

def justify(path: str, query: list[str]) -> list[str]:
    """One minimal justification (Manchester axioms) for `query`
    (e.g. ["unsat", c] / ["subclass", s, t] / ["inconsistent"])."""
    ...

def justify_all(path: str, query: list[str], max: int = 10) -> list[list[str]]:
    """Up to `max` minimal justifications for `query`."""
    ...

def diagnose(path: str) -> tuple[bool, list[str], list[tuple[str, list[str]]]]:
    """(consistent, root_unsat_iris, [(derived_iri, [root_iri, ...]), ...])."""
    ...

def repair(path: str, query: list[str], max: int = 10) -> list[list[str]]:
    """Minimal axiom-removal sets (Manchester) that break `query`."""
    ...

def debug(path: str) -> DebugReport:
    """One-call ontology diagnosis: consistency + root/derived unsat +
    per-root justifications + repairs."""
    ...
```

- [ ] **Step 4: Add the `Typing :: Typed` classifier** to `crates/owl-dl-py/pyproject.toml` — inside the `classifiers = [ … ]` list, add a line:
```toml
    "Typing :: Typed",
```

- [ ] **Step 5: Validate locally**
```bash
cd /data/dumontier/rustdl
python3 -m py_compile crates/owl-dl-py/python/rustdl/__init__.pyi && echo "PYI COMPILE OK"
# AST consistency: every __all__ name (from __init__.py) is declared in __init__.pyi.
python3 - <<'PY'
import ast, pathlib
base = pathlib.Path("crates/owl-dl-py/python/rustdl")
src = ast.parse((base / "__init__.py").read_text())
all_names = None
for n in ast.walk(src):
    if isinstance(n, ast.Assign):
        for t in n.targets:
            if isinstance(t, ast.Name) and t.id == "__all__":
                all_names = [e.value for e in n.value.elts]
assert all_names, "could not find __all__"
stub = ast.parse((base / "__init__.pyi").read_text())
declared = set()
for node in stub.body:
    if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef)):
        declared.add(node.name)
    elif isinstance(node, ast.AnnAssign) and isinstance(node.target, ast.Name):
        declared.add(node.target.id)
    elif isinstance(node, ast.ImportFrom):
        for a in node.names:
            declared.add(a.asname or a.name)
missing = set(all_names) - declared
assert not missing, f"__all__ names missing from __init__.pyi: {sorted(missing)}"
print(f"STUB CONSISTENT: all {len(all_names)} __all__ names declared")
PY
```
Expected: `PYI COMPILE OK` and `STUB CONSISTENT: all 28 __all__ names declared`. If any name is missing, add it to the `.pyi` and re-run.

- [ ] **Step 6: Commit**
```bash
git add crates/owl-dl-py/python/rustdl/py.typed crates/owl-dl-py/python/rustdl/__init__.pyi crates/owl-dl-py/pyproject.toml
git commit -m "feat(py): py.typed + __init__.pyi (typed, discoverable Python surface)"
```

---

### Task 2: Fill missing native docstrings

**Files:** Modify `crates/owl-dl-py/src/queries.rs`

- [ ] **Step 1: Add `///` docs** above each of these `#[pyfunction]`s in `queries.rs` (concise one-liners matching the `.pyi`). Example:
```rust
/// True iff the ontology at `path` is consistent.
#[pyfunction]
pub(crate) fn is_consistent(path: &str) -> PyResult<bool> {
```
Add equivalent one-liners for `is_class_satisfiable`, `is_subclass_of`, `is_instance_of`, `instances_of`, and `realize` (mirror the `.pyi` docstrings). PyO3 turns the `///` into the function's Python `__doc__` so `help(rustdl.is_consistent)` works at runtime.

- [ ] **Step 2: Build + clippy + fmt**
```bash
cargo build -p owl-dl-py
cargo clippy -p owl-dl-py --all-targets -- -D warnings
cargo fmt -p owl-dl-py
```
Green. (Watch `clippy::doc_markdown` — backtick `owl:Nothing`-style tokens / IRIs as needed.)

- [ ] **Step 3: Commit**
```bash
git add crates/owl-dl-py/src/queries.rs
git commit -m "docs(py): fill missing native docstrings (queries) for runtime help()"
```

---

### Task 3: Stub-consistency test + docs + final gate

**Files:** Create `crates/owl-dl-py/tests/python/test_stubs.py`; Modify `CLAUDE.md`

- [ ] **Step 1: Create the consistency test** (runs in CI; also exercises the AST compare). `crates/owl-dl-py/tests/python/test_stubs.py`:

```python
"""The hand-written __init__.pyi must stay in sync with the runtime __all__."""
import ast
import pathlib

import rustdl


def _stub_declared_names() -> set[str]:
    stub = pathlib.Path(rustdl.__file__).with_name("__init__.pyi")
    tree = ast.parse(stub.read_text())
    names: set[str] = set()
    for node in tree.body:
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef)):
            names.add(node.name)
        elif isinstance(node, ast.AnnAssign) and isinstance(node.target, ast.Name):
            names.add(node.target.id)
        elif isinstance(node, ast.ImportFrom):
            for a in node.names:
                names.add(a.asname or a.name)
    return names


def test_pyi_exists_and_py_typed():
    pkg = pathlib.Path(rustdl.__file__).parent
    assert (pkg / "__init__.pyi").is_file()
    assert (pkg / "py.typed").is_file()


def test_every_public_name_is_stubbed():
    declared = _stub_declared_names()
    missing = set(rustdl.__all__) - declared
    assert not missing, f"__all__ names missing from __init__.pyi: {sorted(missing)}"


def test_stub_names_are_real():
    # Every top-level def/class in the stub (minus TypedDict helpers) is a real
    # attribute of the package — the stub doesn't promise names that don't exist.
    helpers = {"RootReport", "DerivedReport", "InconsistencyReport", "DebugReport"}
    for name in _stub_declared_names() - helpers - {"examples"}:
        assert hasattr(rustdl, name), f"stub declares {name!r} but rustdl has no such attr"
```

- [ ] **Step 2: Syntax-check** (pytest itself runs in CI — needs maturin):
```bash
python3 -m py_compile crates/owl-dl-py/tests/python/test_stubs.py && echo "PY COMPILE OK"
```

- [ ] **Step 3: CLAUDE.md** — append to the `owl-dl-py` / Python docs:
```
The Python package is typed (PEP 561): `python/rustdl/py.typed` + a hand-written
`__init__.pyi` covering the full surface (functions, the `Classification` class,
exceptions, and `debug()` TypedDicts). `tests/python/test_stubs.py` guards stub↔`__all__`
drift. See `docs/superpowers/specs/2026-06-21-python-type-stubs-design.md`.
```

- [ ] **Step 4: Final gate**
```bash
cd /data/dumontier/rustdl
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
python3 -m py_compile crates/owl-dl-py/python/rustdl/__init__.pyi crates/owl-dl-py/tests/python/test_stubs.py
# re-run the AST consistency check from Task 1 Step 5
```
All green. After `cargo fmt --all`, `git status --short` and stage any fmt-touched file. Report the `cargo test --workspace` aggregate + py_compile + AST-check results.

- [ ] **Step 5: Commit**
```bash
cd /data/dumontier/rustdl
git add -A
git status --short
git commit -m "test+docs(py): stub-consistency test + CLAUDE note"
```

---

## Self-review notes (author)
- **Spec coverage:** py.typed + __init__.pyi (full surface, TypedDicts) → Task 1; missing native docstrings → Task 2; consistency test → Task 3; classifier + CLAUDE → Tasks 1/3.
- **No runtime risk:** `.pyi` is type-checker-only; the working package is untouched. The only runtime change is added `///` docstrings (`__doc__`).
- **Staleness guard:** the AST consistency check (local, Task 1 Step 5) + the pytest `test_every_public_name_is_stubbed` (CI) both enforce `__all__ ⊆ stub`; `test_stub_names_are_real` enforces the reverse for non-helper names.
- **No placeholders:** the full `.pyi` is provided; `queries.rs` docstrings are one-liners mirroring it.
- **Env honesty:** mypy/pytest/maturin absent locally → local gate is `py_compile` + the AST check + `cargo`; pytest runs in CI (`python-ci.yml`).
- **Types match the implementation:** every signature mirrors the verified native/Python-layer functions and the `debug()` dict shape from the prior sub-project.
