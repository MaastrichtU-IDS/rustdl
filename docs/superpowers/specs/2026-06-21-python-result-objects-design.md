# Python structured result objects — make rustdl adoptable, sub-project 3 (design)

**Date:** 2026-06-21
**Status:** approved (brainstorming) → ready for implementation plan
**Branch:** `feat/python-result-objects`

Third "make adoptable" sub-project — the "objects later" half deferred from
sub-project 1. Give `debug()` a typed, attribute-accessible result object while
keeping the existing dict-style access working. Pure Python; no engine change.

## Scope (focused)

- `debug()` returns a structured **`Diagnosis`** object (with `Root` / `Derived` /
  `Inconsistency` sub-objects). The lightweight primitives `justify` (`list[str]`),
  `diagnose` (tuple), `repair` (`list[list[str]]`) stay flat — they gain nothing from
  wrapping.
- New pure-Python module `python/rustdl/_results.py`. No Rust change (`debug()` still
  composes the same native `diagnose`/`justify`/`repair`).

## The objects (frozen dataclasses)

```python
@dataclass(frozen=True)
class Root(Mapping):
    iri: str
    justification: tuple[str, ...]
    repairs: tuple[tuple[str, ...], ...]
    derives: tuple[str, ...]

@dataclass(frozen=True)
class Derived(Mapping):
    iri: str
    roots: tuple[str, ...]

@dataclass(frozen=True)
class Inconsistency(Mapping):
    justification: tuple[str, ...]
    repairs: tuple[tuple[str, ...], ...]

@dataclass(frozen=True)
class Diagnosis(Mapping):
    consistent: bool
    unsatisfiable: tuple[str, ...]
    roots: tuple[Root, ...]
    derived: tuple[Derived, ...]
    inconsistency: Inconsistency | None
```
Attribute access: `d.consistent`, `d.roots[0].justification`, `d.inconsistency.repairs`.

## Dict compatibility (the compat choice)

Each class implements the **`collections.abc.Mapping`** protocol so the dict-style
access shipped in sub-project 1 keeps working at every level:
`d["roots"][0]["justification"]`, `"roots" in d`, `dict(d)`, `**d`, iteration,
`len(d)`, `.get(k, default)`.

- A shared mixin provides `__getitem__`/`__iter__`/`__len__` over a per-class
  **key list** that matches the old dict exactly. Crucially, `Diagnosis` exposes the
  `"inconsistency"` key **only when inconsistent** (when consistent, `inconsistency`
  is `None` and is NOT in `keys()` — matching the old dict, which omitted the key in
  the consistent case). `__getitem__("inconsistency")` raises `KeyError` when
  consistent.
- `Mapping` provides `keys`/`values`/`items`/`get`/`__contains__`/`__eq__` for free
  once `__getitem__`/`__iter__`/`__len__` are defined.

### The one documented behavior change
`json.dumps(d)` no longer works directly (a `Mapping` is not a `dict` to `json`).
Provide a recursive `d.to_dict()` → `json.dumps(d.to_dict())`. `to_dict()` returns
plain nested `dict`/`list`/`str` (the exact shape the old `debug()` returned). This is
the lone break, documented in the docstring + CHANGELOG. (The AttrDict alternative was
considered and rejected: it preserves `json.dumps(d)` but defeats the typed-attribute
goal of this lane.)

## Typing & exports

- `__init__.pyi`: remove the `debug()` TypedDicts (`DebugReport`/`RootReport`/…),
  declare the four dataclasses (with their fields + `to_dict()`), and
  `def debug(path: str) -> Diagnosis: ...`.
- Export `Diagnosis` / `Root` / `Derived` / `Inconsistency` from `__init__.py`
  (`from ._results import …`) + add to `__all__` (for `isinstance` / annotations).
- Update the stub-consistency test's `__all__` ⇔ `.pyi` expectation for the new names.

## `debug()` change

`debug()` (in `__init__.py`) keeps composing `diagnose`/`justify`/`repair`, but builds
and returns a `Diagnosis` (consistent case → `roots`/`derived` populated,
`inconsistency=None`; inconsistent case → empties + `inconsistency=Inconsistency(...)`).
Its docstring documents attribute + dict access and the `to_dict()` JSON note.

## Soundness

Pure presentation — same sound native calls, wrapped. No engine change; read-only.

## Testing (`tests/python/`, runs in CI)

- `debug()` returns a `Diagnosis`; attribute access works on consistent + inconsistent
  fixtures (`d.consistent`, `d.roots[0].justification`, `d.roots[0].derives`,
  `d.inconsistency.repairs`).
- **Back-compat:** the dict-style assertions still pass (`d["consistent"]`,
  `d["roots"][0]["justification"]`, `"unsatisfiable" in d`, `dict(d)`); `Diagnosis`
  with no unsat → `d.roots == ()`; consistent `d` has no `"inconsistency"` key
  (`KeyError`), inconsistent `d` does.
- `to_dict()` returns the exact legacy nested-dict shape and is `json.dumps`-able.
- `frozen` (immutability) + equality (`Mapping.__eq__`) sanity.
- Stub consistency (`Diagnosis`/`Root`/`Derived`/`Inconsistency` in `__all__` ⇔ `.pyi`);
  `python3 -m py_compile` of `_results.py` + `.pyi`.

## Out of scope (→ later)

Wrapping the flat primitives (`justify`/`repair`/`diagnose`) in objects; RDF/pandas
outputs (next lane); any engine change.
