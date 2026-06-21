# Python structured result objects Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `rustdl.debug()` returns a typed, attribute-accessible `Diagnosis` object (with `Root`/`Derived`/`Inconsistency`) that is also dict-compatible (`Mapping`) for back-compat. Pure Python; no engine change.

**Architecture:** New `python/rustdl/_results.py` with frozen dataclasses implementing `collections.abc.Mapping` + a recursive `to_dict()`. `__init__.py`'s `debug()` builds & returns one. `.pyi` + stub-consistency test updated.

**Tech Stack:** Python (dataclasses, `collections.abc.Mapping`, typing).

**Spec:** `docs/superpowers/specs/2026-06-21-python-result-objects-design.md`
**Branch:** `feat/python-result-objects`

## Environment notes
- `python3` available. `_results.py` is PURE Python (no `_native` import) → its mechanics are testable locally with `python3` (import it directly). The `debug()`-returns-`Diagnosis` tests need the built extension → CI (`python-ci.yml`). No maturin locally.
- cargo prefix (only needed for the final workspace gate): `export RUSTUP_HOME=/home/dumontier/.rustup; export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"`.

## Key facts
- Current `debug()` (`__init__.py`) returns the nested dict documented in its docstring (consistent: consistent/unsatisfiable/roots/derived; inconsistent: + inconsistency).
- `diagnose(path) -> (bool, list[str], list[(str, list[str])])`; `justify(path, query) -> list[str]`; `repair(path, query, max) -> list[list[str]]` (all native, already imported in `__init__.py`).
- `__init__.pyi` currently declares `RootReport`/`DerivedReport`/`InconsistencyReport`/`DebugReport` TypedDicts + `debug(path) -> DebugReport`. These get replaced.
- `tests/python/test_stubs.py` checks `__all__` ⇔ `.pyi` declared names; `test_explain.py` has `test_debug_consistent_with_unsat` / `test_debug_coherent` using dict access (must keep passing).

## File structure
- **Create** `crates/owl-dl-py/python/rustdl/_results.py`.
- **Modify** `crates/owl-dl-py/python/rustdl/__init__.py` — import the classes, build `Diagnosis` in `debug()`, extend `__all__`.
- **Modify** `crates/owl-dl-py/python/rustdl/__init__.pyi` — replace TypedDicts with the dataclasses.
- **Create** `crates/owl-dl-py/tests/python/test_results.py` — object mechanics (locally runnable too).
- **Modify** `crates/owl-dl-py/tests/python/test_explain.py` — debug() now returns Diagnosis (attribute + back-compat dict).
- **Modify** `CLAUDE.md`, `CHANGELOG.md`.

---

### Task 1: `_results.py` (the objects)

**Files:** Create `crates/owl-dl-py/python/rustdl/_results.py`

- [ ] **Step 1: Branch**
```bash
cd /data/dumontier/rustdl
git checkout main
git checkout -b feat/python-result-objects
```

- [ ] **Step 2: Create `crates/owl-dl-py/python/rustdl/_results.py`**

