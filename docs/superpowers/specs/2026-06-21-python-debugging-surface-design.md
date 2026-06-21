# Python debugging surface — make rustdl adoptable, sub-project 1 (design)

**Date:** 2026-06-21
**Status:** approved (brainstorming) → ready for implementation plan
**Branch:** `feat/python-debugging-surface`

First sub-project of "make rustdl adoptable": expose the shipped explanation/debugging
work in the Python API and fix a correctness gap in the existing surface. Goal is
**reach/usability**, not new engine capability.

## Why (two gaps found)

1. **Correctness bug:** the new `materialize_*` natives are registered in `_native`
   but NOT re-exported in `python/rustdl/__init__.py` (`from rustdl._native import …`
   lists only the two original ones). So `rustdl.materialize_inferred_property_assertions(...)`
   — the colleague's function, and the README examples — raises `AttributeError`.
2. **Reach gap:** the explanation/debugging suite (`justify`, `diagnose`, `repair`,
   `prove`) has **no Python binding at all** — CLI-only. The differentiator is invisible
   to Python users.

## Scope

1. **Fix re-exports:** add to `__init__.py` import + `__all__`:
   `materialize_inferred_property_assertions`, `materialize_inferred_data_property_assertions`,
   `materialize_inferred_subobjectproperty_axioms`, `materialize_inferred_subdataproperty_axioms`,
   `materialize_existential_successors`.
2. **Shared query parser:** lift `parse_justify_query` (+ `parse_literal_arg`) from
   `owl-dl-cli` into `owl_dl_reasoner::justify` as `pub fn parse_query(&[String]) ->
   Result<Entailment, ReasonError>`; switch the CLI to it (no behaviour change). Both
   CLI and the new Python bindings use it — so Python supports every query form for free.
3. **New native bindings** (`owl-dl-py/src/explain.rs`, registered in `_native`),
   string/tuple forms:
   - `justify(path, query: list[str]) -> list[str]` — Manchester axioms of one minimal
     justification.
   - `justify_all(path, query: list[str], max=10) -> list[list[str]]`.
   - `diagnose(path) -> (bool, list[str], list[(str, list[str])])` =
     `(consistent, roots, derived_with_roots)`.
   - `repair(path, query: list[str], max=10) -> list[list[str]]` — each repair a list
     of Manchester axioms to remove.
   Axioms render as Manchester with a **default prefix map (full IRIs)** —
   dependency-free, unambiguous.
4. **`rustdl.debug(path) -> dict`** — pure-Python convenience in `__init__.py`
   composing the natives (no new Rust). The headline one-call UX.
5. **Update `__all__`** for the whole surface.

## Result representation (strings now; objects later)

Per the brainstorm: ship string/tuple forms now; **structured dataclasses
(`Justification`, `Diagnosis`, …) are a deferred follow-up sub-project.** The native
bindings return `str`/tuple/list shapes mirroring the `materialize_*` family.

### `rustdl.debug(path) -> dict`

```python
{
  "consistent": bool,
  "unsatisfiable": [iri, ...],
  "roots":   [{"iri": str, "justification": [axiom, ...],
               "repairs": [[axiom, ...], ...], "derives": [iri, ...]}, ...],
  "derived": [{"iri": str, "roots": [iri, ...]}, ...],
}
```
On an inconsistent ontology: `consistent=False`, `unsatisfiable=[]`, `roots`/`derived`
empty, plus
`"inconsistency": {"justification": [axiom, ...], "repairs": [[axiom, ...], ...]}`.
A plain JSON-serializable dict. Implemented by calling `diagnose`, then per root
`justify(["unsat", iri])` + `repair(["unsat", iri])` (and for the inconsistent case
`justify(["inconsistent"])` + `repair(["inconsistent"])`).

## Architecture

- **Native (Rust/PyO3):** new `owl-dl-py/src/explain.rs` — thin wrappers over the
  shipped `justify::{find_one_justification, find_all_justifications, parse_query}`,
  `diagnose`, `find_repairs`. A small `render(component) -> String` using
  `horned_owl::io::omn::AsManchester` + `horned_owl::curie::PrefixMapping::default()`.
  Errors via `reason_error_to_py`. Registered in `_native` (`lib.rs`).
- **Reasoner:** `justify::parse_query` (moved from the CLI).
- **Python layer (`__init__.py`):** the re-export fixes, the `debug()` convenience, and
  `__all__`.

## Soundness

Presentation/reach only — every binding calls a shipped, sound reasoner function
(FP=0 contract intact). No engine change. `debug()` composes them. (The parser move is
a pure refactor; the CLI's behaviour is preserved and smoke-tested.)

## Testing (pytest, `crates/owl-dl-py/tests/python/`)

- **Bug regression:** `rustdl.materialize_inferred_property_assertions` and the other
  four are callable as `rustdl.X` (this is the test that would have caught the missing
  re-export); each returns the expected tuples on a small ontology.
- `justify(path, ["subclass", S, T])` / `["unsat", C]` → expected Manchester axioms;
  `repair(path, ["unsat", C])` → expected removal set(s); `diagnose(path)` → expected
  `(consistent, roots, derived)`.
- `debug(path)` on (a) broken-but-consistent → `roots` populated with justification +
  repairs; (b) inconsistent → `consistent=False` + `inconsistency` section; (c)
  coherent → `unsatisfiable == []`.
- `set(rustdl.__all__)` ⊇ the new names; every `__all__` name is importable.
- **Rust gate:** `cargo build/clippy/test` for `explain.rs` + the moved parser; CLI
  `justify`/`repair` smoke unchanged (parser-move regression).

## Out of scope (→ later sub-projects)

- Structured dataclasses (`Justification` / `Diagnosis` objects) — the "objects later"
  half.
- `.pyi` type stubs; RDF / pandas outputs; a Python `report`-HTML binding (the CLI
  already emits HTML); `prove` (proof trees) in Python.
