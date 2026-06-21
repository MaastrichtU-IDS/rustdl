# Python type stubs + docstrings — make rustdl adoptable, sub-project 2 (design)

**Date:** 2026-06-21
**Status:** approved (brainstorming) → ready for implementation plan
**Branch:** `feat/python-type-stubs`

Second "make adoptable" sub-project. Make the whole Python surface **typed and
discoverable** — IDE autocomplete, type-checker support, and complete `help()`.
Reach/usability only; no engine change.

## Current state (explored)

- **No `.pyi`, no `py.typed`** → the package is untyped to IDEs/type-checkers.
- Docstrings partial: `materialize_*` + `explain` (justify/diagnose/repair) natives
  have `///` (→ `__doc__`); the `queries.rs` natives (`is_consistent`,
  `is_class_satisfiable`, `is_subclass_of`, `is_instance_of`, `instances_of`,
  `realize`) have **none**. Python-layer `classify`/`classify_bytes`/`debug` are
  documented.
- `examples.py` is already inline-annotated (`-> str`, NS constants) → no stub needed.
- maturin ships the `python-source` tree, so files added under `python/rustdl/` are
  packaged automatically.

## Scope

1. **`python/rustdl/py.typed`** — empty PEP 561 marker.
2. **`python/rustdl/__init__.pyi`** — the single source of truth for `import rustdl`
   types (a `.pyi` shadows the `.py` for type-checkers). Covers EVERY `__all__` name.
3. **Fill missing native `///` docs** in `queries.rs` so runtime `help()` is complete.
4. **Stub-consistency test** (pytest, CI) — `__all__` ⇔ `.pyi` declarations match.

## The stub (`__init__.pyi`)

Precise signatures with docstrings:

- **Functions** (exact return types):
  - `classify(path: str, *, per_pair_timeout_ms: int = 1000, saturation_only: bool = False) -> Classification`
  - `classify_bytes(data: bytes, *, format: str, per_pair_timeout_ms: int = 1000, saturation_only: bool = False) -> Classification`
  - `is_consistent(path: str) -> bool`; `is_class_satisfiable(path: str, class_iri: str) -> bool`;
    `is_subclass_of(path: str, sub: str, sup: str) -> bool`;
    `is_instance_of(path: str, class_iri: str, individual_iri: str) -> bool`;
    `instances_of(path: str, class_iri: str) -> list[str]`; `realize(path: str) -> dict[str, list[str]]`
  - `materialize_inferred_subclass_axioms(path: str) -> list[tuple[str, str]]`;
    `…class_assertions(path) -> list[tuple[str, str]]`;
    `materialize_inferred_property_assertions(path) -> list[tuple[str, str, str]]`;
    `…data_property_assertions(path) -> list[tuple[str, str, str, str, str]]`;
    `materialize_inferred_subobjectproperty_axioms(path) -> list[tuple[str, str]]`;
    `…subdataproperty_axioms(path) -> list[tuple[str, str]]`;
    `materialize_existential_successors(path) -> list[tuple[str, str, str, str]]`
  - `justify(path: str, query: list[str]) -> list[str]`;
    `justify_all(path: str, query: list[str], max: int = 10) -> list[list[str]]`;
    `diagnose(path: str) -> tuple[bool, list[str], list[tuple[str, list[str]]]]`;
    `repair(path: str, query: list[str], max: int = 10) -> list[list[str]]`;
    `debug(path: str) -> DebugReport`
- **`debug()` TypedDicts** (3.10-safe — `total=False` for the optional `inconsistency`):
  ```python
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
  ```
- **`Classification`** (`#[pyclass]` members verified in `classify.rs`):
  ```python
  class Classification:
      @property
      def classes(self) -> list[str]: ...
      @property
      def unsatisfiable(self) -> list[str]: ...
      @property
      def inconsistent(self) -> bool: ...
      @property
      def timed_out_pairs(self) -> int: ...
      @property
      def complete(self) -> bool: ...
      def is_subclass(self, sub: str, sup: str) -> bool: ...
      def equivalent_classes(self, cls: str) -> list[str]: ...
      def direct_subsumers(self, cls: str) -> list[str]: ...
      def subclasses_of(self, cls: str) -> list[str]: ...   # pure-Python (bound in __init__)
      def superclasses_of(self, cls: str) -> list[str]: ...  # pure-Python
      def __repr__(self) -> str: ...
  ```
- **Exceptions** & misc: `class RustdlError(Exception): ...`, `class ParseError(RustdlError): ...`,
  `class UnsupportedAxiomError(RustdlError): ...`, `class UnknownClassError(RustdlError): ...`,
  `class IncompleteClassificationWarning(UserWarning): ...`, `__version__: str`,
  `from . import examples as examples`.

## Docstrings

The `.pyi` carries docstrings (IDEs/type-checkers surface stub docstrings — concise
one-liners mirroring the runtime docs). Plus add the missing `///` to the `queries.rs`
natives so runtime `help(rustdl.is_consistent)` works (the already-documented natives
and the Python-layer `classify`/`debug` are fine).

## Validation

- `python3 -m py_compile python/rustdl/__init__.pyi` (the `.pyi` is valid Python
  syntax; `TypedDict`/`@property` parse fine).
- **Stub-consistency test** (pytest in CI + a local `python3` AST check): the set of
  top-level `def`/`class` names + `__version__` in `__init__.pyi` matches
  `rustdl.__all__` exactly (catches a name added to `__all__` but missing from the
  stub, and vice-versa). This is the staleness guard for the hand-written stub.
- If `mypy`/`pyright` is available, a smoke type-check of a tiny usage snippet;
  otherwise the AST check + CI is the gate (no type-checker in the current env).
- Rust gate (`cargo build/clippy`) for the added `queries.rs` docstrings.

## Out of scope (→ later sub-projects)

Auto-generated stubs; a full mypy/pyright CI gate; structured runtime objects (the
"dataclasses" sub-project); RDF/pandas outputs.