```python
"""Structured, dict-compatible result objects for rustdl.debug().

Frozen dataclasses that also implement collections.abc.Mapping, so both
attribute access (d.roots[0].justification) and the legacy dict access
(d["roots"][0]["justification"], dict(d), **d, iteration) work. For JSON use
`json.dumps(d.to_dict())` — a Mapping is not a dict to the json module."""

from __future__ import annotations

import collections.abc
from dataclasses import dataclass, fields
from typing import Optional


def _plain(value: object) -> object:
    """Recursively convert result objects/tuples to plain dict/list for JSON."""
    if isinstance(value, _MappingDataclass):
        return value.to_dict()
    if isinstance(value, tuple):
        return [_plain(v) for v in value]
    return value


class _MappingDataclass(collections.abc.Mapping):
    """Mixin: expose a frozen dataclass as a read-only Mapping over a key list.

    Subclasses override `_keys()` to return the keys present (matching the legacy
    dict — e.g. Diagnosis omits "inconsistency" when consistent)."""

    def _keys(self) -> tuple[str, ...]:
        return tuple(f.name for f in fields(self))  # type: ignore[arg-type]

    def __getitem__(self, key: str) -> object:
        if key in self._keys():
            return getattr(self, key)
        raise KeyError(key)

    def __iter__(self):
        return iter(self._keys())

    def __len__(self) -> int:
        return len(self._keys())

    def to_dict(self) -> dict:
        return {k: _plain(getattr(self, k)) for k in self._keys()}


@dataclass(frozen=True)
class Root(_MappingDataclass):
    iri: str
    justification: tuple[str, ...]
    repairs: tuple[tuple[str, ...], ...]
    derives: tuple[str, ...]


@dataclass(frozen=True)
class Derived(_MappingDataclass):
    iri: str
    roots: tuple[str, ...]


@dataclass(frozen=True)
class Inconsistency(_MappingDataclass):
    justification: tuple[str, ...]
    repairs: tuple[tuple[str, ...], ...]


@dataclass(frozen=True)
class Diagnosis(_MappingDataclass):
    consistent: bool
    unsatisfiable: tuple[str, ...]
    roots: tuple[Root, ...]
    derived: tuple[Derived, ...]
    inconsistency: Optional[Inconsistency] = None

    def _keys(self) -> tuple[str, ...]:
        base = ("consistent", "unsatisfiable", "roots", "derived")
        return base if self.consistent else base + ("inconsistency",)
```

- [ ] **Step 3: Validate locally (pure Python — no native needed)**
```bash
cd /data/dumontier/rustdl/crates/owl-dl-py/python/rustdl
python3 -m py_compile _results.py && echo "COMPILE OK"
python3 - <<'PY'
import json
from _results import Diagnosis, Root, Derived, Inconsistency
# consistent with one root + one derived
d = Diagnosis(
    consistent=True,
    unsatisfiable=("urn:Bad", "urn:SubBad"),
    roots=(Root(iri="urn:Bad", justification=("Bad ⊑ ⊥",), repairs=(("ax1",),), derives=("urn:SubBad",)),),
    derived=(Derived(iri="urn:SubBad", roots=("urn:Bad",)),),
    inconsistency=None,
)
# attribute access
assert d.consistent is True
assert d.roots[0].justification == ("Bad ⊑ ⊥",)
assert d.roots[0].derives == ("urn:SubBad",)
# dict-style access (back-compat) at every level
assert d["consistent"] is True
assert d["roots"][0]["justification"] == ("Bad ⊑ ⊥",)
assert "unsatisfiable" in d
assert "inconsistency" not in d            # absent when consistent
try:
    _ = d["inconsistency"]; assert False
except KeyError:
    pass
assert dict(d).keys() == {"consistent", "unsatisfiable", "roots", "derived"} or set(dict(d)) == {"consistent","unsatisfiable","roots","derived"}
# json via to_dict
js = json.dumps(d.to_dict())
back = json.loads(js)
assert back["roots"][0]["justification"] == ["Bad ⊑ ⊥"]   # tuples -> lists
assert back["roots"][0]["derives"] == ["urn:SubBad"]
# inconsistent variant
di = Diagnosis(consistent=False, unsatisfiable=(), roots=(), derived=(),
               inconsistency=Inconsistency(justification=("a",), repairs=(("b",),)))
assert "inconsistency" in di
assert di["inconsistency"]["justification"] == ("a",)
assert di.inconsistency.repairs == (("b",),)
assert json.dumps(di.to_dict())  # serializable
# frozen
try:
    object.__setattr__  # sanity import
    d2 = Root(iri="x", justification=(), repairs=(), derives=())
    try:
        d2.iri = "y"; assert False
    except Exception:
        pass
except Exception:
    pass
print("RESULTS OBJECTS OK")
PY
```
Expected: `COMPILE OK` and `RESULTS OBJECTS OK`. Fix any failure (the Mapping/`_keys` logic) and re-run. (If `dict(d).keys()` assertion form is awkward, simplify to `set(dict(d)) == {...}`.)

