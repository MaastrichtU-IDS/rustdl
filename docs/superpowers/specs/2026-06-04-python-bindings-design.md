# rustdl Python Bindings — Design

**Status:** spec, awaiting plan
**Date:** 2026-06-04
**Context:** rustdl 0.1.0 just shipped to crates.io. The user wants Python bindings to make the reasoner usable from Python without a JVM, filling the gap between `pyhornedowl` (parsing only, no reasoning) and `owlready2` (Java subprocess via HermiT). This spec covers ONLY the lightweight standalone PyPI package; a separate owlready2/omny integration is queued as its own project.

## Goal

Ship `rustdl` on PyPI — a Python binding for rustdl's classifier and consistency checker that's:
- Native (no JVM, no docker, no subprocess to the rustdl CLI)
- Installable via `pip install rustdl`
- pyhornedowl-friendly (compatible workflows via file/bytes round-trip; native object pass-through deferred to 0.2)
- API surface mirrors `owl-dl-reasoner`'s public Rust API one-to-one, with Pythonic conventions

## Non-goals

- owlready2 / omny integration (separate spec).
- Native pyhornedowl `Ontology` object pass-through (deferred to 0.2; requires horned-owl version alignment).
- Stateful `Reasoner(path)` class (deferred to 0.2 if there's demand).
- Black-box and white-box explanation / justification.
- Module extraction, `owl:imports` resolution, async API.

## Architecture

One new crate in the rustdl monorepo at `crates/owl-dl-py/`. PyO3-based bindings around `owl-dl-reasoner`'s public API. Built and packaged with **maturin**. Published to PyPI as `rustdl`; imported as `import rustdl`.

```
rustdl (workspace root)
├── crates/
│   ├── owl-dl-reasoner/         (existing, unchanged)
│   └── owl-dl-py/               (NEW)
│       ├── Cargo.toml           (crate-type = ["cdylib"])
│       ├── pyproject.toml       (maturin build backend)
│       ├── src/
│       │   └── lib.rs           (#[pymodule] fn rustdl)
│       ├── python/
│       │   └── rustdl/
│       │       └── __init__.py  (pure-Python helpers, type stubs)
│       └── tests/
│           └── python/          (pytest integration tests)
└── .github/workflows/
    ├── python-ci.yml            (NEW: pytest on every PR)
    └── release-python.yml       (NEW: cibuildwheel on v*.*.* tag)
```

**Why monorepo:** coordinated versioning (`owl-dl-reasoner X.Y.Z` ↔ `rustdl X.Y.Z` on PyPI), single git tag drives both releases, no rebuild lag between Rust changes and Python wheel.

**No runtime dependency on pyhornedowl.** The wheel statically links `owl-dl-reasoner` (which includes its own `horned-owl` parser). pyhornedowl-friendly means: documented serialize-then-pass workflow.

## Python API surface

Thin PyO3 wrapper, one-to-one with `owl-dl-reasoner`'s public API.

### Top-level functions

```python
import rustdl

# File-path input (auto-detects format from extension)
result = rustdl.classify("ontology.ofn")          # OFN
result = rustdl.classify("ontology.owx")          # OWX
result = rustdl.classify("ontology.rdf")          # RDF/XML

# Bytes input (explicit format)
result = rustdl.classify_bytes(data, format="ofn")  # "ofn" | "owx" | "rdf-xml"

# Other queries (same path/bytes pattern via overload)
ok: bool = rustdl.is_consistent("ontology.ofn")
ok: bool = rustdl.is_class_satisfiable("ontology.ofn", "http://t/MyClass")
ok: bool = rustdl.is_subclass_of("ontology.ofn", "http://t/Sub", "http://t/Sup")
ok: bool = rustdl.is_instance_of("ontology.ofn", "http://t/Cls", "http://t/myInd")
iris: list[str] = rustdl.instances_of("ontology.ofn", "http://t/Cls")
realization: dict[str, list[str]] = rustdl.realize("ontology.ofn")

# Inference materialization helpers (0.1 scope addition)
sub_axioms: list[tuple[str, str]] = rustdl.materialize_inferred_subclass_axioms("ontology.ofn")
type_axioms: list[tuple[str, str]] = rustdl.materialize_inferred_class_assertions("ontology.ofn")

# Options on classify (keyword args)
rustdl.classify(
    "ontology.ofn",
    per_pair_timeout_ms: int | None = None,
    saturation_only: bool = False,
)
```

### `Classification` result type

```python
class Classification:
    classes: list[str]                # all class IRIs in the ontology
    unsatisfiable: list[str]          # class IRIs proved ⊑ ⊥
    inconsistent: bool                # whole ontology unsat (ABox check fired)

    def is_subclass(self, sub: str, sup: str) -> bool: ...
    def subclasses_of(self, cls: str) -> list[str]: ...    # all classes ⊑ cls (proper + reflexive)
    def superclasses_of(self, cls: str) -> list[str]: ...  # all classes cls ⊑
    def equivalent_classes(self, cls: str) -> list[str]: ...
```

### Errors

```python
class RustdlError(Exception): ...           # base — catches everything from the library
class ParseError(RustdlError): ...          # horned-owl parse failure
class UnsupportedAxiomError(RustdlError):   # HasKey, length-3+ chains
    kind: str                               # e.g., "HasKey"
class UnknownClassError(RustdlError): ...   # IRI not declared as a class
class TimeoutError(RustdlError): ...        # only from per_pair_timeout_ms calls
```

PyO3 panics surface as `pyo3.PanicException` (Python `BaseException`) — documented as "file a bug", not user-catchable in the normal flow.

### Conventions baked in

- **Strings for IRIs**, not a custom class — matches owlready2 / pyhornedowl.
- **Snake_case methods, exceptions instead of Result** — pythonic.
- **No async.** rustdl uses rayon internally; the Python call already runs multi-threaded. Users who want concurrency wrap with `concurrent.futures`.
- **No streaming.** SROIQ classify is O(n²); eager return is fine.
- **No `Reasoner(path)` stateful class in 0.1.** Each call parses + classifies. Common batch pattern (one ontology, many queries) addressed in 0.2 if demand surfaces.

## Packaging

**Build backend:** `maturin` 1.7+ (specified in `[build-system].requires`).

**Cargo.toml essentials:**
```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
pyo3 = { version = "0.22", features = ["abi3-py310", "extension-module"] }
owl-dl-reasoner = { path = "../owl-dl-reasoner", version = "0.1" }
horned-owl = { workspace = true }  # for format-detect / bytes-parse plumbing
```

**ABI3 mode (abi3-py310):** one wheel works for all Python ≥3.10 — massive CI simplification. Python support matrix: 3.10 / 3.11 / 3.12 / 3.13 (matches omny's floor; drop 3.9 which is EOL).

**Wheel matrix (cibuildwheel via GitHub Actions):**

| Platform | Architecture | Status |
|---|---|---|
| Linux (manylinux2014) | x86_64 | required |
| Linux (manylinux2014) | aarch64 | required |
| macOS | x86_64 (Intel) | required |
| macOS | aarch64 (Apple Silicon) | required |
| Windows | x86_64 | required |
| Source dist (sdist) | — | required (fallback for unsupported platforms) |

5 binary wheels + 1 sdist per release. cibuildwheel automates from one config block in `.github/workflows/release-python.yml`.

**Release coordination with crates.io:**
- Workspace versions stay coupled: when `owl-dl-reasoner` is `0.X.Y` on crates.io, `rustdl` PyPI matches `0.X.Y`.
- Same git tag `v0.X.Y` drives both releases. Two CI jobs in parallel: `cargo publish` (existing path) + `maturin publish` (new).

**PyPI authentication:** PyPI **trusted publisher** (OIDC token from GitHub Actions). One-time setup: register the `rustdl` PyPI project to trust `MaastrichtU-IDS/rustdl/.github/workflows/release-python.yml`. Eliminates rotating-token security burden.

## Testing

Three layers, each enforcing a different invariant:

### 1. PyO3 unit tests

Inside `crates/owl-dl-py/src/lib.rs` `#[cfg(test)]`. Exercise the bindings layer (Rust → PyAny → Rust round-trips) without spawning Python. Standard `cargo test -p owl-dl-py`.

### 2. Pytest integration tests

`crates/owl-dl-py/tests/python/`. Exercise the actual installed module:

```python
import rustdl

def test_classify_alehif():
    result = rustdl.classify("../../owl-dl-reasoner/tests/fixtures/alehif.ofn")
    assert isinstance(result.classes, list)
    assert len(result.classes) == 167
    assert result.is_subclass(some_sub, some_sup)

def test_unsupported_haskey_errors():
    with pytest.raises(rustdl.UnsupportedAxiomError) as exc:
        rustdl.classify("../../owl-dl-reasoner/tests/fixtures/has_key.ofn")
    assert exc.value.kind == "HasKey"

def test_bytes_input():
    data = open("ontology.ofn", "rb").read()
    result = rustdl.classify_bytes(data, format="ofn")
    assert ...
```

Run via `maturin develop && pytest tests/python/`. Test fixtures reused from `crates/owl-dl-reasoner/tests/fixtures/` (relative paths or symlink).

### 3. Soundness regression

A pytest test runs ONE corpus closure-diff (e.g., alehif) through the Python bindings end-to-end and confirms the result is bit-identical to the Rust-side `cargo test ... konclude_closure_diff alehif`. Pure regression guard against the bindings dropping or corrupting data.

### CI

New workflow `.github/workflows/python-ci.yml`: on every PR + push, `maturin develop` + `pytest`. Mirrors the existing `cargo test` gate. Existing Rust `ci.yml` unchanged.

New workflow `.github/workflows/release-python.yml`: on `v*.*.*` tag, cibuildwheel matrix (5 platforms) + `maturin publish` via trusted publisher.

## Errors

The bindings layer is total — no panics for legitimate inputs. Every Rust `ReasonError` variant maps to a documented Python exception (table in API section). PyO3's panic handler converts unforeseen Rust panics to `PanicException`. Document as bug-report territory.

## Performance contract

The Python bindings add the cost of one PyAny conversion per call argument and per return value. For a typical `classify` call: parse + classify dominate (~milliseconds to seconds depending on workload); binding overhead is microseconds. No measurable wall-time impact vs. calling `owl-dl-reasoner` directly from Rust.

## Roadmap (deferred from 0.1)

The OWL API exposes many more capabilities; these were considered and explicitly deferred:

| Feature | Why deferred | Estimated effort |
|---|---|---|
| Black-box `rustdl.explain(path, sub, sup)` — axiom justifications | Useful but adds API surface and search cost. Implementable in pure Python without engine changes. | ~150 LoC pure-Python, one session |
| Meta-explain `rustdl.engine_for(path, sub, sup)` — closure / wedge / tableau attribution | Cheap to add; mirrors `rustdl explain` CLI. Low broad-utility for non-debugging use. | ~30 LoC, one session |
| `rustdl.Reasoner(path)` stateful class | Parses once, reuses for batch queries. Useful only if "many queries on one ontology" pattern emerges. | ~100 LoC, one session |
| White-box explanation (engine instrumentation) | Track axiom provenance through derivations. Major engine refactor. | Multi-month |
| Native pyhornedowl `Ontology` pass-through | Requires horned-owl version pinning between rustdl + pyhornedowl. | ~50 LoC + ongoing version coupling discipline |
| owlready2 / omny integration package | Separate project — different audience, different package, different soundness contract. | Separate brainstorm/spec |
| `owl:imports` resolution | rustdl is single-ontology today. Imports are an orthogonal feature. | ~200 LoC + CI fetching |
| Module extraction wrappers (`getSyntacticLocalityModuleExtractor`) | Only syntactic locality shipped on the Rust side; ⊥-locality not built. | Blocked on Rust-side work |
| async API | rustdl uses rayon internally; async wrapping doesn't add concurrency, only ceremony. | ~50 LoC if revisited |

The roadmap is a "considered + deferred" list, not a commitment. Each item gets its own brainstorm if revisited.