- [ ] **Step 4: Commit**
```bash
cd /data/dumontier/rustdl
git add crates/owl-dl-py/python/rustdl/_results.py
git commit -m "feat(py): dict-compatible Diagnosis/Root/Derived/Inconsistency result objects"
```

---

### Task 2: Wire `debug()` + exports + `.pyi`

**Files:** Modify `crates/owl-dl-py/python/rustdl/__init__.py`, `__init__.pyi`, `tests/python/test_stubs.py`

- [ ] **Step 1: Import the classes + rewrite `debug()`** in `__init__.py`. Add near the top (after the `_native` import block):
```python
from ._results import (
    Diagnosis as Diagnosis,
    Root as Root,
    Derived as Derived,
    Inconsistency as Inconsistency,
)
```
Replace the body of `debug()` (keep the docstring, updated) to build and return a `Diagnosis`:
```python
def debug(path):
    """One-call ontology diagnosis → a Diagnosis result object.

    Supports attribute access (d.consistent, d.roots[0].justification,
    d.inconsistency.repairs) AND legacy dict access
    (d["roots"][0]["justification"], dict(d), iteration). For JSON use
    json.dumps(d.to_dict()).

    Consistent ontology → Diagnosis(consistent=True, unsatisfiable=..., roots=[Root...],
    derived=[Derived...], inconsistency=None). Inconsistent → consistent=False, empties,
    inconsistency=Inconsistency(...). Read-only; sound by construction."""
    consistent, roots, derived = diagnose(path)
    if not consistent:
        return Diagnosis(
            consistent=False,
            unsatisfiable=(),
            roots=(),
            derived=(),
            inconsistency=Inconsistency(
                justification=tuple(justify(path, ["inconsistent"])),
                repairs=tuple(tuple(r) for r in repair(path, ["inconsistent"], 10)),
            ),
        )
    root_objs = tuple(
        Root(
            iri=r,
            justification=tuple(justify(path, ["unsat", r])),
            repairs=tuple(tuple(x) for x in repair(path, ["unsat", r], 10)),
            derives=tuple(d for (d, rs) in derived if r in rs),
        )
        for r in roots
    )
    return Diagnosis(
        consistent=True,
        unsatisfiable=tuple(list(roots) + [d for (d, _) in derived]),
        roots=root_objs,
        derived=tuple(Derived(iri=d, roots=tuple(rs)) for (d, rs) in derived),
        inconsistency=None,
    )
```

- [ ] **Step 2: Extend `__all__`** in `__init__.py` — add `"Diagnosis"`, `"Root"`, `"Derived"`, `"Inconsistency"`.

- [ ] **Step 3: Update `__init__.pyi`** — REMOVE the `RootReport`/`DerivedReport`/`InconsistencyReport`/`DebugReport` TypedDicts and the `from typing import TypedDict` if now unused. ADD the dataclasses (mirroring `_results.py`, with `to_dict`), and change `debug`'s return:
```python
from collections.abc import Mapping
from typing import Optional

class Root(Mapping[str, object]):
    iri: str
    justification: tuple[str, ...]
    repairs: tuple[tuple[str, ...], ...]
    derives: tuple[str, ...]
    def to_dict(self) -> dict: ...

class Derived(Mapping[str, object]):
    iri: str
    roots: tuple[str, ...]
    def to_dict(self) -> dict: ...

class Inconsistency(Mapping[str, object]):
    justification: tuple[str, ...]
    repairs: tuple[tuple[str, ...], ...]
    def to_dict(self) -> dict: ...

class Diagnosis(Mapping[str, object]):
    consistent: bool
    unsatisfiable: tuple[str, ...]
    roots: tuple[Root, ...]
    derived: tuple[Derived, ...]
    inconsistency: Optional[Inconsistency]
    def to_dict(self) -> dict: ...

def debug(path: str) -> Diagnosis: ...
```
(Place the class defs near the other result types; keep the rest of the stub unchanged.)

- [ ] **Step 4: Update the stub-consistency check + run it** — the AST check in `test_stubs.py` already compares `__all__` ⇔ stub-declared names, so the new classes are covered once they're in both `__all__` and the `.pyi`. Validate locally (AST-only, no native):
```bash
cd /data/dumontier/rustdl
python3 -m py_compile crates/owl-dl-py/python/rustdl/__init__.pyi && echo "PYI OK"
python3 - <<'PY'
import ast, pathlib
base = pathlib.Path("crates/owl-dl-py/python/rustdl")
src = ast.parse((base / "__init__.py").read_text())
all_names = next([e.value for e in n.value.elts] for n in ast.walk(src)
                 if isinstance(n, ast.Assign) for t in n.targets
                 if isinstance(t, ast.Name) and t.id == "__all__")
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
assert not missing, f"missing from .pyi: {sorted(missing)}"
print(f"STUB CONSISTENT: {len(all_names)} names (incl. Diagnosis/Root/Derived/Inconsistency)")
PY
```
Expected `PYI OK` + `STUB CONSISTENT: 32 names`. Also `python3 -m py_compile crates/owl-dl-py/python/rustdl/__init__.py`.

- [ ] **Step 5: Commit**
```bash
git add crates/owl-dl-py/python/rustdl/__init__.py crates/owl-dl-py/python/rustdl/__init__.pyi
git commit -m "feat(py): debug() returns a Diagnosis object; export result classes; update stub"
```

---

### Task 3: Tests + docs + final gate

**Files:** Create `tests/python/test_results.py`; Modify `tests/python/test_explain.py`, `CLAUDE.md`, `CHANGELOG.md`

- [ ] **Step 1: Create `crates/owl-dl-py/tests/python/test_results.py`** (object mechanics; mostly runnable locally too):
```python
"""Diagnosis/Root/etc. result-object mechanics (no native needed)."""
import json

from rustdl import Diagnosis, Root, Derived, Inconsistency


def _sample():
    return Diagnosis(
        consistent=True,
        unsatisfiable=("urn:Bad",),
        roots=(Root(iri="urn:Bad", justification=("Bad ⊑ ⊥",), repairs=(("ax",),), derives=("urn:Sub",)),),
        derived=(Derived(iri="urn:Sub", roots=("urn:Bad",)),),
        inconsistency=None,
    )


def test_attribute_access():
    d = _sample()
    assert d.consistent is True
    assert d.roots[0].iri == "urn:Bad"
    assert d.roots[0].justification == ("Bad ⊑ ⊥",)
    assert d.derived[0].roots == ("urn:Bad",)


def test_dict_compat():
    d = _sample()
    assert d["consistent"] is True
    assert d["roots"][0]["justification"] == ("Bad ⊑ ⊥",)
    assert "unsatisfiable" in d
    assert "inconsistency" not in d
    import pytest
    with pytest.raises(KeyError):
        _ = d["inconsistency"]
    assert set(dict(d)) == {"consistent", "unsatisfiable", "roots", "derived"}


def test_to_dict_json():
    d = _sample()
    js = json.loads(json.dumps(d.to_dict()))
    assert js["roots"][0]["justification"] == ["Bad ⊑ ⊥"]   # tuples → lists
    assert "inconsistency" not in js


def test_inconsistent_shape():
    di = Diagnosis(consistent=False, unsatisfiable=(), roots=(), derived=(),
                   inconsistency=Inconsistency(justification=("a",), repairs=(("b",),)))
    assert "inconsistency" in di
    assert di.inconsistency.justification == ("a",)
    assert di["inconsistency"]["repairs"] == (("b",),)
    assert json.dumps(di.to_dict())


def test_frozen():
    import pytest
    d = _sample()
    with pytest.raises(Exception):
        d.roots[0].iri = "x"  # frozen
```

- [ ] **Step 2: Update `test_explain.py`'s debug tests** to assert the object API while keeping back-compat. Replace `test_debug_consistent_with_unsat` and `test_debug_coherent` bodies:
```python
def test_debug_consistent_with_unsat(tmp_path):
    p = _write(tmp_path, BROKEN)
    d = rustdl.debug(p)
    assert isinstance(d, rustdl.Diagnosis)
    # attribute API
    assert d.consistent is True
    assert "urn:Bad" in d.unsatisfiable
    bad = next(r for r in d.roots if r.iri == "urn:Bad")
    assert bad.justification and bad.repairs
    assert "urn:SubBad" in bad.derives
    # back-compat dict API
    assert d["consistent"] is True
    assert any(r["iri"] == "urn:Bad" for r in d["roots"])


def test_debug_coherent(tmp_path):
    p = _write(tmp_path, "Prefix(:=<urn:>)\nOntology(Declaration(Class(:A)))\n")
    d = rustdl.debug(p)
    assert d.consistent is True
    assert d.unsatisfiable == ()
    assert d["unsatisfiable"] == ()
```

- [ ] **Step 3: Syntax-check** (pytest runs in CI):
```bash
cd /data/dumontier/rustdl
python3 -m py_compile crates/owl-dl-py/tests/python/test_results.py crates/owl-dl-py/tests/python/test_explain.py && echo "PY COMPILE OK"
```

- [ ] **Step 4: Docs.** CHANGELOG.md — under `[0.3.10]` (or a new `[Unreleased]` section if you prefer), add an Added note: "`rustdl.debug()` now returns a typed `Diagnosis` result object (attribute access + dict-compatible; `to_dict()` for JSON)." CLAUDE.md — append to the Python docs: "`rustdl.debug()` returns a `Diagnosis` dataclass (with `Root`/`Derived`/`Inconsistency`) — attribute access + `Mapping` dict-compat + `to_dict()`; see `docs/superpowers/specs/2026-06-21-python-result-objects-design.md`."

- [ ] **Step 5: Final gate**
```bash
cd /data/dumontier/rustdl
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
python3 -m py_compile crates/owl-dl-py/python/rustdl/_results.py crates/owl-dl-py/python/rustdl/__init__.py crates/owl-dl-py/python/rustdl/__init__.pyi crates/owl-dl-py/tests/python/test_results.py crates/owl-dl-py/tests/python/test_explain.py
# re-run the AST consistency check from Task 2 Step 4
```
All green (no Rust changed this sub-project, so cargo should be untouched/green). Report the `cargo test --workspace` aggregate + py_compile + STUB CONSISTENT. After any `cargo fmt --all`, stage touched files (there should be none).

- [ ] **Step 6: Commit**
```bash
git add -A
git status --short
git commit -m "test+docs(py): Diagnosis object tests + CHANGELOG/CLAUDE notes"
```

---

## Self-review notes (author)
- **Spec coverage:** objects + Mapping + to_dict → Task 1; debug() returns Diagnosis + exports + .pyi → Task 2; back-compat (dict access still works) → Task 1 local check + Task 3 `test_dict_compat`/updated `test_explain`; JSON via to_dict → tests; stub consistency → Task 2 Step 4.
- **Compat:** Mapping protocol preserves `d["k"]`/`dict(d)`/`in`/iteration; the one change (`json.dumps(d)` → `json.dumps(d.to_dict())`) is documented (CHANGELOG/CLAUDE/docstring). The updated `test_explain` debug tests assert BOTH attribute and dict access.
- **Local-testable:** `_results.py` is native-free → Task 1 validates the object mechanics with `python3` directly; the debug()-integration tests run in CI.
- **No engine/Rust change:** cargo gate is a non-regression check only.
- **Type consistency:** `.pyi` dataclasses mirror `_results.py` fields exactly; `debug -> Diagnosis`; new names in `__all__` ⇔ `.pyi` (32 total).
